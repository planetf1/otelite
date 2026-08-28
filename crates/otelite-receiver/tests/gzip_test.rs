// Regression tests: OTLP/HTTP gzip (and deflate) request bodies must be
// decompressed before parsing. Before the fix, the Content-Encoding
// header was ignored by a pass-through stub, so every gzip-encoded
// export failed to parse.
//
// All servers bind 127.0.0.1:0 (OS-assigned free port) and use fresh
// TempDir databases.

use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::common::v1::any_value::Value as ProtoValue;
use opentelemetry_proto::tonic::common::v1::AnyValue;
use opentelemetry_proto::tonic::logs::v1::{LogRecord, ResourceLogs, ScopeLogs};
use otelite_core::storage::StorageBackend;
use otelite_receiver::config::ReceiverConfig;
use otelite_receiver::http::HttpServer;
use otelite_storage::{sqlite::SqliteBackend, StorageConfig};
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(data).expect("write");
    enc.finish().expect("finish gzip")
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(data).expect("write");
    enc.finish().expect("finish deflate")
}

async fn start_server(
    max_message_size: usize,
) -> (String, HttpServer, TempDir, Arc<dyn StorageBackend>) {
    let config = ReceiverConfig::new()
        .with_http_addr("127.0.0.1:0".parse().unwrap())
        .with_max_message_size(max_message_size);
    let server = HttpServer::new(config);

    let temp_dir = TempDir::new().expect("temp dir");
    let storage_config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut storage = SqliteBackend::new(storage_config);
    storage.initialize().await.expect("init storage");
    let storage: Arc<dyn StorageBackend> = Arc::new(storage);

    server.start(storage.clone()).await.expect("start server");
    tokio::time::sleep(Duration::from_millis(100)).await;
    let addr = server.local_addr().await.expect("bound address");

    (format!("http://{addr}"), server, temp_dir, storage)
}

fn logs_request(body: &str) -> ExportLogsServiceRequest {
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
                        value: Some(ProtoValue::StringValue(body.to_string())),
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

fn encode_protobuf(request: &ExportLogsServiceRequest) -> Vec<u8> {
    use prost::Message;
    let mut buf = Vec::new();
    request.encode(&mut buf).expect("encode protobuf");
    buf
}

/// gzip-encoded protobuf must be decompressed and stored.
#[tokio::test]
async fn test_gzip_protobuf_logs() {
    let (base, server, _temp, storage) = start_server(10 * 1024 * 1024).await;
    let client = reqwest::Client::new();

    let body = gzip(&encode_protobuf(&logs_request("gzipped protobuf body")));
    let response = client
        .post(format!("{base}/v1/logs"))
        .header("Content-Type", "application/x-protobuf")
        .header("Content-Encoding", "gzip")
        .body(body)
        .send()
        .await
        .expect("send");
    assert_eq!(response.status(), 200, "gzip protobuf must be accepted");

    let stats = storage.stats().await.expect("stats");
    assert_eq!(stats.log_count, 1, "gzipped log must be stored");

    server.shutdown();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

/// gzip-encoded JSON must be decompressed and stored.
#[tokio::test]
async fn test_gzip_json_logs() {
    let (base, server, _temp, storage) = start_server(10 * 1024 * 1024).await;
    let client = reqwest::Client::new();

    let json = serde_json::json!({
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "timeUnixNano": "1700000000000000000",
                    "severityNumber": 9,
                    "severityText": "INFO",
                    "body": { "stringValue": "gzipped json body" }
                }]
            }]
        }]
    })
    .to_string();

    let response = client
        .post(format!("{base}/v1/logs"))
        .header("Content-Type", "application/json")
        .header("Content-Encoding", "gzip")
        .body(gzip(json.as_bytes()))
        .send()
        .await
        .expect("send");
    assert_eq!(response.status(), 200, "gzip JSON must be accepted");

    let stats = storage.stats().await.expect("stats");
    assert_eq!(stats.log_count, 1, "gzipped JSON log must be stored");

    server.shutdown();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

/// raw-deflate-encoded protobuf must be decompressed and stored.
#[tokio::test]
async fn test_deflate_protobuf_logs() {
    let (base, server, _temp, storage) = start_server(10 * 1024 * 1024).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{base}/v1/logs"))
        .header("Content-Type", "application/x-protobuf")
        .header("Content-Encoding", "deflate")
        .body(deflate(&encode_protobuf(&logs_request(
            "deflated protobuf body",
        ))))
        .send()
        .await
        .expect("send");
    assert_eq!(response.status(), 200, "deflate protobuf must be accepted");

    let stats = storage.stats().await.expect("stats");
    assert_eq!(stats.log_count, 1, "deflated log must be stored");

    server.shutdown();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

/// A decompression bomb must be rejected with 413 once the decoded
/// output passes the configured cap, and nothing may be stored.
#[tokio::test]
async fn test_gzip_decompression_bomb_rejected() {
    // Cap: 1 KiB. The payload decompresses to ~64 KiB of zeros.
    let (base, server, _temp, storage) = start_server(1024).await;
    let client = reqwest::Client::new();

    let mut request = logs_request("bomb");
    // 64 KiB of one repeated character compresses down to a few hundred
    // bytes — a classic bomb: tiny on the wire, far past the 1 KiB cap
    // once decoded.
    if let Some(scope) = request
        .resource_logs
        .get_mut(0)
        .and_then(|rl| rl.scope_logs.get_mut(0))
    {
        if let Some(rec) = scope.log_records.get_mut(0) {
            if let Some(AnyValue {
                value: Some(ProtoValue::StringValue(s)),
            }) = rec.body.as_mut()
            {
                *s = "a".repeat(64 * 1024);
            }
        }
    }

    let encoded = encode_protobuf(&request);
    assert!(encoded.len() > 1024, "test setup: encoded body exceeds cap");
    let compressed = gzip(&encoded);

    let response = client
        .post(format!("{base}/v1/logs"))
        .header("Content-Type", "application/x-protobuf")
        .header("Content-Encoding", "gzip")
        .body(compressed)
        .send()
        .await
        .expect("send");
    assert_eq!(
        response.status(),
        413,
        "decompressed output past the cap must be rejected with PAYLOAD_TOO_LARGE"
    );

    let stats = storage.stats().await.expect("stats");
    assert_eq!(
        stats.log_count, 0,
        "nothing from a rejected bomb may be stored"
    );

    server.shutdown();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

/// Unsupported encodings (br, zstd) must be rejected with 415, and
/// encoding lists must be rejected too.
#[tokio::test]
async fn test_unsupported_encoding_rejected() {
    let (base, server, _temp, _storage) = start_server(10 * 1024 * 1024).await;
    let client = reqwest::Client::new();

    for encoding in ["br", "zstd", "gzip, br"] {
        let response = client
            .post(format!("{base}/v1/logs"))
            .header("Content-Type", "application/x-protobuf")
            .header("Content-Encoding", encoding)
            .body(gzip(&encode_protobuf(&logs_request("x"))))
            .send()
            .await
            .expect("send");
        assert_eq!(
            response.status(),
            415,
            "encoding {encoding} must be rejected with UNSUPPORTED_MEDIA_TYPE"
        );
    }

    server.shutdown();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

/// A truncated/corrupt gzip body is a client error: 400, not a 500.
#[tokio::test]
async fn test_corrupt_gzip_rejected_400() {
    let (base, server, _temp, _storage) = start_server(10 * 1024 * 1024).await;
    let client = reqwest::Client::new();

    let good = gzip(b"some payload that is certainly longer than twelve bytes");
    // Truncating the gzip stream mid-body makes it undecodable.
    let corrupt = &good[..good.len() / 2];

    let response = client
        .post(format!("{base}/v1/logs"))
        .header("Content-Type", "application/x-protobuf")
        .header("Content-Encoding", "gzip")
        .body(corrupt.to_vec())
        .send()
        .await
        .expect("send");
    assert_eq!(
        response.status(),
        400,
        "a corrupt gzip body must be rejected with BAD_REQUEST"
    );

    server.shutdown();
    tokio::time::sleep(Duration::from_millis(100)).await;
}
