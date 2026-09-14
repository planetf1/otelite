// Loader test for the Codex Busy/Idle section's idle-ratio trend tab
// (issue #181): _codexTurnsTrendHtml must render the per-day chart and
// table (bars on a fixed 0-100% scale), _loadCodexTurnsSection must
// render both tabs with the breakdown active by default, and the
// section must keep its empty state and error path.
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
    view.codexTurnsTab = 'breakdown';
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

const TREND_ROWS = [
    { date: '2026-01-01', avg_idle_ratio: 0.625, avg_busy_ms: 200, avg_idle_ms: 300, span_count: 2 },
    { date: '2026-01-02', avg_idle_ratio: 0.1, avg_busy_ms: 900, avg_idle_ms: 100, span_count: 1 },
];

test('trend tab renders chart bars on a 0-100% scale and the daily table', () => {
    const view = makeView();
    const html = view._codexTurnsTrendHtml(TREND_ROWS);
    // Latest-day headline.
    assert.match(html, /Idle ratio per day — 10\.0% on 2026-01-02/);
    // Two bars; the 62.5% day is a 62.5-unit bar on the 100-unit scale.
    assert.match(html, /<rect class="cost-chart-bar" x="0\.000" y="37\.500" width="49\.750" height="62\.500">/);
    assert.match(html, /<title>2026-01-01\nidle 62\.5%\navg busy 200 ms · idle 300 ms\n2 spans<\/title>/);
    // Daily table is newest-first.
    assert.match(html, /<th>Day<\/th><th>Idle %<\/th><th>Avg busy<\/th><th>Avg idle<\/th><th>Spans<\/th>/);
    assert.match(html, /<td>2026-01-02<\/td><td>10\.0%<\/td><td>900 ms<\/td><td>100 ms<\/td><td>1<\/td>/);
    assert.match(html, /<td>2026-01-01<\/td><td>62\.5%<\/td><td>200 ms<\/td><td>300 ms<\/td><td>2<\/td>/);
});

test('trend tab renders the empty state when no spans carry both attributes', () => {
    const view = makeView();
    const html = view._codexTurnsTrendHtml([]);
    assert.match(html, /No Codex idle-ratio data in this window/);
    assert.doesNotMatch(html, /<svg/);
});

test('section loads both tabs with the breakdown active by default', async () => {
    const view = makeView();
    view.api = {
        getCodexTurnBreakdown: async () => ({
            rows: [
                {
                    model: 'gpt-4o', project: 'myproject', turn_count: 5,
                    avg_duration_ms: 2000, avg_busy_ms: 1000, avg_idle_ms: 500, busy_ratio: 0.5,
                },
            ],
        }),
        getCodexIdleRatio: async () => ({ rows: TREND_ROWS }),
    };
    await view._loadCodexTurnsSection();
    assert.equal(view._errors.length, 0);
    assert.ok(view.loadedSections.has('codex_turns'));
    const body = view._bodies['codex_turns'];
    // Tab bar with both tabs; breakdown active by default.
    assert.match(body, /data-codex-tab="breakdown"/);
    assert.match(body, /data-codex-tab="trend"/);
    assert.match(body, /top-n-tab active" data-codex-tab="breakdown"/);
    // Breakdown content is rendered first.
    assert.match(body, /<td>gpt-4o<\/td><td>myproject<\/td>/);
    assert.doesNotMatch(body, /Idle ratio per day/);
});

test('section keeps the empty state when both endpoints are empty', async () => {
    const view = makeView();
    view.api = {
        getCodexTurnBreakdown: async () => ({ rows: [] }),
        getCodexIdleRatio: async () => ({ rows: [] }),
    };
    await view._loadCodexTurnsSection();
    assert.ok(view.loadedSections.has('codex_turns'));
    assert.match(view._bodies['codex_turns'], /No Codex turn data in this window/);
    assert.doesNotMatch(view._bodies['codex_turns'], /top-n-tabs/);
});

test('section surfaces API errors via the section error path', async () => {
    const view = makeView();
    view.api = {
        getCodexTurnBreakdown: async () => {
            throw new Error('boom-codex-turns');
        },
        getCodexIdleRatio: async () => ({ rows: [] }),
    };
    await view._loadCodexTurnsSection();
    assert.equal(view._errors.length, 1);
    assert.equal(view._errors[0][0], 'codex_turns');
    assert.ok(!view.loadedSections.has('codex_turns'));
});
