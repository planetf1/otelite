/**
 * Logs View - Display and interact with log records
 */

class LogsView {
    constructor(apiClient) {
        this.apiClient = apiClient;
        this.logs = [];
        this.filters = {
            severity: '',
            resource: '',
            search: '',
            startTime: null,
            endTime: null
        };
        this.attrFilters = [];
        this.trStart = null;
        this.trEnd = null;
        this.trWindowHours = null;
        this.currentPage = 0;
        this.pageSize = 100;
        this.autoRefresh = false;
        this.refreshInterval = null;
        this.hasGenAiData = false;
        this.llmView = localStorage.getItem('rotel_llm_view') === 'true';
    }

    /**
     * Render the logs view
     */
    render() {
        const container = document.getElementById('logs-view');
        container.innerHTML = `
            <div class="view-header">
                <h2>Logs</h2>
                <div class="view-actions">
                    <button id="refresh-logs" class="btn btn-primary">Refresh</button>
                    <button id="export-logs-json" class="btn btn-secondary">Export JSON</button>
                    <button id="export-logs-csv" class="btn btn-secondary">Export CSV</button>
                    <label class="auto-refresh-toggle">
                        <input type="checkbox" id="auto-refresh-logs">
                        Auto-refresh (5s)
                    </label>
                </div>
            </div>

            <div class="filters">
                <input type="text" id="search-logs" placeholder="Search logs..." class="filter-input">
                <select id="severity-filter" class="filter-select">
                    <option value="">All Severities</option>
                    <option value="TRACE">TRACE</option>
                    <option value="DEBUG">DEBUG</option>
                    <option value="INFO">INFO</option>
                    <option value="WARN">WARN</option>
                    <option value="ERROR">ERROR</option>
                    <option value="FATAL">FATAL</option>
                </select>
                <div class="time-range-bar">
                    <button class="btn-icon" id="tr-prev-logs" title="Previous window">&#8592;</button>
                    <input type="datetime-local" id="tr-start-logs" class="filter-input tr-datetime">
                    <span class="tr-sep">–</span>
                    <input type="datetime-local" id="tr-end-logs" class="filter-input tr-datetime">
                    <button class="btn-icon" id="tr-next-logs" title="Next window">&#8594;</button>
                    <button class="btn-icon" id="tr-now-logs" title="Jump to now">Now</button>
                    <select id="tr-preset-logs" class="filter-select tr-preset">
                        <option value="">Custom</option>
                        <option value="0.25">15 min</option>
                        <option value="1">1 hr</option>
                        <option value="6">6 hr</option>
                        <option value="24">24 hr</option>
                        <option value="168">7 days</option>
                        <option value="720">30 days</option>
                    </select>
                </div>
                <datalist id="logs-resource-keys-list"></datalist>
                <input type="text" id="resource-filter" placeholder="Resource filter (e.g., service.name=my-service)" class="filter-input" list="logs-resource-keys-list">
                <button id="apply-filters" class="btn btn-primary">Apply Filters</button>
                <button id="clear-filters" class="btn btn-secondary">Clear</button>
            </div>

            <div class="attr-filter-bar" id="attr-filter-bar-logs">
                <button id="quick-filter-error-logs" class="btn btn-secondary btn-sm">ERROR</button>
                <button id="llm-view-toggle" class="btn btn-secondary btn-sm hidden">LLM View</button>
                <input type="text" id="attr-key-logs" placeholder="attribute key" class="filter-input attr-filter-key" list="attr-keys-logs-list">
                <datalist id="attr-keys-logs-list"></datalist>
                <select id="attr-op-logs" class="filter-select attr-filter-op">
                    <option value="=">=</option>
                    <option value="!=">&#8800;</option>
                    <option value="exists">exists</option>
                    <option value="!exists">!exists</option>
                </select>
                <input type="text" id="attr-val-logs" placeholder="value" class="filter-input attr-filter-val">
                <button id="add-attr-filter-logs" class="btn btn-primary btn-sm">+ Add</button>
                <div id="attr-chips-logs" class="attr-chips"></div>
            </div>

            <div id="logs-list" class="logs-list"></div>

            <div class="pagination">
                <button id="prev-page" class="btn btn-secondary">Previous</button>
                <span id="page-info">Page 1</span>
                <button id="next-page" class="btn btn-secondary">Next</button>
            </div>
        `;

        this.attachEventListeners();
        this.loadLogs();
        this.loadResourceKeys();
    }

    /**
     * Attach event listeners
     */
    attachEventListeners() {
        document.getElementById('refresh-logs').addEventListener('click', () => this.loadLogs());
        document.getElementById('export-logs-json').addEventListener('click', () => this.exportLogs('json'));
        document.getElementById('export-logs-csv').addEventListener('click', () => this.exportLogs('csv'));
        document.getElementById('auto-refresh-logs').addEventListener('change', (e) => this.toggleAutoRefresh(e.target.checked));
        document.getElementById('apply-filters').addEventListener('click', () => this.applyFilters());
        document.getElementById('clear-filters').addEventListener('click', () => this.clearFilters());
        document.getElementById('prev-page').addEventListener('click', () => this.previousPage());
        document.getElementById('next-page').addEventListener('click', () => this.nextPage());

        // Real-time search
        document.getElementById('search-logs').addEventListener('input', (e) => {
            this.filters.search = e.target.value;
            this.debounceLoadLogs();
        });

        // Time-range bar
        document.getElementById('tr-preset-logs').addEventListener('change', (e) => {
            const hours = e.target.value ? parseFloat(e.target.value) : null;
            if (hours !== null) {
                const now = new Date();
                this.trEnd = now;
                this.trStart = new Date(now.getTime() - hours * 3600000);
                this.trWindowHours = hours;
                this._syncDateInputs('logs');
            } else {
                this.trStart = null;
                this.trEnd = null;
                this.trWindowHours = null;
                this._syncDateInputs('logs');
            }
            this.currentPage = 0;
            this.loadLogs();
        });

        document.getElementById('tr-start-logs').addEventListener('change', () => this._onDateInputChange('logs'));
        document.getElementById('tr-end-logs').addEventListener('change', () => this._onDateInputChange('logs'));

        document.getElementById('tr-prev-logs').addEventListener('click', () => {
            const window = (this.trWindowHours || 1) * 3600000;
            const end = (this.trEnd || new Date()).getTime() - window;
            const start = (this.trStart ? this.trStart.getTime() : end - window) - window;
            this.trEnd = new Date(end);
            this.trStart = new Date(start);
            this._syncDateInputs('logs');
            document.getElementById('tr-preset-logs').value = '';
            this.currentPage = 0;
            this.loadLogs();
        });

        document.getElementById('tr-next-logs').addEventListener('click', () => {
            const now = Date.now();
            const window = (this.trWindowHours || 1) * 3600000;
            let end = (this.trEnd || new Date()).getTime() + window;
            if (end > now) end = now;
            this.trEnd = new Date(end);
            this.trStart = new Date(end - window);
            this._syncDateInputs('logs');
            document.getElementById('tr-preset-logs').value = '';
            this.currentPage = 0;
            this.loadLogs();
        });

        document.getElementById('tr-now-logs').addEventListener('click', () => {
            const now = new Date();
            const window = (this.trWindowHours || 1) * 3600000;
            this.trEnd = now;
            this.trStart = new Date(now.getTime() - window);
            this._syncDateInputs('logs');
            document.getElementById('tr-preset-logs').value = '';
            this.currentPage = 0;
            this.loadLogs();
        });

        // Attribute filter bar
        document.getElementById('add-attr-filter-logs').addEventListener('click', () => this._addAttrFilter());
        document.getElementById('attr-val-logs').addEventListener('keydown', (e) => {
            if (e.key === 'Enter') this._addAttrFilter();
        });
        document.getElementById('quick-filter-error-logs').addEventListener('click', () => {
            this.attrFilters.push({ key: 'severity', op: '=', value: 'ERROR' });
            this._renderAttrChips();
            this.renderLogs();
        });

        document.getElementById('llm-view-toggle').addEventListener('click', () => {
            this.llmView = !this.llmView;
            localStorage.setItem('rotel_llm_view', this.llmView ? 'true' : 'false');
            this._updateLlmToggleButton();
            this.renderLogs();
        });
    }

    _syncDateInputs(suffix) {
        const startEl = document.getElementById(`tr-start-${suffix}`);
        const endEl = document.getElementById(`tr-end-${suffix}`);
        if (startEl) startEl.value = this.trStart ? this._toDatetimeLocal(this.trStart) : '';
        if (endEl) endEl.value = this.trEnd ? this._toDatetimeLocal(this.trEnd) : '';
    }

    _toDatetimeLocal(date) {
        // Format as YYYY-MM-DDTHH:MM (datetime-local value format, local time)
        const pad = n => String(n).padStart(2, '0');
        return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
    }

    _onDateInputChange(suffix) {
        const startEl = document.getElementById(`tr-start-${suffix}`);
        const endEl = document.getElementById(`tr-end-${suffix}`);
        const startVal = startEl ? startEl.value : '';
        const endVal = endEl ? endEl.value : '';
        this.trStart = startVal ? new Date(startVal) : null;
        this.trEnd = endVal ? new Date(endVal) : null;
        if (this.trStart && this.trEnd) {
            this.trWindowHours = (this.trEnd.getTime() - this.trStart.getTime()) / 3600000;
        }
        const presetEl = document.getElementById(`tr-preset-${suffix}`);
        if (presetEl) presetEl.value = '';
        this.currentPage = 0;
        this.loadLogs();
    }

    /**
     * Populate the resource-keys datalist for typeahead
     */
    async loadResourceKeys() {
        try {
            const response = await this.apiClient.getResourceKeys('logs');
            const datalist = document.getElementById('logs-resource-keys-list');
            if (!datalist) return;
            datalist.innerHTML = response.keys
                .map(k => `<option value="${k}=">`)
                .join('');
        } catch (_error) {
            // Non-critical; silently ignore
        }
    }

    /**
     * Load logs from API
     */
    async loadLogs() {
        try {
            const params = {
                limit: this.pageSize,
                offset: this.currentPage * this.pageSize,
                ...this.filters
            };

            if (this.trStart !== null) {
                params.start_time = this.trStart.getTime() * 1_000_000;
                params.end_time = (this.trEnd || new Date()).getTime() * 1_000_000;
            }

            const response = await this.apiClient.getLogs(params);
            this.logs = response.logs;
            this.hasGenAiData = this.logs.some(log =>
                log.attributes && Object.keys(log.attributes).some(k => k.startsWith('gen_ai.'))
            );
            this._updateLlmToggleButton();
            this.renderLogs();
            this.updatePagination(response.total);
        } catch (error) {
            console.error('Failed to load logs:', error);
            this.showError('Failed to load logs');
        }
    }

    /**
     * Render logs list
     */
    renderLogs() {
        const container = document.getElementById('logs-list');

        // Populate attr key autocomplete from loaded data
        this._updateAttrKeyDatalist();

        // Apply client-side attribute filters
        const displayLogs = this.attrFilters.length > 0
            ? this.logs.filter(log => this._matchesAttrFilters(log))
            : this.logs;

        if (displayLogs.length === 0) {
            container.innerHTML = '<div class="empty-state">No logs found</div>';
            return;
        }

        const useLlm = this.llmView && this.hasGenAiData;
        container.innerHTML = displayLogs.map(log => this.renderLogEntry(log, useLlm)).join('');

        // Attach click handlers for expansion
        container.querySelectorAll('.log-entry').forEach((entry, index) => {
            entry.addEventListener('click', () => this.toggleLogExpansion(index));
        });
    }

    /**
     * Render a single log entry
     */
    renderLogEntry(log, useLlm) {
        const timestamp = new Date(log.timestamp / 1000000); // Convert nanoseconds to milliseconds
        const severityClass = `severity-${log.severity.toLowerCase()}`;
        const attrs = log.attributes || {};
        const bodyPreview = this.escapeHtml(log.body.substring(0, 100)) + (log.body.length > 100 ? '...' : '');

        let headerCols;
        if (useLlm) {
            const model = attrs['gen_ai.request.model'] || attrs['gen_ai.response.model'] || '—';
            const rawInput = attrs['gen_ai.usage.input_tokens'];
            const rawOutput = attrs['gen_ai.usage.output_tokens'];
            const inputTokens = rawInput != null ? Number(rawInput).toLocaleString() : '—';
            const outputTokens = rawOutput != null ? Number(rawOutput).toLocaleString() : '—';
            const finishReasonsRaw = attrs['gen_ai.response.finish_reasons'];
            const finishReason = finishReasonsRaw != null
                ? (Array.isArray(finishReasonsRaw) ? finishReasonsRaw.join(', ') : String(finishReasonsRaw))
                : (attrs['gen_ai.response.finish_reason'] || '—');
            headerCols = `
                    <span class="log-timestamp">${timestamp.toISOString()}</span>
                    <span class="log-severity ${severityClass}">${log.severity}</span>
                    <span class="log-col-model" title="${this.escapeHtml(model)}">${this.escapeHtml(String(model))}</span>
                    <span class="log-col-tokens">${this.escapeHtml(inputTokens)}</span>
                    <span class="log-col-tokens">${this.escapeHtml(outputTokens)}</span>
                    <span class="log-col-tokens">${this.escapeHtml(String(finishReason))}</span>
                    <span class="log-body-preview">${bodyPreview}</span>`;
        } else {
            headerCols = `
                    <span class="log-timestamp">${timestamp.toISOString()}</span>
                    <span class="log-severity ${severityClass}">${log.severity}</span>
                    <span class="log-body-preview">${bodyPreview}</span>
                    ${log.trace_id ? `<span class="log-trace-id" title="Trace ID">${log.trace_id.substring(0, 8)}...</span>` : ''}`;
        }

        return `
            <div class="log-entry ${severityClass}" data-timestamp="${log.timestamp}">
                <div class="log-header">
                    ${headerCols}
                </div>
                <div class="log-details" style="display: none;">
                    <div class="log-body">${this.escapeHtml(log.body)}</div>
                    ${log.trace_id ? `<div class="log-field"><strong>Trace ID:</strong> ${log.trace_id}</div>` : ''}
                    ${log.span_id ? `<div class="log-field"><strong>Span ID:</strong> ${log.span_id}</div>` : ''}
                    ${Object.keys(attrs).length > 0 ? `
                        <div class="log-field">
                            <strong>Attributes:</strong>
                            ${this.renderAttributeMap(attrs)}
                        </div>
                    ` : ''}
                    ${log.resource ? `
                        <div class="log-field">
                            <strong>Resource:</strong>
                            ${this.renderJsonBlock(log.resource)}
                        </div>
                    ` : ''}
                </div>
            </div>
        `;
    }

    renderAttributeMap(attributes) {
        const entries = Object.entries(attributes);
        if (entries.length === 0) {
            return '';
        }

        return `
            <div class="attribute-list">
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

    renderJsonBlock(value) {
        const pretty = JSON.stringify(value, null, 2);
        return `<pre class="json-block"><code>${this.syntaxHighlightJson(pretty)}</code></pre>`;
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
     * Toggle log entry expansion
     */
    toggleLogExpansion(index) {
        const entries = document.querySelectorAll('.log-entry');
        const entry = entries[index];
        const details = entry.querySelector('.log-details');

        if (details.style.display === 'none') {
            details.style.display = 'block';
            entry.classList.add('expanded');
        } else {
            details.style.display = 'none';
            entry.classList.remove('expanded');
        }
    }

    /**
     * Apply filters
     */
    applyFilters() {
        this.filters.severity = document.getElementById('severity-filter').value;
        this.filters.resource = document.getElementById('resource-filter').value;
        this.filters.search = document.getElementById('search-logs').value;
        // Time range state is managed live by the time-range-bar controls
        this.currentPage = 0;
        this.loadLogs();
    }

    /**
     * Clear filters
     */
    clearFilters() {
        this.filters = {
            severity: '',
            resource: '',
            search: '',
            startTime: null,
            endTime: null
        };
        this.attrFilters = [];
        this._renderAttrChips();
        this.trStart = null;
        this.trEnd = null;
        this.trWindowHours = null;
        document.getElementById('severity-filter').value = '';
        document.getElementById('resource-filter').value = '';
        document.getElementById('search-logs').value = '';
        document.getElementById('tr-preset-logs').value = '';
        this._syncDateInputs('logs');
        this.currentPage = 0;
        this.loadLogs();
    }

    /**
     * Toggle auto-refresh
     */
    toggleAutoRefresh(enabled) {
        this.autoRefresh = enabled;

        if (enabled) {
            this.refreshInterval = setInterval(() => this.loadLogs(), 5000);
        } else {
            if (this.refreshInterval) {
                clearInterval(this.refreshInterval);
                this.refreshInterval = null;
            }
        }
    }

    /**
     * Export logs in the given format ('json' or 'csv')
     */
    async exportLogs(format) {
        try {
            const params = { format, ...this.filters };
            const blob = await this.apiClient.exportLogs(params);
            const url = window.URL.createObjectURL(blob);
            const a = document.createElement('a');
            a.href = url;
            a.download = `logs.${format}`;
            document.body.appendChild(a);
            a.click();
            window.URL.revokeObjectURL(url);
            document.body.removeChild(a);
        } catch (error) {
            console.error('Failed to export logs:', error);
            this.showError('Failed to export logs');
        }
    }

    /**
     * Previous page
     */
    previousPage() {
        if (this.currentPage > 0) {
            this.currentPage--;
            this.loadLogs();
        }
    }

    /**
     * Next page
     */
    nextPage() {
        this.currentPage++;
        this.loadLogs();
    }

    /**
     * Update pagination info
     */
    updatePagination(total) {
        const pageInfo = document.getElementById('page-info');
        const totalPages = Math.ceil(total / this.pageSize);
        pageInfo.textContent = `Page ${this.currentPage + 1} of ${totalPages} (${total} total)`;

        document.getElementById('prev-page').disabled = this.currentPage === 0;
        document.getElementById('next-page').disabled = this.currentPage >= totalPages - 1;
    }

    /**
     * Debounced load logs (for real-time search)
     */
    debounceLoadLogs() {
        clearTimeout(this.debounceTimer);
        this.debounceTimer = setTimeout(() => {
            this.currentPage = 0;
            this.loadLogs();
        }, 300);
    }

    /**
     * Show error message
     */
    showError(message) {
        const container = document.getElementById('logs-list');
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
     * Add an attribute filter from the input row
     */
    _addAttrFilter() {
        const key = document.getElementById('attr-key-logs').value.trim();
        const op = document.getElementById('attr-op-logs').value;
        const value = document.getElementById('attr-val-logs').value.trim();
        if (!key) return;
        if ((op === '=' || op === '!=') && value === '') return;
        this.attrFilters.push({ key, op, value });
        document.getElementById('attr-key-logs').value = '';
        document.getElementById('attr-val-logs').value = '';
        document.getElementById('attr-op-logs').value = '=';
        this._renderAttrChips();
        this.renderLogs();
    }

    /**
     * Render the attribute filter chips
     */
    _renderAttrChips() {
        const container = document.getElementById('attr-chips-logs');
        if (!container) return;
        container.innerHTML = this.attrFilters.map((f, i) => {
            const label = (f.op === 'exists' || f.op === '!exists')
                ? `${f.key} ${f.op}`
                : `${f.key}${f.op}${f.value}`;
            return `<span class="attr-chip">${this.escapeHtml(label)}<button class="chip-remove" data-index="${i}" title="Remove">&#215;</button></span>`;
        }).join('');
        container.querySelectorAll('.chip-remove').forEach(btn => {
            btn.addEventListener('click', () => {
                const idx = parseInt(btn.getAttribute('data-index'), 10);
                this.attrFilters.splice(idx, 1);
                this._renderAttrChips();
                this.renderLogs();
            });
        });
    }

    /**
     * Populate the attr key datalist from currently loaded logs
     */
    _updateAttrKeyDatalist() {
        const datalist = document.getElementById('attr-keys-logs-list');
        if (!datalist) return;
        const keys = new Set();
        for (const log of this.logs) {
            if (log.attributes) {
                for (const k of Object.keys(log.attributes)) keys.add(k);
            }
            if (log.resource && log.resource.attributes) {
                for (const k of Object.keys(log.resource.attributes)) keys.add(k);
            }
        }
        datalist.innerHTML = Array.from(keys).map(k => `<option value="${this.escapeHtml(k)}">`).join('');
    }

    /**
     * Test a single log entry against all active attrFilters
     */
    _matchesAttrFilters(log) {
        const attrs = log.attributes || {};
        const resAttrs = (log.resource && log.resource.attributes) ? log.resource.attributes : {};
        for (const f of this.attrFilters) {
            const val = f.key in attrs ? attrs[f.key] : (f.key in resAttrs ? resAttrs[f.key] : undefined);
            switch (f.op) {
                case '=':
                    if (String(val) !== f.value) return false;
                    break;
                case '!=':
                    if (String(val) === f.value) return false;
                    break;
                case 'exists':
                    if (!(f.key in attrs) && !(f.key in resAttrs)) return false;
                    break;
                case '!exists':
                    if ((f.key in attrs) || (f.key in resAttrs)) return false;
                    break;
            }
        }
        return true;
    }

    /**
     * Update LLM View toggle button visibility and active state
     */
    _updateLlmToggleButton() {
        const btn = document.getElementById('llm-view-toggle');
        if (!btn) return;
        if (this.hasGenAiData) {
            btn.classList.remove('hidden');
        } else {
            btn.classList.add('hidden');
        }
        btn.textContent = this.llmView ? 'LLM View ✓' : 'LLM View';
    }

    /**
     * Cleanup when view is destroyed
     */
    destroy() {
        if (this.refreshInterval) {
            clearInterval(this.refreshInterval);
        }
        if (this.debounceTimer) {
            clearTimeout(this.debounceTimer);
        }
    }
}

// Export for use in app.js
window.LogsView = LogsView;

// Made with Bob
