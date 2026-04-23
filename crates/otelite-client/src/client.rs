use crate::error::{Error, Result};
use crate::models::{
    LogEntry, LogsQuery, LogsResponse, MetricResponse, Trace, TracesQuery, TracesResponse,
};
use reqwest::Client;
use std::time::Duration;

pub struct ApiClient {
    client: Client,
    base_url: String,
}

impl ApiClient {
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

    pub async fn fetch_logs(&self, params: Vec<(&str, String)>) -> Result<LogsResponse> {
        let url = format!("{}/api/logs", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to fetch logs: HTTP {}",
                response.status()
            )));
        }

        Ok(response.json().await?)
    }

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

        Ok(response.json().await?)
    }

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

        Ok(response.json().await?)
    }

    pub async fn get_logs(&self, query: &LogsQuery) -> Result<LogsResponse> {
        let url = format!("{}/api/logs", self.base_url);
        let response = self.client.get(&url).query(query).send().await?;

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to fetch logs: HTTP {}",
                response.status()
            )));
        }

        Ok(response.json().await?)
    }

    pub async fn fetch_traces(&self, params: Vec<(&str, String)>) -> Result<TracesResponse> {
        let url = format!("{}/api/traces", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to fetch traces: HTTP {}",
                response.status()
            )));
        }

        Ok(response.json().await?)
    }

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

        Ok(response.json().await?)
    }

    pub async fn get_traces(&self, query: &TracesQuery) -> Result<TracesResponse> {
        let url = format!("{}/api/traces", self.base_url);
        let response = self.client.get(&url).query(query).send().await?;

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to fetch traces: HTTP {}",
                response.status()
            )));
        }

        Ok(response.json().await?)
    }

    pub async fn fetch_metrics(&self, params: Vec<(&str, String)>) -> Result<Vec<MetricResponse>> {
        let url = format!("{}/api/metrics", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to fetch metrics: HTTP {}",
                response.status()
            )));
        }

        Ok(response.json().await?)
    }

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

    pub async fn export_logs(&self, params: Vec<(&str, String)>) -> Result<String> {
        let url = format!("{}/api/logs/export", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to export logs: HTTP {}",
                response.status()
            )));
        }

        Ok(response.text().await?)
    }

    pub async fn export_traces(&self, params: Vec<(&str, String)>) -> Result<String> {
        let url = format!("{}/api/traces/export", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to export traces: HTTP {}",
                response.status()
            )));
        }

        Ok(response.text().await?)
    }

    pub async fn export_metrics(&self, params: Vec<(&str, String)>) -> Result<String> {
        let url = format!("{}/api/metrics/export", self.base_url);
        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            return Err(Error::ApiError(format!(
                "Failed to export metrics: HTTP {}",
                response.status()
            )));
        }

        Ok(response.text().await?)
    }

    pub async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/health", self.base_url);
        match self.client.get(&url).send().await {
            Ok(response) => Ok(response.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use mockito::Server;

    #[test]
    fn test_api_client_creation() {
        let client = ApiClient::new("http://localhost:8080".to_string(), Duration::from_secs(30));
        assert!(client.is_ok());
    }

    #[test]
    fn test_api_client_invalid_timeout() {
        let client = ApiClient::new(
            "http://localhost:8080".to_string(),
            Duration::from_millis(1),
        );
        assert!(client.is_ok());
    }

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
                    "resource": null,
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
        let result = client.fetch_logs(vec![("limit", "10".to_string())]).await;

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
                "resource": null,
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
            .with_body(r#"{"logs": [], "total": 0, "limit": 100, "offset": 0}"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.search_logs("error", vec![]).await;

        mock.assert_async().await;
        assert!(result.is_ok());
    }

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
        let result = client.fetch_traces(vec![("limit", "10".to_string())]).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let traces = result.unwrap();
        assert_eq!(traces.traces.len(), 1);
        assert_eq!(traces.traces[0].trace_id, "trace1");
        assert!(!traces.traces[0].has_errors);
    }

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
                        "attributes": {},
                        "resource": null,
                        "status": {"code": "OK", "message": null},
                        "events": []
                    }
                ],
                "start_time": 1705315800000000000,
                "end_time": 1705315800250000000,
                "duration": 250000000,
                "span_count": 1,
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
        assert_eq!(trace.spans.len(), 1);
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
        match result.unwrap_err() {
            Error::NotFound(msg) => assert!(msg.contains("nonexistent")),
            _ => panic!("Expected NotFound error"),
        }
    }

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
                    "attributes": {},
                    "resource": null
                }
            ]"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.fetch_metrics(vec![]).await;

        mock.assert_async().await;
        assert!(result.is_ok());
        let metrics = result.unwrap();
        assert_eq!(metrics.len(), 1);
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
        match result.unwrap_err() {
            Error::NotFound(msg) => assert!(msg.contains("nonexistent_metric")),
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_health_check_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/health")
            .with_status(200)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), Duration::from_secs(30)).unwrap();
        let result = client.health_check().await;

        mock.assert_async().await;
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn test_health_check_unreachable() {
        let client = ApiClient::new(
            "http://127.0.0.1:19999".to_string(),
            Duration::from_millis(100),
        )
        .unwrap();
        let result = client.health_check().await;
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }
}
