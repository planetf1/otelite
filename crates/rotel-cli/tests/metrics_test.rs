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
        no_header: false,
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
                "description": null,
                "unit": null,
                "metric_type": "counter",
                "value": 1234,
                "timestamp": 1705315800000000000,
                "attributes": {
                    "method": "GET",
                    "status": "200"
                },
                "resource": null
            },
            {
                "name": "cpu_usage_percent",
                "description": null,
                "unit": null,
                "metric_type": "gauge",
                "value": 45.2,
                "timestamp": 1705315800000000000,
                "attributes": {
                    "host": "server1"
                },
                "resource": null
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
        Some(10),
        None,
        vec![],
        None,
        None,
    )
    .await;

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

    let result =
        rotel_cli::commands::metrics::handle_list(&client, &config, None, None, vec![], None, None)
            .await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_metrics_list_with_name_filter() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/metrics")
        .match_query(mockito::Matcher::AllOf(vec![mockito::Matcher::UrlEncoded(
            "name".into(),
            "http_requests_total".into(),
        )]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "name": "http_requests_total",
                "metric_type": "counter",
                "value": 1234,
                "timestamp": 1705315800000000000,
                "attributes": {
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
        None,
        None,
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
        .mock("GET", "/api/metrics")
        .match_query(mockito::Matcher::UrlEncoded(
            "name".into(),
            "http_requests_total".into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "name": "http_requests_total",
                "description": null,
                "unit": null,
                "metric_type": "counter",
                "value": 1234,
                "timestamp": 1705315800000000000,
                "attributes": {
                    "method": "GET",
                    "status": "200"
                },
                "resource": null
            }
        ]"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::metrics::handle_show(
        &client,
        &config,
        "http_requests_total",
        vec![],
        None,
    )
    .await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_metrics_get_not_found() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/metrics")
        .match_query(mockito::Matcher::UrlEncoded(
            "name".into(),
            "nonexistent_metric".into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"[]"#)
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::metrics::handle_show(
        &client,
        &config,
        "nonexistent_metric",
        vec![],
        None,
    )
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
        .match_query(mockito::Matcher::AllOf(vec![mockito::Matcher::UrlEncoded(
            "label".into(),
            "method=GET".into(),
        )]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "name": "http_requests_total",
                "metric_type": "counter",
                "value": 1234,
                "timestamp": 1705315800000000000,
                "attributes": {
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
        None,
        None,
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
                "metric_type": "counter",
                "value": 1234,
                "timestamp": 1705315800000000000,
                "attributes": {
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
        None,
        None,
    )
    .await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_metrics_get_with_label_filter() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/metrics")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("name".into(), "http_requests_total".into()),
            mockito::Matcher::UrlEncoded("label".into(), "method=GET".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "name": "http_requests_total",
                "description": null,
                "unit": null,
                "metric_type": "counter",
                "value": 1234,
                "timestamp": 1705315800000000000,
                "attributes": {
                    "method": "GET",
                    "status": "200"
                },
                "resource": null
            }
        ]"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::metrics::handle_show(
        &client,
        &config,
        "http_requests_total",
        vec!["method=GET".to_string()],
        None,
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
        .mock("GET", "/api/metrics")
        .match_query(mockito::Matcher::UrlEncoded(
            "name".into(),
            "cpu_usage_percent".into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "name": "cpu_usage_percent",
                "description": null,
                "unit": null,
                "metric_type": "gauge",
                "value": 45.2,
                "timestamp": 1705315800000000000,
                "attributes": {
                    "host": "server1"
                },
                "resource": null
            },
            {
                "name": "cpu_usage_percent",
                "description": null,
                "unit": null,
                "metric_type": "gauge",
                "value": 52.8,
                "timestamp": 1705315860000000000,
                "attributes": {
                    "host": "server1"
                },
                "resource": null
            },
            {
                "name": "cpu_usage_percent",
                "description": null,
                "unit": null,
                "metric_type": "gauge",
                "value": 48.5,
                "timestamp": 1705315920000000000,
                "attributes": {
                    "host": "server1"
                },
                "resource": null
            }
        ]"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::metrics::handle_show(
        &client,
        &config,
        "cpu_usage_percent",
        vec![],
        None,
    )
    .await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_metrics_histogram_with_percentiles() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/metrics")
        .match_query(mockito::Matcher::UrlEncoded(
            "name".into(),
            "response_time_ms".into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "name": "response_time_ms",
                "description": null,
                "unit": null,
                "metric_type": "histogram",
                "value": {"sum": 150.5, "count": 10, "buckets": []},
                "timestamp": 1705315800000000000,
                "attributes": {
                    "endpoint": "/api/users"
                },
                "resource": null
            }
        ]"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::metrics::handle_show(
        &client,
        &config,
        "response_time_ms",
        vec![],
        None,
    )
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
                "metric_type": "counter",
                "value": 1234,
                "timestamp": 1705315800000000000,
                "attributes": {
                    "method": "GET"
                }
            }
        ]"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result =
        rotel_cli::commands::metrics::handle_list(&client, &config, None, None, vec![], None, None)
            .await;

    mock.assert_async().await;
    assert!(result.is_ok());
}
