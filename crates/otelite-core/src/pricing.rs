//! Model pricing data + cost computation.
//!
//! Primary source: LiteLLM's community-maintained JSON
//! (`model_prices_and_context_window.json`), MIT-licensed © 2023 Berri AI. The
//! server fetches and caches it; this module holds the parsed form and performs
//! lookup / cost math. When LiteLLM is unavailable we fall back to a small
//! hardcoded Claude 4.x table so the UI still shows something reasonable.
//!
//! All rates are USD per token.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Token counts used for cost computation. Fields correspond to what our span
/// ingestion extracts via [`crate::semconv`]; every field is optional so callers
/// can pass only what they have.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
}

/// A single LiteLLM pricing entry. We keep only the fields we use.
///
/// LiteLLM stores many more fields (context windows, modality flags, deprecated
/// dates, ...) — `#[serde(default)]` lets us silently drop them.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct PricingEntry {
    pub input_cost_per_token: f64,
    pub output_cost_per_token: f64,
    pub cache_creation_input_token_cost: Option<f64>,
    pub cache_read_input_token_cost: Option<f64>,
    pub litellm_provider: Option<String>,
}

/// Origin of a resolved cost. Surfaced to the UI so the disclaimer can
/// accurately describe where numbers came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CostSource {
    /// LiteLLM pricing data (live fetch or server cache).
    Litellm,
    /// Hardcoded Claude 4.x fallback (LiteLLM unavailable or entry missing).
    Fallback,
    /// No pricing matched — cost is `None`.
    None,
}

impl CostSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            CostSource::Litellm => "litellm",
            CostSource::Fallback => "fallback",
            CostSource::None => "none",
        }
    }
}

/// Result of a cost lookup + computation.
#[derive(Debug, Clone)]
pub struct CostResult {
    pub cost: Option<f64>,
    pub source: CostSource,
    /// When `cost` is `None`, a short human-readable explanation (e.g.
    /// "no pricing data for claude-foo on bedrock"). Used as a tooltip.
    pub reason: Option<String>,
}

impl CostResult {
    fn none(model: Option<&str>, system: Option<&str>) -> Self {
        let reason = match (model, system) {
            (Some(m), Some(s)) => Some(format!("no pricing data for {m} on {s}")),
            (Some(m), None) => Some(format!("no pricing data for {m}")),
            _ => Some("no pricing data (missing model)".to_string()),
        };
        Self {
            cost: None,
            source: CostSource::None,
            reason,
        }
    }
}

/// Hardcoded fallback for Claude 4.x, matching what Anthropic publishes at
/// <https://www.anthropic.com/pricing>. Rates are USD per token (per-1M divided
/// by 1e6). Cache write multipliers follow Anthropic's published rates
/// (5m ≈ 1.25x, 1h ≈ 2x, read ≈ 0.1x).
struct FallbackEntry {
    input: f64,
    output: f64,
    cache_5m: f64,
    cache_1h: f64,
    cache_read: f64,
}

const MILLION: f64 = 1_000_000.0;

const CLAUDE_OPUS_FALLBACK: FallbackEntry = FallbackEntry {
    input: 15.0 / MILLION,
    output: 75.0 / MILLION,
    cache_5m: 18.75 / MILLION,
    cache_1h: 30.0 / MILLION,
    cache_read: 1.5 / MILLION,
};
const CLAUDE_SONNET_FALLBACK: FallbackEntry = FallbackEntry {
    input: 3.0 / MILLION,
    output: 15.0 / MILLION,
    cache_5m: 3.75 / MILLION,
    cache_1h: 6.0 / MILLION,
    cache_read: 0.3 / MILLION,
};
const CLAUDE_HAIKU_FALLBACK: FallbackEntry = FallbackEntry {
    input: 1.0 / MILLION,
    output: 5.0 / MILLION,
    cache_5m: 1.25 / MILLION,
    cache_1h: 2.0 / MILLION,
    cache_read: 0.1 / MILLION,
};

pub const FALLBACK_LAST_VERIFIED: &str = "2026-05-07";

/// Attribution for LiteLLM data — surfaced in API metadata so the UI can render
/// the notice without embedding it client-side.
pub const LITELLM_SOURCE_URL: &str =
    "https://github.com/BerriAI/litellm/blob/main/model_prices_and_context_window.json";
pub const LITELLM_RAW_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
pub const LITELLM_LICENSE: &str = "MIT — © 2023 Berri AI";

fn claude_fallback(model: &str) -> Option<&'static FallbackEntry> {
    let m = model.to_ascii_lowercase();
    if has_claude_family_token(&m, "opus") {
        Some(&CLAUDE_OPUS_FALLBACK)
    } else if has_claude_family_token(&m, "sonnet") {
        Some(&CLAUDE_SONNET_FALLBACK)
    } else if has_claude_family_token(&m, "haiku") {
        Some(&CLAUDE_HAIKU_FALLBACK)
    } else {
        None
    }
}

/// Return whether a Claude family name occurs as a complete model-name token.
///
/// The fallback rates are deliberately coarse, so partial matches such as
/// `not-an-opus-model` must not produce an authoritative estimate.
fn has_claude_family_token(model: &str, family: &str) -> bool {
    model.match_indices(family).any(|(start, _)| {
        let before = model[..start].chars().next_back();
        let suffix = &model[start + family.len()..];
        let versioned = suffix
            .strip_prefix('-')
            .or_else(|| suffix.strip_prefix('.'))
            .is_some_and(|version| {
                version.starts_with(|character: char| character.is_ascii_digit())
            });
        !before.is_some_and(|character| character.is_ascii_alphanumeric())
            && (suffix.is_empty()
                || suffix.starts_with(|character: char| character.is_ascii_digit())
                || versioned)
    })
}

/// Remove a client-side context-window label that is not part of the model ID
/// sent to the provider.
///
/// Public so callers that join span data against model-keyed sources
/// (e.g. the lines-of-code labels, which carry the bare name) can
/// normalise exactly the way the price lookup does (#178).
pub fn pricing_model_name(model: &str) -> &str {
    model.strip_suffix("[1m]").unwrap_or(model)
}

/// Parsed pricing database. Lookups walk a small list of candidate keys that
/// match LiteLLM's naming conventions (bare model name, provider-prefixed,
/// lowercased, ...). If nothing matches, the Claude 4.x fallback is tried.
#[derive(Debug, Clone, Default)]
pub struct PricingDatabase {
    entries: HashMap<String, PricingEntry>,
    loaded_from_litellm: bool,
}

impl PricingDatabase {
    /// Empty database — all lookups will use the Claude fallback.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Parse raw LiteLLM JSON text. Returns an error if the root isn't an
    /// object; silently ignores individual entries that fail to parse (LiteLLM
    /// occasionally emits entries with non-numeric strings or sentinel values
    /// we can't use).
    pub fn from_litellm_json(raw: &str) -> serde_json::Result<Self> {
        let map: HashMap<String, serde_json::Value> = serde_json::from_str(raw)?;
        let mut entries = HashMap::with_capacity(map.len());
        for (k, v) in map {
            if let Ok(entry) = serde_json::from_value::<PricingEntry>(v) {
                entries.insert(k, entry);
            }
        }
        Ok(Self {
            entries,
            loaded_from_litellm: true,
        })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn is_litellm(&self) -> bool {
        self.loaded_from_litellm
    }

    fn lookup(&self, model: &str, system: Option<&str>) -> Option<&PricingEntry> {
        if self.entries.is_empty() {
            return None;
        }
        let lower = model.to_ascii_lowercase();
        let sys_lower = system.map(|s| s.to_ascii_lowercase());

        // 1. Exact matches (with and without provider prefix, case variants).
        let mut candidates: Vec<String> = Vec::with_capacity(6);
        candidates.push(model.to_string());
        candidates.push(lower.clone());
        if let Some(s) = sys_lower.as_deref() {
            candidates.push(format!("{s}/{model}"));
            candidates.push(format!("{s}/{lower}"));
            match s {
                "aws.bedrock" | "bedrock" => candidates.push(format!("bedrock/{lower}")),
                "gcp.vertex_ai" | "vertex" | "vertex_ai" => {
                    candidates.push(format!("vertex_ai/{lower}"))
                },
                _ => {},
            }
        }
        for k in &candidates {
            if let Some(e) = self.entries.get(k) {
                return Some(e);
            }
        }

        // 2. Substring fallback — any LiteLLM key that contains the model name.
        //    Prefer shorter keys (closer to exact match).
        let mut best: Option<(&str, &PricingEntry)> = None;
        for (k, v) in &self.entries {
            if k.to_ascii_lowercase().contains(&lower) {
                match best {
                    None => best = Some((k, v)),
                    Some((bk, _)) if k.len() < bk.len() => best = Some((k, v)),
                    _ => {},
                }
            }
        }
        best.map(|(_, v)| v)
    }

    /// Compute cost for a single LLM call.
    ///
    /// Matching order:
    /// 1. LiteLLM entry (exact, provider-prefixed, or substring match).
    /// 2. Hardcoded Claude 4.x fallback (matches "opus" / "sonnet" / "haiku" in the model name).
    /// 3. Returns `None` with a reason string for the UI tooltip.
    pub fn compute_cost(
        &self,
        model: Option<&str>,
        usage: TokenUsage,
        system: Option<&str>,
    ) -> CostResult {
        let Some(model) = model else {
            return CostResult::none(None, system);
        };

        let pricing_model = pricing_model_name(model);
        if let Some(entry) = self.lookup(pricing_model, system) {
            if entry.input_cost_per_token > 0.0 || entry.output_cost_per_token > 0.0 {
                let cct = entry
                    .cache_creation_input_token_cost
                    .unwrap_or(entry.input_cost_per_token);
                let crt = entry.cache_read_input_token_cost.unwrap_or(0.0);
                let cost = (usage.input as f64) * entry.input_cost_per_token
                    + (usage.output as f64) * entry.output_cost_per_token
                    + (usage.cache_creation as f64) * cct
                    + (usage.cache_read as f64) * crt;
                return CostResult {
                    cost: Some(cost),
                    source: CostSource::Litellm,
                    reason: None,
                };
            }
        }

        if let Some(fb) = claude_fallback(pricing_model) {
            // Fallback has no way to split 5m vs 1h cache tiers — use 5m rate
            // for the full cache_creation bucket (conservative: under-reports
            // 1h cache which is 2x more expensive).
            let cost = (usage.input as f64) * fb.input
                + (usage.output as f64) * fb.output
                + (usage.cache_creation as f64) * fb.cache_5m
                + (usage.cache_read as f64) * fb.cache_read;
            // Silence unused-field warning: cache_1h exists for future use when
            // span attributes carry the 5m/1h split.
            let _ = fb.cache_1h;
            return CostResult {
                cost: Some(cost),
                source: CostSource::Fallback,
                reason: None,
            };
        }

        CostResult::none(Some(model), system)
    }

    /// Estimated savings from prompt-cache reads: every cached-read token is
    /// billed at the cache-read rate instead of the full input rate, so
    /// `savings = cache_read_tokens × (input_rate − cache_read_rate)`.
    ///
    /// Returns `cost: None` when either rate is unknown — no LiteLLM entry,
    /// or the entry lacks a cache-read rate — and never fabricates a rate.
    /// A zero rate difference (free model) is a known zero, not unknown.
    pub fn compute_cache_savings(
        &self,
        model: Option<&str>,
        cache_read_tokens: u64,
        system: Option<&str>,
    ) -> CostResult {
        let Some(model) = model else {
            return CostResult::none(None, system);
        };

        let pricing_model = pricing_model_name(model);
        if let Some(entry) = self.lookup(pricing_model, system) {
            let Some(crt) = entry.cache_read_input_token_cost else {
                let reason = Some(format!("no cache-read price for {pricing_model}"));
                return CostResult {
                    cost: None,
                    source: CostSource::None,
                    reason,
                };
            };
            let per_token = entry.input_cost_per_token - crt;
            return CostResult {
                cost: Some((cache_read_tokens as f64) * per_token),
                source: CostSource::Litellm,
                reason: None,
            };
        }

        if let Some(fb) = claude_fallback(pricing_model) {
            return CostResult {
                cost: Some((cache_read_tokens as f64) * (fb.input - fb.cache_read)),
                source: CostSource::Fallback,
                reason: None,
            };
        }

        CostResult::none(Some(model), system)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(input: u64, output: u64) -> TokenUsage {
        TokenUsage {
            input,
            output,
            ..Default::default()
        }
    }

    #[test]
    fn empty_db_uses_claude_fallback_for_sonnet() {
        let db = PricingDatabase::empty();
        let result = db.compute_cost(Some("claude-sonnet-4"), u(1_000_000, 1_000_000), None);
        // Sonnet: $3 input + $15 output per 1M = $18
        assert_eq!(result.source, CostSource::Fallback);
        let cost = result.cost.unwrap();
        assert!((cost - 18.0).abs() < 1e-9, "got {cost}");
    }

    #[test]
    fn claude_fallback_matches_haiku() {
        let db = PricingDatabase::empty();
        let result = db.compute_cost(Some("claude-haiku-4.5"), u(1_000_000, 0), None);
        assert_eq!(result.source, CostSource::Fallback);
        assert!((result.cost.unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn claude_fallback_rejects_partial_family_names() {
        let db = PricingDatabase::empty();
        let result = db.compute_cost(Some("not-an-opus-model"), u(1_000_000, 0), None);
        assert_eq!(result.source, CostSource::None);
        assert!(result.cost.is_none());
    }

    #[test]
    fn fallback_applies_cache_creation_and_read_rates() {
        let db = PricingDatabase::empty();
        let usage = TokenUsage {
            input: 0,
            output: 0,
            cache_creation: 1_000_000,
            cache_read: 1_000_000,
        };
        let result = db.compute_cost(Some("claude-sonnet-4"), usage, None);
        // cache_5m $3.75 + cache_read $0.30 per 1M = $4.05
        assert!((result.cost.unwrap() - 4.05).abs() < 1e-9);
    }

    #[test]
    fn unknown_model_returns_none_with_reason() {
        let db = PricingDatabase::empty();
        let result = db.compute_cost(Some("gpt-9000"), u(1, 1), None);
        assert_eq!(result.source, CostSource::None);
        assert!(result.cost.is_none());
        assert!(result.reason.unwrap().contains("gpt-9000"));
    }

    #[test]
    fn missing_model_returns_none() {
        let db = PricingDatabase::empty();
        let result = db.compute_cost(None, u(1, 1), Some("openai"));
        assert_eq!(result.source, CostSource::None);
        assert!(result.cost.is_none());
    }

    #[test]
    fn litellm_parse_and_lookup_exact_key() {
        let json = r#"{
            "gpt-4o": {
                "input_cost_per_token": 2.5e-6,
                "output_cost_per_token": 1.0e-5,
                "litellm_provider": "openai"
            },
            "sample_spec": { "note": "ignored" }
        }"#;
        let db = PricingDatabase::from_litellm_json(json).unwrap();
        assert!(db.is_litellm());
        let result = db.compute_cost(Some("gpt-4o"), u(1_000_000, 1_000_000), None);
        assert_eq!(result.source, CostSource::Litellm);
        // 1M × 2.5e-6 + 1M × 1e-5 = 2.5 + 10 = 12.5
        assert!((result.cost.unwrap() - 12.5).abs() < 1e-6);
    }

    #[test]
    fn litellm_lookup_strips_one_megabyte_window_label() {
        let json = r#"{
            "claude-opus-5": {
                "input_cost_per_token": 5e-6,
                "output_cost_per_token": 2.5e-5
            }
        }"#;
        let db = PricingDatabase::from_litellm_json(json).unwrap();
        let result = db.compute_cost(Some("claude-opus-5[1m]"), u(1_000_000, 1_000_000), None);
        assert_eq!(result.source, CostSource::Litellm);
        assert!((result.cost.unwrap() - 30.0).abs() < 1e-6);
    }

    #[test]
    fn litellm_applies_cache_token_costs_when_present() {
        let json = r#"{
            "claude-sonnet-4": {
                "input_cost_per_token": 3e-6,
                "output_cost_per_token": 1.5e-5,
                "cache_creation_input_token_cost": 3.75e-6,
                "cache_read_input_token_cost": 3e-7
            }
        }"#;
        let db = PricingDatabase::from_litellm_json(json).unwrap();
        let usage = TokenUsage {
            input: 0,
            output: 0,
            cache_creation: 1_000_000,
            cache_read: 1_000_000,
        };
        let result = db.compute_cost(Some("claude-sonnet-4"), usage, None);
        assert_eq!(result.source, CostSource::Litellm);
        // 1M × 3.75e-6 + 1M × 3e-7 = 3.75 + 0.3 = 4.05
        assert!((result.cost.unwrap() - 4.05).abs() < 1e-6);
    }

    #[test]
    fn litellm_lookup_tries_bedrock_prefix() {
        let json = r#"{
            "bedrock/anthropic.claude-sonnet-4-v1:0": {
                "input_cost_per_token": 5e-6,
                "output_cost_per_token": 2e-5
            }
        }"#;
        let db = PricingDatabase::from_litellm_json(json).unwrap();
        let result = db.compute_cost(
            Some("anthropic.claude-sonnet-4-v1:0"),
            u(1_000_000, 0),
            Some("bedrock"),
        );
        assert_eq!(result.source, CostSource::Litellm);
        assert!((result.cost.unwrap() - 5.0).abs() < 1e-6);
    }

    #[test]
    fn litellm_substring_match_as_last_resort() {
        // Caller passes just "sonnet-4" but LiteLLM key is more specific.
        let json = r#"{
            "claude-sonnet-4-20260101": {
                "input_cost_per_token": 3e-6,
                "output_cost_per_token": 1.5e-5
            }
        }"#;
        let db = PricingDatabase::from_litellm_json(json).unwrap();
        let result = db.compute_cost(Some("sonnet-4"), u(1_000_000, 0), None);
        assert_eq!(result.source, CostSource::Litellm);
        assert!((result.cost.unwrap() - 3.0).abs() < 1e-6);
    }

    #[test]
    fn invalid_litellm_entries_are_skipped_not_fatal() {
        // The real LiteLLM JSON contains a `sample_spec` entry with non-numeric
        // fields, and occasional entries where fields are strings like "unknown".
        let json = r#"{
            "sample_spec": { "input_cost_per_token": "unknown" },
            "gpt-4o": {
                "input_cost_per_token": 2.5e-6,
                "output_cost_per_token": 1e-5
            }
        }"#;
        let db = PricingDatabase::from_litellm_json(json).unwrap();
        assert!(!db.is_empty());
        let result = db.compute_cost(Some("gpt-4o"), u(1_000_000, 0), None);
        assert_eq!(result.source, CostSource::Litellm);
    }

    #[test]
    fn litellm_zero_cost_entry_falls_through_to_fallback() {
        let json = r#"{
            "claude-sonnet-4": {
                "input_cost_per_token": 0,
                "output_cost_per_token": 0
            }
        }"#;
        let db = PricingDatabase::from_litellm_json(json).unwrap();
        let result = db.compute_cost(Some("claude-sonnet-4"), u(1_000_000, 0), None);
        assert_eq!(result.source, CostSource::Fallback);
    }

    #[test]
    fn cost_source_serializes_lowercase() {
        let json = serde_json::to_string(&CostSource::Litellm).unwrap();
        assert_eq!(json, "\"litellm\"");
    }

    #[test]
    fn cache_savings_litellm_known_rates() {
        let json = r#"{
            "gpt-4o": {
                "input_cost_per_token": 2.5e-6,
                "output_cost_per_token": 1.0e-5,
                "cache_read_input_token_cost": 0.25e-6
            }
        }"#;
        let db = PricingDatabase::from_litellm_json(json).unwrap();
        // 1M reads × (2.5e-6 − 0.25e-6) = 2.25
        let result = db.compute_cache_savings(Some("gpt-4o"), 1_000_000, None);
        assert_eq!(result.source, CostSource::Litellm);
        assert!((result.cost.unwrap() - 2.25).abs() < 1e-9);
    }

    #[test]
    fn cache_savings_unknown_cache_read_rate_returns_none() {
        // Entry exists but carries no cache-read rate — must not fabricate
        // one (e.g. assuming reads are free).
        let json = r#"{
            "gpt-4o": {
                "input_cost_per_token": 2.5e-6,
                "output_cost_per_token": 1.0e-5
            }
        }"#;
        let db = PricingDatabase::from_litellm_json(json).unwrap();
        let result = db.compute_cache_savings(Some("gpt-4o"), 1_000_000, None);
        assert_eq!(result.source, CostSource::None);
        assert!(result.cost.is_none());
        assert!(result.reason.unwrap().contains("cache-read"));
    }

    #[test]
    fn cache_savings_no_entry_returns_none() {
        let db =
            PricingDatabase::from_litellm_json(r#"{"gpt-4o": {"input_cost_per_token": 2.5e-6}}"#)
                .unwrap();
        let result = db.compute_cache_savings(Some("mystery-model"), 1_000_000, None);
        assert_eq!(result.source, CostSource::None);
        assert!(result.cost.is_none());
    }

    #[test]
    fn cache_savings_fallback_claude_rates() {
        let db = PricingDatabase::empty();
        // Sonnet: input $3/M, cache read $0.30/M → 1M reads save $2.70
        let result = db.compute_cache_savings(Some("claude-sonnet-4"), 1_000_000, None);
        assert_eq!(result.source, CostSource::Fallback);
        assert!((result.cost.unwrap() - 2.70).abs() < 1e-9);
    }

    #[test]
    fn cache_savings_zero_reads_is_known_zero() {
        let db = PricingDatabase::empty();
        let result = db.compute_cache_savings(Some("claude-sonnet-4"), 0, None);
        assert_eq!(result.source, CostSource::Fallback);
        assert_eq!(result.cost.unwrap(), 0.0);
    }

    #[test]
    fn cache_savings_missing_model_returns_none() {
        let db = PricingDatabase::empty();
        let result = db.compute_cache_savings(None, 1_000_000, None);
        assert!(result.cost.is_none());
    }
}
