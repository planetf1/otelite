//! OTLP specification conformance test suite (#1).
//!
//! Verifies transport behaviour against the OTLP spec
//! (https://opentelemetry.io/docs/specs/otlp/):
//! - gRPC: unary RPC per signal type, empty requests succeed,
//!   partial-success response shape, requests > 1 MB are handled
//! - HTTP: protobuf and JSON content types, missing/unsupported
//!   Content-Type rejected, malformed bodies rejected with 400
//! - Data integrity: known values round-trip through storage
//!   (trace_id as lowercase hex, nanosecond timestamps, attributes,
//!   resource attributes)

mod http_test_utils;

use http_test_utils::create_invalid_protobuf;
use opentelemetry_proto::tonic::collector::logs::v1::{
    logs_service_client::LogsServiceClient, ExportLogsServiceRequest,
};
use opentelemetry_proto::tonic::collector::metrics::v1::{
    metrics_service_client::MetricsServiceClient, ExportMetricsServiceRequest,
};
use opentelemetry_proto::tonic::collector::trace::v1::{
    trace_service_client::TraceServiceClient, ExportTraceServiceRequest,
};
use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue};
use opentelemetry_proto::tonic::logs::v1::{LogRecord as ProtoLog, ResourceLogs, ScopeLogs};
use opentelemetry_proto::tonic::metrics::v1::{
    metric::Data, Gauge, Metric as ProtoMetric, NumberDataPoint, ResourceMetrics, ScopeMetrics,
};
use opentelemetry_proto::tonic::resource::v1::Resource as ProtoResource;
use opentelemetry_proto::tonic::trace::v1::{ResourceSpans, ScopeSpans, Span as ProtoSpan};
use otelite_core::storage::{QueryParams, StorageBackend};
use otelite_core::telemetry::log::SeverityLevel;
use otelite_receiver::config::ReceiverConfig;
use otelite_receiver::grpc::GrpcServer;
use otelite_receiver::http::HttpServer;
use otelite_storage::sqlite::SqliteBackend;
use otelite_storage::StorageConfig;
use prost::Message;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

/// Known fixture values for the data-integrity tests.
const TRACE_ID: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
const SPAN_ID: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
const TRACE_ID_HEX: &str = "0102030405060708090a0b0c0d0e0f10";
const SPAN_ID_HEX: &str = "0102030405060708";
const TS_NS: u64 = 1_700_000_000_000_000_000;

fn any_value(s: &str) -> Option<AnyValue> {
    Some(AnyValue {
        value: Some(
            opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(s.to_string()),
        ),
    })
}

fn resource_with_service(service: &str) -> Option<ProtoResource> {
    Some(ProtoResource {
        attributes: vec![KeyValue {
            key: "service.name".to_string(),
            value: any_value(service),
        }],
        dropped_attributes_count: 0,
        entity_refs: vec![],
    })
}

fn log_request(service: &str, body: &str) -> ExportLogsServiceRequest {
    ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: resource_with_service(service),
            scope_logs: vec![ScopeLogs {
                scope: None,
                log_records: vec![ProtoLog {
                    time_unix_nano: TS_NS,
                    observed_time_unix_nano: TS_NS,
                    severity_number: 13,
                    severity_text: "WARN".to_string(),
                    body: any_value(body),
                    attributes: vec![KeyValue {
                        key: "component".to_string(),
                        value: any_value("receiver-test"),
                    }],
                    dropped_attributes_count: 0,
                    flags: 0,
                    trace_id: TRACE_ID.to_vec(),
                    span_id: SPAN_ID.to_vec(),
                    event_name: String::new(),
                }],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    }
}

fn trace_request() -> ExportTraceServiceRequest {
    ExportTraceServiceRequest {
        resource_spans: vec![ResourceSpans {
            resource: resource_with_service("conformance-svc"),
            scope_spans: vec![ScopeSpans {
                scope: None,
                spans: vec![ProtoSpan {
                    trace_id: TRACE_ID.to_vec(),
                    span_id: SPAN_ID.to_vec(),
                    trace_state: String::new(),
                    parent_span_id: vec![],
                    name: "conformance-span".to_string(),
                    kind: 1, // SpanKind::INTERNAL
                    start_time_unix_nano: TS_NS,
                    end_time_unix_nano: TS_NS + 250_000_000, // 250 ms
                    attributes: vec![
                        KeyValue {
                            key: "http.method".to_string(),
                            value: any_value("GET"),
                        },
                        KeyValue {
                            key: "http.status_code".to_string(),
                            value: any_value("200"),
                        },
                    ],
                    dropped_attributes_count: 0,
                    events: vec![],
                    dropped_events_count: 0,
                    links: vec![],
                    dropped_links_count: 0,
                    status: None,
                    flags: 0,
                }],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    }
}

fn metric_request() -> ExportMetricsServiceRequest {
    ExportMetricsServiceRequest {
        resource_metrics: vec![ResourceMetrics {
            resource: resource_with_service("conformance-svc"),
            scope_metrics: vec![ScopeMetrics {
                scope: None,
                metrics: vec![ProtoMetric {
                    name: "conformance.counter".to_string(),
                    description: "Conformance fixture".to_string(),
                    unit: "1".to_string(),
                    data: Some(Data::Gauge(Gauge {
                        data_points: vec![NumberDataPoint {
                            attributes: vec![KeyValue {
                                key: "test_key".to_string(),
                                value: any_value("test_value"),
                            }],
                            start_time_unix_nano: TS_NS - 1_000_000_000,
                            time_unix_nano: TS_NS,
                            value: Some(opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsInt(42)),
                            exemplars: vec![],
                            flags: 0,
                        }],
                    })),
                    metadata: vec![],
                }],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    }
}

fn empty_params() -> QueryParams {
    QueryParams {
        start_time: None,
        end_time: None,
        limit: None,
        trace_id: None,
        span_id: None,
        min_severity: None,
        search_text: None,
        predicates: vec![],
    }
}

async fn create_test_storage() -> (Arc<dyn StorageBackend>, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut storage = SqliteBackend::new(config);
    storage
        .initialize()
        .await
        .expect("Failed to initialize storage");
    (Arc::new(storage), temp_dir)
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind to find a free port");
    listener.local_addr().unwrap().port()
}

/// Start a gRPC OTLP server on a free port with isolated storage.
async fn start_grpc() -> (String, GrpcServer, Arc<dyn StorageBackend>, TempDir) {
    let (storage, dir) = create_test_storage().await;
    let port = free_port();
    let config = ReceiverConfig::new().with_grpc_addr(format!("127.0.0.1:{port}").parse().unwrap());
    let server = GrpcServer::new(config, storage.clone());
    server.start().await.expect("Failed to start gRPC server");
    (format!("http://127.0.0.1:{port}"), server, storage, dir)
}

/// Start an HTTP OTLP server on a free port with isolated storage.
async fn start_http() -> (String, HttpServer, Arc<dyn StorageBackend>, TempDir) {
    let (storage, dir) = create_test_storage().await;
    let port = free_port();
    let config = ReceiverConfig::new().with_http_addr(format!("127.0.0.1:{port}").parse().unwrap());
    let server = HttpServer::new(config);
    server
        .start(storage.clone())
        .await
        .expect("Failed to start HTTP server");
    (format!("http://127.0.0.1:{port}"), server, storage, dir)
}

// ── gRPC transport ─────────────────────────────────────────────────────────

#[tokio::test]
async fn grpc_valid_multi_resource_request_stores_all_signals() {
    let (endpoint, _server, storage, _dir) = start_grpc().await;
    let channel = tonic::transport::Endpoint::from_shared(endpoint)
        .expect("valid endpoint")
        .connect_timeout(Duration::from_secs(5))
        .connect()
        .await
        .expect("client must connect");

    // Two resource blocks (different services) in one request.
    let mut request = log_request("svc-a", "log from svc-a");
    request.resource_logs.push(ResourceLogs {
        resource: resource_with_service("svc-b"),
        scope_logs: vec![ScopeLogs {
            scope: None,
            log_records: vec![log_request("svc-b", "log from svc-b")
                .resource_logs
                .remove(0)
                .scope_logs
                .remove(0)
                .log_records
                .remove(0)],
            schema_url: String::new(),
        }],
        schema_url: String::new(),
    });

    let mut client = LogsServiceClient::new(channel);
    let response = client
        .export(tonic::Request::new(request))
        .await
        .expect("export must succeed")
        .into_inner();

    // Spec: partial success is only reported when something was rejected.
    if let Some(p) = response.partial_success {
        assert_eq!(p.rejected_log_records, 0, "no rejections expected");
    }

    let logs = storage
        .query_logs(&empty_params())
        .await
        .expect("query must succeed");
    assert_eq!(logs.len(), 2, "both resource blocks must be stored");
    let bodies: Vec<&str> = logs.iter().map(|l| l.body.as_str()).collect();
    assert!(bodies.contains(&"log from svc-a"));
    assert!(bodies.contains(&"log from svc-b"));
}

#[tokio::test]
async fn grpc_traces_export_roundtrip() {
    let (endpoint, _server, storage, _dir) = start_grpc().await;
    let channel = tonic::transport::Endpoint::from_shared(endpoint)
        .expect("valid endpoint")
        .connect()
        .await
        .expect("client must connect");

    let mut client = TraceServiceClient::new(channel);
    client
        .export(tonic::Request::new(trace_request()))
        .await
        .expect("export must succeed");

    let spans = storage
        .query_spans(&empty_params())
        .await
        .expect("query must succeed");
    assert_eq!(spans.len(), 1);
    let span = &spans[0];
    // Data integrity: hex IDs, ns timestamps, name, attributes.
    assert_eq!(span.trace_id, TRACE_ID_HEX);
    assert_eq!(span.span_id, SPAN_ID_HEX);
    assert_eq!(span.name, "conformance-span");
    assert_eq!(span.start_time, TS_NS as i64);
    assert_eq!(span.end_time, (TS_NS + 250_000_000) as i64);
    assert_eq!(
        span.attributes.get("http.method").map(String::as_str),
        Some("GET")
    );
    assert_eq!(
        span.attributes.get("http.status_code").map(String::as_str),
        Some("200")
    );
}

#[tokio::test]
async fn grpc_metrics_export_roundtrip() {
    let (endpoint, _server, storage, _dir) = start_grpc().await;
    let channel = tonic::transport::Endpoint::from_shared(endpoint)
        .expect("valid endpoint")
        .connect()
        .await
        .expect("client must connect");

    let mut client = MetricsServiceClient::new(channel);
    client
        .export(tonic::Request::new(metric_request()))
        .await
        .expect("export must succeed");

    let metrics = storage
        .query_metrics(&empty_params())
        .await
        .expect("query must succeed");
    let metric = metrics
        .iter()
        .find(|m| m.name == "conformance.counter")
        .expect("metric must be stored");
    assert_eq!(metric.timestamp, TS_NS as i64);
    assert!(
        matches!(
            metric.metric_type,
            otelite_core::telemetry::metric::MetricType::Gauge(v) if (v - 42.0).abs() < f64::EPSILON
        ),
        "gauge value must round-trip as 42.0, got {:?}",
        metric.metric_type
    );
}

#[tokio::test]
async fn grpc_empty_requests_succeed_for_all_signals() {
    let (endpoint, _server, storage, _dir) = start_grpc().await;
    let channel = tonic::transport::Endpoint::from_shared(endpoint)
        .expect("valid endpoint")
        .connect()
        .await
        .expect("client must connect");

    // Spec: an empty export is a successful no-op, not an error.
    LogsServiceClient::new(channel.clone())
        .export(tonic::Request::new(ExportLogsServiceRequest {
            resource_logs: vec![],
        }))
        .await
        .expect("empty logs export must succeed");

    TraceServiceClient::new(channel.clone())
        .export(tonic::Request::new(ExportTraceServiceRequest {
            resource_spans: vec![],
        }))
        .await
        .expect("empty traces export must succeed");

    MetricsServiceClient::new(channel)
        .export(tonic::Request::new(ExportMetricsServiceRequest {
            resource_metrics: vec![],
        }))
        .await
        .expect("empty metrics export must succeed");

    assert!(
        storage
            .query_logs(&empty_params())
            .await
            .unwrap()
            .is_empty()
            && storage
                .query_spans(&empty_params())
                .await
                .unwrap()
                .is_empty()
            && storage
                .query_metrics(&empty_params())
                .await
                .unwrap()
                .is_empty(),
        "empty exports must store nothing"
    );
}

#[tokio::test]
async fn grpc_partial_success_absent_or_zero_on_success() {
    let (endpoint, _server, _storage, _dir) = start_grpc().await;
    let channel = tonic::transport::Endpoint::from_shared(endpoint)
        .expect("valid endpoint")
        .connect()
        .await
        .expect("client must connect");

    let mut client = TraceServiceClient::new(channel);
    let response = client
        .export(tonic::Request::new(trace_request()))
        .await
        .expect("export must succeed")
        .into_inner();

    // Nothing was rejected, so partial success must be absent or zeroed.
    if let Some(p) = response.partial_success {
        assert_eq!(p.rejected_spans, 0, "no rejections expected");
    }
}

#[tokio::test]
async fn grpc_request_over_1mb_succeeds() {
    let (endpoint, _server, storage, _dir) = start_grpc().await;
    let channel = tonic::transport::Endpoint::from_shared(endpoint)
        .expect("valid endpoint")
        .connect()
        .await
        .expect("client must connect");

    // ~1.2 MB of log bodies (spec requires >1 MB payloads to be handled).
    const N: usize = 4_000;
    let padding = "x".repeat(300);
    let request = ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: resource_with_service("large-svc"),
            scope_logs: vec![ScopeLogs {
                scope: None,
                log_records: (0..N)
                    .map(|i| ProtoLog {
                        time_unix_nano: TS_NS + i as u64,
                        observed_time_unix_nano: TS_NS + i as u64,
                        severity_number: 9,
                        severity_text: "INFO".to_string(),
                        body: any_value(&format!("record {i} {padding}")),
                        attributes: vec![],
                        dropped_attributes_count: 0,
                        flags: 0,
                        trace_id: vec![],
                        span_id: vec![],
                        event_name: String::new(),
                    })
                    .collect(),
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    };
    let encoded = request.encode_to_vec();
    assert!(encoded.len() > 1024 * 1024, "fixture must exceed 1 MB");

    let mut client = LogsServiceClient::new(channel);
    client
        .export(tonic::Request::new(request))
        .await
        .expect("large export must succeed");

    let logs = storage
        .query_logs(&empty_params())
        .await
        .expect("query must succeed");
    assert_eq!(
        logs.len(),
        N,
        "all records from the large batch must be stored"
    );
}

// ── HTTP transport ─────────────────────────────────────────────────────────

#[tokio::test]
async fn http_protobuf_bodies_accepted_for_all_signals() {
    let (base_url, _server, storage, _dir) = start_http().await;
    let client = reqwest::Client::new();

    for (path, body) in [
        ("/v1/traces", trace_request().encode_to_vec()),
        (
            "/v1/logs",
            log_request("http-svc", "http log body").encode_to_vec(),
        ),
        ("/v1/metrics", metric_request().encode_to_vec()),
    ] {
        let response = client
            .post(format!("{base_url}{path}"))
            .header("Content-Type", "application/x-protobuf")
            .body(body)
            .send()
            .await
            .expect("request must succeed");
        assert_eq!(response.status(), 200, "POST {path} protobuf");
    }

    assert_eq!(storage.query_spans(&empty_params()).await.unwrap().len(), 1);
    assert_eq!(storage.query_logs(&empty_params()).await.unwrap().len(), 1);
    assert_eq!(
        storage.query_metrics(&empty_params()).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn http_json_bodies_accepted_for_all_signals() {
    let (base_url, _server, storage, _dir) = start_http().await;
    let client = reqwest::Client::new();

    for (path, body) in [
        ("/v1/traces", http_test_utils::create_traces_json()),
        ("/v1/logs", http_test_utils::create_logs_json()),
        ("/v1/metrics", http_test_utils::create_metrics_json()),
    ] {
        let response = client
            .post(format!("{base_url}{path}"))
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .expect("request must succeed");
        assert_eq!(response.status(), 200, "POST {path} json");
    }

    assert_eq!(storage.query_spans(&empty_params()).await.unwrap().len(), 1);
    assert_eq!(storage.query_logs(&empty_params()).await.unwrap().len(), 1);
    assert_eq!(
        storage.query_metrics(&empty_params()).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn http_missing_content_type_rejected() {
    let (base_url, _server, _storage, _dir) = start_http().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{base_url}/v1/logs"))
        .body(log_request("svc", "body").encode_to_vec())
        .send()
        .await
        .expect("request must succeed");

    // Spec allows 400 or 415 for an unrecognised/missing media type.
    assert!(
        response.status().as_u16() == 400 || response.status().as_u16() == 415,
        "missing Content-Type must be rejected, got {}",
        response.status()
    );
}

#[tokio::test]
async fn http_unsupported_content_type_rejected() {
    let (base_url, _server, _storage, _dir) = start_http().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{base_url}/v1/traces"))
        .header("Content-Type", "text/plain")
        .body("not an otlp payload")
        .send()
        .await
        .expect("request must succeed");

    assert!(
        response.status().as_u16() == 400 || response.status().as_u16() == 415,
        "unsupported Content-Type must be rejected, got {}",
        response.status()
    );
}

#[tokio::test]
async fn http_invalid_protobuf_rejected_with_400() {
    let (base_url, _server, _storage, _dir) = start_http().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{base_url}/v1/logs"))
        .header("Content-Type", "application/x-protobuf")
        .body(create_invalid_protobuf().to_vec())
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(
        response.status(),
        400,
        "malformed protobuf must be rejected with 400"
    );
}

#[tokio::test]
async fn http_invalid_json_rejected_with_400() {
    let (base_url, _server, _storage, _dir) = start_http().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{base_url}/v1/metrics"))
        .header("Content-Type", "application/json")
        .body("{ this is not json")
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(
        response.status(),
        400,
        "malformed JSON must be rejected with 400"
    );
}

#[tokio::test]
async fn http_request_over_1mb_succeeds() {
    let (base_url, _server, storage, _dir) = start_http().await;
    let client = reqwest::Client::new();

    const N: usize = 4_000;
    let padding = "x".repeat(300);
    let request = ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: resource_with_service("large-http-svc"),
            scope_logs: vec![ScopeLogs {
                scope: None,
                log_records: (0..N)
                    .map(|i| ProtoLog {
                        time_unix_nano: TS_NS + i as u64,
                        observed_time_unix_nano: TS_NS + i as u64,
                        severity_number: 9,
                        severity_text: "INFO".to_string(),
                        body: any_value(&format!("record {i} {padding}")),
                        attributes: vec![],
                        dropped_attributes_count: 0,
                        flags: 0,
                        trace_id: vec![],
                        span_id: vec![],
                        event_name: String::new(),
                    })
                    .collect(),
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    };
    let body = request.encode_to_vec();
    assert!(body.len() > 1024 * 1024, "fixture must exceed 1 MB");

    let response = client
        .post(format!("{base_url}/v1/logs"))
        .header("Content-Type", "application/x-protobuf")
        .body(body)
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(
        response.status(),
        200,
        "payloads over 1 MB must be accepted"
    );

    assert_eq!(storage.query_logs(&empty_params()).await.unwrap().len(), N);
}

// ── Data integrity ─────────────────────────────────────────────────────────

#[tokio::test]
async fn http_log_roundtrip_preserves_fields() {
    let (base_url, _server, storage, _dir) = start_http().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{base_url}/v1/logs"))
        .header("Content-Type", "application/x-protobuf")
        .body(log_request("integrity-svc", "integrity log body").encode_to_vec())
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), 200);

    let logs = storage
        .query_logs(&empty_params())
        .await
        .expect("query must succeed");
    assert_eq!(logs.len(), 1);
    let log = &logs[0];

    // Nanosecond timestamps preserved exactly.
    assert_eq!(log.timestamp, TS_NS as i64);
    // Severity number 13 maps to the Warn band.
    assert_eq!(log.severity, SeverityLevel::Warn);
    assert_eq!(log.severity_text.as_deref(), Some("WARN"));
    assert_eq!(log.body, "integrity log body");
    assert_eq!(
        log.attributes.get("component").map(String::as_str),
        Some("receiver-test")
    );
    // Trace linkage and resource attributes preserved.
    assert_eq!(log.trace_id.as_deref(), Some(TRACE_ID_HEX));
    assert_eq!(log.span_id.as_deref(), Some(SPAN_ID_HEX));
    let resource = log.resource.as_ref().expect("resource must be stored");
    assert_eq!(
        resource.attributes.get("service.name").map(String::as_str),
        Some("integrity-svc")
    );
}

#[tokio::test]
async fn trace_id_filter_finds_roundtrip_trace() {
    let (endpoint, _server, storage, _dir) = start_grpc().await;
    let channel = tonic::transport::Endpoint::from_shared(endpoint)
        .expect("valid endpoint")
        .connect()
        .await
        .expect("client must connect");

    let mut client = TraceServiceClient::new(channel);
    client
        .export(tonic::Request::new(trace_request()))
        .await
        .expect("export must succeed");

    let mut params = empty_params();
    params.trace_id = Some(TRACE_ID_HEX.to_string());
    let spans = storage
        .query_spans(&params)
        .await
        .expect("query must succeed");
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].trace_id, TRACE_ID_HEX);

    // A different trace id must not match.
    params.trace_id = Some("ffffffffffffffffffffffffffffffff".to_string());
    let spans = storage
        .query_spans(&params)
        .await
        .expect("query must succeed");
    assert!(spans.is_empty(), "unknown trace id must not match");
}
