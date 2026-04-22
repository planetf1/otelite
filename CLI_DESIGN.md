# Otelite CLI Design - Canonical Noun-Verb Structure

## Design Principles

1. **Consistent noun-verb ordering**: `otelite <noun> <verb> [flags]`
2. **Same verb means same thing**: `show` always means "get one by ID", `list` always means "get many"
3. **Standard flags available everywhere applicable**: `--since`, `--limit`, `--format`
4. **Signal-specific flags only where needed**: `--severity` for logs, `--status` for traces, etc.
5. **Help text follows same template**: Description, Usage, Options, Examples

## Canonical Structure

### Nouns (Signal Types)
- `logs` - Log entries
- `traces` - Distributed traces with spans
- `metrics` - Time-series metrics

### Verbs (Actions)
- `list` - List recent entries (paginated, filterable)
- `show` - Show single entry by ID
- `search` - Full-text search (logs only)
- `export` - Export data to file (JSON/CSV)

### Standard Flags (Available on all `list` commands)

```
--since <duration>    Time range (1h, 24h, 7d) [default: 1h]
--limit <n>           Maximum results [default: 50]
--format <fmt>        Output format: pretty, json [default: pretty]
--output <file>       Write to file instead of stdout
--no-color            Disable colors (global flag)
--no-header           Disable table headers (global flag)
```

### Signal-Specific Flags

**Logs:**
- `--severity <level>` - Filter by severity (ERROR, WARN, INFO, DEBUG, TRACE)

**Traces:**
- `--status <status>` - Filter by status (OK, ERROR)
- `--min-duration <duration>` - Filter by minimum duration (e.g., 1s, 500ms)
- `--service <name>` - Filter by service name

**Metrics:**
- `--name <pattern>` - Filter by metric name pattern
- `--label <key=value>` - Filter by label (can be specified multiple times)

## Command Reference

### Logs

```bash
# List recent logs
otelite logs list [--since <duration>] [--severity <level>] [--limit <n>] [--format <fmt>]

# Show single log entry
otelite logs show <id>

# Search logs by content
otelite logs search <query> [--limit <n>] [--format <fmt>]

# Export logs to file
otelite logs export [--since <duration>] [--severity <level>] [--output <file>] [--format json|csv]
```

### Traces

```bash
# List recent traces
otelite traces list [--since <duration>] [--status <status>] [--min-duration <duration>] [--limit <n>] [--format <fmt>]

# Show single trace with spans
otelite traces show <id>

# Export traces to file
otelite traces export [--since <duration>] [--status <status>] [--output <file>] [--format json|csv]
```

### Metrics

```bash
# List available metrics
otelite metrics list [--since <duration>] [--name <pattern>] [--label <key=value>] [--limit <n>] [--format <fmt>]

# Show metric values by name (CHANGED FROM 'get' TO 'show')
otelite metrics show <name> [--label <key=value>] [--format <fmt>]

# Export metrics to file
otelite metrics export [--since <duration>] [--name <pattern>] [--output <file>] [--format json|csv]
```

## Help Text Template

Every command follows this structure:

```
<Brief description>

Usage: otelite <noun> <verb> [OPTIONS] [ARGS]

Options:
  <Standard flags first, alphabetically>
  <Signal-specific flags next, alphabetically>

Examples:
  <2-3 practical examples showing common use cases>
```

## API Route Mapping

CLI commands map to API routes:

| CLI Command | API Route | Method |
|-------------|-----------|--------|
| `logs list` | `/api/logs` | GET |
| `logs show <id>` | `/api/logs/:id` | GET |
| `logs search <query>` | `/api/logs?search=<query>` | GET |
| `logs export` | `/api/logs/export` | GET |
| `traces list` | `/api/traces` | GET |
| `traces show <id>` | `/api/traces/:id` | GET |
| `traces export` | `/api/traces/export` | GET |
| `metrics list` | `/api/metrics` | GET |
| `metrics show <name>` | `/api/metrics?name=<name>` | GET |
| `metrics export` | `/api/metrics/export` | GET |

Query parameters match CLI flag names (e.g., `--since` → `?since=`, `--severity` → `?severity=`).

## Changes Required

### Immediate Changes (This Bead)

1. **Rename `metrics get` to `metrics show`**
   - Update `MetricsCommands` enum in `main.rs`
   - Update `handle_metrics_command` function
   - Update help text

2. **Add `--since` flag to metrics commands**
   - Add to `MetricsCommands::List`
   - Add to `MetricsCommands::Show` (for filtering time range)
   - Update `handle_list` and `handle_get` (rename to `handle_show`)

3. **Standardize help text**
   - Update clap attributes for consistent descriptions
   - Add examples to help text where missing

### Future Changes (Separate Beads)

1. **Add `export` subcommands** (otelite-fo0)
   - `logs export`
   - `traces export`
   - `metrics export`

2. **Add `--service` flag to traces** (when service tracking is implemented)

3. **Add `--output` flag** (for writing to files)

## Verification

After changes:

```bash
# Build and test
cargo build -p otelite-cli
cargo test -p otelite-cli

# Verify help consistency
otelite --help
otelite logs --help
otelite logs list --help
otelite traces --help
otelite traces list --help
otelite metrics --help
otelite metrics list --help
otelite metrics show --help  # Changed from 'get'

# Test commands work
otelite logs list --since 1h --limit 10
otelite traces list --status ERROR --min-duration 1s
otelite metrics list --name http --since 24h
otelite metrics show http_requests_total --label method=GET
```
