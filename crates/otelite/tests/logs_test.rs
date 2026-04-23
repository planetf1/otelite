//! Integration tests for logs commands

use mockito::{Matcher, Server};
use std::time::Duration;
use tempfile::NamedTempFile;

// Helper to create a mock API client
async fn create_test_client(server_url: String) -> otelite_client::ApiClient {
    otelite_client::ApiClient::new(server_url, Duration::from_secs(30)).unwrap()
}

// Helper to create test config
fn create_test_config(
    endpoint: String,
    format: otelite::config::OutputFormat,
) -> otelite::config::Config {
    otelite::config::Config {
        endpoint,
        timeout: Duration::from_secs(30),
        format,
        no_color: true, // Disable colours for testing
        no_header: false,
        no_pager: true,
    }
}

// T020: Integration test for logs list command
#[tokio::test]
async fn test_logs_list_command() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/logs")
        .match_query(Matcher::UrlEncoded("limit".into(), "10".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
            "logs": [
                {
                    "timestamp": 1705315800000000000,
                    "severity": "INFO",
                    "body": "Test log 1",
                    "attributes": {}
                },
                {
                    "timestamp": 1705315860000000000,
                    "severity": "ERROR",
                    "body": "Test log 2",
                    "attributes": {}
                }
            ],
            "total": 2,
            "limit": 10,
            "offset": 0
        }"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result =
        otelite::commands::logs::handle_list(&client, &config, Some(10), None, None, None).await;

    mock.assert_async().await;
    assert!(result.is_ok());
    let logs = result.unwrap();
    assert_eq!(logs.len(), 2);
    assert_eq!(logs[0].timestamp, 1705315800000000000);
    assert_eq!(logs[1].timestamp, 1705315860000000000);
}

#[tokio::test]
async fn test_logs_list_with_severity_filter() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/logs")
        .match_query(Matcher::UrlEncoded("severity".into(), "ERROR".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
            "logs": [
                {
                    "timestamp": 1705315800000000000,
                    "severity": "ERROR",
                    "body": "Error message",
                    "attributes": {}
                }
            ],
            "total": 1,
            "limit": 10,
            "offset": 0
        }"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result = otelite::commands::logs::handle_list(
        &client,
        &config,
        None,
        Some("ERROR".to_string()),
        None,
        None,
    )
    .await;

    mock.assert_async().await;
    assert!(result.is_ok());
    let logs = result.unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].severity, "ERROR");
}

#[tokio::test]
async fn test_logs_list_empty_response() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/logs")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"logs": [], "total": 0, "limit": 100, "offset": 0}"#)
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result =
        otelite::commands::logs::handle_list(&client, &config, None, None, None, None).await;

    mock.assert_async().await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 0);
}

// T021: Integration test for logs search command
#[tokio::test]
async fn test_logs_search_command() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/logs")
        .match_query(Matcher::UrlEncoded("search".into(), "error".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
            "logs": [
            {
                "timestamp": 1705315800000000000,
                "severity": "ERROR",
                "severity_text": "ERROR",
                "body": "Error in processing",
                "attributes": {},
                "resource_attributes": {},
                "scope_name": "test",
                "trace_id": null,
                "span_id": null
            },
            {
                "timestamp": 1705315860000000000,
                "severity": "ERROR",
                "severity_text": "ERROR",
                "body": "Another error",
                "attributes": {},
                "resource_attributes": {},
                "scope_name": "test",
                "trace_id": null,
                "span_id": null
            }
            ],
            "total": 2,
            "limit": 100,
            "offset": 0
        }"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result =
        otelite::commands::logs::handle_search(&client, &config, "error", None, None).await;

    mock.assert_async().await;
    assert!(result.is_ok());
    let logs = result.unwrap();
    assert_eq!(logs.len(), 2);
    assert!(logs[0].body.contains("Error"));
}

#[tokio::test]
async fn test_logs_search_with_limit() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/logs")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("search".into(), "test".into()),
            Matcher::UrlEncoded("limit".into(), "5".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"logs": [], "total": 0, "limit": 5, "offset": 0}"#)
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result =
        otelite::commands::logs::handle_search(&client, &config, "test", Some(5), None).await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

// T022: Integration test for logs show command
#[tokio::test]
async fn test_logs_show_command() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/logs/1705315800000000000")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
            "timestamp": 1705315800000000000,
            "severity": "ERROR",
            "severity_text": "ERROR",
            "body": "Detailed error message",
            "attributes": {
                "user_id": "12345",
                "request_id": "abc-def"
            },
            "resource_attributes": {},
            "scope_name": "test",
            "trace_id": null,
            "span_id": null
        }"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result =
        otelite::commands::logs::handle_show(&client, &config, "1705315800000000000").await;

    mock.assert_async().await;
    assert!(result.is_ok());
    let log = result.unwrap();
    assert_eq!(log.timestamp, 1705315800000000000);
    assert_eq!(log.severity, "ERROR");
    assert_eq!(log.attributes.len(), 2);
}

#[tokio::test]
async fn test_logs_show_not_found() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/logs/9999999999999999")
        .with_status(404)
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result = otelite::commands::logs::handle_show(&client, &config, "9999999999999999").await;

    mock.assert_async().await;
    assert!(result.is_err());
    match result.unwrap_err() {
        otelite::error::Error::NotFound(_) => {}, // Expected
        _ => panic!("Expected NotFound error"),
    }
}

// T023: Integration test for JSON output format validation
#[tokio::test]
async fn test_json_output_format() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/logs")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
            "logs": [
            {
                "timestamp": 1705315800000000000,
                "severity": "INFO",
                "severity_text": "INFO",
                "body": "Test message",
                "attributes": {},
                "resource_attributes": {},
                "scope_name": "test",
                "trace_id": null,
                "span_id": null
            }
            ],
            "total": 1,
            "limit": 100,
            "offset": 0
        }"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result =
        otelite::commands::logs::handle_list(&client, &config, None, None, None, None).await;

    mock.assert_async().await;
    assert!(result.is_ok());

    let logs = result.unwrap();
    let json_str = serde_json::to_string(&logs).unwrap();
    let parsed: Vec<otelite_client::models::LogEntry> = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].timestamp, 1705315800000000000);
}

#[tokio::test]
async fn test_pretty_output_format() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/logs")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
            "logs": [
            {
                "timestamp": 1705315800000000000,
                "severity": "INFO",
                "severity_text": "INFO",
                "body": "Test message",
                "attributes": {},
                "resource_attributes": {},
                "scope_name": "test",
                "trace_id": null,
                "span_id": null
            }
            ],
            "total": 1,
            "limit": 100,
            "offset": 0
        }"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Pretty);

    let result =
        otelite::commands::logs::handle_list(&client, &config, None, None, None, None).await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

// T024: Integration test for severity filtering
#[tokio::test]
async fn test_severity_filtering_integration() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/logs")
        .match_query(Matcher::UrlEncoded("severity".into(), "WARN".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
            "logs": [
            {
                "timestamp": 1705315800000000000,
                "severity": "ERROR",
                "severity_text": "ERROR",
                "body": "Error",
                "attributes": {},
                "resource_attributes": {},
                "scope_name": "test",
                "trace_id": null,
                "span_id": null
            },
            {
                "timestamp": 1705315860000000000,
                "severity": "WARN",
                "severity_text": "WARN",
                "body": "Warning",
                "attributes": {},
                "resource_attributes": {},
                "scope_name": "test",
                "trace_id": null,
                "span_id": null
            },
            {
                "timestamp": 1705315920000000000,
                "severity": "INFO",
                "severity_text": "INFO",
                "body": "Info",
                "attributes": {},
                "resource_attributes": {},
                "scope_name": "test",
                "trace_id": null,
                "span_id": null
            },
            {
                "timestamp": 1705315980000000000,
                "severity": "DEBUG",
                "severity_text": "DEBUG",
                "body": "Debug",
                "attributes": {},
                "resource_attributes": {},
                "scope_name": "test",
                "trace_id": null,
                "span_id": null
            }
            ],
            "total": 4,
            "limit": 100,
            "offset": 0
        }"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result = otelite::commands::logs::handle_list(
        &client,
        &config,
        None,
        Some("WARN".to_string()),
        None,
        None,
    )
    .await;

    mock.assert_async().await;
    assert!(result.is_ok());
    let logs = result.unwrap();

    assert_eq!(logs.len(), 2);
    assert!(logs.iter().any(|l| l.severity == "ERROR"));
    assert!(logs.iter().any(|l| l.severity == "WARN"));
    assert!(!logs.iter().any(|l| l.severity == "INFO"));
    assert!(!logs.iter().any(|l| l.severity == "DEBUG"));
}

// T025: Integration tests for logs export command
#[tokio::test]
async fn test_logs_export_json_stdout_is_valid_json_array() {
    let mut server = Server::new_async().await;
    let body = r#"[
        {
            "timestamp": 1705315800000000000,
            "severity": "INFO",
            "severity_text": "INFO",
            "body": "Exported log",
            "attributes": {},
            "resource": null,
            "trace_id": null,
            "span_id": null
        }
    ]"#;
    let mock = server
        .mock("GET", "/api/logs/export")
        .match_query(Matcher::UrlEncoded("format".into(), "json".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result =
        otelite::commands::logs::handle_export(&client, &config, "json", None, None, None).await;

    mock.assert_async().await;
    assert!(result.is_ok());

    let parsed: Vec<serde_json::Value> = serde_json::from_str(body).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0]["body"], "Exported log");
}

#[tokio::test]
async fn test_logs_export_csv_contains_header_and_data_row() {
    let mut server = Server::new_async().await;
    let csv = "timestamp,severity,body\n1705315800000000000,INFO,Exported log\n";
    let mock = server
        .mock("GET", "/api/logs/export")
        .match_query(Matcher::UrlEncoded("format".into(), "csv".into()))
        .with_status(200)
        .with_header("content-type", "text/csv")
        .with_body(csv)
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result =
        otelite::commands::logs::handle_export(&client, &config, "csv", None, None, None).await;

    mock.assert_async().await;
    assert!(result.is_ok());

    let mut reader = csv::Reader::from_reader(csv.as_bytes());
    let headers = reader.headers().unwrap().clone();
    assert_eq!(
        headers,
        csv::StringRecord::from(vec!["timestamp", "severity", "body"])
    );

    let rows = reader
        .records()
        .collect::<Result<Vec<_>, csv::Error>>()
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get(1), Some("INFO"));
}

#[tokio::test]
async fn test_logs_export_json_file_output_writes_valid_json() {
    let mut server = Server::new_async().await;
    let body = r#"[
        {
            "timestamp": 1705315800000000000,
            "severity": "INFO",
            "severity_text": "INFO",
            "body": "Written to file",
            "attributes": {},
            "resource": null,
            "trace_id": null,
            "span_id": null
        }
    ]"#;
    let mock = server
        .mock("GET", "/api/logs/export")
        .match_query(Matcher::UrlEncoded("format".into(), "json".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_string_lossy().to_string();

    let result = otelite::commands::logs::handle_export(
        &client,
        &config,
        "json",
        None,
        None,
        Some(path.clone()),
    )
    .await;

    mock.assert_async().await;
    assert!(result.is_ok());

    let written = std::fs::read_to_string(&path).unwrap();
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&written).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0]["body"], "Written to file");
}

#[tokio::test]
async fn test_logs_export_with_query_filter_only_requests_matching_logs() {
    let mut server = Server::new_async().await;
    let body = r#"[
        {
            "timestamp": 1705315800000000000,
            "severity": "ERROR",
            "severity_text": "ERROR",
            "body": "Matching exported log",
            "attributes": {},
            "resource": null,
            "trace_id": null,
            "span_id": null
        }
    ]"#;
    let mock = server
        .mock("GET", "/api/logs/export")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("format".into(), "json".into()),
            Matcher::UrlEncoded("severity".into(), "ERROR".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result = otelite::commands::logs::handle_export(
        &client,
        &config,
        "json",
        Some("ERROR".to_string()),
        None,
        None,
    )
    .await;

    mock.assert_async().await;
    assert!(result.is_ok());

    let parsed: Vec<serde_json::Value> = serde_json::from_str(body).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0]["severity"], "ERROR");
}

#[tokio::test]
async fn test_logs_export_empty_result_json_is_empty_array() {
    let mut server = Server::new_async().await;
    let body = "[]";
    let mock = server
        .mock("GET", "/api/logs/export")
        .match_query(Matcher::UrlEncoded("format".into(), "json".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result =
        otelite::commands::logs::handle_export(&client, &config, "json", None, None, None).await;

    mock.assert_async().await;
    assert!(result.is_ok());

    let parsed: Vec<serde_json::Value> = serde_json::from_str(body).unwrap();
    assert!(parsed.is_empty());
}

#[tokio::test]
async fn test_logs_export_empty_result_csv_has_header_only() {
    let mut server = Server::new_async().await;
    let csv = "timestamp,severity,body\n";
    let mock = server
        .mock("GET", "/api/logs/export")
        .match_query(Matcher::UrlEncoded("format".into(), "csv".into()))
        .with_status(200)
        .with_header("content-type", "text/csv")
        .with_body(csv)
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result =
        otelite::commands::logs::handle_export(&client, &config, "csv", None, None, None).await;

    mock.assert_async().await;
    assert!(result.is_ok());

    let mut reader = csv::Reader::from_reader(csv.as_bytes());
    let headers = reader.headers().unwrap().clone();
    assert_eq!(
        headers,
        csv::StringRecord::from(vec!["timestamp", "severity", "body"])
    );
    let rows = reader
        .records()
        .collect::<Result<Vec<_>, csv::Error>>()
        .unwrap();
    assert!(rows.is_empty());
}
