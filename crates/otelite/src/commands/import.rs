use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use otelite_receiver::{conversion, protocol};
use otelite_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};

use crate::error::{Error, Result};

enum SignalType {
    Metrics,
    Logs,
    Traces,
}

fn detect_signal_type(line: &str) -> Result<SignalType> {
    let v: serde_json::Value = serde_json::from_str(line)?;
    if v.get("resourceMetrics").is_some() {
        Ok(SignalType::Metrics)
    } else if v.get("resourceLogs").is_some() {
        Ok(SignalType::Logs)
    } else if v.get("resourceSpans").is_some() {
        Ok(SignalType::Traces)
    } else {
        Err(Error::InvalidArgument(
            "Cannot detect signal type from JSON keys (expected resourceMetrics, resourceLogs, or resourceSpans). Use --signal-type to specify.".to_string(),
        ))
    }
}

fn parse_signal_type(s: &str) -> Result<SignalType> {
    match s {
        "metrics" => Ok(SignalType::Metrics),
        "logs" => Ok(SignalType::Logs),
        "traces" => Ok(SignalType::Traces),
        other => Err(Error::InvalidArgument(format!(
            "Unknown signal type '{}'. Use metrics, logs, or traces.",
            other
        ))),
    }
}

pub async fn handle_import(
    file: &str,
    signal_type: Option<&str>,
    storage_path: Option<&str>,
) -> Result<()> {
    // Validate explicit signal type before touching the filesystem.
    if let Some(s) = signal_type {
        parse_signal_type(s)?;
    }

    let reader: Box<dyn BufRead> = if file == "-" {
        Box::new(BufReader::new(std::io::stdin()))
    } else {
        let f = std::fs::File::open(file).map_err(|e| {
            Error::IoError(std::io::Error::new(
                e.kind(),
                format!("Cannot open '{}': {}", file, e),
            ))
        })?;
        Box::new(BufReader::new(f))
    };

    let data_dir: PathBuf = match storage_path {
        Some(p) => PathBuf::from(p),
        None => StorageConfig::default().data_dir,
    };

    let config = StorageConfig::default()
        .with_data_dir(data_dir)
        .with_auto_purge(false);

    let mut storage = SqliteBackend::new(config);
    storage.initialize().await?;

    let mut lines = reader.lines();
    let mut imported: usize = 0;
    let mut skipped: usize = 0;
    let mut errors: usize = 0;
    let mut line_number: usize = 0;

    // Read until we find the first non-empty line (needed for auto-detection).
    let mut first_line: Option<String> = None;
    for raw in lines.by_ref() {
        line_number += 1;
        let line = raw?;
        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            skipped += 1;
            continue;
        }
        first_line = Some(trimmed);
        break;
    }

    let first = match first_line {
        None => {
            eprintln!("Import complete: 0 records (file is empty)");
            storage.close().await?;
            return Ok(());
        },
        Some(l) => l,
    };

    // safe: explicit signal type was already validated above
    let sig = match signal_type {
        Some(s) => parse_signal_type(s)?,
        None => detect_signal_type(&first)?,
    };

    // Chain the first line back in so it gets processed with the rest.
    let all_lines =
        std::iter::once(Ok(first)).chain(lines.by_ref().map(|r| r.map(|l| l.trim().to_string())));

    for raw in all_lines {
        let line = raw?;
        // Always advance line_number so error messages are accurate.
        line_number += 1;
        if line.is_empty() {
            skipped += 1;
            continue;
        }
        let data = line.as_bytes();
        match sig {
            SignalType::Metrics => match protocol::parse_metrics_json(data) {
                Err(e) => {
                    eprintln!("Warning: line {}: {}", line_number, e);
                    errors += 1;
                },
                Ok(req) => {
                    for m in &conversion::convert_metrics(req) {
                        if let Err(e) = storage.write_metric(m).await {
                            eprintln!("Warning: line {}: write failed: {}", line_number, e);
                            errors += 1;
                        } else {
                            imported += 1;
                        }
                    }
                },
            },
            SignalType::Logs => match protocol::parse_logs_json(data) {
                Err(e) => {
                    eprintln!("Warning: line {}: {}", line_number, e);
                    errors += 1;
                },
                Ok(req) => {
                    for log in &conversion::convert_logs(req) {
                        if let Err(e) = storage.write_log(log).await {
                            eprintln!("Warning: line {}: write failed: {}", line_number, e);
                            errors += 1;
                        } else {
                            imported += 1;
                        }
                    }
                },
            },
            SignalType::Traces => match protocol::parse_traces_json(data) {
                Err(e) => {
                    eprintln!("Warning: line {}: {}", line_number, e);
                    errors += 1;
                },
                Ok(req) => {
                    for trace in &conversion::convert_traces(req) {
                        for span in &trace.spans {
                            if let Err(e) = storage.write_span(span).await {
                                eprintln!("Warning: line {}: write failed: {}", line_number, e);
                                errors += 1;
                            } else {
                                imported += 1;
                            }
                        }
                    }
                },
            },
        }
    }

    storage.close().await?;

    eprintln!(
        "Import complete: {} records imported ({} errors, {} empty lines skipped)",
        imported, errors, skipped
    );

    if imported == 0 && errors > 0 {
        return Err(Error::ApiError("All lines failed to import".to_string()));
    }

    Ok(())
}
