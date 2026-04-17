//! Integration tests for metrics commands

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

// T066: Integration test for metrics list command
#[tokio::test]
async fn test_metrics_list_command() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/metrics")
        .match_query(mockito::Matcher::UrlEncoded("limit".into(), "10".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "name": "http_requests_total",
                "type": "counter",
                "value": 1234.0,
                "timestamp": "2024-01-15T10:30:00Z",
                "labels": {
                    "method": "GET",
                    "status": "200"
                }
            },
            {
                "name": "cpu_usage_percent",
                "type": "gauge",
                "value": 45.2,
                "timestamp": "2024-01-15T10:30:00Z",
                "labels": {
                    "host": "server1"
                }
            }
        ]"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result =
        rotel_cli::commands::metrics::handle_list(&client, &config, Some(10), None, vec![]).await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_metrics_list_empty() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/metrics")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"[]"#)
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::metrics::handle_list(&client, &config, None, None, vec![]).await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_metrics_list_with_name_filter() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/metrics")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("name".into(), "http_requests_total".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "name": "http_requests_total",
                "type": "counter",
                "value": 1234.0,
                "timestamp": "2024-01-15T10:30:00Z",
                "labels": {
                    "method": "GET"
                }
            }
        ]"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::metrics::handle_list(
        &client,
        &config,
        None,
        Some("http_requests_total".to_string()),
        vec![],
    )
    .await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

// T067: Integration test for metrics get command
#[tokio::test]
async fn test_metrics_get_command() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/metrics/http_requests_total")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "name": "http_requests_total",
                "type": "counter",
                "value": 1234.0,
                "timestamp": "2024-01-15T10:30:00Z",
                "labels": {
                    "method": "GET",
                    "status": "200"
                }
            }
        ]"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result =
        rotel_cli::commands::metrics::handle_get(&client, &config, "http_requests_total", vec![])
            .await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_metrics_get_not_found() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/metrics/nonexistent_metric")
        .with_status(404)
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result =
        rotel_cli::commands::metrics::handle_get(&client, &config, "nonexistent_metric", vec![])
            .await;

    mock.assert_async().await;
    assert!(result.is_err());
}

// T068: Integration test for label filtering
#[tokio::test]
async fn test_metrics_list_with_label_filter() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/metrics")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("label".into(), "method=GET".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "name": "http_requests_total",
                "type": "counter",
                "value": 1234.0,
                "timestamp": "2024-01-15T10:30:00Z",
                "labels": {
                    "method": "GET",
                    "status": "200"
                }
            }
        ]"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::metrics::handle_list(
        &client,
        &config,
        None,
        None,
        vec!["method=GET".to_string()],
    )
    .await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_metrics_list_with_multiple_label_filters() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/metrics")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "name": "http_requests_total",
                "type": "counter",
                "value": 1234.0,
                "timestamp": "2024-01-15T10:30:00Z",
                "labels": {
                    "method": "GET",
                    "status": "200"
                }
            }
        ]"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::metrics::handle_list(
        &client,
        &config,
        None,
        None,
        vec!["method=GET".to_string(), "status=200".to_string()],
    )
    .await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_metrics_get_with_label_filter() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/metrics/http_requests_total")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("label".into(), "method=GET".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "name": "http_requests_total",
                "type": "counter",
                "value": 1234.0,
                "timestamp": "2024-01-15T10:30:00Z",
                "labels": {
                    "method": "GET",
                    "status": "200"
                }
            }
        ]"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::metrics::handle_get(
        &client,
        &config,
        "http_requests_total",
        vec!["method=GET".to_string()],
    )
    .await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

// T069: Integration test for time-series JSON output
#[tokio::test]
async fn test_metrics_time_series_json_output() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/metrics/cpu_usage_percent")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "name": "cpu_usage_percent",
                "type": "gauge",
                "value": 45.2,
                "timestamp": "2024-01-15T10:30:00Z",
                "labels": {
                    "host": "server1"
                }
            },
            {
                "name": "cpu_usage_percent",
                "type": "gauge",
                "value": 52.8,
                "timestamp": "2024-01-15T10:31:00Z",
                "labels": {
                    "host": "server1"
                }
            },
            {
                "name": "cpu_usage_percent",
                "type": "gauge",
                "value": 48.5,
                "timestamp": "2024-01-15T10:32:00Z",
                "labels": {
                    "host": "server1"
                }
            }
        ]"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result =
        rotel_cli::commands::metrics::handle_get(&client, &config, "cpu_usage_percent", vec![])
            .await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_metrics_histogram_with_percentiles() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/metrics/response_time_ms")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "name": "response_time_ms",
                "type": "histogram",
                "value": 150.5,
                "timestamp": "2024-01-15T10:30:00Z",
                "labels": {
                    "endpoint": "/api/users"
                },
                "percentiles": {
                    "p50": 100.0,
                    "p95": 200.0,
                    "p99": 300.0,
                    "p99.9": 500.0
                }
            }
        ]"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result =
        rotel_cli::commands::metrics::handle_get(&client, &config, "response_time_ms", vec![])
            .await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_metrics_json_output_format() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/metrics")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "name": "http_requests_total",
                "type": "counter",
                "value": 1234.0,
                "timestamp": "2024-01-15T10:30:00Z",
                "labels": {
                    "method": "GET"
                }
            }
        ]"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::metrics::handle_list(&client, &config, None, None, vec![]).await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

// Made with Bob
