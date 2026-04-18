//! Traces command handlers

use crate::api::client::ApiClient;
use crate::api::models::TraceEntry;
use crate::config::Config;
use crate::error::Result;
use crate::output::{json, pretty};

/// Handle traces list command
pub async fn handle_list(
    client: &ApiClient,
    config: &Config,
    limit: Option<u32>,
    min_duration: Option<u64>,
    status: Option<String>,
) -> Result<()> {
    let mut params = Vec::new();

    if let Some(limit) = limit {
        params.push(("limit", limit.to_string()));
    }

    if let Some(min_duration) = min_duration {
        params.push(("min_duration", min_duration.to_string()));
    }

    if let Some(status) = status {
        params.push(("status", status));
    }

    let traces_response = client.fetch_traces(params).await?;

    match config.format {
        crate::config::OutputFormat::Pretty => {
            pretty::print_traces_table(&traces_response.traces, config.no_color, config.no_header);
        },
        crate::config::OutputFormat::Json => {
            json::print_traces_json(&traces_response.traces)?;
        },
    }

    Ok(())
}

/// Handle traces show command
pub async fn handle_show(client: &ApiClient, config: &Config, id: &str) -> Result<()> {
    let trace = client.fetch_trace_by_id(id).await?;

    match config.format {
        crate::config::OutputFormat::Pretty => {
            pretty::print_trace_tree(&trace, config.no_color);
        },
        crate::config::OutputFormat::Json => {
            json::print_trace_json(&trace)?;
        },
    }

    Ok(())
}

/// Handle the `traces export` command
#[allow(clippy::too_many_arguments)]
pub async fn handle_export(
    client: &ApiClient,
    _config: &Config,
    format: &str,
    status: Option<String>,
    min_duration: Option<u64>,
    since: Option<String>,
    output: Option<String>,
) -> Result<()> {
    let mut params = vec![("format", format.to_string())];

    if let Some(status) = status {
        params.push(("status", status));
    }

    if let Some(min_duration) = min_duration {
        params.push(("min_duration", min_duration.to_string()));
    }

    if let Some(since) = since {
        params.push(("since", since));
    }

    let data = client.export_traces(params).await?;

    // Write to file or stdout
    if let Some(output_path) = output {
        std::fs::write(&output_path, &data)?;

        // Count entries for progress message
        let count = data.matches("\"id\"").count();

        eprintln!("✓ Exported {} traces to {}", count, output_path);
    } else {
        print!("{}", data);
    }

    Ok(())
}

/// Filter traces by minimum duration (client-side filtering)
pub fn filter_by_duration(traces: Vec<TraceEntry>, min_duration_ms: u64) -> Vec<TraceEntry> {
    traces
        .into_iter()
        .filter(|trace| (trace.duration / 1_000_000) >= min_duration_ms as i64)
        .collect()
}

/// Filter traces by status (client-side filtering)
pub fn filter_by_status(traces: Vec<TraceEntry>, status: &str) -> Vec<TraceEntry> {
    traces
        .into_iter()
        .filter(|trace| {
            let trace_status = if trace.has_errors { "ERROR" } else { "OK" };
            trace_status.eq_ignore_ascii_case(status)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // T043: Unit test for duration filtering logic
    #[test]
    fn test_filter_by_duration() {
        let traces = vec![
            Trace {
                id: "trace-001".to_string(),
                root_span: "fast-request".to_string(),
                duration_ms: 100,
                status: "OK".to_string(),
                spans: vec![],
            },
            Trace {
                id: "trace-002".to_string(),
                root_span: "slow-request".to_string(),
                duration_ms: 2000,
                status: "OK".to_string(),
                spans: vec![],
            },
            Trace {
                id: "trace-003".to_string(),
                root_span: "medium-request".to_string(),
                duration_ms: 500,
                status: "OK".to_string(),
                spans: vec![],
            },
        ];

        // Filter for traces >= 500ms
        let filtered = filter_by_duration(traces.clone(), 500);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].id, "trace-002");
        assert_eq!(filtered[1].id, "trace-003");

        // Filter for traces >= 1000ms
        let filtered = filter_by_duration(traces.clone(), 1000);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "trace-002");

        // Filter for traces >= 0ms (all traces)
        let filtered = filter_by_duration(traces.clone(), 0);
        assert_eq!(filtered.len(), 3);

        // Filter for traces >= 10000ms (no traces)
        let filtered = filter_by_duration(traces, 10000);
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn test_filter_by_duration_empty() {
        let traces: Vec<Trace> = vec![];
        let filtered = filter_by_duration(traces, 1000);
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn test_filter_by_duration_exact_match() {
        let traces = vec![Trace {
            id: "trace-001".to_string(),
            root_span: "request".to_string(),
            duration_ms: 1000,
            status: "OK".to_string(),
            spans: vec![],
        }];

        // Exact match should be included
        let filtered = filter_by_duration(traces.clone(), 1000);
        assert_eq!(filtered.len(), 1);

        // Just above threshold should be excluded
        let filtered = filter_by_duration(traces, 1001);
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn test_filter_by_status() {
        let traces = vec![
            Trace {
                id: "trace-001".to_string(),
                root_span: "success-request".to_string(),
                duration_ms: 100,
                status: "OK".to_string(),
                spans: vec![],
            },
            Trace {
                id: "trace-002".to_string(),
                root_span: "error-request".to_string(),
                duration_ms: 200,
                status: "ERROR".to_string(),
                spans: vec![],
            },
            Trace {
                id: "trace-003".to_string(),
                root_span: "another-success".to_string(),
                duration_ms: 150,
                status: "OK".to_string(),
                spans: vec![],
            },
        ];

        // Filter for OK status
        let filtered = filter_by_status(traces.clone(), "OK");
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].id, "trace-001");
        assert_eq!(filtered[1].id, "trace-003");

        // Filter for ERROR status
        let filtered = filter_by_status(traces.clone(), "ERROR");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "trace-002");

        // Case insensitive
        let filtered = filter_by_status(traces.clone(), "ok");
        assert_eq!(filtered.len(), 2);

        // Non-existent status
        let filtered = filter_by_status(traces, "UNKNOWN");
        assert_eq!(filtered.len(), 0);
    }
}

// Made with Bob
