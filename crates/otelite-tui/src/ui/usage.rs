use crate::state::usage::{CapabilityRow, DailyThroughputRow};
use crate::state::UsageState;
use crate::ui::render_tab_bar;
use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc};
use otelite_core::api::{
    GenAiCapabilityResponse, GenAiMetricCapability, LatencyPercentilesResponse,
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame,
};
use std::collections::HashMap;

/// Project the calendar-day percentile grid (duration metric) into daily
/// throughput rows: one row per day × model, empty days omitted, missing
/// throughput rendered as "—", weak samples (n < 10) marked with "†".
/// Wording mirrors the web's daily throughput table (#144).
pub fn daily_throughput_rows(
    resp: &LatencyPercentilesResponse,
    tz: &str,
) -> Vec<DailyThroughputRow> {
    let series = match resp.metrics.get("duration") {
        Some(s) => s,
        None => return Vec::new(),
    };
    // Invalid timezone falls back to UTC (the API would have 400'd first).
    let tz: chrono_tz::Tz = tz.parse().unwrap_or_else(|_| "UTC".parse().unwrap());
    let mut rows: Vec<DailyThroughputRow> = Vec::new();
    for (model, points) in &series.models {
        for p in points {
            if p.count == 0 {
                continue; // omit empty days
            }
            let n_star = if p.throughput_sample_count > 0 {
                if p.throughput_sample_count < 10 {
                    format!("{}†", p.throughput_sample_count)
                } else {
                    p.throughput_sample_count.to_string()
                }
            } else {
                "—".to_string()
            };
            let tps = match (
                p.throughput_p10_tok_s,
                p.throughput_p50_tok_s,
                p.throughput_p90_tok_s,
            ) {
                (Some(p10), Some(p50), Some(p90)) => {
                    format!("{:.0} / {:.0} / {:.0}", p10, p50, p90)
                },
                _ => "—".to_string(),
            };
            rows.push(DailyThroughputRow {
                day: day_label_local(p.ts, &tz),
                model: model.clone(),
                calls: p.count as usize,
                n_star,
                tps,
            });
        }
    }
    rows.sort_by(|a, b| a.day.cmp(&b.day).then_with(|| a.model.cmp(&b.model)));
    rows
}

/// Local calendar-day label (YYYY-MM-DD) for a bucket-start timestamp in ns.
/// The API aligns `timestamp` to local midnight in the requested timezone, so
/// shifting the instant by that timezone's offset yields the local date.
/// Project the capability report into compact panel rows: one row per
/// provider/model/emitter identity. Cells keep the full vocabulary
/// (`availability/quality[/derivation] (valid/observed)`); derivation is only
/// shown when not `native`. Wording mirrors the CLI's `capabilities` table
/// (issue #120 parity contract).
pub fn capability_rows(resp: &GenAiCapabilityResponse) -> Vec<CapabilityRow> {
    let cell = |m: &GenAiMetricCapability| {
        let mut c = format!("{}/{}", m.availability, m.quality);
        if m.derivation != "native" {
            c.push_str(&format!("/{}", m.derivation));
        }
        let n = if m.observed_count > 0 {
            format!("{}/{} obs", m.valid_count, m.observed_count)
        } else {
            format!("0/{} elig", m.eligible_count)
        };
        format!("{c} ({n})")
    };
    resp.reports
        .iter()
        .map(|r| {
            let identity = match (&r.provider, &r.model) {
                (Some(p), Some(m)) => format!("{p}/{m}"),
                (Some(p), None) => p.clone(),
                (None, Some(m)) => m.clone(),
                (None, None) => "(unknown)".to_string(),
            };
            CapabilityRow {
                identity,
                emitter: r.emitter.clone(),
                requests: r.request_count,
                input: cell(&r.input_tokens),
                output: cell(&r.output_tokens),
                ttft: cell(&r.ttft),
            }
        })
        .collect()
}

fn day_label_local(ts_ns: i64, tz: &chrono_tz::Tz) -> String {
    let dt: DateTime<Utc> = DateTime::from_timestamp_nanos(ts_ns);
    // The offset at this instant shifts the instant into local wall time.
    let local = dt.with_timezone(tz);
    format!(
        "{:04}-{:02}-{:02}",
        local.year(),
        local.month(),
        local.day()
    )
}

pub fn render_usage_view(
    frame: &mut Frame,
    area: Rect,
    state: &UsageState,
    api_error: Option<&str>,
) {
    let content_area = if let Some(err) = api_error {
        let splits = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(area);
        let err_line = Line::from(Span::styled(
            format!(" Error: {} ", err),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
        frame.render_widget(Paragraph::new(err_line), splits[0]);
        splits[1]
    } else {
        area
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Tab bar
            Constraint::Length(4), // Summary cards
            Constraint::Min(6),    // Main tables (scrollable area)
            Constraint::Length(1), // Status bar
        ])
        .split(content_area);

    render_tab_bar(frame, chunks[0], "Usage");
    render_summary_cards(frame, chunks[1], state);
    render_tables(frame, chunks[2], state);
    render_status_bar(frame, chunks[3], state);
}

fn render_summary_cards(frame: &mut Frame, area: Rect, state: &UsageState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
        ])
        .split(area);

    if let Some(ref usage) = state.token_usage {
        let summary = &usage.summary;
        render_card(
            frame,
            cols[0],
            "Requests",
            &summary.total_requests.to_string(),
        );
        render_card(
            frame,
            cols[1],
            "Input Tokens",
            &fmt_tokens(summary.total_input_tokens),
        );
        render_card(
            frame,
            cols[2],
            "Output Tokens",
            &fmt_tokens(summary.total_output_tokens),
        );
        let cache_rate = if summary.total_cache_read_tokens > 0 {
            let denom = summary.total_cache_read_tokens + summary.total_input_tokens;
            if denom > 0 {
                format!(
                    "{:.0}% cached",
                    summary.total_cache_read_tokens as f64 / denom as f64 * 100.0
                )
            } else {
                "—".to_string()
            }
        } else {
            "no cache".to_string()
        };
        render_card(frame, cols[3], "Cache Read", &cache_rate);
    } else if state.is_loading {
        for col in cols.iter() {
            render_card(frame, *col, "…", "loading");
        }
    } else {
        render_card(frame, cols[0], "Requests", "—");
        render_card(frame, cols[1], "Input Tokens", "—");
        render_card(frame, cols[2], "Output Tokens", "—");
        render_card(frame, cols[3], "Cache Read", "—");
    }
}

fn render_card(frame: &mut Frame, area: Rect, label: &str, value: &str) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(label, Style::default().fg(Color::Cyan)));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let text = Paragraph::new(Span::styled(
        value,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(text, inner);
}

fn render_tables(frame: &mut Frame, area: Rect, state: &UsageState) {
    let show_conv = state
        .conversation_depth
        .as_ref()
        .map(|d| d.total_conversations > 0)
        .unwrap_or(false);
    let show_trunc = state.truncation_rate.iter().any(|r| r.truncated > 0);
    let show_cache = state
        .cache_hit_rate
        .iter()
        .any(|r| r.total_cache_read_tokens > 0);
    let show_tools = !state.tool_usage.is_empty();
    let show_errors = !state.error_types.is_empty();
    let show_drift = state.model_drift.iter().any(|p| p.differs);
    let show_approvals = state
        .tool_approvals
        .as_ref()
        .map(|a| a.total > 0)
        .unwrap_or(false);
    let show_stop = !state.stop_reasons.is_empty();
    let show_ctx = !state.context_split.is_empty();
    let show_tool_errs = !state.tool_errors.is_empty();
    let show_hour = state.hour_of_day.iter().any(|b| b.llm_calls > 0);
    let show_calls = !state.calls_series.is_empty();
    let show_daily = !state.daily_throughput.is_empty();
    let show_caps = !state.capabilities.is_empty();

    let mut constraints = vec![Constraint::Min(5)]; // latency always shown
    if show_daily {
        constraints.push(Constraint::Length(9));
    }
    if show_caps {
        constraints.push(Constraint::Length(9));
    }
    if show_tools {
        constraints.push(Constraint::Length(6));
    }
    if show_trunc || show_cache {
        constraints.push(Constraint::Length(6));
    }
    if show_errors || show_drift {
        constraints.push(Constraint::Length(6));
    }
    if show_conv {
        constraints.push(Constraint::Length(5));
    }
    if show_approvals || show_stop {
        constraints.push(Constraint::Length(7));
    }
    if show_ctx || show_tool_errs {
        constraints.push(Constraint::Length(6));
    }
    if show_hour {
        constraints.push(Constraint::Length(9));
    }
    if show_calls {
        constraints.push(Constraint::Length(8));
    }

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    render_latency_table(frame, sections[0], state);

    let mut idx = 1usize;
    if show_daily {
        render_daily_throughput_table(frame, sections[idx], state);
        idx += 1;
    }
    if show_caps {
        render_capability_table(frame, sections[idx], state);
        idx += 1;
    }
    if show_tools {
        render_tool_usage_table(frame, sections[idx], state);
        idx += 1;
    }
    if show_trunc || show_cache {
        render_trunc_cache_row(frame, sections[idx], state, show_trunc, show_cache);
        idx += 1;
    }
    if show_errors || show_drift {
        render_errors_drift_row(frame, sections[idx], state, show_errors, show_drift);
        idx += 1;
    }
    if show_conv {
        render_conv_depth(frame, sections[idx], state);
        idx += 1;
    }
    if show_approvals || show_stop {
        render_approvals_stop_row(frame, sections[idx], state, show_approvals, show_stop);
        idx += 1;
    }
    if show_ctx || show_tool_errs {
        render_ctx_tool_errs_row(frame, sections[idx], state, show_ctx, show_tool_errs);
        idx += 1;
    }
    if show_hour {
        render_hour_of_day(frame, sections[idx], state);
        idx += 1;
    }
    if show_calls {
        render_calls_series(frame, sections[idx], state);
        // idx += 1; // last section
    }
    let _ = idx; // suppress unused warning if all sections are omitted
}

fn render_latency_table(frame: &mut Frame, area: Rect, state: &UsageState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Latency by Model (tok/s = derived: span duration ≠ gen time) ");

    if state.latency_stats.is_empty() {
        let msg = if state.is_loading {
            "Loading…"
        } else {
            "No latency data"
        };
        frame.render_widget(Paragraph::new(msg).block(block), area);
        return;
    }

    let header = Row::new(vec![
        Cell::from("Model").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("N").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("p50ms").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("p95ms").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        // p10/p50/p90: lower-tail / median / upper-reference (#119).
        Cell::from("tok/s p10").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("tok/s p50").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("tok/s p90").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("ctx p50").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("ctx p95").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("o/ctx p50").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("o/ctx p95").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("TTFT p50").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("TTFT p95").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let rows: Vec<Row> = state
        .latency_stats
        .iter()
        .map(|s| {
            let model = s.model.as_deref().unwrap_or("(unknown)");
            let ttft_p50 = if s.ttft_degenerate {
                format!(
                    "buffered ({}%)",
                    s.ttft_degenerate_count * 100 / s.ttft_count
                )
            } else if s.ttft_count > 0 {
                s.ttft_p50_ms.map_or("—".to_string(), |v| v.to_string())
            } else {
                "—".to_string()
            };
            let ttft_p95 = if s.ttft_degenerate {
                format!(
                    "buffered ({}%)",
                    s.ttft_degenerate_count * 100 / s.ttft_count
                )
            } else if s.ttft_count > 0 {
                s.ttft_p95_ms.map_or("—".to_string(), |v| v.to_string())
            } else {
                "—".to_string()
            };
            let ratio_str_p95 = s
                .output_input_ratio_p95
                .map_or("—".to_string(), |v| format!("{:.2}×", v));
            let ratio_p95_cell = Cell::from(ratio_str_p95);
            let p95_dur_cell = if latency_p95_is_slow(s.p95_ms) {
                Cell::from(s.p95_ms.to_string()).style(Style::default().fg(Color::Yellow))
            } else {
                Cell::from(s.p95_ms.to_string())
            };
            let row = Row::new(vec![
                Cell::from(truncate(model, 28)),
                Cell::from(s.count.to_string()),
                Cell::from(s.p50_ms.to_string()),
                p95_dur_cell,
                Cell::from(
                    s.derived_tokens_per_sec_p10
                        .map_or("—".to_string(), |v| format!("{:.0}", v)),
                ),
                Cell::from(
                    s.derived_tokens_per_sec_p50
                        .map_or("—".to_string(), |v| format!("{:.0}", v)),
                ),
                Cell::from(
                    s.derived_tokens_per_sec_p90
                        .map_or("—".to_string(), |v| format!("{:.0}", v)),
                ),
                Cell::from(
                    s.input_tokens_p50
                        .map_or("—".to_string(), |v| fmt_tokens(v as u64)),
                ),
                Cell::from(
                    s.input_tokens_p95
                        .map_or("—".to_string(), |v| fmt_tokens(v as u64)),
                ),
                Cell::from(
                    s.output_input_ratio_p50
                        .map_or("—".to_string(), |v| format!("{:.2}×", v)),
                ),
                ratio_p95_cell,
                Cell::from(ttft_p50),
                Cell::from(ttft_p95),
            ]);
            row
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Min(20),   // Model
            Constraint::Length(5), // N
            Constraint::Length(6), // p50ms
            Constraint::Length(6), // p95ms
            Constraint::Length(9), // tok/s p10
            Constraint::Length(9), // tok/s p50
            Constraint::Length(9), // tok/s p90
            Constraint::Length(7), // ctx p50
            Constraint::Length(7), // ctx p95
            Constraint::Length(9), // o/ctx p50
            Constraint::Length(9), // o/ctx p95
            Constraint::Length(8), // TTFT p50
            Constraint::Length(8), // TTFT p95
        ],
    )
    .header(header)
    .block(block);

    frame.render_widget(table, area);
}

fn render_daily_throughput_table(frame: &mut Frame, area: Rect, state: &UsageState) {
    let tz = state.daily_throughput_tz.as_deref().unwrap_or("?");
    let block = Block::default().borders(Borders::ALL).title(format!(
        " Daily throughput {tz} (tok/s = derived end-to-end, span incl. provider/queue/network) "
    ));

    if state.daily_throughput.is_empty() {
        let msg = if state.is_loading {
            "Loading…"
        } else {
            "No throughput data"
        };
        frame.render_widget(Paragraph::new(msg).block(block), area);
        return;
    }

    let header = Row::new(vec![
        Cell::from("Day").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Model").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("N").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("N*†").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("tok/s p10/p50/p90").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let rows: Vec<Row> = state
        .daily_throughput
        .iter()
        .map(|r| {
            Row::new(vec![
                Cell::from(r.day.clone()),
                Cell::from(truncate(&r.model, 28)),
                Cell::from(r.calls.to_string()),
                Cell::from(r.n_star.clone()),
                Cell::from(r.tps.clone()),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(10), // Day
            Constraint::Min(20),    // Model
            Constraint::Length(5),  // N
            Constraint::Length(5),  // N*†
            Constraint::Min(16),    // tok/s triple
        ],
    )
    .header(header)
    .block(block);

    frame.render_widget(table, area);
}

fn render_capability_table(frame: &mut Frame, area: Rect, state: &UsageState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Telemetry capabilities (availability/quality; unavailable is never zero) ");

    if state.capabilities.is_empty() {
        let msg = if state.is_loading {
            "Loading…"
        } else {
            "No capability data"
        };
        frame.render_widget(Paragraph::new(msg).block(block), area);
        return;
    }

    let header_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let header = Row::new(vec![
        Cell::from("Identity").style(header_style),
        Cell::from("Emitter").style(header_style),
        Cell::from("Req").style(header_style),
        Cell::from("Input").style(header_style),
        Cell::from("Output").style(header_style),
        Cell::from("TTFT").style(header_style),
    ]);

    let rows: Vec<Row> = state
        .capabilities
        .iter()
        .map(|r| {
            let cell_style = |text: &str| {
                if text.contains("/invalid") || text.contains("/degenerate") {
                    Style::default().fg(Color::Yellow)
                } else if text.contains("absent") {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default()
                }
            };
            Row::new(vec![
                Cell::from(truncate(&r.identity, 28)),
                Cell::from(truncate(&r.emitter, 14)),
                Cell::from(r.requests.to_string()),
                Cell::from(r.input.clone()).style(cell_style(&r.input)),
                Cell::from(r.output.clone()).style(cell_style(&r.output)),
                Cell::from(r.ttft.clone()).style(cell_style(&r.ttft)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Min(24),    // Identity
            Constraint::Length(12), // Emitter
            Constraint::Length(5),  // Req
            Constraint::Min(16),    // Input
            Constraint::Min(16),    // Output
            Constraint::Min(16),    // TTFT
        ],
    )
    .header(header)
    .block(block);

    frame.render_widget(table, area);
}

fn render_trunc_cache_row(
    frame: &mut Frame,
    area: Rect,
    state: &UsageState,
    show_trunc: bool,
    show_cache: bool,
) {
    let (left, right) = if show_trunc && show_cache {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
            .split(area);
        (Some(cols[0]), Some(cols[1]))
    } else if show_trunc {
        (Some(area), None)
    } else {
        (None, Some(area))
    };

    if let Some(a) = left {
        render_truncation_table(frame, a, state);
    }
    if let Some(a) = right {
        render_cache_table(frame, a, state);
    }
}

fn render_truncation_table(frame: &mut Frame, area: Rect, state: &UsageState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Truncation Rate ");

    let rows: Vec<Row> = state
        .truncation_rate
        .iter()
        .filter(|r| r.truncated > 0)
        .map(|r| {
            let rate_pct = r.rate * 100.0;
            let color = if rate_pct > 5.0 {
                Color::Red
            } else if rate_pct > 1.0 {
                Color::Yellow
            } else {
                Color::Green
            };
            Row::new(vec![
                Cell::from(truncate(r.model.as_deref().unwrap_or("(unknown)"), 22)),
                Cell::from(r.total.to_string()),
                Cell::from(format!("{:.1}%", rate_pct)).style(Style::default().fg(color)),
            ])
        })
        .collect();

    if rows.is_empty() {
        frame.render_widget(Paragraph::new("No truncations").block(block), area);
        return;
    }

    let header = Row::new(vec![
        Cell::from("Model").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Total").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Rate").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let table = Table::new(
        rows,
        [
            Constraint::Min(10),
            Constraint::Length(6),
            Constraint::Length(6),
        ],
    )
    .header(header)
    .block(block);
    frame.render_widget(table, area);
}

fn render_cache_table(frame: &mut Frame, area: Rect, state: &UsageState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Cache Hit Rate ");

    let rows: Vec<Row> = state
        .cache_hit_rate
        .iter()
        .filter(|r| r.total_cache_read_tokens > 0)
        .map(|r| {
            let hit_pct = r.hit_rate.unwrap_or(0.0) * 100.0;
            let color = if hit_pct >= 20.0 {
                Color::Green
            } else if hit_pct >= 5.0 {
                Color::Yellow
            } else {
                Color::White
            };
            Row::new(vec![
                Cell::from(truncate(r.model.as_deref().unwrap_or("(unknown)"), 22)),
                Cell::from(fmt_tokens(r.total_input_tokens)),
                Cell::from(fmt_tokens(r.total_cache_read_tokens)),
                Cell::from(format!("{:.1}%", hit_pct)).style(Style::default().fg(color)),
            ])
        })
        .collect();

    if rows.is_empty() {
        frame.render_widget(Paragraph::new("No cache reads").block(block), area);
        return;
    }

    let header = Row::new(vec![
        Cell::from("Model").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Input").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("CacheRead").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("HitRate").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let table = Table::new(
        rows,
        [
            Constraint::Min(10),
            Constraint::Length(8),
            Constraint::Length(9),
            Constraint::Length(8),
        ],
    )
    .header(header)
    .block(block);
    frame.render_widget(table, area);
}

fn render_conv_depth(frame: &mut Frame, area: Rect, state: &UsageState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Conversation Depth ");

    if let Some(ref d) = state.conversation_depth {
        if d.total_conversations > 0 {
            let text = format!(
                "  {} conversations  |  avg {:.1} turns  |  p50 {}  |  p95 {}  |  p99 {}",
                d.total_conversations, d.avg_turns, d.p50_turns, d.p95_turns, d.p99_turns
            );
            frame.render_widget(
                Paragraph::new(Span::styled(text, Style::default().fg(Color::White))).block(block),
                area,
            );
            return;
        }
    }
    frame.render_widget(
        Paragraph::new("No conversations with conversation_id observed").block(block),
        area,
    );
}

fn render_tool_usage_table(frame: &mut Frame, area: Rect, state: &UsageState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Tool Usage (success rate) ");

    if state.tool_usage.is_empty() {
        frame.render_widget(Paragraph::new("No tool calls").block(block), area);
        return;
    }

    let header = Row::new(vec![
        Cell::from("Tool").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Calls").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Success%").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Errors").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Avg ms").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let rows: Vec<Row> = state
        .tool_usage
        .iter()
        .map(|r| {
            let success_pct = if r.count > 0 {
                r.success_count as f64 / r.count as f64 * 100.0
            } else {
                0.0
            };
            let color = if success_pct < 90.0 {
                Color::Red
            } else if success_pct < 99.0 {
                Color::Yellow
            } else {
                Color::Green
            };
            Row::new(vec![
                Cell::from(truncate(&r.tool_name, 24)),
                Cell::from(r.count.to_string()),
                Cell::from(format!("{:.1}%", success_pct)).style(Style::default().fg(color)),
                Cell::from(r.error_count.to_string()),
                Cell::from(format!("{:.0}", r.avg_duration_ms)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Min(16),
            Constraint::Length(6),
            Constraint::Length(9),
            Constraint::Length(7),
            Constraint::Length(8),
        ],
    )
    .header(header)
    .block(block);

    frame.render_widget(table, area);
}

fn render_errors_drift_row(
    frame: &mut Frame,
    area: Rect,
    state: &UsageState,
    show_errors: bool,
    show_drift: bool,
) {
    let (left, right) = if show_errors && show_drift {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
            .split(area);
        (Some(cols[0]), Some(cols[1]))
    } else if show_errors {
        (Some(area), None)
    } else {
        (None, Some(area))
    };

    if let Some(a) = left {
        render_error_types_table(frame, a, state);
    }
    if let Some(a) = right {
        render_model_drift_table(frame, a, state);
    }
}

fn render_error_types_table(frame: &mut Frame, area: Rect, state: &UsageState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Error Types ");

    if state.error_types.is_empty() {
        frame.render_widget(Paragraph::new("No errors").block(block), area);
        return;
    }

    // Aggregate by bucket for a compact summary
    let mut bucket_counts: HashMap<&str, usize> = HashMap::new();
    for r in &state.error_types {
        *bucket_counts.entry(r.bucket.as_str()).or_insert(0) += r.count;
    }

    let header = Row::new(vec![
        Cell::from("Bucket").style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Cell::from("Type").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Cnt").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let rows: Vec<Row> = state
        .error_types
        .iter()
        .map(|r| {
            Row::new(vec![
                Cell::from(r.bucket.as_str()).style(Style::default().fg(Color::Red)),
                Cell::from(truncate(&r.error_type, 20)),
                Cell::from(r.count.to_string()).style(Style::default().fg(Color::Yellow)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Min(10),
            Constraint::Length(5),
        ],
    )
    .header(header)
    .block(block);

    frame.render_widget(table, area);
}

fn render_model_drift_table(frame: &mut Frame, area: Rect, state: &UsageState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Model Drift ");

    let drifted: Vec<_> = state.model_drift.iter().filter(|p| p.differs).collect();

    if drifted.is_empty() {
        frame.render_widget(
            Paragraph::new("No drift — request and response models match").block(block),
            area,
        );
        return;
    }

    let header = Row::new(vec![
        Cell::from("Requested").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Served").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("N").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let rows: Vec<Row> = drifted
        .iter()
        .map(|r| {
            let req = r.request_model.as_deref().unwrap_or("(unknown)");
            let resp = r.response_model.as_deref().unwrap_or("(unknown)");
            Row::new(vec![
                Cell::from(truncate(req, 22)),
                Cell::from(truncate(resp, 22)).style(Style::default().fg(Color::Yellow)),
                Cell::from(r.count.to_string()),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Min(14),
            Constraint::Min(14),
            Constraint::Length(5),
        ],
    )
    .header(header)
    .block(block);

    frame.render_widget(table, area);
}

fn render_status_bar(frame: &mut Frame, area: Rect, state: &UsageState) {
    let text = if let Some(ref e) = state.error {
        Line::from(Span::styled(
            format!(" Error: {} | u:Usage r:Refresh q:Quit", e),
            Style::default().fg(Color::Red),
        ))
    } else {
        Line::from(vec![
            Span::styled(" u", Style::default().fg(Color::Yellow)),
            Span::raw(":Usage  "),
            Span::styled("r", Style::default().fg(Color::Yellow)),
            Span::raw(":Refresh  "),
            Span::styled("q", Style::default().fg(Color::Red)),
            Span::raw(":Quit"),
        ])
    };
    frame.render_widget(Paragraph::new(text), area);
}

fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

// ── Pure helpers — extracted so they can be unit-tested without a Frame ──────

/// Returns true when the p95 duration warrants a warning highlight.
pub(crate) fn latency_p95_is_slow(p95_ms: i64) -> bool {
    p95_ms > 30_000
}

/// Returns true when a tool's total wall-clock time is heavy (>5 min).
pub(crate) fn tool_total_is_heavy(total_duration_ms: i64) -> bool {
    total_duration_ms > 300_000
}

fn render_approvals_stop_row(
    frame: &mut Frame,
    area: Rect,
    state: &UsageState,
    show_approvals: bool,
    show_stop: bool,
) {
    let halves = if show_approvals && show_stop {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0)])
            .split(area)
    };

    if show_approvals && show_stop {
        render_tool_approvals(frame, halves[0], state);
        render_stop_reasons(frame, halves[1], state);
    } else if show_approvals {
        render_tool_approvals(frame, halves[0], state);
    } else {
        render_stop_reasons(frame, halves[0], state);
    }
}

fn render_tool_approvals(frame: &mut Frame, area: Rect, state: &UsageState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Tool Approvals ");
    let Some(ref stats) = state.tool_approvals else {
        frame.render_widget(Paragraph::new("No data").block(block), area);
        return;
    };
    if stats.total == 0 {
        frame.render_widget(Paragraph::new("No tool decisions").block(block), area);
        return;
    }
    let auto_pct = stats.auto_accepted as f64 / stats.total as f64 * 100.0;
    let text = format!(
        "Auto-accept: {} ({:.1}%)  User: {}  Rejected: {}  Unknown: {}",
        stats.auto_accepted, auto_pct, stats.user_accepted, stats.rejected, stats.unknown,
    );
    frame.render_widget(Paragraph::new(text).block(block), area);
}

fn render_stop_reasons(frame: &mut Frame, area: Rect, state: &UsageState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Stop Reasons ");
    if state.stop_reasons.is_empty() {
        frame.render_widget(Paragraph::new("No data").block(block), area);
        return;
    }
    let total: usize = state
        .stop_reasons
        .iter()
        .filter(|r| r.reason != "(none)")
        .map(|r| r.count)
        .sum();
    let lines: Vec<Line> = state
        .stop_reasons
        .iter()
        .filter(|r| r.reason != "(none)")
        .map(|r| {
            let pct = if total > 0 {
                r.count as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            Line::from(format!("{}: {} ({:.1}%)", r.reason, r.count, pct))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_ctx_tool_errs_row(
    frame: &mut Frame,
    area: Rect,
    state: &UsageState,
    show_ctx: bool,
    show_tool_errs: bool,
) {
    let halves = if show_ctx && show_tool_errs {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0)])
            .split(area)
    };

    if show_ctx && show_tool_errs {
        render_context_split(frame, halves[0], state);
        render_tool_errors_table(frame, halves[1], state);
    } else if show_ctx {
        render_context_split(frame, halves[0], state);
    } else {
        render_tool_errors_table(frame, halves[0], state);
    }
}

fn render_context_split(frame: &mut Frame, area: Rect, state: &UsageState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Context Split ");
    if state.context_split.is_empty() {
        frame.render_widget(Paragraph::new("No context data").block(block), area);
        return;
    }
    let header = Row::new(vec![
        Cell::from("Context").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Calls").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Avg ms").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    let rows: Vec<Row> = state
        .context_split
        .iter()
        .map(|r| {
            Row::new(vec![
                Cell::from(r.context.clone()),
                Cell::from(r.calls.to_string()),
                Cell::from(if r.avg_ms > 0.0 {
                    format!("{}", r.avg_ms.round() as i64)
                } else {
                    "—".to_string()
                }),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Min(12),
            Constraint::Length(6),
            Constraint::Length(8),
        ],
    )
    .header(header)
    .block(block);
    frame.render_widget(table, area);
}

fn render_tool_errors_table(frame: &mut Frame, area: Rect, state: &UsageState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Top Tool Errors ");
    if state.tool_errors.is_empty() {
        frame.render_widget(Paragraph::new("No tool errors").block(block), area);
        return;
    }
    let header = Row::new(vec![
        Cell::from("Tool").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Error").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("N").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    let rows: Vec<Row> = state
        .tool_errors
        .iter()
        .map(|r| {
            Row::new(vec![
                Cell::from(truncate(&r.tool_name, 16)),
                Cell::from(truncate(&r.error_message, 40)),
                Cell::from(r.count.to_string()),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Min(12),
            Constraint::Min(20),
            Constraint::Length(5),
        ],
    )
    .header(header)
    .block(block);
    frame.render_widget(table, area);
}

fn render_hour_of_day(frame: &mut Frame, area: Rect, state: &UsageState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Activity by Hour of Day (UTC) ");
    let active: Vec<_> = state
        .hour_of_day
        .iter()
        .filter(|b| b.llm_calls > 0 || b.tool_calls > 0)
        .collect();
    if active.is_empty() {
        frame.render_widget(Paragraph::new("No hourly data").block(block), area);
        return;
    }
    let max_llm = active.iter().map(|b| b.llm_calls).max().unwrap_or(1).max(1);
    let header = Row::new(vec![
        Cell::from("Hour").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("LLM").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Bar").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Tools").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    let rows: Vec<Row> = active
        .iter()
        .map(|b| {
            let bar_len = (b.llm_calls * 15 / max_llm).max(if b.llm_calls > 0 { 1 } else { 0 });
            Row::new(vec![
                Cell::from(format!("{:02}:00", b.hour)),
                Cell::from(b.llm_calls.to_string()),
                Cell::from("█".repeat(bar_len)),
                Cell::from(b.tool_calls.to_string()),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Min(10),
            Constraint::Length(6),
        ],
    )
    .header(header)
    .block(block);
    frame.render_widget(table, area);
}

fn render_calls_series(frame: &mut Frame, area: Rect, state: &UsageState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Call Volume Trend ");
    if state.calls_series.is_empty() {
        frame.render_widget(Paragraph::new("No call trend data").block(block), area);
        return;
    }
    let max_reqs = state
        .calls_series
        .iter()
        .map(|p| p.requests)
        .max()
        .unwrap_or(1)
        .max(1);

    let header = Row::new(vec![
        Cell::from("Time").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Model").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Requests").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Bar").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    use chrono::{DateTime, Local, Utc};
    let rows: Vec<Row> = state
        .calls_series
        .iter()
        .map(|p| {
            let dt = DateTime::<Utc>::from_timestamp_nanos(p.timestamp);
            let time_str = dt.with_timezone(&Local).format("%m-%d %H:%M").to_string();
            let model = p.model.as_deref().unwrap_or("(unknown)").to_string();
            let bar_len = (p.requests * 15 / max_reqs).max(if p.requests > 0 { 1 } else { 0 });
            Row::new(vec![
                Cell::from(time_str),
                Cell::from(model),
                Cell::from(p.requests.to_string()),
                Cell::from("█".repeat(bar_len)).style(Style::default().fg(Color::Green)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Min(15),
            Constraint::Length(10),
            Constraint::Min(10),
        ],
    )
    .header(header)
    .block(block);
    frame.render_widget(table, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use otelite_core::api::{
        GenAiCapabilityReport, GenAiCapabilityResponse, GenAiCorrelationProvenance,
        GenAiMetricCapability, LatencyPercentilePoint, LatencyPercentileSeries,
        LatencyPercentilesResponse, LatencyStats,
    };

    fn make_latency(p95_ms: i64, ratio_p95: Option<f64>) -> LatencyStats {
        LatencyStats {
            model: Some("test-model".to_string()),
            count: 10,
            avg_ms: (p95_ms / 2) as f64,
            p50_ms: p95_ms / 2,
            p95_ms,
            p99_ms: p95_ms,
            ttft_p50_ms: None,
            ttft_p95_ms: None,
            ttft_p99_ms: None,
            ttft_count: 0,
            ttft_invalid_count: 0,
            ttft_degenerate_count: 0,
            ttft_degenerate: false,
            derived_tokens_per_sec_p10: None,
            derived_tokens_per_sec_p50: None,
            derived_tokens_per_sec_p90: None,
            derived_tokens_per_sec_p95: None,
            derived_tokens_per_sec_p99: None,
            throughput_sample_count: 0,
            input_tokens_p50: None,
            input_tokens_p95: None,
            input_tokens_p99: None,
            output_input_ratio_p50: None,
            output_input_ratio_p95: ratio_p95,
            output_input_ratio_p99: None,
        }
    }

    #[test]
    fn test_latency_p95_slow_above_30s() {
        let s = make_latency(31_000, None);
        assert!(latency_p95_is_slow(s.p95_ms));
    }

    #[test]
    fn test_latency_p95_slow_at_30s_not_flagged() {
        let s = make_latency(30_000, None);
        assert!(!latency_p95_is_slow(s.p95_ms));
    }

    #[test]
    fn test_tool_heavy_above_5min() {
        let total_ms = 301_000i64;
        assert!(tool_total_is_heavy(total_ms));
    }

    #[test]
    fn test_tool_heavy_at_5min_not_flagged() {
        let total_ms = 300_000i64;
        assert!(!tool_total_is_heavy(total_ms));
    }

    #[test]
    fn test_tool_light_not_heavy() {
        let total_ms = 60_000i64;
        assert!(!tool_total_is_heavy(total_ms));
    }

    #[test]
    fn test_fmt_tokens_boundaries() {
        assert_eq!(fmt_tokens(0), "0");
        assert_eq!(fmt_tokens(999), "999");
        assert_eq!(fmt_tokens(1_000), "1.0k");
        assert_eq!(fmt_tokens(1_000_000), "1.0M");
    }

    #[test]
    fn test_truncate_long_string() {
        let s = "abcdefghijklmnopqrstuvwxyz";
        let t = truncate(s, 10);
        // truncate keeps max-1 bytes then appends '…' (3 UTF-8 bytes) → max+2 bytes total
        assert!(t.len() <= 12, "len was {}", t.len());
        assert!(t.ends_with('…'));
    }

    #[test]
    fn test_truncate_short_string_unchanged() {
        let s = "short";
        assert_eq!(truncate(s, 20), "short");
    }

    fn percentile_point(
        ts: i64,
        count: u64,
        n_star: u64,
        tps: Option<(f64, f64, f64)>,
    ) -> LatencyPercentilePoint {
        LatencyPercentilePoint {
            ts,
            end_ts: ts + 86_400_000_000_000,
            p10_ms: None,
            p50_ms: Some(100.0),
            p90_ms: None,
            p95_ms: Some(200.0),
            p99_ms: None,
            count,
            throughput_p10_tok_s: tps.map(|t| t.0),
            throughput_p50_tok_s: tps.map(|t| t.1),
            throughput_p90_tok_s: tps.map(|t| t.2),
            throughput_sample_count: n_star,
        }
    }

    fn percentile_resp(
        models: Vec<(&str, Vec<LatencyPercentilePoint>)>,
    ) -> LatencyPercentilesResponse {
        let mut series = LatencyPercentileSeries::default();
        for (m, pts) in models {
            series.models.insert(m.to_string(), pts);
        }
        LatencyPercentilesResponse {
            metrics: std::collections::BTreeMap::from([("duration".to_string(), series)]),
            filters_applied: vec![],
        }
    }

    #[test]
    fn test_daily_rows_values_wording_and_empty_day_omission() {
        // 2026-08-23T23:00:00Z == 2026-08-24 00:00 Europe/London (BST).
        let d0 = 1_787_529_600_i64 * 1_000_000_000; // 2026-08-24T00:00Z
                                                    // (verified: epoch 1787529600 = 2026-08-24 00:00 UTC)
        let d1 = d0 + 86_400_000_000_000; // 2026-08-25T00:00Z
        let d2 = d1 + 86_400_000_000_000; // 2026-08-26T00:00Z
        let resp = percentile_resp(vec![
            (
                "alpha",
                vec![
                    // full sample: 12 eligible calls
                    percentile_point(d0, 12, 12, Some((10.0, 20.0, 30.0))),
                    // weak sample: 7 eligible calls -> "7†"
                    percentile_point(d1, 7, 7, Some((5.0, 6.0, 7.0))),
                    // empty day -> omitted entirely
                    percentile_point(d2, 0, 0, None),
                ],
            ),
            (
                "beta",
                // calls present but none throughput-eligible -> "—" / "—"
                vec![percentile_point(d0, 3, 0, None)],
            ),
        ]);
        let rows = daily_throughput_rows(&resp, "UTC");
        assert_eq!(rows.len(), 3, "empty day must be omitted");
        // Sorted day-first, then model: alpha 08-24, beta 08-24, alpha 08-25.
        assert_eq!(rows[0].day, "2026-08-24");
        assert_eq!(rows[0].model, "alpha");
        assert_eq!(rows[0].calls, 12);
        assert_eq!(rows[0].n_star, "12");
        assert_eq!(rows[0].tps, "10 / 20 / 30");
        assert_eq!(rows[1].day, "2026-08-24");
        assert_eq!(rows[1].model, "beta");
        assert_eq!(rows[1].n_star, "—");
        assert_eq!(rows[1].tps, "—");
        assert_eq!(rows[2].day, "2026-08-25");
        assert_eq!(rows[2].model, "alpha");
        assert_eq!(rows[2].n_star, "7†");
        assert_eq!(rows[2].tps, "5 / 6 / 7");
    }

    #[test]
    fn test_daily_rows_local_timezone_date_label() {
        // Bucket aligned to London local midnight 2026-08-24 = 2026-08-23T23:00Z.
        let london_midnight = 1_787_529_600_i64 * 1_000_000_000 - 3_600_000_000_000;
        let resp = percentile_resp(vec![(
            "alpha",
            vec![percentile_point(
                london_midnight,
                12,
                12,
                Some((1.0, 2.0, 3.0)),
            )],
        )]);
        let rows = daily_throughput_rows(&resp, "Europe/London");
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].day, "2026-08-24",
            "label must be the local calendar day"
        );
    }

    #[test]
    fn test_daily_rows_no_metric_is_no_data() {
        let resp = LatencyPercentilesResponse {
            metrics: std::collections::BTreeMap::new(),
            filters_applied: vec![],
        };
        assert!(daily_throughput_rows(&resp, "UTC").is_empty());
    }

    #[test]
    fn test_daily_rows_partial_triple_is_missing() {
        // One missing percentile leg renders as missing, not a partial triple.
        let resp = percentile_resp(vec![(
            "alpha",
            vec![LatencyPercentilePoint {
                throughput_p90_tok_s: None,
                ..percentile_point(1_787_529_600_000_000_000, 12, 12, Some((1.0, 2.0, 3.0)))
            }],
        )]);
        let rows = daily_throughput_rows(&resp, "UTC");
        assert_eq!(rows[0].tps, "—");
    }

    /// (eligible, observed, valid, availability, quality, derivation).
    fn cap(spec: (usize, usize, usize, &str, &str, &str)) -> GenAiMetricCapability {
        let (eligible, observed, valid, availability, quality, derivation) = spec;
        GenAiMetricCapability {
            eligible_count: eligible,
            observed_count: observed,
            valid_count: valid,
            invalid_count: observed.saturating_sub(valid),
            availability: availability.to_string(),
            quality: quality.to_string(),
            derivation: derivation.to_string(),
            source_attributes: Default::default(),
        }
    }

    fn cap_resp(reports: Vec<GenAiCapabilityReport>) -> GenAiCapabilityResponse {
        let canonical_span_count: usize = reports.iter().map(|r| r.request_count).sum();
        GenAiCapabilityResponse {
            reports,
            canonical_span_count,
            duplicate_span_count: 0,
            truncated: false,
            filters_applied: vec![],
        }
    }

    fn cap_report(
        provider: Option<&str>,
        model: Option<&str>,
        emitter: &str,
        count: usize,
        ttft: GenAiMetricCapability,
    ) -> GenAiCapabilityReport {
        let absent = cap((count, 0, 0, "absent", "not_assessed", "unavailable"));
        GenAiCapabilityReport {
            provider: provider.map(str::to_string),
            model: model.map(str::to_string),
            emitter_fingerprint: "fp".into(),
            emitter: emitter.to_string(),
            adapter_rule: "rule".into(),
            request_count: count,
            input_tokens: cap((count, count, count, "available", "reliable", "native")),
            output_tokens: cap((count, count, count, "available", "reliable", "native")),
            cache_creation_tokens: absent.clone(),
            cache_read_tokens: absent,
            ttft,
            correlation: GenAiCorrelationProvenance {
                rule: "none".into(),
                matched_count: 0,
                unmatched_count: 0,
                rejected_count: 0,
                ambiguous_count: 0,
            },
        }
    }

    #[test]
    fn test_capability_rows_vocabulary_and_identity() {
        let full = cap((5, 5, 5, "available", "reliable", "native"));
        let invalid = cap((4, 3, 1, "sparse", "invalid", "native"));
        let degenerate = cap((12, 12, 12, "available", "degenerate", "native"));
        let absent = cap((3, 0, 0, "absent", "not_assessed", "unavailable"));
        let resp = cap_resp(vec![
            cap_report(Some("openai"), Some("gpt-4o"), "standard_otel", 5, full),
            cap_report(
                Some("openai"),
                Some("gpt-4o-mini"),
                "standard_otel",
                4,
                invalid,
            ),
            cap_report(
                Some("anthropic"),
                Some("claude-sonnet-4-5"),
                "standard_otel",
                12,
                degenerate,
            ),
            cap_report(None, Some("claude-opus-4-6"), "claude_code", 3, absent),
        ]);
        let rows = capability_rows(&resp);
        assert_eq!(rows.len(), 4);
        // identity is the provider/model composite (bare model without provider)
        assert_eq!(rows[0].identity, "openai/gpt-4o");
        assert_eq!(rows[3].identity, "claude-opus-4-6");
        assert_eq!(rows[0].requests, 5);
        // native derivation is suppressed in the cell
        assert_eq!(rows[0].input, "available/reliable (5/5 obs)");
        assert_eq!(rows[1].ttft, "sparse/invalid (1/3 obs)");
        assert_eq!(rows[2].ttft, "available/degenerate (12/12 obs)");
        // absent keeps the derivation and shows the eligibility count
        assert_eq!(rows[3].ttft, "absent/not_assessed/unavailable (0/3 elig)");
    }

    #[test]
    fn test_capability_rows_empty() {
        assert!(capability_rows(&cap_resp(vec![])).is_empty());
    }
}
