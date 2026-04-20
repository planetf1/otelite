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
                <div id="trace-detail" class="trace-detail">
                    <div class="empty-state" style="height:100%; display:flex; align-items:center; justify-content:center; color:var(--text-secondary);">
                        Select a trace to view details
                    </div>
                </div>
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
            // Mark the selected entry
            document.querySelectorAll('.trace-entry').forEach(el => {
                el.classList.toggle('selected', el.dataset.traceId === traceId);
            });
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

        const duration = (trace.duration / 1000000).toFixed(2);
        const startTime = new Date(trace.start_time / 1000000);

        // Build span tree
        const spanTree = this.buildSpanTree(trace.spans);

        container.innerHTML = `
            <div class="trace-detail-body">
                <div class="trace-detail-header">
                    <h3>${this.escapeHtml(trace.root_span_name ?? 'Trace Details')}</h3>
                    <span class="trace-duration">${duration}ms · ${trace.span_count} spans</span>
                </div>
                <div class="trace-info">
                    <div class="trace-info-item"><strong>Trace ID:</strong> <code>${trace.trace_id}</code></div>
                    <div class="trace-info-item"><strong>Start:</strong> ${startTime.toISOString()}</div>
                    ${trace.service_names.length > 0 ? `<div class="trace-info-item"><strong>Services:</strong> ${this.escapeHtml(trace.service_names.join(', '))}</div>` : ''}
                </div>
                <div class="trace-waterfall">
                    <div class="span-kind-legend">
                        <span class="span-kind-legend-item"><span class="span-kind-dot" style="background:#3b82f6"></span>server</span>
                        <span class="span-kind-legend-item"><span class="span-kind-dot" style="background:#a855f7"></span>client</span>
                        <span class="span-kind-legend-item"><span class="span-kind-dot" style="background:#6366f1"></span>internal</span>
                        <span class="span-kind-legend-item"><span class="span-kind-dot" style="background:#f59e0b"></span>producer</span>
                        <span class="span-kind-legend-item"><span class="span-kind-dot" style="background:#22c55e"></span>consumer</span>
                        <span class="span-kind-legend-item"><span class="span-kind-dot" style="background:#ef4444"></span>error</span>
                        <span style="margin-left:auto;font-size:0.72rem;color:var(--text-secondary)">Click a span bar for details</span>
                    </div>
                    <div class="waterfall-spans">
                        ${this.renderSpanTree(spanTree, trace.start_time, trace.duration)}
                    </div>
                </div>
            </div>
            <div id="span-detail-panel" class="span-detail-panel" style="display: none;"></div>
        `;

        // Attach click handlers to span bars
        this.attachSpanClickHandlers(trace);
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
     * Map span kind number/string to a CSS class name
     */
    spanKindClass(kind) {
        const kindNames = ['unspecified', 'internal', 'server', 'client', 'producer', 'consumer'];
        if (typeof kind === 'number') {
            return `span-kind-${kindNames[kind] ?? 'unspecified'}`;
        }
        return `span-kind-${String(kind).toLowerCase()}`;
    }

    /**
     * Render span tree as waterfall
     */
    renderSpanTree(spans, traceStart, traceDuration, depth = 0) {
        return spans.map(span => {
            const startOffset = ((span.start_time - traceStart) / traceDuration) * 100;
            const width = Math.max((span.duration / traceDuration) * 100, 0.5);
            const duration = (span.duration / 1000000).toFixed(2);
            const hasError = typeof span.status === 'string'
                ? span.status.toUpperCase() === 'ERROR'
                : span.status && span.status.code === 'Error';
            const kindClass = this.spanKindClass(span.kind);
            const kindLabel = typeof span.kind === 'number'
                ? ['?', 'internal', 'server', 'client', 'producer', 'consumer'][span.kind] ?? '?'
                : String(span.kind ?? '?');

            return `
                <div class="span-row" style="padding-left: ${depth * 16 + 4}px;">
                    <div class="span-info">
                        <span class="span-name ${hasError ? 'span-error' : ''}" title="${this.escapeHtml(span.name)}">${this.escapeHtml(span.name)}</span>
                        <span class="span-kind">${this.escapeHtml(kindLabel)}</span>
                        <span class="span-duration">${duration}ms</span>
                    </div>
                    <div class="span-bar-container">
                        <div class="span-bar ${hasError ? 'span-bar-error' : kindClass}"
                             style="left: ${startOffset}%; width: ${width}%;"
                             data-span-id="${span.span_id}"
                             title="${this.escapeHtml(span.name)}: ${duration}ms">
                        </div>
                    </div>
                </div>
                ${span.children.length > 0 ? this.renderSpanTree(span.children, traceStart, traceDuration, depth + 1) : ''}
            `;
        }).join('');
    }

    renderSpanAttributes(attributes) {
        const entries = Object.entries(attributes ?? {});
        if (entries.length === 0) {
            return '';
        }

        // Check for GenAI attributes
        const genaiInfo = this.extractGenAiInfo(attributes);
        const genaiSection = genaiInfo ? this.renderGenAiInfo(genaiInfo) : '';

        return `
            ${genaiSection}
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

    /**
     * Extract GenAI/LLM information from span attributes
     */
    extractGenAiInfo(attributes) {
        // Check if any gen_ai.* attributes exist
        const hasGenAi = Object.keys(attributes).some(key => key.startsWith('gen_ai.'));
        if (!hasGenAi) {
            return null;
        }

        const info = {
            system: attributes['gen_ai.system'],
            model: attributes['gen_ai.request.model'],
            operation: attributes['gen_ai.operation.name'],
            inputTokens: attributes['gen_ai.usage.input_tokens'] ? parseInt(attributes['gen_ai.usage.input_tokens']) : null,
            outputTokens: attributes['gen_ai.usage.output_tokens'] ? parseInt(attributes['gen_ai.usage.output_tokens']) : null,
            totalTokens: attributes['gen_ai.usage.total_tokens'] ? parseInt(attributes['gen_ai.usage.total_tokens']) : null,
            temperature: attributes['gen_ai.request.temperature'] ? parseFloat(attributes['gen_ai.request.temperature']) : null,
            maxTokens: attributes['gen_ai.request.max_tokens'] ? parseInt(attributes['gen_ai.request.max_tokens']) : null,
            finishReasons: this.parseFinishReasons(attributes['gen_ai.response.finish_reasons'])
        };

        // Calculate total tokens if not provided
        if (!info.totalTokens && info.inputTokens && info.outputTokens) {
            info.totalTokens = info.inputTokens + info.outputTokens;
        }

        return info;
    }

    /**
     * Parse finish reasons from string (JSON array or comma-separated)
     */
    parseFinishReasons(value) {
        if (!value) return [];

        try {
            // Try parsing as JSON array
            return JSON.parse(value);
        } catch {
            // Fall back to comma-separated
            return value.split(',').map(s => s.trim().replace(/^"|"$/g, '')).filter(s => s);
        }
    }

    /**
     * Render GenAI information card
     */
    renderGenAiInfo(info) {
        const systemName = this.getSystemDisplayName(info.system);
        const tokenUsage = this.formatTokenUsage(info);

        return `
            <div class="genai-info-card">
                <div class="genai-header">
                    <span class="genai-badge">🤖 GenAI/LLM</span>
                    ${systemName ? `<span class="genai-system">[${this.escapeHtml(systemName)}]</span>` : ''}
                </div>
                <div class="genai-details">
                    ${info.model ? `<div class="genai-detail-item"><strong>Model:</strong> ${this.escapeHtml(info.model)}</div>` : ''}
                    ${info.operation ? `<div class="genai-detail-item"><strong>Operation:</strong> ${this.escapeHtml(info.operation)}</div>` : ''}
                    ${tokenUsage ? `<div class="genai-detail-item"><strong>Tokens:</strong> <span class="genai-tokens">${tokenUsage}</span></div>` : ''}
                    ${info.temperature !== null ? `<div class="genai-detail-item"><strong>Temperature:</strong> ${info.temperature.toFixed(2)}</div>` : ''}
                    ${info.maxTokens ? `<div class="genai-detail-item"><strong>Max Tokens:</strong> ${info.maxTokens.toLocaleString()}</div>` : ''}
                    ${info.finishReasons.length > 0 ? `<div class="genai-detail-item"><strong>Finish Reasons:</strong> ${info.finishReasons.join(', ')}</div>` : ''}
                </div>
            </div>
        `;
    }

    /**
     * Get display name for GenAI system
     */
    getSystemDisplayName(system) {
        if (!system) return null;

        const names = {
            'openai': 'OpenAI',
            'anthropic': 'Anthropic',
            'azure_openai': 'Azure OpenAI',
            'google': 'Google',
            'cohere': 'Cohere'
        };

        return names[system] || system.charAt(0).toUpperCase() + system.slice(1);
    }

    /**
     * Format token usage for display
     */
    formatTokenUsage(info) {
        if (info.inputTokens && info.outputTokens) {
            return `Input: ${info.inputTokens.toLocaleString()} | Output: ${info.outputTokens.toLocaleString()} | Total: ${info.totalTokens.toLocaleString()}`;
        } else if (info.totalTokens) {
            return `Total: ${info.totalTokens.toLocaleString()}`;
        }
        return null;
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
     * Attach click handlers to span bars
     */
    attachSpanClickHandlers(trace) {
        const spanBars = document.querySelectorAll('.span-bar');
        spanBars.forEach((bar, index) => {
            bar.style.cursor = 'pointer';
            bar.addEventListener('click', (e) => {
                e.stopPropagation();
                const spanId = bar.getAttribute('data-span-id');
                const span = trace.spans.find(s => s.span_id === spanId);
                if (span) {
                    this.showSpanDetail(span, trace);
                }
            });
        });
    }

    /**
     * Show detailed information for a span
     */
    showSpanDetail(span, trace) {
        const panel = document.getElementById('span-detail-panel');
        panel.style.display = 'flex';

        const duration = (span.duration / 1000000).toFixed(2);
        const startTime = new Date(span.start_time / 1000000);
        const hasError = typeof span.status === 'string'
            ? span.status.toUpperCase() === 'ERROR'
            : span.status && span.status.code === 'Error';

        const parent = span.parent_span_id
            ? trace.spans.find(s => s.span_id === span.parent_span_id)
            : null;
        const children = trace.spans.filter(s => s.parent_span_id === span.span_id);

        const kindLabel = typeof span.kind === 'number'
            ? ['?', 'internal', 'server', 'client', 'producer', 'consumer'][span.kind] ?? '?'
            : String(span.kind ?? '?');

        const attrEntries = Object.entries(span.attributes || {});
        const genaiInfo = this.extractGenAiInfo(span.attributes || {});

        const scrollContent = this.buildSpanDetailHTML(span, trace, {
            duration, startTime, hasError, parent, children, kindLabel, attrEntries, genaiInfo
        });

        panel.innerHTML = `
            <div class="span-panel-drag-handle" id="span-panel-drag"></div>
            <div class="span-detail-scroll">
                <div class="span-detail-header-row">
                    <h4 title="${this.escapeHtml(span.name)}">
                        <span class="span-kind">${this.escapeHtml(kindLabel)}</span>
                        ${this.escapeHtml(span.name)}
                        <span class="${hasError ? 'status-error' : 'status-ok'}" style="font-size:0.8rem;font-weight:400;margin-left:0.5rem;">${hasError ? '⚠ ERROR' : '✓ OK'}</span>
                    </h4>
                    <div style="display:flex;gap:0.5rem;flex-shrink:0;">
                        <button id="expand-span-detail" class="btn btn-secondary btn-sm" title="Full screen">&#x26F6;</button>
                        <button id="close-span-detail" class="btn btn-secondary btn-sm" title="Close">×</button>
                    </div>
                </div>
                ${scrollContent}
            </div>
        `;

        document.getElementById('close-span-detail').addEventListener('click', () => {
            panel.style.display = 'none';
        });

        document.getElementById('expand-span-detail').addEventListener('click', () => {
            this.openSpanModal(span, trace, { duration, startTime, hasError, parent, children, kindLabel, attrEntries, genaiInfo });
        });

        this.attachDragResize(panel, document.getElementById('span-panel-drag'));
    }

    buildSpanDetailHTML(span, trace, { duration, startTime, hasError, parent, children, kindLabel, attrEntries, genaiInfo }) {
        return `
            <div class="span-detail-section">
                <h5>Info</h5>
                <div class="span-attrs-grid">
                    <div class="span-attr-row"><span class="span-attr-key">duration</span><span class="span-attr-val">${duration}ms</span></div>
                    <div class="span-attr-row"><span class="span-attr-key">start</span><span class="span-attr-val">${startTime.toISOString()}</span></div>
                    <div class="span-attr-row"><span class="span-attr-key">span_id</span><span class="span-attr-val">${span.span_id}</span></div>
                    <div class="span-attr-row"><span class="span-attr-key">trace_id</span><span class="span-attr-val">${span.trace_id}</span></div>
                    ${parent ? `<div class="span-attr-row"><span class="span-attr-key">parent</span><span class="span-attr-val">${this.escapeHtml(parent.name)}</span></div>` : ''}
                    ${children.length > 0 ? `<div class="span-attr-row" style="grid-column:1/-1"><span class="span-attr-key">children</span><span class="span-attr-val">${children.map(c => this.escapeHtml(c.name)).join(', ')}</span></div>` : ''}
                </div>
            </div>

            ${genaiInfo ? `<div class="span-detail-section">${this.renderGenAiInfo(genaiInfo)}</div>` : ''}

            ${attrEntries.length > 0 ? `
                <div class="span-detail-section">
                    <h5>Attributes (${attrEntries.length})</h5>
                    <div class="span-attrs-grid">
                        ${attrEntries.map(([k, v]) => {
                            const isLong = String(v).length > 80;
                            return `<div class="span-attr-row${isLong ? ' ' : ''}" ${isLong ? 'style="grid-column:1/-1"' : ''}>
                                <span class="span-attr-key">${this.escapeHtml(k)}</span>
                                <span class="span-attr-val${isLong ? ' long' : ''}">${this.escapeHtml(String(v))}</span>
                            </div>`;
                        }).join('')}
                    </div>
                </div>
            ` : ''}

            ${span.events && span.events.length > 0 ? `
                <div class="span-detail-section">
                    <h5>Events (${span.events.length})</h5>
                    <div class="span-events-timeline">${this.renderSpanEvents(span.events, span.start_time)}</div>
                </div>
            ` : ''}
        `;
    }

    /**
     * Drag-to-resize the span detail panel
     */
    attachDragResize(panel, handle) {
        let startY, startH;
        handle.addEventListener('mousedown', e => {
            startY = e.clientY;
            startH = panel.offsetHeight;
            handle.classList.add('dragging');
            const onMove = e => {
                const delta = startY - e.clientY; // drag up = taller
                panel.style.height = Math.max(120, Math.min(window.innerHeight * 0.85, startH + delta)) + 'px';
            };
            const onUp = () => {
                handle.classList.remove('dragging');
                document.removeEventListener('mousemove', onMove);
                document.removeEventListener('mouseup', onUp);
            };
            document.addEventListener('mousemove', onMove);
            document.addEventListener('mouseup', onUp);
            e.preventDefault();
        });
    }

    /**
     * Open span details in a full-screen modal overlay
     */
    openSpanModal(span, trace, context) {
        const existing = document.getElementById('span-modal');
        if (existing) existing.remove();

        const modal = document.createElement('div');
        modal.id = 'span-modal';
        modal.style.cssText = `
            position: fixed; inset: 0; z-index: 9999;
            background: rgba(0,0,0,0.85);
            display: flex; align-items: center; justify-content: center;
            padding: 2rem;
        `;

        const box = document.createElement('div');
        box.style.cssText = `
            background: var(--bg-secondary);
            border: 1px solid var(--accent-color);
            border-radius: 8px;
            width: 100%; max-width: 1100px;
            max-height: 90vh;
            display: flex; flex-direction: column;
            overflow: hidden;
        `;

        const { duration, startTime, hasError, parent, children, kindLabel, attrEntries, genaiInfo } = context;
        box.innerHTML = `
            <div style="display:flex;align-items:center;justify-content:space-between;padding:1rem 1.25rem;border-bottom:1px solid var(--border-color);flex-shrink:0;">
                <h3 style="font-size:1rem;font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;" title="${this.escapeHtml(span.name)}">
                    <span class="span-kind">${this.escapeHtml(kindLabel)}</span>
                    ${this.escapeHtml(span.name)}
                    <span class="${hasError ? 'status-error' : 'status-ok'}" style="font-size:0.85rem;font-weight:400;margin-left:0.5rem;">${hasError ? '⚠ ERROR' : '✓ OK'}</span>
                </h3>
                <button id="close-span-modal" class="btn btn-secondary btn-sm" style="flex-shrink:0;">× Close</button>
            </div>
            <div style="flex:1;overflow-y:auto;padding:1.25rem;">
                ${this.buildSpanDetailHTML(span, trace, context)}
            </div>
        `;

        modal.appendChild(box);
        document.body.appendChild(modal);

        document.getElementById('close-span-modal').addEventListener('click', () => modal.remove());
        modal.addEventListener('click', e => { if (e.target === modal) modal.remove(); });
    }

    /**
     * Render span events as a timeline
     */
    renderSpanEvents(events, spanStartTime) {
        return events.map(event => {
            const eventTime = new Date(event.time / 1000000);
            const offsetMs = ((event.time - spanStartTime) / 1000000).toFixed(2);

            return `
                <div class="span-event">
                    <div class="span-event-header">
                        <span class="span-event-name">${this.escapeHtml(event.name)}</span>
                        <span class="span-event-time">+${offsetMs}ms</span>
                    </div>
                    ${Object.keys(event.attributes || {}).length > 0 ? `
                        <div class="span-event-attributes">
                            ${Object.entries(event.attributes).map(([key, value]) => `
                                <div class="attribute-item">
                                    <span class="attribute-key">${this.escapeHtml(key)}:</span>
                                    <span class="attribute-value">${this.escapeHtml(String(value))}</span>
                                </div>
                            `).join('')}
                        </div>
                    ` : ''}
                </div>
            `;
        }).join('');
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
