//! Metric telemetry types

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a metric data point
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metric {
    /// Metric name
    pub name: String,

    /// Metric description
    pub description: Option<String>,

    /// Metric unit
    pub unit: Option<String>,

    /// Metric type
    pub metric_type: MetricType,

    /// Timestamp in nanoseconds since Unix epoch
    pub timestamp: i64,

    /// Metric attributes
    pub attributes: HashMap<String, String>,

    /// Associated resource
    pub resource: Option<super::Resource>,
}

/// Types of metrics
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MetricType {
    /// Gauge metric (instantaneous value)
    Gauge(f64),

    /// Counter metric (monotonically increasing)
    Counter(u64),

    /// Histogram metric (distribution of values)
    Histogram {
        count: u64,
        sum: f64,
        buckets: Vec<HistogramBucket>,
    },

    /// Summary metric (quantiles)
    Summary {
        count: u64,
        sum: f64,
        quantiles: Vec<Quantile>,
    },
}

/// Histogram bucket
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistogramBucket {
    /// Upper bound of the bucket
    pub upper_bound: f64,

    /// Count of values in this bucket
    pub count: u64,
}

/// Summary quantile
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quantile {
    /// Quantile value (0.0 to 1.0)
    pub quantile: f64,

    /// Value at this quantile
    pub value: f64,
}

impl Metric {
    /// Create a new gauge metric
    pub fn gauge(name: impl Into<String>, value: f64, timestamp: i64) -> Self {
        Self {
            name: name.into(),
            description: None,
            unit: None,
            metric_type: MetricType::Gauge(value),
            timestamp,
            attributes: HashMap::new(),
            resource: None,
        }
    }

    /// Create a new counter metric
    pub fn counter(name: impl Into<String>, value: u64, timestamp: i64) -> Self {
        Self {
            name: name.into(),
            description: None,
            unit: None,
            metric_type: MetricType::Counter(value),
            timestamp,
            attributes: HashMap::new(),
            resource: None,
        }
    }

    /// Add an attribute to the metric
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    /// Set the resource for the metric
    pub fn with_resource(mut self, resource: super::Resource) -> Self {
        self.resource = Some(resource);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gauge_metric() {
        let metric = Metric::gauge("cpu.usage", 75.5, 1234567890);
        assert_eq!(metric.name, "cpu.usage");
        assert_eq!(metric.metric_type, MetricType::Gauge(75.5));
        assert_eq!(metric.timestamp, 1234567890);
    }

    #[test]
    fn test_counter_metric() {
        let metric = Metric::counter("requests.total", 1000, 1234567890);
        assert_eq!(metric.name, "requests.total");
        assert_eq!(metric.metric_type, MetricType::Counter(1000));
    }

    #[test]
    fn test_metric_with_attributes() {
        let metric = Metric::gauge("temperature", 22.5, 1234567890)
            .with_attribute("location", "server-room")
            .with_attribute("sensor", "temp-01");

        assert_eq!(metric.attributes.len(), 2);
        assert_eq!(
            metric.attributes.get("location"),
            Some(&"server-room".to_string())
        );
    }
}
