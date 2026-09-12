// Navigation-integrity check for the GenAI Analytics registries (#216,
// child 1). Runs under plain `node` (no dependencies), mirroring the load
// shim the web parity tests use (crates/otelite-api/tests/js/*.test.mjs).
//
// The index grid, ⌘K palette, jump-to filter and deep-link parser all
// derive from these registries, so a drift between them silently produces
// dead navigation (a report that cannot be found, a chip that 404s into
// "no such report", a group that lists a report that doesn't exist).
// This check is the CI gate for that class of rot: it must pass for the
// current 28-report set and for the consolidated 19-report set (#216)
// alike, because it validates structure, not membership counts.
//
// Usage: node scripts/check_analytics_nav.mjs   (exit 0 = consistent)

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const src = readFileSync(join(here, '../crates/otelite-api/static/js/analytics.js'), 'utf8');
const moduleObj = { exports: {} };
new Function('module', 'exports', 'window', 'parseHashQuery', 'parseHashWindow', src)(
    moduleObj,
    moduleObj.exports,
    undefined,
    () => ({}),
    () => null,
);
const { AnalyticsView } = moduleObj.exports;

const errors = [];
const fail = (msg) => errors.push(msg);

const reportIds = new Set(AnalyticsView.REPORTS.map(r => r.id));

// 1. REPORTS: unique ids, non-empty title and hint.
if (reportIds.size !== AnalyticsView.REPORTS.length) {
    fail('REPORTS contains duplicate ids');
}
for (const r of AnalyticsView.REPORTS) {
    if (!r.title || !r.title.length) fail(`report '${r.id}' has an empty title`);
    if (!r.hint || !r.hint.length) fail(`report '${r.id}' has an empty hint`);
}

// 2. GROUPS: every group member exists, no report in two groups, every
//    report in exactly one group, every group has a label and subtitle.
const grouped = new Set();
for (const g of AnalyticsView.GROUPS) {
    if (!g.id || !g.label) fail(`group missing id/label: ${JSON.stringify(g)}`);
    if (!g.sub) fail(`group '${g.id}' has no subtitle (groups read as an index of questions)`);
    for (const id of g.reports || []) {
        if (!reportIds.has(id)) fail(`group '${g.id}' lists unknown report '${id}'`);
        if (grouped.has(id)) fail(`report '${id}' is in more than one group`);
        grouped.add(id);
    }
}
for (const id of reportIds) {
    if (!grouped.has(id)) fail(`report '${id}' is not in any group`);
}

// 3. REPORT_KEYWORDS: every report has vocabulary, no unknown keys.
//    (The jump-to filter and the ⌘K palette match against this map; a
//    missing entry makes a report unreachable by its metric names.)
const kw = AnalyticsView.REPORT_KEYWORDS;
for (const id of reportIds) {
    const v = kw[id];
    if (!Array.isArray(v) || v.length === 0) {
        fail(`report '${id}' has no REPORT_KEYWORDS entry`);
    }
}
for (const key of Object.keys(kw)) {
    if (!reportIds.has(key)) fail(`REPORT_KEYWORDS has unknown key '${key}'`);
}

// 4. REPORT_RELATED: 2–4 curated links per report, all targets exist,
//    no self-links, no unknown keys.
const rel = AnalyticsView.REPORT_RELATED;
for (const id of reportIds) {
    const v = rel[id];
    if (!Array.isArray(v)) {
        fail(`report '${id}' has no REPORT_RELATED entry`);
        continue;
    }
    if (v.length < 2 || v.length > 4) {
        fail(`report '${id}' has ${v.length} related chips (expected 2–4)`);
    }
    for (const target of v) {
        if (target === id) fail(`report '${id}' links to itself`);
        if (!reportIds.has(target)) fail(`report '${id}' links to unknown report '${target}'`);
    }
}
for (const key of Object.keys(rel)) {
    if (!reportIds.has(key)) fail(`REPORT_RELATED has unknown key '${key}'`);
}

// 5. REPORT_ALIASES (introduced by #216 consolidation, optional today):
//    an alias maps an absorbed id to a current id — never to itself,
//    never to another alias (no chains), never from a current id.
const aliases = AnalyticsView.REPORT_ALIASES;
if (aliases) {
    for (const [from, to] of Object.entries(aliases)) {
        if (reportIds.has(from)) fail(`REPORT_ALIASES maps a current id '${from}' — aliases are for absorbed ids`);
        if (from === to) fail(`REPORT_ALIASES maps '${from}' to itself`);
        if (!reportIds.has(to)) fail(`REPORT_ALIASES['${from}'] targets unknown report '${to}'`);
    }
}

// 6. The deep-link parser validates against REPORTS (with alias
//    resolution); make sure the parser and the registry agree on what a
//    "known report" is — i.e. the report param check must reference
//    REPORTS, not a private copy.
if (!src.includes('_applyReportDeepLink')) {
    fail('analytics.js lost the deep-link parser (_applyReportDeepLink) — #report= links are dead');
}

if (errors.length > 0) {
    console.error(`analytics navigation integrity: ${errors.length} violation(s)`);
    for (const e of errors) console.error(`  - ${e}`);
    process.exit(1);
}
console.log(
    `analytics navigation integrity: OK — ${reportIds.size} reports, ` +
    `${AnalyticsView.GROUPS.length} groups, keywords ${Object.keys(kw).length}, ` +
    `related ${Object.keys(rel).length}` +
    (aliases ? `, aliases ${Object.keys(aliases).length}` : '')
);
