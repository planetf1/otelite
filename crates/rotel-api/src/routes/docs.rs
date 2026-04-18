//! OpenAPI documentation routes

use axum::{routing::get, Router};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::{
    handlers::{health, logs, metrics, traces},
    models::{
        health::{
            ComponentHealth, ComponentStatus, HealthResponse, HealthStatus, ReadinessChecks,
            ReadinessResponse, SystemStats,
        },
        metric::{MetricDataPoint, MetricQueryParams, MetricStats, MetricValue, Percentiles},
        pagination::PaginationMetadata,
        request::LogQueryParams,
        response::{ListResponse, LogEntry, ResourceAttributes, SuccessResponse},
        trace::{SpanEvent, SpanLink, SpanStatus, Trace, TraceQueryParams, TraceSpan},
    },
};

/// OpenAPI documentation structure
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Rotel API",
        version = "0.1.0",
        description = "Shared REST API backend for Rotel frontends",
        license(name = "Apache-2.0")
    ),
    paths(
        logs::list_logs,
        logs::get_log,
        traces::list_traces,
        traces::get_trace,
        metrics::list_metrics,
        metrics::get_metric_stats,
        health::health_check,
        health::readiness_check,
    ),
    components(
        schemas(
            PaginationMetadata,
            ResourceAttributes,
            SuccessResponse,
            LogEntry,
            LogQueryParams,
            ListResponse<LogEntry>,
            Trace,
            TraceSpan,
            TraceQueryParams,
            SpanStatus,
            SpanEvent,
            SpanLink,
            ListResponse<Trace>,
            MetricDataPoint,
            MetricQueryParams,
            MetricStats,
            MetricValue,
            Percentiles,
            ListResponse<MetricDataPoint>,
            HealthResponse,
            HealthStatus,
            SystemStats,
            ComponentHealth,
            ComponentStatus,
            ReadinessResponse,
            ReadinessChecks,
        )
    ),
    tags(
        (name = "logs", description = "Log query endpoints"),
        (name = "traces", description = "Trace query endpoints"),
        (name = "metrics", description = "Metric query endpoints"),
        (name = "health", description = "Health and status endpoints")
    )
)]
pub struct ApiDoc;

/// Create documentation routes
pub fn routes() -> Router {
    Router::new()
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .route("/api-docs/health", get(health_check))
}

/// Health check endpoint for documentation service
async fn health_check() -> &'static str {
    "Documentation service is running"
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_docs_health_check() {
        let app = routes();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api-docs/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}

// Made with Bob
