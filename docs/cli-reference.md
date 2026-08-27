# Otelite CLI Reference

The CLI queries a running `otelite serve` instance (default `localhost:3000`) and prints
results to the terminal. Output can be formatted as a table (`--format pretty`, default) or
JSON (`--format json`) for scripting.

## Global flags

These flags are available on every subcommand:

```text
--endpoint <URL>      Otelite API base URL [default: http://localhost:3000]
--format pretty|json  Output format [default: pretty]
--no-color            Disable colour output
--no-header           Omit table header row
--no-pager            Disable automatic paging of long output
--timeout <secs>      Request timeout [default: 30]
```

---

## Server management

```bash
otelite serve           # Start server in the foreground (Ctrl+C to stop)
otelite start           # Start as a background daemon
otelite stop            # Stop the running daemon
otelite restart         # Stop then start (picks up a freshly built binary)
otelite status          # Show daemon status and resolved listener addresses
```

`serve`, `start`, and `restart` accept the same listener arguments:

```text
--addr <ADDR>       Dashboard and REST API [default: 127.0.0.1:3000]
--grpc-addr <ADDR>  OTLP/gRPC receiver [env: OTELITE_GRPC_ADDR]
                    [default: 127.0.0.1:4317]
--http-addr <ADDR>  OTLP/HTTP receiver [env: OTELITE_HTTP_ADDR]
                    [default: 127.0.0.1:4318]
```

For each receiver, precedence is command-line argument, environment variable,
then loopback default. Pass an explicit `0.0.0.0:<PORT>` address for container
or LAN access. Startup output, `otelite status`, `/api/health`, and the web
status popover report the resolved receiver addresses.

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
--model <name>        Filter to one model
--system <name>       Filter to one provider
--by-model            Break down by model name
--by-system           Break down by provider (openai, anthropic, …)
--by-session          Break down by session.id
--top <N>             Limit to top-N rows (cost / latency)
--format <fmt>        Output format: table | json  [default: table]
```

### Analytics panels

Each flag below adds one extra panel to the output. Stack them freely.

```text
--latency             p50/p95/p99 latency, TTFT p50/p95, derived tok/s, ctx size,
                      out/in ratio per model
--truncation          Truncation rate per model (responses ending with finish_reason=length)
--cache-rate          Prompt-cache hit rate per model — spot prompt-caching wins
--request-params      Distribution of temperature, top_p, max_tokens per model
--conv-depth          Conversation depth distribution (turns per session)
--tools               Per-tool call counts, success rate, errors, avg duration
--error-types         Error bucketing: rate_limit / timeout / context_length /
                      content_filter / auth / server_error / unknown
--model-drift         Request → response model pairs (detect silent provider rerouting)
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

# Top 10 most expensive sessions, JSON for piping
otelite usage --by-session --top 10 --format json | jq
```

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
