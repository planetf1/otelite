/**
 * Traces View - Display and interact with distributed traces
 */

class TracesView {
    constructor(apiClient) {
        this.apiClient = apiClient;
        this.traces = [];
        this.selectedTrace = null;
        this.filters = {
            traceId: '',
            service: '',
            search: '',
            startTime: null,
            endTime: null
        };
        this.currentPage = 0;
        this.pageSize = 50;
        this.autoRefresh = false;
        this.refreshInterval = null;
    }

    /**
     * Render the traces view
     */
    render() {
        const container = document.getElementById('traces-view');
        container.innerHTML = `
            <div class="view-header">
                <h2>Traces</h2>
                <div class="view-actions">
                    <button id="refresh-traces" class="btn btn-primary">Refresh</button>
                    <button id="export-traces" class="btn btn-secondary">Export</button>
                    <label class="auto-refresh-toggle">
                        <input type="checkbox" id="auto-refresh-traces">
                        Auto-refresh (5s)
                    </label>
                </div>
            </div>

            <div class="filters">
                <input type="text" id="trace-id-filter" placeholder="Trace ID" class="filter-input">
                <input type="text" id="service-filter" placeholder="Service name" class="filter-input">
                <input type="text" id="search-traces" placeholder="Search span names..." class="filter-input">
                <button id="apply-trace-filters" class="btn btn-primary">Apply Filters</button>
                <button id="clear-trace-filters" class="btn btn-secondary">Clear</button>
            </div>

            <div class="traces-container">
                <div id="traces-list" class="traces-list"></div>
                <div id="trace-detail" class="trace-detail" style="display: none;"></div>
            </div>

            <div class="pagination">
                <button id="prev-trace-page" class="btn btn-secondary">Previous</button>
                <span id="trace-page-info">Page 1</span>
                <button id="next-trace-page" class="btn btn-secondary">Next</button>
            </div>
        `;

        this.attachEventListeners();
        this.loadTraces();
    }

    /**
     * Attach event listeners
     */
    attachEventListeners() {
        document.getElementById('refresh-traces').addEventListener('click', () => this.loadTraces());
        document.getElementById('export-traces').addEventListener('click', () => this.exportTraces());
        document.getElementById('auto-refresh-traces').addEventListener('change', (e) => this.toggleAutoRefresh(e.target.checked));
        document.getElementById('apply-trace-filters').addEventListener('click', () => this.applyFilters());
        document.getElementById('clear-trace-filters').addEventListener('click', () => this.clearFilters());
        document.getElementById('prev-trace-page').addEventListener('click', () => this.previousPage());
        document.getElementById('next-trace-page').addEventListener('click', () => this.nextPage());
    }

    /**
     * Load traces from API
     */
    async loadTraces() {
        try {
            const params = {
                limit: this.pageSize,
                offset: this.currentPage * this.pageSize,
                ...this.filters
            };

            const response = await this.apiClient.getTraces(params);
            this.traces = response.traces;
            this.renderTraces();
            this.updatePagination(response.total);
        } catch (error) {
            console.error('Failed to load traces:', error);
            this.showError('Failed to load traces');
        }
    }

    /**
     * Render traces list
     */
    renderTraces() {
        const container = document.getElementById('traces-list');

        if (this.traces.length === 0) {
            container.innerHTML = '<div class="empty-state">No traces found</div>';
            return;
        }

        container.innerHTML = this.traces.map(trace => this.renderTraceEntry(trace)).join('');

        // Attach click handlers
        container.querySelectorAll('.trace-entry').forEach((entry, index) => {
            entry.addEventListener('click', () => this.selectTrace(this.traces[index].trace_id));
        });
    }

    /**
     * Render a single trace entry
     */
    renderTraceEntry(trace) {
        const startTime = new Date(trace.start_time / 1000000); // Convert nanoseconds to milliseconds
        const duration = (trace.duration / 1000000).toFixed(2); // Convert to milliseconds
        const errorClass = trace.has_errors ? 'trace-error' : '';

        return `
            <div class="trace-entry ${errorClass}" data-trace-id="${trace.trace_id}">
                <div class="trace-header">
                    <span class="trace-time">${startTime.toLocaleTimeString()}</span>
                    <span class="trace-name">${this.escapeHtml(trace.root_span_name)}</span>
                    <span class="trace-duration">${duration}ms</span>
                    <span class="trace-spans">${trace.span_count} spans</span>
                    ${trace.has_errors ? '<span class="trace-error-badge">ERROR</span>' : ''}
                </div>
                <div class="trace-meta">
                    <span class="trace-id-short" title="${trace.trace_id}">${trace.trace_id.substring(0, 16)}...</span>
                    ${trace.service_names.length > 0 ? `<span class="trace-services">${trace.service_names.join(', ')}</span>` : ''}
                </div>
            </div>
        `;
    }

    /**
     * Select and display trace details
     */
    async selectTrace(traceId) {
        try {
            const trace = await this.apiClient.getTrace(traceId);
            this.selectedTrace = trace;
            this.renderTraceDetail(trace);
        } catch (error) {
            console.error('Failed to load trace details:', error);
            this.showError('Failed to load trace details');
        }
    }

    /**
     * Render trace detail view with waterfall
     */
    renderTraceDetail(trace) {
        const container = document.getElementById('trace-detail');
        container.style.display = 'block';

        const duration = (trace.duration / 1000000).toFixed(2);
        const startTime = new Date(trace.start_time / 1000000);

        // Build span tree
        const spanTree = this.buildSpanTree(trace.spans);

        container.innerHTML = `
            <div class="trace-detail-header">
                <h3>Trace Details</h3>
                <button id="close-trace-detail" class="btn btn-secondary">Close</button>
            </div>
            <div class="trace-info">
                <div class="trace-info-item"><strong>Trace ID:</strong> ${trace.trace_id}</div>
                <div class="trace-info-item"><strong>Start Time:</strong> ${startTime.toISOString()}</div>
                <div class="trace-info-item"><strong>Duration:</strong> ${duration}ms</div>
                <div class="trace-info-item"><strong>Spans:</strong> ${trace.span_count}</div>
                ${trace.service_names.length > 0 ? `<div class="trace-info-item"><strong>Services:</strong> ${trace.service_names.join(', ')}</div>` : ''}
            </div>
            <div class="trace-waterfall">
                <h4>Span Waterfall</h4>
                ${this.renderSpanTree(spanTree, trace.start_time, trace.duration)}
            </div>
        `;

        document.getElementById('close-trace-detail').addEventListener('click', () => {
            container.style.display = 'none';
            this.selectedTrace = null;
        });
    }

    /**
     * Build hierarchical span tree
     */
    buildSpanTree(spans) {
        const spanMap = new Map();
        const roots = [];

        // Create map of all spans
        spans.forEach(span => {
            spanMap.set(span.span_id, { ...span, children: [] });
        });

        // Build tree structure
        spans.forEach(span => {
            const node = spanMap.get(span.span_id);
            if (span.parent_span_id && spanMap.has(span.parent_span_id)) {
                spanMap.get(span.parent_span_id).children.push(node);
            } else {
                roots.push(node);
            }
        });

        return roots;
    }

    /**
     * Render span tree as waterfall
     */
    renderSpanTree(spans, traceStart, traceDuration, depth = 0) {
        return spans.map(span => {
            const startOffset = ((span.start_time - traceStart) / traceDuration) * 100;
            const width = ((span.duration) / traceDuration) * 100;
            const duration = (span.duration / 1000000).toFixed(2);
            const hasError = typeof span.status === 'string'
                ? span.status.toUpperCase() === 'ERROR'
                : span.status && span.status.code === 'Error';

            return `
                <div class="span-row" style="padding-left: ${depth * 20}px;">
                    <div class="span-info">
                        <span class="span-name ${hasError ? 'span-error' : ''}">${this.escapeHtml(span.name)}</span>
                        <span class="span-kind">${this.escapeHtml(String(span.kind ?? 'unknown'))}</span>
                        <span class="span-duration">${duration}ms</span>
                    </div>
                    <div class="span-bar-container">
                        <div class="span-bar ${hasError ? 'span-bar-error' : ''}"
                             style="left: ${startOffset}%; width: ${width}%;"
                             title="${this.escapeHtml(span.name)}: ${duration}ms">
                        </div>
                    </div>
                </div>
                ${this.renderSpanAttributes(span.attributes)}
                ${span.children.length > 0 ? this.renderSpanTree(span.children, traceStart, traceDuration, depth + 1) : ''}
            `;
        }).join('');
    }

    renderSpanAttributes(attributes) {
        const entries = Object.entries(attributes ?? {});
        if (entries.length === 0) {
            return '';
        }

        return `
            <div class="span-attributes">
                ${entries.map(([key, value]) => `
                    <div class="attribute-item">
                        <span class="attribute-key">${this.escapeHtml(key)}</span>
                        ${this.renderAttributeValue(value)}
                    </div>
                `).join('')}
            </div>
        `;
    }

    renderAttributeValue(value) {
        const formatted = this.tryFormatJsonString(value);
        if (formatted) {
            return `
                <div class="attribute-value attribute-value-json">
                    <span class="attribute-preview">${this.escapeHtml(formatted.preview)}</span>
                    <pre class="json-block"><code>${this.syntaxHighlightJson(formatted.pretty)}</code></pre>
                </div>
            `;
        }

        return `<span class="attribute-value">${this.escapeHtml(String(value))}</span>`;
    }

    tryFormatJsonString(value) {
        if (typeof value !== 'string') {
            return null;
        }

        try {
            const parsed = JSON.parse(value);
            const pretty = JSON.stringify(parsed, null, 2);
            return {
                preview: this.describeJsonValue(parsed),
                pretty
            };
        } catch (_error) {
            return null;
        }
    }

    describeJsonValue(value) {
        if (Array.isArray(value)) {
            return `[array, ${value.length} items]`;
        }

        if (value !== null && typeof value === 'object') {
            return `{object, ${Object.keys(value).length} keys}`;
        }

        return String(value);
    }

    syntaxHighlightJson(json) {
        return this.escapeHtml(json)
            .replace(/(&quot;(?:\\.|[^"\\])*&quot;)(\s*:)?/g, (match, stringToken, colon) => {
                if (colon) {
                    return `<span class="json-key">${stringToken}</span><span class="json-punctuation">:</span>`;
                }
                return `<span class="json-string">${stringToken}</span>`;
            })
            .replace(/\b(true|false)\b/g, '<span class="json-boolean">$1</span>')
            .replace(/\bnull\b/g, '<span class="json-null">null</span>')
            .replace(/(-?\b\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b)/g, '<span class="json-number">$1</span>');
    }

    /**
     * Apply filters
     */
    applyFilters() {
        this.filters.traceId = document.getElementById('trace-id-filter').value;
        this.filters.service = document.getElementById('service-filter').value;
        this.filters.search = document.getElementById('search-traces').value;
        this.currentPage = 0;
        this.loadTraces();
    }

    /**
     * Clear filters
     */
    clearFilters() {
        this.filters = {
            traceId: '',
            service: '',
            search: '',
            startTime: null,
            endTime: null
        };
        document.getElementById('trace-id-filter').value = '';
        document.getElementById('service-filter').value = '';
        document.getElementById('search-traces').value = '';
        this.currentPage = 0;
        this.loadTraces();
    }

    /**
     * Toggle auto-refresh
     */
    toggleAutoRefresh(enabled) {
        this.autoRefresh = enabled;

        if (enabled) {
            this.refreshInterval = setInterval(() => this.loadTraces(), 5000);
        } else {
            if (this.refreshInterval) {
                clearInterval(this.refreshInterval);
                this.refreshInterval = null;
            }
        }
    }

    /**
     * Export traces
     */
    async exportTraces() {
        try {
            const params = {
                format: 'json',
                ...this.filters
            };

            const blob = await this.apiClient.exportTraces(params);
            const url = window.URL.createObjectURL(blob);
            const a = document.createElement('a');
            a.href = url;
            a.download = 'traces.json';
            document.body.appendChild(a);
            a.click();
            window.URL.revokeObjectURL(url);
            document.body.removeChild(a);
        } catch (error) {
            console.error('Failed to export traces:', error);
            this.showError('Failed to export traces');
        }
    }

    /**
     * Previous page
     */
    previousPage() {
        if (this.currentPage > 0) {
            this.currentPage--;
            this.loadTraces();
        }
    }

    /**
     * Next page
     */
    nextPage() {
        this.currentPage++;
        this.loadTraces();
    }

    /**
     * Update pagination info
     */
    updatePagination(total) {
        const pageInfo = document.getElementById('trace-page-info');
        const totalPages = Math.ceil(total / this.pageSize);
        pageInfo.textContent = `Page ${this.currentPage + 1} of ${totalPages} (${total} total)`;

        document.getElementById('prev-trace-page').disabled = this.currentPage === 0;
        document.getElementById('next-trace-page').disabled = this.currentPage >= totalPages - 1;
    }

    /**
     * Show error message
     */
    showError(message) {
        const container = document.getElementById('traces-list');
        container.innerHTML = `<div class="error-message">${message}</div>`;
    }

    /**
     * Escape HTML to prevent XSS
     */
    escapeHtml(text) {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }

    /**
     * Cleanup when view is destroyed
     */
    destroy() {
        if (this.refreshInterval) {
            clearInterval(this.refreshInterval);
        }
    }
}

// Export for use in app.js
window.TracesView = TracesView;

// Made with Bob
