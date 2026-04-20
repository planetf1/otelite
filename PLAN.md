# Session Plan: Fix TUI Compilation and Complete API Model Alignment

## Current Status

**Uncommitted Changes:**
- API model alignment work (rotel-core/api.rs created, CLI/TUI models updated)
- 15 files changed, 100 insertions(+), 609 deletions(-)
- Work appears to be from bead rotel-8yo (CLI/TUI model alignment to dashboard API)

**Compilation Errors:**
4 import errors in rotel-tui preventing compilation:
1. `crates/rotel-tui/src/state/logs.rs:211` - missing `Resource` import
2. `crates/rotel-tui/src/ui/traces.rs:797` - missing `Resource` and `SpanStatus` imports
3. `crates/rotel-tui/src/ui/metrics.rs:447` - missing `HistogramBucket` import
4. `crates/rotel-tui/src/ui/metrics.rs:483` - missing `Quantile` import

## Root Cause

The `crates/rotel-tui/src/api/models.rs` file re-exports main types from `rotel_core::api` but doesn't re-export these helper types:
- `Resource`
- `SpanStatus`
- `HistogramBucket`
- `Quantile`

## Solution

Add missing re-exports to `crates/rotel-tui/src/api/models.rs`:

```rust
pub use rotel_core::api::{
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
6. If tests fail: address test assertion issues (likely rotel-ndm bead scope)

## Available Beads (from bd ready)

- **rotel-wtc** (P2): Add documentation tests (doc tests)
- **rotel-ico** (P2): Add mutation testing with cargo-mutants
- **rotel-d5y** (P2): Add property-based testing with proptest
- **rotel-ndm** (P2): Fix remaining test assertions after model alignment
- **rotel-x6a** (P2): Add criterion benchmarks for storage and receiver
- **rotel-cvt** (P2): Standardize dependency versions and bump ratatui
- **rotel-cp2** (P2): Configurable debug logging (file and stderr)
- **rotel-3qj** (P2): Generate and serve OpenAPI specification
- **rotel-25s** (P2): Query parser: parse structured filter expressions
- **rotel-2he** (P2): OTLP specification conformance test suite

## Notes

- The uncommitted changes appear to be completing the API model alignment work
- Need to verify if this is continuation of rotel-8yo or separate work
- After fixing compilation, may need to address test assertions (rotel-ndm scope)
