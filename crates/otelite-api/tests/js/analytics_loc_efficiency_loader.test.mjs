// Loader test for the Code Efficiency report (issue #178):
// _loadLocEfficiencySection must render the per-(tool, model) table in
// response order (cheapest per 100 lines first, unpriced last), show
// unpriced values as dashes — never fabricated zeros — and keep the
// empty state and API error paths.
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
        {
            tool: 'claude_code',
            model: 'claude-sonnet-5',
            total_lines: 100,
            total_cost_usd: 18.0,
            cost_per_100_lines: 18.0,
        },
        {
            tool: 'opencode',
            model: '(unknown)',
            total_lines: 50,
            total_cost_usd: null,
            cost_per_100_lines: null,
        },
    ],
};

test('loc_efficiency loader renders the table in response order with dashes for unpriced', async () => {
    const view = makeView();
    view.api = {
        getLocEfficiency: async () => DATA,
    };
    await view._loadLocEfficiencySection();
    assert.ok(view.loadedSections.has('loc_efficiency'));
    const body = view._bodies['loc_efficiency'];
    // Header + the source note.
    assert.match(body, /<th>Tool<\/th><th>Model<\/th><th>Lines added<\/th><th>Cost<\/th><th>\$ \/ 100 lines<\/th>/);
    assert.match(body, /Only Claude Code and opencode emit lines-of-code metrics/);
    // Row 1: priced values as $ amounts.
    assert.match(body, /<td>claude_code<\/td><td>claude-sonnet-5<\/td><td>100<\/td><td>\$18\.00<\/td><td>\$18\.00<\/td>/);
    // Row 2: unpriced -> dashes, never $0.00.
    assert.match(body, /<td>opencode<\/td><td>\(unknown\)<\/td><td>50<\/td><td>—<\/td><td>—<\/td>/);
    assert.doesNotMatch(body, /\$0\.00/);
});

test('loc_efficiency loader renders the empty state', async () => {
    const view = makeView();
    view.api = {
        getLocEfficiency: async () => ({ rows: [] }),
    };
    await view._loadLocEfficiencySection();
    assert.ok(view.loadedSections.has('loc_efficiency'));
    assert.match(view._bodies['loc_efficiency'], /No lines-of-code data in this window/);
    assert.doesNotMatch(view._bodies['loc_efficiency'], /<table/);
});

test('loc_efficiency loader surfaces API errors via the section error path', async () => {
    const view = makeView();
    view.api = {
        getLocEfficiency: async () => {
            throw new Error('boom-loc-efficiency');
        },
    };
    await view._loadLocEfficiencySection();
    assert.equal(view._errors.length, 1);
    assert.equal(view._errors[0][0], 'loc_efficiency');
    assert.ok(!view.loadedSections.has('loc_efficiency'));
});
