//! Planner-statistics maintenance (issue #191).
//!
//! Without `sqlite_stat1` statistics SQLite's query planner guesses at
//! table and index costs; on a large database (tens of millions of spans)
//! those guesses produced plans 10–100× slower than the indexed ones —
//! report endpoints measured at 32–80 s for a 1-day window. Nothing in the
//! schema/migration path ever ran `ANALYZE`, so a live database grew for
//! months with the planner blind.
//!
//! This module keeps the statistics fresh: the storage backend spawns a
//! maintenance task (dedicated connection, like the purge scheduler) that
//! runs `ANALYZE` at startup and on a daily tick, but only when the stats
//! are missing or older than [`STATS_MAX_AGE`].

use chrono::{Duration, Timelike};
use rusqlite::{params, Connection, Result};

/// Stats older than this are refreshed. Seven days: frequent enough that
/// planner statistics track a growing database, cheap enough to run daily
/// (ANALYZE on a small database is milliseconds; the daily tick is a no-op
/// until the age gate trips).
pub const STATS_MAX_AGE: Duration = Duration::days(7);

const META_KEY: &str = "last_analyze_unix_ns";

/// Run `ANALYZE` if the planner statistics are missing or older than
/// `max_age`. Returns `true` when `ANALYZE` was executed.
///
/// The last-run timestamp lives in a tiny `meta` table (created on demand,
/// so this also works on databases that predate the table).
pub fn maybe_refresh_planner_stats(conn: &mut Connection, max_age: Duration) -> Result<bool> {
    if !stats_are_stale(conn, max_age)? {
        return Ok(false);
    }
    conn.execute_batch("ANALYZE;")?;
    let now_ns = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![META_KEY, now_ns.to_string()],
    )?;
    Ok(true)
}

/// True when the recorded analysis is missing or older than `max_age`.
///
/// A database with statistics but no recorded run (e.g. analyzed manually or
/// by an older version) is adopted: the existing stats are recorded as
/// fresh instead of triggering a redundant re-ANALYZE.
pub fn stats_are_stale(conn: &mut Connection, max_age: Duration) -> Result<bool> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )?;

    let now_ns = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let last_raw: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = ?", [META_KEY], |r| {
            r.get(0)
        })
        .ok();
    let last_ns = last_raw.and_then(|v| v.parse::<i64>().ok());

    if let Some(t) = last_ns {
        return Ok(now_ns.saturating_sub(t) > max_age.num_nanoseconds().unwrap_or(i64::MAX));
    }

    // No recorded run — adopt pre-existing statistics if there are any.
    let stat_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM sqlite_stat1", [], |r| r.get(0))
        .unwrap_or(0);
    if stat_rows > 0 {
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![META_KEY, now_ns.to_string()],
        )?;
        return Ok(false);
    }
    Ok(true)
}

/// Quiet when no signal has been ingested — and no purge has run — in the
/// last `quiet_for` seconds. Each probe is an O(1) rightmost-leaf index
/// lookup on `idx_*_created_at` / `idx_purge_history_start_time`, so this is
/// cheap even on multi-gigabyte databases.
///
/// `ANALYZE` holds the write lock for its whole duration (up to ~96 s on a
/// 35 GB database — far beyond a writer's busy timeout), so it must never
/// run while ingest or a purge can still be active.
pub fn database_is_quiet(conn: &Connection, quiet_for: Duration) -> Result<bool> {
    database_is_quiet_at(conn, chrono::Utc::now(), quiet_for)
}

pub fn database_is_quiet_at(
    conn: &Connection,
    now: chrono::DateTime<chrono::Utc>,
    quiet_for: Duration,
) -> Result<bool> {
    let max_ts: Option<i64> = conn.query_row(
        "SELECT MAX(ts) FROM (
                SELECT MAX(created_at) AS ts FROM spans
                UNION ALL SELECT MAX(created_at) FROM metrics
                UNION ALL SELECT MAX(created_at) FROM logs
                UNION ALL SELECT MAX(start_time) / 1000000000 FROM purge_history
            )",
        [],
        |r| r.get(0),
    )?;
    let quiet_secs = quiet_for.num_seconds();
    let now_ts = now.timestamp();
    Ok(match max_ts {
        // No activity recorded at all (or purge_history absent) → quiet.
        None => true,
        Some(t) => now_ts.saturating_sub(t) >= quiet_secs,
    })
}

/// Heuristic for a small database: `MAX(id)` on the rowid alias bounds the
/// number of spans ever ingested (O(1) rightmost-leaf probe). Below this
/// threshold `ANALYZE` completes in well under a second, so it is safe at
/// any quiet moment, not just in the nightly window.
pub fn is_small_database(conn: &Connection) -> Result<bool> {
    const SMALL_DB_MAX_SPAN_ID: i64 = 500_000;
    let max_id: i64 = conn
        .query_row("SELECT COALESCE(MAX(id), 0) FROM spans", [], |r| r.get(0))
        .unwrap_or(0);
    Ok(max_id < SMALL_DB_MAX_SPAN_ID)
}

/// Decide whether the statistics should be refreshed right now: stale, and
/// the database is quiet, and either small (cheap ANALYZE) or inside the
/// nightly maintenance window (02:00–04:30 local, after the 02:00 purge).
/// Pure decision — the caller performs the refresh.
pub fn should_refresh_now(conn: &mut Connection) -> Result<bool> {
    should_refresh_now_at(conn, chrono::Local::now())
}

pub fn should_refresh_now_at(
    conn: &mut Connection,
    now: chrono::DateTime<chrono::Local>,
) -> Result<bool> {
    if !stats_are_stale(conn, STATS_MAX_AGE)? {
        return Ok(false);
    }
    if !database_is_quiet_at(conn, now.into(), Duration::minutes(1))? {
        return Ok(false);
    }
    if is_small_database(conn)? {
        return Ok(true);
    }
    Ok((2..4).contains(&now.hour()) || (now.hour() == 4 && now.minute() < 30))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file-backed (not :memory:) DB is not required here — the function
    /// takes the connection — but a real table with rows is, so ANALYZE
    /// has something to profile. Includes the tables the quietness check
    /// touches.
    fn conn_with_data() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE spans (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                start_time INTEGER NOT NULL,
                attributes TEXT,
                created_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX idx_spans_name_ts ON spans(name, start_time);
            CREATE TABLE metrics (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE logs (
                id INTEGER PRIMARY KEY,
                timestamp INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE purge_history (
                id INTEGER PRIMARY KEY,
                start_time INTEGER NOT NULL,
                end_time INTEGER NOT NULL
            );
            INSERT INTO spans (name, start_time, attributes)
            WITH RECURSIVE seq(x) AS (SELECT 0 UNION ALL SELECT x + 1 FROM seq WHERE x < 999)
            SELECT 'llm', 1000000000 + x, '{}' FROM seq;",
        )
        .unwrap();
        conn
    }

    fn meta_ts(conn: &Connection) -> i64 {
        conn.query_row("SELECT value FROM meta WHERE key = ?", [META_KEY], |r| {
            r.get::<_, String>(0)
        })
        .unwrap()
        .parse()
        .unwrap()
    }

    #[test]
    fn test_runs_when_stats_missing_and_records_timestamp() {
        let mut conn = conn_with_data();
        // Fresh DB: no meta table, no sqlite_stat1.
        let n = conn
            .execute("CREATE TABLE IF NOT EXISTS sqlite_stat1 (x INT)", [])
            .and_then(|_| conn.query_row("SELECT COUNT(*) FROM sqlite_stat1", [], |r| r.get(0)))
            .unwrap_or(0);
        assert_eq!(n, 0, "precondition: no statistics yet");

        let ran = maybe_refresh_planner_stats(&mut conn, STATS_MAX_AGE).unwrap();
        assert!(ran, "first run must execute ANALYZE");

        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM sqlite_stat1", [], |r| r.get(0))
            .unwrap();
        assert!(rows > 0, "sqlite_stat1 must be populated after ANALYZE");
        assert!(meta_ts(&conn) > 0);
    }

    #[test]
    fn test_skips_fresh_stats() {
        let mut conn = conn_with_data();
        assert!(maybe_refresh_planner_stats(&mut conn, STATS_MAX_AGE).unwrap());
        let ts_before = meta_ts(&conn);

        // Sub-second gap: well inside the 7-day age gate.
        let ran = maybe_refresh_planner_stats(&mut conn, STATS_MAX_AGE).unwrap();
        assert!(!ran, "fresh stats must not trigger another ANALYZE");
        assert_eq!(meta_ts(&conn), ts_before, "timestamp must be untouched");
    }

    #[test]
    fn test_runs_when_stats_stale() {
        let mut conn = conn_with_data();
        assert!(maybe_refresh_planner_stats(&mut conn, STATS_MAX_AGE).unwrap());

        // Backdate the recorded run past the 7-day gate.
        let old = (chrono::Utc::now() - Duration::days(8))
            .timestamp_nanos_opt()
            .unwrap();
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![META_KEY, old.to_string()],
        )
        .unwrap();

        let ran = maybe_refresh_planner_stats(&mut conn, STATS_MAX_AGE).unwrap();
        assert!(ran, "stale stats must be refreshed");
        assert!(meta_ts(&conn) > old, "timestamp must be advanced");
    }

    #[test]
    fn test_corrupt_meta_value_treated_as_missing() {
        let mut conn = conn_with_data();
        conn.execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO meta (key, value) VALUES ('last_analyze_unix_ns', 'not-a-number');",
        )
        .unwrap();
        let ran = maybe_refresh_planner_stats(&mut conn, STATS_MAX_AGE).unwrap();
        assert!(ran, "unparseable timestamp must fall back to a refresh");
    }

    #[test]
    fn test_stats_are_stale_adopts_preexisting_statistics() {
        let mut conn = conn_with_data();
        // Statistics exist (e.g. from a manual ANALYZE) but no recorded run.
        conn.execute_batch("ANALYZE;").unwrap();
        let stale: i64 = conn
            .query_row("SELECT COUNT(*) FROM sqlite_stat1", [], |r| r.get(0))
            .unwrap();
        assert!(stale > 0, "precondition: statistics exist");

        assert!(
            !stats_are_stale(&mut conn, STATS_MAX_AGE).unwrap(),
            "pre-existing statistics must be adopted, not re-analysed"
        );
        // Adoption records the run.
        let ts = meta_ts(&conn);
        assert!(ts > 0);
    }

    fn local_at(h: u32, m: u32) -> chrono::DateTime<chrono::Local> {
        let dt = chrono::Local::now()
            .date_naive()
            .and_hms_opt(h, m, 0)
            .unwrap();
        chrono::TimeZone::from_local_datetime(&chrono::Local, &dt).unwrap()
    }

    #[test]
    fn test_database_is_quiet_empty_db_is_quiet() {
        let conn = conn_with_data();
        // Wipe the fixture rows so every MAX(created_at) is NULL/0-free.
        conn.execute("DELETE FROM spans", []).unwrap();
        assert!(database_is_quiet(&conn, Duration::minutes(1)).unwrap());
    }

    #[test]
    fn test_database_is_quiet_recent_ingest_not_quiet() {
        let conn = conn_with_data();
        let now = chrono::Utc::now();
        let ts = now.timestamp();
        conn.execute("UPDATE spans SET created_at = ? WHERE id = 1", [ts])
            .unwrap();
        assert!(
            !database_is_quiet_at(&conn, now, Duration::minutes(1)).unwrap(),
            "an ingest one second ago must not be quiet"
        );
        // And quiet again 2 minutes "later".
        assert!(
            database_is_quiet_at(&conn, now + Duration::minutes(2), Duration::minutes(1)).unwrap()
        );
    }

    #[test]
    fn test_database_is_quiet_recent_purge_not_quiet() {
        let conn = conn_with_data();
        let now = chrono::Utc::now();
        let ts_ns = now.timestamp_nanos_opt().unwrap();
        conn.execute(
            "INSERT INTO purge_history (start_time, end_time) VALUES (?, ?)",
            rusqlite::params![ts_ns, ts_ns],
        )
        .unwrap();
        assert!(
            !database_is_quiet_at(&conn, now, Duration::minutes(1)).unwrap(),
            "a purge that just finished must not count as quiet"
        );
    }

    #[test]
    fn test_is_small_database() {
        let conn = conn_with_data();
        assert!(is_small_database(&conn).unwrap(), "1000 spans is small");
        // MAX(id) bounds the row count — one high-id row flips the answer.
        conn.execute(
            "INSERT INTO spans (id, name, start_time) VALUES (600000, 'x', 1)",
            [],
        )
        .unwrap();
        assert!(
            !is_small_database(&conn).unwrap(),
            "600k span ids is not small"
        );
    }

    #[test]
    fn test_should_refresh_now_gates() {
        let now_local = local_at(12, 0); // noon — outside the nightly window

        // 1. Fresh stats → never, even when quiet.
        let mut conn = conn_with_data();
        conn.execute("UPDATE spans SET created_at = 0", []).unwrap();
        maybe_refresh_planner_stats(&mut conn, STATS_MAX_AGE).unwrap();
        assert!(!should_refresh_now_at(&mut conn, now_local).unwrap());

        // 2. Stale + not quiet → no (ingest one second before `now_local`).
        let mut conn = conn_with_data();
        let ingest_ts = now_local.with_timezone(&chrono::Utc).timestamp() - 1;
        conn.execute("UPDATE spans SET created_at = ? WHERE id = 1", [ingest_ts])
            .unwrap();
        assert!(!should_refresh_now_at(&mut conn, now_local).unwrap());

        // 3. Stale + quiet + small → yes at any time.
        let mut conn = conn_with_data();
        conn.execute("UPDATE spans SET created_at = 0", []).unwrap();
        assert!(should_refresh_now_at(&mut conn, now_local).unwrap());

        // 4. Stale + quiet + large → only in the nightly window.
        let mut conn = conn_with_data();
        conn.execute("UPDATE spans SET created_at = 0", []).unwrap();
        conn.execute(
            "INSERT INTO spans (id, name, start_time) VALUES (600000, 'x', 1)",
            [],
        )
        .unwrap();
        assert!(
            !should_refresh_now_at(&mut conn, now_local).unwrap(),
            "noon is out of window"
        );
        assert!(
            should_refresh_now_at(&mut conn, local_at(2, 15)).unwrap(),
            "02:15 is inside the nightly window"
        );
        assert!(
            should_refresh_now_at(&mut conn, local_at(4, 29)).unwrap(),
            "04:29 is still inside the nightly window"
        );
        assert!(
            !should_refresh_now_at(&mut conn, local_at(4, 45)).unwrap(),
            "04:45 is outside the nightly window"
        );
    }
}
