// Loader test for the setup view's endpoint placeholders (#73):
// fillEndpointPlaceholders must substitute the host and both OTLP
// ports, and index.html's setup view must use the placeholders (no
// hardcoded localhost endpoints).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { fillEndpointPlaceholders } from '../../static/js/setup.js';

const here = dirname(fileURLToPath(import.meta.url));

test('fillEndpointPlaceholders substitutes host and ports', () => {
    const html =
        '<code>__OTELITE_HOST__:__OTLP_GRPC_PORT__</code>' +
        '<pre>export OTEL_EXPORTER_OTLP_ENDPOINT=http://__OTELITE_HOST__:__OTLP_HTTP_PORT__</pre>';
    const out = fillEndpointPlaceholders(html, '192.168.1.20', 5317, 5318);
    assert.equal(
        out,
        '<code>192.168.1.20:5317</code>' +
            '<pre>export OTEL_EXPORTER_OTLP_ENDPOINT=http://192.168.1.20:5318</pre>'
    );
    // No placeholder survives.
    assert.doesNotMatch(out, /__OTELITE_HOST__|__OTLP_GRPC_PORT__|__OTLP_HTTP_PORT__/);
});

test('fillEndpointPlaceholders accepts string ports', () => {
    const out = fillEndpointPlaceholders(
        '__OTELITE_HOST__:__OTLP_GRPC_PORT__',
        'localhost',
        '4317',
        '9999'
    );
    assert.equal(out, 'localhost:4317');
});

test('index.html setup view uses placeholders, no hardcoded localhost endpoints', () => {
    const html = readFileSync(join(here, '../../static/index.html'), 'utf8');
    // All OTLP endpoints are templated…
    assert.ok(html.includes('__OTELITE_HOST__:__OTLP_GRPC_PORT__'));
    assert.ok(html.includes('__OTELITE_HOST__:__OTLP_HTTP_PORT__'));
    // …and no hardcoded localhost endpoint survives anywhere in the page.
    assert.doesNotMatch(html, /localhost:43\d\d/);
});
