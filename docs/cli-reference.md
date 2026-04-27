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
otelite status          # Show daemon status
```

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

## Usage (GenAI/LLM token summary)

```bash
otelite usage --since 24h
```

Shows a summary of token consumption across all GenAI/LLM spans received in the time window.

Flags:

```text
--since <duration>    Time range: 1h, 24h, 7d, 30d  [default: 24h]
--by-model            Break down by model name
--by-system           Break down by provider (openai, anthropic, …)
--model <name>        Filter to one model
--system <name>       Filter to one provider
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
