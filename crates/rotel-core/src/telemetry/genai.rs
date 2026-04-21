//! GenAI/LLM span detection and parsing.
//!
//! This module provides utilities for detecting and extracting information from
//! OpenTelemetry spans that follow the GenAI semantic conventions.
//!
//! See: https://opentelemetry.io/docs/specs/semconv/gen-ai/

use std::collections::HashMap;

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

        // Extract token counts
        info.input_tokens = attrs
            .get("gen_ai.usage.input_tokens")
            .and_then(|s| s.parse().ok());

        info.output_tokens = attrs
            .get("gen_ai.usage.output_tokens")
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

        // Extract cache token counts
        info.cache_creation_tokens = attrs
            .get("gen_ai.usage.cache_creation.input_tokens")
            .and_then(|v| v.parse().ok());
        info.cache_read_tokens = attrs
            .get("gen_ai.usage.cache_read.input_tokens")
            .and_then(|v| v.parse().ok());

        info
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

    // Fall back to comma-separated
    trimmed
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
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
}
