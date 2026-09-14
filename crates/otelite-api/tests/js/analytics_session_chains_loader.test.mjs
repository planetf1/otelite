// Loader test for the Session Chains report (issue #165):
// _loadSessionChainsSection must render the summary line (chains and
// resumed counts), one table row per chain in server order with
// formatted cost (dash when unpriced) and span, plus the empty state
// and the API error path.
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

// 2026-01-01T00:00:00Z and +17.5 h.
const FIRST = Date.UTC(2026, 0, 1, 0, 0, 0) * 1_000_000;
const LAST = FIRST + (17 * 3600 + 30 * 60) * 1_000_000_000;

const DATA = {
    chains: [
        {
            chain_id: '12345678-aaaa-bbbb-cccc-ddddeeee0000',
            tool: 'claude_code',
            segments: [
                { index: 1, first_seen: FIRST, last_seen: FIRST + 60_000_000_000, turns: 4 },
                { index: 2, first_seen: LAST - 30_000_000_000, last_seen: LAST, turns: 2 },
            ],
            total_turns: 6,
            total_tokens: 123456,
            total_cost_usd: 12.345,
            first_seen: FIRST,
            last_seen: LAST,
        },
        {
            chain_id: 'ffffffff-0000-1111-2222-333344445555',
            tool: 'opencode',
            segments: [{ index: 1, first_seen: FIRST, last_seen: FIRST + 10_000_000_000, turns: 1 }],
            total_turns: 1,
            total_tokens: 42,
            total_cost_usd: null,
            first_seen: FIRST,
            last_seen: FIRST + 10_000_000_000,
        },
    ],
};

test('session_chains loader renders summary line and chain table', async () => {
    const view = makeView();
    const calls = [];
    view.api = {
        getSessionChains: async (p) => {
            calls.push(p);
            return DATA;
        },
    };
    await view._loadSessionChainsSection();
    assert.equal(calls.length, 1);
    assert.ok(view.loadedSections.has('session_chains'));
    const body = view._bodies['session_chains'];
    // Coverage hint explains the segment rule.
    assert.match(body, /exceeds 2 hours/);
    // Summary: 2 chains, 1 resumed.
    assert.match(body, /<strong>2<\/strong> chain\(s\) · <strong>1<\/strong> resumed \(2\+ segments\)/);
    // Table header.
    assert.match(body, /<th>Tool<\/th><th>Chain<\/th><th>Segments<\/th>/);
    // Resumed chain: short id (title carries the full one), two
    // segments, formatted cost and span.
    assert.match(body, /<td title="12345678-aaaa-bbbb-cccc-ddddeeee0000">12345678<\/td>/);
    assert.match(body, /<td>2<\/td>\s*<td>6<\/td>\s*<td>123,456<\/td>\s*<td>\$12.35<\/td>/);
    assert.match(body, /<td>2026-01-01 00:00<\/td>\s*<td>2026-01-01 17:30<\/td>\s*<td>17\.5 h<\/td>/);
    // Unpriced chain: dash cost.
    assert.match(body, /<td>1<\/td>\s*<td>1<\/td>\s*<td>42<\/td>\s*<td>—<\/td>/);
});

test('session_chains loader renders the empty state', async () => {
    const view = makeView();
    view.api = {
        getSessionChains: async () => ({ chains: [] }),
    };
    await view._loadSessionChainsSection();
    assert.ok(view.loadedSections.has('session_chains'));
    const body = view._bodies['session_chains'];
    assert.match(body, /No sessions in this window/);
    assert.doesNotMatch(body, /<table/);
});

test('session_chains loader surfaces API errors via the section error path', async () => {
    const view = makeView();
    view.api = {
        getSessionChains: async () => {
            throw new Error('boom-session-chains');
        },
    };
    await view._loadSessionChainsSection();
    assert.equal(view._errors.length, 1);
    assert.equal(view._errors[0][0], 'session_chains');
    assert.ok(!view.loadedSections.has('session_chains'));
});
