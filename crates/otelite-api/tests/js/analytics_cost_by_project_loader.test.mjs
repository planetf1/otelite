// Loader test for the Cost by Project report (issue #173):
// _loadCostByProjectSection must group rows by project (client-side),
// render one expandable group per project with total + share, order
// groups by cost desc, and render the empty state and the unattributed
// coverage note.
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

test('cost_by_project loader groups by project with totals, share, and detail rows', async () => {
    const view = makeView();
    const calls = [];
    view.api = {
        getCostByProject: async (p) => {
            calls.push(p);
            return {
                rows: [
                    {
                        project: 'api-server', tool: 'opencode', model: 'granite-4.0',
                        requests: 2, input_tokens: 30, output_tokens: 10,
                        cache_creation_tokens: 0, cache_read_tokens: 0,
                        cost_usd: 4.0, cost_source: 'litellm',
                    },
                    {
                        project: 'api-server', tool: 'opencode', model: 'granite-4.1',
                        requests: 1, input_tokens: 1, output_tokens: 1,
                        cache_creation_tokens: 0, cache_read_tokens: 0,
                        cost_usd: 1.0, cost_source: 'litellm',
                    },
                    {
                        project: 'web-ui', tool: 'opencode', model: 'granite-4.0',
                        requests: 1, input_tokens: 7, output_tokens: 3,
                        cache_creation_tokens: 0, cache_read_tokens: 0,
                        cost_usd: null, cost_source: null,
                    },
                ],
            };
        },
    };
    await view._loadCostByProjectSection();
    assert.equal(calls.length, 1);
    assert.ok(view.loadedSections.has('cost_by_project'));
    const body = view._bodies['cost_by_project'];
    // Coverage note: label-less spans are a known limitation.
    assert.match(body, /land in "unattributed"/);
    // Two project groups, cost-desc: api-server ($5.00, 100%) first.
    assert.match(body, /<details class="cbp-project" open>/);
    assert.match(body, /<strong>api-server<\/strong> — \$5\.00 \(100\.0%\) · 2 models/);
    assert.match(body, /<strong>web-ui<\/strong> — — \(0\.0%\) · 1 model/);
    // Detail rows in both groups.
    assert.match(body, /<td>granite-4\.0<\/td><td>2<\/td><td>\$4\.00<\/td>/);
    assert.match(body, /<td>granite-4\.1<\/td><td>1<\/td><td>\$1\.00<\/td>/);
    // Unpriced row renders the dash, and api-server's group opens first.
    assert.match(body, /<td>granite-4\.0<\/td><td>1<\/td><td>—<\/td>/);
    const apiIdx = body.indexOf('api-server');
    const webIdx = body.indexOf('web-ui');
    assert.ok(apiIdx < webIdx, 'cost-desc group order');
});

test('cost_by_project loader renders the empty state', async () => {
    const view = makeView();
    view.api = {
        getCostByProject: async () => ({ rows: [] }),
    };
    await view._loadCostByProjectSection();
    assert.ok(view.loadedSections.has('cost_by_project'));
    const body = view._bodies['cost_by_project'];
    assert.match(body, /No LLM span data in this window/);
    assert.doesNotMatch(body, /<details/);
});

test('cost_by_project loader surfaces API errors via the section error path', async () => {
    const view = makeView();
    view.api = {
        getCostByProject: async () => {
            throw new Error('boom-cost-by-project');
        },
    };
    await view._loadCostByProjectSection();
    assert.equal(view._errors.length, 1);
    assert.equal(view._errors[0][0], 'cost_by_project');
    assert.ok(!view.loadedSections.has('cost_by_project'));
});
