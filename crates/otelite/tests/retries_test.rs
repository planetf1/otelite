//! Integration tests for the retries command

use mockito::{Matcher, Server};
use std::time::Duration;

// Helper to create test config. The command builds its own ApiClient from
// `config.endpoint`, so tests point it at the mock server via the config.
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

#[tokio::test]
async fn test_retries_sends_window_and_limit() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/genai/retries")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("limit".into(), "5".into()),
            Matcher::UrlEncoded("model".into(), "claude-sonnet".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"items": [
                {
                    "trace_id": "abc123def456abc123def456abc123de",
                    "span_id": "span1",
                    "start_time": 1705315800000000000,
                    "duration": 12403000000,
                    "model": "claude-sonnet-5[1m]",
                    "system": "anthropic",
                    "session_id": "8c372ccc-bbc9-403c-98e6-b63c6e17a346",
                    "attempt": 2,
                    "ttft_ms": 11728,
                    "finish_reason": "tool_use"
                }
            ], "filters_applied": ["model"]}"#,
        )
        .create_async()
        .await;

    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let cmd = otelite::commands::retries::RetriesCommand {
        since: "24h".to_string(),
        limit: 5,
        model: Some("claude-sonnet".to_string()),
        session: None,
    };
    let result = cmd.execute(config).await;

    mock.assert_async().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_retries_empty_window_is_ok() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/api/genai/retries")
        .match_query(Matcher::UrlEncoded("limit".into(), "20".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"items": [], "filters_applied": []}"#)
        .create_async()
        .await;

    let config = create_test_config(server.url(), otelite::config::OutputFormat::Json);

    let cmd = otelite::commands::retries::RetriesCommand {
        since: "1h".to_string(),
        limit: 20,
        model: None,
        session: None,
    };
    let result = cmd.execute(config).await;

    mock.assert_async().await;
    assert!(result.is_ok());
}
