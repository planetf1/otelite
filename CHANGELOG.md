# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Entries are written for **end users** — LLM application developers using otelite
to debug their apps. Each bullet describes what users can now *do* (or no longer
have to work around), not implementation detail.

## [Unreleased]

### Added

- Run OTLP/gRPC and OTLP/HTTP receivers on loopback by default, choose their
  listener addresses consistently for `serve`, `start`, and `restart` with
  `--grpc-addr` / `--http-addr` or environment variables, and see the resolved
  endpoints in startup, daemon status, health, and the web status popover.

## [0.1.83] - 2026-08-27

### Added

- **Daily output-throughput views in the web dashboard and TUI.** When the
  selected window spans more than one day, the web Latency section gains an
  "Output throughput by day" table (day × model with calls, the
  throughput-eligible sample, and tok/s p10/p50/p90) aligned to your local
  timezone; the TUI gains a matching daily throughput panel (aligned to
  `$TZ`, or UTC when unset). Weak samples (fewer than 10 eligible calls)
  are marked `†`, and both make clear that tok/s is derived end-to-end
  output throughput — span duration includes provider queue and network
  time, so it is not a provider-reported generation rate.
- **Versioned parity fixture for the throughput analytics family.** The
  API and CLI JSON for the latency/throughput panels is now frozen against
  a versioned fixture of spans (populated, low-sample, cached,
  missing-output, buffered-TTFT and rerouted cohorts), so the API, CLI and
  web surfaces stay byte-compatible. New `docs/throughput-analysis.md`
  documents the throughput formula, percentile estimator, outcome
  inclusion, model identity and the rolling vs calendar-day bucketing
  rules.

## [0.1.82] - 2026-08-27

### Added

- **Provider-aware model identity and rerouting visibility.** Model series
  are now grouped by `provider/model` (bare model name when no provider is
  recorded), so two providers serving the same model name no longer merge
  into one series. A provider's silent rerouting (requesting one model,
  served by another) stays in the requested model's series: the by-model
  table gains `Resp model` (dominant differing response model) and
  `Rerouted` (call count) columns when any rerouting is observed, and the
  model-drift view now honours all known attribute spellings. No
  canonicalisation or family grouping is applied — identifiers stay as
  recorded.
## [0.1.81] - 2026-08-27

### Added

- **Exact time ranges, repeatable model filters and throughput toggles for
  `otelite usage`.** `--start`/`--end` now accept exact values (`YYYY-MM-DD`,
  `YYYY-MM-DDTHH:MM:SS[.ffffff]` — UTC when no zone is given — or epoch
  seconds/nanoseconds) instead of only rolling `--since` durations. `--model`
  can be repeated; a plain value matches exactly, a value with `*` is a glob
  (`claude-opus-*`), and the pattern is applied to every panel — summary
  totals included — on the same cohort. `--latency-series` gains
  `--throughput` (per-bucket tok/s p10/p50/p90 + eligible-call count) and
  supports `--calendar-day`/`--timezone` like the percentile views.

## [0.1.80] - 2026-08-27

### Added

- **Calendar-day latency buckets with timezones.** The latency percentiles
  endpoint (and `otelite usage --latency-percentiles --calendar-day
  [--timezone Europe/London]`) can now bucket by local calendar day in any
  IANA timezone. DST days are correctly 23 or 25 hours, every day in the
  range is present (empty days show zero samples instead of vanishing),
  each bucket carries an explicit end timestamp, and calls are attributed
  to the day they started. The mode is explicit — the default rolling
  `--bucket-secs` grid is unchanged.

## [0.1.79] - 2026-08-27

### Added

- **Lower-tail throughput percentiles.** Latency stats (CLI `usage --latency`,
  GenAI latency table, TUI latency panel) and the bucketed latency percentiles
  now report derived end-to-end output throughput as p10/p50/p90 tok/s —
  lower-tail, median, upper-reference — with the throughput sample count
  (`N`) shown separately from total call count. Buckets with fewer than 10
  eligible calls are flagged (†) because the p10 is a weak estimate at that
  size. Duration/TTFT percentile series also expose the lower-tail p10.

### Changed

- Derived tok/s is now computed per call from the raw nanosecond span
  duration (previously integer-millisecond durations truncated
  sub-millisecond calls); values can shift slightly, and the label makes
  explicit that span duration includes provider, queue and network time —
  it is not pure generation throughput. p95/p99 throughput fields remain in
  the API responses during a compatibility period.

## [0.1.78] - 2026-08-27

### Fixed

- The GenAI view's Cost and Latency charts no longer fail to load with a
  `brushAttrs is not defined` console error; all four time-series charts
  render and support brush-to-focus zoom again.
- The cache hit rate endpoint no longer returns a 500 error on every
  request. The per-model cache hit rate panel (and the TUI panels that read
  the filter-wrapped GenAI list endpoints) work again.

## [0.1.77] - 2026-08-27

### Added

- **Brush-to-focus time zoom in GenAI analytics.** Drag across any
  time-series chart (cost, latency, percentiles, request volume) and every
  section refetches for the selected window — Grafana-style
  zoom-to-selection. A chip shows the zoomed range; its Clear button or
  <kbd>Esc</kbd> restores the previous window, and the zoomed window is
  written to the URL (`#/analytics?…&start=…&end=…`) so a zoomed view
  round-trips as a link. Clicks and sub-minute drags do not zoom, and
  auto-refresh keeps updating the zoomed window while you're in it.

### Changed

- The Tips &amp; shortcuts panels in the GenAI, Logs and Traces views are
  collapsed by default on every load (previously the Logs and Traces tips
  opened by default and the panel state was remembered between visits).

- **Global filter bar for GenAI analytics.** The GenAI tab and the Sessions
  tab gain a shared filter bar — agent (claude / opencode / codex), model,
  provider, project and session — that scopes every analytics section at
  once. The active filters live in the URL (`#/analytics?agent=claude&model=…`),
  so a filtered view round-trips as a link; Clear removes the filters but
  keeps the time window. Each section shows which dimensions its data
  actually honours — dimensions no loaded endpoint supports are greyed out,
  based on the `filters_applied` echo every GenAI endpoint now returns.
  Filter params an endpoint can't apply are ignored, never a 400. The
  Sessions list honours all five dimensions; sessions costs echoes none.

## [0.1.76] - 2026-08-27

### Added

- **Per-project usage: see which project drove the bill.** The GenAI tab
  gains a Projects section and the CLI gains `otelite projects` — per-project
  sessions, cost, token totals and the top models. opencode attributes
  activity by its `project.id` label; codex and claude emit no project label
  today, so their activity is grouped under a clearly-labelled
  `unattributed` row (opencode's own cost counter is `actual`; the rest is
  priced from tokens and marked `estimated`/`mixed`).

- **Latency percentiles: see the tail, not just the average.** The GenAI tab
  gains latency percentile charts (duration and time-to-first-token) with a
  model filter, and the CLI gains
  `otelite usage --latency-percentiles [--model <m>] [--bucket-secs <n>]` —
  p50/p90/p95/p99 per time bucket, all models and per model. TTFT includes
  the codex turn histogram, which is the only TTFT source for codex.

- **Distributions: the shape of any of five cohorts.** One new endpoint,
  `GET /api/genai/distributions?metric=<session_cost|tool_duration|llm_duration|ttft|output_tokens>`,
  returns binned values (linear or log-spaced, up to 100 buckets) plus
  min/mean/p50/p95/p99/max, and the CLI gains
  `otelite histogram <metric> [--since 24h] [--buckets 20] [--scale log]`
  with ASCII bars and the stats line. The GenAI latency section shows the
  request-duration distribution with a linear/log toggle.

- **Zoom into one session.** The Session Report modal now includes a
  session-context section: every span and log for the session (with totals
  when truncated), per-metric aggregates (counts, sums, min/max), and a
  merged timeline — plus "Spans → Traces" and "Logs → Logs view" cross-links
  that open the filtered views. The CLI gains
  `otelite sessions context <session-id> [--start <ns>] [--end <ns>]
  [--limit 500]` and the API gains
  `GET /api/sessions/{id}/context`. Coverage is reported honestly per agent:
  claude is full; opencode spans are partial (only llm/tool spans are
  session-labelled) and codex only exposes `mcp.tools.call` spans, its logs
  via `conversation.id`, and no per-session metrics. The report also fixes
  the GenAI latency percentiles' model filter and the distribution
  chart's scale toggle, which were never wired up.

## [0.1.75] - 2026-08-27

### Added

- **Session cost analysis: which sessions spent what, and which are
  outliers.** The Sessions page now shows each session's cost — opencode
  sessions use their own cost counter, Claude Code sessions are priced from
  token usage (their cost counter under-reports) — plus a log-spaced cost
  distribution chart. Sessions spending more than 3× the median session cost
  are flagged as anomalies. Also available from the CLI:
  `otelite sessions costs --since 24h` (top-cost sessions) and
  `otelite sessions cost-hist --since 24h` (distribution), and from
  `GET /api/sessions/costs` / `GET /api/sessions/cost-distribution`.

## [0.1.74] - 2026-08-27

### Added

- **Per-agent rollup: who did what, and what it cost.** One table for
  opencode, codex and claude — sessions, spend, tokens (input/output/cache/
  reasoning), tool calls and failed requests — with a cost-over-time chart
  per agent. `otelite agents --since 7d` (or `--format json`), the new
  `GET /api/genai/agents` endpoint, and an "Agents" section on the web UI's
  GenAI tab. opencode's spend comes from its own cost counter; codex and
  claude are estimated from tokens × pricing (their cost counters
  under-report), and the distinction is shown as "actual" vs "estimated".
  Sub-agent sessions and codex sub-agent threads are excluded from session
  counts.

## [0.1.73] - 2026-08-27

### Added

- **Reasoning ("thinking") token share by model and effort.** See how much
  of each model's output was thinking, and what that thinking costs —
  `otelite reasoning --since 7d` (or `--format json`), the new
  `GET /api/genai/reasoning_share` endpoint, and a "Reasoning share by
  model" section on the web UI's Cost tab (per-model share bars with a
  per-effort breakdown for codex). Reasoning tokens are billed at each
  model's output rate, so unpriced models show no cost rather than a
  guessed one.

### Changed

- **The web UI's "Analytics" tab is now "GenAI".** Every panel on that tab
  (tokens, cost, models, cache, reasoning, agents, parameters, sessions,
  context) is LLM-specific, and otelite itself stays a generic
  OpenTelemetry store — the tab name now says what it actually shows. The
  page heading and all endpoints are unchanged.

## [0.1.72] - 2026-08-27

### Fixed

- **Cache-economics token figures were inflated for some models.**
  0.1.70/0.1.71 counted a resumed opencode session's re-flushed
  (unchanged) cache counter as fresh usage, overstating cache reads —
  by up to three orders of magnitude for models whose sessions were
  resumed. Flat counters now contribute only their actual change.

## [0.1.71] - 2026-08-27

### Fixed

- **`otelite cache` and the cache-economics API flag.** The
  `by_model=1` form documented for `GET /api/genai/cache_hit_rate`
  (and sent by the web UI) was rejected in 0.1.70, which only accepted
  `by_model=true`. Both forms now work.

## [0.1.70] - 2026-08-27

### Added

- **Quantify what prompt caching is saving you.** The *Cache economics by
  model* section on the GenAI Analytics page (and `otelite cache --since 7d`)
  shows, per model: tokens served from cache vs tokens written to it, the
  read:write ratio (how long the cache pays for itself), the hit rate, and
  an estimated savings in dollars (`cache reads × (input price − cache-read
  price)`). Models without a known cache-read price show `—` instead of a
  fabricated number. `otelite cache --series` adds a time-bucketed
  read/write chart. The API exposes this as
  `GET /api/genai/cache_hit_rate?by_model=1` (the endpoint's original
  per-model hit-rate list is unchanged without that flag).

## [0.1.69] - 2026-08-27

### Added

- **See your provider × model mix at a glance.** The new *Provider Mix*
  section on the GenAI Analytics page (and `otelite providers`) shows a
  stacked token-share bar plus a per-provider, per-model table across
  opencode, codex and Claude Code — which provider served which model, with
  token breakdown, session counts and estimated cost share. When a single
  model is served by several providers its tokens are split across them by
  each provider's share of that model's usage (the response reports
  `method: token-share-split` when that happens). Codex emits no provider
  attribute, so its models are grouped under `(unknown)` rather than
  guessed; cost is estimated from tokens × the pricing table as before.

## [0.1.68] - 2026-08-27

### Added

- **See which sub-agent drove your spend.** The new *Agent Roles* section on
  the GenAI Analytics page (and `otelite usage --agent-roles`) breaks tokens
  and estimated cost down by opencode's `agent` label — orchestrator vs
  reviewer vs executor and friends — with per-role session counts, cache
  read/write and reasoning split, token share, and the top models each role
  used. Cost is estimated from tokens × the pricing table (opencode's own
  cost counter arrives zero-valued); local or unpriced models show "—".

## [0.1.67] - 2026-08-27

### Fixed

- **Session reports and the remaining analytics sections load in well under
  a second instead of 20–300 seconds.** Opening a session's report from the
  Sessions tab no longer takes a minute or more (session-id lookups now use a
  dedicated index instead of scanning and JSON-parsing every span), and the
  finish-reasons, tool-usage, tool-approvals, tool-errors, retrieval-stats and
  hour-of-day analytics sections no longer scan the whole time window (each
  now runs on a narrow partial index). A metric's timeseries also no longer
  loads unrelated metrics from the database, and no longer re-reads the whole
  metrics table just to check whether the metric exists.

## [0.1.66] - 2026-08-27

### Fixed

- **Metrics, Traces and Logs views load instantly instead of taking minutes.**
  The metrics sidebar no longer waits 15–110 seconds on first open (the name
  list and the latest-value-per-metric list now use a covering index instead
  of scanning the whole metrics table), the trace list in the Traces view no
  longer scans every span in the time window to find the most recent traces,
  and the resource-attribute typeahead in the left nav no longer JSON-parses
  every row in the database. Corrupt resource JSON is skipped instead of
  failing the typeahead query.

## [0.1.65] - 2026-08-27

### Added

- **Overview and analytics pages load in seconds instead of 30+ seconds.**
  GenAI analytics queries now run on dedicated read connections with a warm
  page cache and a partial index over LLM spans, so dashboard widgets return
  as soon as their data is ready instead of queueing behind one database
  connection. Short-lived response caching also means the 30-second
  auto-refresh almost always answers instantly.
- **Auto-refresh no longer blanks the charts.** While the dashboard refreshes,
  existing charts stay visible (dimmed) until the new data arrives; if a
  refresh fails, the previous data is kept with a staleness notice instead of
  being wiped.

### Fixed

- **Analytics no longer fails when a corrupt span sits inside the time
  window.** Span records whose attributes JSON is malformed are now skipped
  by GenAI analytics queries instead of failing the whole query with a
  "malformed JSON" error.

## [0.1.64] - 2026-08-18

### Added

- **See call volume trend over time in CLI and TUI.** Added a new `--calls` flag to the `otelite usage` CLI subcommand and rendered a new "Call Volume Trend" panel in the TUI Usage tab, achieving full parity with the web UI's calls-over-time bar chart.

## [0.1.63] - 2026-08-18

### Fixed

- **Trace exports now report rejected malformed spans.** OTLP clients, HTTP
  ingestion and file imports distinguish accepted spans from records rejected
  for invalid trace or span identifiers.
- **Corruption diagnostics no longer warn for absent resources or expose
  telemetry identifiers.** Valid empty resource values remain quiet, while
  malformed stored JSON reports only its structural field and record type.
- **Usage ratio labels now describe output versus full context.** Terminal and
  TUI latency views no longer imply cache tokens are excluded.

## [0.1.62] - 2026-08-18

### Fixed

- **Invalid OTLP trace identifiers are rejected before storage.** Oversized,
  undersized, and all-zero trace or span IDs can no longer create corrupt trace
  records; malformed parent IDs are omitted from otherwise valid spans.
- **Malformed stored telemetry JSON now raises diagnostics.** Read operations
  retain their existing fallback values while logging the affected field and
  record identity, making data corruption visible without exposing payloads.
- **New telemetry preserves instrumentation scope.** Newly ingested logs,
  spans, and metrics retain their scope name and version for reliable
  attribution; existing stored data is unchanged.

## [0.1.61] - 2026-08-18

### Fixed

- **OpenCode prompt-cache token aliases are recognised.** Cache-read and
  cache-creation usage now appears consistently in usage, analytics, and
  capability reports.
- **Output/context ratios now include prompt-cache tokens.** Latency and
  analytics views count uncached input, cache reads, and cache creation in the
  context denominator, so cached requests no longer appear falsely verbose.

## [0.1.60] - 2026-08-18

### Added

- **GenAI telemetry capability API.** `GET /api/genai/capabilities` shows which
  per-model metrics are native, unavailable, sparse, invalid, or degenerate,
  including their source attributes and duplicate-delivery count. This makes
  missing token or TTFT data explicit instead of silently estimating it.

## [0.1.59] - 2026-08-18

### Fixed

- **Model cost estimates now recognise Claude Code's `[1m]` context-window
  label.** Those requests use the underlying model's LiteLLM rate instead of a
  stale fallback rate, so usage reports no longer imply a false 1M-token price
  premium.
- **Buffered LLM responses no longer report completion time as first-token
  latency.** Latency tables, trends, and context-size views now label affected
  groups with the observed buffered share instead of presenting misleading
  TTFT percentiles.

## [0.1.58] - 2026-08-14

### Added

- **Codex CLI model calls now appear in Analytics.** Request counts and latency
  are derived from Codex’s native sampling-request spans without changing the
  raw trace view. Token and cost totals remain unavailable when Codex does not
  emit usage attributes.

### Fixed

- **`otelite status` and `otelite stop` now recognise macOS launchd services
  and locally run Otelite processes.** Service-managed daemons report their
  launchd supervisor instead of appearing stopped, and local `serve` processes
  can be stopped without a PID file.

## [0.1.57] - 2026-08-10

### Added

- **Latency diagnosis card** in the Analytics → Latency section: when TTFT (time-to-first-token) accounts for ≥ 85% of total response time, a plain-language card appears explaining that the wait is provider-side inference — not local tooling, context size, or network overhead — and suggests routing lighter turns to a faster model. Expands to show the per-model TTFT/duration ratio. Visible automatically with no new configuration required.

## [0.1.56] - 2026-08-10

### Internal

- Harden `release.yml` against Semgrep SAST: replace `curl | sh` rustup install with `dtolnay/rust-toolchain` action (no user-visible change).

## [0.1.55] - 2026-08-10

### Internal

- Pin all GitHub Actions `uses:` references in `release.yml` to full commit SHAs
  and replace `curl | sh` cargo-dist installer with a checksum-verified download
  (resolves remaining Semgrep SAST findings in the release workflow).

## [0.1.54] - 2026-08-10

### Internal

- Pin all GitHub Actions `uses:` references to full commit SHAs (supply-chain
  hardening; resolves Semgrep SAST mutable-tag findings across ci.yml,
  security.yml, bump-and-tag.yml, publish.yml).
- Update `quinn-proto` to 0.11.16 (fixes RUSTSEC-2026-0185 — remote memory
  exhaustion via unbounded out-of-order stream reassembly, severity 7.5 high).
- Update `anyhow` to latest (mitigates RUSTSEC-2026-0190 unsoundness warning).

## [0.1.54] - 2026-08-10

### Added

- **CLI `otelite usage`: five new flags for Claude Code analytics.**
  `--tool-approvals` shows the auto-accept / user / rejected decision breakdown.
  `--stop-reasons` shows the `stop_reason` distribution (tool_use vs end_turn).
  `--context-split` shows token usage grouped by `llm_request.context`
  (interaction vs sub_agent). `--tool-errors [N]` lists the top N failed tool
  executions by error message. `--hour-of-day` shows an activity-by-hour table
  (UTC, 0–23, LLM calls + tool calls).

- **Dashboard: five new analytics panels.** The Behavior section gains Tool
  Approval Decisions (auto-accept rate gauge + top rejected tools), Top Tool
  Errors table, and Activity by Hour of Day bar chart. The Reliability section
  gains a Stop Reasons bar chart (claude_code `stop_reason` attribute). The
  Cost section gains a Usage by Request Context table grouped by
  `llm_request.context`. All panels degrade gracefully when no data is present.

- **TUI Usage view: tool approvals, stop reasons, context type split, tool
  errors, and hour-of-day activity panels.** The Usage tab now shows five new
  panels: approval rate breakdown (auto/manual/rejected), stop reason
  distribution (tool_use vs end_turn), context type split with avg latency,
  per-tool error counts, and an activity-by-hour-of-day table. Panels are shown
  only when data is available; all fetches are best-effort so the view degrades
  gracefully when the server is not running these endpoints.

### Fixed

- **`query_hour_of_day` time-range filter now uses `end_time` column** (was
  accidentally using `start_time` for the upper-bound filter, causing wrong
  counts when a time window was specified).
- **`query_tool_errors` now matches boolean `false`** as well as the string
  `"false"` for the `success` attribute, so spans instrumented with a JSON
  boolean are included.

## [0.1.53] - 2026-08-10

### Fixed

- **Model Mix panel and all GenAI analytics now show Claude Code data.** Claude
  Code spans (`claude_code.llm_request`) use flat `model`/`input_tokens` attributes
  instead of the standard `gen_ai.*` markers. The LLM span guard now includes a
  span-name prefix match so every analytics query — Overview Model Mix, Cost,
  Latency, Reliability, Behavior — picks up Claude Code telemetry correctly.
  Add any vendor-specific span name prefix to `VENDOR_SPAN_NAME_PREFIXES` in
  `semconv.rs` to support future non-standard instrumentations without touching SQL.

## [0.1.52] - 2026-07-21

### Fixed

- **Sessions list: input/output token columns now show real values for Claude
  Code sessions.** The sessions list was always showing 0 tokens because it read
  token counts only from the root `claude_code.interaction` span, which carries
  none. Token counts are now aggregated from child `llm_request` spans, matching
  the fix already applied to the Session Report modal.

- **Token counts now populated for all instrumentations that emit bare attribute
  names.** `GenAiSpanInfo` previously only recognised OTel semconv names
  (`gen_ai.usage.input_tokens` etc.). Claude Code and other instrumentation
  libraries emit bare names (`input_tokens`, `output_tokens`,
  `cache_creation_tokens`, `cache_read_tokens`). Both forms are now accepted,
  semconv-prefixed names taking priority. This fixes zero-token display across
  sessions, diagnose, analytics, and the TUI for Claude Code traces.

## [0.1.51] - 2026-07-21

### Added

- **CLI `diagnose`: performance summary block.** `otelite diagnose <session>` now
  prints a summary before the per-interaction table: Total LLM time, p95 turn
  time, slowest turn, slow-turn count (>30 s), cold-start count, and a warning
  when total output exceeds 50 K tokens or p95 exceeds 60 s.

- **CLI `diagnose`: cache-state column.** The per-interaction table gains a
  `Cache` column showing `COLD` / `WARM` / `HOT` / `—` for each turn so you can
  quickly see which interactions rebuilt context from scratch.

- **TUI Latency table: o/i ratio p95 column.** The latency usage table now
  includes an `o/i p95 ⚠` column — rows where the p95 output/input token ratio
  exceeds 200× are highlighted yellow-bold, flagging generation-dominated turns
  as the likely slowness driver.

- **TUI Latency table: slow p95 highlight.** Rows where p95 duration exceeds
  30 s are highlighted amber so the slowest model tiers stand out at a glance.

- **Web Traces: "Slow >30 s" quick-filter button.** A one-click filter above the
  trace list lets you instantly narrow to traces longer than 30 seconds without
  typing an attribute filter.

- **Web Logs: session badge in log rows.** Non-LLM log entries now show a compact
  session ID pill in the collapsed header row; clicking it opens the Session
  Report modal for that session.

- **Web Analytics: session cell navigates to Session Report modal.** Clicking a
  session ID in the Top Spans or latency group tables now opens the full Session
  Report instead of jumping to raw logs.

- **Web Analytics: conversation nav in group table fixed.** Conversation cells
  in the Analytics group table now correctly navigate to traces-by-conversation
  (was navigating to logs-by-session for both sessions and conversations).

## [0.1.50] - 2026-07-21

### Added

- **Sessions list: Duration column.** Each session row now shows wall-clock
  duration (last − first event) so you can see at a glance how long a session
  ran without opening the detail modal.

- **Sessions list: Traces / Logs cross-nav buttons.** Hovering a session row
  reveals inline *Traces* and *Logs* buttons that jump directly to the
  respective view pre-filtered to that session — no need to open the Session
  Report first.

- **Traces list: slow-trace duration highlight.** Trace durations over 30 s now
  appear in amber so long-running traces stand out without scanning the numbers.

- **Traces list: session and model badges.** Trace rows now show colour-coded
  `session` and model-name pills when those attributes are present on the trace.

- **Waterfall: slow-span `slow` badge.** Any span taking more than 5 s in the
  waterfall now shows an amber `slow` badge and the duration value is
  highlighted, immediately drawing attention to the bottleneck spans.

- **Waterfall: LLM and tool badge colour coding.** The inline GenAI badge
  in waterfall rows is now indigo for LLM inference spans and green for tool
  calls, visually separating thinking time from tool execution time.

- **Waterfall: cache state pills inline.** LLM spans in the waterfall now
  show `COLD`/`WARM`/`HOT` cache state pills alongside the token counts so
  you can spot cold-start rebuilds without opening the span detail.

- **Span detail: trace_id is now a clickable link** to the Logs view
  pre-filtered to that trace, completing the span → logs navigation path.

- **Session modal now shows token counts per interaction.** Claude Code sessions
  previously showed blank input/output/cache columns in the interaction table
  because the session root span carries no tokens. Counts are now aggregated
  from child `llm_request` spans so every row shows real numbers.

- **Session modal: performance summary strip.** Clicking a session row now
  shows a chip row with Total LLM time, Slowest turn, p95 turn, and warning
  badges for slow turns (>30 s) and cold-start turns, plus Total output/input
  token counts. Slow interactions are highlighted amber inline.

- **Session modal: actionable findings panel.** Auto-generated diagnostic
  sentences appear below the summary strip when the session has notable issues:
  cold-start rebuilds, high output-token volume on slow turns, and a p95 >60 s
  overall.

- **Session modal: cache state badges.** Each interaction row in the session
  detail table now shows a `COLD` / `WARM` / `HOT` badge indicating how well
  the prompt cache was used that turn.

- **Analytics → Behavior: tool total-time sort.** The MCP/tool-call table is
  now sorted by total wall-clock time (not call count), and includes an inline
  bar so the costliest tools surface immediately.

- **Analytics → Latency: high output-ratio warning.** Rows where p95
  output/input ratio exceeds 200× are highlighted amber so generation-dominated
  latency is obvious at a glance.

- **Analytics → Top Spans: cache state badges.** Every span row now shows
  `COLD` / `WARMING` / `HOT` to identify cold-start requests across all models.

- **Overview: anomaly strip.** The Overview landing page now shows a horizontal
  strip of clickable alert chips whenever the last 7 days contain slow calls
  (>5 min), cold-start spans, high-ratio output events, or slow tools — with
  direct links to the relevant analytics view.

## [0.1.49] - 2026-06-22

### Added

- **Trace→log correlation endpoint.** `GET /api/traces/{id}/logs` returns all
  logs associated with a trace in a single call. AI agents and scripts can now
  fetch logs for a trace without a second round-trip to the logs endpoint.
  Internal: refactored `otelite traces logs` and `otelite diagnose` to use the
  consolidated endpoint. Closes #106.

## [0.1.48] - 2026-06-05

### Fixed

- **gRPC receiver no longer panics when the Python OTel SDK connects.** The
  `max_frame_size` was set to 16,777,216 (16 MiB), which is one byte over the
  HTTP/2 maximum of 16,777,215. The h2 crate asserts this limit on every new
  connection, crashing the worker thread before any data could be received.
  Value corrected to `(1 << 24) - 1`.

## [0.1.47] - 2026-06-05

### Fixed

- **Metrics API now rejects malformed `attrs` filters instead of ignoring them.**
  `GET /api/metrics?attrs=not-json` returns `400 Bad Request` with a clear
  message, so users no longer receive unfiltered metric results after a typo in
  the JSON filter parameter.

- **API time filters now reject invalid ranges.** Logs, traces, and metrics
  endpoints return `400 Bad Request` for negative timestamps or
  `start_time >= end_time` instead of silently returning an empty response;
  valid ranges with no data still return a normal empty `200 OK` response.

## [0.1.46] - 2026-05-28

### Internal

- Clarify bump-and-tag retry behaviour in AGENTS.md changelog discipline section.

## [0.1.45] - 2026-05-25

### Internal

- style: `cargo fmt` on `extract_ttft_secs` in `otelite-core` (trailing-newline fix missed in v0.1.44 release commit).

## [0.1.44] - 2026-05-22

### Fixed

- **TTFT values are now correct for Claude Code spans.** Claude Code emits
  `ttft_ms` (milliseconds); previous releases treated that value as seconds,
  displaying e.g. "1200.0s" instead of "1.2s" in `otelite diagnose`, the
  Session Report modal, and the `SessionDiagnoseResponse` JSON. The shared
  `extract_ttft_secs()` helper in `otelite-core` now converts `ttft_ms` to
  seconds; `gen_ai.server.time_to_first_token` (OTel spec, already seconds)
  is used as-is. Closes #98.

### Added

- **`otelite diagnose` shows cache-write spikes.** A new `Cache+` column in
  both the CLI table and the Session Report modal shows `cache_creation_tokens`
  per interaction — tokens written to the prompt cache that turn. A spike marks
  the interaction where context grew, explaining why subsequent requests became
  slower. Closes #99.

- **Escalation block includes request body size and proxy correlation ID.**
  For errored interactions, `otelite diagnose` and the Session Report modal now
  show `body_length` (bytes, with a rough token estimate) and `prompt.id`
  (LiteLLM's proxy-level correlation ID) — the fields proxy/cloud admins ask
  for first. Closes #100.

- **Session Report modal shows a timeout-fix suggestion.** When streaming stalls
  are detected the modal now recommends a stream-idle timeout value (longest stall
  + 200 s, minimum 500 s), mirroring `otelite diagnose --suggest`. Closes #101.

## [0.1.43] - 2026-05-22

### Added

- **Web: Sessions tab** — new top-level tab lists all sessions with span count, token
  totals, and duration at a glance. Click any row to filter the traces view to that
  session instantly.
- **Web: Overview landing page** — five summary widgets (recent traces, error rate,
  token usage, latency, top models) load lazily so the app feels fast even on large
  datasets. Default time window is now 1 day.
- **Web: Analytics accordion** — Usage metrics are now grouped into collapsible
  sections (Token Usage, Latency, Model/Provider breakdown) making the tab easier
  to navigate.
- **API: `GET /api/sessions`** — list endpoint returning all sessions with aggregate
  stats; backs the new Sessions tab and is usable from any HTTP client.

### Internal

- CI: fix parallel-test SQLite lock in receiver unit tests; recalibrate coverage thresholds.

## [0.1.42] - 2026-05-21

### Added

- **Web: Session Report modal in traces view.** When a trace is filtered by session,
  a "Session Report" banner appears above the list. Clicking it opens a modal with
  the full per-interaction table (input/cached/output tokens, TTFT, duration, status),
  context growth, streaming stall warnings, and an escalation block. Each trace ID
  links back to its waterfall.
- **Web: "Session Report" button on `session.id` span attribute.** The `session.id`
  row in any span's attribute panel now shows a "Session Report" button alongside the
  existing filter link — one click from any open waterfall, no intermediate navigation
  required.
- **API: `GET /api/sessions/:session_id/diagnose`** — server-side endpoint backing
  the session report. Returns `SessionDiagnoseResponse` with per-interaction breakdown
  and context growth; usable from any HTTP client.

## [0.1.41] - 2026-05-21

### Added

- **CLI: `otelite logs show <id> --full` dumps the raw body to stdout.** Useful when the body is a
  large JSON payload (e.g. a 324KB LLM request): pipe it directly to `jq`, `grep`, or a file
  without any formatting overhead. Example: `otelite logs show 171700000000000 --full | jq .messages[-1].content`
- **API + CLI: log bodies now report their original byte size.** `LogEntry` carries `body_length`
  (total bytes) and `body_truncated` (true when the list endpoint capped the body at 512 bytes).
  The single-entry endpoint (`GET /api/logs/:id`) and `logs show` always return the full body.
  Pretty output shows `Body (NNN bytes):` for large bodies so you know when to reach for `--full`.
- **Web: GenAI Usage page is split into four section tabs** — Overview, Performance, Quality, and
  Details. The earlier single-scroll page grew too long; now each concern is on its own tab. Tab
  selection persists across the 30-second auto-refresh.
- **Web: latency-over-time chart on the Performance tab.** Shows average and p95 latency per hour
  as a two-tone bar chart, grouped across all models.
- **Web: latency by context-size table on the Performance tab.** Breaks down average latency, p95,
  and max by input token bucket (0–1K, 1K–10K, 10K–50K, 50K–100K, 100K+) so you can see how
  request size drives latency.
- **CLI: `otelite usage --latency-context`** prints a per-(model, context-bin) latency table.
- **CLI/API: `--span-filter=all` on series endpoints** includes non-GenAI spans in latency and
  calls series queries, grouping by span name instead of model.
- **Traces: streaming stall detection.** `otelite traces show` marks spans where a stream started
  (TTFT present) but the span ended in error after >30 s as `[streaming stall]`.
- **CLI: `otelite diagnose <session-id>`** — one-shot forensic report for an LLM session. Lists
  every interaction with its token counts (input, cached, output), TTFT, duration, and error/stall
  status. Highlights context growth across the session, calls out streaming stalls with optional
  `--suggest` remediation advice, and prints a copy-paste escalation block with timestamps,
  response IDs, and trace IDs.
- **Docs: LLM observability guide** (`docs/llm-observability.md`). Covers end-to-end setup for
  Claude Code, Anthropic SDK, OpenAI SDK, and LiteLLM; explains every GenAI attribute; and walks
  through the investigation workflow from `otelite usage` down to `otelite diagnose`.

## [0.1.40] - 2026-05-21

### Fixed

- **Storage: database no longer corrupts on OS crash or power loss.** SQLite was
  configured with `synchronous=NORMAL`, which skips fsyncing WAL frames and can
  leave the database in an unrecoverable state after an unclean shutdown. Switched
  to `synchronous=FULL` — each committed frame is fsynced before acknowledgement.
  You may lose at most the last in-flight transaction on a hard crash, but the
  database will always open cleanly.
- **Storage: nightly retention purge no longer silently fails to reclaim space.**
  The scheduled purge was attempting `VACUUM` from a second connection while the
  main connection was open. `VACUUM` requires exclusive access and was failing
  silently every night. Replaced with `PRAGMA wal_checkpoint(PASSIVE)`, which is
  safe to run alongside an open connection and correctly flushes WAL frames after
  bulk deletes.

## [0.1.39] - 2026-05-15

### Fixed

- **Web: Status popover now shows the actual configured OTLP ports.** Previously
  the gRPC (`:4317`) and HTTP (`:4318`) values in the status popover were
  hardcoded strings. They are now read from the `/api/health` response, so if
  you run `otelite serve` with non-default ports the popover will reflect them.
- **CLI: `otelite usage --since invalid` now gives a clear error at parse time.**
  Previously the invalid format was silently accepted by clap and only failed
  later with a generic message. Now rejected immediately with
  `"Invalid time duration '…'. Use '1h', '24h', '7d', '30d'"` — before any
  network or storage work begins. Two smoke tests (`usage_since_invalid_format_rejected`,
  `usage_since_valid_formats_accepted`) guard this.

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
