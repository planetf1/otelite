//! Token usage command for GenAI/LLM spans

use crate::error::{Error, Result};
use clap::Args;
use comfy_table::{presets::UTF8_FULL, Cell, Color, ContentArrangement, Table};
use otelite_core::filters::GenAiFilters;
use otelite_core::pricing::{PricingDatabase, TokenUsage};
use otelite_storage::StorageBackend;
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) fn validate_since(s: &str) -> std::result::Result<String, String> {
    let (digits, suffix) = s.split_at(s.len().saturating_sub(1));
    let valid_suffix = matches!(suffix, "h" | "d" | "m");
    let valid_digits = !digits.is_empty() && digits.parse::<u64>().is_ok();
    if valid_suffix && valid_digits {
        Ok(s.to_string())
    } else {
        Err(format!(
            "Invalid time duration '{}'. Use a number followed by 'h' (hours), 'd' (days), or 'm' (minutes), e.g. '1h', '24h', '7d', '30d'",
            s
        ))
    }
}

/// Display token usage statistics for GenAI/LLM spans
#[derive(Debug, Args)]
pub struct UsageCommand {
    /// Time range to query (e.g., "1h", "24h", "7d", "30d")
    #[arg(
        long,
        default_value = "24h",
        value_parser = validate_since,
        conflicts_with_all = ["start", "end"]
    )]
    pub since: String,

    /// Exact start (overrides --since). `YYYY-MM-DD` (midnight UTC),
    /// `YYYY-MM-DDTHH:MM:SS` (UTC when no zone is given), or epoch
    /// seconds/nanoseconds
    #[arg(long, conflicts_with = "since")]
    pub start: Option<String>,

    /// Exact end (overrides --since); same formats as --start. Defaults
    /// to now when --start is given
    #[arg(long, conflicts_with = "since")]
    pub end: Option<String>,

    /// Filter by model. Repeatable. A value without `*` matches exactly;
    /// a value with `*` is a glob (`claude-opus-*`). Multiple values are
    /// ORed. Applied to every panel — summary totals included.
    #[arg(long)]
    pub model: Vec<String>,

    /// Filter by system/provider (e.g., "openai", "anthropic")
    #[arg(long)]
    pub system: Option<String>,

    /// Show detailed breakdown by model
    #[arg(long)]
    pub by_model: bool,

    /// Show detailed breakdown by system
    #[arg(long)]
    pub by_system: bool,

    /// Show the top N most expensive individual LLM calls
    #[arg(long, value_name = "N")]
    pub top: Option<usize>,

    /// Show a cost breakdown aggregated by session ID
    #[arg(long)]
    pub by_session: bool,

    /// Show per-model latency stats with derived token rate, context size, and output/input ratio
    #[arg(long)]
    pub latency: bool,

    /// Show latency trend over time (min/avg/p95/max per time bucket, grouped by model)
    #[arg(long)]
    pub latency_series: bool,

    /// Bucket size in seconds for time-series views (default 3600 = 1 hour)
    #[arg(long, default_value = "3600")]
    pub bucket_secs: u64,

    /// Show call volume trend over time (requests per time bucket, grouped by model)
    #[arg(long)]
    pub calls: bool,

    /// Show per-model truncation rate (finish_reason = max_tokens / length)
    #[arg(long)]
    pub truncation: bool,

    /// Show per-model cache hit rate (cache_read / (cache_read + input))
    #[arg(long)]
    pub cache_rate: bool,

    /// Show request parameter distribution (temperature, max_tokens)
    #[arg(long)]
    pub request_params: bool,

    /// Show conversation depth statistics (turn-count distribution)
    #[arg(long)]
    pub conv_depth: bool,

    /// Show per-tool usage with success rate
    #[arg(long)]
    pub tools: bool,

    /// Show error type breakdown (rate_limit, timeout, context_length, ...)
    #[arg(long)]
    pub error_types: bool,

    /// Show request→response model pairs (detect silent provider rerouting)
    #[arg(long)]
    pub model_drift: bool,

    /// Show latency broken down by input-token context size bin (and model)
    #[arg(long)]
    pub latency_context: bool,

    /// Show bucketed latency percentiles p50/p90/p95/p99 over time
    /// (duration and/or ttft; --model filters the cohort)
    #[arg(long)]
    pub latency_percentiles: bool,

    /// Bucket the latency percentiles by calendar day in --timezone
    /// (DST-aware 23/25-hour days, empty days shown with no percentiles)
    /// instead of the fixed --bucket-secs grid
    #[arg(long)]
    pub calendar_day: bool,

    /// IANA timezone for --calendar-day (e.g. Europe/London); default UTC
    #[arg(long, requires = "calendar_day")]
    pub timezone: Option<String>,

    /// Show throughput columns (tok/s p10/p50/p90 + eligible count) in
    /// --latency-series; --latency and --latency-percentiles show them by
    /// default
    #[arg(long)]
    pub throughput: bool,

    /// Show tool approval/rejection decision summary (Claude Code)
    #[arg(long)]
    pub tool_approvals: bool,

    /// Show Claude Code stop_reason distribution (tool_use / end_turn / …)
    #[arg(long)]
    pub stop_reasons: bool,

    /// Show token usage grouped by llm_request.context (interaction / sub_agent / …)
    #[arg(long)]
    pub context_split: bool,

    /// Show top tool errors from failed tool executions (Claude Code)
    #[arg(long, value_name = "N", default_missing_value = "20")]
    pub tool_errors: Option<usize>,

    /// Show hour-of-day activity distribution (0–23 UTC, LLM + tool calls)
    #[arg(long)]
    pub hour_of_day: bool,

    /// Show cost and token attribution per sub-agent role (opencode `agent` label)
    #[arg(long)]
    pub agent_roles: bool,

    /// Show session × model cross-tab: which sessions used which models and at what cost (#115)
    #[arg(long)]
    pub session_models: bool,

    /// Show speed/effort attribute distribution across Claude Code LLM spans (#114)
    #[arg(long)]
    pub speed: bool,
}

// ── serialisable output types (used for --format json) ───────────────────────

#[derive(serde::Serialize)]
struct ModelRow {
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    requests: usize,
    cost: Option<f64>,
    cost_source: Option<String>,
    /// Dominant differing response model (silent provider rerouting), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    response_model: Option<String>,
    /// Calls whose response model differed from the request model.
    rerouted_count: usize,
}

#[derive(serde::Serialize)]
struct SystemRow {
    system: String,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    requests: usize,
    cost: Option<f64>,
    cost_source: Option<String>,
}

#[derive(serde::Serialize, Default)]
struct SessionRow {
    session_id: String,
    requests: usize,
    input_tokens: u64,
    output_tokens: u64,
    cost: f64,
}

#[derive(serde::Serialize)]
struct UsageOutput {
    summary: otelite_core::api::TokenUsageSummary,
    by_model: Vec<ModelRow>,
    by_system: Vec<SystemRow>,
    by_session: Option<Vec<SessionRow>>,
    top_spans: Option<Vec<otelite_core::api::TopSpan>>,
    pricing_source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency_stats: Option<Vec<otelite_core::api::LatencyStats>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    truncation_rate: Option<Vec<otelite_core::api::TruncationRateByModel>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_hit_rate: Option<Vec<otelite_core::api::CacheHitRateByModel>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_param_profile: Option<otelite_core::api::RequestParamProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    conversation_depth: Option<otelite_core::api::ConversationDepthStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_usage: Option<Vec<otelite_core::api::ToolUsage>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_types: Option<Vec<otelite_core::api::ErrorTypeBreakdown>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_drift: Option<Vec<otelite_core::api::ModelDriftPair>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency_series: Option<Vec<otelite_core::api::LatencySeriesPoint>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency_context: Option<Vec<otelite_core::api::LatencyByContextBin>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency_percentiles: Option<otelite_core::api::LatencyPercentilesResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_approvals: Option<otelite_core::api::ToolApprovalStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_reasons: Option<Vec<otelite_core::api::StopReasonCount>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_split: Option<Vec<otelite_core::api::ContextTypeSplit>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_errors: Option<Vec<otelite_core::api::ToolErrorEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hour_of_day: Option<Vec<otelite_core::api::HourOfDayBucket>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    calls_series: Option<Vec<otelite_core::api::CallsSeriesPoint>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_roles: Option<otelite_core::api::AgentRolesResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_models: Option<otelite_core::api::SessionModelBreakdown>,
    #[serde(skip_serializing_if = "Option::is_none")]
    speed: Option<otelite_core::api::SpeedDistribution>,
}

// ── pricing fetch ─────────────────────────────────────────────────────────────

pub(crate) async fn fetch_pricing() -> PricingDatabase {
    const URL: &str = "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return PricingDatabase::empty(),
    };
    match client.get(URL).send().await {
        Ok(resp) if resp.status().is_success() => match resp.text().await {
            Ok(body) => PricingDatabase::from_litellm_json(&body)
                .unwrap_or_else(|_| PricingDatabase::empty()),
            Err(_) => PricingDatabase::empty(),
        },
        _ => PricingDatabase::empty(),
    }
}

// ── command implementation ────────────────────────────────────────────────────

impl UsageCommand {
    /// Execute the usage command
    pub async fn execute(
        &self,
        storage: Arc<dyn StorageBackend>,
        format: crate::config::OutputFormat,
    ) -> Result<()> {
        // Exact --start/--end win over the rolling --since window.
        let (start_time, end_time) = match (self.start.as_deref(), self.end.as_deref()) {
            (Some(start), end) => {
                let start = parse_cli_time(start)?;
                let end = match end {
                    Some(end) => parse_cli_time(end)?,
                    None => now_ns()?,
                };
                if start >= end {
                    return Err(Error::ApiError("--start must be before --end".to_string()));
                }
                (start, end)
            },
            (None, Some(_)) => return Err(Error::ApiError("--end requires --start".to_string())),
            (None, None) => parse_time_range(&self.since)?,
        };

        // One model cohort for every panel: summary totals and all detail
        // views are computed on the same filtered set (#142).
        let filters = build_model_filters(&self.model);

        let (summary, by_model_raw, by_system_raw) = storage
            .query_token_usage(Some(start_time), Some(end_time), &filters)
            .await
            .map_err(|e| Error::ApiError(format!("Failed to query token usage: {}", e)))?;

        let by_system_raw: Vec<otelite_core::api::SystemUsage> = if let Some(ref f) = self.system {
            by_system_raw
                .into_iter()
                .filter(|s| s.system.contains(f))
                .collect()
        } else {
            by_system_raw
        };

        let pricing_db = fetch_pricing().await;
        let pricing_source = if pricing_db.is_litellm() {
            "LiteLLM".to_string()
        } else {
            "fallback (hardcoded Claude rates)".to_string()
        };

        // Enrich model/system rows with cost. The pricing lookup walks
        // provider-prefixed LiteLLM keys, so the composite identity
        // (`provider/model`) resolves as precisely as the bare name did.
        let by_model: Vec<ModelRow> = by_model_raw
            .iter()
            .map(|m| {
                let usage = TokenUsage {
                    input: m.input_tokens,
                    output: m.output_tokens,
                    ..Default::default()
                };
                let cr = pricing_db.compute_cost(Some(&m.model), usage, None);
                ModelRow {
                    model: m.model.clone(),
                    input_tokens: m.input_tokens,
                    output_tokens: m.output_tokens,
                    total_tokens: m.input_tokens + m.output_tokens,
                    requests: m.requests,
                    cost: cr.cost,
                    cost_source: Some(cr.source.as_str().to_string()),
                    response_model: m.response_model.clone(),
                    rerouted_count: m.rerouted_count,
                }
            })
            .collect();

        let by_system: Vec<SystemRow> = by_system_raw
            .iter()
            .map(|s| {
                let usage = TokenUsage {
                    input: s.input_tokens,
                    output: s.output_tokens,
                    ..Default::default()
                };
                let cr = pricing_db.compute_cost(None, usage, Some(&s.system));
                SystemRow {
                    system: s.system.clone(),
                    input_tokens: s.input_tokens,
                    output_tokens: s.output_tokens,
                    total_tokens: s.input_tokens + s.output_tokens,
                    requests: s.requests,
                    cost: cr.cost,
                    cost_source: Some(cr.source.as_str().to_string()),
                }
            })
            .collect();

        // --top N
        let top_spans: Option<Vec<otelite_core::api::TopSpan>> = if let Some(n) = self.top {
            let mut spans = storage
                .query_top_spans(
                    Some(start_time),
                    Some(end_time),
                    &filters,
                    n,
                    otelite_core::api::TopSpanSort::TotalTokens,
                    false,
                )
                .await
                .map_err(|e| Error::ApiError(format!("Failed to query top spans: {}", e)))?;
            for span in &mut spans {
                let usage = TokenUsage {
                    input: span.input_tokens,
                    output: span.output_tokens,
                    cache_creation: span.cache_creation_tokens,
                    cache_read: span.cache_read_tokens,
                };
                let cr =
                    pricing_db.compute_cost(span.model.as_deref(), usage, span.system.as_deref());
                span.cost = cr.cost;
                span.cost_source = Some(cr.source.as_str().to_string());
                span.cost_reason = cr.reason;
            }
            spans.sort_by(|a, b| {
                b.cost
                    .unwrap_or(0.0)
                    .partial_cmp(&a.cost.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            Some(spans)
        } else {
            None
        };

        // --latency
        let latency_stats: Option<Vec<otelite_core::api::LatencyStats>> = if self.latency {
            Some(
                storage
                    .query_latency_stats(Some(start_time), Some(end_time), &filters)
                    .await
                    .map_err(|e| {
                        Error::ApiError(format!("Failed to query latency stats: {}", e))
                    })?,
            )
        } else {
            None
        };

        // --latency-series
        let latency_series: Option<Vec<otelite_core::api::LatencySeriesPoint>> =
            if self.latency_series {
                Some(
                    storage
                        .query_latency_series(
                            Some(start_time),
                            Some(end_time),
                            self.bucket_secs,
                            &filters,
                            false,
                            self.calendar_day
                                .then(|| self.timezone.as_deref().unwrap_or("UTC")),
                        )
                        .await
                        .map_err(|e| {
                            Error::ApiError(format!("Failed to query latency series: {}", e))
                        })?,
                )
            } else {
                None
            };

        // --truncation
        let truncation_rate: Option<Vec<otelite_core::api::TruncationRateByModel>> =
            if self.truncation {
                Some(
                    storage
                        .query_truncation_rate(Some(start_time), Some(end_time), &filters)
                        .await
                        .map_err(|e| {
                            Error::ApiError(format!("Failed to query truncation rate: {}", e))
                        })?,
                )
            } else {
                None
            };

        // --cache-rate
        let cache_hit_rate: Option<Vec<otelite_core::api::CacheHitRateByModel>> = if self.cache_rate
        {
            Some(
                storage
                    .query_cache_hit_rate(Some(start_time), Some(end_time), &filters)
                    .await
                    .map_err(|e| {
                        Error::ApiError(format!("Failed to query cache hit rate: {}", e))
                    })?,
            )
        } else {
            None
        };

        // --request-params
        let request_param_profile: Option<otelite_core::api::RequestParamProfile> =
            if self.request_params {
                Some(
                    storage
                        .query_request_param_profile(Some(start_time), Some(end_time), &filters)
                        .await
                        .map_err(|e| {
                            Error::ApiError(format!("Failed to query request param profile: {}", e))
                        })?,
                )
            } else {
                None
            };

        // --conv-depth
        let conversation_depth: Option<otelite_core::api::ConversationDepthStats> =
            if self.conv_depth {
                Some(
                    storage
                        .query_conversation_depth(Some(start_time), Some(end_time), &filters)
                        .await
                        .map_err(|e| {
                            Error::ApiError(format!("Failed to query conversation depth: {}", e))
                        })?,
                )
            } else {
                None
            };

        // --tools
        let tool_usage: Option<Vec<otelite_core::api::ToolUsage>> = if self.tools {
            Some(
                storage
                    .query_tool_usage(Some(start_time), Some(end_time), &filters, 50)
                    .await
                    .map_err(|e| Error::ApiError(format!("Failed to query tool usage: {}", e)))?,
            )
        } else {
            None
        };

        // --error-types
        let error_types: Option<Vec<otelite_core::api::ErrorTypeBreakdown>> = if self.error_types {
            Some(
                storage
                    .query_error_types(Some(start_time), Some(end_time), &filters)
                    .await
                    .map_err(|e| Error::ApiError(format!("Failed to query error types: {}", e)))?,
            )
        } else {
            None
        };

        // --model-drift
        let model_drift: Option<Vec<otelite_core::api::ModelDriftPair>> = if self.model_drift {
            Some(
                storage
                    .query_model_drift(Some(start_time), Some(end_time), &filters)
                    .await
                    .map_err(|e| Error::ApiError(format!("Failed to query model drift: {}", e)))?,
            )
        } else {
            None
        };

        // --latency-context
        let latency_context: Option<Vec<otelite_core::api::LatencyByContextBin>> =
            if self.latency_context {
                Some(
                    storage
                        .query_latency_by_context(Some(start_time), Some(end_time), &filters)
                        .await
                        .map_err(|e| {
                            Error::ApiError(format!("Failed to query latency by context: {}", e))
                        })?,
                )
            } else {
                None
            };

        // --latency-percentiles. Calendar-day mode is explicit: only an
        // explicit --calendar-day opts in (UTC when no --timezone).
        let latency_percentiles: Option<otelite_core::api::LatencyPercentilesResponse> =
            if self.latency_percentiles {
                let tz = if self.calendar_day {
                    Some(self.timezone.as_deref().unwrap_or("UTC"))
                } else {
                    None
                };
                Some(
                    storage
                        .query_latency_percentiles(
                            Some(start_time),
                            Some(end_time),
                            &filters,
                            self.bucket_secs,
                            &["duration", "ttft"],
                            tz,
                        )
                        .await
                        .map_err(|e| {
                            Error::ApiError(format!("Failed to query latency percentiles: {}", e))
                        })?,
                )
            } else {
                None
            };

        // --tool-approvals
        let tool_approvals: Option<otelite_core::api::ToolApprovalStats> = if self.tool_approvals {
            Some(
                storage
                    .query_tool_approvals(Some(start_time), Some(end_time), &filters)
                    .await
                    .map_err(|e| {
                        Error::ApiError(format!("Failed to query tool approvals: {}", e))
                    })?,
            )
        } else {
            None
        };

        // --stop-reasons
        let stop_reasons: Option<Vec<otelite_core::api::StopReasonCount>> = if self.stop_reasons {
            Some(
                storage
                    .query_stop_reasons(Some(start_time), Some(end_time), &filters)
                    .await
                    .map_err(|e| Error::ApiError(format!("Failed to query stop reasons: {}", e)))?,
            )
        } else {
            None
        };

        // --context-split
        let context_split: Option<Vec<otelite_core::api::ContextTypeSplit>> = if self.context_split
        {
            Some(
                storage
                    .query_context_type_split(Some(start_time), Some(end_time), &filters)
                    .await
                    .map_err(|e| {
                        Error::ApiError(format!("Failed to query context split: {}", e))
                    })?,
            )
        } else {
            None
        };

        // --tool-errors
        let tool_errors: Option<Vec<otelite_core::api::ToolErrorEntry>> = if let Some(n) =
            self.tool_errors
        {
            Some(
                storage
                    .query_tool_errors(Some(start_time), Some(end_time), &filters, n)
                    .await
                    .map_err(|e| Error::ApiError(format!("Failed to query tool errors: {}", e)))?,
            )
        } else {
            None
        };

        // --hour-of-day
        let hour_of_day: Option<Vec<otelite_core::api::HourOfDayBucket>> = if self.hour_of_day {
            Some(
                storage
                    .query_hour_of_day(Some(start_time), Some(end_time), &filters)
                    .await
                    .map_err(|e| Error::ApiError(format!("Failed to query hour of day: {}", e)))?,
            )
        } else {
            None
        };

        // --agent-roles
        let agent_roles: Option<otelite_core::api::AgentRolesResponse> = if self.agent_roles {
            let mut response = storage
                .query_agent_roles(Some(start_time), Some(end_time))
                .await
                .map_err(|e| Error::ApiError(format!("Failed to query agent roles: {}", e)))?;
            // Cost is estimated from tokens x pricing (opencode's own cost
            // counter is zero-valued). Covers the top-5 models per role.
            for role in &mut response.roles {
                let mut total: f64 = 0.0;
                let mut all_priced = true;
                for m in &mut role.top_models {
                    let usage = TokenUsage {
                        input: m.tokens.input,
                        output: m.tokens.output,
                        cache_creation: m.tokens.cache_write,
                        cache_read: m.tokens.cache_read,
                    };
                    let cr = pricing_db.compute_cost(Some(m.model.as_str()), usage, None);
                    m.cost = cr.cost;
                    m.cost_source = Some(cr.source.as_str().to_string());
                    m.cost_reason = cr.reason;
                    match cr.cost {
                        Some(c) => total += c,
                        None => all_priced = false,
                    }
                }
                role.cost = if all_priced && !role.top_models.is_empty() {
                    Some(total)
                } else {
                    None
                };
            }
            Some(response)
        } else {
            None
        };

        // --calls
        let calls_series: Option<Vec<otelite_core::api::CallsSeriesPoint>> = if self.calls {
            Some(
                storage
                    .query_calls_series(
                        Some(start_time),
                        Some(end_time),
                        &filters,
                        self.bucket_secs,
                        false,
                    )
                    .await
                    .map_err(|e| Error::ApiError(format!("Failed to query calls series: {}", e)))?,
            )
        } else {
            None
        };

        // --by-session
        let by_session: Option<Vec<SessionRow>> = if self.by_session {
            let spans = storage
                .query_top_spans(
                    Some(start_time),
                    Some(end_time),
                    &filters,
                    200,
                    otelite_core::api::TopSpanSort::TotalTokens,
                    false,
                )
                .await
                .map_err(|e| Error::ApiError(format!("Failed to query spans: {}", e)))?;
            let mut map: HashMap<String, SessionRow> = HashMap::new();
            for span in &spans {
                let sid = span
                    .session_id
                    .clone()
                    .unwrap_or_else(|| "(no session)".to_string());
                let usage = TokenUsage {
                    input: span.input_tokens,
                    output: span.output_tokens,
                    cache_creation: span.cache_creation_tokens,
                    cache_read: span.cache_read_tokens,
                };
                let cr =
                    pricing_db.compute_cost(span.model.as_deref(), usage, span.system.as_deref());
                let row = map.entry(sid.clone()).or_insert_with(|| SessionRow {
                    session_id: sid,
                    ..Default::default()
                });
                row.requests += 1;
                row.input_tokens += span.input_tokens;
                row.output_tokens += span.output_tokens;
                row.cost += cr.cost.unwrap_or(0.0);
            }
            let mut rows: Vec<SessionRow> = map.into_values().collect();
            rows.sort_by(|a, b| {
                b.cost
                    .partial_cmp(&a.cost)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            Some(rows)
        } else {
            None
        };

        // --session-models
        let session_models: Option<otelite_core::api::SessionModelBreakdown> = if self
            .session_models
        {
            let mut breakdown = storage
                .query_session_model_breakdown(Some(start_time), Some(end_time))
                .await
                .map_err(|e| {
                    Error::ApiError(format!("Failed to query session_model_breakdown: {}", e))
                })?;
            for row in &mut breakdown.rows {
                let usage = TokenUsage {
                    input: row.input_tokens,
                    output: row.output_tokens,
                    cache_creation: 0,
                    cache_read: 0,
                };
                let cr = pricing_db.compute_cost(Some(&row.model), usage, None);
                row.cost = cr.cost;
            }
            breakdown.rows.sort_by(|a, b| match (a.cost, b.cost) {
                (Some(ac), Some(bc)) => bc.partial_cmp(&ac).unwrap_or(std::cmp::Ordering::Equal),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => b.requests.cmp(&a.requests),
            });
            Some(breakdown)
        } else {
            None
        };

        // --speed
        let speed: Option<otelite_core::api::SpeedDistribution> = if self.speed {
            Some(
                storage
                    .query_speed_distribution(Some(start_time), Some(end_time))
                    .await
                    .map_err(|e| {
                        Error::ApiError(format!("Failed to query speed_distribution: {}", e))
                    })?,
            )
        } else {
            None
        };

        use crate::config::OutputFormat;
        match format {
            OutputFormat::Json | OutputFormat::JsonCompact => {
                let output = UsageOutput {
                    summary,
                    by_model,
                    by_system,
                    by_session,
                    top_spans,
                    pricing_source,
                    latency_stats,
                    truncation_rate,
                    cache_hit_rate,
                    request_param_profile,
                    conversation_depth,
                    tool_usage,
                    error_types,
                    model_drift,
                    latency_series,
                    latency_context,
                    latency_percentiles,
                    tool_approvals,
                    stop_reasons,
                    context_split,
                    tool_errors,
                    hour_of_day,
                    calls_series,
                    agent_roles,
                    session_models,
                    speed,
                };
                let json = if matches!(format, OutputFormat::JsonCompact) {
                    serde_json::to_string(&output)
                } else {
                    serde_json::to_string_pretty(&output)
                };
                println!(
                    "{}",
                    json.map_err(|e| Error::ApiError(format!("JSON serialization failed: {}", e)))?
                );
            },
            OutputFormat::Pretty => {
                println!(
                    "\n{}",
                    format_header(&self.since, self.start.as_deref(), self.end.as_deref())
                );
                println!();

                display_summary(&summary);
                println!();

                if self.by_model
                    || !self.model.is_empty()
                    || (!self.by_system && self.system.is_none())
                {
                    display_by_model(&by_model);
                    println!();
                }

                if self.by_system || self.system.is_some() {
                    display_by_system(&by_system);
                    println!();
                }

                if let Some(ref spans) = top_spans {
                    display_top_spans(spans, self.top.unwrap_or(20));
                    println!();
                }

                if let Some(ref rows) = by_session {
                    display_by_session(rows);
                    println!();
                }

                if let Some(ref stats) = latency_stats {
                    display_latency_stats(stats);
                    println!();
                }

                if let Some(ref rows) = truncation_rate {
                    display_truncation_rate(rows);
                    println!();
                }

                if let Some(ref rows) = cache_hit_rate {
                    display_cache_hit_rate(rows);
                    println!();
                }

                if let Some(ref profile) = request_param_profile {
                    display_request_params(profile);
                    println!();
                }

                if let Some(ref depth) = conversation_depth {
                    display_conv_depth(depth);
                    println!();
                }

                if let Some(ref rows) = tool_usage {
                    display_tool_usage(rows);
                    println!();
                }

                if let Some(ref rows) = error_types {
                    display_error_types(rows);
                    println!();
                }

                if let Some(ref rows) = model_drift {
                    display_model_drift(rows);
                    println!();
                }

                if let Some(ref points) = latency_series {
                    display_latency_series(
                        points,
                        self.throughput,
                        self.calendar_day
                            .then(|| self.timezone.as_deref().unwrap_or("UTC")),
                    );
                    println!();
                }

                if let Some(ref bins) = latency_context {
                    display_latency_context(bins);
                    println!();
                }

                if let Some(ref resp) = latency_percentiles {
                    display_latency_percentiles(
                        resp,
                        &self.model,
                        self.calendar_day
                            .then(|| self.timezone.as_deref().unwrap_or("UTC")),
                    );
                    println!();
                }

                if let Some(ref stats) = tool_approvals {
                    display_tool_approvals(stats);
                    println!();
                }

                if let Some(ref rows) = stop_reasons {
                    display_stop_reasons(rows);
                    println!();
                }

                if let Some(ref rows) = context_split {
                    display_context_split(rows);
                    println!();
                }

                if let Some(ref rows) = tool_errors {
                    display_tool_errors(rows);
                    println!();
                }

                if let Some(ref buckets) = hour_of_day {
                    display_hour_of_day(buckets);
                    println!();
                }

                if let Some(ref points) = calls_series {
                    display_calls_series(points);
                    println!();
                }

                if let Some(ref roles) = agent_roles {
                    display_agent_roles(roles);
                    println!();
                }

                if let Some(ref breakdown) = session_models {
                    display_session_models(breakdown);
                    println!();
                }

                if let Some(ref dist) = speed {
                    display_speed_distribution(dist);
                    println!();
                }

                println!("Pricing source: {}", pricing_source);
            },
        }

        Ok(())
    }
}

// ── display helpers ───────────────────────────────────────────────────────────

/// Current wall-clock time in nanoseconds since the Unix epoch.
pub(crate) fn now_ns() -> Result<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| Error::ApiError(format!("Failed to get current time: {}", e)))
        .map(|d| d.as_nanos() as i64)
}

/// Parse an exact --start/--end value (#142) to epoch nanoseconds:
/// `YYYY-MM-DD` (midnight UTC), `YYYY-MM-DDTHH:MM:SS[.ffffff]` (UTC when no
/// zone is given, RFC 3339 offsets otherwise), or epoch seconds/nanoseconds.
pub(crate) fn parse_cli_time(value: &str) -> Result<i64> {
    // Epoch: 10-digit values are seconds, 16+ are nanoseconds.
    if value.chars().all(|c| c.is_ascii_digit()) {
        let n: i64 = value
            .parse()
            .map_err(|_| Error::ApiError(format!("Invalid epoch time: {value}")))?;
        return Ok(if value.len() >= 16 {
            n
        } else {
            n * 1_000_000_000
        });
    }
    let bad = || {
        Error::ApiError(format!(
            "Invalid time '{value}': use YYYY-MM-DD, YYYY-MM-DDTHH:MM:SS[.ffffff] \
         (UTC when no zone is given), or epoch seconds/nanoseconds"
        ))
    };
    if let Ok(date) = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        let ns = date
            .and_hms_opt(0, 0, 0)
            .and_then(|t| t.and_utc().timestamp_nanos_opt());
        return ns.ok_or_else(bad);
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(value) {
        return dt.timestamp_nanos_opt().ok_or_else(bad);
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
    ] {
        if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(value, fmt) {
            return ndt.and_utc().timestamp_nanos_opt().ok_or_else(bad);
        }
    }
    Err(bad())
}

/// Build the model cohort for every panel from repeated --model values
/// (#142): a single plain value uses the exact-match `model` dimension,
/// globs (`*`) and multiple values use the `models` patterns (ORed).
fn build_model_filters(values: &[String]) -> GenAiFilters {
    match values {
        [] => GenAiFilters::default(),
        [single] if !single.contains('*') => GenAiFilters {
            model: Some(single.clone()),
            ..Default::default()
        },
        _ => GenAiFilters {
            models: Some(values.to_vec()),
            ..Default::default()
        },
    }
}

pub(crate) fn parse_time_range(range: &str) -> Result<(i64, i64)> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| Error::ApiError(format!("Failed to get current time: {}", e)))?
        .as_nanos() as i64;

    let duration_ns = if let Some(stripped) = range.strip_suffix('h') {
        let hours: i64 = stripped
            .parse()
            .map_err(|_| Error::ApiError("Invalid hour format".to_string()))?;
        hours * 3600 * 1_000_000_000
    } else if let Some(stripped) = range.strip_suffix('d') {
        let days: i64 = stripped
            .parse()
            .map_err(|_| Error::ApiError("Invalid day format".to_string()))?;
        days * 24 * 3600 * 1_000_000_000
    } else if let Some(stripped) = range.strip_suffix('m') {
        let minutes: i64 = stripped
            .parse()
            .map_err(|_| Error::ApiError("Invalid minute format".to_string()))?;
        minutes * 60 * 1_000_000_000
    } else {
        return Err(Error::ApiError(
            "Invalid time range format. Use format like '1h', '24h', '7d', '30d'".to_string(),
        ));
    };

    let start_time = now - duration_ns;
    Ok((start_time, now))
}

/// Human-readable range label for the summary header: exact --start/--end
/// when given, otherwise the rolling --since window.
fn format_header(range: &str, start: Option<&str>, end: Option<&str>) -> String {
    match (start, end) {
        (Some(start), Some(end)) => {
            format!("Token Usage Summary ({start} → {end})")
        },
        (Some(start), None) => {
            format!("Token Usage Summary ({start} → now)")
        },
        _ => format!("Token Usage Summary (Last {range})"),
    }
}

fn display_summary(summary: &otelite_core::api::TokenUsageSummary) {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    table.set_header(vec![
        Cell::new("Metric").fg(Color::Cyan),
        Cell::new("Value").fg(Color::Cyan),
    ]);

    table.add_row(vec![
        "Total Input Tokens",
        &format_number(summary.total_input_tokens),
    ]);
    table.add_row(vec![
        "Total Output Tokens",
        &format_number(summary.total_output_tokens),
    ]);
    table.add_row(vec![
        "Total Tokens",
        &format_number(summary.total_input_tokens + summary.total_output_tokens),
    ]);
    table.add_row(vec!["Total Requests", &summary.total_requests.to_string()]);

    if summary.total_cache_creation_tokens > 0 {
        table.add_row(vec![
            "Cache Creation Tokens",
            &format_number(summary.total_cache_creation_tokens),
        ]);
    }
    if summary.total_cache_read_tokens > 0 {
        table.add_row(vec![
            "Cache Read Tokens",
            &format_number(summary.total_cache_read_tokens),
        ]);
    }

    println!("{}", table);
}

fn display_by_model(models: &[ModelRow]) {
    if models.is_empty() {
        println!("No model data available");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    // Rerouting columns are only meaningful when some identity shows them.
    let show_rerouting = models
        .iter()
        .any(|m| m.response_model.is_some() || m.rerouted_count > 0);

    let mut header: Vec<Cell> = vec![
        Cell::new("Model").fg(Color::Cyan),
        Cell::new("Input").fg(Color::Cyan),
        Cell::new("Output").fg(Color::Cyan),
        Cell::new("Total").fg(Color::Cyan),
        Cell::new("Requests").fg(Color::Cyan),
        Cell::new("Cost").fg(Color::Cyan),
    ];
    if show_rerouting {
        header.push(Cell::new("Resp model").fg(Color::Cyan));
        header.push(Cell::new("Rerouted").fg(Color::Cyan));
    }
    table.set_header(header);

    for m in models {
        let mut row: Vec<String> = vec![
            m.model.clone(),
            format_number(m.input_tokens),
            format_number(m.output_tokens),
            format_number(m.total_tokens),
            m.requests.to_string(),
            format_cost(m.cost),
        ];
        if show_rerouting {
            row.push(m.response_model.clone().unwrap_or_else(|| "—".into()));
            row.push(if m.rerouted_count > 0 {
                m.rerouted_count.to_string()
            } else {
                "—".into()
            });
        }
        table.add_row(row.iter().map(|s| s.as_str()).collect::<Vec<_>>());
    }

    println!("Breakdown by Model:");
    println!("{}", table);
    if show_rerouting {
        println!(
            "* Resp model = dominant response model differing from the request model; Rerouted = calls the provider served with a different model."
        );
    }
}

fn display_by_system(systems: &[SystemRow]) {
    if systems.is_empty() {
        println!("No system data available");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    table.set_header(vec![
        Cell::new("System").fg(Color::Cyan),
        Cell::new("Input").fg(Color::Cyan),
        Cell::new("Output").fg(Color::Cyan),
        Cell::new("Total").fg(Color::Cyan),
        Cell::new("Requests").fg(Color::Cyan),
        Cell::new("Cost").fg(Color::Cyan),
    ]);

    for s in systems {
        let display_name = otelite_core::telemetry::GenAiSpanInfo::format_system_name(&s.system);
        table.add_row(vec![
            &display_name,
            &format_number(s.input_tokens),
            &format_number(s.output_tokens),
            &format_number(s.total_tokens),
            &s.requests.to_string(),
            &format_cost(s.cost),
        ]);
    }

    println!("Breakdown by System:");
    println!("{}", table);
}

fn display_top_spans(spans: &[otelite_core::api::TopSpan], n: usize) {
    if spans.is_empty() {
        println!("No LLM spans found in range");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    table.set_header(vec![
        Cell::new("#").fg(Color::Cyan),
        Cell::new("Model").fg(Color::Cyan),
        Cell::new("Session").fg(Color::Cyan),
        Cell::new("Input").fg(Color::Cyan),
        Cell::new("Output").fg(Color::Cyan),
        Cell::new("Cache").fg(Color::Cyan),
        Cell::new("Cost").fg(Color::Cyan),
        Cell::new("Duration").fg(Color::Cyan),
    ]);

    for (i, span) in spans.iter().enumerate() {
        let duration_ms = span.duration / 1_000_000;
        let session = span.session_id.as_deref().unwrap_or("—").to_string();
        let model = span.model.as_deref().unwrap_or("—").to_string();
        table.add_row(vec![
            &(i + 1).to_string(),
            &model,
            &truncate(&session, 24),
            &format_number(span.input_tokens),
            &format_number(span.output_tokens),
            &format_number(span.cache_read_tokens),
            &format_cost(span.cost),
            &format!("{}ms", duration_ms),
        ]);
    }

    println!("Top {} LLM Calls by Cost:", n);
    println!("{}", table);
}

fn display_by_session(rows: &[SessionRow]) {
    if rows.is_empty() {
        println!("No session data found in range");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    table.set_header(vec![
        Cell::new("Session ID").fg(Color::Cyan),
        Cell::new("Requests").fg(Color::Cyan),
        Cell::new("Input").fg(Color::Cyan),
        Cell::new("Output").fg(Color::Cyan),
        Cell::new("Cost").fg(Color::Cyan),
    ]);

    for row in rows {
        table.add_row(vec![
            &truncate(&row.session_id, 32),
            &row.requests.to_string(),
            &format_number(row.input_tokens),
            &format_number(row.output_tokens),
            &format!("${:.4}", row.cost),
        ]);
    }

    println!("Breakdown by Session:");
    println!("{}", table);
}

fn format_cost(cost: Option<f64>) -> String {
    match cost {
        Some(c) => format!("${:.4}", c),
        None => "—".to_string(),
    }
}

fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();

    for (count, c) in s.chars().rev().enumerate() {
        if count > 0 && count % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }

    result.chars().rev().collect()
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

fn display_latency_stats(stats: &[otelite_core::api::LatencyStats]) {
    if stats.is_empty() {
        println!("No latency data available");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    table.set_header(vec![
        Cell::new("Model").fg(Color::Cyan),
        Cell::new("Calls").fg(Color::Cyan),
        Cell::new("p50 ms").fg(Color::Cyan),
        Cell::new("p95 ms").fg(Color::Cyan),
        Cell::new("p99 ms").fg(Color::Cyan),
        Cell::new("Tok/s* p10").fg(Color::Yellow),
        Cell::new("Tok/s* p50").fg(Color::Yellow),
        Cell::new("Tok/s* p90").fg(Color::Yellow),
        Cell::new("N*").fg(Color::Yellow),
        Cell::new("Context p50").fg(Color::Cyan),
        Cell::new("Context p95").fg(Color::Cyan),
        Cell::new("Out/Context p50").fg(Color::Cyan),
        Cell::new("TTFT p50").fg(Color::Yellow),
        Cell::new("TTFT p95").fg(Color::Yellow),
    ]);

    for s in stats {
        let model = s.model.as_deref().unwrap_or("(unknown)");
        let ttft_p50 = if let Some(status) =
            ttft_stream_status(s.ttft_count, s.ttft_degenerate_count, s.ttft_degenerate)
        {
            status
        } else if s.ttft_count > 0 {
            s.ttft_p50_ms
                .map_or("—".to_string(), |v| format!("{}ms", v))
        } else {
            "—".to_string()
        };
        let ttft_p95 = if let Some(status) =
            ttft_stream_status(s.ttft_count, s.ttft_degenerate_count, s.ttft_degenerate)
        {
            status
        } else if s.ttft_count > 0 {
            s.ttft_p95_ms
                .map_or("—".to_string(), |v| format!("{}ms", v))
        } else {
            "—".to_string()
        };
        let tok_rate = |v: Option<f64>| v.map_or("—".to_string(), |x| format!("{x:.0}"));
        // Weak lower-tail marker: p10 is untrustworthy under 10 samples.
        let sample_count = if s.throughput_sample_count < 10 {
            format!("{}†", s.throughput_sample_count)
        } else {
            s.throughput_sample_count.to_string()
        };
        table.add_row(vec![
            model,
            &s.count.to_string(),
            &s.p50_ms.to_string(),
            &s.p95_ms.to_string(),
            &s.p99_ms.to_string(),
            &tok_rate(s.derived_tokens_per_sec_p10),
            &tok_rate(s.derived_tokens_per_sec_p50),
            &tok_rate(s.derived_tokens_per_sec_p90),
            &sample_count,
            &s.input_tokens_p50
                .map_or("—".to_string(), |v| format_number(v as u64)),
            &s.input_tokens_p95
                .map_or("—".to_string(), |v| format_number(v as u64)),
            &s.output_input_ratio_p50
                .map_or("—".to_string(), |v| format!("{:.2}×", v)),
            &ttft_p50,
            &ttft_p95,
        ]);
    }

    println!("Latency Stats by Model:");
    println!("{}", table);
    println!(
        "* Tok/s = derived end-to-end output throughput (output tokens / span duration, which includes provider, queue and network time — not pure generation rate); N = calls with positive output and duration. † p10 is a weak estimate below 10 samples."
    );
    if stats.iter().any(|stats| stats.ttft_degenerate) {
        println!(
            "TTFT is emitter-supplied. “buffered” means most first-token values were near full request duration, so no stream was observed."
        );
    }
}

fn ttft_stream_status(
    ttft_count: usize,
    ttft_degenerate_count: usize,
    ttft_degenerate: bool,
) -> Option<String> {
    ttft_degenerate.then(|| {
        let percentage = ttft_degenerate_count * 100 / ttft_count;
        format!("buffered ({percentage}%)")
    })
}

fn display_latency_series(
    points: &[otelite_core::api::LatencySeriesPoint],
    show_throughput: bool,
    calendar_timezone: Option<&str>,
) {
    use chrono::{DateTime, Local, Utc};

    if points.is_empty() {
        println!("Latency Trend: no data in range");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    let mut header: Vec<Cell> = vec![
        Cell::new("Bucket").fg(Color::Cyan),
        Cell::new("Model").fg(Color::Cyan),
        Cell::new("N").fg(Color::Cyan),
        Cell::new("Err").fg(Color::Red),
        Cell::new("min ms").fg(Color::Green),
        Cell::new("avg ms").fg(Color::Yellow),
        Cell::new("p95 ms").fg(Color::Yellow),
        Cell::new("max ms").fg(Color::Red),
        Cell::new("TTFT avg").fg(Color::Cyan),
        Cell::new("TTFT p95").fg(Color::Cyan),
    ];
    if show_throughput {
        header.push(Cell::new("N*").fg(Color::Cyan));
        header.push(Cell::new("Tok/s* (p10/p50/p90)").fg(Color::Yellow));
    }
    table.set_header(header);

    for p in points {
        let dt = DateTime::<Utc>::from_timestamp_nanos(p.timestamp);
        let bucket_str = match calendar_timezone {
            Some(tz) => match <chrono_tz::Tz as std::str::FromStr>::from_str(tz) {
                Ok(tz) => dt.with_timezone(&tz).format("%Y-%m-%d").to_string(),
                Err(_) => dt.with_timezone(&Local).format("%Y-%m-%d").to_string(),
            },
            None => dt.with_timezone(&Local).format("%m-%d %H:%M").to_string(),
        };
        let model = p.model.as_deref().unwrap_or("(unknown)");
        let err_str = if p.error_count > 0 {
            format!("{}", p.error_count)
        } else {
            "—".to_string()
        };
        let ttft_avg = ttft_stream_status(p.ttft_count, p.ttft_degenerate_count, p.ttft_degenerate)
            .unwrap_or_else(|| {
                p.avg_ttft_ms
                    .map_or("—".to_string(), |v| format!("{:.0}ms", v))
            });
        let ttft_p95 = ttft_stream_status(p.ttft_count, p.ttft_degenerate_count, p.ttft_degenerate)
            .unwrap_or_else(|| {
                p.p95_ttft_ms
                    .map_or("—".to_string(), |v| format!("{:.0}ms", v))
            });

        let mut row: Vec<String> = vec![
            bucket_str,
            model.to_string(),
            p.count.to_string(),
            err_str,
            p.min_ms.to_string(),
            format!("{:.0}", p.avg_ms),
            p.p95_ms.to_string(),
            p.max_ms.to_string(),
            ttft_avg,
            ttft_p95,
        ];
        if show_throughput {
            row.push(p.throughput_sample_count.to_string());
            row.push(
                match (
                    p.throughput_p10_tok_s,
                    p.throughput_p50_tok_s,
                    p.throughput_p90_tok_s,
                ) {
                    (Some(a), Some(b), Some(c)) => format!("{a:.0}/{b:.0}/{c:.0}"),
                    _ => "—".to_string(),
                },
            );
        }
        let row_refs: Vec<&String> = row.iter().collect();
        table.add_row(row_refs);
    }

    println!("Latency Trend (per bucket × model):");
    println!("{table}");
    if show_throughput {
        println!("* N* = throughput-eligible calls (output tokens > 0, duration > 0); Tok/s = derived end-to-end output throughput per call.");
    }
}

fn display_latency_context(bins: &[otelite_core::api::LatencyByContextBin]) {
    if bins.is_empty() {
        println!("Latency by Context Size: no data (requires gen_ai input token attributes)");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    table.set_header(vec![
        Cell::new("Context Bin").fg(Color::Cyan),
        Cell::new("Model").fg(Color::Cyan),
        Cell::new("N").fg(Color::Cyan),
        Cell::new("avg ms").fg(Color::Yellow),
        Cell::new("p95 ms").fg(Color::Yellow),
        Cell::new("max ms").fg(Color::Red),
        Cell::new("TTFT avg").fg(Color::Cyan),
    ]);

    for b in bins {
        let model = b.model.as_deref().unwrap_or("(unknown)");
        let ttft = ttft_stream_status(b.ttft_count, b.ttft_degenerate_count, b.ttft_degenerate)
            .unwrap_or_else(|| {
                b.avg_ttft_ms
                    .map_or("—".to_string(), |v| format!("{:.0}ms", v))
            });
        table.add_row(vec![
            Cell::new(&b.bin),
            Cell::new(model),
            Cell::new(b.count),
            Cell::new(format!("{:.0}", b.avg_ms)),
            Cell::new(b.p95_ms),
            Cell::new(b.max_ms),
            Cell::new(ttft),
        ]);
    }

    println!("Latency by Context Size (input tokens × model):");
    println!("{}", table);
}

fn display_latency_percentiles<'a>(
    resp: &'a otelite_core::api::LatencyPercentilesResponse,
    model_filter: &[String],
    calendar_timezone: Option<&str>,
) {
    use chrono::{DateTime, Local, Utc};
    use otelite_core::filters::model_matches;

    // One row group per (label, points). With a model filter each matching
    // model becomes its own group (patterns are matched against the stored
    // model names, so globs work); without one, rolling mode shows the
    // aggregated "all models" series and calendar mode shows every model
    // separately.
    let pick = |series: &'a otelite_core::api::LatencyPercentileSeries| -> Vec<
        (String, Vec<&'a otelite_core::api::LatencyPercentilePoint>),
    > {
        if model_filter.is_empty() {
            if calendar_timezone.is_some() {
                series
                    .models
                    .iter()
                    .map(|(m, pts)| (m.clone(), pts.iter().collect()))
                    .collect()
            } else {
                vec![("all models".to_string(), series.all.iter().collect())]
            }
        } else {
            let mut matched: Vec<(String, Vec<&'a otelite_core::api::LatencyPercentilePoint>)> =
                Vec::new();
            for (m, pts) in &series.models {
                if model_filter.iter().any(|pat| model_matches(pat, m)) {
                    matched.push((m.clone(), pts.iter().collect()));
                }
            }
            matched
        }
    };

    let mut any = false;
    for (metric, series) in &resp.metrics {
        let groups = pick(series);
        if groups.is_empty() || groups.iter().all(|(_, pts)| pts.is_empty()) {
            if !model_filter.is_empty() {
                println!("Latency Percentiles ({metric}): no data in range");
            }
            continue;
        }
        any = true;
        let pct = |v: Option<f64>| v.map_or("—".to_string(), |x| format!("{x:.0}"));
        for (label, points) in groups {
            if points.is_empty() {
                continue;
            }
            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .set_content_arrangement(ContentArrangement::Dynamic);
            table.set_header(vec![
                Cell::new("Bucket").fg(Color::Cyan),
                Cell::new("Model").fg(Color::Cyan),
                Cell::new("N").fg(Color::Cyan),
                Cell::new("p10 ms").fg(Color::Green),
                Cell::new("p50 ms").fg(Color::Green),
                Cell::new("p90 ms").fg(Color::Yellow),
                Cell::new("p95 ms").fg(Color::Yellow),
                Cell::new("p99 ms").fg(Color::Red),
                Cell::new("Tok/s* (p10/p50/p90)").fg(Color::Yellow),
            ]);
            for p in points {
                let dt = DateTime::<Utc>::from_timestamp_nanos(p.ts);
                // Calendar mode: label the day in the bucket's own timezone.
                let bucket_str = match calendar_timezone {
                    Some(tz) => match <chrono_tz::Tz as std::str::FromStr>::from_str(tz) {
                        Ok(tz) => dt.with_timezone(&tz).format("%Y-%m-%d").to_string(),
                        Err(_) => dt.with_timezone(&Local).format("%Y-%m-%d").to_string(),
                    },
                    None => dt.with_timezone(&Local).format("%m-%d %H:%M").to_string(),
                };
                let tok = match (
                    p.throughput_p10_tok_s,
                    p.throughput_p50_tok_s,
                    p.throughput_p90_tok_s,
                ) {
                    (Some(a), Some(b), Some(c)) => format!("{a:.0}/{b:.0}/{c:.0}"),
                    _ => "—".to_string(),
                };
                table.add_row(vec![
                    Cell::new(bucket_str),
                    Cell::new(&label),
                    Cell::new(p.count),
                    Cell::new(pct(p.p10_ms)),
                    Cell::new(pct(p.p50_ms)),
                    Cell::new(pct(p.p90_ms)),
                    Cell::new(pct(p.p95_ms)),
                    Cell::new(pct(p.p99_ms)),
                    Cell::new(tok),
                ]);
            }
            println!("Latency Percentiles ({metric}, {label}):");
            println!("{table}");
            println!("* Tok/s = derived end-to-end output throughput per call (raw ns durations); N = calls, not the throughput sample.");
            println!();
        }
    }
    if !any {
        println!("Latency Percentiles: no data in range");
    }
}

fn display_truncation_rate(rows: &[otelite_core::api::TruncationRateByModel]) {
    let any_truncated = rows.iter().any(|r| r.truncated > 0);
    if !any_truncated {
        println!("Truncation Rate by Model: no truncations observed");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    table.set_header(vec![
        Cell::new("Model").fg(Color::Cyan),
        Cell::new("Total Calls").fg(Color::Cyan),
        Cell::new("Truncated").fg(Color::Cyan),
        Cell::new("Rate").fg(Color::Cyan),
    ]);

    for r in rows {
        let model = r.model.as_deref().unwrap_or("(unknown)");
        let rate_pct = r.rate * 100.0;
        let rate_color = if rate_pct > 5.0 {
            Color::Red
        } else if rate_pct > 1.0 {
            Color::Yellow
        } else {
            Color::Green
        };
        table.add_row(vec![
            Cell::new(model),
            Cell::new(r.total),
            Cell::new(r.truncated),
            Cell::new(format!("{:.1}%", rate_pct)).fg(rate_color),
        ]);
    }

    println!("Truncation Rate by Model (finish_reason = max_tokens/length):");
    println!("{}", table);
}

fn display_cache_hit_rate(rows: &[otelite_core::api::CacheHitRateByModel]) {
    let any_cache = rows.iter().any(|r| r.total_cache_read_tokens > 0);
    if !any_cache {
        println!("Cache Hit Rate: no cache reads observed");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    table.set_header(vec![
        Cell::new("Model").fg(Color::Cyan),
        Cell::new("Input Tokens").fg(Color::Cyan),
        Cell::new("Cache Read").fg(Color::Cyan),
        Cell::new("Cache Created").fg(Color::Cyan),
        Cell::new("Hit Rate").fg(Color::Cyan),
    ]);

    for r in rows {
        let model = r.model.as_deref().unwrap_or("(unknown)");
        let hit_pct = r.hit_rate.unwrap_or(0.0) * 100.0;
        let rate_color = if hit_pct >= 20.0 {
            Color::Green
        } else if hit_pct >= 5.0 {
            Color::Yellow
        } else {
            Color::White
        };
        table.add_row(vec![
            Cell::new(model),
            Cell::new(format_number(r.total_input_tokens)),
            Cell::new(format_number(r.total_cache_read_tokens)),
            Cell::new(format_number(r.total_cache_creation_tokens)),
            Cell::new(format!("{:.1}%", hit_pct)).fg(rate_color),
        ]);
    }

    println!("Cache Hit Rate by Model (cache_read / (cache_read + input)):");
    println!("{}", table);
}

fn display_request_params(profile: &otelite_core::api::RequestParamProfile) {
    if !profile.temperature_buckets.is_empty() {
        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic);

        table.set_header(vec![
            Cell::new("Temperature").fg(Color::Cyan),
            Cell::new("Calls").fg(Color::Cyan),
        ]);
        for b in &profile.temperature_buckets {
            let temp = b
                .temperature
                .map_or("not set".to_string(), |v| format!("{}", v));
            table.add_row(vec![&temp, &b.count.to_string()]);
        }
        println!("Temperature Distribution:");
        println!("{}", table);
    }

    if !profile.max_tokens_buckets.is_empty() {
        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic);

        table.set_header(vec![
            Cell::new("max_tokens").fg(Color::Cyan),
            Cell::new("Calls").fg(Color::Cyan),
        ]);
        for b in &profile.max_tokens_buckets {
            let mt = b
                .max_tokens
                .map_or("not set".to_string(), |v| format_number(v as u64));
            table.add_row(vec![&mt, &b.count.to_string()]);
        }
        println!("max_tokens Distribution:");
        println!("{}", table);
    }
}

fn display_conv_depth(depth: &otelite_core::api::ConversationDepthStats) {
    if depth.total_conversations == 0 {
        println!("Conversation Depth: no conversations with conversation_id observed");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    table.set_header(vec![
        Cell::new("Metric").fg(Color::Cyan),
        Cell::new("Value").fg(Color::Cyan),
    ]);

    table.add_row(vec![
        "Total conversations",
        &depth.total_conversations.to_string(),
    ]);
    table.add_row(vec!["Avg turns", &format!("{:.1}", depth.avg_turns)]);
    table.add_row(vec!["p50 turns", &depth.p50_turns.to_string()]);
    table.add_row(vec!["p95 turns", &depth.p95_turns.to_string()]);
    table.add_row(vec!["p99 turns", &depth.p99_turns.to_string()]);

    println!("Conversation Depth (turns per conversation_id):");
    println!("{}", table);
}

fn display_tool_usage(rows: &[otelite_core::api::ToolUsage]) {
    if rows.is_empty() {
        println!("Tool Usage: no tool calls observed");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    table.set_header(vec![
        Cell::new("Tool").fg(Color::Cyan),
        Cell::new("Calls").fg(Color::Cyan),
        Cell::new("Success%").fg(Color::Cyan),
        Cell::new("Errors").fg(Color::Cyan),
        Cell::new("Avg ms").fg(Color::Cyan),
    ]);

    for r in rows {
        let success_pct = if r.count > 0 {
            r.success_count as f64 / r.count as f64 * 100.0
        } else {
            0.0
        };
        let color = if success_pct < 90.0 {
            Color::Red
        } else if success_pct < 99.0 {
            Color::Yellow
        } else {
            Color::Green
        };
        table.add_row(vec![
            Cell::new(&r.tool_name),
            Cell::new(r.count),
            Cell::new(format!("{:.1}%", success_pct)).fg(color),
            Cell::new(r.error_count),
            Cell::new(format!("{:.0}", r.avg_duration_ms)),
        ]);
    }

    println!("Tool Usage (success rate = calls without error / total calls):");
    println!("{}", table);
}

fn display_error_types(rows: &[otelite_core::api::ErrorTypeBreakdown]) {
    if rows.is_empty() {
        println!("Error Types: no error spans observed");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    table.set_header(vec![
        Cell::new("Bucket").fg(Color::Red),
        Cell::new("Error Type").fg(Color::Cyan),
        Cell::new("Model").fg(Color::Cyan),
        Cell::new("Count").fg(Color::Yellow),
    ]);

    for r in rows {
        let model = r.model.as_deref().unwrap_or("(unknown)");
        table.add_row(vec![
            Cell::new(&r.bucket).fg(Color::Red),
            Cell::new(&r.error_type),
            Cell::new(model),
            Cell::new(r.count).fg(Color::Yellow),
        ]);
    }

    println!("Error Type Breakdown (sorted by count; raw error_type shown for inspection):");
    println!("{}", table);
}

fn display_model_drift(rows: &[otelite_core::api::ModelDriftPair]) {
    let drifted: Vec<_> = rows.iter().filter(|r| r.differs).collect();
    if drifted.is_empty() {
        println!("Model Drift: no silent rerouting detected (request and response models match)");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    table.set_header(vec![
        Cell::new("Requested Model").fg(Color::Cyan),
        Cell::new("Served Model").fg(Color::Yellow),
        Cell::new("Count").fg(Color::Cyan),
    ]);

    for r in drifted {
        let req = r.request_model.as_deref().unwrap_or("(unknown)");
        let resp = r.response_model.as_deref().unwrap_or("(unknown)");
        table.add_row(vec![
            Cell::new(req),
            Cell::new(resp).fg(Color::Yellow),
            Cell::new(r.count),
        ]);
    }

    println!("Model Drift — provider served a different model than requested:");
    println!("{}", table);
}

fn display_tool_approvals(stats: &otelite_core::api::ToolApprovalStats) {
    let total = stats.total;
    if total == 0 {
        println!("Tool Approvals: no data");
        return;
    }
    let auto_pct = stats.auto_accepted as f64 / total as f64 * 100.0;
    let reject_pct = stats.rejected as f64 / total as f64 * 100.0;

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("Decision").fg(Color::Cyan),
        Cell::new("Count").fg(Color::Cyan),
        Cell::new("Rate").fg(Color::Cyan),
    ]);
    table.add_row(vec![
        "Auto-accept (config)",
        &stats.auto_accepted.to_string(),
        &format!("{:.1}%", auto_pct),
    ]);
    table.add_row(vec![
        "User-accept",
        &stats.user_accepted.to_string(),
        &format!("{:.1}%", stats.user_accepted as f64 / total as f64 * 100.0),
    ]);
    table.add_row(vec![
        "Rejected",
        &stats.rejected.to_string(),
        &format!("{:.1}%", reject_pct),
    ]);
    table.add_row(vec![
        "Unknown",
        &stats.unknown.to_string(),
        &format!("{:.1}%", stats.unknown as f64 / total as f64 * 100.0),
    ]);
    println!("Tool Approval Decisions ({} total):", total);
    println!("{}", table);

    if !stats.top_rejected.is_empty() {
        let mut t2 = Table::new();
        t2.load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic);
        t2.set_header(vec![
            Cell::new("Tool").fg(Color::Cyan),
            Cell::new("Rejections").fg(Color::Cyan),
        ]);
        for e in &stats.top_rejected {
            t2.add_row(vec![&e.tool_name, &e.count.to_string()]);
        }
        println!("Top Rejected Tools:");
        println!("{}", t2);
    }
}

fn display_stop_reasons(rows: &[otelite_core::api::StopReasonCount]) {
    if rows.is_empty() {
        println!("Stop Reasons: no data");
        return;
    }
    let total: usize = rows.iter().map(|r| r.count).sum();
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("Reason").fg(Color::Cyan),
        Cell::new("Count").fg(Color::Cyan),
        Cell::new("%").fg(Color::Cyan),
    ]);
    for r in rows {
        let pct = if total > 0 {
            r.count as f64 / total as f64 * 100.0
        } else {
            0.0
        };
        table.add_row(vec![
            r.reason.as_str(),
            &r.count.to_string(),
            &format!("{:.1}%", pct),
        ]);
    }
    println!("Stop Reasons:");
    println!("{}", table);
}

fn display_context_split(rows: &[otelite_core::api::ContextTypeSplit]) {
    if rows.is_empty() {
        println!("Context split: no data");
        return;
    }
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("Context").fg(Color::Cyan),
        Cell::new("Calls").fg(Color::Cyan),
        Cell::new("Input tokens").fg(Color::Cyan),
        Cell::new("Output tokens").fg(Color::Cyan),
        Cell::new("Avg latency").fg(Color::Cyan),
    ]);
    for r in rows {
        let avg = if r.avg_ms > 0.0 {
            format!("{} ms", r.avg_ms.round() as i64)
        } else {
            "—".to_string()
        };
        table.add_row(vec![
            r.context.as_str(),
            &r.calls.to_string(),
            &format_number(r.input_tokens),
            &format_number(r.output_tokens),
            &avg,
        ]);
    }
    println!("Usage by Request Context (llm_request.context):");
    println!("{}", table);
}

fn display_tool_errors(rows: &[otelite_core::api::ToolErrorEntry]) {
    if rows.is_empty() {
        println!("Tool Errors: none");
        return;
    }
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("Tool").fg(Color::Cyan),
        Cell::new("Error (truncated)").fg(Color::Cyan),
        Cell::new("Count").fg(Color::Cyan),
    ]);
    for r in rows {
        let msg = truncate(&r.error_message, 60);
        table.add_row(vec![r.tool_name.as_str(), &msg, &r.count.to_string()]);
    }
    println!("Top Tool Errors:");
    println!("{}", table);
}

fn display_hour_of_day(buckets: &[otelite_core::api::HourOfDayBucket]) {
    let max_llm = buckets
        .iter()
        .map(|b| b.llm_calls)
        .max()
        .unwrap_or(1)
        .max(1);
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("Hour (UTC)").fg(Color::Cyan),
        Cell::new("LLM calls").fg(Color::Cyan),
        Cell::new("Bar").fg(Color::Cyan),
        Cell::new("Tool calls").fg(Color::Cyan),
    ]);
    for b in buckets {
        let bar_len = (b.llm_calls * 20 / max_llm).max(if b.llm_calls > 0 { 1 } else { 0 });
        let bar = "█".repeat(bar_len);
        table.add_row(vec![
            &format!("{:02}:00", b.hour),
            &b.llm_calls.to_string(),
            &bar,
            &b.tool_calls.to_string(),
        ]);
    }
    println!("Activity by Hour of Day:");
    println!("{}", table);
}

fn display_agent_roles(response: &otelite_core::api::AgentRolesResponse) {
    if response.roles.is_empty() {
        println!("Agent Roles: no data in range (opencode only)");
        return;
    }
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("Role").fg(Color::Cyan),
        Cell::new("Sessions").fg(Color::Cyan),
        Cell::new("Tokens").fg(Color::Cyan),
        Cell::new("In / Out").fg(Color::Cyan),
        Cell::new("Cache r/w").fg(Color::Cyan),
        Cell::new("Reasoning").fg(Color::Cyan),
        Cell::new("Share").fg(Color::Cyan),
        Cell::new("Cost (est.)").fg(Color::Cyan),
        Cell::new("Top model").fg(Color::Cyan),
    ]);
    for r in &response.roles {
        let top_model = r
            .top_models
            .first()
            .map(|m| m.model.as_str())
            .unwrap_or("—");
        let in_out = format!(
            "{}/{}",
            format_number(r.tokens.input),
            format_number(r.tokens.output)
        );
        let cache_rw = format!(
            "{}/{}",
            format_number(r.tokens.cache_read),
            format_number(r.tokens.cache_write)
        );
        let share = r
            .share_pct
            .map(|p| format!("{:.1}%", p))
            .unwrap_or_else(|| "—".to_string());
        let cost = r
            .cost
            .map(|c| format!("${:.4}", c))
            .unwrap_or_else(|| "n/a (local)".to_string());
        table.add_row(vec![
            &r.role,
            &r.sessions.to_string(),
            &format_number(r.tokens.total()),
            &in_out,
            &cache_rw,
            &format_number(r.tokens.reasoning),
            &share,
            &cost,
            &truncate(top_model, 28),
        ]);
    }
    println!("Usage by Sub-Agent Role (opencode `agent` label):");
    println!("{}", table);
    if let Some(unknown) = response.unknown_share_pct {
        println!(
            "  Note: {:.1}% of tokens have no `agent` label (attribution gap).",
            unknown
        );
    }
}

fn display_calls_series(points: &[otelite_core::api::CallsSeriesPoint]) {
    use chrono::{DateTime, Local, Utc};

    if points.is_empty() {
        println!("Call Volume Trend: no data in range");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    table.set_header(vec![
        Cell::new("Bucket").fg(Color::Cyan),
        Cell::new("Model").fg(Color::Cyan),
        Cell::new("Calls / Requests").fg(Color::Cyan),
    ]);

    for p in points {
        let dt = DateTime::<Utc>::from_timestamp_nanos(p.timestamp);
        let bucket_str = dt.with_timezone(&Local).format("%m-%d %H:%M").to_string();
        let model = p.model.as_deref().unwrap_or("(unknown)");

        table.add_row(vec![&bucket_str, model, &p.requests.to_string()]);
    }

    println!("Call Volume Trend (per bucket × model):");
    println!("{}", table);
}

fn display_session_models(breakdown: &otelite_core::api::SessionModelBreakdown) {
    if breakdown.rows.is_empty() {
        println!("Session × Model: no data in range");
        return;
    }
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("Session ID").fg(Color::Cyan),
        Cell::new("Model").fg(Color::Cyan),
        Cell::new("Requests").fg(Color::Cyan),
        Cell::new("Input").fg(Color::Cyan),
        Cell::new("Output").fg(Color::Cyan),
        Cell::new("Est. cost").fg(Color::Cyan),
    ]);
    for row in &breakdown.rows {
        table.add_row(vec![
            &truncate(&row.session_id, 16),
            &row.model,
            &row.requests.to_string(),
            &format_number(row.input_tokens),
            &format_number(row.output_tokens),
            &format_cost(row.cost),
        ]);
    }
    println!("Session × Model breakdown:");
    println!("{}", table);
}

fn display_speed_distribution(dist: &otelite_core::api::SpeedDistribution) {
    if dist.rows.is_empty() {
        println!("Speed distribution: no Claude Code span data in range");
        return;
    }
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("Speed / mode").fg(Color::Cyan),
        Cell::new("Model").fg(Color::Cyan),
        Cell::new("Requests").fg(Color::Cyan),
        Cell::new("Input").fg(Color::Cyan),
        Cell::new("Output").fg(Color::Cyan),
    ]);
    for row in &dist.rows {
        table.add_row(vec![
            row.speed.as_deref().unwrap_or("(not set)"),
            &row.model,
            &row.requests.to_string(),
            &format_number(row.input_tokens),
            &format_number(row.output_tokens),
        ]);
    }
    println!("Speed / Effort mode distribution:");
    println!("{}", table);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_time_range_hours() {
        let (start, end) = parse_time_range("24h").unwrap();
        let diff = end - start;
        let expected = 24 * 3600 * 1_000_000_000i64;
        assert_eq!(diff, expected);
    }

    #[test]
    fn test_parse_time_range_days() {
        let (start, end) = parse_time_range("7d").unwrap();
        let diff = end - start;
        let expected = 7 * 24 * 3600 * 1_000_000_000i64;
        assert_eq!(diff, expected);
    }

    #[test]
    fn test_parse_time_range_minutes() {
        let (start, end) = parse_time_range("30m").unwrap();
        let diff = end - start;
        let expected = 30 * 60 * 1_000_000_000i64;
        assert_eq!(diff, expected);
    }

    #[test]
    fn test_parse_cli_time_formats() {
        let day: i64 = 86_400_000_000_000;
        // 2026-08-01T00:00:00Z == 1785542400 s
        assert_eq!(
            parse_cli_time("2026-08-01").unwrap(),
            1_785_542_400 * 1_000_000_000
        );
        assert_eq!(
            parse_cli_time("2026-08-01T01:30:00").unwrap(),
            1_785_542_400 * 1_000_000_000 + 5_400_000_000_000
        );
        // Space separator and fractional seconds.
        assert_eq!(
            parse_cli_time("2026-08-01 01:30:00").unwrap(),
            1_785_542_400 * 1_000_000_000 + 5_400_000_000_000
        );
        assert_eq!(
            parse_cli_time("2026-08-01T01:30:00.250").unwrap(),
            1_785_542_400 * 1_000_000_000 + 5_400_250_000_000
        );
        // RFC 3339 with explicit offsets.
        assert_eq!(
            parse_cli_time("2026-08-01T02:30:00Z").unwrap(),
            1_785_542_400 * 1_000_000_000 + 9_000_000_000_000
        );
        // 02:30+02:00 == 00:30Z
        assert_eq!(
            parse_cli_time("2026-08-01T02:30:00+02:00").unwrap(),
            1_785_542_400 * 1_000_000_000 + 1_800_000_000_000
        );
        // Epoch seconds and nanoseconds.
        assert_eq!(
            parse_cli_time("1785542400").unwrap(),
            1_785_542_400_000_000_000
        );
        assert_eq!(
            parse_cli_time("1785542400000000000").unwrap(),
            1_785_542_400_000_000_000
        );
        // Sanity: the day constant is consistent.
        assert_eq!(day, 86_400 * 1_000_000_000);
        // Garbage is a clean error, not a panic.
        assert!(parse_cli_time("not-a-time").is_err());
        assert!(parse_cli_time("2026-13-45").is_err());
    }

    #[test]
    fn test_build_model_filters_semantics() {
        // No values: no filter.
        let f = build_model_filters(&[]);
        assert!(f.model.is_none() && f.models.is_none());
        // Single plain value keeps the exact-match dimension.
        let f = build_model_filters(&["claude-sonnet-4-6".to_string()]);
        assert_eq!(f.model.as_deref(), Some("claude-sonnet-4-6"));
        assert!(f.models.is_none());
        // A single glob uses the patterns dimension.
        let f = build_model_filters(&["claude-opus-*".to_string()]);
        assert!(f.model.is_none());
        assert_eq!(
            f.models.as_deref(),
            Some(&vec!["claude-opus-*".to_string()][..])
        );
        // Multiple values are ORed patterns.
        let f = build_model_filters(&["a".to_string(), "b-*".to_string()]);
        assert_eq!(
            f.models.as_deref(),
            Some(&vec!["a".to_string(), "b-*".to_string()][..])
        );
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(1234), "1,234");
        assert_eq!(format_number(1234567), "1,234,567");
        assert_eq!(format_number(123), "123");
    }

    #[test]
    fn test_format_cost() {
        assert_eq!(format_cost(Some(0.1234)), "$0.1234");
        assert_eq!(format_cost(None), "—");
    }
}
