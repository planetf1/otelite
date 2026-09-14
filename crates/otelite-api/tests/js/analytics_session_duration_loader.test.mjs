// Loader test for the Session Duration report (issue #183):
// _loadSessionDurationSection must render the stats line (sessions,
// median, p95, mean), the per-bucket histogram with tooltips, and the
// per-tool × bucket table (totals desc, dash for empty cells) — plus
// the empty state and the API error path.
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

// 6 sessions: opencode 3 (<5m ×2, 5-15m ×1), claude_code 2 (30-60m ×1,
// >60m ×1), codex 1 (<5m ×1).
const DATA = {
    buckets: [
        { tool: 'opencode', bucket: '<5m', count: 2, pct: 33.3 },
        { tool: 'opencode', bucket: '5-15m', count: 1, pct: 16.7 },
        { tool: 'claude_code', bucket: '30-60m', count: 1, pct: 16.7 },
        { tool: 'claude_code', bucket: '>60m', count: 1, pct: 16.7 },
        { tool: 'codex', bucket: '<5m', count: 1, pct: 16.7 },
    ],
    stats: { sessions: 6, median_minutes: 8, p95_minutes: 75, mean_minutes: 12 },
};

test('session_duration loader renders stats line, histogram, and tool table', async () => {
    const view = makeView();
    const calls = [];
    view.api = {
        getSessionDuration: async (p) => {
            calls.push(p);
            return DATA;
        },
    };
    await view._loadSessionDurationSection();
    assert.equal(calls.length, 1);
    assert.ok(view.loadedSections.has('session_duration'));
    const body = view._bodies['session_duration'];
    // Source explanation + stats line.
    assert.match(body, /opencode uses its own duration metric/);
    assert.match(body, /<strong>6<\/strong> session\(s\) · median <strong>8 min<\/strong> · p95 <strong>75 min<\/strong> · mean 12 min/);
    // Histogram: 5 bucket bars on a fixed scale, tooltip per bar
    // (<5m holds 2 opencode + 1 codex = 3 of 6 sessions = 50%; empty
    // buckets render zero-height bars too).
    assert.match(body, /Sessions by length/);
    assert.match(body, /<title><5m\n3 session\(s\) — 50\.0%<\/title>/);
    assert.match(body, /<title>15-30m\n0 session\(s\) — 0\.0%<\/title>/);
    // Bucket axis + table headers are HTML-escaped (<5m, >60m).
    assert.match(body, /<div class="session-duration-axis">/);
    assert.match(body, /<span>&lt;5m<\/span><span>5-15m<\/span>/);
    assert.match(body, /<th>Tool<\/th><th>&lt;5m<\/th><th>5-15m<\/th><th>15-30m<\/th><th>30-60m<\/th><th>&gt;60m<\/th><th>Total<\/th>/);
    // Per-tool table: totals desc (opencode 3, claude_code 2, codex 1),
    // empty cells dashed.
    assert.match(body, /<td>opencode<\/td><td>2<\/td><td>1<\/td><td>—<\/td><td>—<\/td><td>—<\/td><td>3<\/td>/);
    assert.match(body, /<td>claude_code<\/td><td>—<\/td><td>—<\/td><td>—<\/td><td>1<\/td><td>1<\/td><td>2<\/td>/);
    assert.match(body, /<td>codex<\/td><td>1<\/td><td>—<\/td><td>—<\/td><td>—<\/td><td>—<\/td><td>1<\/td>/);
});

test('session_duration loader renders the empty state', async () => {
    const view = makeView();
    view.api = {
        getSessionDuration: async () => ({ buckets: [], stats: { sessions: 0, median_minutes: 0, p95_minutes: 0, mean_minutes: 0 } }),
    };
    await view._loadSessionDurationSection();
    assert.ok(view.loadedSections.has('session_duration'));
    const body = view._bodies['session_duration'];
    assert.match(body, /No session duration data in this window/);
    assert.doesNotMatch(body, /<svg/);
});

test('session_duration loader surfaces API errors via the section error path', async () => {
    const view = makeView();
    view.api = {
        getSessionDuration: async () => {
            throw new Error('boom-session-duration');
        },
    };
    await view._loadSessionDurationSection();
    assert.equal(view._errors.length, 1);
    assert.equal(view._errors[0][0], 'session_duration');
    assert.ok(!view.loadedSections.has('session_duration'));
});
