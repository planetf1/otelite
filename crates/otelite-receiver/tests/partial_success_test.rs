// Partial-success tests (#256): telemetry the internal model cannot store
// (exponential histograms, unset values, +Inf overflow buckets, span
// links) is COUNTED and REPORTED — via OTLP partial success on the
// response and a warn log — never silently dropped, and never asserted
// stored.
//
// The server binds 127.0.0.1:0 (OS-assigned free port) and uses a fresh
// TempDir database.

mod http_test_utils;

use http_test_utils::encode_protobuf;
use opentelemetry_proto::tonic::collector::metrics::v1::metrics_service_client::MetricsServiceClient;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::{
    trace_service_client::TraceServiceClient, ExportTraceServiceRequest,
};
use opentelemetry_proto::tonic::metrics::v1::{
    metric::Data, ExponentialHistogramDataPoint, Gauge, Histogram, HistogramDataPoint, Metric,
    NumberDataPoint, ResourceMetrics, ScopeMetrics,
};
use opentelemetry_proto::tonic::trace::v1::{
    span::Link as SpanLink, ResourceSpans, ScopeSpans, Span,
};
use otelite_core::storage::{QueryParams, StorageBackend};
use otelite_receiver::config::ReceiverConfig;
use otelite_receiver::grpc::GrpcServer;
use otelite_receiver::http::HttpServer;
use otelite_storage::{sqlite::SqliteBackend, StorageConfig};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::sleep;

// ---------------------------------------------------------------------------
// Request builders
// ---------------------------------------------------------------------------

fn gauge(
    name: &str,
    value: Option<opentelemetry_proto::tonic::metrics::v1::number_data_point::Value>,
) -> Metric {
    Metric {
        name: name.to_string(),
        description: String::new(),
        unit: String::new(),
        data: Some(Data::Gauge(Gauge {
            data_points: vec![NumberDataPoint {
                attributes: vec![],
                start_time_unix_nano: 0,
                time_unix_nano: 1,
                value,
                exemplars: vec![],
                flags: 0,
            }],
        })),
        metadata: vec![],
    }
}

fn exp_histogram(name: &str) -> Metric {
    Metric {
        name: name.to_string(),
        description: String::new(),
        unit: String::new(),
        data: Some(Data::ExponentialHistogram(
            opentelemetry_proto::tonic::metrics::v1::ExponentialHistogram {
                aggregation_temporality: 1,
                data_points: vec![ExponentialHistogramDataPoint {
                    attributes: vec![],
                    start_time_unix_nano: 0,
                    time_unix_nano: 1,
                    count: 5,
                    sum: Some(10.0),
                    scale: 0,
                    zero_count: 0,
                    positive: None,
                    negative: None,
                    flags: 0,
                    exemplars: vec![],
                    ..Default::default()
                }],
            },
        )),
        metadata: vec![],
    }
}

/// A classic histogram with 2 bounded observations and 3 in the +Inf
/// overflow bucket (bucket_counts is N+1 entries per the OTLP spec).
fn overflowing_histogram(name: &str) -> Metric {
    Metric {
        name: name.to_string(),
        description: String::new(),
        unit: String::new(),
        data: Some(Data::Histogram(Histogram {
            aggregation_temporality: 1,
            data_points: vec![HistogramDataPoint {
                attributes: vec![],
                start_time_unix_nano: 0,
                time_unix_nano: 1,
                count: 5,
                sum: Some(100.0),
                bucket_counts: vec![2, 3],
                explicit_bounds: vec![10.0],
                exemplars: vec![],
                flags: 0,
                ..Default::default()
            }],
        })),
        metadata: vec![],
    }
}

fn metrics_request(metrics: Vec<Metric>) -> ExportMetricsServiceRequest {
    ExportMetricsServiceRequest {
        resource_metrics: vec![ResourceMetrics {
            resource: None,
            scope_metrics: vec![ScopeMetrics {
                scope: None,
                metrics,
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    }
}

// ---------------------------------------------------------------------------
// gRPC: partial success counts the dropped data points
// ---------------------------------------------------------------------------

#[tokio::test]
async fn grpc_metrics_partial_success_reports_dropped_data_points() {
    let temp_dir = TempDir::new().expect("temp dir");
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.expect("init storage");
    let storage: Arc<dyn StorageBackend> = Arc::new(backend);

    let config = ReceiverConfig::new().with_grpc_addr("127.0.0.1:0".parse().unwrap());
    let server = GrpcServer::new(config, storage.clone());
    server.start().await.expect("start gRPC");
    sleep(Duration::from_millis(100)).await;
    let port = server.local_addr().await.expect("bound address").port();

    let channel = tonic::transport::Endpoint::from_shared(format!("http://127.0.0.1:{port}"))
        .expect("endpoint")
        .connect()
        .await
        .expect("connect");
    let mut client = MetricsServiceClient::new(channel);

    // 1 storable gauge + 1 exponential-histogram point + 1 unset-value
    // gauge + 1 histogram point with a +Inf overflow tail = 3 rejected
    // data points (the overflow point is stored lossily, not fully).
    let request = tonic::Request::new(metrics_request(vec![
        gauge(
            "ok_gauge",
            Some(opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsInt(7)),
        ),
        exp_histogram("exp_hist"),
        gauge("unset_gauge", None),
        overflowing_histogram("overflow_hist"),
    ]));

    let response = client
        .export(request)
        .await
        .expect("export succeeds (partial)")
        .into_inner();

    let partial = response
        .partial_success
        .expect("partial success must be present when telemetry is dropped");
    assert_eq!(
        partial.rejected_data_points, 3,
        "exponential point + unset point + overflow point must be counted: {}",
        partial.error_message
    );
    assert!(
        partial.error_message.contains("exponential-histogram"),
        "the message must say what was dropped: {}",
        partial.error_message
    );

    // Only the fully-representable rows were stored.
    let stored = storage
        .query_metrics(&QueryParams::default())
        .await
        .expect("query metrics");
    let names: Vec<&str> = stored.iter().map(|m| m.name.as_str()).collect();
    assert!(
        names.contains(&"ok_gauge"),
        "the plain gauge must be stored"
    );
    assert!(
        names.contains(&"overflow_hist"),
        "the histogram row must be stored (lossy tail reported)"
    );
    assert!(
        !names.contains(&"exp_hist") && !names.contains(&"unset_gauge"),
        "dropped data points must not be stored"
    );

    server.shutdown();
    sleep(Duration::from_millis(100)).await;
}

/// A clean export (nothing dropped) must NOT carry a partial success —
/// per the OTLP spec, absent/zero means fully accepted.
#[tokio::test]
async fn grpc_metrics_clean_export_has_no_partial_success() {
    let temp_dir = TempDir::new().expect("temp dir");
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.expect("init storage");
    let storage: Arc<dyn StorageBackend> = Arc::new(backend);

    let config = ReceiverConfig::new().with_grpc_addr("127.0.0.1:0".parse().unwrap());
    let server = GrpcServer::new(config, storage.clone());
    server.start().await.expect("start gRPC");
    sleep(Duration::from_millis(100)).await;
    let port = server.local_addr().await.expect("bound address").port();

    let channel = tonic::transport::Endpoint::from_shared(format!("http://127.0.0.1:{port}"))
        .expect("endpoint")
        .connect()
        .await
        .expect("connect");
    let mut client = MetricsServiceClient::new(channel);

    let request = tonic::Request::new(metrics_request(vec![gauge(
        "clean",
        Some(opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsInt(1)),
    )]));
    let response = client.export(request).await.expect("export").into_inner();
    assert!(
        response.partial_success.is_none(),
        "a fully-accepted export must not carry partial success"
    );

    server.shutdown();
    sleep(Duration::from_millis(100)).await;
}

/// Span links are dropped from ACCEPTED spans: the span is stored, the
/// links are counted (handler warn log) — but rejected_spans stays 0,
/// because the protocol field is reserved for whole-span rejections.
#[tokio::test]
async fn grpc_traces_links_dropped_but_span_accepted() {
    let temp_dir = TempDir::new().expect("temp dir");
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.expect("init storage");
    let storage: Arc<dyn StorageBackend> = Arc::new(backend);

    let config = ReceiverConfig::new().with_grpc_addr("127.0.0.1:0".parse().unwrap());
    let server = GrpcServer::new(config, storage.clone());
    server.start().await.expect("start gRPC");
    sleep(Duration::from_millis(100)).await;
    let port = server.local_addr().await.expect("bound address").port();

    let channel = tonic::transport::Endpoint::from_shared(format!("http://127.0.0.1:{port}"))
        .expect("endpoint")
        .connect()
        .await
        .expect("connect");
    let mut client = TraceServiceClient::new(channel);

    let link = SpanLink {
        trace_id: vec![9; 16],
        span_id: vec![9; 8],
        trace_state: String::new(),
        attributes: vec![],
        dropped_attributes_count: 0,
        flags: 0,
    };
    let request = tonic::Request::new(ExportTraceServiceRequest {
        resource_spans: vec![ResourceSpans {
            resource: None,
            scope_spans: vec![ScopeSpans {
                scope: None,
                spans: vec![Span {
                    trace_id: vec![1; 16],
                    span_id: vec![2; 8],
                    name: "linked-span".to_string(),
                    start_time_unix_nano: 1_700_000_000_000_000_000,
                    end_time_unix_nano: 1_700_000_000_500_000_000,
                    links: vec![link.clone(), link],
                    ..Default::default()
                }],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    });

    let response = client
        .export(request)
        .await
        .expect("export succeeds")
        .into_inner();
    // The span was accepted: no rejection report.
    let rejected = response
        .partial_success
        .as_ref()
        .map(|p| p.rejected_spans)
        .unwrap_or(0);
    assert_eq!(rejected, 0, "dropped links are not rejected spans");

    let spans = storage
        .query_spans(&QueryParams::default())
        .await
        .expect("query spans");
    assert_eq!(
        spans.len(),
        1,
        "the span must be stored despite the dropped links"
    );

    server.shutdown();
    sleep(Duration::from_millis(100)).await;
}

// ---------------------------------------------------------------------------
// HTTP: partialSuccess in the JSON response
// ---------------------------------------------------------------------------

#[tokio::test]
async fn http_metrics_partial_success_in_json_response() {
    let temp_dir = TempDir::new().expect("temp dir");
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.expect("init storage");
    let storage: Arc<dyn StorageBackend> = Arc::new(backend);

    let config = ReceiverConfig::new().with_http_addr("127.0.0.1:0".parse().unwrap());
    let server = HttpServer::new(config);
    server.start(storage).await.expect("start HTTP");
    sleep(Duration::from_millis(100)).await;
    let base = format!(
        "http://{}",
        server.local_addr().await.expect("bound address")
    );

    let client = reqwest::Client::new();
    let body = metrics_request(vec![
        gauge(
            "ok_gauge",
            Some(opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsInt(7)),
        ),
        exp_histogram("exp_hist"),
    ]);

    let response = client
        .post(format!("{base}/v1/metrics"))
        .header("Content-Type", "application/x-protobuf")
        .body(encode_protobuf(&body).to_vec())
        .send()
        .await
        .expect("send");
    assert_eq!(response.status(), 200);
    let json: serde_json::Value = response.json().await.expect("json body");

    let partial = json["partialSuccess"]
        .as_object()
        .expect("partialSuccess must be present when telemetry is dropped");
    assert_eq!(
        partial["rejectedDataPoints"].as_u64(),
        Some(1),
        "the exponential-histogram point must be counted: {json}"
    );
    assert!(
        partial["errorMessage"]
            .as_str()
            .expect("errorMessage")
            .contains("exponential-histogram"),
        "the message must say what was dropped: {json}"
    );

    server.shutdown();
    sleep(Duration::from_millis(100)).await;
}
