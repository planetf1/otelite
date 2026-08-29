// Parity tests for the web model-performance diagnosis section (issue
// #121/#154). Runs under `node --test` (no dependencies): asserts the
// rendered classification vocabulary (verbatim from the API object), the
// percentage-unavailable wording, sample counts, confidence, the exact
// comparison intervals and the safe no-data state of
// AnalyticsView._buildModelPerformanceTable, mirroring the TUI's
// model_perf_rows tests and the frozen parity fixture.
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

const delta = (absolute, relative) => ({ absolute, relative });

const metric = (m, cls, conf, cur, prev, d, notes = [], roll = undefined) => ({
    metric: m,
    class: cls,
    confidence: conf,
    eligible_current: 12,
    eligible_preceding: 12,
    ...(roll !== undefined ? { eligible_rolling: roll } : {}),
    current_median: cur,
    preceding_median: prev,
    median_delta_vs_preceding: d,
    ...(notes.length ? { notes } : {}),
});

const assessment = (provider, model, overallClass, overallConf, metrics, notes = []) => ({
    provider,
    model,
    emitter_fingerprint: 'genai-v1-test',
    request_counts: { current: 12, preceding: 12, rolling: 12 },
    metrics,
    overall_class: overallClass,
    overall_confidence: overallConf,
    truncated: false,
    ...(notes.length ? { notes } : {}),
});

const diag = {
    current_window: { start_time: 1787616000000000000, end_time: 1787702400000000000 },
    preceding_window: { start_time: 1787529600000000000, end_time: 1787616000000000000 },
    rolling_window: { start_time: 1787097600000000000, end_time: 1787529600000000000 },
    timezone: 'Europe/London',
    truncated: false,
    identities: [
        { provider: 'openai', model: 'gpt-4o', emitter_fingerprint: 'genai-v1-test' },
        { provider: 'openai', model: 'o2', emitter_fingerprint: 'genai-v1-test' },
        { provider: 'openai', model: 'o3', emitter_fingerprint: 'genai-v1-test' },
        { provider: 'openai', model: 'stable-1', emitter_fingerprint: 'genai-v1-test' },
    ],
    assessments: [
        assessment('openai', 'gpt-4o', 'typical_regression', 'high', [
            metric('duration', 'typical_regression', 'high', 1300, 1000, delta(300, 0.3), [], 12),
            metric('throughput', 'typical_regression', 'high', 77, 100, delta(-23.1, -0.231), [], 12),
            metric('ttft', 'no_material_change', 'high', 200, 200, delta(0, 0)),
            metric('error_rate', 'no_material_change', 'high', 0, 0, delta(0, 0)),
        ]),
        assessment('openai', 'o2', 'error_associated', 'high', [
            metric('duration', 'error_associated', 'high', 1300, 1000, delta(300, 0.3), ['error rate rose 0% → 33.3%']),
            metric('throughput', 'error_associated', 'high', 77, 100, delta(-23.1, -0.231)),
            metric('ttft', 'no_material_change', 'high', 200, 200, delta(0, 0)),
            // Zero baseline: relative is null — "pct unavailable", never 0%.
            metric('error_rate', 'error_associated', 'high', 0.333, 0, delta(0.333, null)),
        ]),
        assessment('openai', 'o3', 'mixed_evidence', 'medium', [
            metric('duration', 'mixed_evidence', 'medium', 1300, 1000, delta(300, 0.3), ['preceding and rolling baselines disagree; both are reported'], 12),
            metric('throughput', 'no_material_change', 'medium', 77, 100, delta(-23.1, -0.231)),
            metric('ttft', 'no_material_change', 'medium', 200, 200, delta(0, 0)),
            metric('error_rate', 'no_material_change', 'medium', 0, 0, delta(0, 0)),
        ]),
        assessment('openai', 'stable-1', 'no_material_change', 'high', [
            metric('duration', 'no_material_change', 'high', 1000, 1000, delta(0, 0), [], 12),
            metric('throughput', 'no_material_change', 'high', 100, 100, delta(0, 0)),
            // Untrusted (degenerate) TTFT: attribution is prevented.
            metric('ttft', 'insufficient_telemetry', 'insufficient', 950, 950, delta(0, 0), ['TTFT is not reliable; attribution is prevented']),
            metric('error_rate', 'no_material_change', 'high', 0, 0, delta(0, 0)),
        ]),
    ],
};

test('model performance renders the exact intervals, rolling baseline and timezone', () => {
    const html = view._buildModelPerformanceTable(diag);
    assert.match(html, /Current \[2026-08-25T00:00:00Z → 2026-08-26T00:00:00Z\)/);
    assert.match(html, /Preceding \[2026-08-24T00:00:00Z → 2026-08-25T00:00:00Z\)/);
    assert.match(html, /Rolling \[2026-08-19T00:00:00Z → 2026-08-24T00:00:00Z\)/);
    assert.match(html, /Timezone Europe\/London/);
});

test('model performance renders classes verbatim with sample counts', () => {
    const html = view._buildModelPerformanceTable(diag);
    for (const cls of ['typical_regression', 'error_associated', 'mixed_evidence', 'no_material_change', 'insufficient_telemetry']) {
        assert.match(html, new RegExp(cls));
    }
    // Overall class and confidence are part of the diagnosis vocabulary.
    assert.match(html, /overall typical_regression \(high\)/);
    assert.match(html, /overall mixed_evidence \(medium\)/);
    // Sample counts: current/preceding and, when rolling is enabled, /rolling.
    assert.match(html, /12\/12\/12/);
    assert.match(html, /12\/12/);
    // Values in native units.
    assert.match(html, /1300 ms/);
    assert.match(html, /77 tok\/s/);
});

test('model performance keeps the percentage-unavailable wording distinct', () => {
    const html = view._buildModelPerformanceTable(diag);
    // o2 error_rate: zero baseline → absolute points + explicit unavailable.
    assert.match(html, /\+33\.3 pts \(pct unavailable\)/);
    // Normal relative state stays a percentage.
    assert.match(html, /\+300\.0 \(30\.00%\)/);
    // The legend explains the wording.
    assert.match(html, /pct unavailable/);
});

test('model performance renders confidence and notes without causal wording', () => {
    const html = view._buildModelPerformanceTable(diag);
    assert.match(html, /insufficient/);
    assert.match(html, /attribution is prevented/);
    assert.match(html, /correlation, not causation/);
    assert.match(html, /both are reported/);
});

test('model performance routes to full JSON evidence for the selected model and interval', () => {
    const html = view._buildModelPerformanceTable(diag);
    assert.match(html, /full JSON/);
    // The href is built with URLSearchParams and inserted unescaped (the
    // browser encodes it on navigation); assert the parameter set verbatim.
    assert.match(html, /<a href="\/api\/genai\/model-performance\?start_time=1787616000000000000&end_time=1787702400000000000&model=o2&provider=openai" target="_blank" rel="noopener">full JSON<\/a>/);
});

test('model performance renders rolling-disabled and no-data states', () => {
    const noRoll = { ...diag, rolling_window: undefined, truncated: true };
    const html = view._buildModelPerformanceTable(noRoll);
    assert.match(html, /Rolling — \(disabled\)/);
    assert.match(html, /bounded sample — older spans excluded/);

    const empty = view._buildModelPerformanceTable({
        current_window: diag.current_window,
        preceding_window: diag.preceding_window,
        truncated: false,
        identities: [],
        assessments: [],
    });
    assert.match(empty, /No LLM request spans in this window\./);
    assert.match(view._buildModelPerformanceTable(null), /No LLM request spans in this window\./);
});
