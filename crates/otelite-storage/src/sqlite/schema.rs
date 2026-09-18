//! SQLite schema definitions and initialization

use crate::error::Result;
use rusqlite::Connection;

/// Initialize the database schema
const V4_BASE_TRIGGERS: &str = "
CREATE TRIGGER IF NOT EXISTS trg_trace_latest_ai AFTER INSERT ON spans
                 BEGIN
                     INSERT INTO trace_latest(trace_id, last_start_time)
                     VALUES (new.trace_id, new.start_time)
                     ON CONFLICT(trace_id) DO UPDATE SET
                         last_start_time = MAX(excluded.last_start_time,
                                               trace_latest.last_start_time);
                 END;
                 CREATE TRIGGER IF NOT EXISTS trg_trace_latest_ad AFTER DELETE ON spans
                 WHEN old.start_time =
                      (SELECT last_start_time FROM trace_latest WHERE trace_id = old.trace_id)
                 BEGIN
                     DELETE FROM trace_latest WHERE trace_id = old.trace_id;
                     INSERT INTO trace_latest(trace_id, last_start_time)
                     SELECT trace_id, MAX(start_time) FROM spans
                     WHERE trace_id = old.trace_id
                     HAVING MAX(start_time) IS NOT NULL;
                 END;
                 CREATE TRIGGER IF NOT EXISTS trg_metrics_daily_tool_ai AFTER INSERT ON metrics
                 WHEN json_valid(new.scope)
                  AND json_extract(new.scope,'$.name') IN (
                      'com.anthropic.claude_code', 'com.opencode', 'codex')
                 BEGIN
                     INSERT INTO metrics_daily_tool(day, tool, datapoints)
                     VALUES (
                         strftime('%Y-%m-%d',
                            datetime(new.timestamp / 1000000000, 'unixepoch')),
                         CASE json_extract(new.scope,'$.name')
                             WHEN 'com.anthropic.claude_code' THEN 'claude_code'
                             WHEN 'com.opencode' THEN 'opencode'
                             WHEN 'codex' THEN 'codex'
                             ELSE json_extract(new.scope,'$.name') END,
                         1)
                     ON CONFLICT(day, tool) DO UPDATE SET
                         datapoints = metrics_daily_tool.datapoints + 1;
                 END;
                 CREATE TRIGGER IF NOT EXISTS trg_metrics_daily_tool_ad AFTER DELETE ON metrics
                 WHEN json_valid(old.scope)
                  AND json_extract(old.scope,'$.name') IN (
                      'com.anthropic.claude_code', 'com.opencode', 'codex')
                 BEGIN
                     UPDATE metrics_daily_tool
                     SET datapoints = datapoints - 1
                     WHERE day = strftime('%Y-%m-%d',
                               datetime(old.timestamp / 1000000000, 'unixepoch'))
                       AND tool = CASE json_extract(old.scope,'$.name')
                               WHEN 'com.anthropic.claude_code' THEN 'claude_code'
                               WHEN 'com.opencode' THEN 'opencode'
                               WHEN 'codex' THEN 'codex'
                               ELSE json_extract(old.scope,'$.name') END;
                     DELETE FROM metrics_daily_tool WHERE datapoints <= 0;
                 END;
                 ";

/// The full v4 sync-trigger set: the literal rollup triggers above plus the
/// LLM-token rollup triggers, whose guard and label expressions are
/// generated from the same semconv functions the backfill and (historically)
/// the queries use, so trigger and backfill cannot drift apart. The `name`
/// column reference is explicit (`new.name` / `old.name`) because
/// unqualified names are ambiguous inside trigger bodies.
pub(crate) fn v4_sync_triggers() -> String {
    use otelite_core::semconv;
    // The LLM guard is total (json_valid-gated), but it can match via a
    // vendor span-name prefix with *corrupt* attributes — the trigger
    // bodies then json_extract those attributes, which raises on malformed
    // JSON and would reject the span INSERT. The validity conjuncts keep
    // corrupt-attribute (or corrupt-scope) rows out of the rollup entirely
    // instead ("corrupt rows contribute nothing"), mirroring the query-side
    // total-extraction semantics.
    let ins_validity = "(new.attributes IS NULL OR json_valid(new.attributes)) \
                        AND (new.scope IS NULL OR json_valid(new.scope))";
    let del_validity = "(old.attributes IS NULL OR json_valid(old.attributes)) \
                        AND (old.scope IS NULL OR json_valid(old.scope))";
    format!(
        "{base}
CREATE TRIGGER IF NOT EXISTS trg_spans_daily_llm_ai AFTER INSERT ON spans
WHEN {ins_guard} AND {ins_validity}
BEGIN
    INSERT INTO spans_daily_llm(
        day, tool, model,
        input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens)
    VALUES (
        strftime('%Y-%m-%d', datetime(new.start_time / 1000000000, 'unixepoch')),
        {tool_new},
        {model_new},
        COALESCE({in_new}, 0),
        COALESCE({out_new}, 0),
        COALESCE({cc_new}, 0),
        COALESCE({cr_new}, 0))
    ON CONFLICT(day, tool, model) DO UPDATE SET
        input_tokens = input_tokens + excluded.input_tokens,
        output_tokens = output_tokens + excluded.output_tokens,
        cache_creation_tokens = cache_creation_tokens + excluded.cache_creation_tokens,
        cache_read_tokens = cache_read_tokens + excluded.cache_read_tokens;
END;
CREATE TRIGGER IF NOT EXISTS trg_spans_daily_llm_ad AFTER DELETE ON spans
WHEN {del_guard} AND {del_validity}
BEGIN
    UPDATE spans_daily_llm
    SET input_tokens = input_tokens - COALESCE({in_old}, 0),
        output_tokens = output_tokens - COALESCE({out_old}, 0),
        cache_creation_tokens = cache_creation_tokens - COALESCE({cc_old}, 0),
        cache_read_tokens = cache_read_tokens - COALESCE({cr_old}, 0)
    WHERE day = strftime('%Y-%m-%d', datetime(old.start_time / 1000000000, 'unixepoch'))
      AND tool = {tool_old}
      AND model = {model_old};
    DELETE FROM spans_daily_llm
    WHERE day = strftime('%Y-%m-%d', datetime(old.start_time / 1000000000, 'unixepoch'))
      AND tool = {tool_old}
      AND model = {model_old}
      AND input_tokens <= 0 AND output_tokens <= 0
      AND cache_creation_tokens <= 0 AND cache_read_tokens <= 0;
END;",
        base = V4_BASE_TRIGGERS,
        ins_validity = ins_validity,
        del_validity = del_validity,
        ins_guard = semconv::llm_span_guard_cols("new.attributes", "new.name"),
        del_guard = semconv::llm_span_guard_cols("old.attributes", "old.name"),
        tool_new = semconv::scope_tool_expr("new.scope"),
        tool_old = semconv::scope_tool_expr("old.scope"),
        model_new = semconv::model_expr("new.attributes"),
        model_old = semconv::model_expr("old.attributes"),
        in_new =
            semconv::coalesce_extract_cast("new.attributes", semconv::INPUT_TOKEN_KEYS, "INTEGER"),
        out_new =
            semconv::coalesce_extract_cast("new.attributes", semconv::OUTPUT_TOKEN_KEYS, "INTEGER"),
        cc_new = semconv::coalesce_extract_cast(
            "new.attributes",
            semconv::CACHE_CREATION_TOKEN_KEYS,
            "INTEGER"
        ),
        cr_new = semconv::coalesce_extract_cast(
            "new.attributes",
            semconv::CACHE_READ_TOKEN_KEYS,
            "INTEGER"
        ),
        in_old =
            semconv::coalesce_extract_cast("old.attributes", semconv::INPUT_TOKEN_KEYS, "INTEGER"),
        out_old =
            semconv::coalesce_extract_cast("old.attributes", semconv::OUTPUT_TOKEN_KEYS, "INTEGER"),
        cc_old = semconv::coalesce_extract_cast(
            "old.attributes",
            semconv::CACHE_CREATION_TOKEN_KEYS,
            "INTEGER"
        ),
        cr_old = semconv::coalesce_extract_cast(
            "old.attributes",
            semconv::CACHE_READ_TOKEN_KEYS,
            "INTEGER"
        ),
    )
}

pub fn initialize_schema(conn: &Connection) -> Result<()> {
    // Enable WAL mode for better concurrency
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;

    // FULL: fsync each WAL frame before commit. Prevents DB corruption on OS crash
    // at the cost of slightly higher write latency. NORMAL is faster but can corrupt
    // the DB on power loss or OS crash during a WAL checkpoint.
    conn.execute_batch("PRAGMA synchronous=FULL;")?;

    // Create logs table
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp INTEGER NOT NULL,
            observed_timestamp INTEGER,
            trace_id TEXT,
            span_id TEXT,
            severity_number INTEGER NOT NULL,
            severity_text TEXT,
            body TEXT NOT NULL,
            attributes TEXT,
            resource TEXT,
            scope TEXT,
            flags INTEGER,
            created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        );",
    )?;

    // Create indexes for logs
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_logs_timestamp ON logs(timestamp);
         CREATE INDEX IF NOT EXISTS idx_logs_severity ON logs(severity_number);
         CREATE INDEX IF NOT EXISTS idx_logs_trace_id ON logs(trace_id) WHERE trace_id IS NOT NULL;
         CREATE INDEX IF NOT EXISTS idx_logs_created_at ON logs(created_at);",
    )?;

    // Create spans table
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS spans (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            trace_id TEXT NOT NULL,
            span_id TEXT NOT NULL,
            parent_span_id TEXT,
            name TEXT NOT NULL,
            kind INTEGER NOT NULL,
            start_time INTEGER NOT NULL,
            end_time INTEGER NOT NULL,
            attributes TEXT,
            events TEXT,
            links TEXT,
            status_code INTEGER,
            status_message TEXT,
            resource TEXT,
            scope TEXT,
            flags INTEGER,
            created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        );",
    )?;

    // Create indexes for spans
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_spans_trace_id ON spans(trace_id);
         CREATE INDEX IF NOT EXISTS idx_spans_span_id ON spans(span_id);
         CREATE INDEX IF NOT EXISTS idx_spans_start_time ON spans(start_time);
         CREATE INDEX IF NOT EXISTS idx_spans_end_time ON spans(end_time);
         CREATE INDEX IF NOT EXISTS idx_spans_parent_span_id ON spans(parent_span_id) WHERE parent_span_id IS NOT NULL;
         CREATE INDEX IF NOT EXISTS idx_spans_created_at ON spans(created_at);
         -- Covering index for per-trace list summaries (#251): the
         -- trace-list endpoint computes MIN(start_time)/MAX(end_time)/
         -- COUNT(*)/error flag per selected trace; covering keeps that
         -- aggregation on the index alone (2.9M-span agent traces made
         -- the row-fetching form take ~5 s on a production database).
         CREATE INDEX IF NOT EXISTS idx_spans_trace_agg ON spans(trace_id, start_time, end_time, status_code);",
    )?;

    // Partial indexes for GenAI analytics. GenAI queries filter by a
    // json_extract OR-guard over `attributes`; without these indexes the
    // planner scans the whole time window (millions of rows) and evaluates
    // the guard per row. The guards are generated by the same semconv
    // functions the queries use, so index and query can never drift apart.
    // A partial index is only eligible when the query's WHERE contains the
    // index WHERE verbatim as a conjunct — which the reader queries do.
    {
        use otelite_core::semconv;
        let llm_guard = semconv::llm_span_guard("attributes");
        let request_guard = semconv::request_span_guard("attributes");
        conn.execute(
            &format!(
                "CREATE INDEX IF NOT EXISTS idx_spans_llm ON spans(start_time) WHERE {llm_guard}"
            ),
            [],
        )?;
        conn.execute(
            &format!(
                "CREATE INDEX IF NOT EXISTS idx_spans_llm_request ON spans(start_time) WHERE {request_guard}"
            ),
            [],
        )?;
    }

    // Partial/expression indexes for the remaining analytics and detail
    // queries. Same drift-proof pattern as the GenAI guards above: the
    // predicates are generated by the same semconv functions the reader
    // queries embed in their WHERE clauses, and each query carries its
    // index predicate verbatim as a conjunct so the planner can use it.
    //
    // All predicates are total (json_valid-gated) because SQLite evaluates
    // partial-index predicates on every INSERT: a raising predicate would
    // reject telemetry batches containing a single corrupt row.
    {
        use otelite_core::semconv;
        let col = "attributes";
        conn.execute(
            &format!(
                "CREATE INDEX IF NOT EXISTS idx_spans_session_id \
                 ON spans({expr}) WHERE {pred}",
                expr = semconv::session_id_expr(col),
                pred = semconv::session_id_index_predicate(col)
            ),
            [],
        )?;
        // Time-ordered window index for session-bearing spans (#192):
        // query_session_quality_map scans a start_time range and needs
        // every session-bearing row inside it. idx_spans_session_id above
        // is keyed on the session id (point lookups); this one is keyed on
        // start_time so the windowed report is an index range scan over
        // the few thousand session spans per day instead of a full
        // table scan with per-row json_extract. Same drift-proof
        // contract as the other partial indexes: the query's WHERE must
        // carry this predicate verbatim as conjuncts
        // (json_valid(...) AND session.id IS NOT NULL) for eligibility.
        conn.execute(
            &format!(
                "CREATE INDEX IF NOT EXISTS idx_spans_session_window \
                 ON spans(start_time) WHERE {pred}",
                pred = semconv::session_id_index_predicate(col)
            ),
            [],
        )?;
        // Window index for span-level reasoning-token aggregation
        // (query_reasoning_share, #192): the query filters on
        // json_valid(attributes) AND the reasoning key IS NOT NULL, so
        // this partial index turns the per-row json_extract scan into an
        // index range scan over the (rare) LLM spans carrying the key.
        conn.execute(
            &format!(
                "CREATE INDEX IF NOT EXISTS idx_spans_reasoning_tokens \
                 ON spans(start_time) WHERE json_valid({col}) \
                 AND json_extract({col}, '$.\"{key}\"') IS NOT NULL",
                key = semconv::REASONING_TOKEN_KEYS[0]
            ),
            [],
        )?;
        conn.execute(
            &format!(
                "CREATE INDEX IF NOT EXISTS idx_spans_finish_reason \
                 ON spans(start_time) WHERE {guard}",
                guard = semconv::finish_reason_guard(col)
            ),
            [],
        )?;
        conn.execute(
            &format!(
                "CREATE INDEX IF NOT EXISTS idx_spans_tool \
                 ON spans(start_time) WHERE {guard}",
                guard = semconv::tool_span_guard(col)
            ),
            [],
        )?;
        conn.execute(
            &format!(
                "CREATE INDEX IF NOT EXISTS idx_spans_retrieval \
                 ON spans(start_time) WHERE {guard}",
                guard = semconv::retrieval_span_guard(col)
            ),
            [],
        )?;
        conn.execute(
            &format!(
                "CREATE INDEX IF NOT EXISTS idx_spans_tool_exec \
                 ON spans(start_time) WHERE name = '{name}'",
                name = semconv::TOOL_EXECUTION_SPAN_NAME
            ),
            [],
        )?;
        conn.execute(
            &format!(
                "CREATE INDEX IF NOT EXISTS idx_spans_tool_approval \
                 ON spans(start_time) WHERE name = '{name}'",
                name = semconv::TOOL_APPROVAL_SPAN_NAME
            ),
            [],
        )?;
        conn.execute(
            &format!(
                "CREATE INDEX IF NOT EXISTS idx_spans_llm_request_name \
                  ON spans(start_time) WHERE name = '{name}'",
                name = semconv::LLM_REQUEST_SPAN_NAME
            ),
            [],
        )?;
        conn.execute(
            &format!(
                "CREATE INDEX IF NOT EXISTS idx_spans_codex_handle_responses \
                  ON spans(start_time) WHERE name = '{name}'",
                name = semconv::CODEX_HANDLE_RESPONSES_SPAN_NAME
            ),
            [],
        )?;
        conn.execute(
            &format!(
                "CREATE INDEX IF NOT EXISTS idx_logs_api_body \
                 ON logs(timestamp) WHERE body = '{body}'",
                body = semconv::API_RESPONSE_BODY_LOG_BODY
            ),
            [],
        )?;
    }

    // Create metrics table
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS metrics (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            description TEXT,
            unit TEXT,
            metric_type INTEGER NOT NULL,
            timestamp INTEGER NOT NULL,
            value_int INTEGER,
            value_double REAL,
            value_histogram TEXT,
            value_summary TEXT,
            attributes TEXT,
            resource TEXT,
            scope TEXT,
            flags INTEGER,
            created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        );",
    )?;

    // Create indexes for metrics
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_metrics_name ON metrics(name);
         CREATE INDEX IF NOT EXISTS idx_metrics_timestamp ON metrics(timestamp);
         CREATE INDEX IF NOT EXISTS idx_metrics_type ON metrics(metric_type);
         CREATE INDEX IF NOT EXISTS idx_metrics_created_at ON metrics(created_at);
         CREATE INDEX IF NOT EXISTS idx_metrics_name_ts ON metrics(name, timestamp);",
    )?;

    // Write-maintained "latest row per metric name" table (#251). The
    // all-time metrics list used to compute it with a GROUP BY over every
    // metrics row (16 s on a production-scale database); with this table
    // it is a handful of index lookups. Triggers keep it in sync — the
    // delete trigger fires only when the deleted row was the latest, so
    // bulk purges of old rows (which never are) stay cheap.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS metric_latest (
            name TEXT PRIMARY KEY,
            timestamp INTEGER NOT NULL,
            id INTEGER NOT NULL
        );
         CREATE TRIGGER IF NOT EXISTS trg_metric_latest_ai AFTER INSERT ON metrics
         BEGIN
             INSERT INTO metric_latest(name, timestamp, id)
             VALUES (new.name, new.timestamp, new.id)
             ON CONFLICT(name) DO UPDATE SET
                 timestamp = MAX(excluded.timestamp, metric_latest.timestamp),
                 id = CASE WHEN excluded.timestamp > metric_latest.timestamp
                           THEN excluded.id ELSE metric_latest.id END;
         END;
         CREATE TRIGGER IF NOT EXISTS trg_metric_latest_ad AFTER DELETE ON metrics
         WHEN old.id = (SELECT id FROM metric_latest WHERE name = old.name)
         BEGIN
             DELETE FROM metric_latest WHERE name = old.name;
             INSERT INTO metric_latest(name, timestamp, id)
             SELECT name, MAX(timestamp), MAX(id) FROM metrics
             WHERE name = old.name
             HAVING MAX(timestamp) IS NOT NULL;
         END;",
    )?;

    // Covering index for cumulative-counter windowed queries
    // (reader::counter_window_deltas): the per-series baseline lookup seeks by
    // the full label set + timestamp and reads the value without a table
    // fetch. The json_valid-gated expressions are total (NULL on missing or
    // malformed attributes), so evaluating them at INSERT time can never
    // raise and break ingestion. The reader's baseline query must use these
    // expressions verbatim for the index to be used (planner contract).
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_metrics_opencode_token_usage ON metrics(
             CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.agent') END,
             CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.model') END,
             CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.type') END,
             CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.\"session.id\"') END,
             timestamp, value_int)
          WHERE name = 'opencode.token.usage';",
    )?;

    // Covering indexes for the agent-rollup cumulative histogram counters
    // (reader::counter_window_deltas with a value expression): same
    // planner contract as idx_metrics_opencode_token_usage — the reader's
    // baseline seeks use these expressions verbatim. The histogram value
    // columns make the baseline seek index-only (no table fetch per
    // candidate row).
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_metrics_opencode_tool_duration ON metrics(
             CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.\"session.id\"') END,
             CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.tool_name') END,
             timestamp,
             CASE WHEN json_valid(value_histogram) THEN json_extract(value_histogram, '$[0]') END)
          WHERE name = 'opencode.tool.duration';",
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_metrics_opencode_session_cost ON metrics(
             CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.\"session.id\"') END,
             timestamp,
             CASE WHEN json_valid(value_histogram) THEN json_extract(value_histogram, '$[1]') END)
          WHERE name = 'opencode.session.cost.total';",
    )?;

    // Covering index for effort × model × type rollup on claude_code.token.usage.
    // Enables O(index) aggregation for the effort breakdown endpoint (#157)
    // without touching the table rows. The json_valid-gated expressions are
    // total so evaluating them at INSERT time never raises.
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_metrics_claude_code_token_effort ON metrics(
             CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.effort') END,
             CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.model') END,
             CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.type') END,
             timestamp, value_int)
          WHERE name = 'claude_code.token.usage';",
    )?;

    // Covering partial index for the tool-mix datapoint scan
    // (reader::query_daily_tool_mix source 1): the index columns are exactly
    // the reader's WHERE/SELECT expressions, so the 30d (day, tool)
    // aggregation is an index-only scan over the three tool scopes instead
    // of a full metrics-table scan with per-row JSON extraction (59 s on
    // the 2026-09-17 production DB, #251). The partial predicate duplicates
    // the reader's WHERE verbatim (planner contract): any divergence
    // degrades the query to a full scan.
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_metrics_tool_scope_ts ON metrics(
             json_extract(scope,'$.name'),
             timestamp)
          WHERE json_valid(scope) AND json_extract(scope,'$.name')
                IN ('com.anthropic.claude_code','com.opencode','codex');",
    )?;

    // Write-maintained rollups for the two list endpoints that outgrew
    // index-only scans on the production database (#251, 2026-09-18, 52 GB):
    //
    //  * `metrics_daily_tool` — per-(day, tool) datapoint counts. The raw
    //    30-day window holds ~5M tool-scoped datapoints; a per-row GROUP BY
    //    (even fully index-covered) measured 9.6 s, nowhere near the <1 s
    //    bar. The rollup keeps ~30 rows per month and the query is a
    //    primary-key range scan.
    //  * `trace_latest` — newest span start per trace. The traces-list
    //    phase-1 reverse walk over idx_spans_start_time is O(spans since
    //    the 50th-newest trace); on a machine where a handful of
    //    multi-million-span agent traces dominate the newest hour that is
    //    ~3M rows (41 s measured). The rollup makes phase 1 an ordered
    //    LIMIT seek.
    //
    // Tables only here: the sync triggers are created by the v4 migration
    // AFTER the one-time backfill, inside one write transaction, so live
    // ingest can neither double-count against the backfill snapshot nor
    // race it (the same ordering bug that bricked the v3 metric_latest
    // backfill in production, 2026-09-18).
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS trace_latest (
            trace_id TEXT PRIMARY KEY,
            last_start_time INTEGER NOT NULL
        );
         CREATE INDEX IF NOT EXISTS idx_trace_latest_start_time
             ON trace_latest(last_start_time, trace_id);
         CREATE TABLE IF NOT EXISTS metrics_daily_tool (
            day TEXT NOT NULL,
            tool TEXT NOT NULL,
            datapoints INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (day, tool)
        );",
    )?;

    // Covering partial index: a trace's root span (newest null-parent
    // span) is a one-entry seek with the name served from the index —
    // the per-trace root-name subqueries in query_trace_summaries used to
    // walk a giant trace's whole index range with table fetches (#251).
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_spans_root_name ON spans(trace_id, start_time, name)
         WHERE parent_span_id IS NULL;",
    )?;

    // Write-maintained per-(day, tool, model) LLM token rollup for
    // query_daily_tool_mix source 2 (#251, 2026-09-18, 52 GB production
    // DB): the raw 30-day window holds ~34k LLM spans, and the raw query
    // (even index-assisted) costs a table fetch plus JSON parsing per
    // span — 31 s cold, and the planner demonstrably does not prefer a
    // covering expression index over the existing start_time partial
    // indexes for this query shape. The rollup makes source 2 a
    // primary-key range scan over a few thousand rows. Sync triggers
    // are armed by the v4 migration (see V4 triggers below), after the
    // one-time backfill, in the same write transaction.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS spans_daily_llm (
            day TEXT NOT NULL,
            tool TEXT NOT NULL,
            model TEXT NOT NULL,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
            cache_read_tokens INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (day, tool, model)
        );",
    )?;

    // Expression index for Codex cwd-based project rollup (#160/#164).
    // Covers run_sampling_request spans only; the cwd value and start_time
    // let project-rollup and busy/idle breakdown queries seek by project
    // without scanning the full span table.
    conn.execute(
        &format!(
            "CREATE INDEX IF NOT EXISTS idx_spans_codex_cwd ON spans(\
              CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.\"{cwd}\"') END,\
              start_time) WHERE name = '{span}'",
            cwd = otelite_core::semconv::CODEX_CWD_KEY,
            span = otelite_core::semconv::CODEX_LLM_REQUEST_SPAN_NAME,
        ),
        [],
    )?;

    // Create purge_history table
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS purge_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            start_time INTEGER NOT NULL,
            end_time INTEGER NOT NULL,
            logs_deleted INTEGER NOT NULL DEFAULT 0,
            spans_deleted INTEGER NOT NULL DEFAULT 0,
            metrics_deleted INTEGER NOT NULL DEFAULT 0
        );",
    )?;

    // Create index for purge_history
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_purge_history_start_time ON purge_history(start_time);",
    )?;

    // Create FTS5 full-text search table for logs
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS logs_fts USING fts5(
            body,
            content='logs',
            content_rowid='id'
        );",
    )?;

    // Create triggers to keep the FTS5 table in sync.
    //
    // `logs_fts` is an external-content table: it stores only the index,
    // and lookups join back into `logs`. Removing an index entry requires
    // the special FTS5 command form
    // `INSERT INTO logs_fts(logs_fts, rowid, body) VALUES ('delete', ...)`
    // — a plain `DELETE FROM logs_fts WHERE rowid = ...` is valid SQL but
    // matches nothing (the FTS table has no rows of its own) and silently
    // leaves orphaned entries behind, which is exactly what the pre-fix
    // triggers did.
    //
    // Migration: pre-fix builds shipped those no-op triggers and left
    // stale index entries behind. Detect them by the missing `'delete'`
    // command in the stored trigger SQL, replace the triggers, and rebuild
    // the index from the content table to drop the orphans. One-time cost,
    // proportional to log volume.
    let broken_fts_triggers = conn
        .query_row(
            "SELECT sql FROM sqlite_master
             WHERE type = 'trigger' AND name = 'logs_fts_delete'",
            [],
            |row| row.get::<_, String>(0),
        )
        .map(|sql| !sql.contains("'delete'"))
        .unwrap_or(false);

    if broken_fts_triggers {
        // Pre-fix builds shipped no-op delete/update triggers, so purged or
        // updated logs kept stale index entries. Replace the triggers (the
        // IF NOT EXISTS batch below recreates them with the correct SQL)
        // and rebuild the index from the content table to drop the
        // orphans. One-time cost, proportional to log volume.
        conn.execute_batch("DROP TRIGGER logs_fts_delete; DROP TRIGGER logs_fts_update;")?;
    }

    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS logs_fts_insert AFTER INSERT ON logs BEGIN
            INSERT INTO logs_fts(rowid, body) VALUES (new.id, new.body);
         END;

         CREATE TRIGGER IF NOT EXISTS logs_fts_delete AFTER DELETE ON logs BEGIN
            INSERT INTO logs_fts(logs_fts, rowid, body) VALUES ('delete', old.id, old.body);
         END;

         CREATE TRIGGER IF NOT EXISTS logs_fts_update AFTER UPDATE ON logs BEGIN
            INSERT INTO logs_fts(logs_fts, rowid, body) VALUES ('delete', old.id, old.body);
            INSERT INTO logs_fts(rowid, body) VALUES (new.id, new.body);
         END;",
    )?;

    if broken_fts_triggers {
        // Rebuild the index from the content table so the stale entries
        // left behind by the old triggers are dropped. A failure here
        // fails initialisation loudly; the triggers are already correct,
        // so only index hygiene (stale terms from updated logs) is at
        // stake on a retry.
        conn.execute("INSERT INTO logs_fts(logs_fts) VALUES ('rebuild')", [])?;
    }

    // ── Versioned migrations (PRAGMA user_version) ─────────────────────────
    // Pre-v2 databases are at user_version 0; each block below runs exactly
    // once per database. Fresh databases pass through the same blocks with
    // empty tables (no-op work).
    //
    // Each migration runs as one explicit write transaction: the work and
    // its version stamp commit or roll back together, so a crash mid-
    // migration re-runs the block on next boot instead of leaving a
    // half-migrated database. (The v3 block additionally had to become
    // idempotent — see below.)
    let version: i32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap_or(0);
    if version < 2 {
        // v2 (#254): a span's identity is (trace_id, span_id). OTLP
        // exporters retry a batch after a timeout while the write is still
        // committing, so retried spans used to insert duplicate rows and
        // inflate span-based analytics (327 duplicate groups on a
        // production database). Drop the retry copies — the highest id in
        // each group is the retry; the lowest is the original — before
        // adding the constraint. One-time cost scales with table size.
        conn.execute("BEGIN IMMEDIATE", [])?;
        let result: rusqlite::Result<usize> = (|| {
            let removed = conn.execute(
                "DELETE FROM spans WHERE id IN (
                     SELECT s1.id FROM spans AS s1
                     JOIN spans AS s2
                       ON s1.trace_id = s2.trace_id AND s1.span_id = s2.span_id
                     WHERE s1.id > s2.id)",
                [],
            )?;
            conn.execute(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_spans_trace_span \
                 ON spans(trace_id, span_id)",
                [],
            )?;
            conn.pragma_update(None, "user_version", 2)?;
            Ok(removed)
        })();
        match result {
            Ok(removed) => {
                conn.execute("COMMIT", [])?;
                if removed > 0 {
                    tracing::info!(
                        "schema v2: dropped {removed} duplicate span rows (OTLP retry dedup)"
                    );
                }
            },
            Err(e) => {
                let _ = conn.execute("ROLLBACK", []);
                return Err(e.into());
            },
        }
    }
    if version < 3 {
        // v3 (#251): backfill metric_latest for databases that predate the
        // write-maintained table. One-time cost scales with the metrics
        // table (same GROUP BY the old all-time list ran per query).
        //
        // The backfill must be a merge, not a plain INSERT (production
        // incident, 2026-09-18): the metric_latest write trigger is active
        // during the backfill, so concurrent ingest — or a re-run after a
        // crash between the INSERT and the version stamp — collided with
        // already-present rows and failed with
        // "UNIQUE constraint failed: metric_latest.name", brick-ing
        // startup on every launchd restart. The ON CONFLICT merge makes
        // the backfill idempotent under both races; the transaction
        // (above) makes backfill + stamp atomic.
        conn.execute("BEGIN IMMEDIATE", [])?;
        let result: rusqlite::Result<usize> = (|| {
            let names = conn.execute(
                "INSERT INTO metric_latest(name, timestamp, id) \
                 SELECT name, MAX(timestamp), MAX(id) FROM metrics GROUP BY name \
                 ON CONFLICT(name) DO UPDATE SET \
                     timestamp = MAX(excluded.timestamp, metric_latest.timestamp), \
                     id = CASE WHEN excluded.timestamp > metric_latest.timestamp \
                               THEN excluded.id ELSE metric_latest.id END",
                [],
            )?;
            conn.pragma_update(None, "user_version", 3)?;
            Ok(names)
        })();
        match result {
            Ok(names) => {
                conn.execute("COMMIT", [])?;
                if names > 0 {
                    tracing::info!(
                        "schema v3: backfilled metric_latest for {names} metric names (one-time)"
                    );
                }
            },
            Err(e) => {
                let _ = conn.execute("ROLLBACK", []);
                return Err(e.into());
            },
        }
    }
    if version < 4 {
        // v4 (#251): backfill the three write-maintained rollups (tables
        // are created in the base DDL) and arm their sync triggers — in
        // this order, inside one write transaction. The transaction holds
        // the write lock for the whole backfill, so live ingest cannot
        // commit between the snapshot and the trigger arming: no
        // double-count (trigger + backfill both counting the same rows)
        // and no gap (rows landing after the snapshot are covered by the
        // triggers, which are active by the time the lock releases). All
        // backfills merge on conflict, so a re-run after a failed
        // migration is safe.
        //
        // One-time cost on a production-scale database: the trace
        // backfill walks idx_spans_trace_agg (~42M entries), the tool
        // backfill groups ~5.5M tool-scoped metric rows, and the LLM
        // token backfill groups ~5M LLM spans. The completion log is
        // conditional on rows moved: on a fresh database the backfills
        // touch nothing, and CLI commands must keep stdout clean for
        // their JSON output (tracing reaches stdout in a CLI process —
        // same discipline as the v2 log).
        conn.execute("BEGIN IMMEDIATE", [])?;
        let result: rusqlite::Result<(usize, usize, usize)> = (|| {
            let trace_rows = conn.execute(
                "INSERT INTO trace_latest(trace_id, last_start_time)
                 SELECT trace_id, MAX(start_time) FROM spans GROUP BY trace_id
                 ON CONFLICT(trace_id) DO UPDATE SET
                     last_start_time = MAX(excluded.last_start_time,
                                           trace_latest.last_start_time)",
                [],
            )?;
            let tool_rows = conn.execute(
                "INSERT INTO metrics_daily_tool(day, tool, datapoints)
                 SELECT strftime('%Y-%m-%d', datetime(timestamp / 1000000000, 'unixepoch')),
                        CASE json_extract(scope,'$.name')
                            WHEN 'com.anthropic.claude_code' THEN 'claude_code'
                            WHEN 'com.opencode' THEN 'opencode'
                            WHEN 'codex' THEN 'codex'
                            ELSE json_extract(scope,'$.name') END,
                        COUNT(*)
                 FROM metrics
                 WHERE json_valid(scope)
                   AND json_extract(scope,'$.name') IN (
                       'com.anthropic.claude_code', 'com.opencode', 'codex')
                 GROUP BY 1, 2
                 ON CONFLICT(day, tool) DO UPDATE SET datapoints = excluded.datapoints",
                [],
            )?;
            let llm_rows = conn.execute(
                &format!(
                    "INSERT INTO spans_daily_llm(
                        day, tool, model,
                        input_tokens, output_tokens,
                        cache_creation_tokens, cache_read_tokens)
                     SELECT strftime('%Y-%m-%d', datetime(start_time / 1000000000, 'unixepoch')),
                            {tool},
                            {model},
                            COALESCE(SUM({input}), 0),
                            COALESCE(SUM({output}), 0),
                            COALESCE(SUM({cc}), 0),
                            COALESCE(SUM({cr}), 0)
                     FROM spans
                     WHERE {guard}
                       AND (attributes IS NULL OR json_valid(attributes))
                       AND (scope IS NULL OR json_valid(scope))
                     GROUP BY 1, 2, 3
                     ON CONFLICT(day, tool, model) DO UPDATE SET
                         input_tokens = excluded.input_tokens,
                         output_tokens = excluded.output_tokens,
                         cache_creation_tokens = excluded.cache_creation_tokens,
                         cache_read_tokens = excluded.cache_read_tokens",
                    tool = otelite_core::semconv::scope_tool_expr("scope"),
                    model = otelite_core::semconv::model_expr("attributes"),
                    input = otelite_core::semconv::coalesce_extract_cast(
                        "attributes",
                        otelite_core::semconv::INPUT_TOKEN_KEYS,
                        "INTEGER",
                    ),
                    output = otelite_core::semconv::coalesce_extract_cast(
                        "attributes",
                        otelite_core::semconv::OUTPUT_TOKEN_KEYS,
                        "INTEGER",
                    ),
                    cc = otelite_core::semconv::coalesce_extract_cast(
                        "attributes",
                        otelite_core::semconv::CACHE_CREATION_TOKEN_KEYS,
                        "INTEGER",
                    ),
                    cr = otelite_core::semconv::coalesce_extract_cast(
                        "attributes",
                        otelite_core::semconv::CACHE_READ_TOKEN_KEYS,
                        "INTEGER",
                    ),
                    guard = otelite_core::semconv::llm_span_guard("attributes"),
                ),
                [],
            )?;
            conn.execute_batch(&v4_sync_triggers())?;
            conn.pragma_update(None, "user_version", 4)?;
            Ok((trace_rows, tool_rows, llm_rows))
        })();
        match result {
            Ok((trace_rows, tool_rows, llm_rows)) => {
                conn.execute("COMMIT", [])?;
                if trace_rows > 0 || tool_rows > 0 || llm_rows > 0 {
                    tracing::info!(
                        "schema v4: backfilled rollups (trace_latest {trace_rows} rows, \
                         metrics_daily_tool {tool_rows} rows, spans_daily_llm {llm_rows} rows) \
                         and armed sync triggers (one-time)"
                    );
                }
            },
            Err(e) => {
                let _ = conn.execute("ROLLBACK", []);
                return Err(e.into());
            },
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn test_schema_initialization() {
        let conn = Connection::open_in_memory().unwrap();
        let result = initialize_schema(&conn);
        assert!(result.is_ok());
    }

    /// #254: the v2 migration dedups spans already retried into a pre-v2
    /// database (duplicate `(trace_id, span_id)` rows from exporter
    /// retries), keeps the first write, adds the uniqueness constraint,
    /// and never re-runs.
    #[test]
    fn test_schema_v2_dedups_retried_spans() {
        let conn = Connection::open_in_memory().unwrap();
        // Simulate a pre-v2 database: a v1-shaped spans table (no unique
        // index, user_version 0) holding a retried export — the same
        // (trace_id, span_id) twice — plus one clean span.
        conn.execute_batch(
            "CREATE TABLE spans (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                trace_id TEXT NOT NULL,
                span_id TEXT NOT NULL,
                parent_span_id TEXT,
                name TEXT NOT NULL,
                kind INTEGER NOT NULL,
                start_time INTEGER NOT NULL,
                end_time INTEGER NOT NULL,
                attributes TEXT,
                events TEXT,
                links TEXT,
                status_code INTEGER,
                status_message TEXT,
                resource TEXT,
                scope TEXT,
                flags INTEGER,
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
            );
            INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time)
                VALUES ('t1', 's1', 'a', 0, 1, 2),
                       ('t1', 's1', 'a', 0, 1, 2),
                       ('t2', 's2', 'b', 0, 3, 4);",
        )
        .unwrap();

        initialize_schema(&conn).unwrap();

        // The retry is dropped; the original (lowest id) is kept.
        let (count, kept_id): (i64, i64) = conn
            .query_row(
                "SELECT COUNT(*), MIN(id) FROM spans \
                 WHERE trace_id = 't1' AND span_id = 's1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(kept_id, 1);
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM spans", [], |row| row.get(0))
            .unwrap();
        assert_eq!(total, 2);

        // Constraint + version are set (v4 = latest; the v2/v3/v4 blocks
        // all ran inside initialize_schema).
        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 4);
        let has_index: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master \
                 WHERE type = 'index' AND name = 'idx_spans_trace_span'",
                [],
                |row| Ok(row.get::<_, i64>(0)? == 1),
            )
            .unwrap();
        assert!(has_index);

        // Re-init is a no-op: the migration ran exactly once.
        initialize_schema(&conn).unwrap();
        let total_after: i64 = conn
            .query_row("SELECT COUNT(*) FROM spans", [], |row| row.get(0))
            .unwrap();
        assert_eq!(total_after, 2);
    }

    /// metric_latest must track the newest row per name under
    /// out-of-order inserts, and recompute when the latest row is
    /// deleted (#251).
    #[test]
    fn test_metric_latest_triggers_maintain_table() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();

        let insert = |name: &str, ts: i64| {
            conn.execute(
                "INSERT INTO metrics (name, metric_type, timestamp, value_int) \
                 VALUES (?1, 0, ?2, 1)",
                rusqlite::params![name, ts],
            )
            .unwrap();
        };
        let latest = |name: &str| -> Option<i64> {
            conn.query_row(
                "SELECT timestamp FROM metric_latest WHERE name = ?1",
                rusqlite::params![name],
                |r| r.get(0),
            )
            .ok()
        };

        // In-order: latest advances.
        insert("m.a", 100);
        assert_eq!(latest("m.a"), Some(100));
        insert("m.a", 200);
        assert_eq!(latest("m.a"), Some(200));

        // Out-of-order (an older row arriving later): latest unchanged.
        insert("m.a", 50);
        assert_eq!(latest("m.a"), Some(200));

        // Deleting a non-latest row: latest unchanged.
        let old_id: i64 = conn
            .query_row(
                "SELECT id FROM metrics WHERE name = 'm.a' AND timestamp = 50",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "DELETE FROM metrics WHERE id = ?1",
            rusqlite::params![old_id],
        )
        .unwrap();
        assert_eq!(latest("m.a"), Some(200));

        // Deleting the latest row: latest recomputes to the new max.
        let newest_id: i64 = conn
            .query_row(
                "SELECT id FROM metrics WHERE name = 'm.a' AND timestamp = 200",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "DELETE FROM metrics WHERE id = ?1",
            rusqlite::params![newest_id],
        )
        .unwrap();
        assert_eq!(latest("m.a"), Some(100));

        // Deleting the final row: the entry disappears.
        let last_id: i64 = conn
            .query_row(
                "SELECT id FROM metrics WHERE name = 'm.a' AND timestamp = 100",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "DELETE FROM metrics WHERE id = ?1",
            rusqlite::params![last_id],
        )
        .unwrap();
        assert_eq!(latest("m.a"), None);
    }

    /// v3 backfill: a database predating metric_latest (simulated by
    /// clearing the table and resetting user_version to 2) gets the
    /// table repopulated from the metrics rows on re-init (#251).
    #[test]
    fn test_schema_v3_backfills_metric_latest() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();

        for (name, ts) in [("m.a", 100), ("m.a", 250), ("m.b", 90)] {
            conn.execute(
                "INSERT INTO metrics (name, metric_type, timestamp, value_int) \
                 VALUES (?1, 0, ?2, 1)",
                rusqlite::params![name, ts],
            )
            .unwrap();
        }

        // Simulate a pre-v3 database: no metric_latest contents, version 2.
        conn.execute("DELETE FROM metric_latest", []).unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();

        initialize_schema(&conn).unwrap();

        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        // v3 and v4 both ran (the simulated DB was at v2).
        assert_eq!(version, 4);
        let rows: Vec<(String, i64)> = conn
            .prepare("SELECT name, timestamp FROM metric_latest ORDER BY name")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![("m.a".to_string(), 250), ("m.b".to_string(), 90)]
        );
    }

    #[test]
    fn test_tables_created() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();

        // Verify logs table exists
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='logs'")
            .unwrap();
        let exists = stmt.exists([]).unwrap();
        assert!(exists);

        // Verify spans table exists
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='spans'")
            .unwrap();
        let exists = stmt.exists([]).unwrap();
        assert!(exists);

        // Verify metrics table exists
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='metrics'")
            .unwrap();
        let exists = stmt.exists([]).unwrap();
        assert!(exists);

        // Verify purge_history table exists
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='purge_history'")
            .unwrap();
        let exists = stmt.exists([]).unwrap();
        assert!(exists);

        // Verify FTS5 table exists
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='logs_fts'")
            .unwrap();
        let exists = stmt.exists([]).unwrap();
        assert!(exists);
    }

    #[test]
    fn test_indexes_created() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();

        // Verify at least one index exists for logs
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='logs'")
            .unwrap();
        let count: i32 = stmt.query_map([], |_| Ok(1)).unwrap().count() as i32;
        assert!(count > 0);
    }

    #[test]
    fn test_wide_window_indexes_created() {
        // #192: the two window indexes that the session-quality and
        // reasoning-share reports depend on must be created by the schema.
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' \
                 AND name IN ('idx_spans_session_window', 'idx_spans_reasoning_tokens')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_fts5_triggers_created() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();

        // Verify FTS5 triggers exist
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='trigger' AND name LIKE 'logs_fts_%'",
            )
            .unwrap();
        let count: i32 = stmt.query_map([], |_| Ok(1)).unwrap().count() as i32;
        assert_eq!(count, 3); // insert, delete, update triggers
    }

    // ── #251 v3/v4 migration hardening ────────────────────────────────────

    /// Production crash loop (2026-09-18): the v3 backfill was a plain
    /// INSERT while metric_latest's write trigger was already active — a
    /// concurrent ingest (or a re-run after a crashed backfill) collided
    /// with an existing row and bricked startup with
    /// "UNIQUE constraint failed: metric_latest.name" on every launchd
    /// restart. The merge backfill must tolerate trigger-maintained rows.
    #[test]
    fn test_schema_v3_backfill_is_idempotent_under_trigger_rows() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();

        // Ingest one metric row: the trigger arms a metric_latest row.
        conn.execute(
            "INSERT INTO metrics (name, metric_type, timestamp, value_int) \
             VALUES ('m.live', 0, 500, 1)",
            [],
        )
        .unwrap();

        // Simulate a database stuck pre-v3 with a trigger-maintained row
        // already present (the crash-loop state).
        conn.pragma_update(None, "user_version", 2).unwrap();

        initialize_schema(&conn).unwrap(); // must not UNIQUE-fail

        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 4, "v3 + v4 run to completion");

        // The merge keeps the trigger's row (same values here).
        let latest_ts: i64 = conn
            .query_row(
                "SELECT timestamp FROM metric_latest WHERE name = 'm.live'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(latest_ts, 500);
    }

    /// The v4 rollups backfill from pre-v4 history and their triggers
    /// keep them in sync: trace_latest is a MAX-merge (an older span must
    /// not rewind the newest start), metrics_daily_tool counts only the
    /// three tool scopes, and deletes decrement to row removal.
    #[test]
    fn test_schema_v4_rollups_backfill_and_stay_in_sync() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();

        // Build the pre-v4 state: drop the armed triggers (inserts below
        // are "history" the backfill, not the triggers, must count),
        // clear the rollups, stamp back to v3.
        conn.execute("DELETE FROM trace_latest", []).unwrap();
        conn.execute("DELETE FROM metrics_daily_tool", []).unwrap();
        for t in [
            "trg_trace_latest_ai",
            "trg_trace_latest_ad",
            "trg_metrics_daily_tool_ai",
            "trg_metrics_daily_tool_ad",
            "trg_spans_daily_llm_ai",
            "trg_spans_daily_llm_ad",
        ] {
            conn.execute(&format!("DROP TRIGGER {t}"), []).unwrap();
        }
        conn.pragma_update(None, "user_version", 3).unwrap();

        // History: two traces; two claude_code + one opencode datapoints
        // on 2026-01-01/02; one non-tool-scope datapoint.
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time,
                                attributes, events, resource, status_code)
             VALUES ('t1', 'a', 's', 0, 100, 110, '{}', '[]', '{}', 0),
                    ('t1', 'b', 's', 0, 200, 210, '{}', '[]', '{}', 0),
                    ('t2', 'c', 's', 0, 300, 310, '{}', '[]', '{}', 0),
                    ('t3', 'x1', 's', 0, 100, 110, '{}', '[]', '{}', 0),
                    ('t3', 'x2', 's', 0, 200, 210, '{}', '[]', '{}', 0)",
            [],
        )
        .unwrap();
        let d1 = 1_767_225_600_000_000_000i64; // 2026-01-01 UTC
        let d2 = 1_767_312_000_000_000_000i64; // 2026-01-02 UTC
        for (ts, scope) in [
            (d1, r#"{"name":"com.anthropic.claude_code"}"#),
            (d1, r#"{"name":"com.anthropic.claude_code"}"#),
            (d2, r#"{"name":"com.opencode"}"#),
            (d1, r#"{"name":"other.scope"}"#),
        ] {
            conn.execute(
                "INSERT INTO metrics (name, metric_type, timestamp, value_int, scope)
                 VALUES ('m', 0, ?, 1, ?)",
                rusqlite::params![ts, scope],
            )
            .unwrap();
        }
        // LLM-span history for the token rollup (two m1 spans on day 1,
        // one m2 span on day 2, one corrupt-attribute span on day 2 that
        // matches the guard via its vendor name; scope unset → tool label
        // 'unknown').
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time,
                                attributes, events, resource, status_code)
             VALUES ('t1', 'llm1', 'x', 0, ?, ?+100,
                     '{\"gen_ai.system\":\"anthropic\",\"gen_ai.usage.input_tokens\":10,\
                       \"gen_ai.usage.output_tokens\":4,\"gen_ai.request.model\":\"m1\"}',
                     '[]', '{}', 0),
                    ('t1', 'llm2', 'x', 0, ?+200, ?+260,
                     '{\"gen_ai.system\":\"anthropic\",\"gen_ai.usage.input_tokens\":6,\
                       \"gen_ai.usage.output_tokens\":2,\"gen_ai.request.model\":\"m1\"}',
                     '[]', '{}', 0),
                    ('t2', 'llm3', 'x', 0, ?+400, ?+460,
                     '{\"gen_ai.system\":\"anthropic\",\"gen_ai.usage.input_tokens\":1,\
                       \"gen_ai.usage.output_tokens\":1,\"gen_ai.request.model\":\"m2\"}',
                     '[]', '{}', 0),
                    ('t2', 'llm4c', 'claude_code.llm_request', 0, ?+500, ?+560,
                     '{corrupt', '[]', '{}', 0)",
            rusqlite::params![d1, d1, d1, d1, d2, d2, d2, d2],
        )
        .unwrap();

        initialize_schema(&conn).unwrap();

        let t1: i64 = conn
            .query_row(
                "SELECT last_start_time FROM trace_latest WHERE trace_id = 't1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            t1,
            d1 + 200,
            "backfill keeps the newest span start (the llm2 span)"
        );
        let t2: i64 = conn
            .query_row(
                "SELECT last_start_time FROM trace_latest WHERE trace_id = 't2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            t2,
            d2 + 500,
            "t2's newest span is the corrupt-attribute span (still a span)"
        );

        let counts: Vec<(String, String, i64)> = conn
            .prepare("SELECT day, tool, datapoints FROM metrics_daily_tool ORDER BY day, tool")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert_eq!(
            counts,
            vec![
                ("2026-01-01".to_string(), "claude_code".to_string(), 2),
                ("2026-01-02".to_string(), "opencode".to_string(), 1),
            ],
            "non-tool-scope datapoints must not enter the rollup"
        );

        // LLM token rollup: backfill grouped the history per
        // (day, tool, model); the two m1 spans summed into one row.
        let llm: Vec<(String, String, i64, i64)> = conn
            .prepare(
                "SELECT day, model, input_tokens, output_tokens
                 FROM spans_daily_llm ORDER BY day, model",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert_eq!(
            llm,
            vec![
                ("2026-01-01".to_string(), "m1".to_string(), 16, 6),
                ("2026-01-02".to_string(), "m2".to_string(), 1, 1),
            ],
            "the token rollup must group and sum the LLM span history, \
             excluding the corrupt-attribute span"
        );

        // Triggers armed (on t3, whose history is small-value starts): a
        // newer span advances, an older span does not rewind, deleting the
        // newest recomputes.
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time,
                                attributes, events, resource, status_code)
             VALUES ('t3', 'd', 's', 0, 400, 410, '{}', '[]', '{}', 0),
                    ('t3', 'e', 's', 0, 50, 60, '{}', '[]', '{}', 0)",
            [],
        )
        .unwrap();
        let t3: i64 = conn
            .query_row(
                "SELECT last_start_time FROM trace_latest WHERE trace_id = 't3'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(t3, 400, "older span must not rewind the rollup");

        let newest_id: i64 = conn
            .query_row("SELECT id FROM spans WHERE span_id = 'd'", [], |r| r.get(0))
            .unwrap();
        conn.execute(
            "DELETE FROM spans WHERE id = ?1",
            rusqlite::params![newest_id],
        )
        .unwrap();
        let t3: i64 = conn
            .query_row(
                "SELECT last_start_time FROM trace_latest WHERE trace_id = 't3'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(t3, 200, "deleting the newest span recomputes the rollup");

        // Metrics rollup: insert increments; deleting the last datapoint
        // of a (day, tool) removes the row.
        let cc: i64 = conn
            .query_row(
                "SELECT datapoints FROM metrics_daily_tool \
                 WHERE day = '2026-01-01' AND tool = 'claude_code'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cc, 2);
        conn.execute(
            "INSERT INTO metrics (name, metric_type, timestamp, value_int, scope)
             VALUES ('m', 0, ?, 1, ?)",
            rusqlite::params![d1, r#"{"name":"com.anthropic.claude_code"}"#],
        )
        .unwrap();
        let cc: i64 = conn
            .query_row(
                "SELECT datapoints FROM metrics_daily_tool \
                 WHERE day = '2026-01-01' AND tool = 'claude_code'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cc, 3, "trigger insert increments the day count");

        let oc_id: i64 = conn
            .query_row(
                "SELECT id FROM metrics WHERE scope = ?1 ORDER BY id DESC LIMIT 1",
                [r#"{"name":"com.opencode"}"#],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "DELETE FROM metrics WHERE id = ?1",
            rusqlite::params![oc_id],
        )
        .unwrap();
        let gone: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM metrics_daily_tool \
                 WHERE day = '2026-01-02' AND tool = 'opencode'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            gone, 0,
            "the last datapoint of a (day, tool) removes the row"
        );

        // Token rollup trigger: a new LLM span increments the
        // (day, model) totals.
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time,
                                attributes, events, resource, status_code)
             VALUES ('t1', 'llm4', 'x', 0, ?, ?+100,
                     '{\"gen_ai.system\":\"anthropic\",\"gen_ai.usage.input_tokens\":5,\
                       \"gen_ai.usage.output_tokens\":3,\"gen_ai.request.model\":\"m1\"}',
                     '[]', '{}', 0)",
            rusqlite::params![d1, d1],
        )
        .unwrap();
        let (input, output): (i64, i64) = conn
            .query_row(
                "SELECT input_tokens, output_tokens FROM spans_daily_llm \
                 WHERE day = '2026-01-01' AND model = 'm1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            (input, output),
            (21, 9),
            "the trigger must add the new span's tokens"
        );

        // A corrupt-attribute span matching the guard via its vendor name
        // must insert cleanly (trigger body must not raise on malformed
        // JSON) and contribute nothing to the rollup.
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time,
                                attributes, events, resource, status_code)
             VALUES ('t1', 'llm5c', 'claude_code.llm_request', 0, ?, ?+100,
                     '{corrupt', '[]', '{}', 0)",
            rusqlite::params![d1, d1],
        )
        .unwrap();
        let (input, output): (i64, i64) = conn
            .query_row(
                "SELECT input_tokens, output_tokens FROM spans_daily_llm \
                 WHERE day = '2026-01-01' AND model = 'm1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            (input, output),
            (21, 9),
            "corrupt rows must not enter the rollup"
        );
    }
}
