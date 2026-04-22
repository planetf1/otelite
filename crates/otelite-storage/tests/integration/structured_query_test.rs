use otelite_core::query::{Operator, QueryPredicate, QueryValue};
use otelite_core::telemetry::log::{LogRecord, SeverityLevel};
use otelite_core::telemetry::metric::{Metric, MetricType};
use otelite_core::telemetry::trace::{Span, SpanKind, SpanStatus, StatusCode};
use otelite_core::telemetry::Resource;
use otelite_storage::sqlite::SqliteBackend;
use otelite_storage::{QueryParams, StorageBackend, StorageConfig};
use std::collections::HashMap;
use tempfile::TempDir;

async fn setup_backend() -> (SqliteBackend, TempDir) {
    let temp_dir = TempDir::new().expect("temp dir");
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.expect("init backend");
    (backend, temp_dir)
}

fn resource_with_service(service_name: &str) -> Resource {
    let mut attrs = HashMap::new();
    attrs.insert("service.name".to_string(), service_name.to_string());
    Resource { attributes: attrs }
}

#[tokio::test]
async fn query_logs_filters_by_attribute_predicates() {
    let (backend, _temp_dir) = setup_backend().await;

    let mut matching_attributes = HashMap::new();
    matching_attributes.insert("gen_ai.system".to_string(), "anthropic".to_string());
    matching_attributes.insert("http.method".to_string(), "POST".to_string());

    let mut non_matching_attributes = HashMap::new();
    non_matching_attributes.insert("gen_ai.system".to_string(), "openai".to_string());
    non_matching_attributes.insert("http.method".to_string(), "GET".to_string());

    backend
        .write_log(&LogRecord {
            timestamp: 1_000,
            observed_timestamp: Some(1_000),
            trace_id: Some("trace-a".to_string()),
            span_id: Some("span-a".to_string()),
            severity: SeverityLevel::Info,
            severity_text: Some("INFO".to_string()),
            body: "matching log body".to_string(),
            attributes: matching_attributes,
            resource: Some(resource_with_service("gateway")),
        })
        .await
        .expect("write matching log");

    backend
        .write_log(&LogRecord {
            timestamp: 2_000,
            observed_timestamp: Some(2_000),
            trace_id: Some("trace-b".to_string()),
            span_id: Some("span-b".to_string()),
            severity: SeverityLevel::Info,
            severity_text: Some("INFO".to_string()),
            body: "non-matching log body".to_string(),
            attributes: non_matching_attributes,
            resource: Some(resource_with_service("worker")),
        })
        .await
        .expect("write non-matching log");

    let params = QueryParams {
        predicates: vec![
            QueryPredicate {
                field: "gen_ai.system".to_string(),
                operator: Operator::Equal,
                value: QueryValue::String("anthropic".to_string()),
            },
            QueryPredicate {
                field: "resource.service.name".to_string(),
                operator: Operator::Equal,
                value: QueryValue::String("gateway".to_string()),
            },
        ],
        ..Default::default()
    };

    let logs = backend.query_logs(&params).await.expect("query logs");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].body, "matching log body");
}

#[tokio::test]
async fn query_spans_filters_by_duration_and_attribute_predicates() {
    let (backend, _temp_dir) = setup_backend().await;

    let mut matching_attributes = HashMap::new();
    matching_attributes.insert("llm.provider".to_string(), "anthropic".to_string());

    let mut non_matching_attributes = HashMap::new();
    non_matching_attributes.insert("llm.provider".to_string(), "openai".to_string());

    backend
        .write_span(&Span {
            trace_id: "trace-1".to_string(),
            span_id: "span-1".to_string(),
            parent_span_id: None,
            name: "matching-span".to_string(),
            kind: SpanKind::Internal,
            start_time: 5_000,
            end_time: 5_900,
            attributes: matching_attributes,
            events: Vec::new(),
            status: SpanStatus {
                code: StatusCode::Ok,
                message: None,
            },
            resource: None,
        })
        .await
        .expect("write matching span");

    backend
        .write_span(&Span {
            trace_id: "trace-2".to_string(),
            span_id: "span-2".to_string(),
            parent_span_id: None,
            name: "short-span".to_string(),
            kind: SpanKind::Internal,
            start_time: 6_000,
            end_time: 6_200,
            attributes: non_matching_attributes,
            events: Vec::new(),
            status: SpanStatus {
                code: StatusCode::Ok,
                message: None,
            },
            resource: None,
        })
        .await
        .expect("write non-matching span");

    let params = QueryParams {
        predicates: vec![
            QueryPredicate {
                field: "duration".to_string(),
                operator: Operator::GreaterThan,
                value: QueryValue::Duration(500),
            },
            QueryPredicate {
                field: "llm.provider".to_string(),
                operator: Operator::Equal,
                value: QueryValue::String("anthropic".to_string()),
            },
        ],
        ..Default::default()
    };

    let spans = backend.query_spans(&params).await.expect("query spans");
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].name, "matching-span");
}

#[tokio::test]
async fn query_metrics_filters_by_attribute_predicates() {
    let (backend, _temp_dir) = setup_backend().await;

    let mut matching_attributes = HashMap::new();
    matching_attributes.insert("env".to_string(), "prod".to_string());

    let mut non_matching_attributes = HashMap::new();
    non_matching_attributes.insert("env".to_string(), "dev".to_string());

    backend
        .write_metric(&Metric {
            name: "request.count".to_string(),
            description: Some("request counter".to_string()),
            unit: Some("count".to_string()),
            metric_type: MetricType::Counter(42),
            timestamp: 10_000,
            attributes: matching_attributes,
            resource: Some(resource_with_service("api")),
        })
        .await
        .expect("write matching metric");

    backend
        .write_metric(&Metric {
            name: "request.count".to_string(),
            description: Some("request counter".to_string()),
            unit: Some("count".to_string()),
            metric_type: MetricType::Counter(7),
            timestamp: 11_000,
            attributes: non_matching_attributes,
            resource: Some(resource_with_service("worker")),
        })
        .await
        .expect("write non-matching metric");

    let params = QueryParams {
        predicates: vec![
            QueryPredicate {
                field: "env".to_string(),
                operator: Operator::Equal,
                value: QueryValue::String("prod".to_string()),
            },
            QueryPredicate {
                field: "resource.service.name".to_string(),
                operator: Operator::Equal,
                value: QueryValue::String("api".to_string()),
            },
        ],
        ..Default::default()
    };

    let metrics = backend.query_metrics(&params).await.expect("query metrics");
    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0].timestamp, 10_000);
}
