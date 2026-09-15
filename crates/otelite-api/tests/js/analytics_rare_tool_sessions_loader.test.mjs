// Loader test for the Rare Tool Sessions report (issue #176):
// _loadRareToolSessionsSection must render the session table (tool,
// 8-char session prefix, UTC start, duration, dominant model, tokens,
// cost, task hint) with dashes for unpriced cost and missing hints —
// plus the empty state and the API error path.
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

// 2026-01-01 00:00:00 UTC.
const D1 = 1767225600000000000;

const DATA = {
    rows: [
        {
            tool: 'pi',
            session_id: 'abcdef12-3456',
            start_time: D1,
            duration_ms: 150000,
            model: 'pi-model',
            input_tokens: 1000,
            output_tokens: 500,
            cache_creation_tokens: 0,
            cache_read_tokens: 100,
            cost_usd: 1.234,
            top_span_name: 'pi.interaction',
            models: [],
        },
        {
            tool: 'deepseek',
            session_id: 'ffff0000-1111',
            start_time: D1 - 3600_000_000_000,
            duration_ms: 7_300_000, // 7300 s -> "2.0 h"
            model: 'ds-model',
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            cost_usd: null,
            top_span_name: '',
            models: [],
        },
    ],
};

test('rare_tool_sessions loader renders the session table with cost and hint', async () => {
    const view = makeView();
    view.api = {
        getRareToolSessions: async () => DATA,
    };
    await view._loadRareToolSessionsSection();
    assert.ok(view.loadedSections.has('rare_tool_sessions'));
    const body = view._bodies['rare_tool_sessions'];
    // Header + source note.
    assert.match(body, /<th>Tool<\/th><th>Session<\/th><th>Started \(UTC\)<\/th><th>Duration<\/th><th>Model<\/th><th>Tokens<\/th><th>Cost<\/th><th>Task hint<\/th>/);
    assert.match(body, /fewer than 10 sessions in this window/);
    // Row 1: 8-char prefix, UTC start, 150000 ms -> "2 min", $1.23, hint.
    assert.match(body, /<td>pi<\/td>/);
    assert.match(body, /<code>abcdef12<\/code>/);
    assert.match(body, /2026-01-01 00:00/);
    assert.match(body, /2 min/);
    assert.match(body, /<td>1,600<\/td>/);
    assert.match(body, /<td>\$1\.23<\/td>/);
    assert.match(body, /pi\.interaction/);
    // Row 2: unpriced -> dash, missing hint -> dash, 2.0 h duration.
    assert.match(body, /<code>ffff0000<\/code>/);
    assert.match(body, /2\.0 h/);
    assert.match(body, /<td>15<\/td>/);
    assert.match(body, /<td>—<\/td><td>—<\/td><\/tr>/);
});

test('rare_tool_sessions loader renders the empty state', async () => {
    const view = makeView();
    view.api = {
        getRareToolSessions: async () => ({ rows: [] }),
    };
    await view._loadRareToolSessionsSection();
    assert.ok(view.loadedSections.has('rare_tool_sessions'));
    assert.match(view._bodies['rare_tool_sessions'], /No rare-tool sessions in this window/);
    assert.doesNotMatch(view._bodies['rare_tool_sessions'], /<table/);
});

test('rare_tool_sessions loader surfaces API errors via the section error path', async () => {
    const view = makeView();
    view.api = {
        getRareToolSessions: async () => {
            throw new Error('boom-rare-tools');
        },
    };
    await view._loadRareToolSessionsSection();
    assert.equal(view._errors.length, 1);
    assert.equal(view._errors[0][0], 'rare_tool_sessions');
    assert.ok(!view.loadedSections.has('rare_tool_sessions'));
});
