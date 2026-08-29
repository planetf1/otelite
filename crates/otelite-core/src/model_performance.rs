//! Deterministic model-performance assessment (#121/#152).
//!
//! The #151 comparison query produces raw per-window metric values and
//! deltas. This module is the single assessment layer: it classifies each
//! monitored metric against the preceding and rolling baselines, derives a
//! confidence from the eligible sample count, and applies the
//! telemetry-quality gate for TTFT. Surfaces render the assessment
//! verbatim — they must never reclassify or recompute.
//!
//! The classification is pure and deterministic: same response in, same
//! assessment out, for every surface (CLI, TUI, web, API).

use crate::api::{
    ModelPerformanceCounts, ModelPerformanceDelta, ModelPerformanceErrorValue,
    ModelPerformanceIdentity, ModelPerformanceMetric, ModelPerformanceSample,
    MODEL_PERFORMANCE_MATERIAL_ERROR_RATE_POINTS, MODEL_PERFORMANCE_MATERIAL_RELATIVE_CHANGE,
    MODEL_PERFORMANCE_MIN_ELIGIBLE_SAMPLES,
};

/// Direction in which a metric's value worsening manifests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorseDirection {
    /// Higher values are worse (duration, TTFT, error rate).
    Up,
    /// Lower values are worse (throughput).
    Down,
}

/// A monitored, classifiable metric. Token metrics are workload *signals*
/// (used by the correlation logic), never regression subjects: they have no
/// good/bad direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum PerformanceMetricKind {
    Duration,
    Throughput,
    Ttft,
    ErrorRate,
}

impl std::fmt::Display for PerformanceMetricKind {
    /// The serde (snake_case) name — the metric vocabulary every surface
    /// renders, so a panel never invents a parallel label.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Duration => "duration",
            Self::Throughput => "throughput",
            Self::Ttft => "ttft",
            Self::ErrorRate => "error_rate",
        };
        f.write_str(s)
    }
}

/// Assessment classes. `WorkloadShiftCorrelated` and `ErrorAssociated`
/// qualify a material change with its observed co-movement (correlation,
/// never causation); `MixedEvidence` means the two baselines disagree and
/// both are reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum PerformanceChangeClass {
    TypicalRegression,
    TailRegression,
    WorkloadShiftCorrelated,
    ErrorAssociated,
    NoMaterialChange,
    InsufficientTelemetry,
    MixedEvidence,
}

impl std::fmt::Display for PerformanceChangeClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            PerformanceChangeClass::TypicalRegression => "typical_regression",
            PerformanceChangeClass::TailRegression => "tail_regression",
            PerformanceChangeClass::WorkloadShiftCorrelated => "workload_shift_correlated",
            PerformanceChangeClass::ErrorAssociated => "error_associated",
            PerformanceChangeClass::NoMaterialChange => "no_material_change",
            PerformanceChangeClass::InsufficientTelemetry => "insufficient_telemetry",
            PerformanceChangeClass::MixedEvidence => "mixed_evidence",
        };
        f.write_str(s)
    }
}

impl PerformanceChangeClass {
    /// Severity used for the per-identity headline: the most severe metric
    /// assessment wins. Insufficient telemetry ranks last — it is the
    /// absence of an assessment, not a finding.
    fn severity(self) -> u8 {
        match self {
            PerformanceChangeClass::TypicalRegression => 6,
            PerformanceChangeClass::TailRegression => 5,
            PerformanceChangeClass::WorkloadShiftCorrelated => 4,
            PerformanceChangeClass::ErrorAssociated => 3,
            PerformanceChangeClass::MixedEvidence => 2,
            PerformanceChangeClass::NoMaterialChange => 1,
            PerformanceChangeClass::InsufficientTelemetry => 0,
        }
    }
}

/// Confidence is a first-class field derived from the eligible sample count
/// of the current window; insufficient evidence suppresses causal wording
/// everywhere (the class is `InsufficientTelemetry`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum PerformanceConfidence {
    Insufficient,
    Low,
    Medium,
    High,
}

/// TTFT trust gate (#111 quality + #120 derivation): conclusions are only
/// permitted for reliable native or structurally correlated values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum TtftTrust {
    /// Reliable native or structurally correlated TTFT.
    Reliable,
    /// Absent, sparse, invalid, or degenerate TTFT: first-response and
    /// decode-rate attribution is prevented.
    Unreliable,
}

impl std::fmt::Display for PerformanceConfidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            PerformanceConfidence::Insufficient => "insufficient",
            PerformanceConfidence::Low => "low",
            PerformanceConfidence::Medium => "medium",
            PerformanceConfidence::High => "high",
        };
        f.write_str(s)
    }
}

fn confidence_for(eligible: usize) -> PerformanceConfidence {
    match eligible {
        0..MODEL_PERFORMANCE_MIN_ELIGIBLE_SAMPLES => PerformanceConfidence::Insufficient,
        MODEL_PERFORMANCE_MIN_ELIGIBLE_SAMPLES..30 => PerformanceConfidence::Low,
        30..100 => PerformanceConfidence::Medium,
        _ => PerformanceConfidence::High,
    }
}

/// A change is material against a baseline when the relative change reaches
/// the named threshold (ms/token metrics) — direction-neutral here; the
/// regression classes apply the direction.
fn relative_material(current: f64, baseline: f64) -> bool {
    if baseline == 0.0 {
        // Zero baseline: any positive current is a step change, a negative
        // one impossible for these metrics.
        return current > 0.0;
    }
    ((current - baseline) / baseline).abs() >= MODEL_PERFORMANCE_MATERIAL_RELATIVE_CHANGE
}

/// Per-metric workload co-movement, labelled *correlation* — never causation.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WorkloadShiftEvidence {
    /// Always documents the relationship label rendered by surfaces.
    pub relationship: String,
    /// Current-window median deltas vs the preceding baseline; `None` = the
    /// token metric had no eligible samples in either window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<ModelPerformanceDelta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<ModelPerformanceDelta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<ModelPerformanceDelta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<ModelPerformanceDelta>,
    /// At least one token metric moved materially.
    pub material: bool,
}

/// One deterministic assessment of a single monitored metric.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelPerformanceMetricAssessment {
    pub metric: PerformanceMetricKind,
    pub class: PerformanceChangeClass,
    pub confidence: PerformanceConfidence,
    pub eligible_current: usize,
    pub eligible_preceding: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eligible_rolling: Option<usize>,
    /// Current-window median (the rate, for error rate).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_median: Option<f64>,
    /// Current-window p95 (duration only); `None` for single-percentile
    /// metrics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_tail: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preceding_median: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preceding_tail: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rolling_median: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rolling_tail: Option<f64>,
    /// Both baselines are reported whenever available — mixed evidence
    /// never picks one silently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub median_delta_vs_preceding: Option<ModelPerformanceDelta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub median_delta_vs_rolling: Option<ModelPerformanceDelta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail_delta_vs_preceding: Option<ModelPerformanceDelta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail_delta_vs_rolling: Option<ModelPerformanceDelta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload_shift: Option<WorkloadShiftEvidence>,
    /// Error-rate delta vs the preceding baseline (percentage points in
    /// `absolute`), present when error co-movement informed the class.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_association: Option<ModelPerformanceDelta>,
    /// Deterministic explanation strings; the wording here is the wording
    /// surfaces may use.
    pub notes: Vec<String>,
}

/// One deterministic assessment for one (provider, model, fingerprint)
/// identity.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelPerformanceAssessment {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub emitter_fingerprint: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub response_models: Vec<String>,
    pub request_counts: ModelPerformanceCounts,
    /// Assessments for duration, throughput, TTFT and error rate, in that
    /// order.
    pub metrics: Vec<ModelPerformanceMetricAssessment>,
    /// Most severe metric class (see `PerformanceChangeClass::severity`).
    pub overall_class: PerformanceChangeClass,
    /// Weakest metric confidence — the weakest evidence governs.
    pub overall_confidence: PerformanceConfidence,
    /// The bounded sample excluded older spans.
    pub truncated: bool,
    pub notes: Vec<String>,
}

fn median(sample: &Option<ModelPerformanceSample>) -> Option<f64> {
    sample
        .as_ref()
        .and_then(|s| s.percentiles.iter().find(|p| p.percentile == 50))
        .map(|p| p.value)
}

fn tail(sample: &Option<ModelPerformanceSample>) -> Option<f64> {
    sample
        .as_ref()
        .and_then(|s| s.percentiles.iter().find(|p| p.percentile == 95))
        .map(|p| p.value)
}

fn value_delta(current: Option<f64>, baseline: Option<f64>) -> Option<ModelPerformanceDelta> {
    match (current, baseline) {
        (Some(c), Some(b)) => Some(ModelPerformanceDelta {
            absolute: c - b,
            relative: if b != 0.0 { Some((c - b) / b) } else { None },
        }),
        _ => None,
    }
}

/// Build the workload co-movement evidence from the token metrics.
fn workload_shift(identity: &ModelPerformanceIdentity) -> WorkloadShiftEvidence {
    let evidence_for = |metric: &ModelPerformanceMetric| -> Option<ModelPerformanceDelta> {
        let current = median(&metric.current)?;
        let baseline = median(&metric.preceding)?;
        Some(ModelPerformanceDelta {
            absolute: current - baseline,
            relative: if baseline != 0.0 {
                Some((current - baseline) / baseline)
            } else {
                None
            },
        })
    };
    let material_of = |metric: &ModelPerformanceMetric| -> bool {
        match (median(&metric.current), median(&metric.preceding)) {
            (Some(c), Some(b)) => c >= 0.0 && b >= 0.0 && relative_material(c, b),
            _ => false,
        }
    };
    let input = evidence_for(&identity.input_tokens);
    let output = evidence_for(&identity.output_tokens);
    let cache_creation = evidence_for(&identity.cache_creation_tokens);
    let cache_read = evidence_for(&identity.cache_read_tokens);
    let material = material_of(&identity.input_tokens)
        || material_of(&identity.output_tokens)
        || material_of(&identity.cache_creation_tokens)
        || material_of(&identity.cache_read_tokens);
    WorkloadShiftEvidence {
        relationship: "correlation (not causation)".to_string(),
        input_tokens: input,
        output_tokens: output,
        cache_creation_tokens: cache_creation,
        cache_read_tokens: cache_read,
        material,
    }
}

#[allow(clippy::too_many_arguments)] // one knob per classification dimension
fn classify(
    metric: PerformanceMetricKind,
    worse: WorseDirection,
    current: &Option<ModelPerformanceSample>,
    preceding: &Option<ModelPerformanceSample>,
    rolling: &Option<ModelPerformanceSample>,
    has_tail: bool,
    workload: Option<&WorkloadShiftEvidence>,
    error_rate_delta: Option<ModelPerformanceDelta>,
) -> ModelPerformanceMetricAssessment {
    let eligible_current = current
        .as_ref()
        .map(|s| s.eligible_count)
        .unwrap_or_default();
    let eligible_preceding = preceding
        .as_ref()
        .map(|s| s.eligible_count)
        .unwrap_or_default();
    let eligible_rolling = rolling.as_ref().map(|s| s.eligible_count);

    let current_median = median(current);
    let preceding_median = median(preceding);
    let rolling_median = median(rolling);
    let current_tail = has_tail.then(|| tail(current)).flatten();
    let preceding_tail = has_tail.then(|| tail(preceding)).flatten();
    let rolling_tail = has_tail.then(|| tail(rolling)).flatten();

    let median_delta_pre = value_delta(current_median, preceding_median);
    let median_delta_roll = value_delta(current_median, rolling_median);
    let tail_delta_pre = value_delta(current_tail, preceding_tail);
    let tail_delta_roll = value_delta(current_tail, rolling_tail);

    let mut notes = Vec::new();

    // Gate 1: insufficient telemetry. Every metric requires at least the
    // minimum eligible requests in the current window; the count is always
    // reported.
    let insufficient =
        current.is_none() || eligible_current < MODEL_PERFORMANCE_MIN_ELIGIBLE_SAMPLES;
    if insufficient {
        notes.push(format!(
            "{}: {} of the {} eligible current-window requests are below the {} minimum — no assessment",
            metric_label(metric),
            eligible_current,
            eligible_current,
            MODEL_PERFORMANCE_MIN_ELIGIBLE_SAMPLES
        ));
        return ModelPerformanceMetricAssessment {
            metric,
            class: PerformanceChangeClass::InsufficientTelemetry,
            confidence: confidence_for(eligible_current),
            eligible_current,
            eligible_preceding,
            eligible_rolling,
            current_median,
            current_tail,
            preceding_median,
            preceding_tail,
            rolling_median,
            rolling_tail,
            median_delta_vs_preceding: median_delta_pre,
            median_delta_vs_rolling: median_delta_roll,
            tail_delta_vs_preceding: tail_delta_pre,
            tail_delta_vs_rolling: tail_delta_roll,
            workload_shift: None,
            error_association: None,
            notes,
        };
    }

    // Materiality per available baseline (direction-neutral).
    let material_pre = match (current_median, preceding_median) {
        (Some(c), Some(b)) => Some(relative_material(c, b)),
        _ => None,
    };
    let material_roll = match (current_median, rolling_median) {
        (Some(c), Some(b)) => Some(relative_material(c, b)),
        _ => None,
    };
    let tail_material_pre = match (current_tail, preceding_tail) {
        (Some(c), Some(b)) => Some(relative_material(c, b)),
        _ => None,
    };
    let tail_material_roll = match (current_tail, rolling_tail) {
        (Some(c), Some(b)) => Some(relative_material(c, b)),
        _ => None,
    };

    // Gate 2: mixed evidence. Both baselines eligible and they disagree on
    // materiality — report both, never pick one silently.
    if let (Some(mp), Some(mr)) = (material_pre, material_roll) {
        if mp != mr {
            notes.push(format!(
                "{}: mixed evidence — the change is material against the preceding baseline {} but {} against the rolling baseline; both are reported",
                metric_label(metric),
                if mp { "so" } else { "not so" },
                if mr { "so" } else { "not so" }
            ));
            return ModelPerformanceMetricAssessment {
                metric,
                class: PerformanceChangeClass::MixedEvidence,
                confidence: confidence_for(eligible_current),
                eligible_current,
                eligible_preceding,
                eligible_rolling,
                current_median,
                current_tail,
                preceding_median,
                preceding_tail,
                rolling_median,
                rolling_tail,
                median_delta_vs_preceding: median_delta_pre,
                median_delta_vs_rolling: median_delta_roll,
                tail_delta_vs_preceding: tail_delta_pre,
                tail_delta_vs_rolling: tail_delta_roll,
                workload_shift: None,
                error_association: None,
                notes,
            };
        }
    }

    // No eligible baseline at all: there is no comparison. Report it as no
    // material change with an explicit note — never imply a comparison ran.
    if material_pre.is_none() && material_roll.is_none() {
        notes.push(format!(
            "{}: no eligible baseline sample in the preceding or rolling window — no comparison made",
            metric_label(metric)
        ));
    }

    // The operative baseline: preceding when it has eligible samples, else
    // rolling.
    let baseline_material = material_pre.or(material_roll);
    let tail_baseline_material = tail_material_pre.or(tail_material_roll);

    let is_worse = |delta: Option<ModelPerformanceDelta>| -> bool {
        match delta {
            Some(d) => match worse {
                WorseDirection::Up => d.absolute > 0.0,
                WorseDirection::Down => d.absolute < 0.0,
            },
            None => false,
        }
    };

    let operative_delta = median_delta_pre.or(median_delta_roll);
    let operative_tail_delta = tail_delta_pre.or(tail_delta_roll);

    let (class, qualified) = if baseline_material == Some(true) && is_worse(operative_delta) {
        // A material worsening. Qualify it with observed co-movement before
        // calling it a plain regression.
        if error_rate_delta.map(|d| d.absolute > 0.0).unwrap_or(false) {
            (PerformanceChangeClass::ErrorAssociated, true)
        } else if workload.map(|w| w.material).unwrap_or(false) {
            (PerformanceChangeClass::WorkloadShiftCorrelated, true)
        } else {
            (PerformanceChangeClass::TypicalRegression, false)
        }
    } else if has_tail && tail_baseline_material == Some(true) && is_worse(operative_tail_delta) {
        if error_rate_delta.map(|d| d.absolute > 0.0).unwrap_or(false) {
            (PerformanceChangeClass::ErrorAssociated, true)
        } else if workload.map(|w| w.material).unwrap_or(false) {
            (PerformanceChangeClass::WorkloadShiftCorrelated, true)
        } else {
            (PerformanceChangeClass::TailRegression, false)
        }
    } else {
        // No material worsening. A material *improvement* is still worth
        // saying, without regression wording.
        if baseline_material == Some(true) {
            notes.push(format!(
                "{}: material change in the better direction — reported as no material change",
                metric_label(metric)
            ));
        }
        (PerformanceChangeClass::NoMaterialChange, false)
    };

    if qualified {
        if let Some(d) = &error_rate_delta {
            notes.push(format!(
                "error co-movement observed (correlation, not causation): {d:?}"
            ));
        }
        if let Some(w) = workload {
            if w.material {
                notes
                    .push("workload co-movement observed (correlation, not causation)".to_string());
            }
        }
    }

    ModelPerformanceMetricAssessment {
        metric,
        class,
        confidence: confidence_for(eligible_current),
        eligible_current,
        eligible_preceding,
        eligible_rolling,
        current_median,
        current_tail,
        preceding_median,
        preceding_tail,
        rolling_median,
        rolling_tail,
        median_delta_vs_preceding: median_delta_pre,
        median_delta_vs_rolling: median_delta_roll,
        tail_delta_vs_preceding: tail_delta_pre,
        tail_delta_vs_rolling: tail_delta_roll,
        workload_shift: workload.cloned().filter(|_| qualified),
        error_association: error_rate_delta.filter(|_| qualified),
        notes,
    }
}

fn metric_label(metric: PerformanceMetricKind) -> &'static str {
    match metric {
        PerformanceMetricKind::Duration => "duration",
        PerformanceMetricKind::Throughput => "throughput",
        PerformanceMetricKind::Ttft => "ttft",
        PerformanceMetricKind::ErrorRate => "error rate",
    }
}

/// Deterministic assessment of one identity. `ttft_trust` gates TTFT
/// conclusions (#111/#120); `truncated` is carried through from the #151
/// sample.
pub fn assess_identity(
    identity: &ModelPerformanceIdentity,
    ttft_trust: TtftTrust,
    truncated: bool,
) -> ModelPerformanceAssessment {
    let workload = workload_shift(identity);

    // Error-rate materiality (percentage points) informs the other metrics.
    let error_delta_pre = identity.error_rate.current.as_ref().and_then(|c| {
        identity
            .error_rate
            .preceding
            .as_ref()
            .map(|p| ModelPerformanceDelta {
                absolute: c.rate - p.rate,
                relative: if p.rate != 0.0 {
                    Some((c.rate - p.rate) / p.rate)
                } else {
                    None
                },
            })
    });
    let error_material = error_delta_pre
        .as_ref()
        .map(|d| d.absolute.abs() >= MODEL_PERFORMANCE_MATERIAL_ERROR_RATE_POINTS)
        .unwrap_or(false);

    let duration = classify(
        PerformanceMetricKind::Duration,
        WorseDirection::Up,
        &identity.duration.current,
        &identity.duration.preceding,
        &identity.duration.rolling,
        true,
        Some(&workload),
        error_delta_pre.filter(|_| error_material),
    );
    let throughput = classify(
        PerformanceMetricKind::Throughput,
        WorseDirection::Down,
        &identity.throughput.current,
        &identity.throughput.preceding,
        &identity.throughput.rolling,
        false,
        Some(&workload),
        error_delta_pre.filter(|_| error_material),
    );
    let mut ttft = classify(
        PerformanceMetricKind::Ttft,
        WorseDirection::Up,
        &identity.ttft.current,
        &identity.ttft.preceding,
        &identity.ttft.rolling,
        false,
        Some(&workload),
        error_delta_pre.filter(|_| error_material),
    );
    if ttft_trust == TtftTrust::Unreliable {
        ttft.class = PerformanceChangeClass::InsufficientTelemetry;
        ttft.notes.push(
            "TTFT values are absent, sparse, invalid, or degenerate: first-response and \
             decode-rate attribution is prevented"
                .to_string(),
        );
    }

    // Error rate assessed on its own terms (percentage points, Up = worse).
    let mut error_rate = classify(
        PerformanceMetricKind::ErrorRate,
        WorseDirection::Up,
        &sample_from_error_value(identity.error_rate.current.as_ref()),
        &sample_from_error_value(identity.error_rate.preceding.as_ref()),
        &sample_from_error_value(identity.error_rate.rolling.as_ref()),
        false,
        Some(&workload),
        None,
    );
    if error_rate.class == PerformanceChangeClass::TypicalRegression {
        error_rate.class = PerformanceChangeClass::ErrorAssociated;
        error_rate.error_association = error_delta_pre;
        error_rate
            .notes
            .push("error rate rose materially".to_string());
    }

    let metrics = vec![duration, throughput, ttft, error_rate];
    let overall_class = metrics
        .iter()
        .map(|m| m.class)
        .max_by_key(|c| c.severity())
        .unwrap_or(PerformanceChangeClass::InsufficientTelemetry);
    let overall_confidence = metrics
        .iter()
        .map(|m| m.confidence)
        .min()
        .unwrap_or(PerformanceConfidence::Insufficient);

    let mut notes = Vec::new();
    if truncated {
        notes.push(
            "the bounded sample excluded older spans; statistics cover the most recent spans only"
                .to_string(),
        );
    }

    ModelPerformanceAssessment {
        provider: identity.provider.clone(),
        model: identity.model.clone(),
        emitter_fingerprint: identity.emitter_fingerprint.clone(),
        response_models: identity.response_models.clone(),
        request_counts: identity.request_counts,
        metrics,
        overall_class,
        overall_confidence,
        truncated,
        notes,
    }
}

/// Sample view over an error-rate value: the rate plays the role of the
/// median, the request count plays the role of the eligible count.
fn sample_from_error_value(
    value: Option<&ModelPerformanceErrorValue>,
) -> Option<ModelPerformanceSample> {
    let value = value?;
    if value.requests == 0 {
        return None;
    }
    Some(ModelPerformanceSample {
        eligible_count: value.requests,
        percentiles: vec![crate::api::ModelPerformancePercentile {
            percentile: 50,
            value: value.rate,
            delta_vs_preceding: None,
            delta_vs_rolling: None,
        }],
    })
}

/// Deterministic assessment of a whole #151 response; the TTFT trust is
/// supplied per identity (it derives from the capability report's
/// quality/derivation for that emitter fingerprint).
pub fn assess_response<F>(
    response: &crate::api::ModelPerformanceResponse,
    mut ttft_trust_for: F,
) -> Vec<ModelPerformanceAssessment>
where
    F: FnMut(&ModelPerformanceIdentity) -> TtftTrust,
{
    response
        .identities
        .iter()
        .map(|identity| assess_identity(identity, ttft_trust_for(identity), response.truncated))
        .collect()
}

/// Derive the TTFT trust gate for one identity from the capability report
/// (#111 quality + #120 derivation). Reliable requires the TTFT metric to
/// be available, reliable, and natively observed or structurally
/// correlated. Fails closed: an unknown fingerprint or any
/// absent/sparse/invalid/degenerate state is `Unreliable`, which prevents
/// first-response and decode-rate attribution.
pub fn ttft_trust_from_capabilities(
    capability: &crate::api::GenAiCapabilityResponse,
    identity: &ModelPerformanceIdentity,
) -> TtftTrust {
    // The fingerprint is not unique across (provider, model) — standard-
    // otel emitters with no service/scope share one — so match the full
    // identity.
    capability
        .reports
        .iter()
        .find(|r| {
            r.emitter_fingerprint == identity.emitter_fingerprint
                && r.provider == identity.provider
                && r.model == identity.model
        })
        .map_or(TtftTrust::Unreliable, |report| {
            let ttft = &report.ttft;
            if ttft.availability == "available"
                && ttft.quality == "reliable"
                && (ttft.derivation == "native" || ttft.derivation == "correlated")
            {
                TtftTrust::Reliable
            } else {
                TtftTrust::Unreliable
            }
        })
}

/// Assemble the full diagnosis envelope (#153): the raw #151 evidence plus
/// the deterministic #152 assessments (TTFT trust from the capability
/// report) plus the echoed timezone. The API handler and the CLI both call
/// this, so their json-compact output is structurally identical.
pub fn build_diagnosis(
    response: &crate::api::ModelPerformanceResponse,
    capability: &crate::api::GenAiCapabilityResponse,
    timezone: Option<String>,
) -> crate::api::ModelPerformanceDiagnosis {
    let assessments = assess_response(response, |id| ttft_trust_from_capabilities(capability, id));
    crate::api::ModelPerformanceDiagnosis {
        current_window: response.current_window,
        preceding_window: response.preceding_window,
        rolling_window: response.rolling_window,
        timezone,
        truncated: response.truncated,
        identities: response.identities.clone(),
        assessments,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{
        ModelPerformanceErrorRate, ModelPerformanceErrorValue, ModelPerformancePercentile,
    };

    fn p50(value: f64) -> Vec<ModelPerformancePercentile> {
        vec![ModelPerformancePercentile {
            percentile: 50,
            value,
            delta_vs_preceding: None,
            delta_vs_rolling: None,
        }]
    }
    fn p50_p95(p50: f64, p95: f64) -> Vec<ModelPerformancePercentile> {
        vec![
            ModelPerformancePercentile {
                percentile: 50,
                value: p50,
                delta_vs_preceding: None,
                delta_vs_rolling: None,
            },
            ModelPerformancePercentile {
                percentile: 95,
                value: p95,
                delta_vs_preceding: None,
                delta_vs_rolling: None,
            },
        ]
    }
    fn sample(
        count: usize,
        percentiles: Vec<ModelPerformancePercentile>,
    ) -> ModelPerformanceSample {
        ModelPerformanceSample {
            eligible_count: count,
            percentiles,
        }
    }
    fn metric(
        current: Option<ModelPerformanceSample>,
        preceding: Option<ModelPerformanceSample>,
    ) -> ModelPerformanceMetric {
        ModelPerformanceMetric {
            current,
            preceding,
            rolling: None,
        }
    }
    fn identity(
        duration: ModelPerformanceMetric,
        throughput: ModelPerformanceMetric,
        ttft: ModelPerformanceMetric,
        error_rate: ModelPerformanceErrorRate,
    ) -> ModelPerformanceIdentity {
        ModelPerformanceIdentity {
            provider: Some("openai".to_string()),
            model: Some("gpt-test".to_string()),
            emitter_fingerprint: "genai-v1-0".to_string(),
            response_models: vec![],
            request_counts: ModelPerformanceCounts {
                current: 12,
                preceding: 12,
                rolling: 0,
            },
            duration,
            throughput,
            ttft,
            input_tokens: metric(None, None),
            cache_creation_tokens: metric(None, None),
            cache_read_tokens: metric(None, None),
            output_tokens: metric(None, None),
            error_rate,
        }
    }
    fn ok_error_rate(requests: usize, errors: usize) -> ModelPerformanceErrorValue {
        ModelPerformanceErrorValue {
            requests,
            errors,
            rate: errors as f64 / requests as f64,
            delta_vs_preceding: None,
            delta_vs_rolling: None,
        }
    }

    /// Baseline identity: 12 eligible everywhere, no change anywhere.
    fn flat_identity() -> ModelPerformanceIdentity {
        identity(
            metric(
                Some(sample(12, p50_p95(100.0, 150.0))),
                Some(sample(12, p50_p95(100.0, 150.0))),
            ),
            metric(Some(sample(12, p50(500.0))), Some(sample(12, p50(500.0)))),
            metric(Some(sample(12, p50(50.0))), Some(sample(12, p50(50.0)))),
            ModelPerformanceErrorRate {
                current: Some(ok_error_rate(12, 0)),
                preceding: Some(ok_error_rate(12, 0)),
                rolling: None,
            },
        )
    }

    #[test]
    fn flat_fixture_is_no_material_change() {
        let a = assess_identity(&flat_identity(), TtftTrust::Reliable, false);
        assert_eq!(a.overall_class, PerformanceChangeClass::NoMaterialChange);
        assert_eq!(
            a.overall_confidence,
            PerformanceConfidence::Low,
            "12 samples = Low"
        );
        for m in &a.metrics {
            assert_eq!(
                m.class,
                PerformanceChangeClass::NoMaterialChange,
                "{:?}",
                m.metric
            );
        }
    }

    #[test]
    fn median_only_worsening_is_typical_regression() {
        let mut id = flat_identity();
        // Median +30%, tail +30% (both move; median is the operative one).
        id.duration = metric(
            Some(sample(12, p50_p95(130.0, 195.0))),
            Some(sample(12, p50_p95(100.0, 150.0))),
        );
        let a = assess_identity(&id, TtftTrust::Reliable, false);
        let duration = &a.metrics[0];
        assert_eq!(duration.class, PerformanceChangeClass::TypicalRegression);
        assert_eq!(
            duration
                .median_delta_vs_preceding
                .as_ref()
                .unwrap()
                .relative,
            Some(0.3)
        );
    }

    #[test]
    fn tail_only_worsening_is_tail_regression() {
        let mut id = flat_identity();
        // Median flat, p95 +33%.
        id.duration = metric(
            Some(sample(12, p50_p95(100.0, 200.0))),
            Some(sample(12, p50_p95(100.0, 150.0))),
        );
        let a = assess_identity(&id, TtftTrust::Reliable, false);
        let duration = &a.metrics[0];
        assert_eq!(duration.class, PerformanceChangeClass::TailRegression);
        assert_eq!(
            duration
                .median_delta_vs_preceding
                .as_ref()
                .unwrap()
                .absolute,
            0.0
        );
    }

    #[test]
    fn throughput_drop_is_regression_too() {
        let mut id = flat_identity();
        id.throughput = metric(Some(sample(12, p50(350.0))), Some(sample(12, p50(500.0))));
        let a = assess_identity(&id, TtftTrust::Reliable, false);
        assert_eq!(
            a.metrics[1].class,
            PerformanceChangeClass::TypicalRegression,
            "lower throughput is worse"
        );
    }

    #[test]
    fn material_improvement_is_not_a_regression() {
        let mut id = flat_identity();
        id.duration = metric(
            Some(sample(12, p50_p95(70.0, 105.0))),
            Some(sample(12, p50_p95(100.0, 150.0))),
        );
        let a = assess_identity(&id, TtftTrust::Reliable, false);
        let duration = &a.metrics[0];
        assert_eq!(duration.class, PerformanceChangeClass::NoMaterialChange);
        assert!(
            duration
                .notes
                .iter()
                .any(|n| n.contains("better direction")),
            "improvement is documented: {:?}",
            duration.notes
        );
    }

    #[test]
    fn workload_co_movement_qualifies_the_regression() {
        let mut id = flat_identity();
        id.duration = metric(
            Some(sample(12, p50_p95(130.0, 195.0))),
            Some(sample(12, p50_p95(100.0, 150.0))),
        );
        // Output tokens +50% (workload shift).
        id.output_tokens = metric(Some(sample(12, p50(150.0))), Some(sample(12, p50(100.0))));
        let a = assess_identity(&id, TtftTrust::Reliable, false);
        let duration = &a.metrics[0];
        assert_eq!(
            duration.class,
            PerformanceChangeClass::WorkloadShiftCorrelated
        );
        let shift = duration.workload_shift.as_ref().unwrap();
        assert_eq!(shift.relationship, "correlation (not causation)");
        assert!(shift.material);
        assert_eq!(shift.output_tokens.as_ref().unwrap().relative, Some(0.5));
    }

    #[test]
    fn error_co_movement_outranks_workload() {
        let mut id = flat_identity();
        id.duration = metric(
            Some(sample(12, p50_p95(130.0, 195.0))),
            Some(sample(12, p50_p95(100.0, 150.0))),
        );
        id.output_tokens = metric(Some(sample(12, p50(150.0))), Some(sample(12, p50(100.0))));
        // 0 errors -> 4 of 12 = 33 points rise.
        id.error_rate = ModelPerformanceErrorRate {
            current: Some(ok_error_rate(12, 4)),
            preceding: Some(ok_error_rate(12, 0)),
            rolling: None,
        };
        let a = assess_identity(&id, TtftTrust::Reliable, false);
        let duration = &a.metrics[0];
        assert_eq!(duration.class, PerformanceChangeClass::ErrorAssociated);
        assert!(
            duration.error_association.is_some(),
            "the error delta is reported"
        );
        assert!(
            duration
                .notes
                .iter()
                .any(|n| n.contains("workload co-movement")),
            "the co-occurring workload shift is still noted"
        );
    }

    #[test]
    fn error_rate_material_change_is_error_associated() {
        let mut id = flat_identity();
        id.error_rate = ModelPerformanceErrorRate {
            current: Some(ok_error_rate(20, 2)), // 10%
            preceding: Some(ok_error_rate(20, 0)),
            rolling: None,
        };
        let a = assess_identity(&id, TtftTrust::Reliable, false);
        let error = &a.metrics[3];
        assert_eq!(error.class, PerformanceChangeClass::ErrorAssociated);
        assert_eq!(error.eligible_current, 20);
    }

    #[test]
    fn mixed_evidence_when_baselines_disagree() {
        let mut id = flat_identity();
        // Current 130 vs preceding 100 (material), vs rolling 125 (not
        // material: 4%).
        id.duration = metric(
            Some(sample(12, p50_p95(130.0, 195.0))),
            Some(sample(12, p50_p95(100.0, 150.0))),
        );
        id.duration.rolling = Some(sample(12, p50_p95(125.0, 187.5)));
        let a = assess_identity(&id, TtftTrust::Reliable, false);
        let duration = &a.metrics[0];
        assert_eq!(duration.class, PerformanceChangeClass::MixedEvidence);
        assert!(
            duration.median_delta_vs_preceding.is_some()
                && duration.median_delta_vs_rolling.is_some(),
            "both baselines are reported"
        );
    }

    #[test]
    fn threshold_boundaries_at_nine_ten_eleven() {
        let make = |n: usize| -> ModelPerformanceIdentity {
            let mut id = flat_identity();
            id.duration = metric(
                Some(sample(n, p50_p95(130.0, 195.0))),
                Some(sample(n, p50_p95(100.0, 150.0))),
            );
            id
        };
        let nine = assess_identity(&make(9), TtftTrust::Reliable, false);
        assert_eq!(
            nine.metrics[0].class,
            PerformanceChangeClass::InsufficientTelemetry,
            "9 samples: below the minimum"
        );
        assert_eq!(nine.metrics[0].eligible_current, 9);

        let ten = assess_identity(&make(10), TtftTrust::Reliable, false);
        assert_eq!(
            ten.metrics[0].class,
            PerformanceChangeClass::TypicalRegression,
            "10 samples: assessable, Low confidence"
        );
        assert_eq!(ten.metrics[0].confidence, PerformanceConfidence::Low);

        let eleven = assess_identity(&make(11), TtftTrust::Reliable, false);
        assert_eq!(
            eleven.metrics[0].class,
            PerformanceChangeClass::TypicalRegression
        );
        assert_eq!(eleven.metrics[0].confidence, PerformanceConfidence::Low);
    }

    #[test]
    fn relative_threshold_boundary() {
        let make = |current: f64| -> ModelPerformanceIdentity {
            let mut id = flat_identity();
            id.duration = metric(
                Some(sample(12, p50_p95(current, current * 1.5))),
                Some(sample(12, p50_p95(100.0, 150.0))),
            );
            id
        };
        // Exactly 20% is material (>= threshold).
        let at = assess_identity(&make(120.0), TtftTrust::Reliable, false);
        assert_eq!(
            at.metrics[0].class,
            PerformanceChangeClass::TypicalRegression
        );
        // 19% is not.
        let below = assess_identity(&make(119.0), TtftTrust::Reliable, false);
        assert_eq!(
            below.metrics[0].class,
            PerformanceChangeClass::NoMaterialChange
        );
    }

    #[test]
    fn unreliable_ttft_prevents_attribution() {
        let mut id = flat_identity();
        id.ttft = metric(Some(sample(50, p50(80.0))), Some(sample(50, p50(50.0))));
        let reliable = assess_identity(&id, TtftTrust::Reliable, false);
        assert_eq!(
            reliable.metrics[2].class,
            PerformanceChangeClass::TypicalRegression
        );

        let unreliable = assess_identity(&id, TtftTrust::Unreliable, false);
        assert_eq!(
            unreliable.metrics[2].class,
            PerformanceChangeClass::InsufficientTelemetry,
            "sample-rich but structurally invalid TTFT is not assessed"
        );
        assert!(unreliable.metrics[2]
            .notes
            .iter()
            .any(|n| n.contains("attribution is prevented")));
    }

    #[test]
    fn confidence_scales_with_samples() {
        let make = |n: usize| {
            let mut id = flat_identity();
            id.duration = metric(
                Some(sample(n, p50_p95(100.0, 150.0))),
                Some(sample(n, p50_p95(100.0, 150.0))),
            );
            id
        };
        assert_eq!(
            assess_identity(&make(9), TtftTrust::Reliable, false).metrics[0].confidence,
            PerformanceConfidence::Insufficient
        );
        assert_eq!(
            assess_identity(&make(10), TtftTrust::Reliable, false).metrics[0].confidence,
            PerformanceConfidence::Low
        );
        assert_eq!(
            assess_identity(&make(30), TtftTrust::Reliable, false).metrics[0].confidence,
            PerformanceConfidence::Medium
        );
        assert_eq!(
            assess_identity(&make(120), TtftTrust::Reliable, false).metrics[0].confidence,
            PerformanceConfidence::High
        );
    }

    #[test]
    fn overall_headline_uses_severity_and_weakest_confidence() {
        let mut id = flat_identity();
        id.duration = metric(
            Some(sample(12, p50_p95(130.0, 195.0))),
            Some(sample(12, p50_p95(100.0, 150.0))),
        );
        // TTFT has no baseline sample -> no comparison, documented.
        id.ttft = metric(Some(sample(12, p50(50.0))), None);
        let a = assess_identity(&id, TtftTrust::Reliable, true);
        assert_eq!(a.overall_class, PerformanceChangeClass::TypicalRegression);
        assert_eq!(a.overall_confidence, PerformanceConfidence::Low);
        let ttft = &a.metrics[2];
        assert_eq!(ttft.class, PerformanceChangeClass::NoMaterialChange);
        assert!(
            ttft.notes.iter().any(|n| n.contains("no comparison made")),
            "missing baseline is documented, not implied: {:?}",
            ttft.notes
        );
        assert!(a.truncated);
        assert!(a.notes.iter().any(|n| n.contains("bounded sample")));
    }
}
