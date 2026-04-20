//! Logs command handlers

use crate::api::client::ApiClient;
use crate::api::models::LogEntry;
use crate::config::{Config, OutputFormat};
use crate::error::Result;
use crate::output::{json, pretty};

/// Handle the `logs list` command
pub async fn handle_list(
    client: &ApiClient,
    config: &Config,
    limit: Option<usize>,
    severity: Option<String>,
    since: Option<String>,
) -> Result<Vec<LogEntry>> {
    let mut params = vec![];

    if let Some(limit) = limit {
        params.push(("limit", limit.to_string()));
    }

    // Clone severity for filtering, move original to params
    let severity_filter = severity.clone();
    if let Some(severity) = severity {
        params.push(("severity", severity));
    }

    if let Some(since) = since {
        params.push(("since", since));
    }

    let logs_response = client.fetch_logs(params).await?;

    // Apply client-side severity filtering if needed
    let filtered_logs = filter_by_severity(logs_response.logs, severity_filter);

    // Output based on format
    match config.format {
        OutputFormat::Pretty => {
            pretty::print_logs_table(&filtered_logs, !config.no_color, config.no_header);
        },
        OutputFormat::Json => {
            json::print_logs_json(&filtered_logs)?;
        },
    }

    Ok(filtered_logs)
}

/// Handle the `logs search` command
pub async fn handle_search(
    client: &ApiClient,
    config: &Config,
    query: &str,
    limit: Option<usize>,
    severity: Option<String>,
) -> Result<Vec<LogEntry>> {
    let mut params = vec![];

    if let Some(limit) = limit {
        params.push(("limit", limit.to_string()));
    }

    if let Some(severity) = &severity {
        params.push(("severity", severity.clone()));
    }

    let logs_response = client.search_logs(query, params).await?;

    // Apply client-side severity filtering if needed
    let filtered_logs = filter_by_severity(logs_response.logs, severity);

    // Output based on format
    match config.format {
        OutputFormat::Pretty => {
            pretty::print_logs_table(&filtered_logs, !config.no_color, config.no_header);
        },
        OutputFormat::Json => {
            json::print_logs_json(&filtered_logs)?;
        },
    }

    Ok(filtered_logs)
}

/// Handle the `logs show` command
pub async fn handle_show(client: &ApiClient, config: &Config, id: &str) -> Result<LogEntry> {
    let timestamp: i64 = id
        .parse()
        .map_err(|_| crate::error::Error::ApiError(format!("Invalid timestamp: {}", id)))?;
    let log = client.fetch_log_by_id(timestamp).await?;

    // Output based on format
    match config.format {
        OutputFormat::Pretty => {
            pretty::print_log_details(&log, !config.no_color);
        },
        OutputFormat::Json => {
            json::print_log_json(&log)?;
        },
    }

    Ok(log)
}

/// Handle the `logs export` command
#[allow(clippy::too_many_arguments)]
pub async fn handle_export(
    client: &ApiClient,
    _config: &Config,
    format: &str,
    severity: Option<String>,
    since: Option<String>,
    output: Option<String>,
) -> Result<()> {
    let mut params = vec![("format", format.to_string())];

    if let Some(severity) = severity {
        params.push(("severity", severity));
    }

    if let Some(since) = since {
        params.push(("since", since));
    }

    let data = client.export_logs(params).await?;

    // Write to file or stdout
    if let Some(output_path) = output {
        std::fs::write(&output_path, &data)?;

        // Count entries for progress message
        let count = if format == "json" {
            // Count JSON array elements
            data.matches("\"id\"").count()
        } else {
            // Count CSV lines (minus header)
            data.lines().count().saturating_sub(1)
        };

        eprintln!("✓ Exported {} log entries to {}", count, output_path);
    } else {
        print!("{}", data);
    }

    Ok(())
}

/// Filter logs by severity level
///
/// Severity hierarchy: ERROR > WARN > INFO > DEBUG > TRACE
/// When filtering by a level, include that level and all higher levels
fn filter_by_severity(logs: Vec<LogEntry>, min_severity: Option<String>) -> Vec<LogEntry> {
    let min_severity = match min_severity {
        Some(s) => s.to_uppercase(),
        None => return logs, // No filtering
    };

    let severity_rank = |s: &str| -> i32 {
        match s.to_uppercase().as_str() {
            "ERROR" => 4,
            "WARN" => 3,
            "INFO" => 2,
            "DEBUG" => 1,
            "TRACE" => 0,
            _ => -1, // Unknown severity, include by default
        }
    };

    let min_rank = severity_rank(&min_severity);

    logs.into_iter()
        .filter(|log| {
            let log_rank = severity_rank(&log.severity);
            log_rank >= min_rank || log_rank == -1
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    // T019: Unit test for severity filtering logic
    #[test]
    fn test_filter_by_severity_no_filter() {
        let logs = create_test_logs();
        let filtered = filter_by_severity(logs.clone(), None);
        assert_eq!(filtered.len(), logs.len());
    }

    #[test]
    fn test_filter_by_severity_error_only() {
        let logs = create_test_logs();
        let filtered = filter_by_severity(logs, Some("ERROR".to_string()));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].severity, "ERROR");
    }

    #[test]
    fn test_filter_by_severity_warn_and_above() {
        let logs = create_test_logs();
        let filtered = filter_by_severity(logs, Some("WARN".to_string()));
        assert_eq!(filtered.len(), 2); // ERROR and WARN
        assert!(filtered.iter().any(|l| l.severity == "ERROR"));
        assert!(filtered.iter().any(|l| l.severity == "WARN"));
    }

    #[test]
    fn test_filter_by_severity_info_and_above() {
        let logs = create_test_logs();
        let filtered = filter_by_severity(logs, Some("INFO".to_string()));
        assert_eq!(filtered.len(), 3); // ERROR, WARN, INFO
    }

    #[test]
    fn test_filter_by_severity_debug_and_above() {
        let logs = create_test_logs();
        let filtered = filter_by_severity(logs, Some("DEBUG".to_string()));
        assert_eq!(filtered.len(), 4); // ERROR, WARN, INFO, DEBUG
    }

    #[test]
    fn test_filter_by_severity_trace_includes_all() {
        let logs = create_test_logs();
        let filtered = filter_by_severity(logs.clone(), Some("TRACE".to_string()));
        assert_eq!(filtered.len(), logs.len()); // All logs
    }

    #[test]
    fn test_filter_by_severity_case_insensitive() {
        let logs = create_test_logs();
        let filtered = filter_by_severity(logs, Some("error".to_string()));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].severity, "ERROR");
    }

    #[test]
    fn test_filter_by_severity_unknown_level() {
        let mut logs = create_test_logs();
        logs.push(LogEntry {
            timestamp: 1705315800000000000,
            severity: "CUSTOM".to_string(),
            severity_text: Some("CUSTOM".to_string()),
            body: "Custom severity".to_string(),
            attributes: HashMap::new(),
            resource: None,
            trace_id: None,
            span_id: None,
        });

        let filtered = filter_by_severity(logs.clone(), Some("INFO".to_string()));
        // Unknown severity should be included
        assert!(filtered.iter().any(|l| l.severity == "CUSTOM"));
    }

    fn create_test_logs() -> Vec<LogEntry> {
        vec![
            LogEntry {
                timestamp: 1705315800000000000,
                severity: "ERROR".to_string(),
                severity_text: Some("ERROR".to_string()),
                body: "Error message".to_string(),
                attributes: HashMap::new(),
                resource: None,
                trace_id: None,
                span_id: None,
            },
            LogEntry {
                timestamp: 1705315860000000000,
                severity: "WARN".to_string(),
                severity_text: Some("WARN".to_string()),
                body: "Warning message".to_string(),
                attributes: HashMap::new(),
                resource: None,
                trace_id: None,
                span_id: None,
            },
            LogEntry {
                timestamp: 1705315920000000000,
                severity: "INFO".to_string(),
                severity_text: Some("INFO".to_string()),
                body: "Info message".to_string(),
                attributes: HashMap::new(),
                resource: None,
                trace_id: None,
                span_id: None,
            },
            LogEntry {
                timestamp: 1705315980000000000,
                severity: "DEBUG".to_string(),
                severity_text: Some("DEBUG".to_string()),
                body: "Debug message".to_string(),
                attributes: HashMap::new(),
                resource: None,
                trace_id: None,
                span_id: None,
            },
            LogEntry {
                timestamp: 1705316040000000000,
                severity: "TRACE".to_string(),
                severity_text: Some("TRACE".to_string()),
                body: "Trace message".to_string(),
                attributes: HashMap::new(),
                resource: None,
                trace_id: None,
                span_id: None,
            },
        ]
    }
}
