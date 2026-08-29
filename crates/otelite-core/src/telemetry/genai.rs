//! GenAI/LLM span detection and parsing.
//!
//! This module provides utilities for detecting and extracting information from
//! OpenTelemetry spans that follow the GenAI semantic conventions.
//!
//! See: https://opentelemetry.io/docs/specs/semconv/gen-ai/

use super::trace::Span;
use crate::semconv;
use std::collections::HashMap;

/// Quality of a time-to-first-token observation after normalisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtftValueQuality {
    /// The span did not carry a supported TTFT attribute.
    Absent,
    /// The value is finite, non-negative and no greater than span duration.
    Valid,
    /// The emitter supplied a value that cannot represent first-token latency.
    Invalid,
}

/// Emitter family recognised from a verified span signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenAiEmitter {
    ClaudeCode,
    Codex,
    OpenCode,
    StandardOtel,
    Unknown,
    Ambiguous,
}

/// Role of a span recognised by a GenAI adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenAiSpanRole {
    /// The span represents one completed request whose duration is meaningful.
    RequestTiming,
    /// The span holds request usage but does not have verified timing semantics.
    RequestUsage,
    /// The span is not a verified GenAI request shape.
    Other,
}

/// Whether one metric attribute was absent, valid, or invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricObservation {
    Absent,
    Valid,
    Invalid,
}

/// How a metric was associated with a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricDerivation {
    /// The value was observed directly on the verified request span.
    Native,
    /// The value was associated by a separately verified correlation rule.
    Correlated,
    /// No value can be associated safely.
    Unavailable,
}

/// Why a metric cannot be used for request-level analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricRejectionReason {
    MissingNativeAttribute,
    UnverifiedUsageSignature,
    NotARequestSpan,
    AmbiguousEmitter,
    InvalidInteger,
    InvalidSeconds,
    InvalidDuration,
}

/// Unit used by the emitter for an observed TTFT attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtftSourceUnit {
    Seconds,
    Milliseconds,
}

/// Stable non-content fields that identify the emitter rule which matched a span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenAiEmitterFingerprint {
    pub adapter_rule: &'static str,
    pub service_name: Option<String>,
    pub scope_name: Option<String>,
    pub scope_version: Option<String>,
}

/// Evidence for one token counter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenMetricEvidence {
    pub observation: MetricObservation,
    pub derivation: MetricDerivation,
    pub source_attribute: Option<&'static str>,
    pub value: Option<u64>,
    pub rejection_reason: Option<MetricRejectionReason>,
}

/// Evidence for one time-to-first-token observation, normalised to seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct TtftMetricEvidence {
    pub observation: MetricObservation,
    pub derivation: MetricDerivation,
    pub source_attribute: Option<&'static str>,
    pub source_unit: Option<TtftSourceUnit>,
    pub seconds: Option<f64>,
    pub rejection_reason: Option<MetricRejectionReason>,
}

/// Capabilities and evidence observed on one span.
#[derive(Debug, Clone, PartialEq)]
pub struct GenAiSpanCapabilities {
    pub emitter: GenAiEmitter,
    pub role: GenAiSpanRole,
    pub fingerprint: GenAiEmitterFingerprint,
    pub input_tokens: TokenMetricEvidence,
    pub output_tokens: TokenMetricEvidence,
    pub cache_creation_tokens: TokenMetricEvidence,
    pub cache_read_tokens: TokenMetricEvidence,
    pub ttft: TtftMetricEvidence,
}

/// Classify one span using verified request signatures and per-metric evidence.
///
/// This deliberately does not correlate separate spans. In particular, Codex
/// usage remains unavailable until a deterministic one-to-one join is verified.
pub fn classify_span_capabilities(span: &Span) -> GenAiSpanCapabilities {
    let attrs = &span.attributes;
    let has_model = first_attribute(attrs, semconv::MODEL_KEYS).is_some();
    let has_standard_metric = first_attribute(attrs, semconv::INPUT_TOKEN_KEYS).is_some()
        || first_attribute(attrs, semconv::OUTPUT_TOKEN_KEYS).is_some()
        || first_attribute(attrs, semconv::CACHE_CREATION_TOKEN_KEYS).is_some()
        || first_attribute(attrs, semconv::CACHE_READ_TOKEN_KEYS).is_some()
        || raw_ttft(attrs).is_some();
    let mut candidates = Vec::new();

    if span.name.starts_with("claude_code.llm_request") && has_model {
        candidates.push((GenAiEmitter::ClaudeCode, "claude-code-request-v1"));
    }
    if span.name == semconv::CODEX_LLM_REQUEST_SPAN_NAME
        && has_model
        && attrs
            .get("otel.scope.name")
            .is_some_and(|scope| scope == semconv::CODEX_OTEL_SCOPE_NAME)
    {
        candidates.push((GenAiEmitter::Codex, "codex-request-v1"));
    }
    if attrs
        .get("openinference.span.kind")
        .is_some_and(|kind| kind == "LLM")
        && attrs
            .get("llm.system")
            .is_some_and(|system| system == "opencode-go")
        && has_model
    {
        candidates.push((GenAiEmitter::OpenCode, "opencode-request-v1"));
    }
    if candidates.is_empty()
        && first_attribute(attrs, semconv::SYSTEM_KEYS).is_some()
        && has_model
        && has_standard_metric
    {
        candidates.push((GenAiEmitter::StandardOtel, "standard-otel-request-v1"));
    }

    let (emitter, role, adapter_rule) = match candidates.as_slice() {
        [] => (
            GenAiEmitter::Unknown,
            GenAiSpanRole::Other,
            "unidentified-v1",
        ),
        [(emitter, rule)] => (*emitter, GenAiSpanRole::RequestTiming, *rule),
        _ => (
            GenAiEmitter::Ambiguous,
            GenAiSpanRole::Other,
            "ambiguous-signature-v1",
        ),
    };
    let fingerprint = GenAiEmitterFingerprint {
        adapter_rule,
        service_name: span
            .resource
            .as_ref()
            .and_then(|resource| resource.service_name().cloned()),
        scope_name: attrs.get("otel.scope.name").cloned(),
        scope_version: attrs.get("otel.scope.version").cloned(),
    };
    let unavailable_reason = match emitter {
        GenAiEmitter::Codex => MetricRejectionReason::UnverifiedUsageSignature,
        GenAiEmitter::Ambiguous => MetricRejectionReason::AmbiguousEmitter,
        GenAiEmitter::Unknown => MetricRejectionReason::NotARequestSpan,
        _ => MetricRejectionReason::MissingNativeAttribute,
    };
    let duration_secs = (span.end_time.saturating_sub(span.start_time)) as f64 / 1_000_000_000.0;
    let native_request = role == GenAiSpanRole::RequestTiming;

    GenAiSpanCapabilities {
        emitter,
        role,
        fingerprint,
        input_tokens: token_evidence(
            attrs,
            semconv::INPUT_TOKEN_KEYS,
            native_request,
            unavailable_reason,
        ),
        output_tokens: token_evidence(
            attrs,
            semconv::OUTPUT_TOKEN_KEYS,
            native_request,
            unavailable_reason,
        ),
        cache_creation_tokens: token_evidence(
            attrs,
            semconv::CACHE_CREATION_TOKEN_KEYS,
            native_request,
            unavailable_reason,
        ),
        cache_read_tokens: token_evidence(
            attrs,
            semconv::CACHE_READ_TOKEN_KEYS,
            native_request,
            unavailable_reason,
        ),
        ttft: ttft_evidence(attrs, duration_secs, native_request, unavailable_reason),
    }
}

/// Extract TTFT from span attributes, normalising to **seconds**.
///
/// Attribute priority:
/// - `gen_ai.server.time_to_first_token` — OTel GenAI spec, already in seconds
/// - `llm.time_to_first_token` — non-standard, assumed seconds
/// - `ttft_ms` — Claude Code custom attribute, in **milliseconds**; divided by 1000
pub fn extract_ttft_secs(attrs: &HashMap<String, String>) -> Option<f64> {
    let (_, raw, unit) = raw_ttft(attrs)?;
    normalise_ttft_secs(raw, unit).ok()
}

fn first_attribute<'a>(
    attrs: &'a HashMap<String, String>,
    keys: &[&'static str],
) -> Option<(&'static str, &'a str)> {
    keys.iter()
        .find_map(|key| attrs.get(*key).map(|value| (*key, value.as_str())))
}

fn raw_ttft(attrs: &HashMap<String, String>) -> Option<(&'static str, &str, TtftSourceUnit)> {
    if let Some(value) = attrs.get("gen_ai.server.time_to_first_token") {
        return Some((
            "gen_ai.server.time_to_first_token",
            value,
            TtftSourceUnit::Seconds,
        ));
    }
    if let Some(value) = attrs.get("llm.time_to_first_token") {
        return Some(("llm.time_to_first_token", value, TtftSourceUnit::Seconds));
    }
    attrs
        .get("ttft_ms")
        .map(|value| ("ttft_ms", value.as_str(), TtftSourceUnit::Milliseconds))
}

fn normalise_ttft_secs(raw: &str, unit: TtftSourceUnit) -> Result<f64, MetricRejectionReason> {
    let value = raw
        .parse::<f64>()
        .map_err(|_| MetricRejectionReason::InvalidSeconds)?;
    if !value.is_finite() {
        return Err(MetricRejectionReason::InvalidSeconds);
    }
    Ok(match unit {
        TtftSourceUnit::Seconds => value,
        TtftSourceUnit::Milliseconds => value / 1000.0,
    })
}

fn token_evidence(
    attrs: &HashMap<String, String>,
    keys: &[&'static str],
    native_request: bool,
    absent_reason: MetricRejectionReason,
) -> TokenMetricEvidence {
    let Some((source_attribute, raw)) = first_attribute(attrs, keys) else {
        return TokenMetricEvidence {
            observation: MetricObservation::Absent,
            derivation: MetricDerivation::Unavailable,
            source_attribute: None,
            value: None,
            rejection_reason: Some(absent_reason),
        };
    };
    match raw.parse::<u64>() {
        Ok(value) => TokenMetricEvidence {
            observation: MetricObservation::Valid,
            derivation: if native_request {
                MetricDerivation::Native
            } else {
                MetricDerivation::Unavailable
            },
            source_attribute: Some(source_attribute),
            value: Some(value),
            rejection_reason: (!native_request).then_some(absent_reason),
        },
        Err(_) => TokenMetricEvidence {
            observation: MetricObservation::Invalid,
            derivation: if native_request {
                MetricDerivation::Native
            } else {
                MetricDerivation::Unavailable
            },
            source_attribute: Some(source_attribute),
            value: None,
            rejection_reason: Some(MetricRejectionReason::InvalidInteger),
        },
    }
}

fn ttft_evidence(
    attrs: &HashMap<String, String>,
    duration_secs: f64,
    native_request: bool,
    absent_reason: MetricRejectionReason,
) -> TtftMetricEvidence {
    let Some((source_attribute, raw, source_unit)) = raw_ttft(attrs) else {
        return TtftMetricEvidence {
            observation: MetricObservation::Absent,
            derivation: MetricDerivation::Unavailable,
            source_attribute: None,
            source_unit: None,
            seconds: None,
            rejection_reason: Some(absent_reason),
        };
    };
    let seconds = match normalise_ttft_secs(raw, source_unit) {
        Ok(seconds) => seconds,
        Err(reason) => {
            return TtftMetricEvidence {
                observation: MetricObservation::Invalid,
                derivation: if native_request {
                    MetricDerivation::Native
                } else {
                    MetricDerivation::Unavailable
                },
                source_attribute: Some(source_attribute),
                source_unit: Some(source_unit),
                seconds: None,
                rejection_reason: Some(reason),
            };
        },
    };
    let rejection_reason = (classify_ttft_value(Some(seconds), duration_secs)
        != TtftValueQuality::Valid)
        .then_some(MetricRejectionReason::InvalidDuration);
    TtftMetricEvidence {
        observation: if rejection_reason.is_some() {
            MetricObservation::Invalid
        } else {
            MetricObservation::Valid
        },
        derivation: if native_request {
            MetricDerivation::Native
        } else {
            MetricDerivation::Unavailable
        },
        source_attribute: Some(source_attribute),
        source_unit: Some(source_unit),
        seconds: rejection_reason.is_none().then_some(seconds),
        rejection_reason: rejection_reason.or((!native_request).then_some(absent_reason)),
    }
}

/// Classify a normalised TTFT value against its enclosing span duration.
///
/// Group-level degeneracy is deliberately not decided here: it requires a
/// population of valid observations and is handled by analytics consumers.
pub fn classify_ttft_value(ttft_secs: Option<f64>, duration_secs: f64) -> TtftValueQuality {
    let Some(ttft_secs) = ttft_secs else {
        return TtftValueQuality::Absent;
    };

    if !ttft_secs.is_finite()
        || !duration_secs.is_finite()
        || ttft_secs.is_sign_negative()
        || duration_secs.is_sign_negative()
        || ttft_secs > duration_secs
    {
        return TtftValueQuality::Invalid;
    }

    TtftValueQuality::Valid
}

/// Information extracted from a GenAI/LLM span.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GenAiSpanInfo {
    /// The GenAI system (e.g., "openai", "anthropic", "azure_openai")
    pub system: Option<String>,
    /// The model name (e.g., "gpt-4", "claude-sonnet-4-20250514")
    pub model: Option<String>,
    /// The response model (may differ from request model due to routing)
    pub response_model: Option<String>,
    /// The operation name (e.g., "chat", "text_completion", "embeddings")
    pub operation: Option<String>,
    /// Number of input tokens
    pub input_tokens: Option<u64>,
    /// Number of output tokens
    pub output_tokens: Option<u64>,
    /// Total tokens (may be computed or explicit)
    pub total_tokens: Option<u64>,
    /// Cache creation input tokens (Anthropic prompt caching)
    pub cache_creation_tokens: Option<u64>,
    /// Cache read input tokens (Anthropic prompt caching)
    pub cache_read_tokens: Option<u64>,
    /// Temperature parameter
    pub temperature: Option<f64>,
    /// Maximum tokens requested
    pub max_tokens: Option<u64>,
    /// Finish reasons (e.g., ["stop", "length", "tool_calls"])
    pub finish_reasons: Vec<String>,
    /// Whether this span has any GenAI attributes
    pub is_genai: bool,
    /// Response ID from the model provider (gen_ai.response.id)
    pub response_id: Option<String>,
    /// Tool name for tool call spans (gen_ai.tool.name)
    pub tool_name: Option<String>,
    /// Tool call ID (gen_ai.tool.call.id)
    pub tool_call_id: Option<String>,
    /// Tool type, e.g. "function" (gen_ai.tool.type)
    pub tool_type: Option<String>,
    /// Top-p sampling parameter (gen_ai.request.top_p)
    pub top_p: Option<f64>,
    /// Random seed for reproducibility (gen_ai.request.seed)
    pub seed: Option<u64>,
}

impl GenAiSpanInfo {
    /// Parse GenAI information from span attributes.
    ///
    /// Returns a `GenAiSpanInfo` with `is_genai = true` if any `gen_ai.*` attributes
    /// are found, otherwise returns a default instance with `is_genai = false`.
    pub fn from_attributes(attrs: &HashMap<String, String>) -> Self {
        let mut info = Self::default();

        // Check for any gen_ai.* attribute to determine if this is a GenAI span
        let has_genai_attrs = attrs.keys().any(|k| k.starts_with("gen_ai."));
        if !has_genai_attrs {
            return info;
        }

        info.is_genai = true;

        // Extract system — prefer gen_ai.provider.name (new), fall back to gen_ai.system (deprecated)
        info.system = attrs
            .get("gen_ai.provider.name")
            .or_else(|| attrs.get("gen_ai.system"))
            .cloned();

        // Extract request model
        info.model = attrs.get("gen_ai.request.model").cloned();

        // Extract response model (may differ from request model due to routing)
        info.response_model = attrs.get("gen_ai.response.model").cloned();

        // Extract operation
        info.operation = attrs.get("gen_ai.operation.name").cloned();

        // Extract token counts — try OTel semconv names first, then the bare
        // names that Claude Code emits (input_tokens, output_tokens, etc.).
        info.input_tokens = attrs
            .get("gen_ai.usage.input_tokens")
            .or_else(|| attrs.get("input_tokens"))
            .and_then(|s| s.parse().ok());

        info.output_tokens = attrs
            .get("gen_ai.usage.output_tokens")
            .or_else(|| attrs.get("output_tokens"))
            .and_then(|s| s.parse().ok());

        // Total tokens: use explicit value if present, otherwise compute from input+output
        info.total_tokens = attrs
            .get("gen_ai.usage.total_tokens")
            .and_then(|s| s.parse().ok())
            .or_else(|| match (info.input_tokens, info.output_tokens) {
                (Some(input), Some(output)) => Some(input + output),
                _ => None,
            });

        // Extract temperature
        info.temperature = attrs
            .get("gen_ai.request.temperature")
            .and_then(|s| s.parse().ok());

        // Extract max_tokens
        info.max_tokens = attrs
            .get("gen_ai.request.max_tokens")
            .and_then(|s| s.parse().ok());

        // Extract finish reasons (may be comma-separated or JSON array)
        if let Some(reasons_str) = attrs.get("gen_ai.response.finish_reasons") {
            info.finish_reasons = parse_finish_reasons(reasons_str);
        }

        info.cache_creation_tokens = first_attribute(attrs, semconv::CACHE_CREATION_TOKEN_KEYS)
            .and_then(|(_, value)| value.parse().ok());
        info.cache_read_tokens = first_attribute(attrs, semconv::CACHE_READ_TOKEN_KEYS)
            .and_then(|(_, value)| value.parse().ok());

        // Extract response ID
        info.response_id = attrs.get("gen_ai.response.id").cloned();

        // Extract tool call fields
        info.tool_name = attrs.get("gen_ai.tool.name").cloned();
        info.tool_call_id = attrs.get("gen_ai.tool.call.id").cloned();
        info.tool_type = attrs.get("gen_ai.tool.type").cloned();

        // Extract sampling parameters
        info.top_p = attrs
            .get("gen_ai.request.top_p")
            .and_then(|s| s.parse().ok());
        info.seed = attrs
            .get("gen_ai.request.seed")
            .and_then(|s| s.parse().ok());

        info
    }

    /// Returns true if this span represents a tool call execution.
    pub fn is_tool_call(&self) -> bool {
        self.operation.as_deref() == Some("execute_tool") || self.tool_name.is_some()
    }

    /// Format token usage as a human-readable string.
    ///
    /// Returns a string like "Input: 1,234 | Output: 567 | Total: 1,801"
    /// or "Total: 1,801" if only total is available.
    pub fn format_token_usage(&self) -> Option<String> {
        match (self.input_tokens, self.output_tokens, self.total_tokens) {
            (Some(input), Some(output), _) => {
                let total = input + output;
                Some(format!(
                    "Input: {} | Output: {} | Total: {}",
                    format_number(input),
                    format_number(output),
                    format_number(total)
                ))
            },
            (None, None, Some(total)) => Some(format!("Total: {}", format_number(total))),
            _ => None,
        }
    }

    /// Format a compact token summary for inline display.
    ///
    /// Returns a string like "(1234→567 tokens)" or "(1801 tokens)".
    pub fn format_token_summary(&self) -> Option<String> {
        match (self.input_tokens, self.output_tokens, self.total_tokens) {
            (Some(input), Some(output), _) => Some(format!("({}→{} tokens)", input, output)),
            (None, None, Some(total)) => Some(format!("({} tokens)", total)),
            _ => None,
        }
    }

    /// Get a display name for the system (e.g., "OpenAI", "Anthropic").
    pub fn system_display_name(&self) -> Option<String> {
        self.system
            .as_deref()
            .map(GenAiSpanInfo::format_system_name)
    }

    /// Format a system/provider identifier as a human-readable display name.
    pub fn format_system_name(s: &str) -> String {
        match s {
            "openai" => "OpenAI".to_string(),
            "anthropic" => "Anthropic".to_string(),
            "azure_openai" => "Azure OpenAI".to_string(),
            "google" => "Google".to_string(),
            "cohere" => "Cohere".to_string(),
            other => {
                let mut chars = other.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            },
        }
    }
}

/// Parse finish reasons from a string.
///
/// Handles comma-separated values and JSON arrays.
fn parse_finish_reasons(s: &str) -> Vec<String> {
    let trimmed = s.trim();

    // Try parsing as JSON array first
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        if let Ok(parsed) = serde_json::from_str::<Vec<String>>(trimmed) {
            return parsed;
        }
    }

    // Fall back to comma-separated, stripping enclosing brackets and
    // quotes per token so a malformed JSON array still yields clean
    // reasons.
    trimmed
        .split(',')
        .map(|s| {
            s.trim()
                .trim_matches(|c| c == '[' || c == ']' || c == '"')
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// Format a number with thousands separators.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::trace::{SpanKind, SpanStatus, StatusCode};

    fn test_span(name: &str, duration_secs: i64, attributes: HashMap<String, String>) -> Span {
        Span {
            trace_id: "trace".to_string(),
            span_id: "span".to_string(),
            parent_span_id: None,
            name: name.to_string(),
            kind: SpanKind::Client,
            start_time: 0,
            end_time: duration_secs * 1_000_000_000,
            attributes,
            events: Vec::new(),
            status: SpanStatus {
                code: StatusCode::Ok,
                message: None,
            },
            resource: None,
        }
    }

    #[test]
    fn test_detect_openai_span() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());
        attrs.insert("gen_ai.request.model".to_string(), "gpt-4".to_string());
        attrs.insert("gen_ai.operation.name".to_string(), "chat".to_string());
        attrs.insert("gen_ai.usage.input_tokens".to_string(), "1234".to_string());
        attrs.insert("gen_ai.usage.output_tokens".to_string(), "567".to_string());

        let info = GenAiSpanInfo::from_attributes(&attrs);

        assert!(info.is_genai);
        assert_eq!(info.system, Some("openai".to_string()));
        assert_eq!(info.model, Some("gpt-4".to_string()));
        assert_eq!(info.operation, Some("chat".to_string()));
        assert_eq!(info.input_tokens, Some(1234));
        assert_eq!(info.output_tokens, Some(567));
        assert_eq!(info.total_tokens, Some(1801));
    }

    #[test]
    fn test_detect_anthropic_span() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "anthropic".to_string());
        attrs.insert(
            "gen_ai.request.model".to_string(),
            "claude-sonnet-4-20250514".to_string(),
        );
        attrs.insert("gen_ai.operation.name".to_string(), "chat".to_string());
        attrs.insert("gen_ai.usage.input_tokens".to_string(), "2000".to_string());
        attrs.insert("gen_ai.usage.output_tokens".to_string(), "500".to_string());
        attrs.insert("gen_ai.request.temperature".to_string(), "0.7".to_string());
        attrs.insert(
            "gen_ai.response.finish_reasons".to_string(),
            "[\"stop\"]".to_string(),
        );

        let info = GenAiSpanInfo::from_attributes(&attrs);

        assert!(info.is_genai);
        assert_eq!(info.system, Some("anthropic".to_string()));
        assert_eq!(info.model, Some("claude-sonnet-4-20250514".to_string()));
        assert_eq!(info.operation, Some("chat".to_string()));
        assert_eq!(info.input_tokens, Some(2000));
        assert_eq!(info.output_tokens, Some(500));
        assert_eq!(info.total_tokens, Some(2500));
        assert_eq!(info.temperature, Some(0.7));
        assert_eq!(info.finish_reasons, vec!["stop".to_string()]);
    }

    #[test]
    fn test_no_genai_attributes() {
        let mut attrs = HashMap::new();
        attrs.insert("http.method".to_string(), "GET".to_string());
        attrs.insert("http.status_code".to_string(), "200".to_string());

        let info = GenAiSpanInfo::from_attributes(&attrs);

        assert!(!info.is_genai);
        assert_eq!(info.system, None);
        assert_eq!(info.model, None);
    }

    #[test]
    fn test_partial_attributes() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());
        // Only system, no other attributes

        let info = GenAiSpanInfo::from_attributes(&attrs);

        assert!(info.is_genai);
        assert_eq!(info.system, Some("openai".to_string()));
        assert_eq!(info.model, None);
        assert_eq!(info.input_tokens, None);
    }

    #[test]
    fn test_token_parsing() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());
        attrs.insert("gen_ai.usage.input_tokens".to_string(), "1000".to_string());
        attrs.insert("gen_ai.usage.output_tokens".to_string(), "500".to_string());

        let info = GenAiSpanInfo::from_attributes(&attrs);

        assert_eq!(info.input_tokens, Some(1000));
        assert_eq!(info.output_tokens, Some(500));
        assert_eq!(info.total_tokens, Some(1500));
    }

    #[test]
    fn test_explicit_total_tokens() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());
        attrs.insert("gen_ai.usage.total_tokens".to_string(), "2000".to_string());

        let info = GenAiSpanInfo::from_attributes(&attrs);

        assert_eq!(info.total_tokens, Some(2000));
        assert_eq!(info.input_tokens, None);
        assert_eq!(info.output_tokens, None);
    }

    #[test]
    fn test_format_token_usage() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());
        attrs.insert("gen_ai.usage.input_tokens".to_string(), "1234".to_string());
        attrs.insert("gen_ai.usage.output_tokens".to_string(), "567".to_string());

        let info = GenAiSpanInfo::from_attributes(&attrs);
        let formatted = info.format_token_usage();

        assert_eq!(
            formatted,
            Some("Input: 1,234 | Output: 567 | Total: 1,801".to_string())
        );
    }

    #[test]
    fn test_format_token_summary() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());
        attrs.insert("gen_ai.usage.input_tokens".to_string(), "1234".to_string());
        attrs.insert("gen_ai.usage.output_tokens".to_string(), "567".to_string());

        let info = GenAiSpanInfo::from_attributes(&attrs);
        let summary = info.format_token_summary();

        assert_eq!(summary, Some("(1234→567 tokens)".to_string()));
    }

    #[test]
    fn test_system_display_name() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());

        let info = GenAiSpanInfo::from_attributes(&attrs);
        assert_eq!(info.system_display_name(), Some("OpenAI".to_string()));

        attrs.insert("gen_ai.system".to_string(), "anthropic".to_string());
        let info = GenAiSpanInfo::from_attributes(&attrs);
        assert_eq!(info.system_display_name(), Some("Anthropic".to_string()));
    }

    #[test]
    fn test_parse_finish_reasons_json() {
        let reasons = parse_finish_reasons("[\"stop\", \"length\"]");
        assert_eq!(reasons, vec!["stop".to_string(), "length".to_string()]);
    }

    #[test]
    fn test_parse_finish_reasons_comma_separated() {
        let reasons = parse_finish_reasons("stop, length");
        assert_eq!(reasons, vec!["stop".to_string(), "length".to_string()]);
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(1234), "1,234");
        assert_eq!(format_number(1234567), "1,234,567");
        assert_eq!(format_number(123), "123");
    }

    #[test]
    fn test_response_id() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());
        attrs.insert(
            "gen_ai.response.id".to_string(),
            "chatcmpl-abc123".to_string(),
        );

        let info = GenAiSpanInfo::from_attributes(&attrs);
        assert_eq!(info.response_id, Some("chatcmpl-abc123".to_string()));
    }

    #[test]
    fn test_extract_cache_token_aliases() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "opencode".to_string());
        attrs.insert(
            "gen_ai.usage.cache_creation_tokens".to_string(),
            "120".to_string(),
        );
        attrs.insert(
            "gen_ai.usage.cache_read_tokens".to_string(),
            "340".to_string(),
        );

        let info = GenAiSpanInfo::from_attributes(&attrs);

        assert_eq!(info.cache_creation_tokens, Some(120));
        assert_eq!(info.cache_read_tokens, Some(340));
    }

    #[test]
    fn test_tool_call_fields() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());
        attrs.insert(
            "gen_ai.operation.name".to_string(),
            "execute_tool".to_string(),
        );
        attrs.insert("gen_ai.tool.name".to_string(), "get_weather".to_string());
        attrs.insert("gen_ai.tool.call.id".to_string(), "call_xyz789".to_string());
        attrs.insert("gen_ai.tool.type".to_string(), "function".to_string());

        let info = GenAiSpanInfo::from_attributes(&attrs);
        assert_eq!(info.tool_name, Some("get_weather".to_string()));
        assert_eq!(info.tool_call_id, Some("call_xyz789".to_string()));
        assert_eq!(info.tool_type, Some("function".to_string()));
    }

    #[test]
    fn test_is_tool_call_via_operation() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());
        attrs.insert(
            "gen_ai.operation.name".to_string(),
            "execute_tool".to_string(),
        );

        let info = GenAiSpanInfo::from_attributes(&attrs);
        assert!(info.is_tool_call());
    }

    #[test]
    fn test_is_tool_call_via_tool_name() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());
        attrs.insert("gen_ai.tool.name".to_string(), "search_docs".to_string());

        let info = GenAiSpanInfo::from_attributes(&attrs);
        assert!(info.is_tool_call());
    }

    #[test]
    fn test_is_not_tool_call() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());
        attrs.insert("gen_ai.operation.name".to_string(), "chat".to_string());

        let info = GenAiSpanInfo::from_attributes(&attrs);
        assert!(!info.is_tool_call());
    }

    #[test]
    fn test_top_p_and_seed() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());
        attrs.insert("gen_ai.request.top_p".to_string(), "0.9".to_string());
        attrs.insert("gen_ai.request.seed".to_string(), "42".to_string());

        let info = GenAiSpanInfo::from_attributes(&attrs);
        assert_eq!(info.top_p, Some(0.9));
        assert_eq!(info.seed, Some(42));
    }

    #[test]
    fn test_extract_ttft_secs_otel_spec() {
        let mut attrs = HashMap::new();
        attrs.insert(
            "gen_ai.server.time_to_first_token".to_string(),
            "1.2".to_string(),
        );
        attrs.insert("ttft_ms".to_string(), "9999".to_string());
        let v = extract_ttft_secs(&attrs).unwrap();
        assert!(
            (v - 1.2).abs() < 0.001,
            "OTel spec attribute should be used as-is (seconds)"
        );
    }

    #[test]
    fn test_extract_ttft_secs_claude_code_ms() {
        let mut attrs = HashMap::new();
        attrs.insert("ttft_ms".to_string(), "1200".to_string());
        let v = extract_ttft_secs(&attrs).unwrap();
        assert!(
            (v - 1.2).abs() < 0.001,
            "ttft_ms must be divided by 1000 to yield seconds"
        );
    }

    #[test]
    fn test_extract_ttft_secs_missing() {
        let attrs = HashMap::new();
        assert!(extract_ttft_secs(&attrs).is_none());
    }

    #[test]
    fn test_extract_ttft_secs_does_not_fall_back_from_invalid_preferred_value() {
        let mut attrs = HashMap::new();
        attrs.insert(
            "gen_ai.server.time_to_first_token".to_string(),
            "not-a-number".to_string(),
        );
        attrs.insert("ttft_ms".to_string(), "1200".to_string());
        assert!(extract_ttft_secs(&attrs).is_none());
    }

    #[test]
    fn test_classify_ttft_value() {
        assert_eq!(classify_ttft_value(None, 1.0), TtftValueQuality::Absent);
        assert_eq!(
            classify_ttft_value(Some(0.25), 1.0),
            TtftValueQuality::Valid
        );
        assert_eq!(
            classify_ttft_value(Some(-0.1), 1.0),
            TtftValueQuality::Invalid
        );
        assert_eq!(
            classify_ttft_value(Some(1.1), 1.0),
            TtftValueQuality::Invalid
        );
        assert_eq!(
            classify_ttft_value(Some(f64::NAN), 1.0),
            TtftValueQuality::Invalid
        );
    }

    #[test]
    fn test_classify_span_capabilities_keeps_codex_timing_only() {
        let mut attrs = HashMap::new();
        attrs.insert("model".to_string(), "codex-test".to_string());
        attrs.insert("otel.scope.name".to_string(), "codex_cli_rs".to_string());
        let capabilities = classify_span_capabilities(&test_span("run_sampling_request", 4, attrs));
        assert_eq!(capabilities.emitter, GenAiEmitter::Codex);
        assert_eq!(capabilities.role, GenAiSpanRole::RequestTiming);
        assert_eq!(
            capabilities.output_tokens.derivation,
            MetricDerivation::Unavailable
        );
        assert_eq!(
            capabilities.output_tokens.rejection_reason,
            Some(MetricRejectionReason::UnverifiedUsageSignature)
        );
    }

    #[test]
    fn test_classify_span_capabilities_recognizes_opencode_flat_tokens() {
        let mut attrs = HashMap::new();
        attrs.insert("openinference.span.kind".to_string(), "LLM".to_string());
        attrs.insert("llm.system".to_string(), "opencode-go".to_string());
        attrs.insert(
            "llm.model_name".to_string(),
            "opencode-test-model".to_string(),
        );
        attrs.insert("llm.usage.prompt_tokens".to_string(), "10".to_string());
        attrs.insert("llm.usage.completion_tokens".to_string(), "4".to_string());
        let capabilities = classify_span_capabilities(&test_span("opencode.llm", 2, attrs));
        assert_eq!(capabilities.emitter, GenAiEmitter::OpenCode);
        assert_eq!(capabilities.input_tokens.value, Some(10));
        assert_eq!(capabilities.output_tokens.value, Some(4));
        assert_eq!(
            capabilities.output_tokens.source_attribute,
            Some("llm.usage.completion_tokens")
        );
    }

    #[test]
    fn test_classify_span_capabilities_accepts_every_token_alias_and_zero_tokens() {
        for keys in [
            semconv::INPUT_TOKEN_KEYS,
            semconv::OUTPUT_TOKEN_KEYS,
            semconv::CACHE_CREATION_TOKEN_KEYS,
            semconv::CACHE_READ_TOKEN_KEYS,
        ] {
            for key in keys {
                let mut attrs = HashMap::new();
                attrs.insert("model".to_string(), "claude-test".to_string());
                attrs.insert((*key).to_string(), "0".to_string());
                let capabilities =
                    classify_span_capabilities(&test_span("claude_code.llm_request", 2, attrs));
                let evidence = if semconv::INPUT_TOKEN_KEYS.contains(key) {
                    &capabilities.input_tokens
                } else if semconv::OUTPUT_TOKEN_KEYS.contains(key) {
                    &capabilities.output_tokens
                } else if semconv::CACHE_CREATION_TOKEN_KEYS.contains(key) {
                    &capabilities.cache_creation_tokens
                } else {
                    &capabilities.cache_read_tokens
                };
                assert_eq!(evidence.observation, MetricObservation::Valid);
                assert_eq!(evidence.value, Some(0));
                assert_eq!(evidence.source_attribute, Some(*key));
            }
        }
    }

    #[test]
    fn test_classify_span_capabilities_marks_unverified_span_unavailable() {
        let mut attrs = HashMap::new();
        attrs.insert("llm.usage.prompt_tokens".to_string(), "10".to_string());
        let capabilities = classify_span_capabilities(&test_span("transport", 2, attrs));
        assert_eq!(capabilities.emitter, GenAiEmitter::Unknown);
        assert_eq!(capabilities.role, GenAiSpanRole::Other);
        assert_eq!(
            capabilities.input_tokens.derivation,
            MetricDerivation::Unavailable
        );
        assert_eq!(
            capabilities.input_tokens.rejection_reason,
            Some(MetricRejectionReason::NotARequestSpan)
        );
    }

    #[test]
    fn test_classify_span_capabilities_keeps_claude_adapter_with_standard_attributes() {
        let mut attrs = HashMap::new();
        attrs.insert("model".to_string(), "claude-test".to_string());
        attrs.insert("gen_ai.provider.name".to_string(), "anthropic".to_string());
        attrs.insert("output_tokens".to_string(), "4".to_string());
        let capabilities =
            classify_span_capabilities(&test_span("claude_code.llm_request", 2, attrs));
        assert_eq!(capabilities.emitter, GenAiEmitter::ClaudeCode);
        assert_eq!(capabilities.output_tokens.value, Some(4));
    }

    // --- Edge cases from issue #9 ---------------------------------------

    #[test]
    fn test_genai_non_numeric_token_count_is_none() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());
        attrs.insert("gen_ai.usage.input_tokens".to_string(), "abc".to_string());

        let info = GenAiSpanInfo::from_attributes(&attrs);

        // Documented behaviour: unparseable counts are dropped, not
        // clamped or panicked on.
        assert_eq!(info.input_tokens, None);
        assert_eq!(info.total_tokens, None);
    }

    #[test]
    fn test_genai_zero_token_count_is_some_zero() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());
        attrs.insert("gen_ai.usage.input_tokens".to_string(), "0".to_string());
        attrs.insert("gen_ai.usage.output_tokens".to_string(), "0".to_string());

        let info = GenAiSpanInfo::from_attributes(&attrs);

        assert_eq!(info.input_tokens, Some(0));
        assert_eq!(info.output_tokens, Some(0));
        assert_eq!(info.total_tokens, Some(0));
    }

    #[test]
    fn test_genai_negative_token_count_is_none() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());
        attrs.insert("gen_ai.usage.input_tokens".to_string(), "-5".to_string());

        let info = GenAiSpanInfo::from_attributes(&attrs);

        // Documented behaviour: u64 parsing rejects negatives, so the
        // value is dropped (None) rather than stored.
        assert_eq!(info.input_tokens, None);
    }

    #[test]
    fn test_genai_empty_finish_reasons_json_array() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());
        attrs.insert(
            "gen_ai.response.finish_reasons".to_string(),
            "[]".to_string(),
        );

        let info = GenAiSpanInfo::from_attributes(&attrs);

        assert!(info.is_genai);
        assert_eq!(info.finish_reasons, Vec::<String>::new());
    }

    #[test]
    fn test_genai_malformed_finish_reasons_falls_back_to_comma_split() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());
        // Unbalanced JSON array: the JSON parse fails and the comma
        // fallback must still produce clean reasons.
        attrs.insert(
            "gen_ai.response.finish_reasons".to_string(),
            "[\"stop\", \"length\"".to_string(),
        );

        let info = GenAiSpanInfo::from_attributes(&attrs);

        assert_eq!(
            info.finish_reasons,
            vec!["stop".to_string(), "length".to_string()]
        );
    }

    #[test]
    fn test_genai_uppercase_system_uses_generic_display() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "OPENAI".to_string());

        let info = GenAiSpanInfo::from_attributes(&attrs);

        // Documented behaviour: system names are matched case-sensitively
        // against the known list; unknown spellings fall through to the
        // capitalised generic (here: unchanged "OPENAI").
        assert_eq!(info.system.as_deref(), Some("OPENAI"));
        assert_eq!(info.system_display_name().as_deref(), Some("OPENAI"));
    }

    #[test]
    fn test_genai_unknown_system_capitalised_generic() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "weirdco".to_string());

        let info = GenAiSpanInfo::from_attributes(&attrs);

        assert_eq!(info.system_display_name().as_deref(), Some("Weirdco"));
    }

    #[test]
    fn test_genai_attributes_without_model() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());
        attrs.insert("gen_ai.operation.name".to_string(), "chat".to_string());

        let info = GenAiSpanInfo::from_attributes(&attrs);

        // The span is recognised as GenAI but carries no model name.
        assert!(info.is_genai);
        assert_eq!(info.model, None);
        assert_eq!(info.operation.as_deref(), Some("chat"));
    }
}
