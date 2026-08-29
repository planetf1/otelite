use otelite_core::api::{
    CacheHitRateByModel, CallsSeriesPoint, ContextTypeSplit, ConversationDepthStats,
    ErrorTypeBreakdown, HourOfDayBucket, LatencyStats, ModelDriftPair, StopReasonCount,
    TokenUsageResponse, ToolApprovalStats, ToolErrorEntry, ToolUsage, TruncationRateByModel,
};

/// One day × model row of the daily throughput panel (issue #119 slice #144).
/// Cell text is pre-formatted so the render path stays trivially testable:
/// "—" for missing values, "7†" for a weak (n < 10) throughput sample.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyThroughputRow {
    pub day: String,
    pub model: String,
    pub calls: usize,
    pub n_star: String,
    pub tps: String,
}

/// One emitter-identity row of the telemetry capability panel (issue #120).
/// Cell text is pre-formatted (`availability/quality[/derivation] (n/m)`) so
/// the render path stays trivially testable and the vocabulary
/// (absent/sparse/invalid/degenerate/correlated/unavailable) stays distinct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRow {
    pub identity: String,
    pub emitter: String,
    pub requests: usize,
    pub input: String,
    pub output: String,
    pub ttft: String,
    /// `matched/unmatched/rejected/ambiguous` candidate counts under the
    /// group's correlation rule, or `—` when no rule applies.
    pub correlation: String,
}

/// One (identity, metric) row of the model-performance diagnosis panel
/// (issue #121/#154). Cell text is pre-formatted so the render path stays
/// trivially testable and the assessment vocabulary (class, confidence,
/// "pct unavailable") comes verbatim from the API object — the panel never
/// recomputes a percentage or reclassifies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPerfRow {
    /// `(provider, request model)` composite; fingerprint kept separate.
    pub identity: String,
    /// Duration / throughput / ttft / error_rate (the four monitored metrics).
    pub metric: String,
    /// Preceding-baseline median (or rate); "—" when no eligible sample.
    pub baseline: String,
    /// Current-window median (or rate); "—" when no eligible sample.
    pub current: String,
    /// Preceding delta: "+300 (+30.00%)", "+30.0000 (pct unavailable)",
    /// "n/a" when there is no baseline sample at all.
    pub delta: String,
    /// Eligible sample counts: `current/preceding` (`/rolling` when present).
    pub samples: String,
    pub class: String,
    pub confidence: String,
}

/// Window header lines for the model-performance panel: the exact selected
/// current, preceding and rolling intervals plus the echoed timezone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelPerfWindows {
    pub lines: Vec<String>,
}

/// State for the Usage analytics view.
#[derive(Debug, Default)]
pub struct UsageState {
    pub token_usage: Option<TokenUsageResponse>,
    pub latency_stats: Vec<LatencyStats>,
    pub truncation_rate: Vec<TruncationRateByModel>,
    pub cache_hit_rate: Vec<CacheHitRateByModel>,
    pub conversation_depth: Option<ConversationDepthStats>,
    pub tool_usage: Vec<ToolUsage>,
    pub error_types: Vec<ErrorTypeBreakdown>,
    pub model_drift: Vec<ModelDriftPair>,
    pub tool_approvals: Option<ToolApprovalStats>,
    pub stop_reasons: Vec<StopReasonCount>,
    pub context_split: Vec<ContextTypeSplit>,
    pub tool_errors: Vec<ToolErrorEntry>,
    pub hour_of_day: Vec<HourOfDayBucket>,
    pub calls_series: Vec<CallsSeriesPoint>,
    /// Daily (calendar-day) throughput rows; empty until fetched.
    pub daily_throughput: Vec<DailyThroughputRow>,
    /// IANA timezone the daily buckets align to (None until fetched).
    pub daily_throughput_tz: Option<String>,
    /// Capability rows; empty until fetched.
    pub capabilities: Vec<CapabilityRow>,
    /// Unidentified-emitter diagnostics (#149): joined required attribute
    /// names and span count; empty until fetched.
    pub unidentified: Vec<(String, usize)>,
    /// Model-performance diagnosis rows (#154); empty until fetched.
    pub model_perf: Vec<ModelPerfRow>,
    /// Model-performance window header lines (#154).
    pub model_perf_windows: ModelPerfWindows,
    /// True once the first model-performance fetch has completed (success or
    /// failure) so the panel renders its no-data state instead of hiding.
    pub model_perf_fetched: bool,
    /// Model-performance fetch failure text; the panel is optional, so a
    /// failure blanks it rather than erroring the view.
    pub model_perf_error: Option<String>,
    pub error: Option<String>,
    pub is_loading: bool,
}

impl UsageState {
    pub fn set_error(&mut self, msg: String) {
        self.error = Some(msg);
        self.is_loading = false;
    }

    pub fn clear_error(&mut self) {
        self.error = None;
    }
}
