// Loader test for the Productivity report (issue #177):
// _loadProductivitySection must query /genai/productivity with the standard
// time window, render the per-day per-tool table with the cost and
// cost-per-commit columns, document the emitter coverage, and render the
// empty state cleanly.
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

const expectedParams = {
    start_time: new Date('2026-09-06T00:00:00Z').getTime() * 1_000_000,
    end_time: new Date('2026-09-07T00:00:00Z').getTime() * 1_000_000,
};

test('productivity loader queries the API with the standard time window', async () => {
    const view = makeView();
    const calls = [];
    view.api = {
        getProductivity: async (p) => {
            calls.push(p);
            return {
                rows: [
                    {
                        day: '2026-09-06',
                        tool: 'claude_code',
                        commits: 3,
                        prs: 1,
                        lines_added: 120,
                        lines_removed: 45,
                        cost_usd: 2.5,
                        cost_per_commit_usd: 2.5 / 3,
                    },
                    {
                        day: '2026-09-07',
                        tool: 'opencode',
                        commits: 0,
                        prs: 0,
                        lines_added: 7,
                        lines_removed: 0,
                        cost_usd: null,
                        cost_per_commit_usd: null,
                    },
                ],
            };
        },
    };
    await view._loadProductivitySection();
    assert.equal(calls.length, 1);
    assert.deepEqual(calls[0], expectedParams);
    assert.ok(view.loadedSections.has('productivity'));
    const body = view._bodies['productivity'];
    // Coverage note: only Claude Code reports commits/PRs.
    assert.match(body, /Commits and PRs are reported by Claude Code only/);
    // Header + one cell per column, in API order (day asc, tool asc).
    assert.match(body, /<th>Cost \/ commit<\/th>/);
    assert.match(body, /<td>2026-09-06<\/td><td>claude_code<\/td><td>3<\/td><td>1<\/td><td>120<\/td><td>45<\/td>/);
    assert.match(body, /\$2\.50/); // cost formatted as USD
    assert.match(body, /\$0\.83/); // cost per commit derived server-side
    // Unpriced row: null cost renders as the em-dash placeholder, commits as 0.
    assert.match(body, /<td>2026-09-07<\/td><td>opencode<\/td><td>0<\/td><td>0<\/td><td>7<\/td><td>0<\/td><td>—<\/td><td>—<\/td>/);
});

test('productivity loader renders the empty state with no rows', async () => {
    const view = makeView();
    view.api = {
        getProductivity: async () => ({ rows: [] }),
    };
    await view._loadProductivitySection();
    assert.ok(view.loadedSections.has('productivity'));
    const body = view._bodies['productivity'];
    assert.match(body, /No commit, PR or lines-of-code data in this window/);
    assert.doesNotMatch(body, /<table/);
});

test('productivity loader surfaces API errors via the section error path', async () => {
    const view = makeView();
    view.api = {
        getProductivity: async () => {
            throw new Error('boom-productivity');
        },
    };
    await view._loadProductivitySection();
    assert.equal(view._errors.length, 1);
    assert.equal(view._errors[0][0], 'productivity');
    assert.match(view._errors[0][1].message, /boom-productivity/);
    assert.ok(!view.loadedSections.has('productivity'));
});
