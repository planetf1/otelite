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
/// This only inspects the span itself; cross-span attribution is a separate
/// step (`correlate_codex_usage`), which is where Codex token counters on
/// usage spans are joined to request spans under a verified one-to-one rule.
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

/// Attribute names a verified emitter signature would still require for an
/// LLM-ish span that matched none (#149). Feeds the capability report's
/// diagnostic block; returns **names only** — never attribute values, span
/// or trace identifiers, or service names (the #120 privacy invariant).
///
/// Rules, per emitter shape:
/// - `claude_code.llm_request*` spans need a model attribute.
/// - Codex request spans need a model attribute and the verified Codex
///   scope.
/// - anything else reports what the most permissive verified signature
///   (`standard-otel-request-v1`) still asks for: a model attribute, a
///   system attribute, and a standard usage/TTFT attribute.
pub fn unidentified_required_attributes(span: &Span) -> Vec<String> {
    let attrs = &span.attributes;
    let has_model = first_attribute(attrs, semconv::MODEL_KEYS).is_some();
    let mut missing: Vec<String> = Vec::new();
    if span.name.starts_with("claude_code.llm_request") {
        if !has_model {
            missing.push(semconv::MODEL_KEYS[0].to_string());
        }
        return missing;
    }
    if span.name == semconv::CODEX_LLM_REQUEST_SPAN_NAME {
        if !has_model {
            missing.push(semconv::MODEL_KEYS[0].to_string());
        }
        let has_verified_scope = attrs
            .get("otel.scope.name")
            .is_some_and(|scope| scope == semconv::CODEX_OTEL_SCOPE_NAME);
        if !has_verified_scope {
            missing.push("otel.scope.name".to_string());
        }
        return missing;
    }
    if !has_model {
        missing.push(semconv::MODEL_KEYS[0].to_string());
    }
    if first_attribute(attrs, semconv::SYSTEM_KEYS).is_none() {
        missing.push(semconv::SYSTEM_KEYS[0].to_string());
    }
    let has_standard_metric = first_attribute(attrs, semconv::INPUT_TOKEN_KEYS).is_some()
        || first_attribute(attrs, semconv::OUTPUT_TOKEN_KEYS).is_some()
        || first_attribute(attrs, semconv::CACHE_CREATION_TOKEN_KEYS).is_some()
        || first_attribute(attrs, semconv::CACHE_READ_TOKEN_KEYS).is_some()
        || raw_ttft(attrs).is_some();
    if !has_standard_metric {
        missing.push(semconv::INPUT_TOKEN_KEYS[0].to_string());
    }
    missing.sort();
    missing
}

/// Extract TTFT from span attributes, normalising to **seconds**.
///
/// Attribute priority:
/// - `gen_ai.server.time_to_first_token` — OTel GenAI spec, already in seconds
/// - `llm.time_to_first_token` — non-standard, assumed seconds
/// - `ttft_ms` — Claude Code custom attribute, in **milliseconds**; divided by 1000
pub fn extract_ttft_secs(attrs: &HashMap<String, String>) -> Option<f64> {
    normalise_span_ttft_secs(attrs).and_then(Result::ok)
}

/// Canonical TTFT normaliser for span attributes: picks the first TTFT
/// attribute by the priority order above, normalises its unit to seconds,
/// and keeps the rejection reason. Returns `None` when no TTFT attribute is
/// present, `Some(Ok(secs))` for a finite non-negative value, and
/// `Some(Err(reason))` for a present but unusable value.
///
/// Every consumer (capability report, latency stats, context-size
/// percentiles) must use this together with `classify_ttft_value` instead
/// of a local copy — that is what keeps unit handling, rejection and
/// quality classification identical across API, CLI, TUI and web.
pub fn normalise_span_ttft_secs(
    attrs: &HashMap<String, String>,
) -> Option<Result<f64, MetricRejectionReason>> {
    let (_, raw, unit) = raw_ttft(attrs)?;
    Some(normalise_ttft_secs(raw, unit))
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
    if !value.is_finite() || value.is_sign_negative() {
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

/// Adapter rule name reported in correlation provenance for the Codex
/// one-to-one usage join.
pub const CODEX_CORRELATION_RULE: &str = "codex-one-to-one-v1";

/// Why a candidate usage span was excluded from request-level attribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrelationRejection {
    /// The enclosing request span ended in an error status.
    IncompleteRequest,
    /// The candidate declares a model that conflicts with the request span.
    ConflictingModel,
}

/// Outcome of the `codex-one-to-one-v1` join for one request span.
///
/// Cardinality is decided on the structurally related candidate set (see
/// `correlate_codex_usage`): anything other than exactly one candidate is a
/// rejected attribution, never a guess.
#[derive(Debug, Clone, PartialEq)]
pub enum CorrelationOutcome {
    /// No usage-bearing candidate in the request's structural neighbourhood.
    Unmatched,
    /// Exactly one candidate, validated; its counters feed the request.
    Matched {
        input_tokens: TokenMetricEvidence,
        output_tokens: TokenMetricEvidence,
        cache_creation_tokens: TokenMetricEvidence,
        cache_read_tokens: TokenMetricEvidence,
    },
    /// The single candidate failed a join invariant.
    Rejected(CorrelationRejection),
    /// Two or more candidates (retries, concurrent sampling, reused
    /// identifiers, turn-level counters): not one-to-one, so no request-level
    /// attribution is made. Carries the candidate count.
    Ambiguous(usize),
}

/// Whether a span matches the Codex request-span signature
/// (`run_sampling_request` under the `codex_cli_rs` scope carrying a model).
///
/// This is the structural half of the `codex-one-to-one-v1` join; it must
/// stay in lockstep with the Codex candidate in `classify_span_capabilities`.
pub fn is_codex_request_span(span: &Span) -> bool {
    span.name == semconv::CODEX_LLM_REQUEST_SPAN_NAME
        && span
            .attributes
            .get("otel.scope.name")
            .is_some_and(|scope| scope == semconv::CODEX_OTEL_SCOPE_NAME)
        && first_attribute(&span.attributes, semconv::MODEL_KEYS).is_some()
}

/// Whether a span is a Codex usage candidate: the verified usage-span
/// signature (`handle_responses` under the `codex_cli_rs` scope) carrying at
/// least one token counter attribute.
pub fn is_codex_usage_candidate(span: &Span) -> bool {
    span.name == semconv::CODEX_HANDLE_RESPONSES_SPAN_NAME
        && span
            .attributes
            .get("otel.scope.name")
            .is_some_and(|scope| scope == semconv::CODEX_OTEL_SCOPE_NAME)
        && semconv::INPUT_TOKEN_KEYS
            .iter()
            .chain(semconv::OUTPUT_TOKEN_KEYS)
            .chain(semconv::CACHE_CREATION_TOKEN_KEYS)
            .chain(semconv::CACHE_READ_TOKEN_KEYS)
            .any(|key| span.attributes.contains_key(*key))
}

/// Token counter evidence taken from a correlated (non-request) usage span.
///
/// Absence of a counter on the candidate is a plain absence, not a
/// rejection: the candidate signature is already verified, so a missing
/// counter just means that metric is not provided for this request.
pub fn correlated_token_evidence(
    attrs: &HashMap<String, String>,
    keys: &[&'static str],
) -> TokenMetricEvidence {
    let mut evidence = token_evidence(
        attrs,
        keys,
        true,
        MetricRejectionReason::MissingNativeAttribute,
    );
    match evidence.observation {
        MetricObservation::Absent => {
            evidence.rejection_reason = None;
        },
        _ => {
            evidence.derivation = MetricDerivation::Correlated;
            evidence.rejection_reason = (evidence.observation == MetricObservation::Invalid)
                .then_some(MetricRejectionReason::InvalidInteger);
        },
    }
    evidence
}

/// Apply the `codex-one-to-one-v1` join rule to one request span.
///
/// `candidates` must already be restricted to spans that pass
/// `is_codex_usage_candidate` and are descendants of the request span within
/// the same trace — the structural half of the rule, which the caller
/// establishes from the stored span tree. This function enforces the rest:
///
/// - the request span must not have ended in an error status;
/// - a model attribute on the candidate must match the request's model;
/// - exactly one candidate may exist (retries, concurrent sampling, reused
///   identifiers and turn-level counters all produce two or more and are
///   rejected as `Ambiguous`).
pub fn correlate_codex_usage(
    request_completed: bool,
    request_model: Option<&str>,
    candidates: &[&Span],
) -> CorrelationOutcome {
    match candidates.len() {
        0 => CorrelationOutcome::Unmatched,
        1 => {
            if !request_completed {
                return CorrelationOutcome::Rejected(CorrelationRejection::IncompleteRequest);
            }
            // Any request-model attribute on the candidate must agree with
            // the request span; response-model attributes are ignored.
            let candidate_models: Vec<&str> = semconv::REQUEST_MODEL_KEYS
                .iter()
                .filter_map(|key| {
                    candidates[0]
                        .attributes
                        .get(*key)
                        .map(|value| value.as_str())
                })
                .collect();
            if candidate_models
                .iter()
                .any(|value| request_model != Some(value))
            {
                return CorrelationOutcome::Rejected(CorrelationRejection::ConflictingModel);
            }
            CorrelationOutcome::Matched {
                input_tokens: correlated_token_evidence(
                    &candidates[0].attributes,
                    semconv::INPUT_TOKEN_KEYS,
                ),
                output_tokens: correlated_token_evidence(
                    &candidates[0].attributes,
                    semconv::OUTPUT_TOKEN_KEYS,
                ),
                cache_creation_tokens: correlated_token_evidence(
                    &candidates[0].attributes,
                    semconv::CACHE_CREATION_TOKEN_KEYS,
                ),
                cache_read_tokens: correlated_token_evidence(
                    &candidates[0].attributes,
                    semconv::CACHE_READ_TOKEN_KEYS,
                ),
            }
        },
        n => CorrelationOutcome::Ambiguous(n),
    }
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
    /// Reasoning / thinking tokens (opencode extended thinking, pi harness)
    pub reasoning_tokens: Option<u64>,
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
        info.reasoning_tokens = first_attribute(attrs, semconv::REASONING_TOKEN_KEYS)
            .and_then(|(_, value)| value.parse::<u64>().ok())
            .filter(|&v| v > 0);

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
                let mut s = format!(
                    "Input: {} | Output: {} | Total: {}",
                    format_number(input),
                    format_number(output),
                    format_number(total)
                );
                if let Some(r) = self.reasoning_tokens {
                    s.push_str(&format!(" | Reasoning: {}", format_number(r)));
                }
                Some(s)
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
    fn test_unidentified_required_attributes_names_only() {
        // Generic LLM-ish span: model present, no system, no usage/TTFT.
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.request.model".to_string(), "mystery".to_string());
        let required = unidentified_required_attributes(&test_span("llm.call", 1, attrs));
        assert_eq!(
            required,
            vec!["gen_ai.provider.name", "gen_ai.usage.input_tokens"],
            "names of the missing standard-signature attributes, sorted"
        );

        // Generic span missing everything a verified signature accepts.
        let attrs = HashMap::new();
        let required = unidentified_required_attributes(&test_span("llm.call", 1, attrs));
        assert_eq!(
            required,
            vec![
                "gen_ai.provider.name",
                "gen_ai.request.model",
                "gen_ai.usage.input_tokens"
            ]
        );

        // A TTFT attribute counts as a standard metric: only the system
        // attribute is still missing.
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());
        attrs.insert("gen_ai.request.model".to_string(), "mystery".to_string());
        attrs.insert(
            "gen_ai.server.time_to_first_token".to_string(),
            "0.5".to_string(),
        );
        let required = unidentified_required_attributes(&test_span("llm.call", 1, attrs));
        assert_eq!(
            required,
            Vec::<String>::new(),
            "fully verified — nothing required"
        );

        // Codex-named span without the verified scope.
        let mut attrs = HashMap::new();
        attrs.insert("model".to_string(), "mystery".to_string());
        let required =
            unidentified_required_attributes(&test_span("run_sampling_request", 1, attrs));
        assert_eq!(required, vec!["otel.scope.name"]);

        // Claude-named span without a model.
        let attrs = HashMap::new();
        let required =
            unidentified_required_attributes(&test_span("claude_code.llm_request", 1, attrs));
        assert_eq!(required, vec!["gen_ai.request.model"]);
    }

    #[test]
    fn test_normalise_span_ttft_secs_keeps_rejection_reasons() {
        let mut attrs = HashMap::new();
        attrs.insert(
            "gen_ai.server.time_to_first_token".to_string(),
            "0.5".to_string(),
        );
        assert_eq!(
            normalise_span_ttft_secs(&attrs),
            Some(Ok(0.5)),
            "finite non-negative value normalises to seconds"
        );

        attrs.insert(
            "gen_ai.server.time_to_first_token".to_string(),
            "-1".to_string(),
        );
        assert_eq!(
            normalise_span_ttft_secs(&attrs),
            Some(Err(MetricRejectionReason::InvalidSeconds)),
            "negative values are rejected by the normaliser itself"
        );
        assert_eq!(
            extract_ttft_secs(&attrs),
            None,
            "extract maps rejections to absence-of-value"
        );

        attrs.insert(
            "gen_ai.server.time_to_first_token".to_string(),
            "inf".to_string(),
        );
        assert_eq!(
            normalise_span_ttft_secs(&attrs),
            Some(Err(MetricRejectionReason::InvalidSeconds))
        );

        attrs.insert(
            "gen_ai.server.time_to_first_token".to_string(),
            "1.5".to_string(),
        );
        attrs.insert("ttft_ms".to_string(), "300".to_string());
        assert_eq!(
            normalise_span_ttft_secs(&attrs),
            Some(Ok(1.5)),
            "spec key wins over the custom millisecond key"
        );
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

    fn codex_usage_span(attributes: HashMap<String, String>) -> Span {
        let mut span = test_span(semconv::CODEX_HANDLE_RESPONSES_SPAN_NAME, 0, attributes);
        span.attributes.insert(
            "otel.scope.name".to_string(),
            semconv::CODEX_OTEL_SCOPE_NAME.to_string(),
        );
        span
    }

    fn codex_usage_attrs(input: Option<&str>, output: Option<&str>) -> HashMap<String, String> {
        let mut attrs = HashMap::new();
        if let Some(input) = input {
            attrs.insert("gen_ai.usage.input_tokens".to_string(), input.to_string());
        }
        if let Some(output) = output {
            attrs.insert("gen_ai.usage.output_tokens".to_string(), output.to_string());
        }
        attrs
    }

    #[test]
    fn codex_usage_candidate_requires_scope_and_counter() {
        // Correct signature: scope + counter.
        assert!(is_codex_usage_candidate(&codex_usage_span(
            codex_usage_attrs(Some("10"), Some("5"),)
        )));
        // Wrong span name.
        let mut wrong_name = codex_usage_span(codex_usage_attrs(Some("10"), Some("5")));
        wrong_name.name = "receiving_stream".to_string();
        assert!(!is_codex_usage_candidate(&wrong_name));
        // Right name, wrong scope (product names alone do not classify).
        let mut wrong_scope = codex_usage_span(codex_usage_attrs(Some("10"), Some("5")));
        wrong_scope
            .attributes
            .insert("otel.scope.name".to_string(), "other_tool".to_string());
        assert!(!is_codex_usage_candidate(&wrong_scope));
        // Right signature but no counter attributes.
        assert!(!is_codex_usage_candidate(&codex_usage_span(HashMap::from(
            [("thread.id".to_string(), "1".to_string())]
        ))));
    }

    #[test]
    fn correlate_codex_usage_matches_one_verified_candidate() {
        let mut attrs = codex_usage_attrs(Some("760018"), Some("738"));
        attrs.insert(
            "gen_ai.usage.cache_read.input_tokens".to_string(),
            "759816".to_string(),
        );
        let candidate = codex_usage_span(attrs);
        let outcome = correlate_codex_usage(true, Some("gpt-5.6-terra"), &[&candidate]);
        match outcome {
            CorrelationOutcome::Matched {
                input_tokens,
                output_tokens,
                cache_creation_tokens,
                cache_read_tokens,
            } => {
                assert_eq!(input_tokens.value, Some(760018));
                assert_eq!(input_tokens.derivation, MetricDerivation::Correlated);
                assert_eq!(output_tokens.value, Some(738));
                assert_eq!(
                    output_tokens.source_attribute,
                    Some("gen_ai.usage.output_tokens")
                );
                assert_eq!(cache_creation_tokens.observation, MetricObservation::Absent);
                assert_eq!(
                    cache_creation_tokens.rejection_reason, None,
                    "a missing counter on a verified candidate is an absence, not a rejection"
                );
                assert_eq!(cache_read_tokens.value, Some(759816));
            },
            other => panic!("expected Matched, got {other:?}"),
        }
    }

    #[test]
    fn correlate_codex_usage_rejects_incomplete_request() {
        let candidate = codex_usage_span(codex_usage_attrs(Some("10"), Some("5")));
        let outcome = correlate_codex_usage(false, Some("m"), &[&candidate]);
        assert_eq!(
            outcome,
            CorrelationOutcome::Rejected(CorrelationRejection::IncompleteRequest)
        );
    }

    #[test]
    fn correlate_codex_usage_rejects_conflicting_model() {
        let mut attrs = codex_usage_attrs(Some("10"), Some("5"));
        attrs.insert("model".to_string(), "other-model".to_string());
        let candidate = codex_usage_span(attrs);
        let outcome = correlate_codex_usage(true, Some("requested-model"), &[&candidate]);
        assert_eq!(
            outcome,
            CorrelationOutcome::Rejected(CorrelationRejection::ConflictingModel)
        );
        // A matching model attribute does not conflict.
        let mut attrs = codex_usage_attrs(Some("10"), Some("5"));
        attrs.insert("model".to_string(), "requested-model".to_string());
        let candidate = codex_usage_span(attrs);
        assert!(matches!(
            correlate_codex_usage(true, Some("requested-model"), &[&candidate]),
            CorrelationOutcome::Matched { .. }
        ));
        // A response-model-only attribute is ignored by the join.
        let mut attrs = codex_usage_attrs(Some("10"), Some("5"));
        attrs.insert(
            "gen_ai.response.model".to_string(),
            "routed-model".to_string(),
        );
        let candidate = codex_usage_span(attrs);
        assert!(matches!(
            correlate_codex_usage(true, Some("requested-model"), &[&candidate]),
            CorrelationOutcome::Matched { .. }
        ));
    }

    #[test]
    fn correlate_codex_usage_rejects_ambiguous_candidates() {
        let a = codex_usage_span(codex_usage_attrs(Some("10"), Some("5")));
        let b = codex_usage_span(codex_usage_attrs(Some("11"), Some("6")));
        assert_eq!(
            correlate_codex_usage(true, Some("m"), &[&a, &b]),
            CorrelationOutcome::Ambiguous(2)
        );
        assert_eq!(
            correlate_codex_usage(true, Some("m"), &[]),
            CorrelationOutcome::Unmatched
        );
    }

    #[test]
    fn correlated_token_evidence_marks_invalid_counters() {
        let attrs = codex_usage_attrs(Some("not-a-number"), Some("5"));
        let evidence = correlated_token_evidence(&attrs, semconv::INPUT_TOKEN_KEYS);
        assert_eq!(evidence.observation, MetricObservation::Invalid);
        assert_eq!(evidence.derivation, MetricDerivation::Correlated);
        assert_eq!(
            evidence.rejection_reason,
            Some(MetricRejectionReason::InvalidInteger)
        );
        let output = correlated_token_evidence(&attrs, semconv::OUTPUT_TOKEN_KEYS);
        assert_eq!(output.observation, MetricObservation::Valid);
        assert_eq!(output.value, Some(5));
        assert_eq!(output.rejection_reason, None);
    }

    #[test]
    fn test_reasoning_tokens_parsed() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "anthropic".to_string());
        attrs.insert("gen_ai.request.model".to_string(), "deepseek-v4-flash".to_string());
        attrs.insert("gen_ai.usage.input_tokens".to_string(), "100".to_string());
        attrs.insert("gen_ai.usage.output_tokens".to_string(), "80".to_string());
        attrs.insert("gen_ai.usage.reasoning_tokens".to_string(), "50".to_string());

        let info = GenAiSpanInfo::from_attributes(&attrs);
        assert_eq!(info.reasoning_tokens, Some(50));
        let fmt = info.format_token_usage().unwrap();
        assert!(fmt.contains("Reasoning: 50"), "expected reasoning in: {fmt}");
    }

    #[test]
    fn test_reasoning_tokens_zero_filtered() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "anthropic".to_string());
        attrs.insert("gen_ai.usage.reasoning_tokens".to_string(), "0".to_string());
        attrs.insert("gen_ai.usage.output_tokens".to_string(), "20".to_string());

        let info = GenAiSpanInfo::from_attributes(&attrs);
        assert_eq!(info.reasoning_tokens, None);
    }

    #[test]
    fn test_reasoning_tokens_absent() {
        let mut attrs = HashMap::new();
        attrs.insert("gen_ai.system".to_string(), "openai".to_string());
        attrs.insert("gen_ai.usage.input_tokens".to_string(), "10".to_string());
        attrs.insert("gen_ai.usage.output_tokens".to_string(), "5".to_string());

        let info = GenAiSpanInfo::from_attributes(&attrs);
        assert_eq!(info.reasoning_tokens, None);
        let fmt = info.format_token_usage().unwrap();
        assert!(!fmt.contains("Reasoning"), "no reasoning field expected: {fmt}");
    }
}
