use crate::server::{AppState, QueryCache};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use rotel_core::api::{
    ErrorResponse, HistogramBucket, HistogramValue, MetricResponse, MetricValue, Quantile,
    Resource, SummaryValue,
};
use rotel_core::telemetry::metric::MetricType;
use rotel_core::telemetry::Metric;
use rotel_storage::QueryParams;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Query parameters for listing metrics
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams)]
pub struct MetricsQuery {
    /// Filter by metric name
    pub name: Option<String>,
    /// Filter by resource attribute (format: key=value)
    pub resource: Option<String>,
    /// Start time (nanoseconds since Unix epoch)
    pub start_time: Option<i64>,
    /// End time (nanoseconds since Unix epoch)
    pub end_time: Option<i64>,
    /// Maximum number of metrics to return
    pub limit: Option<usize>,
    /// Offset for pagination
    pub offset: Option<usize>,
}

/// Query parameters for aggregating metrics
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams)]
pub struct AggregateQuery {
    /// Metric name to aggregate
    pub name: String,
    /// Aggregation function (sum, avg, min, max)
    pub function: String,
    /// Time bucket size in seconds (for time-series aggregation)
    pub bucket_size: Option<i64>,
    /// Start time (nanoseconds since Unix epoch)
    pub start_time: Option<i64>,
    /// End time (nanoseconds since Unix epoch)
    pub end_time: Option<i64>,
}

/// Response structure for aggregated metrics
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AggregateResponse {
    pub name: String,
    pub function: String,
    pub result: f64,
    pub count: usize,
    pub buckets: Option<Vec<TimeBucket>>,
}

/// Time bucket for time-series aggregation
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct TimeBucket {
    pub timestamp: i64,
    pub value: f64,
    pub count: usize,
}

/// List metrics with optional filtering
#[utoipa::path(
    get,
    path = "/api/metrics",
    params(MetricsQuery),
    responses(
        (status = 200, description = "List of metrics", body = Vec<MetricResponse>),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "metrics"
)]
pub async fn list_metrics(
    State(state): State<AppState>,
    Query(query): Query<MetricsQuery>,
) -> Result<Json<Vec<MetricResponse>>, (StatusCode, Json<ErrorResponse>)> {
    // Check cache first
    let cache_key = QueryCache::make_key(&query);
    if let Some(cached) = state.cache.metrics.get(&cache_key) {
        if let Ok(response) = serde_json::from_str(&cached) {
            return Ok(Json(response));
        }
    }

    // Build query parameters
    let mut params = QueryParams::default();

    if let Some(start) = query.start_time {
        params.start_time = Some(start);
    }
    if let Some(end) = query.end_time {
        params.end_time = Some(end);
    }
    if let Some(limit) = query.limit {
        params.limit = Some(limit);
    }

    // Query metrics from storage
    let mut metrics = state.storage.query_metrics(&params).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::storage_error(format!(
                "query metrics: {}",
                e
            ))),
        )
    })?;

    // Filter by name if specified
    if let Some(name_filter) = &query.name {
        metrics.retain(|m| m.name.contains(name_filter));
    }

    // Filter by resource if specified
    if let Some(resource_filter) = &query.resource {
        if let Some((key, value)) = resource_filter.split_once('=') {
            metrics.retain(|m| {
                m.resource
                    .as_ref()
                    .and_then(|r| r.attributes.get(key))
                    .map(|v| v == value)
                    .unwrap_or(false)
            });
        }
    }

    // Apply pagination manually
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(100);

    let metrics: Vec<_> = metrics.into_iter().skip(offset).take(limit).collect();

    // Convert to response format
    let response: Vec<MetricResponse> = metrics
        .into_iter()
        .map(|metric| {
            let (metric_type_str, value) = match &metric.metric_type {
                MetricType::Gauge(v) => ("gauge", MetricValue::Gauge(*v)),
                MetricType::Counter(v) => ("counter", MetricValue::Counter(*v as i64)),
                MetricType::Histogram {
                    count,
                    sum,
                    buckets,
                } => (
                    "histogram",
                    MetricValue::Histogram(HistogramValue {
                        count: *count,
                        sum: *sum,
                        buckets: buckets
                            .iter()
                            .map(|b| HistogramBucket {
                                upper_bound: b.upper_bound,
                                count: b.count,
                            })
                            .collect(),
                    }),
                ),
                MetricType::Summary {
                    count,
                    sum,
                    quantiles,
                } => (
                    "summary",
                    MetricValue::Summary(SummaryValue {
                        count: *count,
                        sum: *sum,
                        quantiles: quantiles
                            .iter()
                            .map(|q| Quantile {
                                quantile: q.quantile,
                                value: q.value,
                            })
                            .collect(),
                    }),
                ),
            };

            MetricResponse {
                name: metric.name,
                description: metric.description,
                unit: metric.unit,
                metric_type: metric_type_str.to_string(),
                value,
                timestamp: metric.timestamp,
                attributes: metric.attributes,
                resource: metric.resource.map(|r| Resource {
                    attributes: r.attributes,
                }),
            }
        })
        .collect();

    Ok(Json(response))
}

/// Get list of unique metric names
#[utoipa::path(
    get,
    path = "/api/metrics/names",
    responses(
        (status = 200, description = "List of unique metric names", body = Vec<String>),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "metrics"
)]
pub async fn list_metric_names(
    State(state): State<AppState>,
) -> Result<Json<Vec<String>>, (StatusCode, Json<ErrorResponse>)> {
    // Query all metrics
    let params = QueryParams::default();
    let metrics = state.storage.query_metrics(&params).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::storage_error(format!(
                "list metric names: {}",
                e
            ))),
        )
    })?;

    // Extract unique names
    let mut names: Vec<String> = metrics
        .into_iter()
        .map(|m| m.name)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    names.sort();
    Ok(Json(names))
}

/// Aggregate metrics by function
#[utoipa::path(
    get,
    path = "/api/metrics/aggregate",
    params(AggregateQuery),
    responses(
        (status = 200, description = "Aggregated metric result", body = AggregateResponse),
        (status = 400, description = "Invalid aggregation function", body = ErrorResponse),
        (status = 404, description = "Metric not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "metrics"
)]
pub async fn aggregate_metrics(
    State(state): State<AppState>,
    Query(query): Query<AggregateQuery>,
) -> Result<Json<AggregateResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Check cache first
    let cache_key = QueryCache::make_key(&query);
    if let Some(cached) = state.cache.metrics.get(&cache_key) {
        if let Ok(response) = serde_json::from_str(&cached) {
            return Ok(Json(response));
        }
    }

    // Build query parameters
    let mut params = QueryParams::default();

    if let Some(start) = query.start_time {
        params.start_time = Some(start);
    }
    if let Some(end) = query.end_time {
        params.end_time = Some(end);
    }

    // Query metrics from storage
    let metrics = state.storage.query_metrics(&params).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::storage_error(format!(
                "aggregate metrics: {}",
                e
            ))),
        )
    })?;

    // Filter by name
    let metrics: Vec<_> = metrics
        .into_iter()
        .filter(|m| m.name == query.name)
        .collect();

    if metrics.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::not_found(format!("Metric '{}'", query.name))),
        ));
    }

    // Perform aggregation based on function
    let result = match query.function.as_str() {
        "sum" => {
            let mut sum = 0.0;
            let mut count = 0;
            for metric in &metrics {
                match &metric.metric_type {
                    MetricType::Gauge(v) => {
                        sum += v;
                        count += 1;
                    },
                    MetricType::Counter(v) => {
                        sum += *v as f64;
                        count += 1;
                    },
                    MetricType::Histogram { sum: s, .. } => {
                        sum += s;
                        count += 1;
                    },
                    MetricType::Summary { sum: s, .. } => {
                        sum += s;
                        count += 1;
                    },
                }
            }
            AggregateResponse {
                name: query.name.clone(),
                function: "sum".to_string(),
                result: sum,
                count,
                buckets: None,
            }
        },
        "avg" => {
            let mut sum = 0.0;
            let mut count = 0;
            for metric in &metrics {
                match &metric.metric_type {
                    MetricType::Gauge(v) => {
                        sum += v;
                        count += 1;
                    },
                    MetricType::Counter(v) => {
                        sum += *v as f64;
                        count += 1;
                    },
                    MetricType::Histogram {
                        sum: s, count: c, ..
                    } => {
                        sum += s;
                        count += *c as usize;
                    },
                    MetricType::Summary {
                        sum: s, count: c, ..
                    } => {
                        sum += s;
                        count += *c as usize;
                    },
                }
            }
            let avg = if count > 0 { sum / count as f64 } else { 0.0 };
            AggregateResponse {
                name: query.name.clone(),
                function: "avg".to_string(),
                result: avg,
                count,
                buckets: None,
            }
        },
        "min" => {
            let mut min = f64::MAX;
            let mut count = 0;
            for metric in &metrics {
                match &metric.metric_type {
                    MetricType::Gauge(v) => {
                        min = min.min(*v);
                        count += 1;
                    },
                    MetricType::Counter(v) => {
                        min = min.min(*v as f64);
                        count += 1;
                    },
                    _ => {},
                }
            }
            AggregateResponse {
                name: query.name.clone(),
                function: "min".to_string(),
                result: if count > 0 { min } else { 0.0 },
                count,
                buckets: None,
            }
        },
        "max" => {
            let mut max = f64::MIN;
            let mut count = 0;
            for metric in &metrics {
                match &metric.metric_type {
                    MetricType::Gauge(v) => {
                        max = max.max(*v);
                        count += 1;
                    },
                    MetricType::Counter(v) => {
                        max = max.max(*v as f64);
                        count += 1;
                    },
                    _ => {},
                }
            }
            AggregateResponse {
                name: query.name.clone(),
                function: "max".to_string(),
                result: if count > 0 { max } else { 0.0 },
                count,
                buckets: None,
            }
        },
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::bad_request(format!(
                    "Invalid aggregation function '{}'. Use: sum, avg, min, max",
                    query.function
                ))),
            ))
        },
    };

    // If bucket_size is specified, perform time-series aggregation
    let result = if let Some(bucket_size) = query.bucket_size {
        let bucket_size_ns = bucket_size * 1_000_000_000; // Convert seconds to nanoseconds

        // Group metrics by time bucket
        let mut buckets: HashMap<i64, Vec<&Metric>> = HashMap::new();
        for metric in &metrics {
            let bucket_timestamp = (metric.timestamp / bucket_size_ns) * bucket_size_ns;
            buckets.entry(bucket_timestamp).or_default().push(metric);
        }

        // Aggregate each bucket
        let mut time_buckets: Vec<TimeBucket> = buckets
            .into_iter()
            .map(|(timestamp, bucket_metrics)| {
                let (sum, count) = match query.function.as_str() {
                    "sum" => {
                        let mut sum = 0.0;
                        let mut count = 0;
                        for metric in bucket_metrics {
                            match &metric.metric_type {
                                MetricType::Gauge(v) => {
                                    sum += v;
                                    count += 1;
                                },
                                MetricType::Counter(v) => {
                                    sum += *v as f64;
                                    count += 1;
                                },
                                MetricType::Histogram { sum: s, .. } => {
                                    sum += s;
                                    count += 1;
                                },
                                MetricType::Summary { sum: s, .. } => {
                                    sum += s;
                                    count += 1;
                                },
                            }
                        }
                        (sum, count)
                    },
                    "avg" => {
                        let mut sum = 0.0;
                        let mut count = 0;
                        for metric in bucket_metrics {
                            match &metric.metric_type {
                                MetricType::Gauge(v) => {
                                    sum += v;
                                    count += 1;
                                },
                                MetricType::Counter(v) => {
                                    sum += *v as f64;
                                    count += 1;
                                },
                                MetricType::Histogram {
                                    sum: s, count: c, ..
                                } => {
                                    sum += s;
                                    count += *c as usize;
                                },
                                MetricType::Summary {
                                    sum: s, count: c, ..
                                } => {
                                    sum += s;
                                    count += *c as usize;
                                },
                            }
                        }
                        let avg = if count > 0 { sum / count as f64 } else { 0.0 };
                        (avg, count)
                    },
                    _ => (0.0, 0),
                };

                TimeBucket {
                    timestamp,
                    value: sum,
                    count,
                }
            })
            .collect();

        time_buckets.sort_by_key(|b| b.timestamp);

        AggregateResponse {
            buckets: Some(time_buckets),
            ..result
        }
    } else {
        result
    };

    Ok(Json(result))
}

/// Export metrics as JSON
#[utoipa::path(
    get,
    path = "/api/metrics/export",
    params(MetricsQuery),
    responses(
        (status = 200, description = "Exported metrics in JSON format"),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "metrics"
)]
pub async fn export_metrics(
    State(state): State<AppState>,
    Query(query): Query<MetricsQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    // Build query parameters
    let mut params = QueryParams::default();

    if let Some(start) = query.start_time {
        params.start_time = Some(start);
    }
    if let Some(end) = query.end_time {
        params.end_time = Some(end);
    }

    // Query metrics from storage
    let metrics = state.storage.query_metrics(&params).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::storage_error(format!(
                "export metrics: {}",
                e
            ))),
        )
    })?;

    // Filter by name if specified
    let metrics: Vec<_> = if let Some(name_filter) = &query.name {
        metrics
            .into_iter()
            .filter(|m| m.name.contains(name_filter))
            .collect()
    } else {
        metrics
    };

    // Convert to JSON
    let json = serde_json::to_string_pretty(&metrics).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::internal_error(format!(
                "Failed to serialize metrics: {}",
                e
            ))),
        )
    })?;

    Ok((StatusCode::OK, [("Content-Type", "application/json")], json))
}
