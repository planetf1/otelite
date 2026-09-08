// Regression tests for the cross-tool report loaders (issue #189):
// _loadCrossToolTtftSection, _loadHookOverheadSection and
// _loadReasoningShareSection once called a nonexistent `this._timeParams()`
// and failed with "Failed to load: this._timeParams is not a function" on
// every expand. They must query the API with the standard time window
// ({start_time, end_time} in ns, as produced by _baseParams).
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
    view._setSectionBody = (id, html) => {
        view._bodies[id] = html;
    };
    return view;
}

const expectedParams = {
    start_time: new Date('2026-09-06T00:00:00Z').getTime() * 1_000_000,
    end_time: new Date('2026-09-07T00:00:00Z').getTime() * 1_000_000,
};

test('cross_tool_ttft loader queries the API with the standard time window', async () => {
    const view = makeView();
    const calls = [];
    view.api = {
        getCrossToolTtft: async (p) => {
            calls.push(p);
            return { rows: [] };
        },
    };
    await view._loadCrossToolTtftSection();
    assert.equal(calls.length, 1, 'getCrossToolTtft not called — loader threw?');
    assert.deepEqual(calls[0], expectedParams);
    assert.ok(view.loadedSections.has('cross_tool_ttft'));
});

test('hook_overhead loader queries the API with the standard time window', async () => {
    const view = makeView();
    const calls = [];
    view.api = {
        getHookOverhead: async (p) => {
            calls.push(p);
            return { rows: [] };
        },
    };
    await view._loadHookOverheadSection();
    assert.equal(calls.length, 1, 'getHookOverhead not called — loader threw?');
    assert.deepEqual(calls[0], expectedParams);
    assert.ok(view.loadedSections.has('hook_overhead'));
});

test('reasoning_share loader queries the API with the standard time window', async () => {
    const view = makeView();
    const calls = [];
    view.api = {
        getReasoningShare: async (p) => {
            calls.push(p);
            return { models: [], effort: [] };
        },
    };
    await view._loadReasoningShareSection();
    assert.equal(calls.length, 1, 'getReasoningShare not called — loader threw?');
    assert.deepEqual(calls[0], expectedParams);
    assert.ok(view.loadedSections.has('reasoning_share'));
});
