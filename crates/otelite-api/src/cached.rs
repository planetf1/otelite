//! Response-caching middleware for read-only API endpoints.
//!
//! Analytics endpoints on a large database take seconds even with a warmed
//! storage layer, and the web UI refreshes every 20-30 seconds. This layer
//! gives read-only endpoints a short-TTL response cache plus in-flight
//! deduplication: concurrent identical requests (e.g. two dashboard widgets
//! hitting the same endpoint at once) collapse into a single query.
//!
//! Only GET requests to a small whitelist of read-only endpoints are cached;
//! everything else passes through untouched. Error responses are never
//! cached. The cache is cleared on data-mutating operations (purge).

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    body::{to_bytes, Body},
    extract::Request,
    http::{Method, StatusCode},
    middleware::Next,
    response::Response,
};

use crate::cache::LruCache;

/// Maximum response body size (bytes) that will be cached. Analytics
/// responses are small JSON; anything bigger is passed through uncached.
const MAX_CACHE_BODY_BYTES: usize = 8 * 1024 * 1024;

/// How long a dedup waiter will block on an in-flight request before
/// giving up and running its own copy (guards against a cancelled owner).
const DEDUP_WAIT_TIMEOUT: Duration = Duration::from_secs(120);

/// A (status, content-type, body) triple recovered from an in-flight or
/// cached response.
type CachedBody = (u16, Option<String>, Vec<u8>);

/// Bookkeeping for one in-flight request that duplicates may await.
struct InFlight {
    done: Arc<tokio::sync::Notify>,
    response: Mutex<Option<CachedBody>>,
}

/// RAII cleanup for an in-flight dedup entry.
///
/// The entry is removed when the owner future is dropped — including
/// on client disconnect (cancellation) and panics — not just at the
/// end of the happy path. Without this, a cancelled owner leaves a
/// stale entry behind and every subsequent duplicate waits out the
/// full [`DEDUP_WAIT_TIMEOUT`] before running its own copy.
///
/// Removal is conditional on the map still holding this exact
/// [`Arc`]: a guard that outlives a re-registration (the stale entry
/// was already evicted and a fresh owner inserted) must not delete
/// the newcomer's entry.
///
/// The shared state is held by [`Arc`] rather than a reference so the
/// guard stays `Send`/`'static`-compatible inside the middleware
/// future (axum requires the handler future to be `Send`).
struct InFlightGuard {
    cache: Arc<CacheState>,
    key: String,
    entry: Arc<InFlight>,
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        let mut map = self
            .cache
            .in_flight
            .lock()
            .expect("in_flight mutex poisoned");
        match map.get(&self.key) {
            Some(current) if Arc::ptr_eq(current, &self.entry) => {
                map.remove(&self.key);
            },
            _ => {},
        }
        // Wake waiters still parked on this entry so they notice it is
        // gone and fall through to their own request.
        self.entry.done.notify_waiters();
    }
}

/// Which response cache a whitelisted path belongs to. Each bucket has its
/// own size/TTL: GenAI analytics are the expensive ones (20s), the stats
/// totals tolerate 60s staleness (a recompute is a full-table index scan),
/// the sessions list is cheap enough for a 10s TTL, and the metrics list /
/// metric names / resource-key typeahead are near-static (a few hundred rows
/// that only grow) so they get longer TTLs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Bucket {
    Genai,
    Stats,
    Sessions,
    Metrics,
    Names,
    Keys,
}

impl Bucket {
    fn ttl(&self) -> Duration {
        match self {
            Bucket::Genai => Duration::from_secs(20),
            Bucket::Stats => Duration::from_secs(60),
            Bucket::Sessions => Duration::from_secs(10),
            Bucket::Metrics => Duration::from_secs(60),
            Bucket::Names => Duration::from_secs(600),
            Bucket::Keys => Duration::from_secs(300),
        }
    }
}

/// Shared state for the caching middleware.
pub struct CacheState {
    genai: LruCache<String, String>,
    stats: LruCache<String, String>,
    sessions: LruCache<String, String>,
    metrics: LruCache<String, String>,
    names: LruCache<String, String>,
    keys: LruCache<String, String>,
    in_flight: Mutex<HashMap<String, Arc<InFlight>>>,
}

impl CacheState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            genai: LruCache::new(500, Bucket::Genai.ttl()),
            stats: LruCache::new(50, Bucket::Stats.ttl()),
            sessions: LruCache::new(200, Bucket::Sessions.ttl()),
            metrics: LruCache::new(200, Bucket::Metrics.ttl()),
            names: LruCache::new(50, Bucket::Names.ttl()),
            keys: LruCache::new(30, Bucket::Keys.ttl()),
            in_flight: Mutex::new(HashMap::new()),
        })
    }

    /// Cache bucket for a request path, or None to pass through uncached.
    fn policy(path: &str) -> Option<Bucket> {
        if path == "/api/stats" {
            Some(Bucket::Stats)
        } else if path == "/api/sessions" {
            Some(Bucket::Sessions)
        } else if path == "/api/metrics/names" {
            Some(Bucket::Names)
        } else if path == "/api/metrics" || path.starts_with("/api/metrics/") {
            Some(Bucket::Metrics)
        } else if path == "/api/resource-keys" {
            Some(Bucket::Keys)
        } else if path.starts_with("/api/genai/") {
            Some(Bucket::Genai)
        } else {
            None
        }
    }

    fn get(&self, key: &str, bucket: Bucket) -> Option<String> {
        let k = key.to_string();
        match bucket {
            Bucket::Genai => self.genai.get(&k),
            Bucket::Stats => self.stats.get(&k),
            Bucket::Sessions => self.sessions.get(&k),
            Bucket::Metrics => self.metrics.get(&k),
            Bucket::Names => self.names.get(&k),
            Bucket::Keys => self.keys.get(&k),
        }
    }

    fn insert(&self, key: &str, value: String, bucket: Bucket) {
        match bucket {
            Bucket::Genai => self.genai.insert(key.to_string(), value),
            Bucket::Stats => self.stats.insert(key.to_string(), value),
            Bucket::Sessions => self.sessions.insert(key.to_string(), value),
            Bucket::Metrics => self.metrics.insert(key.to_string(), value),
            Bucket::Names => self.names.insert(key.to_string(), value),
            Bucket::Keys => self.keys.insert(key.to_string(), value),
        }
    }

    /// Clear every cache. Called after data-mutating operations.
    pub fn clear_all(&self) {
        self.genai.clear();
        self.stats.clear();
        self.sessions.clear();
        self.metrics.clear();
        self.names.clear();
        self.keys.clear();
    }
}

/// Tower middleware factory: wrap the API router with
/// `axum::middleware::from_fn_with_state(state, cache_handler)`.
pub async fn cache_handler(
    axum::extract::State(cache): axum::extract::State<Arc<CacheState>>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    let bucket = match req.method() == Method::GET {
        true => match CacheState::policy(&path) {
            Some(b) => b,
            None => return next.run(req).await,
        },
        false => return next.run(req).await,
    };

    let key = match req.uri().query() {
        Some(q) => format!("{}?{}", path, q),
        None => path,
    };

    // 1. Fresh cache hit.
    if let Some(cached) = cache.get(&key, bucket) {
        return json_response(&cached);
    }

    // 2. In-flight dedup: join an identical request already running.
    let (entry, is_owner) = {
        let mut map = cache.in_flight.lock().expect("in_flight mutex poisoned");
        match map.get(&key) {
            Some(existing) => (Arc::clone(existing), false),
            None => {
                let entry = Arc::new(InFlight {
                    done: Arc::new(tokio::sync::Notify::new()),
                    response: Mutex::new(None),
                });
                map.insert(key.clone(), Arc::clone(&entry));
                (entry, true)
            },
        }
    };

    if !is_owner {
        // Wait for the owner to publish its result. `notify_waiters` only
        // wakes already-polling waiters, so re-arm the notified future every
        // second: the response is checked after registering it each round,
        // which bounds any lost-wakeup delay to one second.
        let deadline = tokio::time::Instant::now() + DEDUP_WAIT_TIMEOUT;
        loop {
            let notified = entry.done.notified();
            tokio::pin!(notified);
            if let Some((status, content_type, body)) =
                entry.response.lock().expect("poisoned").clone()
            {
                return rebuild_response(status, content_type, body);
            }
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            // The owner's cleanup guard removes the entry when its
            // request is cancelled or panics. If the entry we are
            // waiting on is gone, no result is coming — stop waiting
            // out the full timeout (bounds the delay to ~1 s).
            let still_present = cache
                .in_flight
                .lock()
                .expect("in_flight mutex poisoned")
                .get(&key)
                .is_some_and(|current| Arc::ptr_eq(current, &entry));
            if !still_present {
                break;
            }
            let _ = tokio::time::timeout(Duration::from_secs(1), notified).await;
        }
        // The owner did not finish in time (or was cancelled by a client
        // disconnect and will never publish). Evict the stale in-flight
        // entry so later duplicates become fresh owners instead of
        // waiting out the timeout again, then run this request ourselves.
        // Only evict if this exact entry is still registered — a fresh
        // owner may have taken the key in the meantime. Scoped so the
        // guard does not live across the await below (a held MutexGuard
        // would make the middleware future !Send).
        {
            let mut map = cache.in_flight.lock().expect("in_flight mutex poisoned");
            match map.get(&key) {
                Some(current) if Arc::ptr_eq(current, &entry) => {
                    map.remove(&key);
                },
                _ => {},
            }
        }
        entry.done.notify_waiters();
        return next.run(req).await;
    }

    // Owner cleanup: removes the entry (and wakes waiters) whenever
    // this future ends — normal return, client disconnect, or panic.
    // Declared after the wait branch so waiters' `return` paths never
    // hold it; it drops when the function unwinds below.
    let _guard = InFlightGuard {
        cache: Arc::clone(&cache),
        key: key.clone(),
        entry: Arc::clone(&entry),
    };

    // 3. Owner: run the real handler, capture the body, publish.
    let response = next.run(req).await;
    let status = response.status();
    let response = if status.is_success() {
        let (parts, body) = response.into_parts();
        match to_bytes(body, MAX_CACHE_BODY_BYTES).await {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                // Only cache valid JSON; pass anything else through.
                if serde_json::from_str::<serde_json::Value>(&text).is_ok() {
                    cache.insert(&key, text, bucket);
                }
                let content_type = parts
                    .headers
                    .get(axum::http::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());
                *entry.response.lock().expect("poisoned") =
                    Some((status.as_u16(), content_type, bytes.to_vec()));
                Response::from_parts(parts, Body::from(bytes))
            },
            Err(err) => {
                // Body too large or unreadable: unblock waiters, don't cache.
                // The body was consumed while buffering, so the only honest
                // response left is an error.
                *entry.response.lock().expect("poisoned") = None;
                tracing::debug!("Not caching {} response: {}", key, err);
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(
                        r#"{{"error":"response too large to cache: {}"}}"#,
                        err
                    )))
                    .expect("valid error response")
            },
        }
    } else {
        // Never cache errors; waiters fall through to their own request.
        *entry.response.lock().expect("poisoned") = None;
        response
    };

    // The guard (installed before the wait branch) removes the
    // in-flight entry and wakes waiters when this function returns.
    response
}

/// Rebuild a 200 JSON response from cached bytes.
fn json_response(body: &str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header("x-cache", "hit")
        .body(Body::from(body.to_string()))
        .expect("valid cached response")
}

/// Rebuild a response from a captured (status, content-type, body) triple.
fn rebuild_response(status: u16, content_type: Option<String>, body: Vec<u8>) -> Response {
    let mut builder =
        Response::builder().status(StatusCode::from_u16(status).unwrap_or(StatusCode::OK));
    if let Some(ct) = content_type {
        builder = builder.header(axum::http::header::CONTENT_TYPE, ct);
    }
    builder
        .header("x-cache", "dedup")
        .body(Body::from(body))
        .expect("valid rebuilt response")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_whitelist() {
        assert_eq!(CacheState::policy("/api/genai/usage"), Some(Bucket::Genai));
        assert_eq!(CacheState::policy("/api/stats"), Some(Bucket::Stats));
        assert_eq!(CacheState::policy("/api/sessions"), Some(Bucket::Sessions));
        assert_eq!(CacheState::policy("/api/metrics"), Some(Bucket::Metrics));
        assert_eq!(
            CacheState::policy("/api/metrics/names"),
            Some(Bucket::Names)
        );
        assert_eq!(
            CacheState::policy("/api/metrics/timeseries/whatever"),
            Some(Bucket::Metrics)
        );
        assert_eq!(CacheState::policy("/api/resource-keys"), Some(Bucket::Keys));
        assert_eq!(CacheState::policy("/api/traces"), None);
        assert_eq!(CacheState::policy("/api/health"), None);
        assert_eq!(CacheState::policy("/api/admin/purge"), None);
        // genai subpaths match, but a lookalike prefix does not
        assert_eq!(
            CacheState::policy("/api/genai/other/thing"),
            Some(Bucket::Genai)
        );
        assert_eq!(CacheState::policy("/api/genai2/usage"), None);
        assert_eq!(CacheState::policy("/api/metricstore"), None);
    }

    #[test]
    fn test_bucket_ttls() {
        assert_eq!(Bucket::Genai.ttl(), Duration::from_secs(20));
        assert_eq!(Bucket::Stats.ttl(), Duration::from_secs(60));
        assert_eq!(Bucket::Sessions.ttl(), Duration::from_secs(10));
        assert_eq!(Bucket::Metrics.ttl(), Duration::from_secs(60));
        assert_eq!(Bucket::Names.ttl(), Duration::from_secs(600));
        assert_eq!(Bucket::Keys.ttl(), Duration::from_secs(300));
    }

    #[test]
    fn test_cache_clear_all() {
        let state = CacheState::new();
        state.insert("k1", "v1".to_string(), Bucket::Genai);
        state.insert("k2", "v2".to_string(), Bucket::Stats);
        state.insert("k3", "v3".to_string(), Bucket::Sessions);
        state.insert("k4", "v4".to_string(), Bucket::Metrics);
        state.insert("k5", "v5".to_string(), Bucket::Names);
        state.insert("k6", "v6".to_string(), Bucket::Keys);
        assert!(state.get("k1", Bucket::Genai).is_some());
        assert!(state.get("k2", Bucket::Stats).is_some());
        assert!(state.get("k3", Bucket::Sessions).is_some());
        assert!(state.get("k4", Bucket::Metrics).is_some());
        assert!(state.get("k5", Bucket::Names).is_some());
        assert!(state.get("k6", Bucket::Keys).is_some());
        state.clear_all();
        assert!(state.get("k1", Bucket::Genai).is_none());
        assert!(state.get("k2", Bucket::Stats).is_none());
        assert!(state.get("k3", Bucket::Sessions).is_none());
        assert!(state.get("k4", Bucket::Metrics).is_none());
        assert!(state.get("k5", Bucket::Names).is_none());
        assert!(state.get("k6", Bucket::Keys).is_none());
    }

    // ── Middleware integration tests ────────────────────────────────────────

    use axum::{routing::get, Json, Router};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::ServiceExt;

    fn get_request(uri: &str) -> Request {
        Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("valid request")
    }

    async fn body_text(response: Response) -> String {
        let bytes = http_body_util::BodyExt::collect(response.into_body())
            .await
            .expect("body readable")
            .to_bytes();
        String::from_utf8(bytes.to_vec()).expect("utf-8 body")
    }

    /// Handler counting how many times it actually runs.
    async fn counting_handler(count: Arc<AtomicUsize>) -> Json<serde_json::Value> {
        let n = count.fetch_add(1, Ordering::SeqCst) + 1;
        Json(serde_json::json!({ "handler_runs": n }))
    }

    #[tokio::test]
    async fn test_ttl_cache_hit_skips_handler() {
        let count = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&count);
        let app = Router::new()
            .route(
                "/api/genai/usage",
                get(move |_req: axum::extract::Request| async move { counting_handler(c).await }),
            )
            .layer(axum::middleware::from_fn_with_state(
                CacheState::new(),
                cache_handler,
            ));

        let first = app
            .clone()
            .oneshot(get_request("/api/genai/usage?start=1&end=2"))
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        assert!(first.headers().get("x-cache").is_none());

        // Same path + query within the TTL: served from cache, handler not
        // re-run.
        let second = app
            .clone()
            .oneshot(get_request("/api/genai/usage?start=1&end=2"))
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::OK);
        assert_eq!(
            second
                .headers()
                .get("x-cache")
                .and_then(|v| v.to_str().ok()),
            Some("hit")
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(body_text(second).await, r#"{"handler_runs":1}"#);

        // Different query params → different key → handler runs again.
        let third = app
            .clone()
            .oneshot(get_request("/api/genai/usage?start=1&end=3"))
            .await
            .unwrap();
        assert_eq!(third.status(), StatusCode::OK);
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_in_flight_dedup_collapses_concurrent_requests() {
        let count = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&count);
        let app = Router::new()
            .route(
                "/api/genai/usage",
                get(move |_req: axum::extract::Request| async move {
                    // Long enough that two concurrent requests overlap.
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    counting_handler(c).await
                }),
            )
            .layer(axum::middleware::from_fn_with_state(
                CacheState::new(),
                cache_handler,
            ));

        let uri = "/api/genai/usage?start=9&end=9";
        let (first, second) = tokio::join!(
            app.clone().oneshot(get_request(uri)),
            app.clone().oneshot(get_request(uri)),
        );
        let first = first.unwrap();
        let second = second.unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(second.status(), StatusCode::OK);
        // The handler ran exactly once despite two concurrent requests.
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(body_text(first).await, body_text(second).await);
    }

    #[tokio::test]
    async fn test_non_whitelisted_path_passes_through() {
        let count = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&count);
        let app = Router::new()
            .route(
                "/api/traces",
                get(move |_req: axum::extract::Request| async move { counting_handler(c).await }),
            )
            .layer(axum::middleware::from_fn_with_state(
                CacheState::new(),
                cache_handler,
            ));

        let _ = app
            .clone()
            .oneshot(get_request("/api/traces"))
            .await
            .unwrap();
        let second = app
            .clone()
            .oneshot(get_request("/api/traces"))
            .await
            .unwrap();
        // Not whitelisted: every request reaches the handler, no x-cache.
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert!(second.headers().get("x-cache").is_none());
    }

    /// Regression test: when the owner of an in-flight dedup entry is
    /// cancelled (client disconnect), its RAII guard must remove the
    /// entry. Before the fix the entry lingered and every duplicate —
    /// including waiters already parked on it — had to wait out the
    /// full 120 s DEDUP_WAIT_TIMEOUT before running its own copy.
    #[tokio::test]
    async fn test_cancelled_owner_cleans_up_in_flight_entry() {
        let count = Arc::new(AtomicUsize::new(0));
        let state = CacheState::new();
        let c = Arc::clone(&count);
        let app = Router::new()
            .route(
                "/api/genai/usage",
                get(move |_req: axum::extract::Request| async move {
                    // Long enough that the owner is cancelled mid-run.
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    counting_handler(c).await
                }),
            )
            .layer(axum::middleware::from_fn_with_state(
                Arc::clone(&state),
                cache_handler,
            ));

        let uri = "/api/genai/usage?start=7&end=7";

        // Owner request first, so it is guaranteed to register the
        // in-flight entry before the duplicate joins it.
        let owner_app = app.clone();
        let owner = tokio::spawn(async move { owner_app.oneshot(get_request(uri)).await });
        tokio::time::sleep(Duration::from_millis(150)).await;
        eprintln!(
            "DBG after owner sleep: count={} inflight={}",
            count.load(Ordering::SeqCst),
            state.in_flight.lock().expect("poisoned").len()
        );

        // A concurrent duplicate parks on the owner's entry.
        let waiter_app = app.clone();
        let waiter = tokio::spawn(async move { waiter_app.oneshot(get_request(uri)).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        eprintln!(
            "DBG after waiter sleep: count={} inflight={}",
            count.load(Ordering::SeqCst),
            state.in_flight.lock().expect("poisoned").len()
        );

        // Client disconnect: drop the owner mid-run.
        owner.abort();

        let started = tokio::time::Instant::now();
        let waiter_response = waiter
            .await
            .expect("waiter task must not abort")
            .expect("waiter request completes");
        let waiter_elapsed = started.elapsed();

        // The waiter must get a usable response quickly. Depending on
        // scheduling, the owner's handler may or may not have started
        // before the abort, so 1 or 2 handler runs are both correct —
        // what must NOT happen is the waiter waiting out the 120 s
        // dedup timeout on the dead entry.
        let waiter_status = waiter_response.status();
        let waiter_body = body_text(waiter_response).await;
        assert_eq!(waiter_status, StatusCode::OK);
        let runs = count.load(Ordering::SeqCst);
        assert!(
            (1..=2).contains(&runs),
            "expected the owner's and/or waiter's handler run, got {runs}"
        );
        let parsed: serde_json::Value =
            serde_json::from_str(&waiter_body).expect("waiter got valid JSON");
        assert!(parsed.get("handler_runs").is_some());
        assert!(
            waiter_elapsed < Duration::from_secs(5),
            "waiter waited {waiter_elapsed:?} after the owner's cancellation; a cleaned-up entry lets it run its own copy within ~1 s"
        );

        // The key is free again: no in-flight entry lingers, and a
        // follow-up request is served immediately (cache hit on the
        // waiter's result).
        assert!(state.in_flight.lock().expect("poisoned").is_empty());
        let follower = app
            .clone()
            .oneshot(get_request(uri))
            .await
            .expect("follower request completes");
        assert_eq!(follower.status(), StatusCode::OK);
        assert!(state.in_flight.lock().expect("poisoned").is_empty());
    }

    /// The guard's removal is conditional on the map still holding the
    /// guard's exact entry: a stale guard whose entry was already
    /// evicted and replaced must not delete the newcomer's entry.
    #[test]
    fn test_stale_in_flight_guard_keeps_new_owner() {
        let state = CacheState::new();
        let key = "/api/stats".to_string();

        let old_entry = Arc::new(InFlight {
            done: Arc::new(tokio::sync::Notify::new()),
            response: Mutex::new(None),
        });
        state
            .in_flight
            .lock()
            .expect("poisoned")
            .insert(key.clone(), Arc::clone(&old_entry));

        // A fresh owner re-registers the same key, then the stale guard
        // drops (as an aborted owner's would, after the re-registration).
        let new_entry = Arc::new(InFlight {
            done: Arc::new(tokio::sync::Notify::new()),
            response: Mutex::new(None),
        });
        state
            .in_flight
            .lock()
            .expect("poisoned")
            .insert(key.clone(), Arc::clone(&new_entry));

        let stale_guard = InFlightGuard {
            cache: Arc::clone(&state),
            key,
            entry: old_entry,
        };
        drop(stale_guard);

        let map = state.in_flight.lock().expect("poisoned");
        assert!(
            map.get("/api/stats")
                .is_some_and(|current| Arc::ptr_eq(current, &new_entry)),
            "the stale guard must not evict the new owner's entry"
        );
    }
}
