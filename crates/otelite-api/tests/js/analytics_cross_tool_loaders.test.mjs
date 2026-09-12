// Regression tests for the cross-tool report loaders (issue #189):
// _loadTtftSection (merged from _loadCodexTtftSection and
// _loadCrossToolTtftSection by #220), _loadHookOverheadSection and
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

test('ttft loader queries both endpoints with the standard time window', async () => {
    const view = makeView();
    const calls = [];
    view.api = {
        getCodexTtft: async (p) => {
            calls.push(['codex', p]);
            return { models: [{ model: 'gpt-5-codex', count: 10, p50_ms: 500, p90_ms: 900, p95_ms: 1200 }] };
        },
        getCrossToolTtft: async (p) => {
            calls.push(['cross', p]);
            return { rows: [{ tool: 'claude', model: 'sonnet', count: 5, avg_ms: 800, min_ms: 100, p90_ms: 1500, max_ms: 2000 }] };
        },
    };
    await view._loadTtftSection();
    assert.equal(calls.length, 2, 'both TTFT endpoints must be queried');
    for (const [, p] of calls) assert.deepEqual(p, expectedParams);
    assert.ok(view.loadedSections.has('ttft'));
    const body = view._bodies['ttft'];
    assert.match(body, /Codex <span class="dim">\((histogram metrics)\)<\/span>/);
    assert.match(body, /gpt-5-codex/);
    assert.match(body, /Claude Code · opencode · pi/);
    assert.match(body, /<td>sonnet<\/td>/);
});

test('ttft loader degrades per section when one endpoint fails (#216 requirement 2)', async () => {
    const view = makeView();
    view.api = {
        getCodexTtft: async () => { throw new Error('boom-codex'); },
        getCrossToolTtft: async () => ({ rows: [{ tool: 'claude', model: 'sonnet', count: 5, avg_ms: 800, min_ms: 100, p90_ms: null, max_ms: 2000 }] }),
    };
    await view._loadTtftSection();
    assert.ok(view.loadedSections.has('ttft'));
    const body = view._bodies['ttft'];
    // The failed section shows its error inline…
    assert.match(body, /Couldn't load Codex TTFT: boom-codex/);
    // …and the other section still renders its data.
    assert.match(body, /<td>sonnet<\/td>/);
});

test('ttft loader renders both empty states with no data', async () => {
    const view = makeView();
    view.api = {
        getCodexTtft: async () => ({ models: [] }),
        getCrossToolTtft: async () => ({ rows: [] }),
    };
    await view._loadTtftSection();
    const body = view._bodies['ttft'];
    assert.match(body, /No Codex TTFT data in this window/);
    assert.match(body, /No TTFT span data in this window/);
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
