// JSON tree rendering for the logs detail panel (#47).
//
// Pure functions — no DOM, no imports — so the node parity tests can
// import this module directly (same pattern as setup.js). The tree is
// built from <details>/<summary> elements: nested objects/arrays are
// collapsible, the first two levels start open, and deeper nesting is
// collapsed to keep large payloads (multi-hundred-KB LLM request
// bodies) navigable without rendering a wall of text.
const MAX_JSON_TREE_DEPTH = 8;

function escapeHtml(text) {
    return String(text)
        .replaceAll('&', '&amp;')
        .replaceAll('<', '&lt;')
        .replaceAll('>', '&gt;')
        .replaceAll('"', '&quot;')
        .replaceAll("'", '&#39;');
}

/**
 * Render a parsed JSON value as collapsible-tree HTML.
 * @param {unknown} value parsed JSON value
 * @param {number} depth current nesting depth (0 at the root)
 * @returns {string} HTML fragment
 */
export function renderJsonTree(value, depth = 0) {
    if (depth > MAX_JSON_TREE_DEPTH) {
        return `<span class="json-scalar json-string">"…"</span>`;
    }
    if (value === null) {
        return `<span class="json-scalar json-null">null</span>`;
    }
    if (typeof value === 'boolean') {
        return `<span class="json-scalar json-boolean">${value}</span>`;
    }
    if (typeof value === 'number') {
        return `<span class="json-scalar json-number">${value}</span>`;
    }
    if (typeof value === 'string') {
        return `<span class="json-scalar json-string">${escapeHtml(JSON.stringify(value))}</span>`;
    }
    if (Array.isArray(value)) {
        if (value.length === 0) return `<span class="json-scalar">[&thinsp;]</span>`;
        return `<div class="json-tree-array">${value.map((item, i) => {
            const isObj = item !== null && typeof item === 'object';
            if (isObj) {
                const isOpen = depth < 2 ? ' open' : '';
                return `<details class="json-tree-node"${isOpen}><summary class="json-tree-summary"><span class="json-tree-key json-tree-idx">[${i}]</span></summary><div class="json-tree-children">${renderJsonTree(item, depth + 1)}</div></details>`;
            }
            return `<div class="json-tree-leaf"><span class="json-tree-key json-tree-idx">[${i}]</span><span class="json-tree-sep">:&nbsp;</span>${renderJsonTree(item, depth + 1)}</div>`;
        }).join('')}</div>`;
    }
    // Plain object
    const keys = Object.keys(value);
    if (keys.length === 0) return `<span class="json-scalar">{&thinsp;}</span>`;
    return `<div class="json-tree-obj">${keys.map(k => {
        const v = value[k];
        const isObj = v !== null && typeof v === 'object';
        const keyHtml = `<span class="json-tree-key">${escapeHtml(k)}</span>`;
        if (isObj) {
            const isOpen = depth < 2 ? ' open' : '';
            return `<details class="json-tree-node"${isOpen}><summary class="json-tree-summary">${keyHtml}</summary><div class="json-tree-children">${renderJsonTree(v, depth + 1)}</div></details>`;
        }
        return `<div class="json-tree-leaf">${keyHtml}<span class="json-tree-sep">:&nbsp;</span>${renderJsonTree(v, depth + 1)}</div>`;
    }).join('')}</div>`;
}
