# Otelite CLI Reference

Two ways of getting data:

- `logs`, `traces`, `metrics`, `llm`, `retries`, `diagnose` and `tui` query a
  running `otelite serve` instance (default `localhost:3000`) over HTTP;
  point them elsewhere with `--endpoint`.
- The GenAI analytics commands — `usage`, `providers`, `cache`, `reasoning`,
  `agents`, `projects`, `sessions`, `histogram`, `capabilities`,
  `model-performance` — plus `mcp` open the local database directly
  (`~/.otelite/data`, or `OTELITE_DATA_DIR` if set). They need no running
  daemon and can be pointed at a different database.

All commands print results to the terminal. Output can be formatted as a
table (`--format pretty`, default) or JSON (`--format json`) for scripting.

## Global flags

These flags are available on every subcommand:

```text
--endpoint <URL>      Otelite API base URL [default: http://localhost:3000]
--format pretty|json  Output format [default: pretty]
--no-color            Disable colour output
--no-header           Omit table header row
--no-pager            Disable automatic paging of long output
--timeout <secs>      Request timeout [default: 30]
--log-level <lvl>     Log level: trace|debug|info|warn|error [default: info]
--log-file <path>     Log to file (daily-rotated) instead of stderr
--log-format <fmt>    Log format: text|json [default: text]
```

---

## Server management

```bash
otelite serve           # Start server in the foreground (Ctrl+C to stop)
otelite start           # Start as a background daemon
otelite stop            # Stop the running daemon
otelite restart         # Stop then start (picks up a freshly built binary)
otelite status          # Show daemon status
```

`serve` and `start` take `--addr <ip:port>` (default `127.0.0.1:3000`)
and `--storage-path <dir>` (default `~/.otelite/data`). The OTLP
receivers bind to the same IP as `--addr` on ports 4317 (gRPC) and
4318 (HTTP); override the ports with `OTELITE_OTLP_GRPC_PORT` and
`OTELITE_OTLP_HTTP_PORT`.

Useful environment variables:

```text
OTELITE_DATA_DIR           Data directory (database + PID file + logs)
OTELITE_OTLP_GRPC_PORT     OTLP gRPC port [default: 4317]
OTELITE_OTLP_HTTP_PORT     OTLP HTTP port [default: 4318]
RUST_LOG                   Standard Rust log filter (overrides --log-level)
```

`OTELITE_DATA_DIR` gives a fully isolated instance — point read-only
commands at another database, or run a second otelite alongside the
default one.

Daemon behaviour worth knowing:

- `start` refuses to spawn when a daemon is already listening on the
  OTLP gRPC port (service-managed or hand-run daemons leave no PID
  file), and fails loudly — removing the PID file — if the spawned
  process exits immediately (e.g. a port collision).
- The daemon shuts down gracefully on SIGTERM/Ctrl-C (in-flight work
  gets a short bounded drain), so `otelite stop` rarely needs to
  escalate to SIGKILL.
- Daemon logs are daily-rotated: `~/.otelite/data/otelite.log.YYYY-MM-DD`
  (or under `OTELITE_DATA_DIR`).
- A corrupt or stale PID file is recovered from automatically
  (removed, then the daemon is discovered via the OTLP port).

### System service

`otelite service` runs the daemon as a boot-persistent system service
instead of a `start`-spawned process:

```bash
otelite service install      # create the unit, then load and start it
otelite service uninstall    # stop it and remove the unit (data untouched)
```

On macOS this is a launchd service (`dev.otelite.daemon`); on Linux a
systemd unit. Any `OTELITE_*` environment variables set in your shell at
install time are carried into the service.

---

## Logs

### List recent logs

```bash
otelite logs list
```

```text
┌─────────────────────┬─────────────────────┬──────────┬─────────────────────────────────────┐
│ ID                  ┆ Timestamp           ┆ Severity ┆ Message                             │
╞═════════════════════╪═════════════════════╪══════════╪═════════════════════════════════════╡
│ 1776870650059000000 ┆ 2026-04-22 15:10:50 ┆ INFO     ┆ claude_code.api_request_body        │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ 1776870650054000000 ┆ 2026-04-22 15:10:50 ┆ INFO     ┆ claude_code.hook_execution_complete │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ 1776870649988000000 ┆ 2026-04-22 15:10:49 ┆ INFO     ┆ claude_code.hook_execution_start    │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ 1776870649988000000 ┆ 2026-04-22 15:10:49 ┆ INFO     ┆ claude_code.tool_result             │
└─────────────────────┴─────────────────────┴──────────┴─────────────────────────────────────┘
```

### Filter by severity

```bash
otelite logs list --severity ERROR
```

```text
┌─────────────────────┬─────────────────────┬──────────┬─────────────────────────────────────────┐
│ ID                  ┆ Timestamp           ┆ Severity ┆ Message                                 │
╞═════════════════════╪═════════════════════╪══════════╪═════════════════════════════════════════╡
│ 1776723334112526080 ┆ 2026-04-20 22:15:34 ┆ ERROR    ┆ Test ERROR log from inject_test_data.sh │
└─────────────────────┴─────────────────────┴──────────┴─────────────────────────────────────────┘
```

### Full-text search

```bash
otelite logs search "api_request" --limit 4
```

```text
┌─────────────────────┬─────────────────────┬──────────┬──────────────────────────────┐
│ ID                  ┆ Timestamp           ┆ Severity ┆ Message                      │
╞═════════════════════╪═════════════════════╪══════════╪══════════════════════════════╡
│ 1776870670153000000 ┆ 2026-04-22 15:11:10 ┆ INFO     ┆ claude_code.api_request_body │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ 1776870669982000000 ┆ 2026-04-22 15:11:09 ┆ INFO     ┆ claude_code.api_request      │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ 1776870666145000000 ┆ 2026-04-22 15:11:06 ┆ INFO     ┆ claude_code.api_request_body │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ 1776870665957000000 ┆ 2026-04-22 15:11:05 ┆ INFO     ┆ claude_code.api_request      │
└─────────────────────┴─────────────────────┴──────────┴──────────────────────────────┘
```

### JSON output for scripting

```bash
otelite --format json logs list --severity ERROR | jq '.[].body'
```

---

## Traces

### List recent traces

```bash
otelite traces list
```

```text
┌──────────────────────────────────┬────────────────────────────┬──────────┬────────┬───────┐
│ Trace ID                         ┆ Root Span                  ┆ Duration ┆ Status ┆ Spans │
╞══════════════════════════════════╪════════════════════════════╪══════════╪════════╪═══════╡
│ 382240cb628c341584d6ad2c1955929d ┆ claude_code.tool.execution ┆ 187016ms ┆ ERROR  ┆ 28    │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┤
│ 5844823bd19bd5aa18ac0968fcc368b0 ┆ claude_code.llm_request    ┆ 5121ms   ┆ OK     ┆ 1     │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┤
│ 53ecf8c87568682bc4f4344bbacb3059 ┆ claude_code.llm_request    ┆ 138240ms ┆ OK     ┆ 43    │
└──────────────────────────────────┴────────────────────────────┴──────────┴────────┴───────┘
```

### Filter by minimum duration

```bash
otelite traces list --min-duration 10s
```

### Show trace with full span tree

```bash
otelite traces show 53ecf8c87568682bc4f4344bbacb3059
```

```text
Trace ID: 53ecf8c87568682bc4f4344bbacb3059
Duration: 1828021ms
Status:   ERROR

Spans:
claude_code.interaction (1819151ms)
  terminal.type: ghostty
  span.type: interaction
  user_prompt_length: 159
  session.id: 19b6add0-e3af-422e-9863-00cc100f0d74
  ├─ claude_code.llm_request (31321ms) [Anthropic] aws/claude-sonnet-4-6
    gen_ai.system: anthropic
    gen_ai.request.model: aws/claude-sonnet-4-6
    input_tokens: 1
    output_tokens: 384
    cache_read_tokens: 123249
    cache_creation_tokens: 277
    ttft_ms: 5673
  ├─ claude_code.tool (1ms)
    tool_name: TodoWrite
    ├─ claude_code.tool.execution (0ms)
      success: true
    ├─ claude_code.tool.blocked_on_user (1ms)
      decision: unknown
```

---

## Metrics

### List all metrics

```bash
otelite metrics list
```

```text
┌─────────────────────────────────────┬───────────┬─────────────────────────┬─────────────────────┐
│ Name                                ┆ Type      ┆ Value                   ┆ Timestamp           │
╞═════════════════════════════════════╪═══════════╪═════════════════════════╪═════════════════════╡
│ claude_code.active_time.total       ┆ counter   ┆ 55.00                   ┆ 2026-04-22 14:50:24 │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ claude_code.token.usage             ┆ counter   ┆ 1.00                    ┆ 2026-04-22 15:09:29 │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ http.request.duration               ┆ histogram ┆ count=150, sum=18750.00 ┆ 2026-04-20 22:15:34 │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ http.requests.total                 ┆ counter   ┆ 1234.00                 ┆ 2026-04-20 22:15:34 │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ memory.usage                        ┆ gauge     ┆ 52428800.00             ┆ 2026-04-20 22:15:34 │
└─────────────────────────────────────┴───────────┴─────────────────────────┴─────────────────────┘
```

---

## Usage (GenAI/LLM analytics)

```bash
otelite usage --since 24h
```

Shows token consumption, cost, and analytics across all GenAI/LLM spans received in the time
window. Without flags, you get a token + cost summary. Flags below add per-model breakdowns
and analytics panels.

### Common flags

```text
--since <duration>    Time range: 1h, 24h, 7d, 30d  [default: 24h]
--start <ts>          Exact start instead of a rolling window: YYYY-MM-DD
                      (UTC midnight), YYYY-MM-DDTHH:MM:SS, or epoch
                      seconds/nanoseconds
--end <ts>            Exact end; same formats as --start, defaults to now
                      (requires --start)
--model <name>        Filter by model; repeatable and ORed. A value
                      without `*` matches exactly, with `*` it is a glob
                      (claude-opus-*). Applies to every panel
--system <name>       Filter to one provider
--by-model            Break down by model name
--by-system           Break down by provider (openai, anthropic, …)
--by-session          Break down by session.id
--top <N>             Add a panel of the N most expensive individual
                      LLM calls
--format <fmt>        Output format: pretty | json  [default: pretty]
```

### Analytics panels

Each flag below adds one extra panel to the output. Stack them freely.

```text
# latency & throughput
--latency                 p50/p95/p99 latency, TTFT p50/p95, derived tok/s,
                          ctx size, out/in ratio per model
--latency-series          Latency trend over time: min/avg/p95/max per time
                          bucket, grouped by model
--bucket-secs             Bucket size in seconds for the time-series views
                          (default 3600 = 1 hour)
--calls                   Call volume trend over time: requests per time
                          bucket, grouped by model
--latency-context         Latency broken down by input-token context-size bin
                          (and model)
--latency-percentiles     p50/p90/p95/p99 latency over time, for duration and
                          TTFT; --model narrows the cohort
--calendar-day            Bucket --latency-percentiles by calendar day in
                          --timezone (DST-aware 23/25-hour days, empty days
                          shown with no percentiles) instead of the fixed
                          --bucket-secs grid
--timezone                IANA timezone for --calendar-day (e.g.
                          Europe/London); default UTC
--throughput              Tok/s p10/p50/p90 + eligible-call count columns in
                          --latency-series; --latency and
                          --latency-percentiles show them by default
--cross-tool-ttft         Cross-tool first-token latency comparison (Claude
                          Code, opencode, pi), from spans
--human-latency           Human response latency per tool: the gap between
                          turns — reading/thinking time — with each tool's
                          busiest hours

# cost & tokens
--truncation              Truncation rate per model (responses ending with
                          finish_reason=length)
--cache-rate              Prompt-cache hit rate per model — spot
                          prompt-caching wins
--request-params          Distribution of temperature, max_tokens per model
--reasoning-share         Reasoning (thinking) token share per model —
                          opencode + Codex thinking tokens as % of output
--effort-breakdown        Codex reasoning-effort × token-type breakdown:
                          effort level vs cost per model
--efficiency              Cross-agent efficiency: tokens/commit and
                          tokens/LOC for Claude Code + opencode
--loc-efficiency          Lines-of-code efficiency per tool and model
                          ($ per 100 added lines)
--cost-projection         Project this month's cost at the trailing 7-day
                          rate, by model
--cost-by-project         LLM cost by project × tool × model
--session-depth-cost      Session depth vs cost: median/p95 cost by
                          turn-count bucket
--hour-of-day             Hour-of-day activity distribution (0–23 UTC, LLM +
                          tool calls)

# sessions & context
--conv-depth              Conversation depth distribution (turns per session)
--context-composition     Per-session context composition: the fixed prefix
                          (minimum cache-read replayed every call — system
                          prompt, tools, skills) vs peak context, and the
                          growth between them
--context-split           Token usage grouped by request context
                          (llm_request.context: interaction / sub_agent / …)
--session-models          Session × model cross-tab: which sessions used
                          which models and at what cost
--session-duration        Session-duration distribution per tool: length
                          trends + outliers
--session-chains          Session chains: resumed sessions rolled up into
                          work threads
--session-quality         Session quality summary — clean / degraded /
                          errored counts
--rare-tools              Rare-tool sessions (pi, deepseek, experimental
                          harnesses — main tools and busy tools excluded)
--time-in-tool            Active engagement time per tool per day: where the
                          AI time went
--daily-tool-mix          Daily activity mix per tool (Claude Code / opencode
                          / Codex datapoints) plus LLM token volume and
                          estimated cost per day
--productivity            Git output per tool per day: commits, PRs, lines
                          added/removed
--model-selection-heatmap Model-selection heatmap: (role × tool × model)
                          request counts

# tools & agents
--tools                   Per-tool call counts, success rate, errors, avg
                          duration
--tool-approvals          Tool approval/rejection decision summary (Claude
                          Code)
--tool-errors [N]         Top N tool errors from failed tool executions
                          (Claude Code); N defaults to 20
--tool-failures           opencode tool failure rates: which tools fail most
                          and at what percentage
--tool-switch-overhead    Tool-switch overhead: gaps and cold TTFT at tool
                          boundaries
--stop-reasons            Claude Code stop_reason distribution (tool_use /
                          end_turn / …)
--agent-roles             Cost and token attribution per sub-agent role
                          (opencode agent label)
--multi-agent             Codex multi-agent spawn/resume topology by
                          sub-agent role
--skill-activity          Codex skill injection counts — which skills fire
                          implicitly and how often
--skill-outcomes          Token efficiency comparison: sessions with vs
                          without each skill
--speed                   Speed/effort attribute distribution across Claude
                          Code LLM spans

# errors & health
--error-types             Error bucketing: rate_limit / timeout /
                          context_length / content_filter / auth /
                          server_error / unknown
--model-drift             Request → response model pairs (detect silent
                          provider rerouting)
--recent-errors           The most recent error events, from spans and logs
                          (newest first)
--recent-errors-limit <N> Maximum number of rows for --recent-errors
                          (default 50)

# codex
--codex-subagents         Codex sub-agent volume: thread starts per main
                          thread, spawn-role breakdown, and a daily rollup.
                          Volume only — Codex spans carry no usage attributes,
                          so no cost figures
--codex-ttft              Codex first-token latency percentiles (p50/p90/p95)
                          per model
--codex-turns             Codex turn busy/idle breakdown per model and
                          project
--codex-idle-ratio        Codex idle-ratio trend per day: model wait vs tool
                          execution
--guardian                Codex Guardian review summary: risk levels,
                          actions, approval rate
--hook-overhead           Codex hook overhead: total and average invocation
                          time per hook event type
--bob-hook-overhead       Bob hook overhead (empty until Bob emits hook
                          telemetry)
```

### Examples

```bash
# Token + cost summary, broken down per model
otelite usage --since 24h --by-model

# Full latency picture for the last 7 days, including TTFT
otelite usage --since 7d --latency

# Why are calls failing?
otelite usage --since 1h --error-types

# Did the provider serve a different model than I asked for?
otelite usage --model-drift

# Cost per session, plus the 10 most expensive individual calls (JSON)
otelite usage --by-session --top 10 --format json | jq

# How much of each session's context is the fixed prefix vs what grew?
otelite usage --since 7d --context-composition

# Which Codex sessions spawn sub-agents, and what are they used for?
otelite usage --since 7d --codex-subagents

# Exact calendar interval instead of a rolling window
otelite usage --start 2026-09-10 --end 2026-09-17 --by-model

# Where is this month's spend heading?
otelite usage --cost-projection
```

### Capability coverage: `otelite capabilities`

```bash
otelite capabilities --since 7d
```

Shows which GenAI telemetry capabilities each emitter identity (provider/model +
verified signature) actually provides, per metric: availability (`available` |
`sparse` | `absent`), quality (`reliable` | `invalid` | `degenerate` |
`not_assessed`) and derivation (`native` | `correlated` | `unavailable`).
`absent` means the metric is not provided — it is never a measured zero.

The report is computed from the most recent bounded span sample (newest first),
so it is always fast; `truncated` is shown when older spans were excluded.

The **Correlation** column reports, per group, how usage metrics that live on
separate spans were joined back to request spans. Codex reports request timing
(`run_sampling_request`) and token usage (`handle_responses`) on different
spans, so otelite joins them with a strict one-to-one rule
(`codex-one-to-one-v1`): same trace, usage span structurally inside the
request span, request not errored, model compatible, and exactly one usage
candidate. The column shows `matched/unmatched/rejected/ambiguous` candidate
counts; retries, concurrent sampling, reused identifiers and turn-level
counters land in `ambiguous` or `rejected` and are never guessed into a
value. Emitters whose usage rides on the request span itself (OpenAI,
Anthropic, Claude Code, OpenCode) carry no join rule and show `—`. Only
counts and the rule name are exposed — never span or trace identifiers.

```bash
# Which emitters actually provide TTFT in the last month?
otelite capabilities --since 30d

# Machine-readable report (identical to the API response)
otelite capabilities --since 7d --format json | jq
```

### Model performance diagnosis

Compare a model's duration, throughput, TTFT and error rate in an exact
interval against the preceding interval and an optional rolling baseline.
The intervals are required; `--start`/`--end` accept dates (UTC midnight)
or RFC 3339 timestamps, and `--rolling` a duration (`24h`, `7d`).

```bash
# Did gpt-4o regress this week versus the week before, with a 7-day rolling baseline?
otelite model-performance --start 2026-08-18 --end 2026-08-25 --rolling 7d

# One provider, one model, JSON (deep-equal to GET /api/genai/model-performance)
otelite model-performance --start 2026-08-18 --end 2026-08-25 --rolling 7d \
    --provider openai --model gpt-4o --format json-compact

# Align calendar-day reasoning to a timezone (intervals stay exact UTC ns)
otelite model-performance --start 2026-08-18 --end 2026-08-25 --timezone Europe/London
```

The output reports, per (provider, model, emitter fingerprint) identity:
the exact current/preceding/rolling intervals, per-median and per-tail
(p95) baselines and deltas, eligible sample counts, a deterministic class
(`typical_regression`, `tail_regression`, `workload_shift_correlated`,
`error_associated`, `mixed_evidence`, `insufficient_telemetry`,
`no_material_change`) and a confidence level. Workload and error
co-movement is always labelled *correlation, not causation*; TTFT
conclusions are suppressed when TTFT is not reliable; a zero baseline keeps
the relative change percentage-unavailable rather than printing a fake 0%.
See the LLM observability guide for the thresholds and what the diagnosis
will not claim.

---

## Providers

```bash
otelite providers --since 24h
otelite providers --since 7d --format json
```

The provider × model mix: per provider and model, tokens (input, output,
cache read/write, reasoning), session count, estimated cost and share of
total tokens. Cost is priced from the LiteLLM model table (falls back to
built-in Claude rates); models without a known price show `—`. Like
`usage`, reads the local database directly — no daemon required.

---

## Cache

```bash
otelite cache --since 24h
otelite cache --since 7d --series --bucket-secs 86400
```

Prompt-cache economics per model: tokens served from cache vs tokens
written, the read:write ratio (how long the cache pays for itself), hit
rate, and estimated savings (cache reads priced at the input − cache-read
delta). `--series` adds a time-bucketed read/write series; `--bucket-secs`
sets the bucket size (default 3600). Models without a known cache-read
price show `—`, and the total is flagged "partial". Reads the local
database directly.

---

## Reasoning

```bash
otelite reasoning --since 7d
```

How much of each model's output was thinking tokens, with the thinking
cost priced at the model's output rate. Also prints a reasoning-effort
breakdown (calls and tokens per effort level) when the spans carry it.
Models without thinking tokens show a 0% share — an empty window prints
"No token activity". Reads the local database directly.

---

## Agents

```bash
otelite agents --since 24h
```

Per-harness rollup over the window: sessions, cost, tokens, tool calls
and retries, sorted by cost. A cost marked `(actual)` comes from the
harness's own cost counter rather than token × price. Reads the local
database directly.

---

## Projects

```bash
otelite projects --since 24h
```

Per-project usage: sessions, cost, tokens and the dominant model,
grouped by `project.id`. opencode labels its spans with the project;
Codex and Claude Code carry no project label today, so their usage
lands under `unattributed`. Reads the local database directly.

---

## Sessions

```bash
otelite sessions costs --since 24h --top 20
otelite sessions cost-hist --since 7d --buckets 30
```

`costs` lists the top-cost sessions (default 50, `--top` to change) with
agent, cost, tokens, wall-clock duration, and an anomaly flag (`!`) when
a session costs more than 3× the median. `cost-hist` is a log-spaced
ASCII histogram of per-session costs.

```bash
otelite sessions context <session-id>
```

Everything observed for one session: spans, logs and metric aggregates
on one timeline, plus a merged event timeline. `--start`/`--end` (epoch
nanoseconds) bound the window and `--limit` caps rows (default 500, cap
5000). Session IDs come from `sessions costs` or the web Sessions tab.
Reads the local database directly.

---

## Histogram

```bash
otelite histogram session_cost --since 7d --scale log
otelite histogram ttft --since 24h --buckets 30
```

Distribution of a named metric cohort as an ASCII histogram with summary
stats (n, min, p50, p95, p99, max, mean). Cohorts: `session_cost` (USD),
`tool_duration`, `llm_duration`, `ttft` (ms) and `output_tokens`.
`--buckets` sets the bucket count (default 20, cap 100); `--scale log`
switches to log-spaced bins, which suits the heavy tails of cost and
latency. Reads the local database directly.

---

## LLM requests

```bash
otelite llm
otelite llm --model claude-sonnet --status error
otelite llm --trace <trace-id>
```

Recent individual LLM requests: time, model, in / cached / out tokens,
duration, cost, status and a short trace id. `--status ok` keeps
`end_turn`/`stop` finish reasons, `error` keeps the rest (filtered
client-side); `--model` is a substring match, `--session` filters by
session ID, `--since` defaults to 1h and `--limit` to 30. `--trace`
prints every span in that trace with its `gen_ai.*` attributes. Unlike
most analytics commands, `llm` queries the running daemon via
`--endpoint`.

---

## Retries

```bash
otelite retries
otelite retries --model claude-sonnet --limit 50
```

Recent retried LLM calls: requests that failed on the first attempt and
then succeeded. Each row shows time, model, attempt number, TTFT, stop
reason and short session/trace ids for drill-down. `--since` defaults to
24h and `--limit` to 20. Queries the running daemon via `--endpoint`.

---

## Importing from files

`otelite import` loads telemetry from a newline-delimited JSON file (JSONL) without a running
receiver. Each line must be a complete OTLP JSON export request — the format produced by standard
OTLP file exporters.

### Import into the default database

```bash
otelite import telemetry.jsonl
```

### Import into an isolated database

Useful for CI artifacts or any data you want to keep separate from your live session:

```bash
otelite import telemetry.jsonl --storage-path ./ci-run-42
otelite serve --storage-path ./ci-run-42    # browse the imported data
```

### Force signal type (skip auto-detection)

```bash
otelite import metrics.jsonl --signal-type metrics
otelite import spans.jsonl   --signal-type traces
otelite import app.jsonl     --signal-type logs
```

### Read from stdin

```bash
cat metrics.jsonl | otelite import -
```

Signal type is auto-detected from the top-level key of the first non-empty line
(`resourceMetrics`, `resourceLogs`, or `resourceSpans`). A summary is printed to stderr on
completion:

```text
Import complete: 1247 records imported (0 errors, 3 empty lines skipped)
```

> **Note on historical data**: the metrics web UI time range selector offers presets up to 24h.
> Data older than that is still stored and queryable via `otelite metrics list`, but will not
> appear in the dashboard graphs without selecting a wider time range.

---

## MCP server (AI agent access)

`otelite mcp` speaks the [Model Context Protocol](https://modelcontextprotocol.io) over
stdin/stdout (newline-delimited JSON-RPC 2.0), so AI agents — Claude Code, Cursor, and any
other MCP client — can query your telemetry directly, with no dashboard in between.

### Connect Claude Code

```bash
claude mcp add otelite -- otelite mcp
```

Or in the MCP client config file:

```json
{
  "mcpServers": {
    "otelite": { "command": "otelite", "args": ["mcp"] }
  }
}
```

The server reads the same database as the other read-only commands
(`OTELITE_DATA_DIR` or `~/.otelite/data`), so it works without a running daemon.

### Tools

| Tool | Arguments | Returns |
|------|-----------|---------|
| `query_logs` | `severity?` (min level), `search?`, `since?` (e.g. `24h`), `limit?` (≤500) | Newest-first matching logs with RFC 3339 `time`, severity, body, attributes |
| `query_traces` | `status?` (ERROR/OK), `min_duration?` (ms), `service?`, `limit?` (≤100) | Newest-first trace summaries: root span, duration, span count, services, status |
| `get_trace` | `trace_id` (required) | The full trace: every span with attributes, events, status, resource |
| `get_usage` | `since?` (default `24h`), `model?` (exact or `*` glob) | GenAI token totals plus per-model and per-provider breakdowns |

Tool failures (unknown severity, missing trace, ...) come back as an MCP tool result with
`isError: true` and an actionable message, so the agent can correct itself.

### Try it without an agent

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | otelite mcp
echo '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_usage","arguments":{"since":"1h"}}}' | otelite mcp
```

---

## Diagnose

`otelite diagnose` fetches all traces for a session and prints a per-interaction forensic
report: token counts, latency, errors, streaming stalls, and a copy-paste escalation block.

### Basic usage

```bash
otelite diagnose <session-id>
```

```text
Session: 3f7a2c1e-8b4d-4f12-a9e1-d2c3b4a5f6e7
Model:   claude-opus-4-5   Interactions: 12   2026-05-21 14:30–15:02
Errors:  1   Stalls: 1

   #      Time   Input tok   Cached    TTFT  Duration   Out tok  Status          Trace
------------------------------------------------------------------------------------
   1  14:30:01       42.3K     38.1K    1.2s     8.3s     1.2K  OK              3f7a2c1e8b4d
   2  14:33:47       85.7K     79.2K    1.5s    12.1s     2.8K  OK              4a1b2c3d4e5f
   3  14:41:22      129.4K    123.8K    1.1s    10.8s     1.5K  OK              5b6c7d8e9f0a
   4  14:52:08      174.2K    168.9K    3.2s  4m38s       0.8K  ERROR [stall]   6c7d8e9f0a1b
   ...

Context growth: 42K → 312K tokens across 12 interactions (peak: 312K)

⚠  1 streaming stall(s) detected.
   Interaction #4: 278000ms duration, ~174K tokens

Escalation info
  Session:   3f7a2c1e-8b4d-4f12-a9e1-d2c3b4a5f6e7
  Model:     claude-opus-4-5
  Timestamps: 2026-05-21T14:52:08Z
  Response IDs: msg_01XYZ...
  Trace IDs:  6c7d8e9f0a1b2c3d
  Peak input: 312K tokens
```

### Stall remediation suggestions

```bash
otelite diagnose <session-id> --suggest
```

Adds a recommended proxy/load-balancer stream-idle timeout based on the longest observed stall.

### Connect to a non-default server

```bash
otelite --endpoint http://localhost:3000 diagnose <session-id>
```

---

## Common patterns

```bash
# Tail logs in real-time (re-run every second via watch)
watch -n1 otelite --no-color --no-header --no-pager logs list --limit 20

# Export all ERROR logs to a file
otelite --format json logs list --severity ERROR > errors.json

# Find the slowest traces in the last hour
otelite traces list --min-duration 5s --since 1h

# Get a trace as JSON and extract span names
otelite --format json traces show <trace-id> | jq '.spans[].name'

# Count log entries by severity (requires jq)
otelite --format json logs list --limit 1000 | jq 'group_by(.severity) | map({severity: .[0].severity, count: length})'
```
