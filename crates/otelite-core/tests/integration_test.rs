//! Type round-trip integration tests.
//!
//! Replaces the placeholder `example_e2e_test.rs` and
//! `example_integration_test.rs` files (#9): verifies that the telemetry
//! types convert to their API representations and survive a JSON
//! serialise/deserialise cycle unchanged.

use otelite_core::api::{LogEntry, MetricResponse, SpanEntry};
use otelite_core::telemetry::log::{LogRecord, SeverityLevel};
use otelite_core::telemetry::metric::{Metric, MetricType};
use otelite_core::telemetry::trace::{Span, SpanKind, SpanStatus, StatusCode as SpanStatusCode};
use otelite_core::telemetry::Resource;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;

/// Assert that `value` survives a JSON round trip: the deserialised value
/// must re-serialise to an identical JSON value.
fn assert_json_roundtrip<T: Serialize + DeserializeOwned>(value: &T) {
    let before = serde_json::to_value(value).expect("serialise original");
    let back: T = serde_json::from_value(before.clone())
        .expect("deserialise should succeed for a self-serialised value");
    let after = serde_json::to_value(&back).expect("serialise round-tripped value");
    assert_eq!(before, after, "round trip changed the serialised data");
}

fn test_resource() -> Resource {
    let mut attributes = HashMap::new();
    attributes.insert("service.name".to_string(), "roundtrip-service".to_string());
    Resource { attributes }
}

#[test]
fn test_log_entry_json_roundtrip() {
    let mut attributes = HashMap::new();
    attributes.insert("http.method".to_string(), "GET".to_string());

    let record = LogRecord {
        timestamp: 1_700_000_000_123_456_789,
        observed_timestamp: Some(1_700_000_000_234_567_890),
        severity: SeverityLevel::Warn,
        severity_text: Some("WARNING".to_string()),
        body: "request failed: connection reset".to_string(),
        attributes,
        resource: Some(test_resource()),
        trace_id: Some("0af7651916cd43dd8448eb211c80319c".to_string()),
        span_id: Some("b7ad6b7169203331".to_string()),
    };

    let entry = LogEntry::from(record);
    assert_json_roundtrip(&entry);

    // Assert specific values, not just "doesn't panic".
    let json = serde_json::to_value(&entry).unwrap();
    assert_eq!(json["timestamp"].as_i64(), Some(1_700_000_000_123_456_789));
    assert_eq!(json["severity"], "WARN");
    assert_eq!(json["severity_text"], "WARNING");
    assert_eq!(json["body"], "request failed: connection reset");
    assert_eq!(json["trace_id"], "0af7651916cd43dd8448eb211c80319c");
    assert_eq!(json["span_id"], "b7ad6b7169203331");
    assert_eq!(json["attributes"]["http.method"], "GET");
    assert_eq!(
        json["resource"]["attributes"]["service.name"],
        "roundtrip-service"
    );
}

#[test]
fn test_span_entry_json_roundtrip() {
    let mut attributes = HashMap::new();
    attributes.insert("gen_ai.system".to_string(), "openai".to_string());

    let span = Span {
        trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".to_string(),
        span_id: "00f067aa0ba902b7".to_string(),
        parent_span_id: Some("1cf067aa0ba902b7".to_string()),
        name: "opencode.llm".to_string(),
        kind: SpanKind::Client,
        start_time: 1_000_000_000,
        end_time: 1_500_000_000,
        attributes,
        status: SpanStatus {
            code: SpanStatusCode::Ok,
            message: None,
        },
        events: Vec::new(),
        resource: Some(test_resource()),
    };

    let entry = SpanEntry::from(span);
    assert_json_roundtrip(&entry);

    let json = serde_json::to_value(&entry).unwrap();
    assert_eq!(json["trace_id"], "4bf92f3577b34da6a3ce929d0e0e4736");
    assert_eq!(json["span_id"], "00f067aa0ba902b7");
    assert_eq!(json["parent_span_id"], "1cf067aa0ba902b7");
    assert_eq!(json["name"], "opencode.llm");
    assert_eq!(json["kind"], "Client");
    assert_eq!(json["duration"], 500_000_000);
    assert_eq!(json["status"]["code"], "Ok");
}

#[test]
fn test_metric_response_json_roundtrip() {
    let metric = Metric {
        name: "gen_ai.client.token.usage".to_string(),
        description: Some("Token usage".to_string()),
        unit: Some("tokens".to_string()),
        metric_type: MetricType::Gauge(42.5),
        timestamp: 1_700_000_000_000_000_000,
        attributes: HashMap::new(),
        resource: Some(test_resource()),
    };

    let response = MetricResponse::from(metric);
    assert_json_roundtrip(&response);

    let json = serde_json::to_value(&response).unwrap();
    assert_eq!(json["name"], "gen_ai.client.token.usage");
    assert_eq!(json["metric_type"], "gauge");
    assert_eq!(json["value"], 42.5);
    assert_eq!(json["timestamp"].as_i64(), Some(1_700_000_000_000_000_000));
}

#[test]
fn test_histogram_metric_response_json_roundtrip() {
    let metric = Metric {
        name: "http.server.duration".to_string(),
        description: None,
        unit: Some("ms".to_string()),
        metric_type: MetricType::Histogram {
            count: 10,
            sum: 250.0,
            buckets: vec![otelite_core::telemetry::metric::HistogramBucket {
                upper_bound: 100.0,
                count: 7,
            }],
        },
        timestamp: 1_700_000_000_000_000_001,
        attributes: HashMap::new(),
        resource: None,
    };

    let response = MetricResponse::from(metric);
    assert_json_roundtrip(&response);

    let json = serde_json::to_value(&response).unwrap();
    assert_eq!(json["metric_type"], "histogram");
    assert_eq!(json["value"]["sum"], 250.0);
    assert_eq!(json["value"]["count"], 10);
    assert_eq!(json["value"]["buckets"][0]["upper_bound"], 100.0);
    assert_eq!(json["value"]["buckets"][0]["count"], 7);
}
