//! Metric-specific models

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use utoipa::{IntoParams, ToSchema};
use validator::Validate;

use super::response::ResourceAttributes;

/// Metric data point
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MetricDataPoint {
    /// Metric identifier
    pub id: String,

    /// Metric name
    pub name: String,

    /// Metric type (GAUGE, COUNTER, HISTOGRAM, SUMMARY)
    pub metric_type: String,

    /// Timestamp (Unix timestamp in nanoseconds)
    pub timestamp: i64,

    /// Metric value
    pub value: MetricValue,

    /// Resource attributes (service info)
    pub resource: ResourceAttributes,

    /// Metric attributes (labels/tags)
    pub attributes: HashMap<String, String>,

    /// Unit of measurement (e.g., "bytes", "ms", "requests")
    pub unit: Option<String>,

    /// Description of the metric
    pub description: Option<String>,
}

/// Metric value variants
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", content = "data")]
pub enum MetricValue {
    /// Single numeric value (for GAUGE and COUNTER)
    Number(f64),

    /// Histogram data with buckets
    Histogram(HistogramData),

    /// Summary data with quantiles
    Summary(SummaryData),
}

/// Histogram metric data
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HistogramData {
    /// Total count of observations
    pub count: u64,

    /// Sum of all observed values
    pub sum: f64,

    /// Histogram buckets
    pub buckets: Vec<HistogramBucket>,
}

/// Histogram bucket
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HistogramBucket {
    /// Upper bound of the bucket (inclusive)
    pub upper_bound: f64,

    /// Count of observations in this bucket
    pub count: u64,
}

/// Summary metric data
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SummaryData {
    /// Total count of observations
    pub count: u64,

    /// Sum of all observed values
    pub sum: f64,

    /// Quantile values
    pub quantiles: Vec<Quantile>,
}

/// Quantile value
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Quantile {
    /// Quantile (0.0 to 1.0, e.g., 0.5 for median, 0.95 for 95th percentile)
    pub quantile: f64,

    /// Value at this quantile
    pub value: f64,
}

/// Metric query parameters
#[derive(Debug, Clone, Deserialize, Serialize, Validate, IntoParams, ToSchema)]
pub struct MetricQueryParams {
    /// Maximum number of metrics to return (default: 100, max: 1000)
    #[validate(range(min = 1, max = 1000))]
    #[serde(default = "default_limit")]
    pub limit: usize,

    /// Offset for pagination (default: 0)
    #[serde(default)]
    pub offset: usize,

    /// Filter by metric name
    pub name: Option<String>,

    /// Filter by metric type (GAUGE, COUNTER, HISTOGRAM, SUMMARY)
    pub metric_type: Option<String>,

    /// Filter by service name
    pub service_name: Option<String>,

    /// Start time filter (Unix timestamp in milliseconds)
    pub start_time: Option<i64>,

    /// End time filter (Unix timestamp in milliseconds)
    pub end_time: Option<i64>,

    /// Relative time range (e.g., "1h", "30m", "7d")
    pub since: Option<String>,

    /// Filter by attribute key-value pairs (format: "key:value")
    pub attributes: Option<Vec<String>>,
}

fn default_limit() -> usize {
    100
}

impl Default for MetricQueryParams {
    fn default() -> Self {
        Self {
            limit: default_limit(),
            offset: 0,
            name: None,
            metric_type: None,
            service_name: None,
            start_time: None,
            end_time: None,
            since: None,
            attributes: None,
        }
    }
}

/// Aggregated metric statistics
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MetricStats {
    /// Metric name
    pub name: String,

    /// Number of data points
    pub count: usize,

    /// Minimum value
    pub min: f64,

    /// Maximum value
    pub max: f64,

    /// Average value
    pub avg: f64,

    /// Sum of all values
    pub sum: f64,

    /// Standard deviation
    pub stddev: Option<f64>,

    /// Percentiles (p50, p95, p99)
    pub percentiles: Option<Percentiles>,
}

/// Percentile values
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Percentiles {
    /// 50th percentile (median)
    pub p50: f64,

    /// 95th percentile
    pub p95: f64,

    /// 99th percentile
    pub p99: f64,
}

impl MetricStats {
    /// Calculate statistics from a list of numeric values
    pub fn from_values(name: String, values: &[f64]) -> Self {
        if values.is_empty() {
            return Self {
                name,
                count: 0,
                min: 0.0,
                max: 0.0,
                avg: 0.0,
                sum: 0.0,
                stddev: None,
                percentiles: None,
            };
        }

        let count = values.len();
        let sum: f64 = values.iter().sum();
        let avg = sum / count as f64;
        let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        // Calculate standard deviation
        let variance: f64 = values.iter().map(|v| (v - avg).powi(2)).sum::<f64>() / count as f64;
        let stddev = Some(variance.sqrt());

        // Calculate percentiles
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let percentiles = Some(Percentiles {
            p50: percentile(&sorted, 0.50),
            p95: percentile(&sorted, 0.95),
            p99: percentile(&sorted, 0.99),
        });

        Self {
            name,
            count,
            min,
            max,
            avg,
            sum,
            stddev,
            percentiles,
        }
    }
}

/// Calculate percentile from sorted values
fn percentile(sorted_values: &[f64], p: f64) -> f64 {
    if sorted_values.is_empty() {
        return 0.0;
    }

    let index = (p * (sorted_values.len() - 1) as f64).round() as usize;
    sorted_values[index.min(sorted_values.len() - 1)]
}

// Made with Bob
