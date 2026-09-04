use crate::error::{Error, Result};
use crate::models::{
    LogEntry, LogsQuery, LogsResponse, MetricResponse, Trace, TracesQuery, TracesResponse,
};
use reqwest::Client;
use std::time::Duration;

/// Map a non-2xx status to the client error it deserves: 404 becomes a
/// `NotFound` (the CLI exits 3 for it), everything else an `ApiError`.
fn non_success(status: reqwest::StatusCode, context: &str) -> Error {
    if status.as_u16() == 404 {
        Error::NotFound(format!("{}: resource not found", context))
    } else {
        Error::ApiError(format!("{}: HTTP {}", context, status))
    }
}

pub struct ApiClient {
    client: Client,
    base_url: String,
}

impl ApiClient {
    pub fn new(endpoint: String, timeout: Duration) -> Result<Self> {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| Error::ConnectionError(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            client,
            base_url: endpoint,
        })
    }

    pub async fn fetch_logs(&self, params: Vec<(&str, String)>) -> Result<LogsResponse> {
        let url = format!("{}/api/logs", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(non_success(response.status(), "Failed to fetch logs"));
        }

        Ok(response.json().await?)
    }

    pub async fn fetch_log_by_id(&self, timestamp: i64) -> Result<LogEntry> {
        let url = format!("{}/api/logs/{}", self.base_url, timestamp);
        let response = self.client.get(&url).send().await?;

        if response.status().as_u16() == 404 {
            return Err(Error::NotFound(format!(
                "Log at timestamp '{}' not found",
                timestamp
            )));
        }

        if !response.status().is_success() {
            return Err(non_success(response.status(), "Failed to fetch log"));
        }

        Ok(response.json().await?)
    }

    pub async fn search_logs(
        &self,
        query: &str,
        params: Vec<(&str, String)>,
    ) -> Result<LogsResponse> {
        let url = format!("{}/api/logs", self.base_url);
        let mut all_params = vec![("search", query.to_string())];
        all_params.extend(params);

        let response = self.client.get(&url).query(&all_params).send().await?;

        if !response.status().is_success() {
            return Err(non_success(response.status(), "Failed to search logs"));
        }

        Ok(response.json().await?)
    }

    pub async fn get_logs(&self, query: &LogsQuery) -> Result<LogsResponse> {
        let url = format!("{}/api/logs", self.base_url);
        let response = self.client.get(&url).query(query).send().await?;

        if !response.status().is_success() {
            return Err(non_success(response.status(), "Failed to fetch logs"));
        }

        Ok(response.json().await?)
    }

    pub async fn fetch_traces(&self, params: Vec<(&str, String)>) -> Result<TracesResponse> {
        let url = format!("{}/api/traces", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(non_success(response.status(), "Failed to fetch traces"));
        }

        Ok(response.json().await?)
    }

    pub async fn fetch_trace_by_id(&self, id: &str) -> Result<Trace> {
        let url = format!("{}/api/traces/{}", self.base_url, id);
        let response = self.client.get(&url).send().await?;

        if response.status().as_u16() == 404 {
            return Err(Error::NotFound(format!("Trace '{}' not found", id)));
        }

        if !response.status().is_success() {
            return Err(non_success(response.status(), "Failed to fetch trace"));
        }

        Ok(response.json().await?)
    }

    pub async fn get_traces(&self, query: &TracesQuery) -> Result<TracesResponse> {
        let url = format!("{}/api/traces", self.base_url);
        let response = self.client.get(&url).query(query).send().await?;

        if !response.status().is_success() {
            return Err(non_success(response.status(), "Failed to fetch traces"));
        }

        Ok(response.json().await?)
    }

    pub async fn fetch_metrics(&self, params: Vec<(&str, String)>) -> Result<Vec<MetricResponse>> {
        let url = format!("{}/api/metrics", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(non_success(response.status(), "Failed to fetch metrics"));
        }

        Ok(response.json().await?)
    }

    pub async fn fetch_metric_by_name(
        &self,
        name: &str,
        params: Vec<(&str, String)>,
    ) -> Result<Vec<MetricResponse>> {
        let url = format!("{}/api/metrics", self.base_url);
        let mut all_params = vec![("name", name.to_string())];
        all_params.extend(params);

        let response = self.client.get(&url).query(&all_params).send().await?;

        if !response.status().is_success() {
            return Err(non_success(response.status(), "Failed to fetch metric"));
        }

        let metrics: Vec<MetricResponse> = response.json().await?;

        if metrics.is_empty() {
            return Err(Error::NotFound(format!("Metric '{}' not found", name)));
        }

        Ok(metrics)
    }

    pub async fn export_logs(&self, params: Vec<(&str, String)>) -> Result<String> {
        let url = format!("{}/api/logs/export", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(non_success(response.status(), "Failed to export logs"));
        }

        Ok(response.text().await?)
    }

    pub async fn export_traces(&self, params: Vec<(&str, String)>) -> Result<String> {
        let url = format!("{}/api/traces/export", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(non_success(response.status(), "Failed to export traces"));
        }

        Ok(response.text().await?)
    }

    pub async fn export_metrics(&self, params: Vec<(&str, String)>) -> Result<String> {
        let url = format!("{}/api/metrics/export", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(non_success(response.status(), "Failed to export metrics"));
        }

        Ok(response.text().await?)
    }

    pub async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/health", self.base_url);
        match self.client.get(&url).send().await {
            Ok(response) => Ok(response.status().is_success()),
            Err(_) => Ok(false),
        }
    }

    /// Fetch logs associated with a specific trace ID.
    /// Uses the dedicated `/api/traces/{trace_id}/logs` endpoint for
    /// single-round-trip trace→log correlation.
    ///
    /// Optionally filter by full-text search in log body (e.g. "api_request_body").
    pub async fn fetch_logs_for_trace(
        &self,
        trace_id: &str,
        limit: Option<usize>,
        search: Option<&str>,
    ) -> Result<LogsResponse> {
        let url = format!("{}/api/traces/{}/logs", self.base_url, trace_id);
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(n) = limit {
            params.push(("limit", n.to_string()));
        }
        if let Some(s) = search {
            params.push(("search", s.to_string()));
        }
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch logs for trace",
            ));
        }

        Ok(response.json().await?)
    }

    pub async fn fetch_token_usage(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::TokenUsageResponse> {
        let url = format!("{}/api/genai/usage", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch token usage",
            ));
        }
        Ok(response.json().await?)
    }

    pub async fn fetch_latency_series(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<Vec<otelite_core::api::LatencySeriesPoint>> {
        let url = format!("{}/api/genai/latency_series", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        Self::read_items(response, "latency series").await
    }

    pub async fn fetch_calls_series(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<Vec<otelite_core::api::CallsSeriesPoint>> {
        let url = format!("{}/api/genai/calls_series", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        Self::read_items(response, "calls series").await
    }

    pub async fn fetch_latency_by_context(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<Vec<otelite_core::api::LatencyByContextBin>> {
        let url = format!("{}/api/genai/latency_by_context", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        Self::read_items(response, "latency by context").await
    }

    pub async fn fetch_latency_stats(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<Vec<otelite_core::api::LatencyStats>> {
        let url = format!("{}/api/genai/latency_stats", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        Self::read_items(response, "latency stats").await
    }

    /// Fetch a generic distribution over a named metric cohort (issue #133).
    pub async fn fetch_distribution(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::DistributionResponse> {
        let url = format!("{}/api/genai/distributions", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch distribution",
            ));
        }
        Ok(response.json().await?)
    }

    /// Fetch bucketed latency percentiles (issue #132).
    pub async fn fetch_latency_percentiles(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::LatencyPercentilesResponse> {
        let url = format!("{}/api/genai/latency_percentiles", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch latency percentiles",
            ));
        }
        Ok(response.json().await?)
    }

    pub async fn fetch_genai_capabilities(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::GenAiCapabilityResponse> {
        let url = format!("{}/api/genai/capabilities", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch GenAI capabilities",
            ));
        }
        Ok(response.json().await?)
    }

    /// Fetch the model-performance diagnosis (issue #121/#154). The caller
    /// selects the current interval, the optional rolling baseline length and
    /// an optional model/provider filter.
    pub async fn fetch_model_performance(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::ModelPerformanceDiagnosis> {
        let url = format!("{}/api/genai/model-performance", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch model performance",
            ));
        }
        Ok(response.json().await?)
    }

    pub async fn fetch_truncation_rate(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<Vec<otelite_core::api::TruncationRateByModel>> {
        let url = format!("{}/api/genai/truncation_rate", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        Self::read_items(response, "truncation rate").await
    }

    pub async fn fetch_cache_hit_rate(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<Vec<otelite_core::api::CacheHitRateByModel>> {
        let url = format!("{}/api/genai/cache_hit_rate", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        Self::read_items(response, "cache hit rate").await
    }

    pub async fn fetch_agent_roles(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::AgentRolesResponse> {
        let url = format!("{}/api/genai/agent_roles", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch agent roles",
            ));
        }
        Ok(response.json().await?)
    }

    pub async fn fetch_provider_mix(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::ProviderMixResponse> {
        let url = format!("{}/api/genai/provider_mix", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch provider mix",
            ));
        }
        Ok(response.json().await?)
    }

    /// Cache economics (per-model read/write split, hit rate, read:write
    /// ratio, estimated savings, time-bucketed series). Pass
    /// `("by_model", "1".into())` in `params`.
    pub async fn fetch_cache_economics(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::CacheEconomicsResponse> {
        let url = format!("{}/api/genai/cache_hit_rate", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch cache economics",
            ));
        }
        Ok(response.json().await?)
    }

    pub async fn fetch_reasoning_share(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::ReasoningShareResponse> {
        let url = format!("{}/api/genai/reasoning_share", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch reasoning share",
            ));
        }
        Ok(response.json().await?)
    }

    pub async fn fetch_agents(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::AgentRollupResponse> {
        let url = format!("{}/api/genai/agents", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch agent rollup",
            ));
        }
        Ok(response.json().await?)
    }

    pub async fn fetch_projects(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::ProjectRollupResponse> {
        let url = format!("{}/api/genai/projects", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch project rollup",
            ));
        }
        Ok(response.json().await?)
    }

    pub async fn fetch_session_costs(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::SessionCostResponse> {
        let url = format!("{}/api/sessions/costs", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch session costs",
            ));
        }
        Ok(response.json().await?)
    }

    pub async fn fetch_session_cost_distribution(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::CostDistributionResponse> {
        let url = format!("{}/api/sessions/cost-distribution", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch session cost distribution",
            ));
        }
        Ok(response.json().await?)
    }

    pub async fn fetch_conversation_depth(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::ConversationDepthStats> {
        let url = format!("{}/api/genai/conversation_depth", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch conversation depth",
            ));
        }
        Ok(response.json().await?)
    }

    pub async fn fetch_tool_usage(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<Vec<otelite_core::api::ToolUsage>> {
        let url = format!("{}/api/genai/tool_usage", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        Self::read_items(response, "tool usage").await
    }

    pub async fn fetch_error_types(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<Vec<otelite_core::api::ErrorTypeBreakdown>> {
        let url = format!("{}/api/genai/error_types", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        Self::read_items(response, "error types").await
    }

    pub async fn fetch_model_drift(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<Vec<otelite_core::api::ModelDriftPair>> {
        let url = format!("{}/api/genai/model_drift", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        Self::read_items(response, "model drift").await
    }

    pub async fn fetch_session_diagnose(
        &self,
        session_id: &str,
    ) -> Result<otelite_core::api::SessionDiagnoseResponse> {
        let url = format!("{}/api/sessions/{}/diagnose", self.base_url, session_id);
        let response = self.client.get(&url).send().await?;

        if response.status().as_u16() == 404 {
            return Err(Error::NotFound(format!(
                "Session '{}' not found",
                session_id
            )));
        }

        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch session diagnose",
            ));
        }

        Ok(response.json().await?)
    }

    /// List recent sessions with summary stats.
    /// `params` are forwarded as query string (e.g. start_time, end_time, limit).
    pub async fn fetch_sessions(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::SessionListResponse> {
        let url = format!("{}/api/sessions", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(response.status(), "Failed to fetch sessions"));
        }
        Ok(response.json().await?)
    }

    pub async fn fetch_tool_approvals(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::ToolApprovalStats> {
        let url = format!("{}/api/genai/tool_approvals", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch tool approvals",
            ));
        }
        Ok(response.json().await?)
    }

    pub async fn fetch_stop_reasons(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<Vec<otelite_core::api::StopReasonCount>> {
        let url = format!("{}/api/genai/stop_reasons", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        Self::read_items(response, "stop reasons").await
    }

    pub async fn fetch_context_type_split(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<Vec<otelite_core::api::ContextTypeSplit>> {
        let url = format!("{}/api/genai/context_type_split", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        Self::read_items(response, "context type split").await
    }

    pub async fn fetch_tool_errors(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<Vec<otelite_core::api::ToolErrorEntry>> {
        let url = format!("{}/api/genai/tool_errors", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        Self::read_items(response, "tool errors").await
    }

    pub async fn fetch_hour_of_day(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<Vec<otelite_core::api::HourOfDayBucket>> {
        let url = format!("{}/api/genai/hour_of_day", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        Self::read_items(response, "hour of day").await
    }

    pub async fn fetch_top_spans(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<Vec<otelite_core::api::TopSpan>> {
        let url = format!("{}/api/genai/top_spans", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        Self::read_items(response, "top spans").await
    }

    pub async fn fetch_tool_failure_rates(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::ToolFailureRatesResponse> {
        let url = format!("{}/api/genai/tool_failure_rates", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch tool failure rates",
            ));
        }
        Ok(response.json().await?)
    }

    pub async fn fetch_daily_tool_mix(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::DailyToolMixResponse> {
        let url = format!("{}/api/genai/daily_tool_mix", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch daily tool mix",
            ));
        }
        Ok(response.json().await?)
    }

    pub async fn fetch_skill_activity(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::SkillActivityResponse> {
        let url = format!("{}/api/genai/skill_activity", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch skill activity",
            ));
        }
        Ok(response.json().await?)
    }

    pub async fn fetch_session_quality_summary(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::SessionQualitySummary> {
        let url = format!("{}/api/genai/session_quality_summary", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch session quality summary",
            ));
        }
        Ok(response.json().await?)
    }

    pub async fn fetch_skill_outcomes(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::SkillOutcomesResponse> {
        let url = format!("{}/api/genai/skill_outcomes", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch skill outcomes",
            ));
        }
        Ok(response.json().await?)
    }

    pub async fn fetch_model_selection_heatmap(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::ModelSelectionHeatmapResponse> {
        let url = format!("{}/api/genai/model_selection_heatmap", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch model selection heatmap",
            ));
        }
        Ok(response.json().await?)
    }

    pub async fn fetch_recent_errors(
        &self,
        params: Vec<(&str, String)>,
    ) -> Result<otelite_core::api::RecentErrorsResponse> {
        let url = format!("{}/api/genai/recent_errors", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;
        if !response.status().is_success() {
            return Err(non_success(
                response.status(),
                "Failed to fetch recent errors",
            ));
        }
        Ok(response.json().await?)
    }

    /// The GenAI list endpoints wrap their array payload in an
    /// `{ items, filters_applied }` envelope (#135). Parse it and return
    /// the items; the TUI has no filter bar, so the echo is discarded.
    async fn read_items<T: serde::de::DeserializeOwned>(
        response: reqwest::Response,
        what: &str,
    ) -> Result<Vec<T>> {
        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to fetch {}: HTTP {}",
                what,
                response.status()
            )));
        }
        let envelope: serde_json::Value = response.json().await?;
        let items = envelope.get("items").ok_or_else(|| {
            Error::ApiError(format!("{} response is missing its 'items' envelope", what))
        })?;
        Ok(serde_json::from_value(items.clone())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use mockito::Server;

    #[test]
    fn test_api_client_creation() {
        let client = ApiClient::new("http://localhost:8080".to_string(), Duration::from_secs(30));
        assert!(client.is_ok());
    }

    #[test]
    fn test_api_client_invalid_timeout() {
        let client = ApiClient::new(
            "http://localhost:8080".to_string(),
            Duration::from_millis(1),
        );
        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn test_fetch_logs_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/logs")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "logs": [
                {
                    "timestamp": 1705315800000000000,
                    "severity": "INFO",
                    "severity_text": "INFO",
                    "body": "Test log message",
                    "attributes": {},
                    "resource": null,
                    "trace_id": null,
                    "span_id": null
                }
                ],
                "total": 1,
                "limit": 10,
                "offset": 0
            }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_logs(vec![("limit", "10".to_string())]).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let logs = result.unwrap();
        assert_eq!(logs.logs.len(), 1);
        assert_eq!(logs.logs[0].timestamp, 1705315800000000000);
        assert_eq!(logs.logs[0].severity, "INFO");
    }

    #[tokio::test]
    async fn test_fetch_logs_empty_response() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/logs")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"logs": [], "total": 0, "limit": 100, "offset": 0}"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_logs(vec![]).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().logs.len(), 0);
    }

    #[tokio::test]
    async fn test_fetch_logs_server_error() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/logs")
            .with_status(500)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_logs(vec![]).await;

        mock.assert_async().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::ApiError(msg) => assert!(msg.contains("500")),
            _ => panic!("Expected ApiError"),
        }
    }

    #[tokio::test]
    async fn test_fetch_log_by_id_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/logs/1705315800000000000")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "timestamp": 1705315800000000000,
                "severity": "ERROR",
                "severity_text": "ERROR",
                "body": "Error occurred",
                "attributes": {"key": "value"},
                "resource": null,
                "trace_id": null,
                "span_id": null
            }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_log_by_id(1705315800000000000).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let log = result.unwrap();
        assert_eq!(log.timestamp, 1705315800000000000);
        assert_eq!(log.severity, "ERROR");
        assert_eq!(log.body, "Error occurred");
    }

    #[tokio::test]
    async fn test_fetch_log_by_id_not_found() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/logs/9999999999999999")
            .with_status(404)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_log_by_id(9999999999999999).await;

        mock.assert_async().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::NotFound(msg) => assert!(msg.contains("9999999999999999")),
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_search_logs_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/logs")
            .match_query(mockito::Matcher::AllOf(vec![mockito::Matcher::UrlEncoded(
                "search".into(),
                "error".into(),
            )]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"logs": [], "total": 0, "limit": 100, "offset": 0}"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.search_logs("error", vec![]).await;

        mock.assert_async().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_fetch_traces_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/traces")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "traces": [
                {
                    "trace_id": "trace1",
                    "root_span_name": "http-request",
                    "start_time": 1705315800000000000,
                    "duration": 1500000000,
                    "span_count": 1,
                    "service_names": [],
                    "has_errors": false
                }
                ],
                "total": 1,
                "limit": 10,
                "offset": 0
            }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_traces(vec![("limit", "10".to_string())]).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let traces = result.unwrap();
        assert_eq!(traces.traces.len(), 1);
        assert_eq!(traces.traces[0].trace_id, "trace1");
        assert!(!traces.traces[0].has_errors);
    }

    #[tokio::test]
    async fn test_fetch_trace_by_id_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/traces/trace123")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "trace_id": "trace123",
                "spans": [
                    {
                        "span_id": "span1",
                        "trace_id": "trace123",
                        "parent_span_id": null,
                        "name": "database-query",
                        "kind": "Internal",
                        "start_time": 1705315800000000000,
                        "end_time": 1705315800250000000,
                        "duration": 250000000,
                        "attributes": {},
                        "resource": null,
                        "status": {"code": "OK", "message": null},
                        "events": []
                    }
                ],
                "start_time": 1705315800000000000,
                "end_time": 1705315800250000000,
                "duration": 250000000,
                "span_count": 1,
                "service_names": []
            }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_trace_by_id("trace123").await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let trace = result.unwrap();
        assert_eq!(trace.trace_id, "trace123");
        assert_eq!(trace.spans.len(), 1);
    }

    #[tokio::test]
    async fn test_fetch_trace_by_id_not_found() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/traces/nonexistent")
            .with_status(404)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_trace_by_id("nonexistent").await;

        mock.assert_async().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::NotFound(msg) => assert!(msg.contains("nonexistent")),
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_fetch_metrics_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/metrics")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"[
                {
                    "name": "http_requests_total",
                    "description": null,
                    "unit": null,
                    "metric_type": "counter",
                    "value": 1234,
                    "timestamp": 1705315800000000000,
                    "attributes": {},
                    "resource": null
                }
            ]"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_metrics(vec![]).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let metrics = result.unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].name, "http_requests_total");
    }

    #[tokio::test]
    async fn test_fetch_metric_by_name_not_found() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/metrics")
            .match_query(mockito::Matcher::UrlEncoded(
                "name".into(),
                "nonexistent_metric".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"[]"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client
            .fetch_metric_by_name("nonexistent_metric", vec![])
            .await;

        mock.assert_async().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::NotFound(msg) => assert!(msg.contains("nonexistent_metric")),
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_health_check_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/health")
            .with_status(200)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.health_check().await;

        mock.assert_async().await;
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn test_health_check_unreachable() {
        let client = ApiClient::new(
            "http://127.0.0.1:19999".to_string(),
            Duration::from_millis(100),
        )
        .unwrap();
        let result = client.health_check().await;
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_fetch_logs_for_trace_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/traces/trace123/logs")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "logs": [
                {
                    "timestamp": 1705315800000000000,
                    "severity": "ERROR",
                    "severity_text": "ERROR",
                    "body": "timeout in trace",
                    "attributes": {"body_length": "2048"},
                    "resource": null,
                    "trace_id": "trace123",
                    "span_id": "span001"
                }
                ],
                "total": 1,
                "limit": 10,
                "offset": 0
            }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client
            .fetch_logs_for_trace("trace123", Some(10), None)
            .await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let logs_response = result.unwrap();
        assert_eq!(logs_response.logs.len(), 1);
        assert_eq!(logs_response.total, 1);
        assert_eq!(logs_response.logs[0].trace_id, Some("trace123".to_string()));
        assert_eq!(
            logs_response.logs[0].attributes.get("body_length"),
            Some(&"2048".to_string())
        );
    }

    #[tokio::test]
    async fn test_fetch_logs_for_trace_with_search() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/traces/trace123/logs")
            .match_query(mockito::Matcher::UrlEncoded(
                "search".into(),
                "api_request_body".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "logs": [],
                "total": 0,
                "limit": 5,
                "offset": 0
            }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client
            .fetch_logs_for_trace("trace123", Some(5), Some("api_request_body"))
            .await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let logs_response = result.unwrap();
        assert_eq!(logs_response.logs.len(), 0);
        assert_eq!(logs_response.total, 0);
    }

    #[tokio::test]
    async fn test_fetch_logs_for_trace_not_found() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/traces/nonexistent/logs")
            .with_status(404)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_logs_for_trace("nonexistent", None, None).await;

        mock.assert_async().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_fetch_tool_approvals_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/genai/tool_approvals")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"total":10,"auto_accepted":8,"user_accepted":1,"rejected":1,"unknown":0,"top_rejected":[],"filters_applied":["agent","model","provider","project","session"]}"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_tool_approvals(vec![]).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let stats = result.unwrap();
        assert_eq!(stats.total, 10);
        assert_eq!(stats.auto_accepted, 8);
    }

    #[tokio::test]
    async fn test_fetch_stop_reasons_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/genai/stop_reasons")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"items":[{"reason":"tool_use","count":85},{"reason":"end_turn","count":15}],"filters_applied":[]}"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_stop_reasons(vec![]).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let reasons = result.unwrap();
        assert_eq!(reasons.len(), 2);
        assert_eq!(reasons[0].reason, "tool_use");
        assert_eq!(reasons[0].count, 85);
    }

    #[tokio::test]
    async fn test_fetch_context_type_split_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/genai/context_type_split")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"items":[{"context":"code","calls":42,"input_tokens":1000,"output_tokens":500,"avg_ms":120.5}],"filters_applied":[]}"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_context_type_split(vec![]).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let split = result.unwrap();
        assert_eq!(split.len(), 1);
        assert_eq!(split[0].context, "code");
        assert_eq!(split[0].calls, 42);
    }

    #[tokio::test]
    async fn test_fetch_tool_errors_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/genai/tool_errors")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"items":[{"tool_name":"bash","error_message":"Permission denied","count":3}],"filters_applied":[]}"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_tool_errors(vec![]).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let errors = result.unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].tool_name, "bash");
        assert_eq!(errors[0].count, 3);
    }

    #[tokio::test]
    async fn test_fetch_hour_of_day_success() {
        let mut server = Server::new_async().await;
        // Return 24 buckets as the real endpoint does
        let items: String = (0u32..24)
            .map(|h| format!(r#"{{"hour":{h},"llm_calls":{},"tool_calls":{}}}"#, h * 2, h))
            .collect::<Vec<_>>()
            .join(",");
        let body = format!(r#"{{"items":[{items}],"filters_applied":[]}}"#);
        let mock = server
            .mock("GET", "/api/genai/hour_of_day")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_hour_of_day(vec![]).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let buckets = result.unwrap();
        assert_eq!(buckets.len(), 24);
        assert_eq!(buckets[0].hour, 0);
        assert_eq!(buckets[0].llm_calls, 0);
        assert_eq!(buckets[23].hour, 23);
        assert_eq!(buckets[23].llm_calls, 46);
    }

    #[tokio::test]
    async fn test_fetch_cache_hit_rate_unwraps_items_envelope() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/genai/cache_hit_rate")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"items":[{"model":"claude-sonnet-4-5","total_input_tokens":100,
                        "total_cache_read_tokens":40,"total_cache_creation_tokens":10,
                        "hit_rate":0.2857}],
                    "filters_applied":["model"]}"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client
            .fetch_cache_hit_rate(vec![("model", "claude-sonnet-4-5".into())])
            .await;

        mock.assert_async().await;
        let rows = result.expect("envelope response must parse");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].model.as_deref(), Some("claude-sonnet-4-5"));
        assert_eq!(rows[0].total_cache_read_tokens, 40);
    }

    #[tokio::test]
    async fn test_fetch_cache_hit_rate_missing_items_errors() {
        // A bare array (pre-#135 shape) must fail loudly, not parse as empty.
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/genai/cache_hit_rate")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"[]"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_cache_hit_rate(vec![]).await;

        mock.assert_async().await;
        let err = result.expect_err("bare array is not the envelope shape");
        assert!(
            err.to_string().contains("missing its 'items' envelope"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn test_fetch_genai_capabilities_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/genai/capabilities")
            .match_query(mockito::Matcher::UrlEncoded(
                "model".to_string(),
                "gpt-5".to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "reports": [],
                    "canonical_span_count": 0,
                    "duplicate_span_count": 0,
                    "truncated": false,
                    "filters_applied": ["agent", "model", "provider", "project", "session"]
                }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client
            .fetch_genai_capabilities(vec![("model", "gpt-5".to_string())])
            .await;

        mock.assert_async().await;
        let report = result.unwrap();
        assert_eq!(report.canonical_span_count, 0);
        assert!(!report.truncated);
    }

    #[tokio::test]
    async fn test_fetch_model_performance_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/genai/model-performance")
            .match_query(mockito::Matcher::UrlEncoded(
                "start_time".to_string(),
                "100".to_string(),
            ))
            .match_query(mockito::Matcher::UrlEncoded(
                "end_time".to_string(),
                "200".to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "current_window": {"start_time": 100, "end_time": 200},
                    "preceding_window": {"start_time": 0, "end_time": 100},
                    "truncated": false,
                    "identities": [],
                    "assessments": []
                }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client
            .fetch_model_performance(vec![
                ("start_time", "100".to_string()),
                ("end_time", "200".to_string()),
            ])
            .await;

        mock.assert_async().await;
        let diag = result.unwrap();
        assert_eq!(diag.current_window.start_time, 100);
        assert_eq!(diag.preceding_window.end_time, 100);
        assert!(diag.rolling_window.is_none());
        assert!(diag.identities.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_model_performance_server_error() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/genai/model-performance")
            .with_status(500)
            .create_async()
            .await;
        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_model_performance(vec![]).await;
        mock.assert_async().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::ApiError(msg) => assert!(msg.contains("500")),
            _ => panic!("Expected ApiError"),
        }
    }

    #[tokio::test]
    async fn test_fetch_tool_approvals_server_error() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/genai/tool_approvals")
            .with_status(500)
            .create_async()
            .await;
        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_tool_approvals(vec![]).await;
        mock.assert_async().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::ApiError(msg) => assert!(msg.contains("500")),
            _ => panic!("Expected ApiError"),
        }
    }

    #[tokio::test]
    async fn test_fetch_stop_reasons_server_error() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/genai/stop_reasons")
            .with_status(500)
            .create_async()
            .await;
        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_stop_reasons(vec![]).await;
        mock.assert_async().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::ApiError(msg) => assert!(msg.contains("500")),
            _ => panic!("Expected ApiError"),
        }
    }

    #[tokio::test]
    async fn test_fetch_context_type_split_server_error() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/genai/context_type_split")
            .with_status(500)
            .create_async()
            .await;
        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_context_type_split(vec![]).await;
        mock.assert_async().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::ApiError(msg) => assert!(msg.contains("500")),
            _ => panic!("Expected ApiError"),
        }
    }

    #[tokio::test]
    async fn test_fetch_tool_errors_server_error() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/genai/tool_errors")
            .with_status(500)
            .create_async()
            .await;
        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_tool_errors(vec![]).await;
        mock.assert_async().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::ApiError(msg) => assert!(msg.contains("500")),
            _ => panic!("Expected ApiError"),
        }
    }

    #[tokio::test]
    async fn test_fetch_hour_of_day_server_error() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/genai/hour_of_day")
            .with_status(500)
            .create_async()
            .await;
        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_hour_of_day(vec![]).await;
        mock.assert_async().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::ApiError(msg) => assert!(msg.contains("500")),
            _ => panic!("Expected ApiError"),
        }
    }
    #[tokio::test]
    async fn test_fetch_tool_approvals_echoes_filters_applied() {
        // #135: every genai endpoint echoes the filter dimensions it applied;
        // unsupported params are ignored, never a 400.
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/genai/tool_approvals")
            .match_query(mockito::Matcher::UrlEncoded(
                "agent".to_string(),
                "opencode".to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"total":0,"auto_accepted":0,"user_accepted":0,"rejected":0,"unknown":0,"top_rejected":[],"filters_applied":[]}"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client
            .fetch_tool_approvals(vec![("agent", "opencode".to_string())])
            .await;

        mock.assert_async().await;
        let stats = result.unwrap();
        // rollup endpoint: the bar's dimensions aren't applied
        assert_eq!(stats.filters_applied, Vec::<String>::new());
    }
}
