# LLM Observability with Otelite

Otelite receives OpenTelemetry (OTLP) signals — logs, traces, and metrics — from your AI
applications and stores them locally in SQLite. It understands the
[GenAI semantic conventions](https://opentelemetry.io/docs/specs/semconv/gen-ai/) (`gen_ai.*`
attributes), so token usage, latency, caching, and errors surface without any post-processing.

**What you get:** per-call token accounting, prompt-cache hit rates, TTFT, finish-reason
distribution, streaming stall detection, and session-level forensic reports — all queryable from
the terminal or a browser dashboard at `http://localhost:3000`.

---

## Starting the server

```bash
otelite serve
```

Listens on:

```text
OTLP gRPC  localhost:4317
OTLP HTTP  localhost:4318
Dashboard  http://localhost:3000
```

---

## Sending telemetry from your application

### Claude Code

Claude Code emits OTLP natively. Point it at otelite before launching:

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
claude  # or any Claude Code invocation
```

### Anthropic Python SDK

```bash
pip install opentelemetry-instrumentation-anthropic
```

```python
from opentelemetry import trace
from opentelemetry.exporter.otlp.proto.http.trace_exporter import OTLPSpanExporter
from opentelemetry.instrumentation.anthropic import AnthropicInstrumentor
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import BatchSpanProcessor

provider = TracerProvider()
provider.add_span_processor(
    BatchSpanProcessor(OTLPSpanExporter(endpoint="http://localhost:4318/v1/traces"))
)
trace.set_tracer_provider(provider)

AnthropicInstrumentor().instrument()

import anthropic
client = anthropic.Anthropic()
# All subsequent calls are now traced
```

### OpenAI SDK

```bash
pip install opentelemetry-instrumentation-openai
```

```python
from opentelemetry.instrumentation.openai import OpenAIInstrumentor

# Same TracerProvider setup as above, then:
OpenAIInstrumentor().instrument()

import openai
client = openai.OpenAI()
```

### LiteLLM proxy

In your LiteLLM config:

```yaml
litellm_settings:
  success_callback: ["otel"]
```

```bash
export OTEL_ENDPOINT_HTTP_URL=http://localhost:4318/v1/traces
litellm --config config.yaml
```

---

## What gets captured

Each LLM call span carries these attributes when the instrumentation library follows the GenAI
conventions:

```text
gen_ai.system                          Provider: anthropic, openai, aws_bedrock, …
gen_ai.request.model                   Model you asked for
gen_ai.response.model                  Model that actually served the request (may differ)
gen_ai.usage.input_tokens              Prompt tokens billed
gen_ai.usage.output_tokens             Completion tokens billed
gen_ai.usage.cache_read.input_tokens   Tokens served from prompt cache
gen_ai.usage.cache_creation.input_tokens  Tokens written into the prompt cache
gen_ai.server.time_to_first_token      Milliseconds to first streamed token (TTFT)
gen_ai.response.finish_reasons         stop | length | tool_calls | content_filter
session.id                             Logical session / conversation identifier
gen_ai.conversation.id                 Turn-level grouping within a session
```

Logs linked to a trace (request/response bodies, tool results) carry the same `trace_id` and
`span_id` so you can correlate them later.

---

## Token usage and cost: `otelite usage`

`otelite usage` aggregates GenAI spans and renders analytics panels. Without flags it shows a
cost + token summary for the last 24 hours.

### Basic summary

```bash
otelite usage --since 24h --by-model
```

### Latency breakdown

```bash
otelite usage --since 7d --latency
```

Adds p50/p95/p99 total span latency and p50/p95 TTFT per model.

For what the derived `tok/s` figures actually measure (and what they do not),
the percentile estimator, and the rolling vs calendar-day bucketing rules,
see [throughput-analysis.md](throughput-analysis.md).

### Prompt-cache efficiency

```bash
otelite usage --cache-rate
```

Shows the ratio of `cache_read_input_tokens` to total input tokens per model. A low cache rate on
a heavily-repeated system prompt indicates the prompt prefix is mutating between calls (common
when timestamps or UUIDs are injected early in the context).

### Context-length exhaustion

```bash
otelite usage --truncation
```

Shows the fraction of responses that ended with `finish_reason=length` per model. Any model above
~2% warrants investigation — either the context is too large or `max_tokens` is set too low.

### Error categorisation

```bash
otelite usage --since 1h --error-types
```

Buckets failures into: `rate_limit`, `timeout`, `context_length`, `content_filter`, `auth`,
`server_error`, `unknown`. Use this before filing a support ticket to distinguish provider-side
errors from client-side configuration problems.

### Silent model rerouting

```bash
otelite usage --model-drift
```

Lists `request.model` → `response.model` pairs. A mismatch means the provider silently served a
different model — common in Bedrock cross-region inference and OpenAI model aliases.

### Latency by context size

```bash
otelite usage --latency-context
```

Shows p95 latency bucketed by input-token count:

```text
0–1K     →  p95  2.1s
1K–10K   →  p95  4.8s
10K–50K  →  p95 14.2s
50K–100K →  p95 38.7s
100K+    →  p95 91.3s
```

Use this to identify where your application crosses a latency cliff, and whether batching or
context trimming would have a measurable effect.

---

## Model performance diagnosis: `otelite model-performance`

`otelite usage` shows what is happening. `otelite model-performance` answers
*whether a model's behaviour changed* in an exact interval — and says what
evidence it cannot speak to.

```bash
# Did gpt-4o get slower this week than the week before?
otelite model-performance --start 2026-08-18 --end 2026-08-25 --rolling 7d

# One model, machine-readable (deep-equal to the API response)
otelite model-performance --start 2026-08-18 --end 2026-08-25 --rolling 7d \
    --model gpt-4o --format json-compact
```

### What it compares

For every **identity** — the triple *(provider, requested model, emitter
fingerprint)* — it compares the selected interval (the **current** window)
against two baselines:

- **Preceding** — the equal-length interval immediately before the current
  one. Derived; you cannot select it independently.
- **Rolling** — an optional baseline you size (`--rolling 7d`); it sits
  entirely before the preceding window (the three windows never overlap).
  When a baseline has no eligible samples,
  the other is used; when neither does, no comparison is made and that is
  reported, never implied away.

The same diagnosis is served by `GET /api/genai/model-performance`, the TUI
Usage view and the web analytics page — all four render the same frozen
object, and the CLI's `json-compact` output deep-equals the API response.

### The canonical request population

Only GenAI spans that carry request-timing semantics are counted: either a
known emitter signature (Claude Code, Codex, OpenCode) or standard OTel
GenAI attributes on the span itself (`gen_ai.system` + model + token
metrics). Delivery duplicates are deduplicated (only the first delivery of
each span counts), and the most-recent 100,000 spans are the sample bound;
when the bound is hit the diagnosis carries a `truncated` flag rather than
silently dropping history.

### The identity key

- **Provider** — `gen_ai.system` (e.g. `openai`, `anthropic`).
- **Requested model** — `gen_ai.request.model`. The *served* model
  (`gen_ai.response.model`) is reported separately: a provider that reroutes
  you to another model is a finding, not a silent merge.
- **Emitter fingerprint** — a hash over the emitter's rule, service name and
  instrumentation scope. Two different instrumentation stacks calling the
  same provider and model are separate identities, because their latency
  measurements are not interchangeable.

### Percentile direction

- **Duration** and **throughput** are assessed on the **median (p50)** —
  the typical call — with the **p95 tail** as a second check. A change that
  only shows up in the tail is classified as a *tail regression*, not a
  typical one.
- **Throughput** is the per-call rate (output tokens ÷ span duration, in
  tokens/s) — the derived throughput of issue #119. Span duration is **not**
  generation time: it includes queueing, transport and streaming gaps, so
  the diagnosis never makes decode-speed claims from it.
- **TTFT** is assessed only where issue #111's quality gate says TTFT is
  reliable (native, or structurally correlated). Where it is absent, sparse,
  invalid or degenerate (e.g. equal to total duration), TTFT conclusions are
  *prevented* — the diagnosis says so explicitly instead of attributing
  first-response or decode-rate behaviour to noise.
- **Error rate** is errors ÷ requests per identity, compared in percentage
  points.

### Materiality thresholds

A change is material when:

| Metric | Material when | Constant |
|---|---|---|
| Duration / throughput / TTFT | relative change ≥ 20% | `MODEL_PERFORMANCE_MATERIAL_RELATIVE_CHANGE` |
| Error rate | absolute change ≥ 5 percentage points | `MODEL_PERFORMANCE_MATERIAL_ERROR_RATE_POINTS` |
| Any metric | eligible current-window samples ≥ 10 | `MODEL_PERFORMANCE_MIN_ELIGIBLE_SAMPLES` |

Below 10 eligible samples the metric is `insufficient_telemetry` — a
first-class state with the count reported, not a suppressed number.
Confidence (`insufficient` / `low` / `medium` / `high`) is derived from the
eligible sample counts and travels with every assessment.

### What the classes mean

| Class | Meaning |
|---|---|
| `no_material_change` | No material worsening (a material *improvement* is reported with a note, without regression wording) |
| `typical_regression` | The median worsened materially |
| `tail_regression` | The median held; the p95 tail worsened materially |
| `workload_shift_correlated` | Duration/throughput worsened **and** workload (tokens) moved materially — co-movement is correlation, not causation |
| `error_associated` | Worsening accompanied by a rising error rate — correlation, not causation |
| `mixed_evidence` | The preceding and rolling baselines disagree on materiality — both deltas are reported, neither is silently dropped |
| `insufficient_telemetry` | Below the sample minimum; the count is reported |

### What it will not tell you

- **It is not a benchmark.** It measures your recorded traffic, not a
  controlled experiment; it cannot say whether a change is "real" at the
  provider — only what your spans show and how much evidence stands behind it.
- **No decode-speed claims.** Span duration is end-to-end wall time; only
  TTFT (when #111-trusted) and the derived per-call throughput speak to
  generation behaviour.
- **No causal language.** Workload and error co-movement is always labelled
  correlation.

---

## Session-level investigation

### Find slow or failing sessions

```bash
# All traces for a session
otelite traces list --session 19b6add0-e3af-422e-9863-00cc100f0d74

# Only the ones that errored
otelite traces list --session 19b6add0-e3af-422e-9863-00cc100f0d74 --status ERROR

# Anything that took longer than 60 seconds
otelite traces list --session 19b6add0-e3af-422e-9863-00cc100f0d74 --min-duration 60s
```

### One-shot forensic report: `otelite diagnose` (new)

`otelite diagnose` collects everything relevant for a session into a single structured report:
total tokens, per-model breakdown, latency distribution, finish reasons, errors, and any detected
streaming stalls. Use it before opening a support ticket or handing off a bug report.

```bash
otelite diagnose 19b6add0-e3af-422e-9863-00cc100f0d74
```

The report includes model, timestamps, request body lengths, and any provider-assigned request
IDs extracted from response headers — exactly what a model provider's support team will ask for.

---

## Trace inspection

### Span tree

```bash
otelite traces show 382240cb628c341584d6ad2c1955929d
```

```text
Trace ID: 382240cb628c341584d6ad2c1955929d
Duration: 187016ms
Status:   ERROR

Spans:
claude_code.interaction (187016ms)
  session.id: 19b6add0-e3af-422e-9863-00cc100f0d74
  ├─ claude_code.llm_request (31321ms) [Anthropic] aws/claude-sonnet-4-6
      gen_ai.request.model: aws/claude-sonnet-4-6
      gen_ai.usage.input_tokens: 45231
      gen_ai.usage.output_tokens: 384
      gen_ai.usage.cache_read.input_tokens: 123249
      ttft_ms: 5673
  ├─ claude_code.llm_request (152840ms) [streaming stall] aws/claude-sonnet-4-6
      gen_ai.server.time_to_first_token: 4821ms
      gen_ai.response.finish_reasons: error
```

The `[streaming stall]` annotation means TTFT was present (the connection opened and tokens
started flowing) but the span ended in an error after an unusually long wall-clock time. See
[Streaming stalls](#streaming-stalls) below.

### Logs linked to a trace

```bash
otelite traces logs 382240cb628c341584d6ad2c1955929d
```

Lists every log record that shares the same trace ID — request bodies, response bodies, tool
results. Use this to see the exact prompt and completion for a flagged call without hunting
through `otelite logs list`.

---

## Streaming stalls

**Pattern:** `gen_ai.server.time_to_first_token` is set (tokens started flowing) but the span
duration is >> TTFT and `finish_reason` is `error` — typically after 300s or 500s exactly.

**Root cause:** An intermediary (LiteLLM, API gateway, load balancer) has an idle-stream timeout
that fires while the model is still generating. The connection drops, the span ends in error.
The model itself was working; the problem is in the network path.

**Common threshold:** 300s is the default LiteLLM → Bedrock stream idle timeout. This is hit
regularly on large-context requests with slow generation.

**Remediation:**

1. Identify the proxy/load balancer in the call path.
2. Raise its stream-idle timeout to ≥500s (Bedrock models can take 400s+ on 100K-token contexts).
3. For LiteLLM: set `stream_timeout: 500` in the model config.
4. Re-run the failing session and confirm `finish_reason` flips to `stop`.

To find all stalled calls in a time window:

```bash
otelite usage --since 24h --error-types   # look for timeout bucket
otelite traces list --min-duration 290s --status ERROR
```

---

## Large request bodies

Log records carrying request or response bodies can exceed 100 KB. The default table output
truncates them. Use `--full` to dump the raw body to stdout and pipe it to a tool:

```bash
otelite logs show 1776870650059000000 --full | jq .
```

Extract just the system prompt from a request body:

```bash
otelite logs show 1776870650059000000 --full | jq '.messages[] | select(.role=="system") | .content'
```

Count tokens in the raw body without sending it anywhere:

```bash
otelite logs show 1776870650059000000 --full | wc -c
```

---

## Escalation: gathering support artifacts

When filing a ticket with a model provider, you need: model name, timestamps, body size, and any
provider-assigned request IDs. `otelite diagnose` pre-fills all of this:

```bash
otelite diagnose 19b6add0-e3af-422e-9863-00cc100f0d74 --format json > session-report.json
```

If you need the raw request body for a specific failing call:

```bash
# Find the trace ID from the diagnose report, then get its logs
otelite traces logs <trace-id> --severity ERROR --format json | jq '.[0].body'

# Or get the full body of a specific log record
otelite logs show <log-id> --full
```

Combine with `--model-drift` to rule out silent rerouting before assuming the model you asked
for is at fault:

```bash
otelite usage --since 1h --model-drift --format json | jq '.[] | select(.request_model != .response_model)'
```

---

## Web dashboard

`http://localhost:3000` — the Usage page has four tabs:

- **Overview** — token spend, cost, call volume over time
- **Performance** — latency distribution, TTFT, throughput
- **Quality** — finish reasons, error rates, truncation rate, cache hit rate
- **Details** — filterable table of individual LLM calls with all GenAI attributes inline
