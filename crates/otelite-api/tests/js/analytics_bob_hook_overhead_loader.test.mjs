// Loader test for the Bob Hook Overhead report (issue #167):
// _loadBobHookOverheadSection must render the empty state with the
// "Bob does not emit hook telemetry yet" note (not a bare no-data
// message), and render the invocations table when rows exist.
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

test('bob_hook_overhead empty state explains the missing upstream telemetry', async () => {
    const view = makeView();
    const calls = [];
    view.api = {
        getBobHookOverhead: async (p) => {
            calls.push(p);
            return { rows: [], grand_total_ms: 0 };
        },
    };
    await view._loadBobHookOverheadSection();
    assert.equal(calls.length, 1);
    assert.ok(view.loadedSections.has('bob_hook_overhead'));
    const body = view._bodies['bob_hook_overhead'];
    // Not a bare "no data" — it names the upstream dependency.
    assert.match(body, /Bob does not emit hook telemetry yet/);
    assert.doesNotMatch(body, /<table/);
});

test('bob_hook_overhead renders the invocations table when rows exist', async () => {
    const view = makeView();
    view.api = {
        getBobHookOverhead: async () => ({
            rows: [
                { event: 'Stop', count: 5, total_ms: 2500, avg_ms: 500 },
                { event: 'PrePrompt', count: 2, total_ms: 400, avg_ms: 200 },
            ],
            grand_total_ms: 2900,
        }),
    };
    await view._loadBobHookOverheadSection();
    assert.ok(view.loadedSections.has('bob_hook_overhead'));
    const body = view._bodies['bob_hook_overhead'];
    assert.match(body, /Grand total hook time: <strong>3 s<\/strong>/);
    assert.match(body, /<th>Hook event<\/th><th>Invocations<\/th>/);
    assert.match(body, /<td>Stop<\/td>\s*<td>5<\/td>\s*<td>2,500 ms<\/td>\s*<td>500 ms<\/td>/);
    assert.match(body, /<td>PrePrompt<\/td>\s*<td>2<\/td>\s*<td>400 ms<\/td>\s*<td>200 ms<\/td>/);
});

test('bob_hook_overhead loader surfaces API errors via the section error path', async () => {
    const view = makeView();
    view.api = {
        getBobHookOverhead: async () => {
            throw new Error('boom-bob-hook-overhead');
        },
    };
    await view._loadBobHookOverheadSection();
    assert.equal(view._errors.length, 1);
    assert.equal(view._errors[0][0], 'bob_hook_overhead');
    assert.ok(!view.loadedSections.has('bob_hook_overhead'));
});
