//! Metric query handlers

use axum::{
    extract::{Path, Query},
    Json,
};

use crate::{
    error::{ApiError, ApiResult},
    models::{
        metric::{
            HistogramBucket, HistogramData, MetricDataPoint, MetricQueryParams, MetricStats,
            MetricValue, Quantile, SummaryData,
        },
        ListResponse, PaginationMetadata, ResourceAttributes,
    },
};

/// Handler for GET /api/v1/metrics - List metrics with filtering and pagination
///
/// Query metrics with optional filtering by name, type, service name, and time ranges.
/// Results are paginated and sorted by timestamp (newest first).
///
/// # Query Parameters
/// - `name`: Filter by metric name
/// - `metric_type`: Filter by type (GAUGE, COUNTER, HISTOGRAM, SUMMARY)
/// - `service_name`: Filter by service name
/// - `start_time`: Start of time range (Unix timestamp in ms)
/// - `end_time`: End of time range (Unix timestamp in ms)
/// - `since`: Relative time range (e.g., "1h", "30m", "7d")
/// - `attributes`: Filter by attributes (format: "key:value")
/// - `offset`: Pagination offset (default: 0)
/// - `limit`: Number of results per page (default: 100, max: 1000)
///
/// # Example
/// ```text
/// GET /api/v1/metrics?name=http_requests_total&metric_type=COUNTER&limit=50
/// ```text
#[utoipa::path(
    get,
    path = "/api/v1/metrics",
    params(MetricQueryParams),
    responses(
        (status = 200, description = "List of metrics", body = ListResponse<MetricDataPoint>),
        (status = 400, description = "Invalid query parameters"),
        (status = 500, description = "Internal server error")
    ),
    tag = "metrics"
)]
pub async fn list_metrics(
    Query(params): Query<MetricQueryParams>,
) -> ApiResult<Json<ListResponse<MetricDataPoint>>> {
    // Validate parameters
    if let Err(e) = validator::Validate::validate(&params) {
        return Err(ApiError::ValidationError(e.to_string()));
    }

    // TODO: Query storage backend
    // For now, return mock data
    let metrics = vec![
        create_mock_metric("metric-1", "http_requests_total", "COUNTER", 1500.0),
        create_mock_metric("metric-2", "memory_usage_bytes", "GAUGE", 850_000_000.0),
        create_mock_metric("metric-3", "request_duration_ms", "HISTOGRAM", 0.0),
    ];

    // Apply filters
    let filtered_metrics: Vec<MetricDataPoint> = metrics
        .into_iter()
        .filter(|metric| {
            // Filter by name
            if let Some(ref name) = params.name {
                if !metric.name.eq_ignore_ascii_case(name) {
                    return false;
                }
            }

            // Filter by metric type
            if let Some(ref metric_type) = params.metric_type {
                if !metric.metric_type.eq_ignore_ascii_case(metric_type) {
                    return false;
                }
            }

            // Filter by service name
            if let Some(ref service_name) = params.service_name {
                if !metric
                    .resource
                    .service_name
                    .as_ref()
                    .map(|s| s.eq_ignore_ascii_case(service_name))
                    .unwrap_or(false)
                {
                    return false;
                }
            }

            true
        })
        .collect();

    let total = filtered_metrics.len();

    // Apply pagination
    let paginated_metrics: Vec<MetricDataPoint> = filtered_metrics
        .into_iter()
        .skip(params.offset)
        .take(params.limit)
        .collect();

    let count = paginated_metrics.len();

    let pagination = PaginationMetadata::new(total, params.offset, params.limit, count);

    Ok(Json(ListResponse::new(paginated_metrics, pagination)))
}

/// Handler for GET /api/v1/metrics/{name}/stats - Get metric statistics
///
/// Calculate aggregated statistics for a specific metric including min, max, avg,
/// percentiles, and other statistical measures.
///
/// # Path Parameters
/// - `name`: Metric name
///
/// # Query Parameters
/// - `start_time`: Start of time range (Unix timestamp in ms)
/// - `end_time`: End of time range (Unix timestamp in ms)
/// - `since`: Relative time range (e.g., "1h", "30m", "7d")
///
/// # Example
/// ```text
/// GET /api/v1/metrics/request_duration_ms/stats?since=1h
/// ```text
#[utoipa::path(
    get,
    path = "/api/v1/metrics/{name}/stats",
    params(
        ("name" = String, Path, description = "Metric name")
    ),
    responses(
        (status = 200, description = "Metric statistics", body = MetricStats),
        (status = 404, description = "Metric not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "metrics"
)]
pub async fn get_metric_stats(Path(name): Path<String>) -> ApiResult<Json<MetricStats>> {
    // TODO: Query storage backend and calculate real statistics
    // For now, return mock statistics
    if name == "request_duration_ms" {
        let mock_values = vec![10.0, 25.0, 50.0, 75.0, 100.0, 150.0, 200.0, 300.0];
        Ok(Json(MetricStats::from_values(name, &mock_values)))
    } else {
        Err(ApiError::NotFound(format!("Metric '{}' not found", name)))
    }
}

/// Create a mock metric for testing
fn create_mock_metric(id: &str, name: &str, metric_type: &str, value: f64) -> MetricDataPoint {
    use std::collections::HashMap;

    let resource = ResourceAttributes {
        service_name: Some("rotel-api".to_string()),
        service_version: Some("0.1.0".to_string()),
        service_instance_id: Some("instance-1".to_string()),
        attributes: HashMap::new(),
    };

    let metric_value = match metric_type {
        "HISTOGRAM" => MetricValue::Histogram(HistogramData {
            count: 100,
            sum: 5000.0,
            buckets: vec![
                HistogramBucket {
                    upper_bound: 10.0,
                    count: 20,
                },
                HistogramBucket {
                    upper_bound: 50.0,
                    count: 50,
                },
                HistogramBucket {
                    upper_bound: 100.0,
                    count: 80,
                },
                HistogramBucket {
                    upper_bound: 1.7976931348623157e308,
                    count: 100,
                },
            ],
        }),
        "SUMMARY" => MetricValue::Summary(SummaryData {
            count: 100,
            sum: 5000.0,
            quantiles: vec![
                Quantile {
                    quantile: 0.5,
                    value: 45.0,
                },
                Quantile {
                    quantile: 0.95,
                    value: 95.0,
                },
                Quantile {
                    quantile: 0.99,
                    value: 99.0,
                },
            ],
        }),
        _ => MetricValue::Number(value),
    };

    MetricDataPoint {
        id: id.to_string(),
        name: name.to_string(),
        metric_type: metric_type.to_string(),
        timestamp: chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
        value: metric_value,
        resource,
        attributes: {
            let mut attrs = HashMap::new();
            attrs.insert("environment".to_string(), "production".to_string());
            attrs
        },
        unit: Some(
            match name {
                "memory_usage_bytes" => "bytes",
                "request_duration_ms" => "ms",
                _ => "count",
            }
            .to_string(),
        ),
        description: Some(format!("Mock metric: {}", name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_metrics() {
        let params = MetricQueryParams::default();
        let result = list_metrics(Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert_eq!(response.items.len(), 3);
        assert_eq!(response.pagination.total, 3);
    }

    #[tokio::test]
    async fn test_list_metrics_with_name_filter() {
        let params = MetricQueryParams {
            name: Some("http_requests_total".to_string()),
            ..Default::default()
        };
        let result = list_metrics(Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].name, "http_requests_total");
    }

    #[tokio::test]
    async fn test_list_metrics_with_type_filter() {
        let params = MetricQueryParams {
            metric_type: Some("GAUGE".to_string()),
            ..Default::default()
        };
        let result = list_metrics(Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        for metric in &response.items {
            assert_eq!(metric.metric_type, "GAUGE");
        }
    }

    #[tokio::test]
    async fn test_get_metric_stats_found() {
        let result = get_metric_stats(Path("request_duration_ms".to_string())).await;
        assert!(result.is_ok());

        let stats = result.unwrap().0;
        assert_eq!(stats.name, "request_duration_ms");
        assert!(stats.count > 0);
        assert!(stats.percentiles.is_some());
    }

    #[tokio::test]
    async fn test_get_metric_stats_not_found() {
        let result = get_metric_stats(Path("nonexistent".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_metric_stats_calculation() {
        let values = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        let stats = MetricStats::from_values("test_metric".to_string(), &values);

        assert_eq!(stats.count, 5);
        assert_eq!(stats.min, 10.0);
        assert_eq!(stats.max, 50.0);
        assert_eq!(stats.avg, 30.0);
        assert_eq!(stats.sum, 150.0);
        assert!(stats.stddev.is_some());
        assert!(stats.percentiles.is_some());

        let percentiles = stats.percentiles.unwrap();
        assert_eq!(percentiles.p50, 30.0);
    }
}

// Made with Bob
