# Session Plan: Fix TUI Compilation and Complete API Model Alignment

## Current Status

**Uncommitted Changes:**
- API model alignment work (otelite-core/api.rs created, CLI/TUI models updated)
- 15 files changed, 100 insertions(+), 609 deletions(-)
- Work appears to be from bead otelite-8yo (CLI/TUI model alignment to dashboard API)

**Compilation Errors:**
4 import errors in otelite-tui preventing compilation:
1. `crates/otelite-tui/src/state/logs.rs:211` - missing `Resource` import
2. `crates/otelite-tui/src/ui/traces.rs:797` - missing `Resource` and `SpanStatus` imports
3. `crates/otelite-tui/src/ui/metrics.rs:447` - missing `HistogramBucket` import
4. `crates/otelite-tui/src/ui/metrics.rs:483` - missing `Quantile` import

## Root Cause

The `crates/otelite-tui/src/api/models.rs` file re-exports main types from `otelite_core::api` but doesn't re-export these helper types:
- `Resource`
- `SpanStatus`
- `HistogramBucket`
- `Quantile`

## Solution

Add missing re-exports to `crates/otelite-tui/src/api/models.rs`:

```rust
pub use otelite_core::api::{
    HistogramBucket, LogEntry, LogsResponse, MetricResponse, MetricValue,
    Quantile, Resource, SpanEntry, SpanStatus, TraceDetail, TraceEntry,
    TracesResponse,
};
```

## Next Steps

1. ✅ Create this plan document
2. Switch to code mode to fix the imports
3. Run `cargo test --workspace --no-run` to verify compilation
4. Run `cargo test --workspace` to check for test failures
5. If tests pass: commit, push, and move to next bead
6. If tests fail: address test assertion issues (likely otelite-ndm bead scope)

## Available Beads (from bd ready)

- **otelite-wtc** (P2): Add documentation tests (doc tests)
- **otelite-ico** (P2): Add mutation testing with cargo-mutants
- **otelite-d5y** (P2): Add property-based testing with proptest
- **otelite-ndm** (P2): Fix remaining test assertions after model alignment
- **otelite-x6a** (P2): Add criterion benchmarks for storage and receiver
- **otelite-cvt** (P2): Standardize dependency versions and bump ratatui
- **otelite-cp2** (P2): Configurable debug logging (file and stderr)
- **otelite-3qj** (P2): Generate and serve OpenAPI specification
- **otelite-25s** (P2): Query parser: parse structured filter expressions
- **otelite-2he** (P2): OTLP specification conformance test suite

## Notes

- The uncommitted changes appear to be completing the API model alignment work
- Need to verify if this is continuation of otelite-8yo or separate work
- After fixing compilation, may need to address test assertions (otelite-ndm scope)
