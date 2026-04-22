# Otelite TUI Quickstart Guide

The Otelite Terminal User Interface (TUI) provides a keyboard-driven dashboard for viewing
OpenTelemetry data in real-time — no browser required.

## Starting the TUI

The TUI connects to a running `otelite serve` instance on `localhost:3000` by default:

```bash
otelite tui
```

Options:

```bash
otelite tui --api-url http://localhost:3000   # default
otelite tui --view traces                     # open directly on Traces tab
otelite tui --view metrics                    # open directly on Metrics tab
otelite tui --refresh-interval 5             # poll every 5 seconds (default: 2)
```

## Layout

Every view follows the same three-row structure:

```text
┌─ tab bar ──────────────────────────────────────────────────────────────┐
│ [l:Logs]   t:Traces   m:Metrics      ?:Help   q:Quit                  │
├─ main content ─────────────────────────────────────────────────────────┤
│                                                                         │
│   (table or split table/detail panel)                                  │
│                                                                         │
├─ status bar ───────────────────────────────────────────────────────────┤
│  LOGS  Connected | Logs: 42 | ↑↓/jk: Navigate  Enter: Detail ...      │
└────────────────────────────────────────────────────────────────────────┘
```

The active tab is shown with a **cyan highlight** and square brackets. Inactive tabs are white.

---

## Logs View (`l`)

```text
 [l:Logs]   t:Traces   m:Metrics      ?:Help   q:Quit

┌ Logs (42) ─────────────────────────────────────────────────────────────────┐
│ Timestamp        │ Severity  │ Message                                      │
│══════════════════╪═══════════╪══════════════════════════════════════════════│
│ 2026-04-22 15:11 │ INFO      │ claude_code.api_request_body                 │
│ 2026-04-22 15:11 │ INFO      │ claude_code.api_request                      │
│ 2026-04-22 15:10 │ INFO      │ claude_code.hook_execution_complete          │
│ 2026-04-22 15:10 │ INFO      │ claude_code.tool_result                      │
│ 2026-04-22 14:01 │ WARN      │ slow query detected: 450ms                   │
│ 2026-04-20 22:15 │ ERROR     │ Test ERROR log from inject_test_data.sh      │
│ 2026-04-20 22:15 │ DEBUG     │ http server initializing                     │
└────────────────────────────────────────────────────────────────────────────┘

 LOGS  Connected | Logs: 42 | ↑↓/jk: Navigate  Enter: Detail  f: Filter
```

- Severity is **color-coded**: ERROR=red, WARN=yellow, INFO=white, DEBUG=gray, TRACE=dark gray
- Message body is also tinted by severity for at-a-glance scanning
- Press **Enter** on any row to open a detail panel on the right showing full attributes and resource

**Filtering:**

```text
f         → opens filter prompt:  Filter: █  (Enter to apply, Esc to cancel)
            type  severity=ERROR   or  service=myapp   or just text
Esc       → clears active filter
```

**Log detail panel (Enter):**

```text
┌ Logs (42) ──────────────────┐┌ Log Detail ─────────────────┐
│ ...table rows...             ││ Timestamp: 2026-04-20 22:15 │
│ 2026-04-20 22:15  ERROR  Tes>││ Severity:  ERROR            │
│                              ││                             │
│                              ││ Message:                    │
│                              ││ Test ERROR log from         │
│                              ││ inject_test_data.sh         │
│                              ││                             │
│                              ││ Attributes:                 │
│                              ││   service.name: test-svc    │
│                              ││   host.name: localhost      │
└──────────────────────────────┘└─────────────────────────────┘
```

---

## Traces View (`t`)

```text
 l:Logs   [t:Traces]   m:Metrics      ?:Help   q:Quit

┌ Traces (3) ────────────────────────────────────────────────────────────────┐
│ Time     │ E │ Operation                    │ Duration  │ Spans │ Services │
│══════════╪═══╪══════════════════════════════╪═══════════╪═══════╪══════════│
│ 15:11:10 │   │ claude_code.llm_request      │ 5121ms    │ 1     │ claude   │
│ 15:10:46 │ ⚠ │ claude_code.tool.execution   │ 187016ms  │ 28    │ claude   │
│ 14:01:24 │   │ claude_code.llm_request      │ 138240ms  │ 43    │ claude   │
└────────────────────────────────────────────────────────────────────────────┘

 TRACES  Connected | Traces: 3 | ↑↓/jk: Navigate  Enter: Detail  f: Filter
```

- The **E** column shows `⚠` for traces with errors (row is highlighted red)
- Press **Enter** to load the trace and show a span waterfall on the right

**Span waterfall (Enter → trace detail):**

```text
┌ Traces (3) ──────────────────┐┌ Trace Detail ────────────────────────────┐
│ ...table...                  ││ Trace ID: 53ecf8c87568682bc4f4344bbacb   │
│ 15:10:46 ⚠ claude_code.tool>││ Operation: claude_code.interaction        │
│                              ││ Duration:  138240ms | Spans: 43           │
│                              ││                                           │
│                              ││ Span Waterfall:                           │
│                              ││                                           │
│                              ││   claude_code.interaction  ██████░░  138s │
│                              ││ ▶ └ claude_code.llm_request ░░█░░░░  31s  │
│                              ││     └ claude_code.tool      ░░░█░░░   1ms │
│                              ││       └ tool.execution      ░░░█░░░   0ms │
│                              ││       └ tool.blocked_on_us  ░░░█░░░   1ms │
│                              ││                                           │
│                              ││ Press Enter on a span to view details     │
└──────────────────────────────┘└───────────────────────────────────────────┘
```

- The **timing bar** (█/░) shows each span's position and duration within the overall trace
- Bar colour: green=normal, yellow=slow (>50% of trace), red=error
- **Selected span** is highlighted cyan with `▶` marker
- Navigate spans with `↑↓` or `j/k`; press **Enter** on a span for full attribute detail
- GenAI spans show an inline badge: `[claude-sonnet-4-6 · 1→384]` (input→output tokens)

---

## Metrics View (`m`)

```text
 l:Logs   t:Traces   [m:Metrics]      ?:Help   q:Quit

┌ Metrics (11) ─────────────────────────────────────────────────────────────┐
│ Name                              │ Type      │ Latest Value │ Description │
│═══════════════════════════════════╪═══════════╪══════════════╪═════════════│
│ claude_code.active_time.total     │ counter   │ 55.00        │             │
│ claude_code.token.usage           │ counter   │ 1.00         │             │
│ http.request.duration             │ histogram │ avg 125.0    │             │
│ http.requests.total               │ counter   │ 1234.00      │             │
│ memory.usage                      │ gauge     │ 52428800.00  │             │
└────────────────────────────────────────────────────────────────────────────┘

 METRICS  Connected | Metrics: 11 | ↑↓/jk: Navigate  Enter: Detail
```

- Metric type is colour-coded: counter=green, gauge=blue, histogram=magenta, summary=yellow
- Press **Enter** to see a sparkline trend chart and stats panel

**Metric detail with sparkline:**

```text
┌ Metric Info ─────────────────────────────────────────────────────────────┐
│ Name:  http.request.duration                                              │
│ Type:  histogram                                                          │
│ Unit:  ms                                                                 │
│ Value: count=150, sum=18750.00                                            │
│ Description: Duration of HTTP requests                                    │
└───────────────────────────────────────────────────────────────────────────┘
┌ Trend (last 20 points) ──────────────────────────────────────────────────┐
│  ▁▂▁▃▄▃▅▄▆▄▅▅▃▄▅▆▇▆▅▄                                                   │
└───────────────────────────────────────────────────────────────────────────┘
┌ Stats ────────────────────────────────────────────────────────────────────┐
│ Min: 45.00  Max: 312.00  Current: 125.00 ↑                               │
└───────────────────────────────────────────────────────────────────────────┘
```

---

## Keyboard Reference

| Key                    | Action                                     |
| ---------------------- | ------------------------------------------ |
| `l` / `t` / `m`        | Switch to Logs / Traces / Metrics view     |
| `Tab` / `Shift+Tab`    | Cycle to next / previous view              |
| `↑` / `↓` or `j` / `k` | Navigate items                             |
| `PgDn` / `PgUp`        | Page through list / scroll detail text     |
| `Enter`                | Open detail panel                          |
| `Esc`                  | Close detail / clear filter                |
| `f`                    | Open filter prompt (`key=value` or text)   |
| `s`                    | Toggle auto-scroll (Logs view)             |
| `r`                    | Force refresh                              |
| `?`                    | Help screen                                |
| `q` / `Ctrl+C`         | Quit                                       |

Filter syntax examples:

```text
severity=ERROR        → only ERROR-level logs
service=my-svc        → filter by service name
error=true            → only traces with errors (Traces view)
database timeout      → full-text search (any view)
```

---

## Troubleshooting

**"Cannot connect to API"** — make sure `otelite serve` is running:

```bash
otelite serve
```

**Display looks garbled** — minimum terminal size is 80×24. Resize the window and the TUI
redraws automatically. Ensure `$TERM` reports a colour terminal (`xterm-256color` or similar).

**`?:Help` / `q:Quit` are invisible** — your terminal may not support true 16-colour output.
Try `otelite tui --no-color` to fall back to plain styling.
