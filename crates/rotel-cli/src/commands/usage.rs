//! Token usage command for GenAI/LLM spans

use crate::error::{Error, Result};
use clap::Args;
use comfy_table::{presets::UTF8_FULL, Cell, Color, ContentArrangement, Table};
use rotel_storage::StorageBackend;
use std::sync::Arc;

/// Display token usage statistics for GenAI/LLM spans
#[derive(Debug, Args)]
pub struct UsageCommand {
    /// Time range to query (e.g., "1h", "24h", "7d", "30d")
    #[arg(long, default_value = "24h")]
    pub since: String,

    /// Filter by model name (e.g., "gpt-4", "claude-sonnet-4")
    #[arg(long)]
    pub model: Option<String>,

    /// Filter by system/provider (e.g., "openai", "anthropic")
    #[arg(long)]
    pub system: Option<String>,

    /// Show detailed breakdown by model
    #[arg(long)]
    pub by_model: bool,

    /// Show detailed breakdown by system
    #[arg(long)]
    pub by_system: bool,
}

impl UsageCommand {
    /// Execute the usage command
    pub async fn execute(&self, storage: Arc<dyn StorageBackend>) -> Result<()> {
        // Parse time range
        let (start_time, _end_time) = parse_time_range(&self.since)?;

        // Query token usage from storage
        let (summary, by_model, by_system) = storage
            .query_token_usage(Some(start_time), None)
            .await
            .map_err(|e| Error::ApiError(format!("Failed to query token usage: {}", e)))?;

        // Filter results if requested
        let by_model: Vec<rotel_core::api::ModelUsage> = if let Some(ref model_filter) = self.model
        {
            by_model
                .into_iter()
                .filter(|m: &rotel_core::api::ModelUsage| m.model.contains(model_filter))
                .collect()
        } else {
            by_model
        };

        let by_system: Vec<rotel_core::api::SystemUsage> =
            if let Some(ref system_filter) = self.system {
                by_system
                    .into_iter()
                    .filter(|s: &rotel_core::api::SystemUsage| s.system.contains(system_filter))
                    .collect()
            } else {
                by_system
            };

        // Display results
        println!("\n{}", format_header(&self.since));
        println!();

        // Summary table
        display_summary(&summary);
        println!();

        // Detailed breakdowns if requested or if filtering
        if self.by_model || self.model.is_some() || (!self.by_system && self.system.is_none()) {
            display_by_model(&by_model);
            println!();
        }

        if self.by_system || self.system.is_some() {
            display_by_system(&by_system);
            println!();
        }

        Ok(())
    }
}

/// Parse time range string into start and end timestamps (nanoseconds)
fn parse_time_range(range: &str) -> Result<(i64, i64)> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| Error::ApiError(format!("Failed to get current time: {}", e)))?
        .as_nanos() as i64;

    // Parse duration from string (e.g., "24h", "7d", "30d")
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

/// Format header with time range
fn format_header(range: &str) -> String {
    format!("Token Usage Summary (Last {})", range)
}

/// Display summary table
fn display_summary(summary: &rotel_core::api::TokenUsageSummary) {
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

/// Display breakdown by model
fn display_by_model(models: &[rotel_core::api::ModelUsage]) {
    if models.is_empty() {
        println!("No model data available");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    table.set_header(vec![
        Cell::new("Model").fg(Color::Cyan),
        Cell::new("Input Tokens").fg(Color::Cyan),
        Cell::new("Output Tokens").fg(Color::Cyan),
        Cell::new("Total Tokens").fg(Color::Cyan),
        Cell::new("Requests").fg(Color::Cyan),
    ]);

    for model in models {
        let total = model.input_tokens + model.output_tokens;
        table.add_row(vec![
            &model.model,
            &format_number(model.input_tokens),
            &format_number(model.output_tokens),
            &format_number(total),
            &model.requests.to_string(),
        ]);
    }

    println!("Breakdown by Model:");
    println!("{}", table);
}

/// Display breakdown by system
fn display_by_system(systems: &[rotel_core::api::SystemUsage]) {
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
        Cell::new("Input Tokens").fg(Color::Cyan),
        Cell::new("Output Tokens").fg(Color::Cyan),
        Cell::new("Total Tokens").fg(Color::Cyan),
        Cell::new("Requests").fg(Color::Cyan),
    ]);

    for system in systems {
        let total = system.input_tokens + system.output_tokens;
        let display_name = rotel_core::telemetry::GenAiSpanInfo::format_system_name(&system.system);
        table.add_row(vec![
            &display_name,
            &format_number(system.input_tokens),
            &format_number(system.output_tokens),
            &format_number(total),
            &system.requests.to_string(),
        ]);
    }

    println!("Breakdown by System:");
    println!("{}", table);
}

/// Format number with thousands separators
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
    fn test_format_number() {
        assert_eq!(format_number(1234), "1,234");
        assert_eq!(format_number(1234567), "1,234,567");
        assert_eq!(format_number(123), "123");
    }
}
