//! Tests for GenAI token usage API endpoint

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use otelite_api::{DashboardConfig, DashboardServer};
use otelite_core::api::{AgentRollupResponse, TokenUsageResponse};
use otelite_core::telemetry::log::{LogRecord, SeverityLevel};
use otelite_core::telemetry::metric::{Metric, MetricType};
use otelite_core::telemetry::trace::{Span, SpanKind, SpanStatus, StatusCode as SpanStatusCode};
use otelite_storage::sqlite::SqliteBackend;
use otelite_storage::{StorageBackend, StorageConfig};
use std::collections::HashMap;
use std::sync::Arc;
use tower::ServiceExt;

async fn setup_test_server() -> (DashboardServer, Arc<dyn StorageBackend>, tempfile::TempDir) {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let config = DashboardConfig::default();
    let storage_config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut storage = SqliteBackend::new(storage_config);
    storage.initialize().await.unwrap();
    let storage: Arc<dyn StorageBackend> = Arc::new(storage);

    let server = DashboardServer::new(config, storage.clone());
    (server, storage, temp_dir)
}

#[tokio::test]
async fn test_get_token_usage_empty() {
    let (server, _storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/genai/usage")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let usage: TokenUsageResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(usage.summary.total_input_tokens, 0);
    assert_eq!(usage.summary.total_output_tokens, 0);
    assert_eq!(usage.summary.total_requests, 0);
    assert_eq!(usage.by_model.len(), 0);
    assert_eq!(usage.by_system.len(), 0);
}

#[tokio::test]
async fn test_get_token_usage_with_time_params() {
    let (server, _storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/genai/usage?start_time=1000&end_time=2000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let usage: TokenUsageResponse = serde_json::from_slice(&body).unwrap();

    // Should return empty results (placeholder implementation)
    assert_eq!(usage.summary.total_input_tokens, 0);
    assert_eq!(usage.summary.total_output_tokens, 0);
}

#[tokio::test]
async fn test_get_token_usage_response_structure() {
    let (server, _storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/genai/usage")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let usage: TokenUsageResponse = serde_json::from_slice(&body).unwrap();

    // Verify response structure (values are u64, so always >= 0)
    assert!(usage.by_model.is_empty() || !usage.by_model.is_empty());
    assert!(usage.by_system.is_empty() || !usage.by_system.is_empty());
}

#[tokio::test]
async fn test_get_tool_approvals_empty() {
    let (server, _storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/genai/tool_approvals")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let stats: otelite_core::api::ToolApprovalStats = serde_json::from_slice(&body).unwrap();
    assert_eq!(stats.total, 0);
    assert_eq!(stats.auto_accepted, 0);
}

#[tokio::test]
async fn test_get_stop_reasons_empty() {
    let (server, _storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/genai/stop_reasons")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let reasons: Vec<otelite_core::api::StopReasonCount> = items(&body, "stop_reasons");
    assert!(reasons.is_empty());
}

#[tokio::test]
async fn test_get_context_type_split_empty() {
    let (server, _storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/genai/context_type_split")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let split: Vec<otelite_core::api::ContextTypeSplit> = items(&body, "context_type_split");
    // context_type_split returns empty vec for empty DB
    assert!(split.is_empty());
}

#[tokio::test]
async fn test_get_tool_errors_empty() {
    let (server, _storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/genai/tool_errors")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let errors: Vec<otelite_core::api::ToolErrorEntry> = items(&body, "tool_errors");
    assert!(errors.is_empty());
}

#[tokio::test]
async fn test_get_hour_of_day_empty() {
    let (server, _storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/genai/hour_of_day")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let buckets: Vec<otelite_core::api::HourOfDayBucket> = items(&body, "hour_of_day");
    // Empty DB returns 24 zero-filled buckets (one per hour)
    assert_eq!(buckets.len(), 24);
    assert!(buckets
        .iter()
        .all(|b| b.llm_calls == 0 && b.tool_calls == 0));
}

// ── per-harness agent rollup (issue #125) ──────────────────────────────────

const R0: i64 = 1_700_000_000_000_000_000;
const R1: i64 = R0 + 1_000_000_000; // 1-second window [R0, R1]

fn agent_metric(
    name: &str,
    timestamp: i64,
    counter: Option<u64>,
    histogram: Option<(u64, f64)>,
    attributes: &[(&str, &str)],
) -> Metric {
    let metric_type = match (counter, histogram) {
        (Some(v), None) => MetricType::Counter(v),
        (None, Some((count, sum))) => MetricType::Histogram {
            count,
            sum,
            buckets: vec![],
        },
        _ => MetricType::Gauge(0.0),
    };
    let mut m = Metric {
        name: name.to_string(),
        description: None,
        unit: None,
        metric_type,
        timestamp,
        attributes: HashMap::new(),
        resource: None,
    };
    for (k, v) in attributes {
        m.attributes.insert(k.to_string(), v.to_string());
    }
    m
}

#[tokio::test]
async fn test_get_agents_empty() {
    let (server, _storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/genai/agents?start_time={R0}&end_time={R1}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let rollup: AgentRollupResponse = serde_json::from_slice(&body).unwrap();
    assert!(rollup.agents.is_empty(), "empty DB -> no agent rows");
}

#[tokio::test]
async fn test_get_agents_with_data() {
    let (server, storage, _temp_dir) = setup_test_server().await;

    // opencode: 1 in-window top-level session, cumulative token counters
    // (input 100 -> 250, reasoning 50 -> 60), cost counter 0.5 -> 1.25,
    // tool calls 10 -> 14, retries 3 -> 5.
    storage
        .write_metric(&agent_metric(
            "opencode.session.count",
            R1,
            Some(1),
            None,
            &[("session.id", "s1"), ("is_subagent", "false")],
        ))
        .await
        .unwrap();
    for (ts, input, reasoning) in [(R0 - 1_000_000_000, 100u64, 50u64), (R1, 250, 60)] {
        storage
            .write_metric(&agent_metric(
                "opencode.token.usage",
                ts,
                Some(input),
                None,
                &[
                    ("agent", "a"),
                    ("model", "m1"),
                    ("type", "input"),
                    ("session.id", "s1"),
                ],
            ))
            .await
            .unwrap();
        storage
            .write_metric(&agent_metric(
                "opencode.token.usage",
                ts,
                Some(reasoning),
                None,
                &[
                    ("agent", "a"),
                    ("model", "m1"),
                    ("type", "reasoning"),
                    ("session.id", "s1"),
                ],
            ))
            .await
            .unwrap();
    }
    for (ts, sum) in [(R0 - 1_000_000_000, 0.5f64), (R1, 1.25)] {
        storage
            .write_metric(&agent_metric(
                "opencode.session.cost.total",
                ts,
                None,
                Some((2, sum)),
                &[("session.id", "s1")],
            ))
            .await
            .unwrap();
    }
    for (ts, count) in [(R0 - 1_000_000_000, 10u64), (R1, 14)] {
        storage
            .write_metric(&agent_metric(
                "opencode.tool.duration",
                ts,
                None,
                Some((count, 0.0)),
                &[("session.id", "s1"), ("tool_name", "Bash")],
            ))
            .await
            .unwrap();
    }
    for (ts, retries) in [(R0 - 1_000_000_000, 3u64), (R1, 5)] {
        storage
            .write_metric(&agent_metric(
                "opencode.retry.count",
                ts,
                Some(retries),
                None,
                &[("session.id", "s1")],
            ))
            .await
            .unwrap();
    }

    // codex: 3 cli sessions, turn tokens (input 100, output 50, total 150),
    // 2 tool calls, 1 failed request.
    storage
        .write_metric(&agent_metric(
            "codex.thread.started",
            R1,
            Some(3),
            None,
            &[("session_source", "cli")],
        ))
        .await
        .unwrap();
    for (tt, sum) in [("input", 100.0f64), ("output", 50.0), ("total", 150.0)] {
        storage
            .write_metric(&agent_metric(
                "codex.turn.token_usage",
                R1,
                None,
                Some((1, sum)),
                &[("model", "c1"), ("token_type", tt)],
            ))
            .await
            .unwrap();
    }
    storage
        .write_metric(&agent_metric(
            "codex.tool.call",
            R1,
            Some(1),
            None,
            &[("tool", "shell")],
        ))
        .await
        .unwrap();
    storage
        .write_metric(&agent_metric(
            "codex.tool.call",
            R1,
            Some(1),
            None,
            &[("tool", "apply_patch")],
        ))
        .await
        .unwrap();
    storage
        .write_metric(&agent_metric(
            "codex.api_request",
            R1,
            Some(1),
            None,
            &[("success", "false")],
        ))
        .await
        .unwrap();

    // claude: 1 in-window session, per-event tokens, 1 tool-execution span.
    storage
        .write_metric(&agent_metric(
            "claude_code.session.count",
            R1,
            Some(1),
            None,
            &[("session.id", "s2")],
        ))
        .await
        .unwrap();
    storage
        .write_metric(&agent_metric(
            "claude_code.token.usage",
            R1,
            Some(1000),
            None,
            &[("session.id", "s2"), ("model", "k1"), ("type", "input")],
        ))
        .await
        .unwrap();
    storage
        .write_metric(&agent_metric(
            "claude_code.token.usage",
            R1,
            Some(300),
            None,
            &[("session.id", "s2"), ("model", "k1"), ("type", "output")],
        ))
        .await
        .unwrap();
    let mut span = Span {
        trace_id: "t".to_string(),
        span_id: "sp1".to_string(),
        parent_span_id: None,
        name: "claude_code.tool.execution".to_string(),
        kind: SpanKind::Internal,
        start_time: R1,
        end_time: R1 + 1,
        attributes: HashMap::new(),
        status: SpanStatus {
            code: SpanStatusCode::Ok,
            message: None,
        },
        events: Vec::new(),
        resource: None,
    };
    storage.write_span(&span).await.unwrap();
    let _ = &mut span;

    let app = server.build_router();
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/genai/agents?start_time={R0}&end_time={R1}&bucket_secs=1"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let rollup: AgentRollupResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(rollup.agents.len(), 3);

    let find = |name: &str| {
        rollup
            .agents
            .iter()
            .find(|a| a.agent == name)
            .unwrap_or_else(|| panic!("{name} missing"))
    };

    // Sorted by cost desc: opencode (actual $0.75) first; codex/claude have
    // no pricing in the test DB -> None cost, alphabetical tie-break.
    assert_eq!(rollup.agents[0].agent, "opencode");
    assert_eq!(rollup.agents[1].agent, "claude");
    assert_eq!(rollup.agents[2].agent, "codex");

    let oc = find("opencode");
    assert_eq!(oc.sessions, 1);
    assert_eq!(oc.cost_usd, Some(0.75), "opencode cost is its own counter");
    assert_eq!(oc.cost_source.as_deref(), Some("actual"));
    assert_eq!(oc.tokens.input, 150);
    assert_eq!(oc.tokens.reasoning, 10);
    assert_eq!(oc.tool_calls, 4);
    assert_eq!(oc.retries, Some(2));
    assert_eq!(oc.series.len(), 1, "one 1s bucket in the window");

    let cx = find("codex");
    assert_eq!(cx.sessions, 3);
    assert_eq!(
        cx.cost_usd, None,
        "no pricing in test DB -> no fabricated cost"
    );
    assert_eq!(cx.cost_source.as_deref(), Some("estimated"));
    assert_eq!(
        cx.tokens.input, 100,
        "total token_type must not double-count"
    );
    assert_eq!(cx.tokens.output, 50);
    assert_eq!(cx.tool_calls, 2);
    assert_eq!(cx.retries, Some(1));

    let cl = find("claude");
    assert_eq!(cl.sessions, 1);
    assert_eq!(cl.cost_usd, None);
    assert_eq!(cl.tokens.input, 1000);
    assert_eq!(cl.tokens.output, 300);
    assert_eq!(cl.tool_calls, 1, "tool.execution span count");
    assert_eq!(cl.retries, None, "claude emits no retry telemetry");
}

use otelite_core::api::{CostDistributionResponse, SessionCostResponse};

#[tokio::test]
async fn test_get_session_costs_empty() {
    let (server, _storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/sessions/costs?start_time={R0}&end_time={R1}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let costs: SessionCostResponse = serde_json::from_slice(&body).unwrap();
    assert!(costs.sessions.is_empty());
    assert_eq!(costs.median_cost_usd, None);
    assert!(costs.anomaly_rule.contains("3 x median"));
}

#[tokio::test]
async fn test_get_session_costs_with_data() {
    let (server, storage, _temp_dir) = setup_test_server().await;

    // Four opencode sessions with distinct cumulative cost counters (last
    // value in the window is the total): 1.0, 2.0, 3.0, 100.0 → median
    // 2.5, threshold 7.5 → only the $100 session is anomalous.
    for (sid, cost) in [("s1", 1.0), ("s2", 2.0), ("s3", 3.0), ("s4", 100.0)] {
        storage
            .write_metric(&agent_metric(
                "opencode.session.cost.total",
                R0 - 1_000_000_000,
                None,
                Some((1, 0.0)),
                &[("session.id", sid)],
            ))
            .await
            .unwrap();
        storage
            .write_metric(&agent_metric(
                "opencode.session.cost.total",
                R1,
                None,
                Some((2, cost)),
                &[("session.id", sid), ("project.id", "proj-1")],
            ))
            .await
            .unwrap();
    }
    storage
        .write_metric(&agent_metric(
            "opencode.session.duration",
            R1,
            None,
            Some((1, 60_000.0)),
            &[("session.id", "s1")],
        ))
        .await
        .unwrap();
    storage
        .write_metric(&agent_metric(
            "opencode.session.token.total",
            R1,
            None,
            Some((1, 12_345.0)),
            &[("session.id", "s1")],
        ))
        .await
        .unwrap();

    // claude: one session from llm_request span attributes (no cost counter
    // is trusted — priced by the API layer, which has no pricing rows in the
    // test DB, so its cost stays None).
    let mut span = Span {
        trace_id: "t".to_string(),
        span_id: "sp1".to_string(),
        parent_span_id: None,
        name: "claude_code.llm_request".to_string(),
        kind: SpanKind::Internal,
        start_time: R1,
        end_time: R1 + 1,
        attributes: HashMap::new(),
        status: SpanStatus {
            code: SpanStatusCode::Ok,
            message: None,
        },
        events: Vec::new(),
        resource: None,
    };
    span.attributes
        .insert("session.id".to_string(), "c1".to_string());
    span.attributes
        .insert("model".to_string(), "m1".to_string());
    span.attributes
        .insert("input_tokens".to_string(), "100".to_string());
    span.attributes
        .insert("output_tokens".to_string(), "40".to_string());
    span.attributes
        .insert("cache_read_tokens".to_string(), "10".to_string());
    span.attributes
        .insert("cache_creation_tokens".to_string(), "5".to_string());
    storage.write_span(&span).await.unwrap();

    let app = server.build_router();
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/sessions/costs?start_time={R0}&end_time={R1}&limit=50"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let costs: SessionCostResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(costs.sessions.len(), 5);
    // cost desc: s4 ($100) first, then s3/s2/s1, unpriced claude last.
    assert_eq!(
        costs
            .sessions
            .iter()
            .map(|s| s.session_id.as_str())
            .collect::<Vec<_>>(),
        vec!["s4", "s3", "s2", "s1", "c1"]
    );
    assert_eq!(costs.median_cost_usd, Some(2.5), "median of [1,2,3,100]");
    assert!(costs.sessions[0].anomaly, "$100 > 3 x $2.5");
    assert!(!costs.sessions[1].anomaly, "$3 < $7.5");
    assert!(!costs.sessions[4].anomaly, "no cost → cannot be anomalous");

    let s4 = &costs.sessions[0];
    assert_eq!(s4.agent, "opencode");
    assert_eq!(s4.cost_usd, Some(100.0), "last cumulative value, not a sum");
    assert_eq!(s4.cost_source.as_deref(), Some("actual"));
    assert_eq!(s4.project_id.as_deref(), Some("proj-1"));

    let s1 = &costs.sessions[3];
    assert_eq!(s1.tokens, 12_345);
    assert_eq!(s1.duration_secs, Some(60.0), "60 000 ms → 60.0 s");

    let c1 = &costs.sessions[4];
    assert_eq!(c1.agent, "claude");
    assert_eq!(c1.tokens, 155);
    assert_eq!(
        c1.cost_usd, None,
        "no pricing rows → null, not a fabricated zero"
    );
    assert_eq!(c1.cost_source.as_deref(), Some("estimated"));

    // limit truncates the listing but the anomaly flag survived it: refetch
    // with limit=2 — s4 (the outlier) must still be flagged.
    let app = server.build_router();
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/sessions/costs?start_time={R0}&end_time={R1}&limit=2"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let limited: SessionCostResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(limited.sessions.len(), 2);
    assert!(limited.sessions[0].anomaly);
    assert_eq!(
        limited.median_cost_usd,
        Some(2.5),
        "median over the full window"
    );
}

#[tokio::test]
async fn test_get_session_cost_distribution() {
    let (server, storage, _temp_dir) = setup_test_server().await;

    // Costs 0, 0.01, 150, 1000 across four opencode sessions, plus one
    // unpriced claude session (excluded from the distribution). With 4
    // buckets the bounds are [0,100) [100,215.4) [215.4,464.2) [464.2,1000].
    for (sid, cost) in [("d0", 0.0), ("d1", 0.01), ("d2", 150.0), ("d3", 1000.0)] {
        storage
            .write_metric(&agent_metric(
                "opencode.session.cost.total",
                R1,
                None,
                Some((1, cost)),
                &[("session.id", sid)],
            ))
            .await
            .unwrap();
    }
    let mut span = Span {
        trace_id: "t".to_string(),
        span_id: "sp1".to_string(),
        parent_span_id: None,
        name: "claude_code.llm_request".to_string(),
        kind: SpanKind::Internal,
        start_time: R1,
        end_time: R1 + 1,
        attributes: HashMap::new(),
        status: SpanStatus {
            code: SpanStatusCode::Ok,
            message: None,
        },
        events: Vec::new(),
        resource: None,
    };
    span.attributes
        .insert("session.id".to_string(), "c1".to_string());
    span.attributes
        .insert("input_tokens".to_string(), "10".to_string());
    span.attributes
        .insert("output_tokens".to_string(), "10".to_string());
    storage.write_span(&span).await.unwrap();

    let app = server.build_router();
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/sessions/cost-distribution?start_time={R0}&end_time={R1}&buckets=4"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let dist: CostDistributionResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(dist.buckets.len(), 4);
    let total: u64 = dist.buckets.iter().map(|b| b.count).sum();
    assert_eq!(total, 4, "unpriced session excluded");
    // [0, 0.1) catches both zero-ish costs: 0 and 0.01
    assert_eq!(dist.buckets[0].count, 2);
    assert_eq!(dist.buckets[0].min_usd, 0.0);
    // $150 sits in the second bucket, $1000 in the inclusive last one.
    assert_eq!(dist.buckets[1].count, 1);
    assert_eq!(dist.buckets[2].count, 0);
    assert_eq!(dist.buckets[3].count, 1);
    assert_eq!(dist.buckets[3].max_usd, 1000.0);
    // bounds ascend and are contiguous
    for w in dist.buckets.windows(2) {
        assert!(w[0].max_usd <= w[1].max_usd);
        assert!((w[0].max_usd - w[1].min_usd).abs() < 1e-9);
    }
}

// ── per-project rollup (issue #127) ───────────────────────────────────────

#[tokio::test]
async fn test_get_projects_empty() {
    let (server, _storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/genai/projects?start_time={R0}&end_time={R1}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let rollup: otelite_core::api::ProjectRollupResponse = serde_json::from_slice(&body).unwrap();
    assert!(rollup.projects.is_empty(), "empty DB -> no project rows");
}

#[tokio::test]
async fn test_get_projects_with_data() {
    let (server, storage, _temp_dir) = setup_test_server().await;

    // opencode: two top-level sessions in distinct projects, one
    // label-less; cumulative token + cost counters per session.
    for (sid, project) in [("s1", Some("projA")), ("s2", Some("projB")), ("s3", None)] {
        let mut attrs: Vec<(&str, &str)> = vec![("session.id", sid), ("is_subagent", "false")];
        if let Some(p) = project {
            attrs.push(("project.id", p));
        }
        storage
            .write_metric(&agent_metric(
                "opencode.session.count",
                R1,
                Some(1),
                None,
                &attrs,
            ))
            .await
            .unwrap();
    }
    // s1: input 100 -> 250 across the window, cost 0.5 -> 1.25.
    for (ts, input) in [(R0 - 1_000_000_000, 100u64), (R1, 250)] {
        storage
            .write_metric(&agent_metric(
                "opencode.token.usage",
                ts,
                Some(input),
                None,
                &[
                    ("agent", "a"),
                    ("model", "m1"),
                    ("type", "input"),
                    ("session.id", "s1"),
                ],
            ))
            .await
            .unwrap();
    }
    for (ts, sum) in [(R0 - 1_000_000_000, 0.5f64), (R1, 1.25)] {
        storage
            .write_metric(&agent_metric(
                "opencode.session.cost.total",
                ts,
                None,
                Some((2, sum)),
                &[("session.id", "s1")],
            ))
            .await
            .unwrap();
    }
    // s3 (no project label): 50 output tokens, no cost counter rows.
    storage
        .write_metric(&agent_metric(
            "opencode.token.usage",
            R1,
            Some(50),
            None,
            &[
                ("agent", "a"),
                ("model", "m2"),
                ("type", "output"),
                ("session.id", "s3"),
            ],
        ))
        .await
        .unwrap();

    // codex: 2 cli sessions, 400 input tokens — no project label exists.
    storage
        .write_metric(&agent_metric(
            "codex.thread.started",
            R1,
            Some(2),
            None,
            &[("session_source", "cli")],
        ))
        .await
        .unwrap();
    storage
        .write_metric(&agent_metric(
            "codex.turn.token_usage",
            R1,
            None,
            Some((1, 400.0)),
            &[("model", "c1"), ("token_type", "input")],
        ))
        .await
        .unwrap();

    let app = server.build_router();
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/genai/projects?start_time={R0}&end_time={R1}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let rollup: otelite_core::api::ProjectRollupResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        rollup.projects.len(),
        3,
        "projA + projB + unattributed: {rollup:?}"
    );

    let find = |pid: &str| {
        rollup
            .projects
            .iter()
            .find(|p| p.project_id == pid)
            .unwrap_or_else(|| panic!("{pid} missing"))
    };

    let a = find("projA");
    assert_eq!(a.sessions, 1);
    assert_eq!(
        a.cost_usd,
        Some(0.75),
        "s1 counter delta, no pricing needed"
    );
    assert_eq!(a.cost_source.as_deref(), Some("actual"));
    assert_eq!(a.tokens.input, 150, "250 - 100 baseline");

    let b = find("projB");
    assert_eq!(b.sessions, 1);
    assert_eq!(b.cost_usd, None, "no counter rows, no priced tokens");

    let u = find("unattributed");
    assert_eq!(u.sessions, 3, "1 label-less opencode + 2 codex");
    assert!(
        u.cost_usd.is_none() || u.cost_usd == Some(0.0),
        "no priced models in test pricing DB: {u:?}"
    );
    assert_eq!(u.tokens.input, 400, "codex histogram sum");
    assert_eq!(u.tokens.output, 50, "s3 output");
    assert!(
        u.top_models.iter().all(|m| m.cost_usd.is_none()),
        "no pricing in test DB: {u:?}"
    );

    // Sorted by cost desc: projA ($0.75) first, the rest alphabetical.
    assert_eq!(rollup.projects[0].project_id, "projA");
}

fn llm_request_span(
    span_id: &str,
    model: &str,
    start_time: i64,
    duration_ms: i64,
    ttft_ms: Option<i64>,
) -> Span {
    let mut attributes: HashMap<String, String> = HashMap::new();
    attributes.insert("session.id".to_string(), "s1".to_string());
    attributes.insert("model".to_string(), model.to_string());
    if let Some(ttft) = ttft_ms {
        attributes.insert("ttft_ms".to_string(), ttft.to_string());
    }
    Span {
        trace_id: "t".to_string(),
        span_id: span_id.to_string(),
        parent_span_id: None,
        name: "claude_code.llm_request".to_string(),
        kind: SpanKind::Internal,
        start_time,
        end_time: start_time + duration_ms * 1_000_000,
        attributes,
        status: SpanStatus {
            code: SpanStatusCode::Ok,
            message: None,
        },
        events: Vec::new(),
        resource: None,
    }
}

#[tokio::test]
async fn test_recent_retries_empty() {
    let (server, _storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    let (status, v) = get_json(
        &app,
        &format!("/api/genai/retries?start_time={R0}&end_time={R1}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = v.get("items").expect("items");
    assert!(items.is_array());
    assert_eq!(items.as_array().unwrap().len(), 0);
    assert!(v.get("filters_applied").is_some());
}

#[tokio::test]
async fn test_recent_retries_lists_only_retried_spans() {
    let (server, storage, _temp_dir) = setup_test_server().await;

    // A retried call (attempt 2) inside the window.
    let mut retried = llm_request_span(
        "sp-retry",
        "claude-sonnet-5[1m]",
        R0 + 100_000_000,
        12_000,
        Some(11_700),
    );
    retried
        .attributes
        .insert("attempt".to_string(), "2".to_string());
    storage.write_span(&retried).await.unwrap();

    // A normal call (attempt 1) — excluded.
    let normal = llm_request_span(
        "sp-normal",
        "claude-sonnet-5[1m]",
        R0 + 200_000_000,
        5_000,
        None,
    );
    storage.write_span(&normal).await.unwrap();

    // A retried call that started before the window — excluded (spans are
    // attributed by start time, #203).
    let mut old = llm_request_span(
        "sp-old",
        "claude-sonnet-5[1m]",
        R0 - 500_000_000,
        1_000,
        None,
    );
    old.attributes
        .insert("attempt".to_string(), "3".to_string());
    storage.write_span(&old).await.unwrap();

    let app = server.build_router();
    let (status, v) = get_json(
        &app,
        &format!("/api/genai/retries?start_time={R0}&end_time={R1}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = v["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["span_id"], "sp-retry");
    assert_eq!(items[0]["attempt"], 2);
    assert_eq!(items[0]["ttft_ms"], 11_700);
    assert_eq!(items[0]["session_id"], "s1");
    assert_eq!(items[0]["model"], "claude-sonnet-5[1m]");
    assert_eq!(items[0]["trace_id"], "t");
}

#[tokio::test]
async fn test_get_latency_percentiles_empty() {
    let (server, _storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/genai/latency_percentiles?start_time={R0}&end_time={R1}"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let resp: otelite_core::api::LatencyPercentilesResponse =
        serde_json::from_slice(&body).unwrap();
    assert!(
        resp.metrics
            .values()
            .all(|s| s.all.is_empty() && s.models.is_empty()),
        "empty DB -> no percentile points: {resp:?}"
    );
}

#[tokio::test]
async fn test_get_latency_percentiles_with_data() {
    let (server, storage, _temp_dir) = setup_test_server().await;

    storage
        .write_span(&llm_request_span("sp1", "modelA", R0, 100, Some(20)))
        .await
        .unwrap();
    storage
        .write_span(&llm_request_span(
            "sp2",
            "modelA",
            R0 + 500_000_000,
            300,
            Some(90),
        ))
        .await
        .unwrap();
    storage
        .write_span(&llm_request_span(
            "sp3",
            "modelB",
            R0 + 250_000_000,
            400,
            Some(150),
        ))
        .await
        .unwrap();

    let app = server.build_router();
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/genai/latency_percentiles?start_time={R0}&end_time={R1}&bucket_secs=3600"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let resp: otelite_core::api::LatencyPercentilesResponse =
        serde_json::from_slice(&body).unwrap();

    // R0..R1 spans all fall into one hourly bucket (bucket start floored).
    let dur = &resp.metrics["duration"];
    assert_eq!(dur.all.len(), 1, "single bucket: {resp:?}");
    assert_eq!(dur.all[0].count, 3);
    // durations [100, 300, 400] sorted: p50=300, p90/p95/p99=400
    assert_eq!(dur.all[0].p50_ms, Some(300.0));
    assert_eq!(dur.all[0].p90_ms, Some(400.0));
    assert_eq!(dur.all[0].p95_ms, Some(400.0));
    assert_eq!(dur.all[0].p99_ms, Some(400.0));
    assert_eq!(dur.models.len(), 2);
    assert_eq!(dur.models["modelA"][0].count, 2);
    assert_eq!(dur.models["modelB"][0].count, 1);

    let tt = &resp.metrics["ttft"];
    assert_eq!(tt.all[0].count, 3, "all three spans carried valid ttft");
    assert_eq!(tt.all[0].p50_ms, Some(90.0));
    assert_eq!(tt.all[0].p90_ms, Some(150.0));

    // Unknown metric name → 400, not 500.
    let app = server.build_router();
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/genai/latency_percentiles?start_time={R0}&end_time={R1}&metrics=bogus"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

async fn distribution_body(
    app: axum::Router,
    uri: String,
) -> (StatusCode, otelite_core::api::DistributionResponse) {
    let response = app
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let resp: otelite_core::api::DistributionResponse = serde_json::from_slice(&body).unwrap();
    (status, resp)
}

#[tokio::test]
async fn test_get_distributions_empty() {
    let (server, _storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    let (status, resp) = distribution_body(
        app,
        format!("/api/genai/distributions?metric=llm_duration&start_time={R0}&end_time={R1}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(resp.buckets.is_empty(), "empty DB -> no buckets: {resp:?}");
    assert!(resp.stats.is_none());
    assert_eq!(resp.metric, "llm_duration");
    assert_eq!(resp.unit, "ms");
    assert_eq!(resp.scale, "linear");

    // session_cost is priced in the API layer; empty DB -> empty distribution.
    let app = server.build_router();
    let (status, resp) = distribution_body(
        app,
        format!(
            "/api/genai/distributions?metric=session_cost&start_time={R0}&end_time={R1}&scale=log"
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp.unit, "usd");
    assert!(resp.buckets.is_empty());
}

#[tokio::test]
async fn test_get_distributions_with_data() {
    let (server, storage, _temp_dir) = setup_test_server().await;
    storage
        .write_span(&llm_request_span("sp1", "modelA", R0, 100, Some(20)))
        .await
        .unwrap();
    storage
        .write_span(&llm_request_span(
            "sp2",
            "modelA",
            R0 + 100_000_000,
            300,
            Some(90),
        ))
        .await
        .unwrap();
    storage
        .write_span(&llm_request_span(
            "sp3",
            "modelB",
            R0 + 200_000_000,
            500,
            None,
        ))
        .await
        .unwrap();

    let app = server.build_router();
    let (status, resp) = distribution_body(
        app,
        format!(
            "/api/genai/distributions?metric=llm_duration&start_time={R0}&end_time={R1}&buckets=3&scale=log"
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp.buckets.iter().map(|b| b.count).sum::<u64>(), 3);
    let s = resp.stats.as_ref().expect("stats present");
    assert_eq!(s.count, 3);
    assert_eq!(s.min, 100.0);
    assert_eq!(s.max, 500.0);
    assert!((s.mean - (100.0 + 300.0 + 500.0) / 3.0).abs() < 1e-9);
    // sorted [100, 300, 500]: p50 -> idx (3-1)*0.5=1 -> 300
    assert_eq!(s.p50, 300.0);
    assert_eq!(s.p95, 500.0);

    // ttft cohort: two span values, third span has none.
    let app = server.build_router();
    let (status, resp) = distribution_body(
        app,
        format!("/api/genai/distributions?metric=ttft&start_time={R0}&end_time={R1}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let s = resp.stats.as_ref().unwrap();
    assert_eq!(s.count, 2, "sp1 + sp2 carried valid ttft");
    assert_eq!(s.min, 20.0);
    assert_eq!(s.max, 90.0);

    // Unknown metric → 400 (error body, not a distribution).
    let app = server.build_router();
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/genai/distributions?metric=bogus&start_time={R0}&end_time={R1}"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Bad scale → 400.
    let app = server.build_router();
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/genai/distributions?metric=ttft&start_time={R0}&end_time={R1}&scale=exponential"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// ── session context (issue #134) ──────────────────────────────────────────

fn ctx_span(span_id: &str, session_id: &str, start: i64, duration_ms: i64) -> Span {
    let mut attributes: HashMap<String, String> = HashMap::new();
    attributes.insert("session.id".to_string(), session_id.to_string());
    attributes.insert("model".to_string(), "claude-sonnet-5".to_string());
    Span {
        trace_id: "t".to_string(),
        span_id: span_id.to_string(),
        parent_span_id: None,
        name: "claude_code.llm_request".to_string(),
        kind: SpanKind::Internal,
        start_time: start,
        end_time: start + duration_ms * 1_000_000,
        attributes,
        status: SpanStatus {
            code: SpanStatusCode::Ok,
            message: None,
        },
        events: Vec::new(),
        resource: None,
    }
}

fn ctx_log(ts: i64, body: &str, severity: i32, attributes: &[(&str, &str)]) -> LogRecord {
    let mut log = LogRecord::new(SeverityLevel::from_i32(severity).unwrap(), body, ts);
    for (k, v) in attributes {
        log.attributes.insert(k.to_string(), v.to_string());
    }
    log
}

#[tokio::test]
async fn test_get_session_context_empty() {
    let (server, _storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/sessions/no-such-session/context")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_get_session_context_mixed_session() {
    let (server, storage, _temp_dir) = setup_test_server().await;
    let sid = "ses_ctx_1";
    storage
        .write_span(&ctx_span("c1", sid, R0, 100))
        .await
        .unwrap();
    storage
        .write_span(&ctx_span("c2", sid, R0 + 900_000_000, 300))
        .await
        .unwrap();
    storage
        .write_log(&ctx_log(
            R0 + 100_000_000,
            "claude_code.api_request",
            13,
            &[("session.id", sid), ("event.name", "api_request")],
        ))
        .await
        .unwrap();
    storage
        .write_log(&ctx_log(
            R0 + 200_000_000,
            "claude_code.tool_result",
            17,
            &[("session.id", sid)],
        ))
        .await
        .unwrap();
    // opencode metric for this session (gauge points)
    storage
        .write_metric(&agent_metric(
            "opencode.message.count",
            R0 + 300_000_000,
            Some(3),
            None,
            &[("session.id", sid), ("project.id", "proj-7")],
        ))
        .await
        .unwrap();

    let app = server.build_router();
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/sessions/{sid}/context"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let ctx: otelite_core::api::SessionContextResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(ctx.session.id, sid);
    // claude spans present → agent claude, full span coverage
    assert_eq!(ctx.session.agent.as_deref(), Some("claude"));
    assert_eq!(ctx.session.span_coverage, "full");
    // project.id leaks in from the opencode metric row — it is
    // scoped to opencode.* metric names only
    assert_eq!(ctx.session.project_id.as_deref(), Some("proj-7"));
    assert_eq!(ctx.spans_total, 2);
    assert_eq!(ctx.logs_total, 2);
    assert_eq!(ctx.spans[0].duration_ns, 100_000_000);
    assert_eq!(ctx.logs[0].severity.as_deref(), Some("WARN"));
    assert_eq!(ctx.logs[1].severity.as_deref(), Some("ERROR"));
    assert_eq!(ctx.metrics.len(), 1);
    assert_eq!(ctx.metrics[0].count, 1);
    assert_eq!(ctx.metrics[0].sum, Some(3.0));
    // timeline: 2 spans + 2 logs merged ascending
    assert_eq!(ctx.timeline.len(), 4);
    assert_eq!(ctx.timeline[0].kind, "span");
    assert_eq!(ctx.timeline[0].ts, R0);
    assert_eq!(
        ctx.timeline
            .iter()
            .find(|e| e.kind == "span")
            .unwrap()
            .label,
        "claude_code.llm_request claude-sonnet-5"
    );

    // limit caps rows, not totals
    let app = server.build_router();
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/sessions/{sid}/context?limit=1"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let ctx: otelite_core::api::SessionContextResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(ctx.spans.len(), 1);
    assert_eq!(ctx.spans_total, 2);
    assert_eq!(ctx.timeline.len(), 1);
}

#[tokio::test]
async fn test_get_session_context_window_filter() {
    let (server, storage, _temp_dir) = setup_test_server().await;
    let sid = "ses_ctx_2";
    storage
        .write_span(&ctx_span("c1", sid, R0, 100))
        .await
        .unwrap();
    storage
        .write_span(&ctx_span("c2", sid, R0 + 900_000_000, 300))
        .await
        .unwrap();

    let app = server.build_router();
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/sessions/{sid}/context?start_time={}",
                    R0 + 500_000_000
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let ctx: otelite_core::api::SessionContextResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        ctx.spans.len(),
        1,
        "only the later span starts in the window"
    );
    assert_eq!(
        ctx.spans_total, 1,
        "totals count the queried scope (window, when given)"
    );
}

/// Unwrap the `GenAiItemsResponse` envelope (#135) into `items`.
fn items<T: serde::de::DeserializeOwned>(body: &[u8], label: &str) -> T {
    let v: serde_json::Value = serde_json::from_slice(body).unwrap_or_else(|e| {
        panic!(
            "parse {label} response: {e}: {}",
            String::from_utf8_lossy(body)
        )
    });
    serde_json::from_value(
        v.get("items")
            .cloned()
            .unwrap_or_else(|| panic!("missing items in {label} response: {v}")),
    )
    .unwrap_or_else(|e| panic!("parse {label} items: {e}"))
}

// ── #135: global filter bar contract ────────────────────────────────────────
//
// Every genai endpoint accepts the five filter-bar params, applies the subset
// it supports, and echoes `filters_applied`. Unsupported params are ignored
// (never a 400).

async fn get_json(app: &axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value =
        serde_json::from_slice(&body).unwrap_or_else(|e| panic!("parse {uri}: {e}"));
    (status, v)
}

#[tokio::test]
async fn test_filters_applied_echoed_on_all_genai_endpoints() {
    let (server, _storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    // Struct-response endpoints: filters_applied is a field on the body.
    // Rollup endpoints require an explicit window (codex rollup).
    let w = format!("start_time={R0}&end_time={R1}");
    let struct_endpoints: Vec<String> = [
        "/api/genai/usage?agent=claude",
        "/api/genai/latency_percentiles",
        "/api/genai/capabilities",
        "/api/genai/retry_stats",
        "/api/genai/retries",
        "/api/genai/retrieval_stats",
        "/api/genai/request_param_profile",
        "/api/genai/conversation_depth",
        "/api/genai/tool_approvals",
        &format!("/api/genai/agents?{w}"),
        &format!("/api/genai/projects?{w}"),
        &format!("/api/genai/agent_roles?{w}"),
        &format!("/api/genai/provider_mix?{w}"),
        &format!("/api/genai/reasoning_share?{w}"),
        "/api/genai/pricing_metadata",
        "/api/genai/tool_failure_rates",
        "/api/genai/daily_tool_mix",
        "/api/genai/skill_activity",
        "/api/genai/session_quality_summary",
        "/api/genai/skill_outcomes",
        "/api/genai/model_selection_heatmap",
        "/api/genai/recent_errors",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for uri in &struct_endpoints {
        let (status, v) = get_json(&app, uri).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{uri} -> {status}: {}",
            String::from_utf8_lossy(
                &axum::body::to_bytes(
                    app.clone()
                        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                        .await
                        .unwrap()
                        .into_body(),
                    usize::MAX,
                )
                .await
                .unwrap()
            )
        );
        let applied = v.get("filters_applied").and_then(|x| x.as_array());
        assert!(applied.is_some(), "{uri} missing filters_applied: {v}");
    }

    // Wrapped array endpoints: { items, filters_applied }.
    let wrapped_endpoints = [
        "/api/genai/cost_series",
        "/api/genai/top_spans",
        "/api/genai/top_sessions",
        "/api/genai/top_conversations",
        "/api/genai/finish_reasons",
        "/api/genai/latency_stats",
        "/api/genai/error_rate",
        "/api/genai/tool_usage",
        "/api/genai/truncation_rate",
        "/api/genai/latency_series",
        "/api/genai/calls_series",
        "/api/genai/latency_by_context",
        "/api/genai/error_types",
        "/api/genai/model_drift",
        "/api/genai/stop_reasons",
        "/api/genai/context_type_split",
        "/api/genai/tool_errors",
        "/api/genai/hour_of_day",
        "/api/genai/agent_framework_defs",
        "/api/genai/cache_hit_rate",
    ];
    for uri in wrapped_endpoints {
        let (status, v) = get_json(&app, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert!(v.get("items").is_some(), "{uri} missing items: {v}");
        let applied = v.get("filters_applied").and_then(|x| x.as_array());
        assert!(applied.is_some(), "{uri} missing filters_applied: {v}");
    }

    // Sessions list + costs
    for uri in ["/api/sessions", "/api/sessions/costs"] {
        let (status, v) = get_json(&app, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert!(
            v.get("filters_applied")
                .and_then(|x| x.as_array())
                .is_some(),
            "{uri} missing filters_applied: {v}"
        );
    }
}

#[tokio::test]
async fn test_filter_params_accepted_never_400() {
    let (server, _storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    // All five params at once, including an unknown agent family: 200, and
    // the unknown family is not echoed as applied.
    let uri = "/api/genai/top_spans?agent=made-up-agent&model=x&provider=y&project=p&session=s";
    let (status, v) = get_json(&app, uri).await;
    assert_eq!(status, StatusCode::OK, "unknown agent must not 400");
    let applied: Vec<String> = v["filters_applied"]
        .as_array()
        .cloned()
        .unwrap()
        .into_iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert!(!applied.iter().any(|d| d == "agent"), "{applied:?}");
    for dim in ["model", "provider", "project", "session"] {
        assert!(applied.iter().any(|d| d == dim), "{applied:?}");
    }
}

#[tokio::test]
async fn test_session_filter_scopes_results() {
    let (server, storage, _temp_dir) = setup_test_server().await;

    // Two claude-family sessions. The session list keys interactions on
    // spans carrying `gen_ai.*` attributes (see root_llm_span), so both spans
    // need one.
    let mut span_a = llm_request_span("a1", "modelA", R0, 100, None);
    span_a
        .attributes
        .insert("gen_ai.request.model".to_string(), "modelA".to_string());
    storage.write_span(&span_a).await.unwrap();
    let mut span_b = llm_request_span("b1", "modelB", R0 + 200_000_000, 200, None);
    span_b
        .attributes
        .insert("session.id".to_string(), "s2".to_string());
    span_b
        .attributes
        .insert("gen_ai.request.model".to_string(), "modelB".to_string());
    storage.write_span(&span_b).await.unwrap();

    let app = server.build_router();

    // Unfiltered: both sessions in the list.
    let (status, v) = get_json(
        &app,
        &format!("/api/sessions?start_time={R0}&end_time={R1}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["sessions"].as_array().unwrap().len(), 2, "{v}");

    // Filtered: only session s1.
    let (status, v) = get_json(
        &app,
        &format!("/api/sessions?start_time={R0}&end_time={R1}&session=s1"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let sessions = v["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1, "{v}");
    assert_eq!(sessions[0]["session_id"], "s1");
    let applied: Vec<String> = v["filters_applied"]
        .as_array()
        .cloned()
        .unwrap()
        .into_iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert!(applied.iter().any(|d| d == "session"), "{applied:?}");
}

#[tokio::test]
async fn test_model_filter_scopes_top_spans() {
    let (server, storage, _temp_dir) = setup_test_server().await;

    storage
        .write_span(&llm_request_span("sp-a", "modelA", R0, 100, None))
        .await
        .unwrap();
    storage
        .write_span(&llm_request_span(
            "sp-b",
            "modelB",
            R0 + 500_000_000,
            200,
            None,
        ))
        .await
        .unwrap();

    let app = server.build_router();

    let (status, v) = get_json(
        &app,
        &format!("/api/genai/top_spans?start_time={R0}&end_time={R1}&model=modelA"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{v}");
    assert_eq!(items[0]["span_id"], "sp-a");
    let applied: Vec<String> = v["filters_applied"]
        .as_array()
        .cloned()
        .unwrap()
        .into_iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert!(applied.iter().any(|d| d == "model"), "{applied:?}");
}

/// Issue #139: the default branch returns a `Vec`, i.e. a JSON array. #135
/// merged `filters_applied` into the payload expecting an object, so every
/// request 500'd. It must now use the standard { items, filters_applied }
/// envelope, and the by_model=1 economics branch must stay an object.
#[tokio::test]
async fn test_cache_hit_rate_envelope_and_filters() {
    let (server, storage, _temp_dir) = setup_test_server().await;

    // One llm_span_guard cohort span with input + cache-read tokens.
    let mut span = llm_request_span("ch-1", "modelA", R0, 100, None);
    span.attributes
        .insert("gen_ai.system".to_string(), "anthropic".to_string());
    span.attributes
        .insert("gen_ai.usage.input_tokens".to_string(), "100".to_string());
    span.attributes.insert(
        "gen_ai.usage.cache_read.input_tokens".to_string(),
        "40".to_string(),
    );
    storage.write_span(&span).await.unwrap();

    let app = server.build_router();

    // Default branch: envelope, not a 500.
    let (status, v) = get_json(
        &app,
        &format!("/api/genai/cache_hit_rate?start_time={R0}&end_time={R1}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{v}");
    // Identity is `provider/model` when a provider is recorded (#143).
    assert_eq!(items[0]["model"], "anthropic/modelA");
    assert_eq!(items[0]["total_input_tokens"], 100);
    assert_eq!(items[0]["total_cache_read_tokens"], 40);
    // hit_rate = 40 / (40 + 100)
    let rate = items[0]["hit_rate"].as_f64().unwrap();
    assert!((rate - 40.0 / 140.0).abs() < 1e-12);
    assert_eq!(v["filters_applied"], serde_json::json!([]));

    // Model filter applies and is echoed.
    let (status, v) = get_json(
        &app,
        &format!("/api/genai/cache_hit_rate?start_time={R0}&end_time={R1}&model=modelA"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["items"].as_array().unwrap().len(), 1, "{v}");
    let applied: Vec<String> = v["filters_applied"]
        .as_array()
        .cloned()
        .unwrap()
        .into_iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert!(applied.iter().any(|d| d == "model"), "{applied:?}");

    // by_model=1: economics object — no items key — with an empty echo.
    let (status, v) = get_json(
        &app,
        &format!("/api/genai/cache_hit_rate?start_time={R0}&end_time={R1}&by_model=1"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert!(v.get("items").is_none(), "{v}");
    assert_eq!(v["filters_applied"], serde_json::json!([]));
}

/// Issue #119/#140: latency stats and percentile series expose the
/// per-call throughput triple (p10/p50/p90 tok/s) with a sample count
/// distinct from the total call count, plus the lower-tail duration p10.
#[tokio::test]
async fn test_latency_throughput_fields_end_to_end() {
    let (server, storage, _temp_dir) = setup_test_server().await;

    // Three 100 ms calls at 10, 100 and 1000 tok/s (1/10/100 output
    // tokens), spaced 1 ms apart so every span fits the 1 s test window.
    for (i, tokens) in [1i64, 10, 100].into_iter().enumerate() {
        let mut span = llm_request_span(
            &format!("tp-{i}"),
            "modelA",
            R0 + i as i64 * 1_000_000,
            100,
            None,
        );
        span.attributes
            .insert("gen_ai.usage.output_tokens".to_string(), tokens.to_string());
        storage.write_span(&span).await.unwrap();
    }
    // A duration-only call: in `count`, not in the throughput sample.
    storage
        .write_span(&llm_request_span(
            "tp-nout",
            "modelA",
            R0 + 3_000_000,
            100,
            None,
        ))
        .await
        .unwrap();

    let app = server.build_router();

    // Aggregate stats.
    let (status, v) = get_json(
        &app,
        &format!("/api/genai/latency_stats?start_time={R0}&end_time={R1}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let row = v["items"].as_array().unwrap()[0].clone();
    assert_eq!(row["count"], 4);
    assert_eq!(row["throughput_sample_count"], 3);
    assert_eq!(row["derived_tokens_per_sec_p10"], 10.0);
    assert_eq!(row["derived_tokens_per_sec_p50"], 100.0);
    assert_eq!(row["derived_tokens_per_sec_p90"], 1000.0);
    // p95/p99 retained during the compatibility period.
    assert_eq!(row["derived_tokens_per_sec_p95"], 1000.0);
    assert_eq!(row["derived_tokens_per_sec_p99"], 1000.0);

    // Percentile series: lower-tail p10 + throughput triple per bucket.
    let (status, v) = get_json(
        &app,
        &format!(
            "/api/genai/latency_percentiles?start_time={R0}&end_time={R1}&bucket_secs=3600&metrics=duration"
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let point = v["metrics"]["duration"]["all"].as_array().unwrap()[0].clone();
    assert_eq!(point["count"], 4);
    assert_eq!(point["throughput_sample_count"], 3);
    assert_eq!(point["throughput_p10_tok_s"], 10.0);
    assert_eq!(point["throughput_p50_tok_s"], 100.0);
    assert_eq!(point["throughput_p90_tok_s"], 1000.0);
    assert!(point["p10_ms"].is_number());
}

/// Issue #119/#141: calendar-day bucketing is explicit (calendar_day +
/// timezone), emits explicit bucket end timestamps, and includes empty
/// days with null percentiles. Invalid parameters are 400s, not 500s.
#[tokio::test]
async fn test_latency_percentiles_calendar_day() {
    let (server, storage, _temp_dir) = setup_test_server().await;
    let day: i64 = 86_400_000_000_000;
    // R0 is not on a UTC midnight; floor to one so the window is exactly
    // three full calendar days.
    let d0 = (R0 / day) * day;

    // One call on day 0, one on day 1; day 2 stays empty.
    storage
        .write_span(&llm_request_span(
            "cd-0",
            "modelA",
            d0 + 3_600_000_000_000,
            100,
            None,
        ))
        .await
        .unwrap();
    storage
        .write_span(&llm_request_span(
            "cd-1",
            "modelA",
            d0 + day + 3_600_000_000_000,
            100,
            None,
        ))
        .await
        .unwrap();

    let app = server.build_router();

    let (status, v) = get_json(
        &app,
        &format!(
            "/api/genai/latency_percentiles?start_time={d0}&end_time={}&calendar_day=1&timezone=UTC&metrics=duration",
            d0 + 3 * day
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let points = v["metrics"]["duration"]["all"].as_array().unwrap();
    assert_eq!(points.len(), 3, "three calendar days, empty one included");
    assert_eq!(points[0]["count"], 1);
    assert_eq!(points[1]["count"], 1);
    assert_eq!(points[2]["count"], 0);
    assert!(points[2]["p50_ms"].is_null(), "empty day: null percentiles");
    for (i, p) in points.iter().enumerate() {
        assert_eq!(p["ts"], d0 + i as i64 * day);
        assert_eq!(
            p["end_ts"],
            d0 + (i + 1) as i64 * day,
            "explicit bucket end timestamp"
        );
    }

    // Invalid parameters → 400, with a message.
    for (name, url) in [
        (
            "bad timezone",
            &format!(
                "/api/genai/latency_percentiles?start_time={R0}&end_time={}&calendar_day=1&timezone=Not/AZone",
                R0 + day
            ),
        ),
        (
            "timezone without calendar_day",
            &format!(
                "/api/genai/latency_percentiles?start_time={R0}&end_time={}&timezone=UTC",
                R0 + day
            ),
        ),
        (
            "calendar_day without window",
            &String::from("/api/genai/latency_percentiles?calendar_day=1&timezone=UTC"),
        ),
        (
            "bogus calendar_day value",
            &format!(
                "/api/genai/latency_percentiles?start_time={R0}&end_time={}&calendar_day=maybe",
                R0 + day
            ),
        ),
    ] {
        let (status, v) = get_json(&app, url).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name}: {v}");
    }

    // Rolling mode is unaffected: no empty buckets, no 400s, points carry
    // end_ts one bucket width past the start.
    let (status, v) = get_json(
        &app,
        &format!(
            "/api/genai/latency_percentiles?start_time={d0}&end_time={}&bucket_secs=3600&metrics=duration",
            d0 + 2 * day
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let points = v["metrics"]["duration"]["all"].as_array().unwrap();
    assert_eq!(points.len(), 2, "rolling: only non-empty buckets");
    for p in points {
        assert_eq!(p["end_ts"], p["ts"].as_i64().unwrap() + 3_600_000_000_000);
    }
}

// ── Daily Tool Mix: tokens + cost (#179) ──────────────────────────────────────

fn daily_mix_span(span_id: &str, scope: &str, model: &str, start: i64, tokens: (u64, u64)) -> Span {
    let (in_t, out_t) = tokens;
    let mut attributes: HashMap<String, String> = HashMap::new();
    attributes.insert("otel.scope.name".to_string(), scope.to_string());
    attributes.insert("gen_ai.system".to_string(), "anthropic".to_string());
    attributes.insert("gen_ai.request.model".to_string(), model.to_string());
    attributes.insert("gen_ai.usage.input_tokens".to_string(), in_t.to_string());
    attributes.insert("gen_ai.usage.output_tokens".to_string(), out_t.to_string());
    Span {
        trace_id: "t-daily-mix".to_string(),
        span_id: span_id.to_string(),
        parent_span_id: None,
        name: "llm_request".to_string(),
        kind: SpanKind::Internal,
        start_time: start,
        end_time: start + 1_000_000,
        attributes,
        status: SpanStatus {
            code: SpanStatusCode::Ok,
            message: None,
        },
        events: Vec::new(),
        resource: None,
    }
}

#[tokio::test]
async fn test_daily_tool_mix_tokens_and_cost() {
    let (server, storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    // Empty state: no rows, no model breakdown.
    let (status, v) = get_json(&app, "/api/genai/daily_tool_mix").await;
    assert_eq!(status, StatusCode::OK);
    assert!(v["rows"].as_array().unwrap().is_empty());
    assert!(v["model_rows"].as_array().unwrap().is_empty());

    // Two LLM spans, same day + tool, different models.
    // Day = 2026-01-01 (UTC).
    let d1 = 1_767_225_600_000_000_000_i64;
    let s1 = daily_mix_span(
        "s1",
        "com.anthropic.claude_code",
        "claude-sonnet-5",
        d1 + 1_000_000,
        (100, 50),
    );
    let s2 = daily_mix_span(
        "s2",
        "com.anthropic.claude_code",
        "claude-opus-5",
        d1 + 2_000_000,
        (200, 80),
    );
    // A second day + tool whose model has no pricing data (not a Claude
    // family name, and no such model exists in any pricing source) — its
    // row must carry a null cost, never a fabricated one.
    let s3 = daily_mix_span(
        "s3",
        "pi-otel",
        "unknown-vendor-model-9.9",
        d1 + 86_400 * 1_000_000_000 + 1_000_000,
        (5, 6),
    );
    storage.write_span_batch(&[s1, s2, s3]).await.unwrap();

    // Windowed query — a different cache key than the empty-state call
    // above (the GenAI bucket caches responses; a same-key repeat would
    // serve the pre-write empty response).
    let (status, v) = get_json(
        &app,
        &format!(
            "/api/genai/daily_tool_mix?start_time={d1}&end_time={}",
            d1 + 2 * 86_400 * 1_000_000_000
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // (day, tool) rows: merged token totals, no metric datapoints.
    let rows = v["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{v}");
    assert_eq!(rows[0]["day"], "2026-01-01");
    assert_eq!(rows[0]["tool"], "claude_code");
    assert_eq!(rows[0]["datapoints"], 0);
    assert_eq!(rows[0]["input_tokens"], 300);
    assert_eq!(rows[0]["output_tokens"], 130);
    assert_eq!(rows[0]["cache_read_tokens"], 0);
    // Claude family models price via the deterministic fallback table.
    let cost = rows[0]["total_cost_usd"].as_f64();
    assert!(
        cost.is_some() && cost.unwrap() > 0.0,
        "expected priced cost: {v}"
    );

    // No pricing data for granite-4.0 -> null, not a fabricated zero.
    assert_eq!(rows[1]["day"], "2026-01-02");
    assert_eq!(rows[1]["tool"], "pi");
    assert_eq!(rows[1]["input_tokens"], 5);
    assert_eq!(rows[1]["output_tokens"], 6);
    assert!(rows[1]["total_cost_usd"].is_null(), "{v}");

    // Per-model breakdown (day asc, tool asc, model asc): the API's pricing input.
    let mr = v["model_rows"].as_array().unwrap();
    assert_eq!(mr.len(), 3, "{v}");
    assert_eq!(mr[0]["model"], "claude-opus-5");
    assert_eq!(mr[0]["input_tokens"], 200);
    assert_eq!(mr[0]["output_tokens"], 80);
    assert_eq!(mr[0]["cache_creation_tokens"], 0);
    assert_eq!(mr[1]["model"], "claude-sonnet-5");
    assert_eq!(mr[1]["input_tokens"], 100);
    assert_eq!(mr[1]["output_tokens"], 50);
    assert_eq!(mr[2]["day"], "2026-01-02");
    assert_eq!(mr[2]["model"], "unknown-vendor-model-9.9");
    assert_eq!(mr[2]["input_tokens"], 5);
    assert_eq!(mr[2]["output_tokens"], 6);

    assert_eq!(v["tools"][0], "claude_code");
    assert_eq!(v["tools"][1], "pi");
}

// ── Productivity: per-day counters + cost join (#177) ────────────────────────

#[tokio::test]
async fn test_productivity_rows_and_cost() {
    let (server, storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    // Empty state.
    let (status, v) = get_json(&app, "/api/genai/productivity").await;
    assert_eq!(status, StatusCode::OK);
    assert!(v["rows"].as_array().unwrap().is_empty());

    // Day 1 = 2026-01-01 (UTC), day 2 = 2026-01-02.
    let d1 = 1_767_225_600_000_000_000_i64;
    let d2 = d1 + 86_400 * 1_000_000_000;

    // claude_code.commit.count: cumulative 2 on d1, 4 on d2 -> deltas 2, 2.
    storage
        .write_metric(&agent_metric(
            "claude_code.commit.count",
            d1 + 1_000_000,
            Some(2),
            None,
            &[],
        ))
        .await
        .unwrap();
    storage
        .write_metric(&agent_metric(
            "claude_code.commit.count",
            d2 + 1_000_000,
            Some(4),
            None,
            &[],
        ))
        .await
        .unwrap();
    // claude_code.pull_request.count: 1 on d1.
    storage
        .write_metric(&agent_metric(
            "claude_code.pull_request.count",
            d1 + 2_000_000,
            Some(1),
            None,
            &[],
        ))
        .await
        .unwrap();
    // claude_code.lines_of_code.count: added 10, removed 4 (both d1).
    storage
        .write_metric(&agent_metric(
            "claude_code.lines_of_code.count",
            d1 + 3_000_000,
            Some(10),
            None,
            &[("type", "added")],
        ))
        .await
        .unwrap();
    storage
        .write_metric(&agent_metric(
            "claude_code.lines_of_code.count",
            d1 + 4_000_000,
            Some(4),
            None,
            &[("type", "removed")],
        ))
        .await
        .unwrap();
    // opencode.lines_of_code.total: added 7 (d2).
    storage
        .write_metric(&agent_metric(
            "opencode.lines_of_code.total",
            d2 + 5_000_000,
            Some(7),
            None,
            &[("type", "added")],
        ))
        .await
        .unwrap();

    // LLM spans for the cost join: a priced Claude model on d1
    // (claude_code scope), an unpriced synthetic model on d2 (opencode
    // scope) — mirrors the #179 pricing-branch fixtures.
    let s1 = daily_mix_span(
        "p1",
        "com.anthropic.claude_code",
        "claude-sonnet-5",
        d1 + 10_000_000,
        (100, 50),
    );
    let s2 = daily_mix_span(
        "p2",
        "com.opencode",
        "unknown-vendor-model-9.9",
        d2 + 10_000_000,
        (5, 6),
    );
    storage.write_span_batch(&[s1, s2]).await.unwrap();

    // Windowed query — a different cache key than the empty-state call.
    let (status, v) = get_json(
        &app,
        &format!(
            "/api/genai/productivity?start_time={d1}&end_time={}",
            d2 + 86_400 * 1_000_000_000
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let rows = v["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 3, "{v}");

    // d1, claude_code: full counters + priced cost, cost-per-commit
    // derived as cost / commits.
    let r0 = &rows[0];
    assert_eq!(r0["day"], "2026-01-01");
    assert_eq!(r0["tool"], "claude_code");
    assert_eq!(r0["commits"], 2);
    assert_eq!(r0["prs"], 1);
    assert_eq!(r0["lines_added"], 10);
    assert_eq!(r0["lines_removed"], 4);
    let cost = r0["cost_usd"].as_f64();
    assert!(
        cost.is_some() && cost.unwrap() > 0.0,
        "claude family prices via the fallback table: {v}"
    );
    let per_commit = r0["cost_per_commit_usd"].as_f64();
    assert!(
        per_commit.is_some() && (per_commit.unwrap() - cost.unwrap() / 2.0).abs() < 1e-9,
        "cost_per_commit must be cost / commits: {v}"
    );

    // d2, claude_code: only the commit counter moved (delta 4 - 2 = 2);
    // no cost that day.
    let r1 = &rows[1];
    assert_eq!(r1["day"], "2026-01-02");
    assert_eq!(r1["tool"], "claude_code");
    assert_eq!(r1["commits"], 2);
    assert_eq!(r1["prs"], 0);
    assert_eq!(r1["lines_added"], 0);
    assert_eq!(r1["lines_removed"], 0);
    assert!(r1["cost_usd"].is_null(), "{v}");
    assert!(r1["cost_per_commit_usd"].is_null(), "{v}");

    // d2, opencode: LOC only — commits/PRs are 0, never missing; the
    // synthetic model has no pricing data -> null cost, not fabricated.
    let r2 = &rows[2];
    assert_eq!(r2["day"], "2026-01-02");
    assert_eq!(r2["tool"], "opencode");
    assert_eq!(r2["commits"], 0);
    assert_eq!(r2["prs"], 0);
    assert_eq!(r2["lines_added"], 7);
    assert_eq!(r2["lines_removed"], 0);
    assert!(r2["cost_usd"].is_null(), "{v}");
    assert!(r2["cost_per_commit_usd"].is_null(), "{v}");
}

// ── Cost projection: trailing-rate monthly projection (#170) ─────────────────

#[tokio::test]
async fn test_cost_projection_with_and_without_data() {
    let (server, storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    // Empty state: no priced spend -> all-zero projection.
    let (status, v) = get_json(&app, "/api/genai/cost_projection").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["avg_daily_7d"], 0.0);
    assert_eq!(v["avg_daily_30d"], 0.0);
    assert_eq!(v["projected_month_total"], 0.0);
    assert!(v["by_model"].as_array().unwrap().is_empty());

    // Three priced Claude-family spans within the trailing 7 days.
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        * 1_000_000_000;
    let day = 86_400 * 1_000_000_000;
    let s1 = daily_mix_span(
        "cp1",
        "com.anthropic.claude_code",
        "claude-sonnet-5",
        now_ns - day,
        (1000, 500),
    );
    let s2 = daily_mix_span(
        "cp2",
        "com.anthropic.claude_code",
        "claude-sonnet-5",
        now_ns - 2 * day,
        (1000, 500),
    );
    let s3 = daily_mix_span(
        "cp3",
        "com.anthropic.claude_code",
        "claude-sonnet-5",
        now_ns - 3 * day,
        (1000, 500),
    );
    storage.write_span_batch(&[s1, s2, s3]).await.unwrap();

    // end_time pinned to the same instant the spans were placed relative
    // to — and a different cache key than the empty-state call above.
    let (status, v) = get_json(
        &app,
        &format!("/api/genai/cost_projection?end_time={now_ns}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Claude family prices via the deterministic fallback table.
    assert!(v["avg_daily_7d"].as_f64().unwrap() > 0.0, "{v}");
    assert!(v["avg_daily_30d"].as_f64().unwrap() > 0.0, "{v}");
    assert!(v["projected_month_total"].as_f64().unwrap() > 0.0, "{v}");

    // days_remaining: full days after today in the current UTC month
    // (±1 tolerance for a midnight race between the test and the server).
    use chrono::Datelike;
    let now_date = chrono::Utc::now().date_naive();
    let expected_remaining = now_date.num_days_in_month() as u32 - now_date.day();
    let remaining = v["days_remaining"].as_u64().unwrap() as u32;
    assert!(
        (expected_remaining as i64 - remaining as i64).abs() <= 1,
        "expected ~{expected_remaining} days remaining, got {remaining}: {v}"
    );

    // by_model: the one priced model (the cost series carries the
    // composite provider/model identity), projected desc.
    let models = v["by_model"].as_array().unwrap();
    assert_eq!(models.len(), 1, "{v}");
    assert_eq!(models[0]["model"], "anthropic/claude-sonnet-5");
    assert!(models[0]["avg_daily"].as_f64().unwrap() > 0.0, "{v}");
    assert!(models[0]["projected"].as_f64().unwrap() > 0.0, "{v}");
}

// ── Cost by project × model × tool (#173) ────────────────────────────────────

/// LLM span with an optional `project.id` attribute (the daily_mix_span
/// helper predates #173 and cannot carry it).
// Six parameters mirror the six span fields the fixture varies — one per
// test assertion axis; restructuring them would obscure the call sites.
#[allow(clippy::too_many_arguments)]
fn cbp_span(
    span_id: &str,
    scope: &str,
    model: &str,
    start: i64,
    project_id: Option<&str>,
    tokens: (u64, u64),
) -> Span {
    let (in_t, out_t) = tokens;
    let mut attributes: HashMap<String, String> = HashMap::new();
    attributes.insert("otel.scope.name".to_string(), scope.to_string());
    attributes.insert("gen_ai.system".to_string(), "anthropic".to_string());
    attributes.insert("gen_ai.request.model".to_string(), model.to_string());
    attributes.insert("gen_ai.usage.input_tokens".to_string(), in_t.to_string());
    attributes.insert("gen_ai.usage.output_tokens".to_string(), out_t.to_string());
    if let Some(p) = project_id {
        attributes.insert("project.id".to_string(), p.to_string());
    }
    Span {
        trace_id: "t-cost-by-project".to_string(),
        span_id: span_id.to_string(),
        parent_span_id: None,
        name: "llm_request".to_string(),
        kind: SpanKind::Internal,
        start_time: start,
        end_time: start + 1_000_000,
        attributes,
        status: SpanStatus {
            code: SpanStatusCode::Ok,
            message: None,
        },
        events: Vec::new(),
        resource: None,
    }
}

#[tokio::test]
async fn test_cost_by_project_rows_and_pricing() {
    let (server, storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    // Empty state.
    let (status, v) = get_json(&app, "/api/genai/cost_by_project").await;
    assert_eq!(status, StatusCode::OK);
    assert!(v["rows"].as_array().unwrap().is_empty());

    // Day = 2026-01-01 (UTC).
    let d1 = 1_767_225_600_000_000_000_i64;

    let spans = vec![
        // opencode, project "api-server": two granite-4.0 spans...
        cbp_span(
            "c1",
            "com.opencode",
            "granite-4.0",
            d1 + 1_000_000,
            Some("api-server"),
            (10, 5),
        ),
        cbp_span(
            "c2",
            "com.opencode",
            "granite-4.0",
            d1 + 2_000_000,
            Some("api-server"),
            (20, 5),
        ),
        // ...and one granite-4.1 span.
        cbp_span(
            "c3",
            "com.opencode",
            "granite-4.1",
            d1 + 3_000_000,
            Some("api-server"),
            (1, 1),
        ),
        // opencode, second project "web-ui".
        cbp_span(
            "c4",
            "com.opencode",
            "granite-4.0",
            d1 + 4_000_000,
            Some("web-ui"),
            (7, 3),
        ),
        // claude_code: no project label -> unattributed, priced via the
        // deterministic Claude fallback table.
        cbp_span(
            "c5",
            "com.anthropic.claude_code",
            "claude-sonnet-5",
            d1 + 5_000_000,
            None,
            (100, 50),
        ),
        // opencode with an EMPTY project label -> also unattributed.
        cbp_span(
            "c6",
            "com.opencode",
            "granite-4.0",
            d1 + 6_000_000,
            Some(""),
            (2, 2),
        ),
    ];
    storage.write_span_batch(&spans).await.unwrap();

    // Windowed query — a different cache key than the empty-state call.
    let (status, v) = get_json(
        &app,
        &format!(
            "/api/genai/cost_by_project?start_time={d1}&end_time={}",
            d1 + 86_400 * 1_000_000_000
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // (project asc, tool asc, model asc).
    let rows = v["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 5, "{v}");
    let keys: Vec<(&str, &str, &str)> = rows
        .iter()
        .map(|r| {
            (
                r["project"].as_str().unwrap(),
                r["tool"].as_str().unwrap(),
                r["model"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        keys,
        vec![
            ("api-server", "opencode", "granite-4.0"),
            ("api-server", "opencode", "granite-4.1"),
            ("unattributed", "claude_code", "claude-sonnet-5"),
            ("unattributed", "opencode", "granite-4.0"),
            ("web-ui", "opencode", "granite-4.0"),
        ]
    );

    // Grouping: the two api-server granite-4.0 spans merge into one row.
    let r0 = &rows[0];
    assert_eq!(r0["requests"], 2);
    assert_eq!(r0["input_tokens"], 30);
    assert_eq!(r0["output_tokens"], 10);
    // granite-4.0 has no pricing data -> null cost, never a zero.
    assert!(r0["cost_usd"].is_null(), "{v}");

    // The unattributed Claude row prices via the fallback table.
    let r2 = &rows[2];
    assert_eq!(r2["requests"], 1);
    assert_eq!(r2["input_tokens"], 100);
    assert!(
        r2["cost_usd"].as_f64().unwrap() > 0.0,
        "claude family prices: {v}"
    );
    assert!(r2["cost_source"].is_string(), "{v}");

    // Empty-label opencode span lands in unattributed alongside it, split
    // by tool.
    let r3 = &rows[3];
    assert_eq!(r3["requests"], 1);
    assert_eq!(r3["input_tokens"], 2);
    assert!(r3["cost_usd"].is_null(), "{v}");
}

// ── Session depth vs cost (#180) ────────────────────────────────────────────

/// LLM span carrying a `session.id` (the daily_mix_span / cbp_span helpers
/// predate #180 and cannot carry it).
#[allow(clippy::too_many_arguments)]
fn sd_span(
    span_id: &str,
    scope: &str,
    model: &str,
    start: i64,
    session_id: Option<&str>,
    tokens: (u64, u64),
) -> Span {
    let (in_t, out_t) = tokens;
    let mut attributes: HashMap<String, String> = HashMap::new();
    attributes.insert("otel.scope.name".to_string(), scope.to_string());
    attributes.insert("gen_ai.system".to_string(), "anthropic".to_string());
    attributes.insert("gen_ai.request.model".to_string(), model.to_string());
    attributes.insert("gen_ai.usage.input_tokens".to_string(), in_t.to_string());
    attributes.insert("gen_ai.usage.output_tokens".to_string(), out_t.to_string());
    if let Some(s) = session_id {
        attributes.insert("session.id".to_string(), s.to_string());
    }
    Span {
        trace_id: "t-session-depth".to_string(),
        span_id: span_id.to_string(),
        parent_span_id: None,
        name: "llm_request".to_string(),
        kind: SpanKind::Internal,
        start_time: start,
        end_time: start + 1_000_000,
        attributes,
        status: SpanStatus {
            code: SpanStatusCode::Ok,
            message: None,
        },
        events: Vec::new(),
        resource: None,
    }
}

#[tokio::test]
async fn test_session_depth_cost_bucketing_and_pricing() {
    let (server, storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    // Empty state.
    let (status, v) = get_json(&app, "/api/genai/session_depth_cost").await;
    assert_eq!(status, StatusCode::OK);
    assert!(v["rows"].as_array().unwrap().is_empty());

    // Day = 2026-01-01 (UTC).
    let d1 = 1_767_225_600_000_000_000_i64;

    // Session s-a (opencode, priced Claude): 6 spans -> bucket "6-15".
    let mut spans: Vec<Span> = (0..6)
        .map(|i| {
            sd_span(
                &format!("sd-a{i}"),
                "com.opencode",
                "claude-sonnet-5",
                d1 + 1_000_000 + i * 1_000,
                Some("s-a"),
                (100, 50),
            )
        })
        .collect();
    // Session s-b (opencode, unpriced synthetic model): 3 spans ->
    // bucket "1-5"; counted in the bucket but carries no cost.
    spans.extend((0..3).map(|i| {
        sd_span(
            &format!("sd-b{i}"),
            "com.opencode",
            "unknown-vendor-model-9.9",
            d1 + 2_000_000 + i * 1_000,
            Some("s-b"),
            (10, 5),
        )
    }));
    // Session s-c (opencode, priced Claude): 2 spans -> bucket "1-5".
    spans.extend((0..2).map(|i| {
        sd_span(
            &format!("sd-c{i}"),
            "com.opencode",
            "claude-sonnet-5",
            d1 + 3_000_000 + i * 1_000,
            Some("s-c"),
            (10, 5),
        )
    }));
    // Session s-d (claude_code, priced Claude): a single span -> excluded
    // from the stats (not a multi-turn conversation).
    spans.push(sd_span(
        "sd-d0",
        "com.anthropic.claude_code",
        "claude-sonnet-5",
        d1 + 4_000_000,
        Some("s-d"),
        (50, 25),
    ));
    // No session.id: never part of the report.
    spans.push(sd_span(
        "sd-e0",
        "com.opencode",
        "claude-sonnet-5",
        d1 + 5_000_000,
        None,
        (10, 5),
    ));
    storage.write_span_batch(&spans).await.unwrap();

    // Windowed query — a different cache key than the empty-state call.
    let (status, v) = get_json(
        &app,
        &format!(
            "/api/genai/session_depth_cost?start_time={d1}&end_time={}",
            d1 + 86_400 * 1_000_000_000
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // (tool asc, bucket asc): opencode 1-5, opencode 6-15.
    // The single-turn claude_code session and the session-less span
    // contribute nothing.
    let rows = v["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{v}");
    let keys: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| (r["tool"].as_str().unwrap(), r["bucket"].as_str().unwrap()))
        .collect();
    assert_eq!(keys, vec![("opencode", "1-5"), ("opencode", "6-15")]);

    // opencode 1-5: two sessions (s-b unpriced + s-c priced), avg turns
    // (3 + 2) / 2 = 2.5. Cost stats come from the single priced session,
    // so median == p95 > 0.
    let r0 = &rows[0];
    assert_eq!(r0["sessions"], 2);
    assert!(
        (r0["avg_turns"].as_f64().unwrap() - 2.5).abs() < 1e-9,
        "{v}"
    );
    let m0 = r0["median_cost_usd"].as_f64().unwrap();
    assert!(m0 > 0.0, "priced session must carry a cost: {v}");
    let p0 = r0["p95_cost_usd"].as_f64().unwrap();
    assert!(
        (p0 - m0).abs() < 1e-9,
        "single priced session: p95 == median"
    );

    // opencode 6-15: one session, 6 turns, priced.
    let r1 = &rows[1];
    assert_eq!(r1["sessions"], 1);
    assert!(
        (r1["avg_turns"].as_f64().unwrap() - 6.0).abs() < 1e-9,
        "{v}"
    );
    assert!(r1["median_cost_usd"].as_f64().unwrap() > 0.0, "{v}");
    assert!(r1["p95_cost_usd"].as_f64().unwrap() > 0.0, "{v}");
}

// ── Time in tool (#172) ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_time_in_tool_gaps_and_validation() {
    let (server, storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    // Empty state.
    let (status, v) = get_json(&app, "/api/genai/time_in_tool").await;
    assert_eq!(status, StatusCode::OK);
    assert!(v["rows"].as_array().unwrap().is_empty());

    // max_gap_secs validation: zero and above the 86400 s limit are 400.
    let (status, _) = get_json(&app, "/api/genai/time_in_tool?max_gap_secs=0").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = get_json(&app, "/api/genai/time_in_tool?max_gap_secs=86401").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Day = 2026-01-01 (UTC).
    let d1 = 1_767_225_600_000_000_000_i64;
    let sec = 1_000_000_000_i64;
    let base = d1 + 3600 * sec;

    // s-x (opencode): gaps of 240 s (counts) and 360 s (context switch,
    // zero) -> 240 s = 4.0 minutes.
    let spans = vec![
        sd_span(
            "tt-x1",
            "com.opencode",
            "test-model",
            base,
            Some("s-x"),
            (1, 1),
        ),
        sd_span(
            "tt-x2",
            "com.opencode",
            "test-model",
            base + 240 * sec,
            Some("s-x"),
            (1, 1),
        ),
        sd_span(
            "tt-x3",
            "com.opencode",
            "test-model",
            base + 600 * sec,
            Some("s-x"),
            (1, 1),
        ),
        // s-y (claude_code): a single 120 s gap -> 2.0 minutes.
        sd_span(
            "tt-y1",
            "com.anthropic.claude_code",
            "test-model",
            base + 10 * sec,
            Some("s-y"),
            (1, 1),
        ),
        sd_span(
            "tt-y2",
            "com.anthropic.claude_code",
            "test-model",
            base + 130 * sec,
            Some("s-y"),
            (1, 1),
        ),
    ];
    storage.write_span_batch(&spans).await.unwrap();

    // Windowed query (default 300 s ceiling) — a different cache key
    // than the empty-state call.
    let (status, v) = get_json(
        &app,
        &format!(
            "/api/genai/time_in_tool?start_time={d1}&end_time={}",
            d1 + 86_400 * sec
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // (date asc, tool asc): claude_code then opencode, both on day 1.
    let rows = v["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{v}");
    let keys: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| (r["tool"].as_str().unwrap(), r["date"].as_str().unwrap()))
        .collect();
    assert_eq!(
        keys,
        vec![("claude_code", "2026-01-01"), ("opencode", "2026-01-01"),]
    );
    assert_eq!(rows[0]["sessions"], 1);
    assert!(
        (rows[0]["active_minutes"].as_f64().unwrap() - 2.0).abs() < 1e-9,
        "{v}"
    );
    assert_eq!(rows[1]["sessions"], 1);
    // 240 s gap counts, the 360 s gap is a context switch.
    assert!(
        (rows[1]["active_minutes"].as_f64().unwrap() - 4.0).abs() < 1e-9,
        "{v}"
    );
    assert_eq!(v["tools"], serde_json::json!(["claude_code", "opencode"]));

    // A 100 s ceiling drops both the 240 s and 360 s gaps and the 120 s
    // gap: sessions still count, active minutes go to zero.
    let (status, v) = get_json(
        &app,
        &format!(
            "/api/genai/time_in_tool?start_time={d1}&end_time={}&max_gap_secs=100",
            d1 + 86_400 * sec
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let rows = v["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{v}");
    for r in rows {
        assert_eq!(r["active_minutes"], 0.0, "{v}");
        assert_eq!(
            r["sessions"], 1,
            "sessions count regardless of the ceiling: {v}"
        );
    }
}

// ── Bob hook overhead (#167) ─────────────────────────────────────────────────

#[tokio::test]
async fn test_bob_hook_overhead_scoped_to_bob_metric() {
    let (server, storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    // Empty state: Bob emits no hook telemetry yet.
    let (status, v) = get_json(&app, "/api/genai/bob_hook_overhead").await;
    assert_eq!(status, StatusCode::OK);
    assert!(v["rows"].as_array().unwrap().is_empty());
    assert_eq!(v["grand_total_ms"], 0.0);

    // A Codex hook row must NOT appear in the Bob view.
    storage
        .write_metric(&agent_metric(
            "codex.hooks.run.duration_ms",
            R0,
            None,
            Some((3, 450.0)),
            &[("hook_name", "PreToolUse")],
        ))
        .await
        .unwrap();
    // Two Bob hook rows: PrePrompt (2 calls / 400 ms) and Stop
    // (5 calls / 2500 ms).
    storage
        .write_metric(&agent_metric(
            "bob.hooks.run.duration_ms",
            R0,
            None,
            Some((2, 400.0)),
            &[("hook_name", "PrePrompt")],
        ))
        .await
        .unwrap();
    storage
        .write_metric(&agent_metric(
            "bob.hooks.run.duration_ms",
            R1,
            None,
            Some((5, 2500.0)),
            &[("hook_name", "Stop")],
        ))
        .await
        .unwrap();

    // Windowed query — a different cache key than the empty-state call.
    let (status, v) = get_json(
        &app,
        &format!("/api/genai/bob_hook_overhead?start_time={R0}&end_time={R1}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let rows = v["rows"].as_array().unwrap();
    // Ordered by total_ms desc: Stop then PrePrompt; the Codex row is
    // scoped out.
    assert_eq!(rows.len(), 2, "{v}");
    assert_eq!(rows[0]["event"], "Stop");
    assert_eq!(rows[0]["count"], 5);
    assert!(
        (rows[0]["total_ms"].as_f64().unwrap() - 2500.0).abs() < 0.01,
        "{v}"
    );
    assert!(
        (rows[0]["avg_ms"].as_f64().unwrap() - 500.0).abs() < 0.01,
        "{v}"
    );
    assert_eq!(rows[1]["event"], "PrePrompt");
    assert_eq!(rows[1]["count"], 2);
    assert!(
        (rows[1]["avg_ms"].as_f64().unwrap() - 200.0).abs() < 0.01,
        "{v}"
    );
    // Grand total sums the Bob rows only.
    assert!(
        (v["grand_total_ms"].as_f64().unwrap() - 2900.0).abs() < 0.01,
        "{v}"
    );
}

// ── Tool switch overhead (#166) ─────────────────────────────────────────────

/// LLM span with session.id and an optional OTel TTFT attribute (seconds,
/// the wire form the normaliser accepts).
fn tsw_span(span_id: &str, scope: &str, start: i64, session: &str, ttft_secs: Option<f64>) -> Span {
    let mut attributes: HashMap<String, String> = HashMap::new();
    attributes.insert("otel.scope.name".to_string(), scope.to_string());
    attributes.insert("gen_ai.system".to_string(), "anthropic".to_string());
    attributes.insert("gen_ai.request.model".to_string(), "test-model".to_string());
    attributes.insert("gen_ai.usage.input_tokens".to_string(), "1".to_string());
    attributes.insert("gen_ai.usage.output_tokens".to_string(), "1".to_string());
    attributes.insert("session.id".to_string(), session.to_string());
    if let Some(t) = ttft_secs {
        attributes.insert(
            "gen_ai.server.time_to_first_token".to_string(),
            t.to_string(),
        );
    }
    Span {
        trace_id: "t-tool-switch".to_string(),
        span_id: span_id.to_string(),
        parent_span_id: None,
        name: "llm_request".to_string(),
        kind: SpanKind::Internal,
        start_time: start,
        end_time: start + 1_000_000,
        attributes,
        status: SpanStatus {
            code: SpanStatusCode::Ok,
            message: None,
        },
        events: Vec::new(),
        resource: None,
    }
}

#[tokio::test]
async fn test_tool_switch_overhead_detects_boundaries() {
    let (server, storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    // Empty state: no switches.
    let (status, v) = get_json(&app, "/api/genai/tool_switch_overhead").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["switches"], 0);
    assert!(v["by_transition"].as_array().unwrap().is_empty());

    let d1 = 1_767_225_600_000_000_000_i64;
    let ms = 1_000_000_i64;
    // Session s1: opencode (warm continuation), then a switch to codex.
    // Cold codex TTFT 0.8 s, warm opencode TTFT 0.1 s.
    let spans = vec![
        tsw_span("tsw-1", "com.opencode", d1, "s1", Some(0.1)),
        tsw_span("tsw-2", "com.opencode", d1 + 500 * ms, "s1", Some(0.1)),
        tsw_span("tsw-3", "codex_exec", d1 + 5_500 * ms, "s1", Some(0.8)),
        // Session s2: a single-tool session (no boundary).
        tsw_span("tsw-4", "com.opencode", d1, "s2", None),
        tsw_span("tsw-5", "com.opencode", d1 + 100 * ms, "s2", None),
    ];
    storage.write_span_batch(&spans).await.unwrap();

    // Windowed query — a different cache key than the empty-state call.
    let (status, v) = get_json(
        &app,
        &format!(
            "/api/genai/tool_switch_overhead?start_time={d1}&end_time={}",
            d1 + 86_400_000_000_000
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Exactly one boundary (opencode -> codex in s1).
    assert_eq!(v["switches"], 1, "{v}");
    // Gap = 5500 - 500 = 5000 ms.
    assert!(
        (v["avg_gap_ms"].as_f64().unwrap() - 5000.0).abs() < 1e-9,
        "{v}"
    );
    // Cold = the boundary span (0.8 s -> 800 ms); warm = same-tool
    // continuations: s1 opencode@500ms (0.1 s) and s2 opencode@100ms —
    // s2 spans carry no TTFT, so warm = [100.0].
    assert!(
        (v["avg_ttft_cold_ms"].as_f64().unwrap() - 800.0).abs() < 1e-9,
        "{v}"
    );
    assert!(
        (v["avg_ttft_warm_ms"].as_f64().unwrap() - 100.0).abs() < 1e-9,
        "{v}"
    );
    assert!(
        (v["overhead_ratio"].as_f64().unwrap() - 8.0).abs() < 1e-9,
        "{v}"
    );

    let t = &v["by_transition"][0];
    assert_eq!(t["from"], "opencode");
    assert_eq!(t["to"], "codex");
    assert_eq!(t["count"], 1);
    assert!(
        (t["avg_gap_ms"].as_f64().unwrap() - 5000.0).abs() < 1e-9,
        "{v}"
    );
    // The destination (codex) has no warm baseline, so the delta is
    // unmeasured — null, not zero.
    assert!(t["avg_ttft_delta_ms"].is_null(), "{v}");
}

// ── Session chains (#165) ───────────────────────────────────────────────────

/// LLM span with session.id and controlled model/tokens for the
/// session-chain fixtures. Six parameters mirror the span fields the
/// fixture varies — one per test assertion axis.
#[allow(clippy::too_many_arguments)]
fn sch_span(
    span_id: &str,
    scope: &str,
    model: &str,
    start: i64,
    session: &str,
    tokens: (u64, u64),
) -> Span {
    let (in_t, out_t) = tokens;
    let mut attributes: HashMap<String, String> = HashMap::new();
    attributes.insert("otel.scope.name".to_string(), scope.to_string());
    attributes.insert("gen_ai.system".to_string(), "anthropic".to_string());
    attributes.insert("gen_ai.request.model".to_string(), model.to_string());
    attributes.insert("gen_ai.usage.input_tokens".to_string(), in_t.to_string());
    attributes.insert("gen_ai.usage.output_tokens".to_string(), out_t.to_string());
    attributes.insert("session.id".to_string(), session.to_string());
    Span {
        trace_id: "t-session-chains".to_string(),
        span_id: span_id.to_string(),
        parent_span_id: None,
        name: "llm_request".to_string(),
        kind: SpanKind::Internal,
        start_time: start,
        end_time: start + 1_000_000,
        attributes,
        status: SpanStatus {
            code: SpanStatusCode::Ok,
            message: None,
        },
        events: Vec::new(),
        resource: None,
    }
}

#[tokio::test]
async fn test_session_chains_links_resumed_bursts() {
    let (server, storage, _temp_dir) = setup_test_server().await;
    let app = server.build_router();

    // Empty state.
    let (status, v) = get_json(&app, "/api/sessions/chains").await;
    assert_eq!(status, StatusCode::OK);
    assert!(v["chains"].as_array().unwrap().is_empty());

    // Validation: a zero window is rejected.
    let (status, _) = get_json(&app, "/api/sessions/chains?chain_window_secs=0").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let d1 = 1_767_225_600_000_000_000_i64;
    let sec = 1_000_000_000_i64;
    // ch-1 (opencode, priced Claude): two bursts under one UUID — the
    // --continue pattern. Burst 1: two spans 0.5 s apart; burst 2: one
    // span 3 h later (> the default 2 h window).
    let spans = vec![
        sch_span(
            "sch-1",
            "com.opencode",
            "claude-sonnet-5",
            d1,
            "ch-1",
            (10, 5),
        ),
        sch_span(
            "sch-2",
            "com.opencode",
            "claude-sonnet-5",
            d1 + 500_000_000,
            "ch-1",
            (20, 5),
        ),
        sch_span(
            "sch-3",
            "com.opencode",
            "claude-sonnet-5",
            d1 + 3 * 3600 * sec,
            "ch-1",
            (30, 5),
        ),
        // ch-2 (claude_code, unpriced model): a single span.
        sch_span(
            "sch-4",
            "com.anthropic.claude_code",
            "unknown-vendor-model-9.9",
            d1 + 10 * sec,
            "ch-2",
            (3, 2),
        ),
    ];
    storage.write_span_batch(&spans).await.unwrap();

    // Windowed query — a different cache key than the empty-state call.
    let (status, v) = get_json(
        &app,
        &format!(
            "/api/sessions/chains?start_time={d1}&end_time={}",
            d1 + 86_400 * sec
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Sorted by total tokens desc: ch-1 (75) before ch-2 (5).
    let chains = v["chains"].as_array().unwrap();
    assert_eq!(chains.len(), 2, "{v}");
    let c1 = &chains[0];
    assert_eq!(c1["chain_id"], "ch-1");
    assert_eq!(c1["tool"], "opencode");
    // Two segments (the 3 h gap splits at the default 2 h window).
    let segs = c1["segments"].as_array().unwrap();
    assert_eq!(segs.len(), 2, "{v}");
    assert_eq!(segs[0]["turns"], 2);
    assert_eq!(segs[1]["turns"], 1);
    assert_eq!(c1["total_turns"], 3);
    assert_eq!(c1["total_tokens"], 75);
    // Priced via the deterministic Claude fallback.
    assert!(c1["total_cost_usd"].as_f64().unwrap() > 0.0, "{v}");
    assert_eq!(c1["first_seen"], d1);
    assert_eq!(c1["last_seen"], d1 + 3 * 3600 * sec);

    // The unpriced single-burst chain: null cost, one segment.
    let c2 = &chains[1];
    assert_eq!(c2["chain_id"], "ch-2");
    assert_eq!(c2["tool"], "claude_code");
    assert_eq!(c2["segments"].as_array().unwrap().len(), 1);
    assert_eq!(c2["total_turns"], 1);
    assert_eq!(c2["total_tokens"], 5);
    assert!(c2["total_cost_usd"].is_null(), "{v}");

    // A 4 h window swallows the 3 h gap: ch-1 becomes one segment.
    let (status, v) = get_json(
        &app,
        &format!(
            "/api/sessions/chains?start_time={d1}&end_time={}&chain_window_secs=14400",
            d1 + 86_400 * sec
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let wide = v["chains"].as_array().unwrap();
    assert_eq!(wide[0]["segments"].as_array().unwrap().len(), 1, "{v}");
}
