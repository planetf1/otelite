# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Entries are written for **end users** — LLM application developers using otelite
to debug their apps. Each bullet describes what users can now *do* (or no longer
have to work around), not implementation detail.

## [Unreleased]

### Fixed

- **Metrics API now rejects malformed `attrs` filters instead of ignoring them.**
  `GET /api/metrics?attrs=not-json` returns `400 Bad Request` with a clear
  message, so users no longer receive unfiltered metric results after a typo in
  the JSON filter parameter.

## [0.1.39] - 2026-05-15

### Fixed

- **Web: API errors now surface as readable messages instead of an empty view.**
  Failed fetches (5xx, network errors) now show the server's error detail (e.g.
  "HTTP 500: database not initialized") rather than leaving the panel blank.
  The fetch wrapper in `api.js` parses the JSON error body; all view `catch`
  blocks pass the message through to the on-screen error banner.
- **Web: Loading and empty states are now visually distinct.** Logs, Traces,
  and Metrics views show a spinner while the initial fetch is in flight. Once
  the response arrives empty, the empty-state message ("No logs found", etc.)
  replaces it — so users can tell whether data is still arriving or genuinely
  absent.
- **Receiver: 11 edge-case conversion scenarios now have explicit tests.**
  Covers NaN gauge values, negative-integer counter wrapping, negative-float
  counter saturation (→ 0), ExponentialHistogram silent-skip, zero-bucket
  histograms, `u64::MAX` timestamp bit-reinterpretation, deeply-nested
  `ArrayValue`/`KvlistValue`, `None`-value attributes, and 120-attribute spans.
  All document existing behaviour so future changes to the casting logic are
  caught immediately.

## [0.1.38] - 2026-05-15

### Fixed

- **`otelite usage` no longer panics on every invocation.** A clap flag
  collision between the global `--format` and a duplicate local one in the
  `usage` subcommand caused `thread 'main' panicked: Mismatch between
  definition and access of 'format'` on any `otelite usage ...` call. All the
  analytics flags shipped in v0.1.33 (`--latency`, `--tools`, `--error-types`,
  `--model-drift`, etc.) were unreachable from the CLI as a result. Removed
  the duplicate flag and route through the global `--format` (which now
  accepts `pretty` / `json` / `json-compact` for `usage` like other commands).
- **`otelite usage` no longer panics with "Cannot start a runtime from within
  a runtime"** — `create_storage` was spinning a fresh tokio runtime inside
  `#[tokio::main]`. Now async, awaited normally.

- **Service no longer crashes on storage mutex poison.** Any panic inside a
  database operation previously poisoned a `std::sync::Mutex`, causing every
  subsequent call to fail with a `PoisonError` and terminate the process.
  Replaced with `parking_lot::Mutex` (no poison semantics) across all 38 call
  sites in the storage layer.
- **Retention purge no longer blocks queries.** The nightly purge task
  previously held the main database connection lock for the entire delete run
  (potentially millions of rows). It now opens a dedicated connection, so reads
  and writes continue unaffected during purge.
- **`GET /api/metrics/aggregate` returns 200 instead of 404 when the time
  window is empty.** Previously, requesting an aggregate for a metric that
  exists but has no data in the selected window returned 404, making it
  impossible to distinguish "unknown metric" from "no data here". Now returns
  200 with `count: 0, result: 0.0`; 404 is reserved for metric names that
  have never been ingested.
- **`GET /api/metrics?limit=N` is now bounded.** Passing an arbitrarily large
  `?limit=` value could exhaust server memory. Capped at 1000 (matching the
  traces endpoint). The CLI `--limit` flag and the API field description now
  document this cap.

### Added

- **CLI smoke tests** (`crates/otelite/tests/cli_smoke_test.rs`) — every
  subcommand and every `usage` flag combination must parse and `--help` must
  exit 0. These would have caught both regressions above immediately. Runs in
  ~0.5s in CI.

## [0.1.37] - 2026-05-15

### Changed

- **Docs catch up with shipped features** — `docs/cli-reference.md` `otelite usage` section
  now lists all 16 flags (was 5) with a panel table and worked examples;
  `docs/tui-quickstart.md` gains a Usage view section with panel descriptions and the `u` /
  `r` keybindings; `README.md` GenAI feature blurb surfaces the analytics range (cost, TTFT,
  error types, tool success, model drift, etc.) instead of just "token counts".

## [0.1.36] - 2026-05-15

### Internal

- `.fastembed_cache/` ignored by git (local artifact from RAM/embeddings tooling).

## [0.1.35] - 2026-05-15

### Internal

- Release CI now rotates `CHANGELOG.md` `[Unreleased]` → `[X.Y.Z] - DATE`
  automatically and fails the release if `[Unreleased]` is empty. Stops
  shipped versions from going undocumented (see `scripts/rotate-changelog.sh`,
  `AGENTS.md` → Changelog discipline).

## [0.1.34] - 2026-05-15

### Changed

- **CHANGELOG backfilled** — v0.0.1 through v0.1.32 now have user-facing release
  notes. Previously most releases shipped with no notes; entries reconstructed
  from git history, written from the user's perspective.

### Internal

- `AGENTS.md` Definition of Done now requires a `CHANGELOG.md` `[Unreleased]`
  entry for user-visible changes (gate enforced from v0.1.35 onward).

## [0.1.33] - 2026-05-15

### Added

- **Error type breakdown** — when LLM calls fail, see them bucketed into actionable
  categories (`rate_limit`, `timeout`, `context_length`, `content_filter`, `auth`,
  `server_error`, `unknown`) instead of a single error rate. Available in CLI
  (`otelite usage --error-types`), TUI Usage tab, web UI, and via
  `GET /api/genai/error_types`. Answers "why are my LLM calls failing?" at a glance.
- **Model version drift detection** — surface cases where a provider silently routed
  your request to a different model snapshot than you asked for (e.g. you requested
  `claude-3-5-sonnet`, got `claude-3-5-sonnet-20241022`). CLI flag
  `--model-drift`, TUI section, web UI panel, `GET /api/genai/model_drift`.
- **Per-tool success rate in CLI/TUI** — tool-call success/error counts and average
  duration are now visible in the CLI (`--tools`) and TUI Usage tab (the web UI
  already had this). Spot which tools your agent calls most and which fail.
- **TTFT (time-to-first-token) p50/p95 in CLI and TUI** — the latency tables in
  `otelite usage --latency` and the TUI Usage tab now include TTFT percentile
  columns. Renders `—` when the model isn't streaming.

## [0.1.32] - 2026-05-08

### Fixed

- **Usage page now scrolls** — the web UI Usage tab no longer cut off content when
  it was longer than the viewport.
- **Mellea setup snippet corrected** — quickstart copy-paste now uses
  `uv pip install` and one `export` per line so it actually works on first try.

## [0.1.31] - 2026-05-07

### Fixed

- **Metrics chart bucket options** — restored the missing 15-minute and 1-day
  bucket choices so you can pick the right granularity for short-window debugging
  and multi-day trend views.

## [0.1.30] - 2026-05-07

### Added

- **Auto-bucket selection on metrics chart** — the histogram bucket size now
  matches the selected time window automatically (1m buckets for an hour, 1h for
  30 days, etc.) so the chart isn't either flat or unreadably noisy.

## [0.1.29] - 2026-05-07

### Added

- **Filter Usage analytics by model** — the Usage page now accepts a model filter,
  so you can isolate cost/latency/error stats for a single LLM in a multi-model app.

## [0.1.28] - 2026-05-07

### Fixed

- **Top-N tab persists across auto-refresh** — the selected tab (cost / latency /
  errors) no longer resets every refresh interval.
- **Clearer finish-reason labels** — display text uses human terms (e.g.
  `max_tokens`) instead of internal enum names.

## [0.1.27] - 2026-05-07

### Fixed

- **Crash on malformed `finish_reasons`** — type guards added so unexpected
  attribute shapes from custom instrumentations no longer 500 the API.

## [0.1.26] - 2026-05-07

### Added

- **Cost in CLI usage output** — `otelite usage` now shows estimated $ spend per
  model (matching the web UI), so you can spot cost spikes from the terminal.
- **Top-N analytics tabs in web UI** — dedicated tabs for top spans by cost,
  latency, and other axes on the Usage page.

## [0.1.25] - 2026-05-07

### Changed

- **Unified time-range filter UI** — the Metrics page time picker now matches
  Logs / Traces / Usage so behaviour is consistent across the app.

## [0.1.24] - 2026-05-07

### Added

- **Bookmarkable tabs (URL hash routing)** — the active tab (Logs, Traces, Usage,
  Metrics) is now reflected in the URL, so you can bookmark and share specific views.
- **Date inputs prefilled with sensible defaults** — time-range pickers no longer
  start blank.

## [0.1.23] - 2026-05-07

### Changed

- **Costs computed server-side** — pricing is now centralized so CLI, TUI, and web
  UI always agree on what a span cost. Previously the web UI did its own math.

### Fixed

- **Usage page polish** — chart axis labels, empty-column handling, and "All time"
  default range.
- **Claude finish_reasons edge cases** — no longer 500s when response body or
  attributes are malformed.

## [0.1.22] - 2026-05-07

### Added

- **OpenInference span support** — agent conversations from OpenInference-instrumented
  apps (e.g. LlamaIndex, LangChain via OpenInference) now render as chat bubbles,
  with span-kind chips (LLM / tool / retrieval) and a RAG widget showing retrieved
  documents in trace detail.

## [0.1.21] - 2026-05-07

### Fixed

- **Latency endpoint route** — corrected `/api/genai/latency` path so the Usage
  page latency table loads.

## [0.1.20] - 2026-05-07

### Added

- **Generic LLM analytics (any provider)** — latency p50/p95/p99, time-to-first-token,
  error rate, tool-use frequency, and retry counts are now surfaced for *any* LLM
  provider that emits OpenTelemetry GenAI spans (Claude, OpenAI, Gemini, local models).
  Previously only cost was generic; everything else was Claude-specific.

## [0.1.18 – 0.1.19] - 2026-05-07

### Added

- **Mellea integration quickstart** — one-liner setup snippet and docs for using
  otelite as the observability backend for Mellea agents.

## [0.1.17] - 2026-05-07

### Added

- **Usage analytics page** — new `Usage` tab in the web UI with a cost-over-time
  chart, top-N spans by cost/latency, and a finish-reason breakdown (`stop`,
  `max_tokens`, `tool_use`, etc.) so you can see *why* generations stop.

## [0.1.16] - 2026-05-07

### Added

- **Full `gen_ai.*` attribute capture** — every OpenTelemetry GenAI semantic-convention
  attribute is now indexed and queryable (request/response models, token counts,
  tool details, finish reasons, etc.).
- **Server-side attribute filtering** — Logs / Traces / Metrics APIs now accept
  arbitrary attribute filters as query params; no more downloading and filtering
  client-side.
- **Filter by LLM model name** — across all data views.

## [0.1.15] - 2026-05-07

### Fixed

- **Span event wall-clock times** — events from claude-sdk, LangChain, and other
  instrumentations now line up correctly on the trace timeline.
- **Token / cost aggregation across instrumentation styles** — totals now sum
  consistently regardless of which OTel instrumentation library produced the spans.

## [0.1.14] - 2026-05-07

### Added

- **Prompt-turn aggregation** — group related LLM calls by user-defined boundaries
  (e.g. one logical conversation turn) to see end-to-end latency and token spend
  per turn instead of per HTTP call.
- **Cache-tier breakdown** — separate prompt-cache hits from live compute in cost
  views, so you can see how much your caching strategy is saving.
- **Cost estimation for major providers** — Claude, OpenAI, Gemini, and others
  computed server-side using current public rate cards.

## [0.1.13] - 2026-05-07

### Added

- **Click `session.id` to pivot** — clicking a session ID in any log/trace/metric
  detail view filters the rest of the app to that session, so you can trace one
  user's request end-to-end.
- **Local-time timestamps everywhere** — CLI and web UI both render
  `YYYY-MM-DD HH:mm:ss.SSS` in the local timezone.
- **Currency formatting for costs** — CLI shows `$0.0123` instead of raw floats.
- **Span status messages in trace detail** — error messages from failed spans are
  now shown alongside their status code.
- **Resource attributes (host / arch / OS) in traces view** — quickly see which
  machine produced a trace.
- **Cleaner attribute lists** — internal noise (`duration_ms`, `otel.scope.*`)
  filtered from the displayed attribute list.

## [0.1.12] - 2026-05-07

### Fixed

- **Logs and traces UI use local time** — timestamps no longer display in UTC.

## [0.1.11] - 2026-04-30

### Fixed

- **Traces with many spans no longer drop from the list** — the trace list view
  was previously paginating mid-trace and losing some.
- **Default data directory restored to `~/.otelite/data`** — reverts an accidental
  path change in 0.1.10.

## [0.1.10] - 2026-04-30

### Added

- **JSON rendering in web UI** — attribute values and log bodies that contain JSON
  are syntax-highlighted with collapse/expand toggles (▶/▼) and a `[raw]` button
  to switch back to the original string. Small JSON values (≤ 400 chars) auto-expand;
  large ones (LLM prompts, responses) auto-collapse.
- **Truncated-JSON repair** — values that look like truncated JSON (start with `{`
  or `[` but don't parse) are auto-closed and pretty-printed with a small amber
  `[truncated]` badge so you can still read the structure.
- **Log ↔ trace cross-navigation** — `trace_id` is a clickable link in both
  directions: from a log to the trace's spans, and from a trace back to its logs.
- **Filter logs by trace** — `GET /api/logs?trace_id=...` returns only logs for a
  given trace.

### Changed

- `GET /api/logs` response now includes `trace_id` and `span_id` per entry
  (already stored; now exposed).

## [0.1.9] - 2026-04-30

### Added

- **GitHub repo link** — dashboard footer and startup banner link to the otelite
  repository for docs and issue reporting.

## [0.1.5] - 2026-04-30

### Added

- **`otelite import <file>`** — ingest a JSONL telemetry file captured during a
  CI/CD run or saved replay, without needing a live OTLP receiver. Auto-detects
  signal types (logs / traces / metrics) and tolerates malformed lines with a
  diagnostic summary.
- **Homebrew install on macOS** — `brew install otelite`.

## [0.0.1 – 0.1.4] - 2026-04-23 to 2026-04-24

### Added

- **Initial public release** — local OTLP receiver (gRPC :4317, HTTP :4318),
  SQLite storage, CLI (`logs` / `traces` / `metrics` / `serve`), TUI, and embedded
  web UI on port 3000. Designed for LLM application developers debugging on their
  own machine.

### Internal

- v0.1.1 – v0.1.4 were release-pipeline plumbing (crates.io publishing, GitHub
  Actions workflow, cargo-dist setup) with no user-visible changes.

## [0.1.0-alpha] - 2026-04-17

### Added

- Initial alpha
- Project constitution and 7 core principles
- Workspace structure, dev environment scripts, testing infrastructure
  (cargo-nextest, cargo-llvm-cov, fixtures)
- Code-quality tooling (clippy.toml, rustfmt.toml, pre-commit hooks)
- Security tooling (gitleaks config, secret-detection pre-commit)
- Initial documentation (README, contributing, architecture, this changelog)
