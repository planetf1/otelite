// Setup view endpoint placeholders (#73).
//
// The setup view's OTLP snippets use __OTELITE_HOST__ /
// __OTLP_GRPC_PORT__ / __OTLP_HTTP_PORT__ placeholders; app.js fills
// them on load with the browser's host and the server's real receiver
// ports (from /api/health). This helper is pure — no DOM, no imports —
// so the node parity tests can import it directly.
export function fillEndpointPlaceholders(html, host, grpcPort, httpPort) {
    return html
        .split('__OTELITE_HOST__').join(host)
        .split('__OTLP_GRPC_PORT__').join(String(grpcPort))
        .split('__OTLP_HTTP_PORT__').join(String(httpPort));
}
