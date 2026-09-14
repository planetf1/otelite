// Loader test for the Tool Switch Overhead report (issue #166):
// _loadToolSwitchOverheadSection must render the empty state when no
// switches were detected, and otherwise render the summary line plus
// the per-transition table with signed TTFT deltas (dash when
// unmeasured).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const src = readFileSync(join(here, '../../static/js/analytics.js'), 'utf8');
const moduleObj = { exports: {} };
new Function('module', 'exports', 'window', 'parseHashQuery', 'parseHashWindow', src)(
    moduleObj,
    moduleObj.exports,
    undefined,
    () => ({}),
    () => null,
);
const { AnalyticsView } = moduleObj.exports;

function makeView() {
    const view = Object.create(AnalyticsView.prototype);
    view.loadedSections = new Set();
    view.trStart = new Date('2026-09-06T00:00:00Z');
    view.trEnd = new Date('2026-09-07T00:00:00Z');
    view._setSectionLoading = () => {};
    view._bodies = {};
    view._errors = [];
    view._setSectionBody = (id, html) => {
        view._bodies[id] = html;
    };
    view._setSectionError = (id, err) => {
        view._errors.push([id, err]);
    };
    return view;
}

test('tool_switch_overhead renders summary line and transition table', async () => {
    const view = makeView();
    const calls = [];
    view.api = {
        getToolSwitchOverhead: async (p) => {
            calls.push(p);
            return {
                switches: 4,
                avg_gap_ms: 1234,
                avg_ttft_cold_ms: 800,
                avg_ttft_warm_ms: 100,
                overhead_ratio: 8.0,
                by_transition: [
                    { from: 'opencode', to: 'codex', count: 3, avg_gap_ms: 1500, avg_ttft_delta_ms: 200 },
                    { from: 'codex', to: 'opencode', count: 1, avg_gap_ms: 600, avg_ttft_delta_ms: null },
                ],
            };
        },
    };
    await view._loadToolSwitchOverheadSection();
    assert.equal(calls.length, 1);
    assert.ok(view.loadedSections.has('tool_switch_overhead'));
    const body = view._bodies['tool_switch_overhead'];
    // Summary line: switch count, avg gap, cold/warm TTFT, ratio.
    assert.match(body, /<strong>4<\/strong> switch\(es\) — avg gap <strong>1,234 ms<\/strong>/);
    assert.match(body, /TTFT cold 800 ms · warm 100 ms · ratio 8.00/);
    // Table header.
    assert.match(body, /<th>Transition<\/th><th>Switches<\/th><th>Avg gap<\/th><th>Avg TTFT delta<\/th>/);
    // Rows in server order; measured delta signed, unmeasured a dash.
    assert.match(body, /<td>opencode → codex<\/td>\s*<td>3<\/td>\s*<td>1,500 ms<\/td>\s*<td>\+200 ms<\/td>/);
    assert.match(body, /<td>codex → opencode<\/td>\s*<td>1<\/td>\s*<td>600 ms<\/td>\s*<td>—<\/td>/);
});

test('tool_switch_overhead renders the empty state with no table', async () => {
    const view = makeView();
    view.api = {
        getToolSwitchOverhead: async () => ({
            switches: 0,
            avg_gap_ms: 0,
            avg_ttft_cold_ms: null,
            avg_ttft_warm_ms: null,
            overhead_ratio: null,
            by_transition: [],
        }),
    };
    await view._loadToolSwitchOverheadSection();
    assert.ok(view.loadedSections.has('tool_switch_overhead'));
    const body = view._bodies['tool_switch_overhead'];
    assert.match(body, /No tool switches detected in this window/);
    assert.doesNotMatch(body, /<table/);
});

test('tool_switch_overhead loader surfaces API errors via the section error path', async () => {
    const view = makeView();
    view.api = {
        getToolSwitchOverhead: async () => {
            throw new Error('boom-tool-switch');
        },
    };
    await view._loadToolSwitchOverheadSection();
    assert.equal(view._errors.length, 1);
    assert.equal(view._errors[0][0], 'tool_switch_overhead');
    assert.ok(!view.loadedSections.has('tool_switch_overhead'));
});
