//! Integration tests for metrics commands

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

// T066: Integration test for metrics list command
#[tokio::test]
async fn test_metrics_list_command() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/metrics")
        .match_query(Matcher::UrlEncoded("limit".into(), "10".into()))
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
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result = otelite::commands::metrics::handle_list(
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
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result =
        otelite::commands::metrics::handle_list(&client, &config, None, None, vec![], None, None)
            .await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_metrics_list_with_name_filter() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/metrics")
        .match_query(Matcher::AllOf(vec![Matcher::UrlEncoded(
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
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result = otelite::commands::metrics::handle_list(
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
        .match_query(Matcher::UrlEncoded(
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
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result = otelite::commands::metrics::handle_show(
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
        .match_query(Matcher::UrlEncoded(
            "name".into(),
            "nonexistent_metric".into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"[]"#)
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result = otelite::commands::metrics::handle_show(
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
        .match_query(Matcher::AllOf(vec![Matcher::UrlEncoded(
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
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result = otelite::commands::metrics::handle_list(
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
        .match_query(Matcher::Any)
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
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result = otelite::commands::metrics::handle_list(
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
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("name".into(), "http_requests_total".into()),
            Matcher::UrlEncoded("label".into(), "method=GET".into()),
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
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result = otelite::commands::metrics::handle_show(
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
        .match_query(Matcher::UrlEncoded(
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
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result = otelite::commands::metrics::handle_show(
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
        .match_query(Matcher::UrlEncoded(
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
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result =
        otelite::commands::metrics::handle_show(&client, &config, "response_time_ms", vec![], None)
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
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result =
        otelite::commands::metrics::handle_list(&client, &config, None, None, vec![], None, None)
            .await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

// T070: Integration tests for metrics export command
#[tokio::test]
async fn test_metrics_export_json_stdout_is_valid_json_array() {
    let mut server = Server::new_async().await;
    let body = r#"[
        {
            "name": "http_requests_total",
            "description": null,
            "unit": null,
            "metric_type": "counter",
            "value": 1234,
            "timestamp": 1705315800000000000,
            "attributes": {
                "method": "GET"
            },
            "resource": null
        }
    ]"#;
    let mock = server
        .mock("GET", "/api/metrics/export")
        .match_query(Matcher::UrlEncoded("format".into(), "json".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result =
        otelite::commands::metrics::handle_export(&client, &config, "json", None, None, None).await;

    mock.assert_async().await;
    assert!(result.is_ok());

    let parsed: Vec<serde_json::Value> = serde_json::from_str(body).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0]["name"], "http_requests_total");
}

#[tokio::test]
async fn test_metrics_export_json_file_output_writes_valid_json() {
    let mut server = Server::new_async().await;
    let body = r#"[
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
    ]"#;
    let mock = server
        .mock("GET", "/api/metrics/export")
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

    let result = otelite::commands::metrics::handle_export(
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
    assert_eq!(parsed[0]["name"], "cpu_usage_percent");
}

#[tokio::test]
async fn test_metrics_export_with_data_includes_metric_name_and_values() {
    let mut server = Server::new_async().await;
    let body = r#"[
        {
            "name": "request_duration_ms",
            "description": null,
            "unit": "ms",
            "metric_type": "histogram",
            "value": {"sum": 42.5, "count": 3, "buckets": []},
            "timestamp": 1705315800000000000,
            "attributes": {
                "endpoint": "/api/orders"
            },
            "resource": null
        }
    ]"#;
    let mock = server
        .mock("GET", "/api/metrics/export")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("format".into(), "json".into()),
            Matcher::UrlEncoded("name".into(), "request_duration_ms".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result = otelite::commands::metrics::handle_export(
        &client,
        &config,
        "json",
        Some("request_duration_ms".to_string()),
        None,
        None,
    )
    .await;

    mock.assert_async().await;
    assert!(result.is_ok());

    let parsed: Vec<serde_json::Value> = serde_json::from_str(body).unwrap();
    assert_eq!(parsed[0]["name"], "request_duration_ms");
    assert_eq!(parsed[0]["value"]["count"], 3);
}

#[tokio::test]
async fn test_metrics_export_empty_result_is_empty_array() {
    let mut server = Server::new_async().await;
    let body = "[]";
    let mock = server
        .mock("GET", "/api/metrics/export")
        .match_query(Matcher::UrlEncoded("format".into(), "json".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let result =
        otelite::commands::metrics::handle_export(&client, &config, "json", None, None, None).await;

    mock.assert_async().await;
    assert!(result.is_ok());

    let parsed: Vec<serde_json::Value> = serde_json::from_str(body).unwrap();
    assert!(parsed.is_empty());
}
