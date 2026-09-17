// Loader test for the Context Composition report (#113/#244):
// _loadContextCompositionSection must render the CLI-style summary line
// (session count, fixed-prefix median/max, growth median), one table row
// per session in server order (fixed_prefix desc) with the session id
// truncated to 8 chars (full id in the title), plus the empty state and
// the API error path.
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
    sessions: [
        {
            session_id: '12345678-aaaa-bbbb-cccc-ddddeeee0000',
            request_count: 133,
            cached_requests: 120,
            fixed_prefix: 212389,
            peak_context: 403280,
            growth: 190891,
            first_request_ns: FIRST,
            last_request_ns: LAST,
        },
        {
            session_id: '00000002-5555-6666-7777-888888888888',
            request_count: 35,
            cached_requests: 30,
            fixed_prefix: 100000,
            peak_context: 150000,
            growth: 50000,
            first_request_ns: FIRST + 2_000_000_000,
            last_request_ns: FIRST + 9_000_000_000,
        },
        {
            session_id: '00000001-1111-2222-3333-444444444444',
            request_count: 8,
            cached_requests: 6,
            fixed_prefix: 67740,
            peak_context: 154236,
            growth: 86496,
            first_request_ns: FIRST + 1_000_000_000,
            last_request_ns: FIRST + 5_000_000_000,
        },
    ],
};

test('context_composition loader renders summary line and session table', async () => {
    const view = makeView();
    const calls = [];
    view.api = {
        getContextComposition: async (p) => {
            calls.push(p);
            return DATA;
        },
    };
    await view._loadContextCompositionSection();
    assert.equal(calls.length, 1);
    assert.ok(view.loadedSections.has('context_composition'));
    const body = view._bodies['context_composition'];
    // Coverage hint explains the prefix estimate.
    assert.match(body, /minimum cache-read/);
    // Summary: 3 sessions, prefix median 100,000 / max 212,389, growth median 86,496.
    assert.match(body, /<strong>3<\/strong> session\(s\) with cache data — fixed prefix median <strong>100,000<\/strong>, max <strong>212,389<\/strong>/);
    assert.match(body, /in-session growth median <strong>86,496<\/strong>/);
    // Table header.
    assert.match(body, /<th>Session<\/th><th>Reqs<\/th><th>Cached<\/th><th>Fixed prefix<\/th>/);
    // First row: truncated id (title carries the full one), formatted
    // counts and the UTC window.
    assert.match(body, /<td title="12345678-aaaa-bbbb-cccc-ddddeeee0000">12345678<\/td>/);
    assert.match(body, /<td>133<\/td>\s*<td>120<\/td>\s*<td>212,389<\/td>\s*<td>403,280<\/td>\s*<td>190,891<\/td>/);
    assert.match(body, /<td>2026-01-01 00:00<\/td>\s*<td>2026-01-01 17:30<\/td>/);
    // Server order is preserved (fixed_prefix desc): the 100,000-prefix
    // session renders before the 67,740-prefix one.
    assert.ok(body.indexOf('00000002') < body.indexOf('00000001'));
});

test('context_composition loader renders the empty state', async () => {
    const view = makeView();
    view.api = {
        getContextComposition: async () => ({ sessions: [] }),
    };
    await view._loadContextCompositionSection();
    assert.ok(view.loadedSections.has('context_composition'));
    const body = view._bodies['context_composition'];
    assert.match(body, /No sessions with cache data in this window/);
    assert.doesNotMatch(body, /<table/);
});

test('context_composition loader surfaces API errors via the section error path', async () => {
    const view = makeView();
    view.api = {
        getContextComposition: async () => {
            throw new Error('boom-context-composition');
        },
    };
    await view._loadContextCompositionSection();
    assert.equal(view._errors.length, 1);
    assert.equal(view._errors[0][0], 'context_composition');
    assert.ok(!view.loadedSections.has('context_composition'));
});
