//! Integration tests for traces commands

use mockito::Server;
use std::time::Duration;

// Helper to create a mock API client
async fn create_test_client(server_url: String) -> rotel_cli::api::client::ApiClient {
    rotel_cli::api::client::ApiClient::new(server_url, Duration::from_secs(30)).unwrap()
}

// Helper to create test config
fn create_test_config(
    endpoint: String,
    format: rotel_cli::config::OutputFormat,
) -> rotel_cli::config::Config {
    rotel_cli::config::Config {
        endpoint,
        timeout: Duration::from_secs(30),
        format,
        no_color: true, // Disable colors for testing
        no_header: false,
    }
}

// T044: Integration test for traces list command
#[tokio::test]
async fn test_traces_list_command() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/traces")
        .match_query(mockito::Matcher::UrlEncoded("limit".into(), "10".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"traces": [
            {
                "trace_id": "trace1",
                "root_span_name": "http-request",
                "start_time": 1705315800000000000,
                "duration": 1500000000,
                "span_count": 1,
                "service_names": [],
                "has_errors": false
            },
            {
                "trace_id": "trace2",
                "root_span_name": "database-query",
                "start_time": 1705315860000000000,
                "duration": 250000000,
                "span_count": 1,
                "service_names": [],
                "has_errors": false
            }
            ], "total": 2, "limit": 10, "offset": 0}"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result =
        rotel_cli::commands::traces::handle_list(&client, &config, Some(10), None, None).await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_traces_list_empty() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/traces")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"traces": [], "total": 0, "limit": 100, "offset": 0}"#)
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::traces::handle_list(&client, &config, None, None, None).await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

// T045: Integration test for traces show command
#[tokio::test]
async fn test_traces_show_command() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/traces/trace123")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
            "trace_id": "trace123",
            "spans": [
                {
                    "span_id": "span1",
                    "trace_id": "trace123",
                    "parent_span_id": null,
                    "name": "http-request",
                    "kind": "Internal",
                    "start_time": 1705315800000000000,
                    "end_time": 1705315801500000000,
                    "duration": 1500000000,
                    "attributes": {"http.method": "GET"},
                    "resource": null,
                    "status": {"code": "OK", "message": null},
                    "events": []
                },
                {
                    "span_id": "span2",
                    "trace_id": "trace123",
                    "parent_span_id": "span1",
                    "name": "database-query",
                    "kind": "Internal",
                    "start_time": 1705315800100000000,
                    "end_time": 1705315800350000000,
                    "duration": 250000000,
                    "attributes": {"db.system": "postgresql"},
                    "resource": null,
                    "status": {"code": "OK", "message": null},
                    "events": []
                }
            ],
            "start_time": 1705315800000000000,
            "end_time": 1705315801500000000,
            "duration": 1500000000,
            "span_count": 2,
            "service_names": []
        }"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::traces::handle_show(&client, &config, "trace123").await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_traces_show_not_found() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/traces/nonexistent")
        .with_status(404)
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::traces::handle_show(&client, &config, "nonexistent").await;

    mock.assert_async().await;
    assert!(result.is_err());
}

// T046: Integration test for duration filtering
#[tokio::test]
async fn test_traces_list_with_duration_filter() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/traces")
        .match_query(mockito::Matcher::UrlEncoded(
            "min_duration".into(),
            "1000".into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"traces": [
            {
                "trace_id": "trace1",
                "root_span_name": "slow-request",
                "start_time": 1705315800000000000,
                "duration": 2000000000,
                "span_count": 1,
                "service_names": [],
                "has_errors": false
            }
            ], "total": 1, "limit": 100, "offset": 0}"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result =
        rotel_cli::commands::traces::handle_list(&client, &config, None, Some(1000), None).await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_traces_list_with_status_filter() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/traces")
        .match_query(mockito::Matcher::UrlEncoded(
            "status".into(),
            "ERROR".into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"traces": [
            {
                "trace_id": "trace1",
                "root_span_name": "failed-request",
                "start_time": 1705315800000000000,
                "duration": 500000000,
                "span_count": 1,
                "service_names": [],
                "has_errors": true
            }
            ], "total": 1, "limit": 100, "offset": 0}"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::traces::handle_list(
        &client,
        &config,
        None,
        None,
        Some("ERROR".to_string()),
    )
    .await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

// T047: Integration test for span tree format output
#[tokio::test]
async fn test_traces_show_with_span_tree() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/traces/trace123")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
            "trace_id": "trace123",
            "spans": [
                {"span_id": "span1", "trace_id": "trace123", "parent_span_id": null, "name": "http-request", "kind": "Internal", "start_time": 1705315800000000000, "end_time": 1705315801500000000, "duration": 1500000000, "attributes": {}, "resource": null, "status": {"code": "OK", "message": null}, "events": []},
                {"span_id": "span2", "trace_id": "trace123", "parent_span_id": "span1", "name": "middleware", "kind": "Internal", "start_time": 1705315800100000000, "end_time": 1705315801100000000, "duration": 1000000000, "attributes": {}, "resource": null, "status": {"code": "OK", "message": null}, "events": []},
                {"span_id": "span3", "trace_id": "trace123", "parent_span_id": "span2", "name": "handler", "kind": "Internal", "start_time": 1705315800200000000, "end_time": 1705315801000000000, "duration": 800000000, "attributes": {}, "resource": null, "status": {"code": "OK", "message": null}, "events": []},
                {"span_id": "span4", "trace_id": "trace123", "parent_span_id": "span3", "name": "database-query", "kind": "Internal", "start_time": 1705315800300000000, "end_time": 1705315800550000000, "duration": 250000000, "attributes": {}, "resource": null, "status": {"code": "OK", "message": null}, "events": []}
            ],
            "start_time": 1705315800000000000,
            "end_time": 1705315801500000000,
            "duration": 1500000000,
            "span_count": 4,
            "service_names": []
        }"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Pretty);

    let result = rotel_cli::commands::traces::handle_show(&client, &config, "trace123").await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

// T048: Integration test for JSON output with complete span structure
#[tokio::test]
async fn test_traces_json_output_with_spans() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/traces/trace123")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
            "trace_id": "trace123",
            "spans": [
                {"span_id": "span1", "trace_id": "trace123", "parent_span_id": null, "name": "http-request", "kind": "Internal", "start_time": 1705315800000000000, "end_time": 1705315801500000000, "duration": 1500000000, "attributes": {"http.method": "GET", "http.url": "/api/users"}, "resource": null, "status": {"code": "OK", "message": null}, "events": []},
                {"span_id": "span2", "trace_id": "trace123", "parent_span_id": "span1", "name": "database-query", "kind": "Internal", "start_time": 1705315800100000000, "end_time": 1705315800350000000, "duration": 250000000, "attributes": {"db.system": "postgresql", "db.statement": "SELECT * FROM users"}, "resource": null, "status": {"code": "OK", "message": null}, "events": []}
            ],
            "start_time": 1705315800000000000,
            "end_time": 1705315801500000000,
            "duration": 1500000000,
            "span_count": 2,
            "service_names": []
        }"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::traces::handle_show(&client, &config, "trace123").await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_traces_list_pretty_output() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/traces")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"traces": [
            {
                "trace_id": "trace1",
                "root_span_name": "http-request",
                "start_time": 1705315800000000000,
                "duration": 1500000000,
                "span_count": 1,
                "service_names": [],
                "has_errors": false
            }
            ], "total": 1, "limit": 100, "offset": 0}"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Pretty);

    let result = rotel_cli::commands::traces::handle_list(&client, &config, None, None, None).await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

// Made with Bob
