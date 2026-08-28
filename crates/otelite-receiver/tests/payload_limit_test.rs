// Regression tests: the configured max_message_size must actually be
// enforced on both OTLP transports. Before the fix, HTTP bodies were
// capped by axum's built-in 2 MB limit (not the configured value) and
// gRPC messages by tonic's built-in 4 MB default — and neither limit
// was reachable from ReceiverConfig.
//
// All servers bind 127.0.0.1:0 (OS-assigned free port) and use fresh
// TempDir databases.

use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::{
    trace_service_client::TraceServiceClient, ExportTraceServiceRequest,
};
use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue};
use opentelemetry_proto::tonic::logs::v1::{LogRecord, ResourceLogs, ScopeLogs};
use opentelemetry_proto::tonic::trace::v1::{ResourceSpans, ScopeSpans, Span};
use otelite_core::storage::StorageBackend;
use otelite_receiver::config::ReceiverConfig;
use otelite_receiver::grpc::GrpcServer;
use otelite_receiver::http::HttpServer;
use otelite_storage::{sqlite::SqliteBackend, StorageConfig};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

const SMALL_LIMIT: usize = 256; // bytes — tiny, so limits trip fast

async fn temp_storage() -> (Arc<dyn StorageBackend>, TempDir) {
    let temp_dir = TempDir::new().expect("temp dir");
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut storage = SqliteBackend::new(config);
    storage.initialize().await.expect("init test storage");
    (Arc::new(storage), temp_dir)
}

fn log_request(body_size: usize) -> ExportLogsServiceRequest {
    ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: None,
            scope_logs: vec![ScopeLogs {
                scope: None,
                log_records: vec![LogRecord {
                    time_unix_nano: 1_700_000_000_000_000_000,
                    observed_time_unix_nano: 1_700_000_000_000_000_000,
                    severity_number: 9,
                    severity_text: "INFO".to_string(),
                    body: Some(AnyValue {
                        value: Some(
                            opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(
                                "x".repeat(body_size),
                            ),
                        ),
                    }),
                    attributes: vec![],
                    dropped_attributes_count: 0,
                    flags: 0,
                    trace_id: vec![],
                    span_id: vec![],
                    event_name: String::new(),
                }],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    }
}

fn encode(request: &impl prost::Message) -> Vec<u8> {
    let mut buf = Vec::new();
    request.encode(&mut buf).expect("encode");
    buf
}

/// A ~600 byte log export: above the 256 B cap, below the old 2 MB axum
/// default.
#[tokio::test]
async fn test_http_payload_limit() {
    let config = ReceiverConfig::new()
        .with_http_addr("127.0.0.1:0".parse().unwrap())
        .with_max_message_size(SMALL_LIMIT);
    let server = HttpServer::new(config);
    let (storage, _temp_dir) = temp_storage().await;
    server.start(storage.clone()).await.expect("start server");
    tokio::time::sleep(Duration::from_millis(100)).await;
    let addr = server.local_addr().await.expect("bound address");

    let client = reqwest::Client::new();

    // A small valid export must still be accepted (216 B encoded < 256).
    let small = log_request(100);
    assert!(
        encode(&small).len() < SMALL_LIMIT,
        "test setup: small body must fit"
    );
    let response = client
        .post(format!("http://{}/v1/logs", addr))
        .header("Content-Type", "application/x-protobuf")
        .body(encode(&small))
        .send()
        .await
        .expect("send");
    assert_eq!(response.status(), 200, "small export must be accepted");

    // An oversized export must be rejected with 413, not 200.
    let big = log_request(600);
    assert!(
        encode(&big).len() > SMALL_LIMIT,
        "test setup: big body must exceed cap"
    );
    let response = client
        .post(format!("http://{}/v1/logs", addr))
        .header("Content-Type", "application/x-protobuf")
        .body(encode(&big))
        .send()
        .await
        .expect("send");
    assert_eq!(
        response.status(),
        413,
        "oversized export must be rejected with PAYLOAD_TOO_LARGE"
    );

    // And nothing oversized may have reached the database.
    let stats = storage.stats().await.expect("stats");
    assert_eq!(stats.log_count, 1, "only the small export may be stored");

    server.shutdown();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

/// gRPC: messages above the configured cap must come back as
/// RESOURCE_EXHAUSTED (tonic's standard code for oversize messages),
/// and small messages must still be stored.
#[tokio::test]
async fn test_grpc_payload_limit() {
    // Bind a free port via a throwaway listener.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind free port");
    let port = listener.local_addr().expect("port").port();
    drop(listener);

    let config = ReceiverConfig::new()
        .with_grpc_addr(format!("127.0.0.1:{port}").parse().unwrap())
        .with_max_message_size(SMALL_LIMIT);
    let (storage, _temp_dir) = temp_storage().await;
    let server = GrpcServer::new(config, storage.clone());
    server.start().await.expect("start grpc server");

    let endpoint = tonic::transport::Endpoint::from_shared(format!("http://127.0.0.1:{port}"))
        .expect("endpoint")
        .connect_timeout(Duration::from_secs(5));
    let channel = endpoint.connect().await.expect("connect");
    let mut client = TraceServiceClient::new(channel);

    let make_export = |name_size: usize| ExportTraceServiceRequest {
        resource_spans: vec![ResourceSpans {
            resource: None,
            scope_spans: vec![ScopeSpans {
                scope: None,
                spans: vec![Span {
                    trace_id: vec![1; 16],
                    span_id: vec![1; 8],
                    name: "y".repeat(name_size),
                    attributes: vec![KeyValue {
                            key: "filler".to_string(),
                            value: Some(AnyValue {
                                value: Some(
                                    opentelemetry_proto::tonic::common::v1::any_value::Value::
                                        StringValue("z".repeat(name_size)),
                                ),
                            }),
                        }],
                    start_time_unix_nano: 1_700_000_000_000_000_000,
                    end_time_unix_nano: 1_700_000_000_000_000_000,
                    ..Default::default()
                }],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    };

    // Small export succeeds and is stored.
    client
        .export(tonic::Request::new(make_export(10)))
        .await
        .expect("small export must succeed");

    // Oversized export is rejected. (tonic 0.14 maps oversize messages to
    // OUT_OF_RANGE rather than the spec-suggested RESOURCE_EXHAUSTED;
    // either way the export fails and the client sees an error.)
    let err = client
        .export(tonic::Request::new(make_export(600)))
        .await
        .expect_err("oversized export must be rejected");
    assert!(
        matches!(
            err.code(),
            tonic::Code::OutOfRange | tonic::Code::ResourceExhausted
        ),
        "oversize message must be rejected with a size-related status, got {:?}",
        err.code()
    );

    // Only the small span may have been stored.
    let stats = storage.stats().await.expect("stats");
    assert_eq!(stats.span_count, 1, "only the small export may be stored");

    server.shutdown();
    tokio::time::sleep(Duration::from_millis(100)).await;
}
