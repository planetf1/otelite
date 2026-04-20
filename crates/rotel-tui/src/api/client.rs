// API client implementation - waiting for UI integration
#![allow(dead_code)]

use anyhow::{Context, Result};
use reqwest::Client;

use super::models::*;

/// HTTP client for Rotel API
#[derive(Clone)]
pub struct ApiClient {
    base_url: String,
    client: Client,
}

impl ApiClient {
    /// Create a new API client
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: Client::new(),
        }
    }

    /// Fetch logs from the API
    pub async fn get_logs(&self, query: &LogsQuery) -> Result<LogsResponse> {
        let url = format!("{}/api/logs", self.base_url);

        let response = self
            .client
            .get(&url)
            .query(query)
            .send()
            .await
            .context("Failed to fetch logs")?;

        if !response.status().is_success() {
            anyhow::bail!("API returned error: {}", response.status());
        }

        response
            .json::<LogsResponse>()
            .await
            .context("Failed to parse logs response")
    }

    /// Fetch a single log entry by ID
    #[allow(dead_code)]
    pub async fn get_log(&self, id: &str) -> Result<LogEntry> {
        let url = format!("{}/api/logs/{}", self.base_url, id);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch log")?;

        if !response.status().is_success() {
            anyhow::bail!("API returned error: {}", response.status());
        }

        response
            .json::<LogEntry>()
            .await
            .context("Failed to parse log response")
    }

    /// Fetch traces from the API
    pub async fn get_traces(&self, query: &TracesQuery) -> Result<TracesResponse> {
        let url = format!("{}/api/traces", self.base_url);

        let response = self
            .client
            .get(&url)
            .query(query)
            .send()
            .await
            .context("Failed to fetch traces")?;

        if !response.status().is_success() {
            anyhow::bail!("API returned error: {}", response.status());
        }

        response
            .json::<TracesResponse>()
            .await
            .context("Failed to parse traces response")
    }

    /// Fetch a single trace with all spans
    pub async fn get_trace(&self, trace_id: &str) -> Result<Trace> {
        let url = format!("{}/api/traces/{}", self.base_url, trace_id);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch trace")?;

        if !response.status().is_success() {
            anyhow::bail!("API returned error: {}", response.status());
        }

        response
            .json::<Trace>()
            .await
            .context("Failed to parse trace response")
    }

    /// Fetch metrics from the API
    pub async fn get_metrics(&self) -> Result<MetricsResponse> {
        let url = format!("{}/api/metrics", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch metrics")?;

        if !response.status().is_success() {
            anyhow::bail!("API returned error: {}", response.status());
        }

        response
            .json::<MetricsResponse>()
            .await
            .context("Failed to parse metrics response")
    }

    /// Fetch a single metric by name
    pub async fn get_metric(&self, name: &str) -> Result<Metric> {
        let url = format!("{}/api/metrics/{}", self.base_url, name);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch metric")?;

        if !response.status().is_success() {
            anyhow::bail!("API returned error: {}", response.status());
        }

        response
            .json::<Metric>()
            .await
            .context("Failed to parse metric response")
    }

    /// Check if the API is reachable
    pub async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/health", self.base_url);

        match self.client.get(&url).send().await {
            Ok(response) => Ok(response.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}
