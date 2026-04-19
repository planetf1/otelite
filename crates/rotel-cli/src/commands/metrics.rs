//! Metrics command handlers

use crate::api::client::ApiClient;
use crate::api::models::MetricResponse;
use crate::config::Config;
use crate::error::Result;
use crate::output::{json, pretty};

/// Handle metrics list command
#[allow(clippy::too_many_arguments)]
pub async fn handle_list(
    client: &ApiClient,
    config: &Config,
    limit: Option<u32>,
    name: Option<String>,
    labels: Vec<String>,
    since: Option<String>,
) -> Result<()> {
    let mut params = Vec::new();

    if let Some(limit) = limit {
        params.push(("limit", limit.to_string()));
    }

    if let Some(name) = name {
        params.push(("name", name));
    }

    if let Some(since) = since {
        params.push(("since", since));
    }

    // Add label filters as query parameters
    for label in labels {
        params.push(("label", label));
    }

    let metrics = client.fetch_metrics(params).await?;

    match config.format {
        crate::config::OutputFormat::Pretty => {
            pretty::print_metrics_table(&metrics, config.no_color, config.no_header);
        },
        crate::config::OutputFormat::Json => {
            json::print_metrics_json(&metrics)?;
        },
    }

    Ok(())
}

/// Handle metrics show command
pub async fn handle_show(
    client: &ApiClient,
    config: &Config,
    name: &str,
    labels: Vec<String>,
    since: Option<String>,
) -> Result<()> {
    let mut params = Vec::new();

    if let Some(since) = since {
        params.push(("since", since));
    }

    // Add label filters as query parameters
    for label in labels {
        params.push(("label", label));
    }

    let metrics = client.fetch_metric_by_name(name, params).await?;

    // Display all matching metrics (may be multiple with different label combinations)
    match config.format {
        crate::config::OutputFormat::Pretty => {
            if metrics.is_empty() {
                println!("No metrics found with name '{}'", name);
            } else if metrics.len() == 1 {
                pretty::print_metric_details(&metrics[0], config.no_color);
            } else {
                // Multiple metrics with same name but different labels
                pretty::print_metrics_table(&metrics, config.no_color, config.no_header);
            }
        },
        crate::config::OutputFormat::Json => {
            if metrics.len() == 1 {
                json::print_metric_json(&metrics[0])?;
            } else {
                json::print_metrics_json(&metrics)?;
            }
        },
    }

    Ok(())
}

/// Handle the `metrics export` command
#[allow(clippy::too_many_arguments)]
pub async fn handle_export(
    client: &ApiClient,
    _config: &Config,
    format: &str,
    name: Option<String>,
    since: Option<String>,
    output: Option<String>,
) -> Result<()> {
    let mut params = vec![("format", format.to_string())];

    if let Some(name) = name {
        params.push(("name", name));
    }

    if let Some(since) = since {
        params.push(("since", since));
    }

    let data = client.export_metrics(params).await?;

    // Write to file or stdout
    if let Some(output_path) = output {
        std::fs::write(&output_path, &data)?;

        // Count entries for progress message
        let count = data.matches("\"name\"").count();

        eprintln!("✓ Exported {} metrics to {}", count, output_path);
    } else {
        print!("{}", data);
    }

    Ok(())
}

/// Filter metrics by label key-value pairs (client-side filtering)
/// Labels should be in format "key=value"
pub fn filter_by_labels(
    metrics: Vec<MetricResponse>,
    label_filters: &[String],
) -> Vec<MetricResponse> {
    if label_filters.is_empty() {
        return metrics;
    }

    // Parse label filters into key-value pairs
    let filters: Vec<(&str, &str)> = label_filters
        .iter()
        .filter_map(|filter| {
            let parts: Vec<&str> = filter.splitn(2, '=').collect();
            if parts.len() == 2 {
                Some((parts[0], parts[1]))
            } else {
                None
            }
        })
        .collect();

    metrics
        .into_iter()
        .filter(|metric| {
            // Metric must match ALL label filters
            filters.iter().all(|(key, value)| {
                metric
                    .attributes
                    .get(*key)
                    .map(|v| v == value)
                    .unwrap_or(false)
            })
        })
        .collect()
}

/// Filter metrics by name pattern (client-side filtering)
pub fn filter_by_name(metrics: Vec<MetricResponse>, name_pattern: &str) -> Vec<MetricResponse> {
    metrics
        .into_iter()
        .filter(|metric| metric.name.contains(name_pattern))
        .collect()
}

/// Filter metrics by type (client-side filtering)
pub fn filter_by_type(metrics: Vec<MetricResponse>, metric_type: &str) -> Vec<MetricResponse> {
    metrics
        .into_iter()
        .filter(|metric| metric.metric_type.eq_ignore_ascii_case(metric_type))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::{MetricResponse, MetricValue};
    use std::collections::HashMap;

    // T065: Unit tests for label filtering logic
    #[test]
    fn test_filter_by_labels_single_label() {
        let metrics = vec![
            MetricResponse {
                name: "http_requests_total".to_string(),
                description: None,
                unit: None,
                metric_type: "counter".to_string(),
                value: MetricValue::Counter(100),
                timestamp: 1234567890000000000,
                attributes: HashMap::from([("method".to_string(), "GET".to_string())]),
                resource: None,
            },
            MetricResponse {
                name: "http_requests_total".to_string(),
                description: None,
                unit: None,
                metric_type: "counter".to_string(),
                value: MetricValue::Counter(50),
                timestamp: 1234567890000000000,
                attributes: HashMap::from([("method".to_string(), "POST".to_string())]),
                resource: None,
            },
            MetricResponse {
                name: "http_requests_total".to_string(),
                description: None,
                unit: None,
                metric_type: "counter".to_string(),
                value: MetricValue::Counter(25),
                timestamp: 1234567890000000000,
                attributes: HashMap::from([("method".to_string(), "DELETE".to_string())]),
                resource: None,
            },
        ];

        let filters = vec!["method=GET".to_string()];
        let filtered = filter_by_labels(metrics, &filters);
        assert_eq!(filtered.len(), 1);
        if let MetricValue::Counter(val) = filtered[0].value {
            assert_eq!(val, 100);
        }
    }

    #[test]
    fn test_filter_by_labels_multiple_labels() {
        let metrics = vec![
            MetricResponse {
                name: "http_requests_total".to_string(),
                description: None,
                unit: None,
                metric_type: "counter".to_string(),
                value: MetricValue::Counter(100),
                timestamp: 1234567890000000000,
                attributes: HashMap::from([
                    ("method".to_string(), "GET".to_string()),
                    ("status".to_string(), "200".to_string()),
                ]),
                resource: None,
            },
            MetricResponse {
                name: "http_requests_total".to_string(),
                description: None,
                unit: None,
                metric_type: "counter".to_string(),
                value: MetricValue::Counter(50),
                timestamp: 1234567890000000000,
                attributes: HashMap::from([
                    ("method".to_string(), "GET".to_string()),
                    ("status".to_string(), "404".to_string()),
                ]),
                resource: None,
            },
            MetricResponse {
                name: "http_requests_total".to_string(),
                description: None,
                unit: None,
                metric_type: "counter".to_string(),
                value: MetricValue::Counter(25),
                timestamp: 1234567890000000000,
                attributes: HashMap::from([
                    ("method".to_string(), "POST".to_string()),
                    ("status".to_string(), "200".to_string()),
                ]),
                resource: None,
            },
        ];

        // Filter for GET requests with 200 status
        let filters = vec!["method=GET".to_string(), "status=200".to_string()];
        let filtered = filter_by_labels(metrics, &filters);
        assert_eq!(filtered.len(), 1);
        if let MetricValue::Counter(val) = filtered[0].value {
            assert_eq!(val, 100);
        }
    }

    #[test]
    fn test_filter_by_labels_no_match() {
        let metrics = vec![MetricResponse {
            name: "http_requests_total".to_string(),
            description: None,
            unit: None,
            metric_type: "counter".to_string(),
            value: MetricValue::Counter(100),
            timestamp: 1234567890000000000,
            attributes: HashMap::from([("method".to_string(), "GET".to_string())]),
            resource: None,
        }];

        let filters = vec!["method=POST".to_string()];
        let filtered = filter_by_labels(metrics, &filters);
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn test_filter_by_labels_empty_filters() {
        let metrics = vec![
            MetricResponse {
                name: "metric1".to_string(),
                description: None,
                unit: None,
                metric_type: "counter".to_string(),
                value: MetricValue::Counter(100),
                timestamp: 1234567890000000000,
                attributes: HashMap::new(),
                resource: None,
            },
            MetricResponse {
                name: "metric2".to_string(),
                description: None,
                unit: None,
                metric_type: "gauge".to_string(),
                value: MetricValue::Gauge(50.0),
                timestamp: 1234567890000000000,
                attributes: HashMap::new(),
                resource: None,
            },
        ];

        let filters: Vec<String> = vec![];
        let filtered = filter_by_labels(metrics.clone(), &filters);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filter_by_labels_invalid_format() {
        let metrics = vec![MetricResponse {
            name: "http_requests_total".to_string(),
            description: None,
            unit: None,
            metric_type: "counter".to_string(),
            value: MetricValue::Counter(100),
            timestamp: 1234567890000000000,
            attributes: HashMap::from([("method".to_string(), "GET".to_string())]),
            resource: None,
        }];

        // Invalid filter format (no '=')
        let filters = vec!["method".to_string()];
        let filtered = filter_by_labels(metrics.clone(), &filters);
        // Should return all metrics since filter is invalid
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_filter_by_labels_partial_match() {
        let metrics = vec![MetricResponse {
            name: "http_requests_total".to_string(),
            description: None,
            unit: None,
            metric_type: "counter".to_string(),
            value: MetricValue::Counter(100),
            timestamp: 1234567890000000000,
            attributes: HashMap::from([
                ("method".to_string(), "GET".to_string()),
                ("status".to_string(), "200".to_string()),
            ]),
            resource: None,
        }];

        // Filter requires both labels, but metric only matches one
        let filters = vec!["method=GET".to_string(), "endpoint=/api".to_string()];
        let filtered = filter_by_labels(metrics, &filters);
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn test_filter_by_name() {
        let metrics = vec![
            MetricResponse {
                name: "http_requests_total".to_string(),
                description: None,
                unit: None,
                metric_type: "counter".to_string(),
                value: MetricValue::Counter(100),
                timestamp: 1234567890000000000,
                attributes: HashMap::new(),
                resource: None,
            },
            MetricResponse {
                name: "http_response_time_ms".to_string(),
                description: None,
                unit: None,
                metric_type: "histogram".to_string(),
                value: MetricValue::Histogram {
                    count: 10,
                    sum: 1500.0,
                    buckets: vec![],
                },
                timestamp: 1234567890000000000,
                attributes: HashMap::new(),
                resource: None,
            },
            MetricResponse {
                name: "cpu_usage_percent".to_string(),
                description: None,
                unit: None,
                metric_type: "gauge".to_string(),
                value: MetricValue::Gauge(45.0),
                timestamp: 1234567890000000000,
                attributes: HashMap::new(),
                resource: None,
            },
        ];

        // Filter for metrics with "http" in name
        let filtered = filter_by_name(metrics.clone(), "http");
        assert_eq!(filtered.len(), 2);

        // Filter for metrics with "cpu" in name
        let filtered = filter_by_name(metrics.clone(), "cpu");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "cpu_usage_percent");

        // Filter for metrics with "memory" in name (no match)
        let filtered = filter_by_name(metrics, "memory");
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn test_filter_by_type() {
        let metrics = vec![
            MetricResponse {
                name: "requests_total".to_string(),
                description: None,
                unit: None,
                metric_type: "counter".to_string(),
                value: MetricValue::Counter(100),
                timestamp: 1234567890000000000,
                attributes: HashMap::new(),
                resource: None,
            },
            MetricResponse {
                name: "cpu_usage".to_string(),
                description: None,
                unit: None,
                metric_type: "gauge".to_string(),
                value: MetricValue::Gauge(45.0),
                timestamp: 1234567890000000000,
                attributes: HashMap::new(),
                resource: None,
            },
            MetricResponse {
                name: "response_time".to_string(),
                description: None,
                unit: None,
                metric_type: "histogram".to_string(),
                value: MetricValue::Histogram {
                    count: 10,
                    sum: 1500.0,
                    buckets: vec![],
                },
                timestamp: 1234567890000000000,
                attributes: HashMap::new(),
                resource: None,
            },
        ];

        // Filter for counters
        let filtered = filter_by_type(metrics.clone(), "counter");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "requests_total");

        // Filter for gauges (case insensitive)
        let filtered = filter_by_type(metrics.clone(), "GAUGE");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "cpu_usage");

        // Filter for histograms
        let filtered = filter_by_type(metrics.clone(), "histogram");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "response_time");

        // Filter for summary (no match)
        let filtered = filter_by_type(metrics, "summary");
        assert_eq!(filtered.len(), 0);
    }
}

// Made with Bob
