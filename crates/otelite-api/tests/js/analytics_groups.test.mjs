// Parity tests for the analytics report categories, filter, and pins
// (issue #186). Runs under `node --test` (no dependencies): asserts the
// REPORTS/GROUPS registries stay in lockstep with the sectionLoaders
// registry (so a new report cannot be added to one and forgotten in the
// others), and the rendering of the group/section shells including the
// pin button and open-by-default behaviour for pinned reports.
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

const view = Object.create(AnalyticsView.prototype);
// Constructor state used by the shell renderers (the node fixture skips the
// constructor to avoid side effects — mirror the fields it would set).
view.sectionLoaders = {};
view.openSections = new Set();
view.pinned = new Set();

// Minimal localStorage stub for the pin persistence tests.
function stubLocalStorage(items = {}) {
    const store = new Map(Object.entries(items));
    return {
        getItem: (k) => (store.has(k) ? store.get(k) : null),
        setItem: (k, v) => store.set(k, String(v)),
        _dump: () => Object.fromEntries(store),
    };
}

test('REPORTS registry matches the sectionLoaders keys exactly', () => {
    view._registerSectionLoaders();
    const loaderKeys = new Set(Object.keys(view.sectionLoaders));
    const reportIds = new Set(AnalyticsView.REPORTS.map(r => r.id));

    assert.equal(reportIds.size, AnalyticsView.REPORTS.length, 'duplicate report ids');
    assert.equal(loaderKeys.size, reportIds.size, 'REPORTS and sectionLoaders drifted');
    for (const id of reportIds) {
        assert.ok(loaderKeys.has(id), `report '${id}' has no section loader`);
    }
    for (const r of AnalyticsView.REPORTS) {
        assert.ok(r.title.length > 0, `report '${r.id}' has an empty title`);
        assert.ok(r.hint.length > 0, `report '${r.id}' has an empty hint`);
    }
});

test('GROUPS partition the REPORTS — every report in exactly one group', () => {
    const reportIds = new Set(AnalyticsView.REPORTS.map(r => r.id));
    const seen = new Set();
    for (const g of AnalyticsView.GROUPS) {
        assert.ok(g.id.length > 0 && g.label.length > 0, 'group missing id/label');
        for (const id of g.reports) {
            assert.ok(reportIds.has(id), `group '${g.id}' lists unknown report '${id}'`);
            assert.ok(!seen.has(id), `report '${id}' is in more than one group`);
            seen.add(id);
        }
    }
    assert.equal(seen.size, reportIds.size, 'some reports are not in any group');
});

test('_renderGroupShell renders nested section shells and the count label', () => {
    view.pinned = new Set();
    const html = view._renderGroupShell('latency', 'Latency', ['latency', 'codex_ttft']);
    assert.match(html, /<details class="analytics-group" id="analytics-group-latency">/);
    assert.match(html, /<span class="analytics-group-title">Latency<\/span>/);
    assert.match(html, /<span class="analytics-group-count" id="analytics-group-count-latency">2 reports<\/span>/);
    assert.match(html, /id="analytics-section-latency"/);
    assert.match(html, /id="analytics-section-codex_ttft"/);
});

test('_renderGroupShell honours the open flag and pinned styling', () => {
    view.pinned = new Set();
    assert.match(view._renderGroupShell('cost', 'Cost', ['cost'], true), /<details[^>]+ id="analytics-group-cost" open>/);
    assert.doesNotMatch(view._renderGroupShell('cost', 'Cost', ['cost']), /open/);
    assert.match(
        view._renderGroupShell('pinned', 'Pinned', ['cost']),
        /class="analytics-group analytics-group-pinned" id="analytics-group-pinned"/,
    );
    // Empty report list (fresh Pinned group) still renders the shell —
    // the caller appends the moved <details> element afterwards.
    const empty = view._renderGroupShell('pinned', 'Pinned', []);
    assert.match(empty, /id="analytics-group-body-pinned"><\/div>[\s]*<\/details>[\s]*$/);
    assert.doesNotMatch(empty, /analytics-section-/);
});

test('_renderSectionShell renders the pin button and pinned open state', () => {
    view.pinned = new Set();
    const plain = view._renderSectionShell('cost');
    assert.match(plain, /<details class="analytics-section" id="analytics-section-cost">/);
    assert.match(plain, /class="pin-btn" data-pin="cost"[\s\S]*aria-pressed="false"/);
    assert.doesNotMatch(plain, /open/);

    view.pinned = new Set(['cost']);
    const pinned = view._renderSectionShell('cost');
    assert.match(pinned, /<details class="analytics-section" id="analytics-section-cost" open>/);
    assert.match(pinned, /class="pin-btn pinned" data-pin="cost"[\s\S]*aria-pressed="true"/);
    assert.match(pinned, /title="Unpin report"/);

    assert.equal(view._renderSectionShell('no_such_report'), '');
});

test('_loadPins keeps valid ids and drops corrupt storage', () => {
    globalThis.localStorage = stubLocalStorage({
        'otelite.analytics.pinned': JSON.stringify(['cost', 'no_such_report', 'latency']),
    });
    const pins = view._loadPins();
    assert.deepEqual([...pins].sort(), ['cost', 'latency']);

    globalThis.localStorage = stubLocalStorage({ 'otelite.analytics.pinned': '{corrupt' });
    assert.equal(view._loadPins().size, 0);

    globalThis.localStorage = stubLocalStorage({ 'otelite.analytics.pinned': '{"a":1}' });
    assert.equal(view._loadPins().size, 0);

    delete globalThis.localStorage;
    // No localStorage at all (node, private browsing) — unpinned, no throw.
    assert.equal(view._loadPins().size, 0);
});

test('_savePins writes the pinned id list as JSON', () => {
    const ls = stubLocalStorage();
    globalThis.localStorage = ls;
    view.pinned = new Set(['cost', 'latency']);
    view._savePins();
    assert.deepEqual(JSON.parse(ls._dump()['otelite.analytics.pinned']), ['cost', 'latency']);
    delete globalThis.localStorage;
});
