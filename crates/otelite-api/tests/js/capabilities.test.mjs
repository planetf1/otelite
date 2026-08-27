// Parity tests for the web telemetry-capabilities table (issue #120).
// Runs under `node --test` (no dependencies): asserts the rendered
// availability/quality/derivation vocabulary, the counts, the empty state
// and the metadata line of AnalyticsView._buildCapabilitiesTable, mirroring
// the TUI's capability_rows tests and the CLI parity fixture.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
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

const cap = (eligible, observed, valid, invalid, availability, quality, derivation) => ({
    eligible_count: eligible,
    observed_count: observed,
    valid_count: valid,
    invalid_count: invalid,
    availability,
    quality,
    derivation,
    source_attributes: {},
});

const report = (provider, model, emitter, requestCount, m) => ({
    provider,
    model,
    emitter_fingerprint: 'fp',
    emitter,
    adapter_rule: 'rule',
    request_count: requestCount,
    input_tokens: m.input,
    output_tokens: m.output,
    cache_creation_tokens: m.cacheW,
    cache_read_tokens: m.cacheR,
    ttft: m.ttft,
    correlation: { rule: 'none', matched_count: 0, unmatched_count: 0, rejected_count: 0, ambiguous_count: 0 },
});

const full = cap(5, 5, 5, 0, 'available', 'reliable', 'native');
const absent = cap(5, 0, 0, 0, 'absent', 'not_assessed', 'unavailable');

const response = {
    reports: [
        report('openai', 'gpt-4o', 'standard_otel', 5, {
            input: full,
            output: full,
            cacheW: absent,
            cacheR: absent,
            ttft: full,
        }),
        report('openai', 'gpt-4o-mini', 'standard_otel', 4, {
            input: full,
            output: cap(4, 2, 2, 0, 'sparse', 'reliable', 'native'),
            cacheW: absent,
            cacheR: absent,
            ttft: cap(4, 3, 1, 2, 'sparse', 'invalid', 'native'),
        }),
        report(null, 'claude-opus-4-6', 'claude_code', 3, {
            input: full,
            output: full,
            cacheW: absent,
            cacheR: absent,
            ttft: absent,
        }),
    ],
    canonical_span_count: 12,
    duplicate_span_count: 1,
    truncated: false,
    filters_applied: [],
};

test('capability table renders the distinct vocabulary with counts', () => {
    const html = view._buildCapabilitiesTable(response);
    // Identity rendering (provider/model composite; bare model when no provider).
    assert.match(html, /openai\/gpt-4o-mini/);
    assert.match(html, /claude-opus-4-6/);
    // Available + reliable + native: derivation suppressed, obs counts shown.
    assert.match(html, /available\/reliable \(5\/5 obs\)/);
    // Sparse + invalid must stay distinct from available/absent.
    assert.match(html, /sparse\/reliable \(2\/2 obs\)/);
    assert.match(html, /sparse\/invalid \(1\/3 obs\)/);
    // Absent shows the eligibility count and the unavailable derivation.
    assert.match(html, /absent\/not_assessed\/unavailable \(0\/5 elig\)/);
    // Metadata line.
    assert.match(html, /12 canonical request spans/);
    assert.match(html, /1 duplicate OTLP deliveries collapsed/);
    // Vocabulary legend.
    assert.match(html, /degenerate/);
    assert.match(html, /correlated/);
});

test('capability table flags invalid and absent cells for styling', () => {
    const html = view._buildCapabilitiesTable(response);
    assert.match(html, /class="warn"[^>]*>sparse\/invalid/);
    assert.match(html, /class="dim"[^>]*>absent\/not_assessed/);
});

test('capability table no-data state', () => {
    const html = view._buildCapabilitiesTable({
        reports: [],
        canonical_span_count: 0,
        duplicate_span_count: 0,
        truncated: false,
        filters_applied: [],
    });
    assert.match(html, /No LLM request spans in this window\./);
});

test('capability table declares truncation and null response', () => {
    const truncated = { ...response, truncated: true, canonical_span_count: 10000 };
    assert.match(view._buildCapabilitiesTable(truncated), /bounded sample — older spans excluded/);
    const html = view._buildCapabilitiesTable(null);
    assert.match(html, /No LLM request spans in this window\./);
});
