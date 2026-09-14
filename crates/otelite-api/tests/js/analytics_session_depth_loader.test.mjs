// Loader test for the Session Depth vs Cost report (issue #180):
// _loadSessionDepthSection must render one table row per (tool, bucket)
// in server order, format the cost cells (dash when a bucket has no
// priced sessions), and handle the empty state and the API error path.
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

test('session_depth loader renders one row per (tool, bucket) with cost formatting', async () => {
    const view = makeView();
    const calls = [];
    view.api = {
        getSessionDepthCost: async (p) => {
            calls.push(p);
            return {
                rows: [
                    {
                        tool: 'opencode', bucket: '1-5', sessions: 2,
                        avg_turns: 2.5, median_cost_usd: 0.21, p95_cost_usd: 0.21,
                    },
                    {
                        tool: 'opencode', bucket: '6-15', sessions: 1,
                        avg_turns: 6.0, median_cost_usd: null, p95_cost_usd: null,
                    },
                ],
            };
        },
    };
    await view._loadSessionDepthSection();
    assert.equal(calls.length, 1);
    assert.ok(view.loadedSections.has('session_depth'));
    const body = view._bodies['session_depth'];
    // Coverage hint + table header.
    assert.match(body, /fewer than 2 turns are excluded/);
    assert.match(body, /<th>Median cost<\/th><th>p95 cost<\/th>/);
    // Priced row: formatted dollar cells.
    assert.match(body, /<td>opencode<\/td><td>1-5<\/td><td>2<\/td><td>2\.5<\/td><td>\$0\.21<\/td><td>\$0\.21<\/td>/);
    // Unpriced bucket: dash cells, counts still shown.
    assert.match(body, /<td>opencode<\/td><td>6-15<\/td><td>1<\/td><td>6\.0<\/td><td>—<\/td><td>—<\/td>/);
});

test('session_depth loader renders the empty state', async () => {
    const view = makeView();
    view.api = {
        getSessionDepthCost: async () => ({ rows: [] }),
    };
    await view._loadSessionDepthSection();
    assert.ok(view.loadedSections.has('session_depth'));
    const body = view._bodies['session_depth'];
    assert.match(body, /No multi-turn sessions in this window/);
    assert.doesNotMatch(body, /<table/);
});

test('session_depth loader surfaces API errors via the section error path', async () => {
    const view = makeView();
    view.api = {
        getSessionDepthCost: async () => {
            throw new Error('boom-session-depth');
        },
    };
    await view._loadSessionDepthSection();
    assert.equal(view._errors.length, 1);
    assert.equal(view._errors[0][0], 'session_depth');
    assert.ok(!view.loadedSections.has('session_depth'));
});
