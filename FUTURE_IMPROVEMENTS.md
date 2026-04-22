# Future Improvements for Otelite

## Completed ✅

### ~~1. Add Serde Derives to otelite-core Types~~ ✅
**Status**: COMPLETED - All types now have proper Serialize/Deserialize derives

**Changes Made**:
- Added `serde = { workspace = true, features = ["derive"] }` to otelite-core Cargo.toml
- Added `#[derive(Serialize, Deserialize)]` to all telemetry types:
  - `Resource`, `LogRecord`, `SeverityLevel` in log.rs
  - `Metric`, `MetricType`, `HistogramBucket`, `Quantile` in metric.rs
  - `Trace`, `Span`, `SpanKind`, `SpanEvent`, `SpanStatus`, `StatusCode` in trace.rs
- Added safe conversion methods (`from_i32`, `to_i32`) for all enums
- Simplified writer.rs and reader.rs to use proper serialization

### ~~2. Add Safe Enum Conversions~~ ✅
**Status**: COMPLETED - All enums now have safe conversion methods

**Changes Made**:
- Added `from_i32()` and `to_i32()` methods to:
  - `SeverityLevel` in log.rs
  - `SpanKind` and `StatusCode` in trace.rs
- Removed unsafe `std::mem::transmute` calls from reader.rs
- All conversions now use safe, explicit methods with proper error handling

## High Priority

### 1. Add Integration Tests for Serialization
**Issue**: Need tests to verify serialization round-trips work correctly.

**Solution**:
- Add tests that write and read back telemetry data
- Verify JSON format matches expectations
- Validate against OpenTelemetry spec

### 2. Optimize JSON Storage
**Issue**: Storing complex types as JSON strings in SQLite may impact performance.

**Solution**:
- Consider using SQLite JSON1 extension for better querying
- Or normalize data into separate tables for better performance

## Medium Priority

### 3. Add Proper Error Context
**Issue**: Some errors lose context during conversion.

**Solution**:
- Use `thiserror` context features more extensively
- Add more specific error variants

### 4. Add Benchmarks
**Issue**: No performance benchmarks for storage operations.

**Solution**:
- Add criterion benchmarks for write/read operations
- Track performance over time

## Low Priority

### 5. Add Compression
**Issue**: Large JSON blobs could benefit from compression.

**Solution**:
- Consider compressing large attribute maps
- Use SQLite's built-in compression features

---

**Created**: 2026-04-17
**Last Updated**: 2026-04-17 16:11 UTC
**Completed Items**: 2
