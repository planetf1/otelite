// Loader test for the Human Response Latency report (issue #171):
// _loadHumanResponseLatencySection must render the per-tool percentile
// table and the hour-of-day heatmap (24 UTC columns, shading by p50,
// blank cells for hours without gaps) — plus the empty state and the
// API error path.
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
    by_tool: [
        { tool: 'pi', p50_ms: 8200, p90_ms: 25000, p95_ms: 41200, n: 42 },
    ],
    by_hour: [
        { hour: 9, tool: 'pi', p50_ms: 12000, n: 12 },
        { hour: 14, tool: 'pi', p50_ms: 5000, n: 10 },
    ],
};

test('human_response_latency renders the percentile table and the heatmap', async () => {
    const view = makeView();
    view.api = {
        getHumanResponseLatency: async () => DATA,
    };
    await view._loadHumanResponseLatencySection();
    assert.ok(view.loadedSections.has('human_response_latency'));
    const body = view._bodies['human_response_latency'];
    // Hint + percentile table.
    assert.match(body, /gaps over 30 min are excluded/);
    assert.match(body, /<th>Tool<\/th><th>p50<\/th><th>p90<\/th><th>p95<\/th><th>Gaps<\/th>/);
    assert.match(body, /<td>pi<\/td><td>8\.2 s<\/td><td>25\.0 s<\/td><td>41\.2 s<\/td><td>42<\/td>/);
    // Heatmap: 24 hour columns, shaded cells for 09h and 14h with
    // tooltips, blank cells elsewhere.
    assert.match(body, /Hour of day \(UTC\)/);
    assert.match(body, /title="pi 09:00 UTC — p50 12\.0 s, 12 gap\(s\)"/);
    assert.match(body, /title="pi 14:00 UTC — p50 5\.0 s, 10 gap\(s\)"/);
    const cells = (body.match(/human-latency-cell/g) || []).length;
    assert.equal(cells, 2);
    const blank = (body.match(/<td><\/td>/g) || []).length;
    assert.equal(blank, 22);
    // The slower hour (12 s) shades deeper than the faster one (5 s).
    const m09 = body.match(/rgba\(220, 60, 40, ([0-9.]+)\)" title="pi 09:00/);
    const m14 = body.match(/rgba\(220, 60, 40, ([0-9.]+)\)" title="pi 14:00/);
    assert.ok(parseFloat(m09[1]) > parseFloat(m14[1]), 'slower hour must shade deeper');
});

test('human_response_latency renders the empty state', async () => {
    const view = makeView();
    view.api = {
        getHumanResponseLatency: async () => ({ by_tool: [], by_hour: [] }),
    };
    await view._loadHumanResponseLatencySection();
    assert.ok(view.loadedSections.has('human_response_latency'));
    assert.match(view._bodies['human_response_latency'], /No multi-turn sessions in this window/);
    assert.doesNotMatch(view._bodies['human_response_latency'], /<table/);
});

test('human_response_latency surfaces API errors via the section error path', async () => {
    const view = makeView();
    view.api = {
        getHumanResponseLatency: async () => {
            throw new Error('boom-human-latency');
        },
    };
    await view._loadHumanResponseLatencySection();
    assert.equal(view._errors.length, 1);
    assert.equal(view._errors[0][0], 'human_response_latency');
    assert.ok(!view.loadedSections.has('human_response_latency'));
});
