//! Smoke tests: every subcommand must parse and `--help` must succeed.
//!
//! These catch clap registration bugs (duplicate global flags, ValueEnum type
//! mismatches, conflicting subcommand args) at CI time. The first iteration
//! caught a panic where `commands/usage.rs` defined a local `--format` arg
//! that collided with the global `--format` in `main.rs`.

use assert_cmd::Command;
use predicates::prelude::*;

// Test isolation (#192 gate): every invocation gets its own empty data
// dir so the suite never touches the developer's real otelite data and
// parallel tests never contend over one DB's first-run schema writes
// (index creation on a large shared DB outlives the 10 s busy timeout).
static SMOKE_DATA_DIR_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn otelite() -> Command {
    let n = SMOKE_DATA_DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut cmd = Command::cargo_bin("otelite").expect("otelite binary should build");
    cmd.env(
        "OTELITE_DATA_DIR",
        std::env::temp_dir().join(format!("otelite-smoke-{}-{n}", std::process::id())),
    );
    cmd
}

#[test]
fn root_help_succeeds() {
    otelite().arg("--help").assert().success();
}

#[test]
fn version_succeeds() {
    otelite().arg("--version").assert().success();
}

/// Each top-level subcommand must accept `--help`.
/// Add new subcommands here when introduced.
#[test]
fn subcommand_help_succeeds() {
    let subcommands = [
        "serve",
        "start",
        "stop",
        "restart",
        "status",
        "service",
        "logs",
        "traces",
        "metrics",
        "usage",
        "capabilities",
        "tui",
        "import",
        "mcp",
    ];
    for sub in subcommands {
        otelite()
            .args([sub, "--help"])
            .assert()
            .success()
            .stdout(predicates::str::is_empty().not());
    }
}

/// Nested subcommands (e.g. `logs list`, `traces show`) must accept `--help`.
#[test]
fn nested_subcommand_help_succeeds() {
    let pairs = [
        ("logs", "list"),
        ("logs", "show"),
        ("logs", "search"),
        ("logs", "export"),
        ("traces", "list"),
        ("traces", "show"),
        ("traces", "export"),
        ("metrics", "list"),
        ("metrics", "show"),
        ("metrics", "export"),
        ("service", "install"),
    ];
    for (parent, child) in pairs {
        otelite().args([parent, child, "--help"]).assert().success();
    }
}

/// `usage` must parse with every analytics flag combination — these were
/// added across releases and any one of them could collide with a global
/// flag in future. Parsing-only check; we don't execute against a database.
#[test]
fn usage_flag_combinations_parse() {
    let flag_groups: &[&[&str]] = &[
        &["--since", "24h"],
        &["--by-model"],
        &["--by-system"],
        &["--by-session"],
        &["--top", "10"],
        &["--latency"],
        &["--truncation"],
        &["--cache-rate"],
        &["--request-params"],
        &["--conv-depth"],
        &["--tools"],
        &["--error-types"],
        &["--model-drift"],
        &["--tool-approvals"],
        &["--stop-reasons"],
        &["--context-split"],
        &["--tool-errors", "10"], // explicit N (bare --tool-errors not used with --help due to clap ordering)
        &["--hour-of-day"],
        &["--calls"],
        &["--format", "json"],
        &["--format", "pretty"],
        &["--format", "json-compact"],
    ];
    for flags in flag_groups {
        let mut cmd = otelite();
        cmd.arg("usage").args(*flags).arg("--help");
        cmd.assert().success();
    }
}

/// `--since` with an invalid format must be rejected at parse time with a
/// friendly message, not a panic or a cryptic internal error.
#[test]
fn usage_since_invalid_format_rejected() {
    let invalid_values = ["invalid", "abc", "1x", "24", "h", ""];
    for val in invalid_values {
        otelite()
            .args(["usage", "--since", val, "--help"])
            .assert()
            .failure()
            .stderr(predicates::str::contains("Invalid time duration"));
    }
}

/// `--since` with valid formats must be accepted.
#[test]
fn usage_since_valid_formats_accepted() {
    let valid_values = ["1h", "24h", "7d", "30d", "15m", "1d"];
    for val in valid_values {
        otelite()
            .args(["usage", "--since", val, "--help"])
            .assert()
            .success();
    }
}

/// Global flags must be accepted on any subcommand without panicking.
#[test]
fn global_flags_accepted_on_subcommands() {
    for sub in ["logs", "traces", "metrics", "usage"] {
        otelite()
            .args(["--format", "json", sub, "--help"])
            .assert()
            .success();
        otelite()
            .args(["--no-color", sub, "--help"])
            .assert()
            .success();
        otelite()
            .args(["--timeout", "5", sub, "--help"])
            .assert()
            .success();
    }
}

// ── issue #142: exact time ranges and repeated --model ───────────────────────

/// --start/--end conflict with the rolling --since window.
#[test]
fn usage_start_conflicts_with_since() {
    otelite()
        .args(["usage", "--since", "24h", "--start", "2026-08-01"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("cannot be used with"));
}

/// --end without --start is rejected before any query.
#[test]
fn usage_end_requires_start() {
    otelite()
        .args(["usage", "--end", "2026-08-08"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--end requires --start"));
}

/// Repeated --model and the calendar/throughput flags parse together.
#[test]
fn usage_repeated_model_and_calendar_flags_parse() {
    otelite()
        .args([
            "usage",
            "--latency-percentiles",
            "--calendar-day",
            "--timezone",
            "Europe/London",
            "--model",
            "claude-opus-*",
            "--model",
            "claude-sonnet-*",
            "--throughput",
            "--start",
            "2026-08-01",
            "--end",
            "2026-08-08",
        ])
        .assert()
        .success();
}
