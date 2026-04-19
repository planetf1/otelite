//! HTTP client for Rotel backend API

use crate::api::models::{LogEntry, LogsResponse, MetricResponse, TraceDetail, TracesResponse};
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
    pub async fn fetch_logs(&self, params: Vec<(&str, String)>) -> Result<LogsResponse> {
        let url = format!("{}/api/logs", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to fetch logs: HTTP {}",
                response.status()
            )));
        }

        let logs_response = response.json().await?;
        Ok(logs_response)
    }

    /// Fetch a single log by timestamp
    pub async fn fetch_log_by_id(&self, timestamp: i64) -> Result<LogEntry> {
        let url = format!("{}/api/logs/{}", self.base_url, timestamp);
        let response = self.client.get(&url).send().await?;

        if response.status().as_u16() == 404 {
            return Err(Error::NotFound(format!(
                "Log at timestamp '{}' not found",
                timestamp
            )));
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

    /// Search logs by query string (uses the same endpoint as fetch_logs with search param)
    pub async fn search_logs(
        &self,
        query: &str,
        params: Vec<(&str, String)>,
    ) -> Result<LogsResponse> {
        let url = format!("{}/api/logs", self.base_url);
        let mut all_params = vec![("search", query.to_string())];
        all_params.extend(params);

        let response = self.client.get(&url).query(&all_params).send().await?;

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to search logs: HTTP {}",
                response.status()
            )));
        }

        let logs_response = response.json().await?;
        Ok(logs_response)
    }

    /// Fetch traces from the backend
    pub async fn fetch_traces(&self, params: Vec<(&str, String)>) -> Result<TracesResponse> {
        let url = format!("{}/api/traces", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to fetch traces: HTTP {}",
                response.status()
            )));
        }

        let traces_response = response.json().await?;
        Ok(traces_response)
    }

    /// Fetch a single trace by ID
    pub async fn fetch_trace_by_id(&self, id: &str) -> Result<TraceDetail> {
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
    pub async fn fetch_metrics(&self, params: Vec<(&str, String)>) -> Result<Vec<MetricResponse>> {
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
    ) -> Result<Vec<MetricResponse>> {
        let url = format!("{}/api/metrics", self.base_url);
        let mut all_params = vec![("name", name.to_string())];
        all_params.extend(params);

        let response = self.client.get(&url).query(&all_params).send().await?;

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to fetch metric: HTTP {}",
                response.status()
            )));
        }

        let metrics: Vec<MetricResponse> = response.json().await?;

        if metrics.is_empty() {
            return Err(Error::NotFound(format!("Metric '{}' not found", name)));
        }

        Ok(metrics)
    }

    /// Export logs from the backend
    pub async fn export_logs(&self, params: Vec<(&str, String)>) -> Result<String> {
        let url = format!("{}/api/logs/export", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to export logs: HTTP {}",
                response.status()
            )));
        }

        let body = response.text().await?;
        Ok(body)
    }

    /// Export traces from the backend
    pub async fn export_traces(&self, params: Vec<(&str, String)>) -> Result<String> {
        let url = format!("{}/api/traces/export", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to export traces: HTTP {}",
                response.status()
            )));
        }

        let body = response.text().await?;
        Ok(body)
    }

    /// Export metrics from the backend
    pub async fn export_metrics(&self, params: Vec<(&str, String)>) -> Result<String> {
        let url = format!("{}/api/metrics/export", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to export metrics: HTTP {}",
                response.status()
            )));
        }

        let body = response.text().await?;
        Ok(body)
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
                r#"{
                "logs": [
                {
                    "timestamp": 1705315800000000000,
                    "severity": "INFO",
                    "severity_text": "INFO",
                    "body": "Test log message",
                    "attributes": {},
                    "resource_attributes": {},
                    "scope_name": "test",
                    "trace_id": null,
                    "span_id": null
                }
                ],
                "total": 1,
                "limit": 10,
                "offset": 0
            }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let params = vec![("limit", "10".to_string())];
        let result = client.fetch_logs(params).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let logs = result.unwrap();
        assert_eq!(logs.logs.len(), 1);
        assert_eq!(logs.logs[0].timestamp, 1705315800000000000);
        assert_eq!(logs.logs[0].severity, "INFO");
    }

    #[tokio::test]
    async fn test_fetch_logs_empty_response() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/logs")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"logs": [], "total": 0, "limit": 100, "offset": 0}"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_logs(vec![]).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().logs.len(), 0);
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
            .mock("GET", "/api/logs/1705315800000000000")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "timestamp": 1705315800000000000,
                "severity": "ERROR",
                "severity_text": "ERROR",
                "body": "Error occurred",
                "attributes": {"key": "value"},
                "resource_attributes": {},
                "scope_name": "test",
                "trace_id": null,
                "span_id": null
            }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_log_by_id(1705315800000000000).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let log = result.unwrap();
        assert_eq!(log.timestamp, 1705315800000000000);
        assert_eq!(log.severity, "ERROR");
        assert_eq!(log.body, "Error occurred");
    }

    #[tokio::test]
    async fn test_fetch_log_by_id_not_found() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/logs/9999999999999999")
            .with_status(404)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_log_by_id(9999999999999999).await;

        mock.assert_async().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::NotFound(msg) => assert!(msg.contains("9999999999999999")),
            _ => panic!("Expected NotFound error"),
        }
    }

    // T016: Unit test for ApiClient::search_logs
    #[tokio::test]
    async fn test_search_logs_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/logs")
            .match_query(mockito::Matcher::AllOf(vec![mockito::Matcher::UrlEncoded(
                "search".into(),
                "error".into(),
            )]))
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

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.search_logs("error", vec![]).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let logs = result.unwrap();
        assert_eq!(logs.logs.len(), 2);
        assert!(logs.logs[0].body.contains("Error"));
        assert!(logs.logs[1].body.contains("error"));
    }

    #[tokio::test]
    async fn test_search_logs_no_results() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/logs")
            .match_query(mockito::Matcher::UrlEncoded(
                "search".into(),
                "nonexistent".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"logs": [], "total": 0, "limit": 100, "offset": 0}"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.search_logs("nonexistent", vec![]).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().logs.len(), 0);
    }

    #[tokio::test]
    async fn test_search_logs_with_filters() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/logs")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("search".into(), "error".into()),
                mockito::Matcher::UrlEncoded("severity".into(), "ERROR".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"logs": [], "total": 0, "limit": 100, "offset": 0}"#)
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
                r#"{
                "traces": [
                {
                    "trace_id": "trace1",
                    "root_span_name": "http-request",
                    "start_time": 1705315800000000000,
                    "duration": 1500000000,
                    "span_count": 1,
                    "service_names": [],
                    "has_errors": false
                }
                ],
                "total": 1,
                "limit": 10,
                "offset": 0
            }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let params = vec![("limit", "10".to_string())];
        let result = client.fetch_traces(params).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let traces = result.unwrap();
        assert_eq!(traces.traces.len(), 1);
        assert_eq!(traces.traces[0].trace_id, "trace1");
        assert_eq!(traces.traces[0].root_span_name, "http-request");
        assert_eq!(traces.traces[0].duration, 1500000000);
        assert!(!traces.traces[0].has_errors);
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
            .with_body(r#"{"traces": [], "total": 0, "limit": 100, "offset": 0}"#)
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
                "trace_id": "trace123",
                "spans": [
                    {
                        "span_id": "span1",
                        "trace_id": "trace123",
                        "parent_span_id": null,
                        "name": "database-query",
                        "kind": "Internal",
                        "start_time": 1705315800000000000,
                        "end_time": 1705315800250000000,
                        "duration": 250000000,
                        "attributes": {"db.system": "postgresql"},
                        "resource": null,
                        "status": {"code": "OK", "message": null},
                        "events": []
                    },
                    {
                        "span_id": "span2",
                        "trace_id": "trace123",
                        "parent_span_id": "span1",
                        "name": "query-execution",
                        "kind": "Internal",
                        "start_time": 1705315800100000000,
                        "end_time": 1705315800250000000,
                        "duration": 150000000,
                        "attributes": {},
                        "resource": null,
                        "status": {"code": "OK", "message": null},
                        "events": []
                    }
                ],
                "start_time": 1705315800000000000,
                "end_time": 1705315800250000000,
                "duration": 250000000,
                "span_count": 2,
                "service_names": []
            }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_trace_by_id("trace123").await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let trace = result.unwrap();
        assert_eq!(trace.trace_id, "trace123");
        assert_eq!(trace.duration, 250000000);
        assert_eq!(trace.spans.len(), 2);
        assert_eq!(trace.spans[0].name, "database-query");
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
    // T061: Unit test for ApiClient::fetch_metrics
    #[tokio::test]
    async fn test_fetch_metrics_success() {
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
                    "description": null,
                    "unit": null,
                    "metric_type": "counter",
                    "value": 1234,
                    "timestamp": 1705315800000000000,
                    "attributes": {"method": "GET", "status": "200"},
                    "resource": null
                },
                {
                    "name": "response_time_ms",
                    "description": null,
                    "unit": null,
                    "metric_type": "histogram",
                    "value": {"sum": 150.5, "count": 10, "buckets": []},
                    "timestamp": 1705315800000000000,
                    "attributes": {},
                    "resource": null
                }
            ]"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let params = vec![("limit", "10".to_string())];
        let result = client.fetch_metrics(params).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let metrics = result.unwrap();
        assert_eq!(metrics.len(), 2);
        assert_eq!(metrics[0].name, "http_requests_total");
        assert_eq!(metrics[0].metric_type, "counter");
        assert_eq!(metrics[1].name, "response_time_ms");
        assert_eq!(metrics[1].metric_type, "histogram");
    }

    #[tokio::test]
    async fn test_fetch_metrics_empty_response() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/metrics")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"[]"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_metrics(vec![]).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_fetch_metrics_error() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/metrics")
            .with_status(500)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_metrics(vec![]).await;

        mock.assert_async().await;
        assert!(result.is_err());
        match result {
            Err(Error::ApiError(msg)) => assert!(msg.contains("500")),
            _ => panic!("Expected ApiError"),
        }
    }

    // T062: Unit test for ApiClient::fetch_metric_by_name
    #[tokio::test]
    async fn test_fetch_metric_by_name_success() {
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
                    "attributes": {"method": "GET", "status": "200"},
                    "resource": null
                },
                {
                    "name": "http_requests_total",
                    "description": null,
                    "unit": null,
                    "metric_type": "counter",
                    "value": 567,
                    "timestamp": 1705315740000000000,
                    "attributes": {"method": "POST", "status": "201"},
                    "resource": null
                }
            ]"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client
            .fetch_metric_by_name("http_requests_total", vec![])
            .await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let metrics = result.unwrap();
        assert_eq!(metrics.len(), 2);
        assert_eq!(metrics[0].name, "http_requests_total");
    }

    #[tokio::test]
    async fn test_fetch_metric_by_name_not_found() {
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

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client
            .fetch_metric_by_name("nonexistent_metric", vec![])
            .await;

        mock.assert_async().await;
        assert!(result.is_err());
        match result {
            Err(Error::NotFound(msg)) => assert!(msg.contains("nonexistent_metric")),
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_fetch_metric_by_name_with_filters() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/metrics")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("name".into(), "response_time_ms".into()),
                mockito::Matcher::UrlEncoded("since".into(), "1h".into()),
                mockito::Matcher::UrlEncoded("label".into(), "method=GET".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"[{"name":"response_time_ms","description":null,"unit":null,"metric_type":"gauge","value":42.5,"timestamp":1705315800000000000,"attributes":{},"resource":null}]"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let params = vec![
            ("since", "1h".to_string()),
            ("label", "method=GET".to_string()),
        ];
        let result = client
            .fetch_metric_by_name("response_time_ms", params)
            .await;

        mock.assert_async().await;
        assert!(result.is_ok());
    }
}

// Made with Bob
