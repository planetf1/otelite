# Rotel TUI Keyboard Shortcuts Reference

Complete reference for all keyboard shortcuts in the Rotel Terminal User Interface.

## Global Shortcuts

These shortcuts work in any view:

| Key | Action | Description |
|-----|--------|-------------|
| `q` | Quit | Exit the application |
| `Ctrl+C` | Force Quit | Immediately terminate the application |
| `l` | Logs View | Switch to the logs view |
| `t` | Traces View | Switch to the traces view |
| `m` | Metrics View | Switch to the metrics view |
| `?` | Help | Show the help screen with all shortcuts |
| `h` | Help | Alternative key for showing help |
| `Esc` | Back/Cancel | Close detail panels or return to previous view |
| `r` | Refresh | Manually refresh data from the API |

## Navigation Shortcuts

These shortcuts work within list views (logs, traces, metrics):

| Key | Action | Description |
|-----|--------|-------------|
| `↑` | Move Up | Select the previous item in the list |
| `↓` | Move Down | Select the next item in the list |
| `Enter` | Select/Expand | Show details for the selected item |
| `Esc` | Close Details | Return to the list view from detail panel |

## Search and Filter

| Key | Action | Description |
|-----|--------|-------------|
| `/` | Search | Start searching within the current view |
| `f` | Filter | Apply filters to the current view |
| `Esc` | Clear Search | Cancel search or clear search query |

## Logs View Specific

| Key | Action | Description |
|-----|--------|-------------|
| `s` | Toggle Auto-scroll | Enable/disable automatic scrolling to newest logs |
| `/` | Search Logs | Search in log messages, severity, and attributes |
| `f` | Filter by Severity | Filter logs by severity level (ERROR, WARN, INFO, DEBUG, TRACE) |
| `Enter` | View Log Details | Show full log entry with all attributes and resource info |

### Logs View Detail Panel

When viewing a log's details:

| Key | Action | Description |
|-----|--------|-------------|
| `Esc` | Close Details | Return to logs list |
| `↑/↓` | Scroll | Scroll through log details if content is long |

## Traces View Specific

| Key | Action | Description |
|-----|--------|-------------|
| `/` | Search Traces | Search in trace IDs, operation names, and service names |
| `f` | Filter Traces | Filter by error status, duration, or service |
| `c` | Critical Path | Highlight the critical path in the trace (longest span chain) |
| `Enter` | View Trace Details | Show span waterfall and detailed trace information |

### Traces View Detail Panel

When viewing a trace's details:

| Key | Action | Description |
|-----|--------|-------------|
| `Esc` | Close Details | Return to traces list |
| `↑/↓` | Navigate Spans | Move through spans in the waterfall view |
| `Enter` | Span Details | Show detailed information for the selected span |
| `c` | Toggle Critical Path | Highlight/unhighlight the critical path |

## Metrics View Specific

| Key | Action | Description |
|-----|--------|-------------|
| `/` | Search Metrics | Search in metric names, descriptions, and units |
| `f` | Filter Metrics | Filter by metric type (gauge, counter, histogram) or unit |
| `Enter` | View Metric Details | Show sparkline chart and detailed metric information |
| `+` or `=` | Zoom In | Zoom in on the metric chart (show fewer data points) |
| `-` | Zoom Out | Zoom out on the metric chart (show more data points) |

### Metrics View Detail Panel

When viewing a metric's details:

| Key | Action | Description |
|-----|--------|-------------|
| `Esc` | Close Details | Return to metrics list |
| `↑/↓` | Navigate Data Points | Move through individual data points |
| `←/→` | Time Navigation | Navigate through time series data |
| `+/-` | Zoom | Adjust the time range displayed in the chart |

## Help View

When the help screen is displayed:

| Key | Action | Description |
|-----|--------|-------------|
| `Esc` | Close Help | Return to the previous view |
| `q` | Close Help | Alternative key to close help |
| `↑/↓` | Scroll | Scroll through help content |

## Tips for Efficient Navigation

### Quick View Switching

The view switching keys (`l`, `t`, `m`) work from any view, allowing you to quickly jump between different data types without returning to a main menu.

### Search Workflow

1. Press `/` to start searching
2. Type your search query (case-insensitive)
3. Results filter automatically as you type
4. Press `Esc` to clear the search
5. Press `/` again to search for something else

### Filter Workflow

1. Press `f` to open the filter dialog
2. Select the filter type (severity, status, type, etc.)
3. Choose the filter value
4. Press `Enter` to apply
5. Press `f` again to modify or clear filters

### Detail Panel Navigation

When viewing details:
- Use `↑/↓` to scroll through content
- Press `Enter` on nested items to drill down further
- Press `Esc` to go back one level
- Press `Esc` multiple times to return to the main list

### Keyboard-Only Operation

The TUI is designed for complete keyboard-only operation:
- No mouse required
- All functions accessible via keyboard
- Vim-like navigation patterns where appropriate
- Consistent key bindings across views

## Customization

Currently, keyboard shortcuts are not customizable. If you need different key bindings, please open an issue on the GitHub repository.

## Accessibility

### Terminal Requirements

- Minimum terminal size: 80x24 characters
- Color support: 256 colors recommended
- Unicode support: Required for proper rendering of charts and borders

### Screen Reader Compatibility

The TUI is primarily visual and may have limited screen reader support. For accessible alternatives, consider using the Rotel CLI or API directly.

## See Also

- [Quickstart Guide](tui-quickstart.md) - Getting started with the TUI
- [Troubleshooting Guide](tui-troubleshooting.md) - Common issues and solutions
- [Main README](../README.md) - Overall Rotel documentation

<!-- Made with Bob -->
