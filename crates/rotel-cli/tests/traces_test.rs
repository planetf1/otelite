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
            r#"[
            {
                "id": "trace1",
                "root_span": "http-request",
                "duration_ms": 1500,
                "status": "OK",
                "spans": [
                    {
                        "id": "span1",
                        "name": "http-request",
                        "parent_id": null,
                        "start_time": "2024-01-15T10:30:00Z",
                        "duration_ms": 1500,
                        "attributes": {}
                    }
                ]
            },
            {
                "id": "trace2",
                "root_span": "database-query",
                "duration_ms": 250,
                "status": "OK",
                "spans": [
                    {
                        "id": "span2",
                        "name": "database-query",
                        "parent_id": null,
                        "start_time": "2024-01-15T10:31:00Z",
                        "duration_ms": 250,
                        "attributes": {}
                    }
                ]
            }
        ]"#,
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
        .with_body(r#"[]"#)
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
            "id": "trace123",
            "root_span": "http-request",
            "duration_ms": 1500,
            "status": "OK",
            "spans": [
                {
                    "id": "span1",
                    "name": "http-request",
                    "parent_id": null,
                    "start_time": "2024-01-15T10:30:00Z",
                    "duration_ms": 1500,
                    "attributes": {"http.method": "GET"}
                },
                {
                    "id": "span2",
                    "name": "database-query",
                    "parent_id": "span1",
                    "start_time": "2024-01-15T10:30:00.100Z",
                    "duration_ms": 250,
                    "attributes": {"db.system": "postgresql"}
                }
            ]
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
            r#"[
            {
                "id": "trace1",
                "root_span": "slow-request",
                "duration_ms": 2000,
                "status": "OK",
                "spans": []
            }
        ]"#,
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
        .match_query(mockito::Matcher::UrlEncoded("status".into(), "ERROR".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "id": "trace1",
                "root_span": "failed-request",
                "duration_ms": 500,
                "status": "ERROR",
                "spans": []
            }
        ]"#,
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
            "id": "trace123",
            "root_span": "http-request",
            "duration_ms": 1500,
            "status": "OK",
            "spans": [
                {
                    "id": "span1",
                    "name": "http-request",
                    "parent_id": null,
                    "start_time": "2024-01-15T10:30:00Z",
                    "duration_ms": 1500,
                    "attributes": {}
                },
                {
                    "id": "span2",
                    "name": "middleware",
                    "parent_id": "span1",
                    "start_time": "2024-01-15T10:30:00.100Z",
                    "duration_ms": 1000,
                    "attributes": {}
                },
                {
                    "id": "span3",
                    "name": "handler",
                    "parent_id": "span2",
                    "start_time": "2024-01-15T10:30:00.200Z",
                    "duration_ms": 800,
                    "attributes": {}
                },
                {
                    "id": "span4",
                    "name": "database-query",
                    "parent_id": "span3",
                    "start_time": "2024-01-15T10:30:00.300Z",
                    "duration_ms": 250,
                    "attributes": {}
                }
            ]
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
            "id": "trace123",
            "root_span": "http-request",
            "duration_ms": 1500,
            "status": "OK",
            "spans": [
                {
                    "id": "span1",
                    "name": "http-request",
                    "parent_id": null,
                    "start_time": "2024-01-15T10:30:00Z",
                    "duration_ms": 1500,
                    "attributes": {"http.method": "GET", "http.url": "/api/users"}
                },
                {
                    "id": "span2",
                    "name": "database-query",
                    "parent_id": "span1",
                    "start_time": "2024-01-15T10:30:00.100Z",
                    "duration_ms": 250,
                    "attributes": {"db.system": "postgresql", "db.statement": "SELECT * FROM users"}
                }
            ]
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
            r#"[
            {
                "id": "trace1",
                "root_span": "http-request",
                "duration_ms": 1500,
                "status": "OK",
                "spans": []
            }
        ]"#,
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