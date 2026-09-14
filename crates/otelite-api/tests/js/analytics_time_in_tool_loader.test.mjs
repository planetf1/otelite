// Loader test for the Time in Tool report (issue #172):
// _loadTimeInToolSection must render the window summary line (tools in
// minutes-desc order), the stacked per-day SVG chart with per-segment
// tooltips, and the per-tool summary table — plus the empty state and
// the API error path.
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

const DATA = {
    rows: [
        { tool: 'opencode', date: '2026-01-01', active_minutes: 30, sessions: 2, avg_session_minutes: 15.0 },
        { tool: 'opencode', date: '2026-01-02', active_minutes: 10, sessions: 1, avg_session_minutes: 10.0 },
        { tool: 'claude_code', date: '2026-01-01', active_minutes: 45, sessions: 1, avg_session_minutes: 45.0 },
    ],
    tools: ['claude_code', 'opencode'],
};

test('time_in_tool loader renders summary line, stacked chart, and tool table', async () => {
    const view = makeView();
    const calls = [];
    view.api = {
        getTimeInTool: async (p) => {
            calls.push(p);
            return DATA;
        },
    };
    await view._loadTimeInToolSection();
    assert.equal(calls.length, 1);
    assert.ok(view.loadedSections.has('time_in_tool'));
    const body = view._bodies['time_in_tool'];

    // Coverage hint.
    assert.match(body, /longer gaps are treated as context switches/);
    // Window summary: minutes-desc order (claude_code 45, opencode 30+10).
    assert.match(body, /<strong>In this window:<\/strong> claude_code 45 min · opencode 40 min/);
    // Chart: total header (45 + 30 + 10 = 85) and stacked bars with
    // per-segment tooltips.
    assert.match(body, /Active minutes per day — 85 min total across 2 days/);
    assert.match(body, /<rect class="time-chart-bar"/);
    assert.match(body, /2026-01-01\nopencode: 30 min/);
    assert.match(body, /2026-01-02\nopencode: 10 min/);
    assert.match(body, /2026-01-01\nclaude_code: 45 min/);
    // Legend mentions both tools.
    assert.match(body, /claude_code/);
    // Per-tool table: opencode 40 min across 3 sessions (avg 13.3),
    // claude_code 45 min across 1 session.
    assert.match(body, /<td>claude_code<\/td><td>45 min<\/td><td>1<\/td><td>45\.0<\/td>/);
    assert.match(body, /<td>opencode<\/td><td>40 min<\/td><td>3<\/td><td>13\.3<\/td>/);
});

test('time_in_tool loader renders the empty state', async () => {
    const view = makeView();
    view.api = {
        getTimeInTool: async () => ({ rows: [], tools: [] }),
    };
    await view._loadTimeInToolSection();
    assert.ok(view.loadedSections.has('time_in_tool'));
    const body = view._bodies['time_in_tool'];
    assert.match(body, /No session data in this window/);
    assert.doesNotMatch(body, /<svg/);
    assert.doesNotMatch(body, /In this window/);
});

test('time_in_tool loader surfaces API errors via the section error path', async () => {
    const view = makeView();
    view.api = {
        getTimeInTool: async () => {
            throw new Error('boom-time-in-tool');
        },
    };
    await view._loadTimeInToolSection();
    assert.equal(view._errors.length, 1);
    assert.equal(view._errors[0][0], 'time_in_tool');
    assert.ok(!view.loadedSections.has('time_in_tool'));
});
