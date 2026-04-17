//! HTTP client for Rotel backend API

use crate::api::models::{LogEntry, Metric, Trace};
use crate::error::{Error, Result};
use reqwest::Client;
use std::time::Duration;

/// API client for communicating with Rotel backend
pub struct ApiClient {
    client: Client,
    base_url: String,
}

impl ApiClient {
    /// Create a new API client
    pub fn new(endpoint: String, timeout: Duration) -> Result<Self> {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| Error::ConnectionError(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            client,
            base_url: endpoint,
        })
    }

    /// Fetch logs from the backend
    pub async fn fetch_logs(&self, params: Vec<(&str, String)>) -> Result<Vec<LogEntry>> {
        let url = format!("{}/api/logs", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to fetch logs: HTTP {}",
                response.status()
            )));
        }

        let logs = response.json().await?;
        Ok(logs)
    }

    /// Fetch a single log by ID
    pub async fn fetch_log_by_id(&self, id: &str) -> Result<LogEntry> {
        let url = format!("{}/api/logs/{}", self.base_url, id);
        let response = self.client.get(&url).send().await?;

        if response.status().as_u16() == 404 {
            return Err(Error::NotFound(format!("Log '{}' not found", id)));
        }

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to fetch log: HTTP {}",
                response.status()
            )));
        }

        let log = response.json().await?;
        Ok(log)
    }

    /// Search logs by query string
    pub async fn search_logs(
        &self,
        query: &str,
        params: Vec<(&str, String)>,
    ) -> Result<Vec<LogEntry>> {
        let url = format!("{}/api/logs/search", self.base_url);
        let mut all_params = vec![("q", query.to_string())];
        all_params.extend(params);

        let response = self.client.get(&url).query(&all_params).send().await?;

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to search logs: HTTP {}",
                response.status()
            )));
        }

        let logs = response.json().await?;
        Ok(logs)
    }

    /// Fetch traces from the backend
    pub async fn fetch_traces(&self, params: Vec<(&str, String)>) -> Result<Vec<Trace>> {
        let url = format!("{}/api/traces", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to fetch traces: HTTP {}",
                response.status()
            )));
        }

        let traces = response.json().await?;
        Ok(traces)
    }

    /// Fetch a single trace by ID
    pub async fn fetch_trace_by_id(&self, id: &str) -> Result<Trace> {
        let url = format!("{}/api/traces/{}", self.base_url, id);
        let response = self.client.get(&url).send().await?;

        if response.status().as_u16() == 404 {
            return Err(Error::NotFound(format!("Trace '{}' not found", id)));
        }

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to fetch trace: HTTP {}",
                response.status()
            )));
        }

        let trace = response.json().await?;
        Ok(trace)
    }

    /// Fetch metrics from the backend
    pub async fn fetch_metrics(&self, params: Vec<(&str, String)>) -> Result<Vec<Metric>> {
        let url = format!("{}/api/metrics", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to fetch metrics: HTTP {}",
                response.status()
            )));
        }

        let metrics = response.json().await?;
        Ok(metrics)
    }

    /// Fetch a single metric by name
    pub async fn fetch_metric_by_name(
        &self,
        name: &str,
        params: Vec<(&str, String)>,
    ) -> Result<Vec<Metric>> {
        let url = format!("{}/api/metrics/{}", self.base_url, name);
        let response = self.client.get(&url).query(&params).send().await?;

        if response.status().as_u16() == 404 {
            return Err(Error::NotFound(format!("Metric '{}' not found", name)));
        }

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to fetch metric: HTTP {}",
                response.status()
            )));
        }

        let metrics = response.json().await?;
        Ok(metrics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[test]
    fn test_api_client_creation() {
        let client = ApiClient::new("http://localhost:8080".to_string(), Duration::from_secs(30));
        assert!(client.is_ok());
    }

    #[test]
    fn test_api_client_invalid_timeout() {
        // Very short timeout should still create client successfully
        let client = ApiClient::new(
            "http://localhost:8080".to_string(),
            Duration::from_millis(1),
        );
        assert!(client.is_ok());
    }

    // T014: Unit test for ApiClient::fetch_logs
    #[tokio::test]
    async fn test_fetch_logs_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/logs")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"[
                {
                    "id": "log1",
                    "timestamp": "2024-01-15T10:30:00Z",
                    "severity": "INFO",
                    "message": "Test log message",
                    "attributes": {}
                }
            ]"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let params = vec![("limit", "10".to_string())];
        let result = client.fetch_logs(params).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let logs = result.unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].id, "log1");
        assert_eq!(logs[0].severity, "INFO");
    }

    #[tokio::test]
    async fn test_fetch_logs_empty_response() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/logs")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("[]")
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_logs(vec![]).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_fetch_logs_server_error() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/logs")
            .with_status(500)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_logs(vec![]).await;

        mock.assert_async().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::ApiError(msg) => assert!(msg.contains("500")),
            _ => panic!("Expected ApiError"),
        }
    }

    // T015: Unit test for ApiClient::fetch_log_by_id
    #[tokio::test]
    async fn test_fetch_log_by_id_success() {
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
                "message": "Error occurred",
                "attributes": {"key": "value"}
            }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_log_by_id("log123").await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let log = result.unwrap();
        assert_eq!(log.id, "log123");
        assert_eq!(log.severity, "ERROR");
        assert_eq!(log.message, "Error occurred");
    }

    #[tokio::test]
    async fn test_fetch_log_by_id_not_found() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/logs/nonexistent")
            .with_status(404)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_log_by_id("nonexistent").await;

        mock.assert_async().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::NotFound(msg) => assert!(msg.contains("nonexistent")),
            _ => panic!("Expected NotFound error"),
        }
    }

    // T016: Unit test for ApiClient::search_logs
    #[tokio::test]
    async fn test_search_logs_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/logs/search")
            .match_query(mockito::Matcher::AllOf(vec![mockito::Matcher::UrlEncoded(
                "q".into(),
                "error".into(),
            )]))
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

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.search_logs("error", vec![]).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let logs = result.unwrap();
        assert_eq!(logs.len(), 2);
        assert!(logs[0].message.contains("Error"));
        assert!(logs[1].message.contains("error"));
    }

    #[tokio::test]
    async fn test_search_logs_no_results() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/logs/search")
            .match_query(mockito::Matcher::UrlEncoded(
                "q".into(),
                "nonexistent".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("[]")
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.search_logs("nonexistent", vec![]).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_search_logs_with_filters() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/logs/search")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("q".into(), "error".into()),
                mockito::Matcher::UrlEncoded("severity".into(), "ERROR".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("[]")
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let params = vec![("severity", "ERROR".to_string())];
        let result = client.search_logs("error", params).await;

        mock.assert_async().await;
        assert!(result.is_ok());
    }

    // T038: Unit test for ApiClient::fetch_traces
    #[tokio::test]
    async fn test_fetch_traces_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/traces")
            .match_query(mockito::Matcher::Any)
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
                }
            ]"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let params = vec![("limit", "10".to_string())];
        let result = client.fetch_traces(params).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let traces = result.unwrap();
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].id, "trace1");
        assert_eq!(traces[0].root_span, "http-request");
        assert_eq!(traces[0].duration_ms, 1500);
        assert_eq!(traces[0].status, "OK");
    }

    #[tokio::test]
    async fn test_fetch_traces_with_filters() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/traces")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("min_duration".into(), "1000".into()),
                mockito::Matcher::UrlEncoded("status".into(), "ERROR".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"[]"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let params = vec![
            ("min_duration", "1000".to_string()),
            ("status", "ERROR".to_string()),
        ];
        let result = client.fetch_traces(params).await;

        mock.assert_async().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_fetch_traces_error() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/traces")
            .with_status(500)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_traces(vec![]).await;

        mock.assert_async().await;
        assert!(result.is_err());
        match result {
            Err(Error::ApiError(msg)) => assert!(msg.contains("500")),
            _ => panic!("Expected ApiError"),
        }
    }

    // T039: Unit test for ApiClient::fetch_trace_by_id
    #[tokio::test]
    async fn test_fetch_trace_by_id_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/traces/trace123")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "id": "trace123",
                "root_span": "database-query",
                "duration_ms": 250,
                "status": "OK",
                "spans": [
                    {
                        "id": "span1",
                        "name": "database-query",
                        "parent_id": null,
                        "start_time": "2024-01-15T10:30:00Z",
                        "duration_ms": 250,
                        "attributes": {"db.system": "postgresql"}
                    },
                    {
                        "id": "span2",
                        "name": "query-execution",
                        "parent_id": "span1",
                        "start_time": "2024-01-15T10:30:00.100Z",
                        "duration_ms": 150,
                        "attributes": {}
                    }
                ]
            }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_trace_by_id("trace123").await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let trace = result.unwrap();
        assert_eq!(trace.id, "trace123");
        assert_eq!(trace.root_span, "database-query");
        assert_eq!(trace.duration_ms, 250);
        assert_eq!(trace.spans.len(), 2);
    }

    #[tokio::test]
    async fn test_fetch_trace_by_id_not_found() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/traces/nonexistent")
            .with_status(404)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_trace_by_id("nonexistent").await;

        mock.assert_async().await;
        assert!(result.is_err());
        match result {
            Err(Error::NotFound(msg)) => assert!(msg.contains("nonexistent")),
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_fetch_trace_by_id_error() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/traces/trace123")
            .with_status(500)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_trace_by_id("trace123").await;

        mock.assert_async().await;
        assert!(result.is_err());
        match result {
            Err(Error::ApiError(msg)) => assert!(msg.contains("500")),
            _ => panic!("Expected ApiError"),
        }
    }
}

// Made with Bob
