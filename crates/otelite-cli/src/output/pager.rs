//! Pager support for long CLI output

use crate::config::Config;
use std::io::{self, Write};

/// Determines if output should be paged based on terminal state and config
pub fn should_use_pager(config: &Config, output_lines: usize) -> bool {
    // Don't page if explicitly disabled
    if config.no_pager {
        return false;
    }

    // Don't page if stdout is not a TTY (e.g., piped to another command)
    if !atty::is(atty::Stream::Stdout) {
        return false;
    }

    // Get terminal height
    let terminal_height = get_terminal_height().unwrap_or(24);

    // Use pager if output exceeds terminal height (leave some room for prompt)
    output_lines > (terminal_height.saturating_sub(2))
}

/// Get the terminal height in lines
fn get_terminal_height() -> Option<usize> {
    term_size::dimensions().map(|(_, height)| height)
}

/// Write output through a pager if appropriate, otherwise write directly to stdout
pub fn write_with_pager(config: &Config, content: &str) -> io::Result<()> {
    let lines = content.lines().count();

    if should_use_pager(config, lines) {
        // Try to use $PAGER environment variable first
        if let Ok(pager_cmd) = std::env::var("PAGER") {
            if let Err(e) = write_to_external_pager(&pager_cmd, content) {
                // If external pager fails, fall back to minus
                tracing::debug!(
                    "External pager '{}' failed: {}, falling back to minus",
                    pager_cmd,
                    e
                );
                write_to_minus_pager(config, content)?;
            }
        } else {
            // Use minus as default pager
            write_to_minus_pager(config, content)?;
        }
    } else {
        // Output is short enough, write directly to stdout
        print!("{}", content);
        io::stdout().flush()?;
    }

    Ok(())
}

/// Write content to an external pager command (e.g., less, more)
fn write_to_external_pager(pager_cmd: &str, content: &str) -> io::Result<()> {
    use std::process::{Command, Stdio};

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(pager_cmd)
        .stdin(Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(content.as_bytes())?;
        stdin.flush()?;
    }

    child.wait()?;
    Ok(())
}

/// Write content to the minus pager
fn write_to_minus_pager(config: &Config, content: &str) -> io::Result<()> {
    use minus::{LineNumbers, Pager};

    let pager = Pager::new();

    // Disable line numbers for cleaner output
    let _ = pager.set_line_numbers(LineNumbers::Disabled);

    // Set prompt based on color settings
    if config.no_color {
        let _ = pager.set_prompt("-- Press q to quit, / to search --");
    } else {
        let _ = pager.set_prompt("\x1b[7m-- Press q to quit, / to search --\x1b[0m");
    }

    // Push content to pager
    let _ = pager.push_str(content);

    // Run the pager (this blocks until user exits)
    minus::page_all(pager).map_err(|e| io::Error::other(format!("Pager error: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OutputFormat;
    use std::time::Duration;

    fn test_config(no_pager: bool, no_color: bool) -> Config {
        Config {
            endpoint: "http://localhost:3000".to_string(),
            timeout: Duration::from_secs(30),
            format: OutputFormat::Pretty,
            no_color,
            no_header: false,
            no_pager,
        }
    }

    #[test]
    fn test_should_use_pager_disabled() {
        let config = test_config(true, false);
        assert!(!should_use_pager(&config, 100));
    }

    #[test]
    fn test_should_use_pager_short_output() {
        let config = test_config(false, false);
        // Short output should not trigger pager
        assert!(!should_use_pager(&config, 5));
    }

    #[test]
    fn test_get_terminal_height() {
        // This test may fail in CI environments without a TTY
        // Just verify it returns a reasonable value or None
        if let Some(height) = get_terminal_height() {
            assert!(height > 0);
            assert!(height < 1000); // Sanity check
        }
    }
}
