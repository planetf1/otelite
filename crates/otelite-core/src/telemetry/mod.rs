//! Telemetry data types for OTLP signals

pub mod formatting;
pub mod genai;
pub mod log;
pub mod metric;
pub mod resource;
pub mod trace;

pub use formatting::{format_attribute_preview, format_attribute_value};
pub use genai::{
    classify_span_capabilities, classify_ttft_value, correlate_codex_usage,
    correlated_token_evidence, extract_ttft_secs, is_codex_request_span, is_codex_usage_candidate,
    normalise_span_ttft_secs, unidentified_required_attributes, CorrelationOutcome,
    CorrelationRejection, GenAiEmitter, GenAiEmitterFingerprint, GenAiSpanCapabilities,
    GenAiSpanInfo, GenAiSpanRole, MetricDerivation, MetricObservation, MetricRejectionReason,
    TokenMetricEvidence, TtftMetricEvidence, TtftSourceUnit, TtftValueQuality,
    CODEX_CORRELATION_RULE,
};
pub use log::LogRecord;
pub use metric::Metric;
pub use resource::Resource;
pub use trace::{Span, Trace};
