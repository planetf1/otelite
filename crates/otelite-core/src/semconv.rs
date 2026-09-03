//! GenAI semantic-convention attribute vocabulary.
//!
//! Instrumentations from different LLM frameworks use different attribute names
//! for the same concept. Each constant below is the authoritative priority-ordered
//! list of aliases for one concept. Analytics code projects these lists into SQL
//! COALESCE fragments via [`coalesce_extract`] / [`coalesce_extract_cast`].
//!
//! Adding a new framework's aliases means appending to the relevant list here —
//! no SQL needs to change.
//!
//! Priority order within each list: OTel GenAI standard → OpenLLMetry/OpenInference
//! (`llm.*`) → raw vendor names → Claude Code flat names.

/// Prompt / input tokens.
pub const INPUT_TOKEN_KEYS: &[&str] = &[
    "gen_ai.usage.input_tokens",
    "gen_ai.usage.prompt_tokens",
    "llm.usage.prompt_tokens",
    "llm.token_count.prompt",
    "prompt_tokens",
    "input_tokens",
];

/// Completion / output tokens.
pub const OUTPUT_TOKEN_KEYS: &[&str] = &[
    "gen_ai.usage.output_tokens",
    "gen_ai.usage.completion_tokens",
    "llm.usage.completion_tokens",
    "llm.token_count.completion",
    "completion_tokens",
    "output_tokens",
];

/// Tokens written into the prompt cache on this request.
pub const CACHE_CREATION_TOKEN_KEYS: &[&str] = &[
    "gen_ai.usage.cache_creation.input_tokens",
    // Codex reports cache writes under the `cache_write` dotted form
    // (verified against live codex_cli_rs spans).
    "gen_ai.usage.cache_write.input_tokens",
    "gen_ai.usage.cache_creation_input_tokens",
    "gen_ai.usage.cache_creation_tokens",
    "cache_creation_input_tokens",
    "cache_creation_tokens",
];

/// Tokens read from the prompt cache on this request.
pub const CACHE_READ_TOKEN_KEYS: &[&str] = &[
    "gen_ai.usage.cache_read.input_tokens",
    "gen_ai.usage.cache_read_input_tokens",
    "gen_ai.usage.cache_read_tokens",
    "cache_read_input_tokens",
    "cache_read_tokens",
];

/// Reasoning / thinking token count attribute key.
///
/// Emitted by the opencode-plugin-otel on every LLM span when the model
/// performs extended thinking (`gen_ai.usage.reasoning_tokens`), and by
/// the pi harness (`gen_ai.usage.reasoning_tokens`).
pub const REASONING_TOKEN_KEYS: &[&str] = &["gen_ai.usage.reasoning_tokens"];

/// Model identifier.
pub const MODEL_KEYS: &[&str] = &[
    "gen_ai.request.model",
    "gen_ai.response.model",
    "llm.request.model",
    "llm.model_name",
    "model",
];

/// Request model identifier.
///
/// Capability reports must not substitute a response model, because providers
/// can route a request to a different serving model.
pub const REQUEST_MODEL_KEYS: &[&str] = &[
    "gen_ai.request.model",
    "llm.request.model",
    "llm.model_name",
    "model",
];

/// Response model identifier (the model that actually served the response).
///
/// Providers may route a request to a different serving model, so this is
/// exposed separately for rerouting analysis and must never substitute for
/// the request model in model identity.
pub const RESPONSE_MODEL_KEYS: &[&str] = &["gen_ai.response.model", "llm.response.model"];

/// Provider / system (openai, anthropic, bedrock, ...).
pub const SYSTEM_KEYS: &[&str] = &[
    "gen_ai.provider.name",
    "gen_ai.system",
    "llm.system",
    "llm.vendor",
];

/// Attributes whose presence identifies a span as a GenAI / LLM call.
/// Used as a WHERE-clause guard for analytics that only apply to LLM spans.
pub const LLM_SPAN_MARKER_KEYS: &[&str] = &[
    "gen_ai.system",
    "gen_ai.provider.name",
    "llm.system",
    "llm.vendor",
    "llm.request.model",
];

/// OpenInference span-kind values that count as LLM activity for analytics.
pub const OPENINFERENCE_LLM_KINDS: &[&str] = &["LLM", "EMBEDDING"];

/// Span name prefixes for instrumentations that don't use the standard GenAI
/// attribute markers. Each entry is used as a LIKE pattern (`<prefix>%`).
///
/// Claude Code emits `claude_code.llm_request` spans with flat `model`,
/// `input_tokens`, `output_tokens` attributes but no `gen_ai.*` / `llm.*`
/// marker attributes, so the normal guard misses them entirely.
pub const VENDOR_SPAN_NAME_PREFIXES: &[&str] = &[LLM_REQUEST_SPAN_NAME];

/// Codex CLI's span for one completed model sampling request.
///
/// Codex adds its flat `model` attribute to several internal spans in a turn.
/// Only this span represents one model call; its nested
/// `model_client.stream_responses_api` span is transport timing, not another
/// request. Codex currently does not emit token usage or request-outcome
/// attributes, so it is only included in request-count and latency analytics.
pub const CODEX_LLM_REQUEST_SPAN_NAME: &str = "run_sampling_request";
pub const CODEX_OTEL_SCOPE_NAME: &str = "codex_cli_rs";

/// Codex CLI's span for one sampling-request response handling pass.
///
/// Carries `codex.request.reasoning_effort` and (on a subset of spans)
/// `codex.usage.reasoning_output_tokens`, but **no model attribute** — codex
/// does not attach the model to its spans (verified on the live DB 2026-08-27:
/// 0 of ~1M spans carry a model), so effort cannot be attributed per model.
pub const CODEX_HANDLE_RESPONSES_SPAN_NAME: &str = "handle_responses";

/// Codex attribute carrying the reasoning effort of a sampling request
/// (values observed: low, medium, high, xhigh).
pub const CODEX_REASONING_EFFORT_KEY: &str = "codex.request.reasoning_effort";

/// Codex attribute carrying reasoning-output tokens for one sampling request
/// (string-valued, present on a subset of spans).
pub const CODEX_REASONING_OUTPUT_TOKENS_KEY: &str = "codex.usage.reasoning_output_tokens";

/// Codex attribute carrying the working-directory path for a sampling request.
/// Present on every `run_sampling_request` span; used for project-level rollup.
/// Value: absolute path string, e.g. `/Users/jonesn/src/mellea`.
pub const CODEX_CWD_KEY: &str = "cwd";

/// Codex attribute carrying CPU-busy nanoseconds for a sampling request turn.
/// The complement of `CODEX_IDLE_NS_KEY`; together they decompose total
/// turn duration into model-wait time vs. active processing time.
pub const CODEX_BUSY_NS_KEY: &str = "busy_ns";

/// Codex attribute carrying idle (model-wait) nanoseconds for a sampling
/// request turn.
pub const CODEX_IDLE_NS_KEY: &str = "idle_ns";

/// Attribute carrying the agent session identifier (Claude Code `session.id`).
pub const SESSION_ID_KEY: &str = "session.id";

/// Claude Code attribute indicating thinking/effort speed mode of a request.
/// Values observed: "normal", "extended".
/// Claude Code only; not part of OTel GenAI semconv (as of 2026-09).
pub const SPEED_KEY: &str = "speed";

/// Span name for one Claude Code LLM request.
pub const LLM_REQUEST_SPAN_NAME: &str = "claude_code.llm_request";

/// Prefix of Claude Code tool span names (`claude_code.tool.Bash`, ...).
pub const TOOL_SPAN_NAME_PREFIX: &str = "claude_code.tool";

/// Span name for one Claude Code tool execution.
pub const TOOL_EXECUTION_SPAN_NAME: &str = "claude_code.tool.execution";

/// Span name for one Claude Code tool-approval prompt.
pub const TOOL_APPROVAL_SPAN_NAME: &str = "claude_code.tool.blocked_on_user";

/// Log `body` value carrying a raw Claude Code API response body.
pub const API_RESPONSE_BODY_LOG_BODY: &str = "claude_code.api_response_body";

/// Attribute keys that name the tool a span executed, in priority order.
pub const TOOL_NAME_KEYS: &[&str] = &["gen_ai.tool.name", "tool.name", "tool_name"];

/// Attribute carrying a singular LLM finish reason.
pub const FINISH_REASON_KEY: &str = "gen_ai.response.finish_reason";

/// Attribute carrying a plural LLM finish-reason array.
pub const FINISH_REASONS_KEY: &str = "gen_ai.response.finish_reasons";

/// Build a `COALESCE(json_extract(col, '$."k1"'), ...)` expression over `keys`.
pub fn coalesce_extract(attributes_col: &str, keys: &[&str]) -> String {
    coalesce_inner(attributes_col, keys, None)
}

/// Build a `COALESCE(CAST(json_extract(col, '$."k1"') AS <cast>), ...)` expression.
pub fn coalesce_extract_cast(attributes_col: &str, keys: &[&str], cast: &str) -> String {
    coalesce_inner(attributes_col, keys, Some(cast))
}

fn coalesce_inner(attributes_col: &str, keys: &[&str], cast: Option<&str>) -> String {
    assert!(!keys.is_empty(), "coalesce over empty key list");
    let parts: Vec<String> = keys
        .iter()
        .map(|k| match cast {
            Some(c) => format!(
                "CAST(json_extract({col}, '$.\"{k}\"') AS {c})",
                col = attributes_col,
                k = k,
                c = c
            ),
            None => format!(
                "json_extract({col}, '$.\"{k}\"')",
                col = attributes_col,
                k = k
            ),
        })
        .collect();
    format!("COALESCE({})", parts.join(", "))
}

/// Parenthesised OR-chain that matches spans with standard LLM telemetry.
///
/// Includes the OpenInference `openinference.span.kind` IN (...) clause and
/// vendor-specific span-name prefix patterns. This deliberately excludes Codex
/// sampling spans because they lack token and request-outcome attributes.
///
/// Every `json_extract` clause is gated by `json_valid` so the guard is
/// *total*: it returns false (instead of raising "malformed JSON") for rows
/// whose `attributes` text is corrupt or NULL. This matters for two reasons:
/// - the guard is used as a partial-index predicate, which SQLite evaluates
///   on every INSERT; a raising predicate would reject valid telemetry
///   batches that contain a single corrupt span;
/// - without it, one corrupt row inside a time window would make every GenAI
///   analytics query over that window fail.
pub fn llm_span_guard(attributes_col: &str) -> String {
    let mut clauses: Vec<String> = LLM_SPAN_MARKER_KEYS
        .iter()
        .map(|k| {
            format!(
                "(json_valid({col}) AND json_extract({col}, '$.\"{k}\"') IS NOT NULL)",
                col = attributes_col,
                k = k
            )
        })
        .collect();
    let kinds = OPENINFERENCE_LLM_KINDS
        .iter()
        .map(|k| format!("'{}'", k))
        .collect::<Vec<_>>()
        .join(", ");
    clauses.push(format!(
        "(json_valid({col}) AND json_extract({col}, '$.\"openinference.span.kind\"') IN ({kinds}))",
        col = attributes_col,
        kinds = kinds
    ));
    for prefix in VENDOR_SPAN_NAME_PREFIXES {
        clauses.push(format!("name LIKE '{}%'", prefix));
    }
    format!("({})", clauses.join(" OR "))
}

/// LLM guard for analytics which only require a completed request and duration.
///
/// Extends [`llm_span_guard`] with Codex's native completed-sampling span while
/// excluding nested transport spans.
pub fn request_span_guard(attributes_col: &str) -> String {
    let llm_guard = llm_span_guard(attributes_col);
    let codex_guard = format!(
        "(name = '{span_name}' \
          AND json_valid({col}) \
          AND json_extract({col}, '$.\"model\"') IS NOT NULL \
          AND json_extract({col}, '$.\"otel.scope.name\"') = '{scope_name}')",
        span_name = CODEX_LLM_REQUEST_SPAN_NAME,
        col = attributes_col,
        scope_name = CODEX_OTEL_SCOPE_NAME,
    );
    format!("({llm_guard} OR {codex_guard})")
}

/// Total-form SQL expression for the session-id attribute lookup.
///
/// A bare `json_extract(attributes, '$."session.id"')` raises on corrupt JSON;
/// this `json_valid`-gated form returns NULL instead. The same expression
/// defines `idx_spans_session_id`, so `session.id` predicates seek the index
/// instead of scanning the table.
pub fn session_id_expr(attributes_col: &str) -> String {
    format!(
        "CASE WHEN json_valid({col}) THEN json_extract({col}, '$.\"{key}\"') END",
        col = attributes_col,
        key = SESSION_ID_KEY
    )
}

/// Partial-index predicate pairing [`session_id_expr`]: only spans that carry
/// a session id are indexed.
///
/// A query on the expression must also carry this predicate (as conjuncts)
/// for SQLite to consider the partial index at all — the equality itself
/// implies it, so adding it is semantically redundant but planner-required.
pub fn session_id_index_predicate(attributes_col: &str) -> String {
    format!(
        "json_valid({col}) AND json_extract({col}, '$.\"{key}\"') IS NOT NULL",
        col = attributes_col,
        key = SESSION_ID_KEY
    )
}

/// Guard matching spans that carry a finish reason, singular attribute or
/// plural array attribute. Total (every `json_extract` is `json_valid`-gated)
/// for partial-index use.
pub fn finish_reason_guard(attributes_col: &str) -> String {
    format!(
        "((json_valid({col}) AND json_extract({col}, '$.\"{s}\"') IS NOT NULL) \
          OR (json_valid({col}) AND json_extract({col}, '$.\"{p}\"') IS NOT NULL))",
        col = attributes_col,
        s = FINISH_REASON_KEY,
        p = FINISH_REASONS_KEY
    )
}

/// Guard matching spans that identify a tool either by a tool-name attribute
/// or by a Claude Code tool span name. Total for partial-index use (the
/// attribute branch is `json_valid`-gated; the name branch needs no JSON).
pub fn tool_span_guard(attributes_col: &str) -> String {
    let attr_clauses = TOOL_NAME_KEYS
        .iter()
        .map(|k| {
            format!(
                "json_extract({col}, '$.\"{k}\"') IS NOT NULL",
                col = attributes_col,
                k = k
            )
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    format!(
        "((json_valid({col}) AND ({attrs})) \
          OR (name LIKE '{prefix}%' AND name != '{prefix}'))",
        col = attributes_col,
        attrs = attr_clauses,
        prefix = TOOL_SPAN_NAME_PREFIX
    )
}

/// Guard matching retrieval spans: OpenInference `RETRIEVER` span kind or a
/// `retrieval.query` attribute. Total for partial-index use.
pub fn retrieval_span_guard(attributes_col: &str) -> String {
    format!(
        "(json_valid({col}) AND (json_extract({col}, '$.\"openinference.span.kind\"') = 'RETRIEVER' \
          OR json_extract({col}, '$.\"{k}\"') IS NOT NULL))",
        col = attributes_col,
        k = "retrieval.query"
    )
}

// ── Agent-emitted metric names and label paths ───────────────────────────────
// Metric names as emitted by the agent OTel SDKs (external instruments, not
// otelite's own). Label paths are JSON paths into the metrics `attributes`
// column.

/// Metric names emitted by agent harnesses.
pub mod metric_names {
    /// Cumulative per-(session, model, type, project) token counter.
    pub const OPENCODE_TOKEN_USAGE: &str = "opencode.token.usage";
    /// Per-(session, model, provider, agent, project) usage marker.
    pub const OPENCODE_MODEL_USAGE: &str = "opencode.model.usage";
    /// Per-session start marker (`value_int = 1`); the distinct
    /// `session.id` values in a window are that window's sessions.
    pub const OPENCODE_SESSION_COUNT: &str = "opencode.session.count";
    /// Cumulative per-session cost histogram; `value_histogram[1]` is the
    /// session's total cost (USD) so far.
    pub const OPENCODE_SESSION_COST_TOTAL: &str = "opencode.session.cost.total";
    /// Cumulative per-session duration histogram; `value_histogram[1]` is
    /// the session's total duration (milliseconds) so far.
    pub const OPENCODE_SESSION_DURATION: &str = "opencode.session.duration";
    /// Cumulative per-session token histogram; `value_histogram[1]` is the
    /// session's total tokens so far.
    pub const OPENCODE_SESSION_TOKEN_TOTAL: &str = "opencode.session.token.total";
    /// Cumulative per-(session, tool_name) tool-duration histogram;
    /// `value_histogram[0]` is the session's total tool calls so far.
    pub const OPENCODE_TOOL_DURATION: &str = "opencode.tool.duration";
    /// Cumulative per-session retry counter.
    pub const OPENCODE_RETRY_COUNT: &str = "opencode.retry.count";
    /// Per-turn token histogram from the codex CLI; `value_histogram[1]` is
    /// the turn's token count for the `token_type` label.
    pub const CODEX_TURN_TOKEN_USAGE: &str = "codex.turn.token_usage";
    /// Per-turn time-to-first-token histogram from the codex CLI
    /// (`value_histogram` in ms); the only TTFT source for codex, whose
    /// `run_sampling_request` spans carry no TTFT attribute.
    pub const CODEX_TURN_TTFT: &str = "codex.turn.ttft.duration_ms";
    /// Per-turn end-to-end duration histogram from the codex CLI (ms).
    pub const CODEX_TURN_E2E_DURATION: &str = "codex.turn.e2e_duration_ms";
    /// Per-event thread-start marker; `value_int` is the number of threads
    /// started. `session_source = 'cli'` rows are user-initiated codex
    /// sessions (sub-agent threads carry their own `session_source`).
    pub const CODEX_THREAD_STARTED: &str = "codex.thread.started";
    /// Per-event tool call (`value_int` calls per row).
    pub const CODEX_TOOL_CALL: &str = "codex.tool.call";
    /// Per-event API request (`value_int` requests per row).
    pub const CODEX_API_REQUEST: &str = "codex.api_request";
    /// Per-event token usage from Claude Code (`value_int` tokens per row),
    /// keyed by `session.id`, `model`, `type`.
    pub const CLAUDE_CODE_TOKEN_USAGE: &str = "claude_code.token.usage";
    /// Per-session start marker (`value_int = 1`); the distinct
    /// `session.id` values in a window are that window's sessions.
    pub const CLAUDE_CODE_SESSION_COUNT: &str = "claude_code.session.count";
    /// Per-session commit counter from Claude Code.
    pub const CLAUDE_CODE_COMMIT_COUNT: &str = "claude_code.commit.count";
    /// Per-session PR counter from Claude Code.
    pub const CLAUDE_CODE_PR_COUNT: &str = "claude_code.pull_request.count";
    /// Cumulative per-session lines-of-code gauge from Claude Code.
    pub const CLAUDE_CODE_LINES_OF_CODE: &str = "claude_code.lines_of_code.count";
    /// Cumulative per-session lines-of-code gauge from opencode.
    pub const OPENCODE_LINES_OF_CODE: &str = "opencode.lines_of_code.total";
    /// Guardian review event from Codex (value_int = 1 per review).
    pub const CODEX_GUARDIAN_REVIEW: &str = "codex.guardian.review";
    /// Multi-agent spawn event from Codex (value_int = 1 per spawn).
    pub const CODEX_MULTI_AGENT_SPAWN: &str = "codex.multi_agent.spawn";
    /// Multi-agent resume event from Codex (value_int = 1 per resume).
    pub const CODEX_MULTI_AGENT_RESUME: &str = "codex.multi_agent.resume";
    /// Per-event MCP tool call from Codex (value_int = 1 per call).
    pub const CODEX_MCP_CALL: &str = "codex.mcp.call";
    /// Hook invocation duration histogram from Codex.
    pub const CODEX_HOOKS_RUN_DURATION: &str = "codex.hooks.run.duration_ms";
}

/// Attribute label paths for agent metrics.
pub mod metric_labels {
    /// Sub-agent role (opencode `agent` label, e.g. "orchestrator").
    pub const AGENT: &str = "$.agent";
    pub const MODEL: &str = "$.model";
    /// Token category (see [`opencode_token_types`]).
    pub const TYPE: &str = "$.type";
    pub const SESSION_ID: &str = "$.\"session.id\"";
    /// Per-turn token category (see [`codex_token_types`]).
    pub const TOKEN_TYPE: &str = "$.token_type";
    /// Tool name on `opencode.tool.duration`.
    pub const TOOL_NAME: &str = "$.tool_name";
    /// "true"/"false" on `opencode.session.count`; top-level sessions are
    /// everything that is not "true".
    pub const IS_SUBAGENT: &str = "$.is_subagent";
    /// Flat key (quoted JSON path): opencode's per-project identifier on
    /// `token.usage`, `session.cost.total`, `session.count`, `model.usage`
    /// and `retry.count`. Absent on codex/claude metrics.
    pub const PROJECT_ID: &str = "$.\"project.id\"";
    /// "cli" for user-initiated codex runs, "subagent_*" otherwise.
    pub const SESSION_SOURCE: &str = "$.session_source";
    /// "true"/"false" on `codex.api_request`.
    pub const SUCCESS: &str = "$.success";
    /// Effort label on `claude_code.token.usage` (low/medium/high/xhigh).
    pub const EFFORT: &str = "$.effort";
    /// query_source label on `claude_code.token.usage` (main/sub-agent).
    pub const QUERY_SOURCE: &str = "$.query_source";
    /// Lines-of-code type label (added/removed).
    pub const LOC_TYPE: &str = "$.type";
    /// Guardian review decision label (approved/denied).
    pub const DECISION: &str = "$.decision";
    /// Guardian risk level label (low/medium/high/none).
    pub const RISK_LEVEL: &str = "$.risk_level";
    /// Guardian action label (unified_exec/apply_patch/mcp_tool_call).
    pub const ACTION: &str = "$.action";
    /// Multi-agent role label (deep_reviewer, worker, etc.).
    pub const ROLE: &str = "$.role";
    /// MCP server name label.
    pub const MCP_SERVER: &str = "$.server";
    /// MCP tool name label.
    pub const MCP_TOOL: &str = "$.tool";
    /// MCP/API call status label ("ok" / "error").
    pub const STATUS: &str = "$.status";
}

/// `type` label values on `opencode.token.usage`.
pub mod opencode_token_types {
    pub const INPUT: &str = "input";
    pub const OUTPUT: &str = "output";
    pub const REASONING: &str = "reasoning";
    pub const CACHE_READ: &str = "cacheRead";
    pub const CACHE_WRITE: &str = "cacheCreation";
}

/// `token_type` label values on `codex.turn.token_usage`. `total` is the sum
/// of the other categories and must never be counted (double-counting).
pub mod codex_token_types {
    pub const INPUT: &str = "input";
    pub const OUTPUT: &str = "output";
    pub const REASONING: &str = "reasoning_output";
    pub const CACHE_READ: &str = "cached_input";
    pub const CACHE_WRITE: &str = "cache_write_input";
}

/// Canonical harness names used in per-agent rollup responses (the "agent"
/// field identifies the harness, not a sub-agent role).
pub mod agent_names {
    pub const OPENCODE: &str = "opencode";
    pub const CODEX: &str = "codex";
    pub const CLAUDE: &str = "claude";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesce_extract_formats_keys() {
        let sql = coalesce_extract("attributes", &["a.b", "c"]);
        assert_eq!(
            sql,
            "COALESCE(json_extract(attributes, '$.\"a.b\"'), json_extract(attributes, '$.\"c\"'))"
        );
    }

    #[test]
    fn coalesce_extract_cast_wraps_each_clause() {
        let sql = coalesce_extract_cast("attributes", INPUT_TOKEN_KEYS, "INTEGER");
        assert!(sql.starts_with("COALESCE(CAST(json_extract(attributes, "));
        assert!(sql.contains("gen_ai.usage.input_tokens"));
        assert!(sql.contains("input_tokens"));
        assert!(sql.contains("AS INTEGER"));
    }

    #[test]
    fn llm_span_guard_includes_openinference_kinds() {
        let sql = llm_span_guard("attributes");
        assert!(sql.contains("gen_ai.system"));
        assert!(sql.contains("llm.request.model"));
        assert!(sql.contains("openinference.span.kind"));
        assert!(sql.contains("'LLM'"));
        assert!(sql.contains("'EMBEDDING'"));
        assert!(sql.starts_with('(') && sql.ends_with(')'));
    }

    #[test]
    fn llm_span_guard_includes_vendor_span_name_prefixes() {
        let sql = llm_span_guard("attributes");
        // Claude Code spans must be matched even without gen_ai.* attributes.
        assert!(
            sql.contains("name LIKE 'claude_code.llm_request%'"),
            "expected claude_code.llm_request LIKE clause in: {sql}"
        );
    }

    #[test]
    fn request_span_guard_includes_strict_codex_request_signature() {
        let sql = request_span_guard("attributes");
        assert!(
            sql.contains("name = 'run_sampling_request'"),
            "expected Codex request span clause in: {sql}"
        );
        assert!(
            sql.contains("json_extract(attributes, '$.\"model\"') IS NOT NULL"),
            "expected Codex model requirement in: {sql}"
        );
        assert!(
            sql.contains("json_extract(attributes, '$.\"otel.scope.name\"') = 'codex_cli_rs'"),
            "expected Codex scope requirement in: {sql}"
        );
        assert!(
            !llm_span_guard("attributes").contains("run_sampling_request"),
            "standard LLM guard must exclude Codex spans without token or outcome attributes"
        );
    }

    #[test]
    fn session_id_expr_is_total_and_predicate_pairs() {
        let expr = session_id_expr("attributes");
        assert_eq!(
            expr,
            "CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.\"session.id\"') END"
        );
        let pred = session_id_index_predicate("attributes");
        assert_eq!(
            pred,
            "json_valid(attributes) AND json_extract(attributes, '$.\"session.id\"') IS NOT NULL"
        );
    }

    #[test]
    fn finish_reason_guard_covers_singular_and_plural() {
        let sql = finish_reason_guard("attributes");
        assert!(sql.contains(
            "json_extract(attributes, '$.\"gen_ai.response.finish_reason\"') IS NOT NULL"
        ));
        assert!(sql.contains(
            "json_extract(attributes, '$.\"gen_ai.response.finish_reasons\"') IS NOT NULL"
        ));
        // Every extract is gated.
        assert_eq!(sql.matches("json_extract").count(), 2);
        assert_eq!(sql.matches("json_valid").count(), 2);
    }

    #[test]
    fn tool_span_guard_covers_attributes_and_names() {
        let sql = tool_span_guard("attributes");
        for key in TOOL_NAME_KEYS {
            assert!(
                sql.contains(&format!(
                    "json_extract(attributes, '$.\"{key}\"') IS NOT NULL"
                )),
                "expected {key} clause in: {sql}"
            );
        }
        assert!(
            sql.contains("name LIKE 'claude_code.tool%' AND name != 'claude_code.tool'"),
            "expected tool name prefix clause in: {sql}"
        );
    }

    #[test]
    fn retrieval_span_guard_covers_kind_and_query() {
        let sql = retrieval_span_guard("attributes");
        assert!(
            sql.contains("json_extract(attributes, '$.\"openinference.span.kind\"') = 'RETRIEVER'")
        );
        assert!(sql.contains("json_extract(attributes, '$.\"retrieval.query\"') IS NOT NULL"));
        assert!(sql.contains("json_valid(attributes)"));
    }
}
