# Implementation Plan: OTLP-to-Internal Type Conversion (rotel-3eh)

## Overview
Implement conversion functions to transform OTLP protobuf types into rotel-core internal types. This enables the receiver to process incoming telemetry data and store it in the internal format.

## Design Decisions (from user clarification)

1. **Resource handling**: Store in the `resource` field of LogRecord/Metric, not merged into attributes
   - For Spans: resource goes on Trace (Span has no resource field)

2. **Attribute conversion**: Convert all OTLP AnyValue types to strings
   - IntValue → string representation
   - BoolValue → "true"/"false"
   - DoubleValue → string representation
   - StringValue → as-is
   - ArrayValue/KvlistValue → JSON string representation

3. **Error handling**: Infallible conversions with sensible defaults
   - Missing severity → SeverityLevel::Info
   - Missing body → empty string
   - Missing name → empty string
   - Invalid IDs → skip or use empty string

4. **Scope information**: Preserve as attributes
   - Add "otel.scope.name" and "otel.scope.version" to each record's attributes

## File Structure

### New file: `crates/rotel-receiver/src/conversion.rs`

```rust
// Module structure:
// - Helper functions for common conversions
// - convert_logs(ExportLogsServiceRequest) -> Vec<LogRecord>
// - convert_traces(ExportTraceServiceRequest) -> Vec<Trace>
// - convert_metrics(ExportMetricsServiceRequest) -> Vec<Metric>
// - Tests module
```

## Implementation Steps

### Step 1: Add rotel-core dependency
**File**: `crates/rotel-receiver/Cargo.toml`

Add under `[dependencies]`:
```toml
rotel-core = { path = "../rotel-core" }
```

### Step 2: Declare conversion module
**File**: `crates/rotel-receiver/src/lib.rs`

Add after existing module declarations:
```rust
pub mod conversion;
```

### Step 3: Create conversion.rs with helper functions

**Helper functions needed**:

1. `convert_resource(otlp_resource: Option<Resource>) -> Option<rotel_core::telemetry::Resource>`
   - Convert OTLP Resource to internal Resource
   - Extract attributes from KeyValue vec

2. `convert_attributes(kvs: Vec<KeyValue>) -> HashMap<String, String>`
   - Convert KeyValue vec to HashMap<String, String>
   - Handle all AnyValue types

3. `any_value_to_string(value: &AnyValue) -> String`
   - Convert AnyValue enum to string representation
   - Handle StringValue, IntValue, BoolValue, DoubleValue, BytesValue, ArrayValue, KvlistValue

4. `bytes_to_hex(bytes: &[u8]) -> String`
   - Convert Vec<u8> to hex string (for trace_id, span_id)
   - Use lowercase hex

5. `convert_severity(severity_number: i32) -> SeverityLevel`
   - Map OTLP severity numbers to internal SeverityLevel enum
   - Use SeverityLevel::from_i32() or manual mapping
   - Default to Info for unknown values

### Step 4: Implement convert_logs

**Signature**: `pub fn convert_logs(request: ExportLogsServiceRequest) -> Vec<rotel_core::telemetry::LogRecord>`

**Algorithm**:
```
For each ResourceLogs in request.resource_logs:
  - Convert resource to internal Resource

  For each ScopeLogs in resource_logs.scope_logs:
    - Extract scope name and version

    For each LogRecord in scope_logs.log_records:
      - Create internal LogRecord with:
        - timestamp: time_unix_nano as i64
        - observed_timestamp: Some(observed_time_unix_nano as i64)
        - severity: convert_severity(severity_number)
        - severity_text: Some(severity_text) if not empty
        - body: extract from body AnyValue (default to empty string)
        - attributes: convert_attributes(attributes) + scope info
        - trace_id: Some(bytes_to_hex(trace_id)) if not empty
        - span_id: Some(bytes_to_hex(span_id)) if not empty
        - resource: cloned internal Resource

      Add to result vec

Return flattened vec of all LogRecords
```

**Edge cases**:
- Empty request → return empty vec
- Missing body → use empty string
- Missing severity → use Info (9)
- Empty trace_id/span_id → None
- Missing resource → None

### Step 5: Implement convert_traces

**Signature**: `pub fn convert_traces(request: ExportTraceServiceRequest) -> Vec<rotel_core::telemetry::Trace>`

**Algorithm**:
```
Create HashMap<String, Trace> to group spans by trace_id

For each ResourceSpans in request.resource_spans:
  - Convert resource to internal Resource

  For each ScopeSpans in resource_spans.scope_spans:
    - Extract scope name and version

    For each Span in scope_spans.spans:
      - Convert trace_id to hex string
      - Get or create Trace for this trace_id
      - Create internal Span with:
        - trace_id: hex string
        - span_id: bytes_to_hex(span_id)
        - parent_span_id: Some(bytes_to_hex(parent_span_id)) if not empty
        - name: span.name (default to empty string)
        - kind: SpanKind::from_i32(kind).unwrap_or(SpanKind::Internal)
        - start_time: start_time_unix_nano as i64
        - end_time: end_time_unix_nano as i64
        - attributes: convert_attributes(attributes) + scope info
        - events: convert span.events to Vec<SpanEvent>
        - status: convert span.status to SpanStatus

      Add span to Trace.spans
      Set Trace.resource if not already set

Return HashMap.into_values().collect()
```

**SpanEvent conversion**:
```rust
For each event in span.events:
  SpanEvent {
    name: event.name,
    timestamp: event.time_unix_nano as i64,
    attributes: convert_attributes(event.attributes),
  }
```

**SpanStatus conversion**:
```rust
SpanStatus {
  code: StatusCode::from_i32(status.code).unwrap_or(StatusCode::Unset),
  message: if status.message.is_empty() { None } else { Some(status.message) },
}
```

**Edge cases**:
- Empty request → return empty vec
- Missing parent_span_id → None
- Missing status → StatusCode::Unset with None message
- Empty events → empty vec

### Step 6: Implement convert_metrics

**Signature**: `pub fn convert_metrics(request: ExportMetricsServiceRequest) -> Vec<rotel_core::telemetry::Metric>`

**Algorithm**:
```
For each ResourceMetrics in request.resource_metrics:
  - Convert resource to internal Resource

  For each ScopeMetrics in resource_metrics.scope_metrics:
    - Extract scope name and version

    For each Metric in scope_metrics.metrics:
      - Determine metric type from metric.data
      - For each data point in the metric:
        - Create internal Metric with:
          - name: metric.name
          - description: Some(metric.description) if not empty
          - unit: Some(metric.unit) if not empty
          - metric_type: convert based on data type (see below)
          - timestamp: data_point.time_unix_nano as i64
          - attributes: convert_attributes(data_point.attributes) + scope info
          - resource: cloned internal Resource

        Add to result vec

Return flattened vec of all Metrics
```

**MetricType conversion**:
- **Gauge**: Extract value from NumberDataPoint
  ```rust
  MetricType::Gauge(match data_point.value {
    Some(Value::AsDouble(v)) => v,
    Some(Value::AsInt(v)) => v as f64,
    None => 0.0,
  })
  ```

- **Sum** (treat as Counter if monotonic): Extract value from NumberDataPoint
  ```rust
  MetricType::Counter(match data_point.value {
    Some(Value::AsInt(v)) => v as u64,
    Some(Value::AsDouble(v)) => v as u64,
    None => 0,
  })
  ```

- **Histogram**: Extract buckets and counts
  ```rust
  MetricType::Histogram {
    count: histogram_data_point.count,
    sum: histogram_data_point.sum,
    buckets: histogram_data_point.bucket_counts
      .iter()
      .zip(histogram_data_point.explicit_bounds.iter())
      .map(|(count, bound)| HistogramBucket {
        upper_bound: *bound,
        count: *count,
      })
      .collect(),
  }
  ```

- **Summary**: Extract quantiles
  ```rust
  MetricType::Summary {
    count: summary_data_point.count,
    sum: summary_data_point.sum,
    quantiles: summary_data_point.quantile_values
      .iter()
      .map(|qv| Quantile {
        quantile: qv.quantile,
        value: qv.value,
      })
      .collect(),
  }
  ```

**Edge cases**:
- Empty request → return empty vec
- Unknown metric type → skip (or default to Gauge with 0.0)
- Missing data points → skip metric
- Missing value → use 0.0 or 0

### Step 7: Write comprehensive tests

**Test structure** (in `#[cfg(test)] mod tests` at end of conversion.rs):

#### Helper tests:
- `test_bytes_to_hex` - verify hex conversion
- `test_any_value_to_string_all_types` - test all AnyValue variants
- `test_convert_attributes` - test KeyValue to HashMap conversion
- `test_convert_resource` - test Resource conversion
- `test_convert_severity` - test all severity levels

#### Logs tests:
- `test_convert_empty_logs_request` - empty request returns empty vec
- `test_convert_single_log` - one log with all fields populated
- `test_convert_multiple_resources` - multiple resource_logs with different attributes
- `test_convert_missing_fields` - log with no body, no severity, no attributes
- `test_convert_log_with_trace_context` - log with trace_id and span_id
- `test_convert_log_with_scope_info` - verify scope attributes added

#### Traces tests:
- `test_convert_empty_traces_request` - empty request returns empty vec
- `test_convert_single_span` - one span with all fields
- `test_convert_multiple_spans_same_trace` - multiple spans grouped into one Trace
- `test_convert_multiple_traces` - multiple trace_ids create multiple Traces
- `test_convert_span_with_parent` - parent_span_id preserved
- `test_convert_span_with_events` - events converted correctly
- `test_convert_span_with_status` - status converted correctly
- `test_convert_span_kinds` - all SpanKind values

#### Metrics tests:
- `test_convert_empty_metrics_request` - empty request returns empty vec
- `test_convert_gauge_metric` - gauge with double value
- `test_convert_counter_metric` - sum/counter with int value
- `test_convert_histogram_metric` - histogram with buckets
- `test_convert_summary_metric` - summary with quantiles
- `test_convert_multiple_data_points` - one metric with multiple data points creates multiple internal Metrics
- `test_convert_missing_metric_value` - handle missing value gracefully

**Test data**: Reuse structures from `tests/grpc_test_utils.rs` where possible

### Step 8: Integration with signal handlers (future work, not in this bead)

After this bead is complete, the conversion functions will be called from:
- `crates/rotel-receiver/src/signals/logs.rs` - LogsHandler::process()
- `crates/rotel-receiver/src/signals/traces.rs` - TracesHandler::process()
- `crates/rotel-receiver/src/signals/metrics.rs` - MetricsHandler::process()

This will be done in a separate bead (rotel-xfw) that injects the storage backend.

## Verification Commands

```bash
# Build
cargo build --workspace

# Test conversion module specifically
cargo test -p rotel-receiver -- conversion

# Run all receiver tests
cargo test -p rotel-receiver

# Clippy
cargo clippy -p rotel-receiver -- -D warnings

# Format check
cargo fmt --check
```

## Acceptance Criteria

- [x] rotel-core added as dependency in rotel-receiver/Cargo.toml
- [x] conversion module declared in lib.rs
- [x] conversion.rs created with all three public functions
- [x] Helper functions implemented for common conversions
- [x] convert_logs flattens OTLP structure and preserves all data
- [x] convert_traces groups spans by trace_id into Trace structs
- [x] convert_metrics handles all metric types (Gauge, Counter, Histogram, Summary)
- [x] All edge cases handled gracefully (empty requests, missing fields)
- [x] Scope information preserved as attributes
- [x] Resource information preserved in resource field
- [x] Comprehensive tests cover all functions and edge cases
- [x] All quality gates pass (build, test, clippy, fmt)

## Notes

- This bead focuses solely on type conversion, not storage integration
- The conversion functions are pure and stateless
- All conversions are infallible (no Result return type)
- Timestamp conversion: OTLP uses u64 nanos, internal uses i64 nanos (cast is safe for reasonable timestamps)
- Hex encoding for IDs uses lowercase (standard convention)
- Scope info uses "otel.scope.name" and "otel.scope.version" prefix to avoid conflicts
