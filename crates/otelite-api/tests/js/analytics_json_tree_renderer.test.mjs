// Renderer test for the logs-view JSON tree (#47): renderJsonTree must
// produce a navigable <details> tree with escaped keys/values, collapsed
// deep nesting, and stable scalar rendering. The module is pure (no DOM),
// so it can be imported directly here.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { renderJsonTree } from '../../static/js/json_tree.js';

test('scalars render as typed spans', () => {
    assert.equal(renderJsonTree(null), `<span class="json-scalar json-null">null</span>`);
    assert.equal(renderJsonTree(true), `<span class="json-scalar json-boolean">true</span>`);
    assert.equal(renderJsonTree(42), `<span class="json-scalar json-number">42</span>`);
    assert.equal(renderJsonTree('hi'), `<span class="json-scalar json-string">&quot;hi&quot;</span>`);
});

test('flat object renders key/leaf rows', () => {
    const html = renderJsonTree({ a: 1, b: 'x' });
    assert.match(html, /class="json-tree-obj"/);
    assert.match(html, /<span class="json-tree-key">a<\/span>/);
    assert.match(html, /<span class="json-tree-key">b<\/span>/);
    assert.match(html, /json-tree-leaf/);
    assert.match(html, /json-number">1<\/span>/);
    assert.doesNotMatch(html, /<details/);
});

test('nested object is collapsible and open only in the first two levels', () => {
    const html = renderJsonTree({ a: { b: { c: { d: 1 } } } });
    // depth 0 (a) and depth 1 (b) start open; depth 2 (c) is collapsed
    const nodes = html.match(/<details class="json-tree-node"( open)?>/g) ?? [];
    assert.equal(nodes.length, 3);
    assert.equal(nodes.filter((n) => n.includes(' open')).length, 2);
});

test('arrays render indexed collapsible nodes and leaves', () => {
    const html = renderJsonTree([1, { x: 2 }, 'y']);
    assert.match(html, /class="json-tree-array"/);
    assert.match(html, /<span class="json-tree-key json-tree-idx">\[0\]<\/span>/);
    assert.match(html, /<span class="json-tree-key json-tree-idx">\[2\]<\/span>/);
    // the object element is a collapsible node, the scalars are leaves
    assert.match(html, /<details class="json-tree-node" open><summary class="json-tree-summary"><span class="json-tree-key json-tree-idx">\[1\]<\/span>/);
});

test('empty container and deep-nesting edge cases', () => {
    assert.equal(renderJsonTree({}), `<span class="json-scalar">{&thinsp;}</span>`);
    assert.equal(renderJsonTree([]), `<span class="json-scalar">[&thinsp;]</span>`);
    // deeper than MAX_JSON_TREE_DEPTH (8) truncates to an ellipsis scalar
    const deep = { a: { b: { c: { d: { e: { f: { g: { h: { i: { j: 1 } } } } } } } } } };
    assert.match(renderJsonTree(deep), /json-string">"…"<\/span>/);
});

test('keys and string values are HTML-escaped', () => {
    const html = renderJsonTree({ '<script>alert(1)</script>': 'a < b & "c"' });
    assert.doesNotMatch(html, /<script>alert/);
    assert.match(html, /&lt;script&gt;alert\(1\)&lt;\/script&gt;/);
    assert.match(html, /a &lt; b &amp; \\&quot;c\\&quot;/);
});
