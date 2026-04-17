# Rotel TUI Troubleshooting Guide

Common issues and solutions for the Rotel Terminal User Interface.

## Table of Contents

- [Installation Issues](#installation-issues)
- [Connection Problems](#connection-problems)
- [Display Issues](#display-issues)
- [Performance Problems](#performance-problems)
- [Data Issues](#data-issues)
- [Keyboard and Input](#keyboard-and-input)
- [Configuration Issues](#configuration-issues)
- [Getting Help](#getting-help)

## Installation Issues

### Binary Not Found After Installation

**Problem**: After running `cargo install rotel-tui`, the `rotel-tui` command is not found.

**Solution**:
1. Ensure Cargo's bin directory is in your PATH:
   ```bash
   echo $PATH | grep -q "$HOME/.cargo/bin" || echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.bashrc
   source ~/.bashrc
   ```

2. Verify the binary was installed:
   ```bash
   ls -la ~/.cargo/bin/rotel-tui
   ```

3. If missing, try reinstalling:
   ```bash
   cargo install --force rotel-tui
   ```

### Build Fails with Dependency Errors

**Problem**: `cargo build` fails with errors about missing dependencies or incompatible versions.

**Solution**:
1. Update Rust to the latest stable version:
   ```bash
   rustup update stable
   ```

2. Clean the build cache:
   ```bash
   cargo clean
   cargo build --release
   ```

3. Check minimum Rust version (1.77+):
   ```bash
   rustc --version
   ```

## Connection Problems

### Cannot Connect to Rotel API

**Problem**: TUI shows "Connection failed" or "API unreachable" error.

**Symptoms**:
- Empty views with connection error message
- "Failed to fetch data" notifications
- Timeout errors

**Solutions**:

1. **Verify API is running**:
   ```bash
   curl http://localhost:8080/health
   # Should return: {"status":"healthy"}
   ```

2. **Check API URL in config**:
   ```bash
   cat ~/.config/rotel/tui.toml
   # Verify api_url is correct
   ```

3. **Test with explicit URL**:
   ```bash
   rotel-tui --api-url http://localhost:8080
   ```

4. **Check firewall settings**:
   - Ensure port 8080 is not blocked
   - Try disabling firewall temporarily to test

5. **Verify network connectivity**:
   ```bash
   ping localhost
   telnet localhost 8080
   ```

### Intermittent Connection Drops

**Problem**: Connection works initially but drops after some time.

**Solutions**:

1. **Increase timeout in config**:
   ```toml
   [api]
   timeout_seconds = 30  # Increase from default 10
   ```

2. **Check API server logs** for errors or resource issues

3. **Monitor network stability**:
   ```bash
   ping -c 100 localhost
   ```

4. **Reduce refresh rate** to decrease load:
   ```toml
   [refresh]
   interval_seconds = 5  # Increase from default 2
   ```

## Display Issues

### Garbled or Corrupted Display

**Problem**: Terminal shows strange characters, overlapping text, or corrupted UI.

**Solutions**:

1. **Verify terminal supports 256 colors**:
   ```bash
   echo $TERM
   # Should be xterm-256color or similar
   ```

2. **Set correct TERM variable**:
   ```bash
   export TERM=xterm-256color
   rotel-tui
   ```

3. **Try different terminal emulator**:
   - iTerm2 (macOS)
   - Alacritty (cross-platform)
   - Windows Terminal (Windows)

4. **Resize terminal** to minimum 80x24:
   ```bash
   # Check current size
   tput cols  # Should be >= 80
   tput lines # Should be >= 24
   ```

5. **Clear terminal before starting**:
   ```bash
   clear && rotel-tui
   ```

### Unicode Characters Not Displaying

**Problem**: Box drawing characters, arrows, or other symbols appear as question marks or squares.

**Solutions**:

1. **Verify UTF-8 locale**:
   ```bash
   locale | grep UTF-8
   # Should show UTF-8 encoding
   ```

2. **Set UTF-8 locale**:
   ```bash
   export LC_ALL=en_US.UTF-8
   export LANG=en_US.UTF-8
   ```

3. **Use ASCII-only mode** (if available in future versions):
   ```bash
   rotel-tui --ascii-only
   ```

### Colors Not Displaying Correctly

**Problem**: Colors are wrong, missing, or all appear the same.

**Solutions**:

1. **Test terminal color support**:
   ```bash
   curl -s https://gist.githubusercontent.com/lilydjwg/fdeaf79e921c2f413f44b6f613f6ad53/raw/94d8b2be62657e96488038b0e547e3009ed87d40/colors.py | python3
   ```

2. **Force 256 color mode**:
   ```bash
   TERM=xterm-256color rotel-tui
   ```

3. **Disable colors** (if needed):
   ```bash
   NO_COLOR=1 rotel-tui
   ```

### Screen Flickers or Flashes

**Problem**: Display flickers, especially when data updates.

**Solutions**:

1. **Increase refresh interval**:
   ```toml
   [refresh]
   interval_seconds = 5  # Reduce update frequency
   ```

2. **Disable auto-scroll** in logs view (press `s`)

3. **Use different terminal emulator** with better rendering

## Performance Problems

### High CPU Usage

**Problem**: TUI consumes excessive CPU resources.

**Solutions**:

1. **Increase refresh interval**:
   ```toml
   [refresh]
   interval_seconds = 5  # Default is 2
   ```

2. **Reduce data volume**:
   - Apply filters to limit displayed items
   - Use time range filters
   - Close detail panels when not needed

3. **Check for runaway processes**:
   ```bash
   top -p $(pgrep rotel-tui)
   ```

4. **Update to latest version** (may include performance fixes)

### High Memory Usage

**Problem**: TUI uses excessive memory or grows over time.

**Solutions**:

1. **Restart TUI periodically** if memory grows unbounded

2. **Reduce data retention**:
   ```toml
   [performance]
   max_items_in_memory = 500  # Default is 1000
   ```

3. **Apply filters** to reduce data volume

4. **Monitor memory usage**:
   ```bash
   ps aux | grep rotel-tui
   ```

### Slow Response to Input

**Problem**: Keyboard input is laggy or delayed.

**Solutions**:

1. **Increase debounce interval**:
   ```toml
   [performance]
   min_refresh_interval_ms = 200  # Default is 100
   ```

2. **Close detail panels** to reduce rendering load

3. **Reduce terminal size** if very large

4. **Check system resources**:
   ```bash
   top
   # Look for other processes consuming CPU
   ```

## Data Issues

### No Data Displayed

**Problem**: Views are empty even though data should be available.

**Solutions**:

1. **Verify data exists in API**:
   ```bash
   curl http://localhost:8080/api/v1/logs | jq
   curl http://localhost:8080/api/v1/traces | jq
   curl http://localhost:8080/api/v1/metrics | jq
   ```

2. **Check time range filters**:
   - Press `f` to view/clear filters
   - Ensure time range includes recent data

3. **Manually refresh**:
   - Press `r` to force refresh

4. **Check API logs** for errors

### Data Not Updating

**Problem**: Data is stale and doesn't refresh automatically.

**Solutions**:

1. **Verify auto-refresh is enabled**:
   ```toml
   [refresh]
   enabled = true
   interval_seconds = 2
   ```

2. **Manually refresh** with `r` key

3. **Check API is receiving new data**:
   ```bash
   # Send test data
   curl -X POST http://localhost:8080/v1/logs \
     -H "Content-Type: application/json" \
     -d '{"resourceLogs":[...]}'
   ```

4. **Restart TUI** to clear any stuck state

### Incorrect or Corrupted Data

**Problem**: Data displays incorrectly or appears corrupted.

**Solutions**:

1. **Verify data format** in API response:
   ```bash
   curl http://localhost:8080/api/v1/logs | jq '.[0]'
   ```

2. **Check for API version mismatch**:
   - Ensure TUI and API versions are compatible
   - Update both to latest versions

3. **Clear cache** (if caching is implemented):
   ```bash
   rm -rf ~/.cache/rotel/
   ```

## Keyboard and Input

### Keyboard Shortcuts Not Working

**Problem**: Pressing keys doesn't trigger expected actions.

**Solutions**:

1. **Check terminal key bindings**:
   - Some terminals intercept certain key combinations
   - Try alternative shortcuts (e.g., `h` instead of `?` for help)

2. **Verify terminal mode**:
   ```bash
   stty -a | grep -i echo
   # Should show echo enabled
   ```

3. **Test in different terminal emulator**

4. **Check for conflicting tmux/screen bindings**

### Cannot Type in Search Box

**Problem**: Search mode doesn't accept input.

**Solutions**:

1. **Ensure search mode is active**:
   - Press `/` to enter search mode
   - Look for search prompt at bottom of screen

2. **Check terminal input mode**:
   ```bash
   stty sane
   ```

3. **Restart TUI** if input is stuck

## Configuration Issues

### Config File Not Found

**Problem**: TUI doesn't load configuration file.

**Solutions**:

1. **Create config directory**:
   ```bash
   mkdir -p ~/.config/rotel
   ```

2. **Create default config**:
   ```bash
   rotel-tui --print-default-config > ~/.config/rotel/tui.toml
   ```

3. **Verify config location**:
   ```bash
   rotel-tui --config ~/.config/rotel/tui.toml
   ```

### Invalid Configuration

**Problem**: TUI fails to start with config error.

**Solutions**:

1. **Validate TOML syntax**:
   ```bash
   # Use online TOML validator or:
   python3 -c "import toml; toml.load('~/.config/rotel/tui.toml')"
   ```

2. **Check for typos** in config keys

3. **Use default config** temporarily:
   ```bash
   rotel-tui --no-config
   ```

4. **Review error message** for specific issue

## Getting Help

### Enable Debug Logging

For detailed troubleshooting information:

```bash
RUST_LOG=debug rotel-tui 2> rotel-tui.log
```

Then check `rotel-tui.log` for detailed error messages.

### Collect System Information

When reporting issues, include:

```bash
# System info
uname -a
echo $TERM
locale

# Rotel version
rotel-tui --version

# Rust version
rustc --version

# Terminal size
tput cols
tput lines

# Config
cat ~/.config/rotel/tui.toml
```

### Report Issues

If you encounter a bug or need help:

1. **Check existing issues**: https://github.com/yourusername/rotel/issues
2. **Create new issue** with:
   - Description of problem
   - Steps to reproduce
   - System information (see above)
   - Debug logs (if applicable)
   - Screenshots (if display issue)

### Community Support

- **GitHub Discussions**: Ask questions and share tips
- **Discord/Slack**: Real-time community support (if available)
- **Documentation**: Check other docs in `docs/` directory

## See Also

- [Quickstart Guide](tui-quickstart.md) - Getting started with the TUI
- [Keyboard Shortcuts](tui-shortcuts.md) - Complete shortcuts reference
- [Main README](../README.md) - Overall Rotel documentation

<!-- Made with Bob -->
