# Rotel TUI Quickstart Guide

The Rotel Terminal User Interface (TUI) provides a powerful, keyboard-driven interface for viewing and analyzing OpenTelemetry data in real-time.

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/planetf1/rotel.git
cd rotel

# Build the TUI
cargo build --release --package rotel-tui

# Run the TUI
./target/release/rotel-tui
```

## Quick Start

### Basic Usage

Start the TUI with default settings (connects to `http://localhost:4318`):

```bash
rotel-tui
```

### Custom Configuration

Connect to a different Rotel API endpoint:

```bash
rotel-tui --api-url http://localhost:8080
```

Set refresh interval (in seconds):

```bash
rotel-tui --refresh-interval 5
```

Start with a specific view:

```bash
rotel-tui --initial-view traces
```

## Navigation

### View Switching

- **`l`** - Switch to Logs view
- **`t`** - Switch to Traces view  
- **`m`** - Switch to Metrics view
- **`?` or `h`** - Show help screen

### Within Views

- **`↑/↓`** - Navigate up/down through items
- **`Enter`** - Expand selected item to show details
- **`Esc`** - Close detail panel or return to previous view
- **`/`** - Start search
- **`f`** - Apply filters
- **`r`** - Refresh data manually

### Application Control

- **`q`** - Quit application
- **`Ctrl+C`** - Force quit

## Views Overview

### Logs View

View and search through log entries with:
- Timestamp, severity level, and message
- Color-coded severity (ERROR=red, WARN=yellow, INFO=green, DEBUG=blue)
- Full log details including attributes and resource information
- Search and filter capabilities
- Auto-scroll toggle with `s` key

### Traces View

Analyze distributed traces with:
- Trace ID, operation name, duration, and status
- Span waterfall visualization showing hierarchy
- Error highlighting for failed spans
- Detailed span information including attributes, events, and links
- Filter by duration, status, or service

### Metrics View

Monitor metrics with:
- Metric name, type, latest value, and data points
- Sparkline charts showing value trends
- Min/max/avg statistics
- Filter by metric type or unit
- Zoom controls with `+` and `-` keys

## Tips

1. **Performance**: The TUI automatically limits data to 1000 items per view to maintain responsiveness
2. **Refresh**: Data refreshes automatically based on the configured interval (default: 5 seconds)
3. **Search**: Use `/` to search within the current view - search is case-insensitive
4. **Filters**: Press `f` to apply filters - useful for focusing on specific severity levels, services, or metric types
5. **Help**: Press `?` anytime to see the full keyboard shortcuts reference

## Troubleshooting

### Connection Issues

If you see "Failed to fetch" errors:
1. Verify the Rotel API is running: `curl http://localhost:4318/health`
2. Check the API URL is correct: `rotel-tui --api-url http://your-host:port`
3. Ensure network connectivity between TUI and API

### Performance Issues

If the TUI feels slow:
1. Increase the refresh interval: `rotel-tui --refresh-interval 10`
2. Use filters to reduce the amount of data displayed
3. Check your terminal emulator performance

### Display Issues

If the UI looks corrupted:
1. Ensure your terminal supports 256 colors
2. Try resizing the terminal window (minimum 80x24 recommended)
3. Check terminal compatibility with `echo $TERM`

## Next Steps

- See [Keyboard Shortcuts Reference](tui-shortcuts.md) for complete key bindings
- Read [Troubleshooting Guide](tui-troubleshooting.md) for detailed problem resolution
- Check the main [README](../README.md) for overall Rotel documentation

## Examples

### Monitoring Production Logs

```bash
# Connect to production API and start in logs view
rotel-tui --api-url https://prod-rotel.example.com --initial-view logs

# Once running:
# 1. Press 'f' to filter by severity
# 2. Select ERROR to see only errors
# 3. Press '/' to search for specific terms
# 4. Press Enter on a log to see full details
```

### Analyzing Trace Performance

```bash
# Start in traces view with faster refresh
rotel-tui --initial-view traces --refresh-interval 2

# Once running:
# 1. Look for traces with long durations
# 2. Press Enter to see span waterfall
# 3. Identify slow spans in the hierarchy
# 4. Press 'f' to filter by service or status
```

### Monitoring Metrics

```bash
# Start in metrics view
rotel-tui --initial-view metrics

# Once running:
# 1. Use arrow keys to browse metrics
# 2. Press Enter to see sparkline chart
# 3. Use '+' and '-' to zoom in/out
# 4. Press 'f' to filter by metric type
```

<!-- Made with Bob -->
