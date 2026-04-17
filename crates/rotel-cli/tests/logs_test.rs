//! Integration tests for logs commands

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

// T020: Integration test for logs list command
#[tokio::test]
async fn test_logs_list_command() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/logs")
        .match_query(mockito::Matcher::UrlEncoded("limit".into(), "10".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "id": "log1",
                "timestamp": "2024-01-15T10:30:00Z",
                "severity": "INFO",
                "message": "Test log 1",
                "attributes": {}
            },
            {
                "id": "log2",
                "timestamp": "2024-01-15T10:31:00Z",
                "severity": "ERROR",
                "message": "Test log 2",
                "attributes": {}
            }
        ]"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result =
        rotel_cli::commands::logs::handle_list(&client, &config, Some(10), None, None).await;

    mock.assert_async().await;
    assert!(result.is_ok());
    let logs = result.unwrap();
    assert_eq!(logs.len(), 2);
    assert_eq!(logs[0].id, "log1");
    assert_eq!(logs[1].id, "log2");
}

#[tokio::test]
async fn test_logs_list_with_severity_filter() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/logs")
        .match_query(mockito::Matcher::UrlEncoded(
            "severity".into(),
            "ERROR".into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "id": "log1",
                "timestamp": "2024-01-15T10:30:00Z",
                "severity": "ERROR",
                "message": "Error message",
                "attributes": {}
            }
        ]"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::logs::handle_list(
        &client,
        &config,
        None,
        Some("ERROR".to_string()),
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
        .with_body("[]")
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::logs::handle_list(&client, &config, None, None, None).await;

    mock.assert_async().await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 0);
}

// T021: Integration test for logs search command
#[tokio::test]
async fn test_logs_search_command() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/logs/search")
        .match_query(mockito::Matcher::UrlEncoded("q".into(), "error".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "id": "log1",
                "timestamp": "2024-01-15T10:30:00Z",
                "severity": "ERROR",
                "message": "Error in processing",
                "attributes": {}
            },
            {
                "id": "log2",
                "timestamp": "2024-01-15T10:31:00Z",
                "severity": "ERROR",
                "message": "Another error",
                "attributes": {}
            }
        ]"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result =
        rotel_cli::commands::logs::handle_search(&client, &config, "error", None, None).await;

    mock.assert_async().await;
    assert!(result.is_ok());
    let logs = result.unwrap();
    assert_eq!(logs.len(), 2);
    assert!(logs[0].message.contains("Error"));
}

#[tokio::test]
async fn test_logs_search_with_limit() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/logs/search")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("q".into(), "test".into()),
            mockito::Matcher::UrlEncoded("limit".into(), "5".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[]")
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result =
        rotel_cli::commands::logs::handle_search(&client, &config, "test", Some(5), None).await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

// T022: Integration test for logs show command
#[tokio::test]
async fn test_logs_show_command() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/logs/log123")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
            "id": "log123",
            "timestamp": "2024-01-15T10:30:00Z",
            "severity": "ERROR",
            "message": "Detailed error message",
            "attributes": {
                "user_id": "12345",
                "request_id": "abc-def"
            }
        }"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::logs::handle_show(&client, &config, "log123").await;

    mock.assert_async().await;
    assert!(result.is_ok());
    let log = result.unwrap();
    assert_eq!(log.id, "log123");
    assert_eq!(log.severity, "ERROR");
    assert_eq!(log.attributes.len(), 2);
}

#[tokio::test]
async fn test_logs_show_not_found() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/logs/nonexistent")
        .with_status(404)
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::logs::handle_show(&client, &config, "nonexistent").await;

    mock.assert_async().await;
    assert!(result.is_err());
    match result.unwrap_err() {
        rotel_cli::error::Error::NotFound(_) => {}, // Expected
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
            r#"[
            {
                "id": "log1",
                "timestamp": "2024-01-15T10:30:00Z",
                "severity": "INFO",
                "message": "Test message",
                "attributes": {}
            }
        ]"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    let result = rotel_cli::commands::logs::handle_list(&client, &config, None, None, None).await;

    mock.assert_async().await;
    assert!(result.is_ok());

    // Verify JSON serialization works
    let logs = result.unwrap();
    let json_str = serde_json::to_string(&logs).unwrap();
    let parsed: Vec<rotel_cli::api::models::LogEntry> = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].id, "log1");
}

#[tokio::test]
async fn test_pretty_output_format() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/logs")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "id": "log1",
                "timestamp": "2024-01-15T10:30:00Z",
                "severity": "INFO",
                "message": "Test message",
                "attributes": {}
            }
        ]"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Pretty);

    let result = rotel_cli::commands::logs::handle_list(&client, &config, None, None, None).await;

    mock.assert_async().await;
    assert!(result.is_ok());
    // Pretty format should not panic
}

// T024: Integration test for severity filtering
#[tokio::test]
async fn test_severity_filtering_integration() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/logs")
        .match_query(mockito::Matcher::UrlEncoded(
            "severity".into(),
            "WARN".into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
            {
                "id": "log1",
                "timestamp": "2024-01-15T10:30:00Z",
                "severity": "ERROR",
                "message": "Error",
                "attributes": {}
            },
            {
                "id": "log2",
                "timestamp": "2024-01-15T10:31:00Z",
                "severity": "WARN",
                "message": "Warning",
                "attributes": {}
            },
            {
                "id": "log3",
                "timestamp": "2024-01-15T10:32:00Z",
                "severity": "INFO",
                "message": "Info",
                "attributes": {}
            },
            {
                "id": "log4",
                "timestamp": "2024-01-15T10:33:00Z",
                "severity": "DEBUG",
                "message": "Debug",
                "attributes": {}
            }
        ]"#,
        )
        .create_async()
        .await;

    let client = create_test_client(server.url()).await;
    let config = create_test_config(server.url(), rotel_cli::config::OutputFormat::Json);

    // Test filtering at WARN level (should include ERROR and WARN)
    let result = rotel_cli::commands::logs::handle_list(
        &client,
        &config,
        None,
        Some("WARN".to_string()),
        None,
    )
    .await;

    mock.assert_async().await;
    assert!(result.is_ok());
    let logs = result.unwrap();

    // Client-side filtering should keep ERROR and WARN, filter out INFO and DEBUG
    assert_eq!(logs.len(), 2);
    assert!(logs.iter().any(|l| l.severity == "ERROR"));
    assert!(logs.iter().any(|l| l.severity == "WARN"));
    assert!(!logs.iter().any(|l| l.severity == "INFO"));
    assert!(!logs.iter().any(|l| l.severity == "DEBUG"));
}

// Made with Bob
