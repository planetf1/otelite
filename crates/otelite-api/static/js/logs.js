/**
 * Logs View - Display and interact with log records
 */

function formatTs(date) {
    const p = n => String(n).padStart(2, '0');
    const ms = String(date.getMilliseconds()).padStart(3, '0');
    return `${date.getFullYear()}-${p(date.getMonth()+1)}-${p(date.getDate())} ` +
           `${p(date.getHours())}:${p(date.getMinutes())}:${p(date.getSeconds())}.${ms}`;
}

// Parse an api_response_body log into structured LLM data, returning null for other logs.
function extractLlmUsage(log) {
    if (!log || log.body !== 'claude_code.api_response_body') return null;
    const bodyStr = log.attributes?.body;
    if (!bodyStr) return null;
    try {
        const parsed = JSON.parse(bodyStr);
        return {
            model: parsed.model ?? null,
            usage: parsed.usage ?? null,
            stop_reason: parsed.stop_reason ?? null,
            service_tier: parsed.usage?.service_tier ?? null,
            web_search_requests: parsed.usage?.server_tool_use?.web_search_requests ?? 0,
            web_fetch_requests: parsed.usage?.server_tool_use?.web_fetch_requests ?? 0,
        };
    } catch { return null; }
}

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
        this.filters.trace_id = null;
        this.filters.session_id = null;
        this.filters.prompt_id = null;
        this.filters.conversation_id = null;
        this.filters.model = null;
        this.attrFilters = [];
        this.observedModels = new Set();
        // Default time window: 1 day. With "all time" the page used to load
        // hundreds of thousands of rows on every visit.
        const now = new Date();
        this.trWindowHours = 24;
        this.trEnd = now;
        this.trStart = new Date(now.getTime() - this.trWindowHours * 3600000);
        this.currentPage = 0;
        this.pageSize = 100;
        this.autoRefresh = false;
        this.refreshInterval = null;
        this.hasGenAiData = false;
        this.llmView = localStorage.getItem('otelite_llm_view') === 'true';
    }

    /**
     * Render the logs view
     */
    render() {
        const container = document.getElementById('logs-view');
        container.innerHTML = `
            ${this._renderTipsPanel()}
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
                    <input type="text" id="tr-start-logs" class="filter-input tr-datetime" placeholder="YYYY-MM-DD HH:MM" autocomplete="off">
                    <span class="tr-sep">–</span>
                    <input type="text" id="tr-end-logs" class="filter-input tr-datetime" placeholder="YYYY-MM-DD HH:MM" autocomplete="off">
                    <button class="btn-icon" id="tr-next-logs" title="Next window">&#8594;</button>
                    <button class="btn-icon" id="tr-now-logs" title="Jump to now">Now</button>
                    <select id="tr-preset-logs" class="filter-select tr-preset">
                        <option value="">All time</option>
                        <option value="0.25">15 min</option>
                        <option value="1">1 hr</option>
                        <option value="6">6 hr</option>
                        <option value="24" selected>24 hr</option>
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
                <span class="quick-filter-label">Quick:</span>
                <button id="quick-filter-error-logs" class="btn btn-secondary btn-sm">Errors</button>
                <button id="llm-view-toggle" class="btn btn-secondary btn-sm hidden">LLM View</button>
                <select id="model-filter-logs" class="filter-select hidden">
                    <option value="">All models</option>
                </select>
                <span style="width:1px;height:1.2em;background:var(--border-color);display:inline-block;margin:0 0.3rem;"></span>
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
        this._syncDateInputs('logs');
        this.loadLogs();
        this.loadResourceKeys();
    }

    /**
     * Build the collapsible tips panel HTML for the logs view.
     */
    _renderTipsPanel() {
        // Collapsed by default on every load; no persistence.
        return `
            <details class="tips-panel" id="tips-panel-logs">
                <summary>Tips</summary>
                <div class="tips-panel-body">
                    <ul>
                        <li>Click a session / prompt ID in any log detail to filter logs for that scope</li>
                        <li>Toggle "LLM View" (button appears when GenAI data is present) for model / tokens / finish reason columns</li>
                        <li>Filter by model using the Model dropdown (populates from observed values)</li>
                        <li>Filtering by prompt.id adds an aggregated cost / token / cache banner above the list</li>
                    </ul>
                </div>
            </details>
        `;
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
            localStorage.setItem('otelite_llm_view', this.llmView ? 'true' : 'false');
            this._updateLlmToggleButton();
            this.renderLogs();
        });

        const modelSel = document.getElementById('model-filter-logs');
        if (modelSel) {
            modelSel.addEventListener('change', (e) => {
                this.filters.model = e.target.value || null;
                this.currentPage = 0;
                this.loadLogs();
            });
        }
    }

    _syncDateInputs(suffix) {
        const startEl = document.getElementById(`tr-start-${suffix}`);
        const endEl = document.getElementById(`tr-end-${suffix}`);
        if (startEl) startEl.value = this.trStart ? this._toDatetimeLocal(this.trStart) : '';
        if (endEl) endEl.value = this.trEnd ? this._toDatetimeLocal(this.trEnd) : '';
    }

    // When the user has no explicit range ("All time"), show what range the
    // query actually covered by populating the date inputs from returned
    // rows. Visual only — we don't mutate trStart/trEnd.
    _prefillDateInputsFromRows(rows, tsField = 'timestamp') {
        if (this.trStart !== null || this.trEnd !== null) return;
        const startEl = document.getElementById('tr-start-logs');
        const endEl = document.getElementById('tr-end-logs');
        if (!startEl || !endEl) return;
        if (!Array.isArray(rows) || rows.length === 0) {
            if (startEl) startEl.value = '';
            if (endEl) endEl.value = '';
            return;
        }
        const timestamps = rows.map(r => r[tsField]).filter(t => typeof t === 'number');
        if (timestamps.length === 0) return;
        const minMs = Math.min(...timestamps) / 1_000_000;
        const maxMs = Math.max(...timestamps) / 1_000_000;
        startEl.value = this._toDatetimeLocal(new Date(minMs));
        endEl.value = this._toDatetimeLocal(new Date(maxMs));
    }

    _toDatetimeLocal(date) {
        // Format as YYYY-MM-DD HH:MM (ISO-style, locale-independent)
        const pad = n => String(n).padStart(2, '0');
        return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
    }

    _parseDatetimeInput(str) {
        if (!str) return null;
        // Accept YYYY-MM-DD HH:MM, YYYY-MM-DDTHH:MM, or YYYY-MM-DD
        const normalized = str.trim().replace('T', ' ');
        const m = normalized.match(/^(\d{4}-\d{2}-\d{2})(?:\s+(\d{2}:\d{2}))?$/);
        if (!m) return null;
        return new Date(`${m[1]}T${m[2] || '00:00'}`);
    }

    _onDateInputChange(suffix) {
        const startEl = document.getElementById(`tr-start-${suffix}`);
        const endEl = document.getElementById(`tr-end-${suffix}`);
        const startVal = startEl ? startEl.value : '';
        const endVal = endEl ? endEl.value : '';
        this.trStart = this._parseDatetimeInput(startVal);
        this.trEnd = this._parseDatetimeInput(endVal);
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
        document.getElementById('logs-list').innerHTML =
            '<div class="loading-state"><span class="spinner-sm"></span> Loading…</div>';
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

            // Server only supports '=' and '!='. Forward those; keep exists/!exists client-side.
            const serverAttrs = this.attrFilters.filter(f => f.op === '=' || f.op === '!=');
            if (serverAttrs.length > 0) {
                params.attrs = JSON.stringify(serverAttrs);
            }

            const response = await this.apiClient.getLogs(params);
            this.logs = response.logs;
            this._updateObservedModels();
            this.hasGenAiData = this.logs.some(log => {
                if (log.attributes && Object.keys(log.attributes).some(k => k.startsWith('gen_ai.'))) {
                    return true;
                }
                if (log.body === 'claude_code.api_response_body' && log.attributes && log.attributes.body) {
                    try {
                        const parsed = JSON.parse(log.attributes.body);
                        return parsed && parsed.model != null;
                    } catch { return false; }
                }
                return false;
            });
            this._updateLlmToggleButton();
            this.renderLogs();
            this.updatePagination(response.total);
            this._prefillDateInputsFromRows(this.logs, 'timestamp');
        } catch (error) {
            console.error('Failed to load logs:', error);
            this.showError(`Failed to load logs: ${error.message}`);
        }
    }

    /**
     * Render logs list
     */
    renderLogs() {
        const container = document.getElementById('logs-list');

        // Populate attr key autocomplete from loaded data
        this._updateAttrKeyDatalist();

        // Server applies '=' and '!='. Only exists/!exists remain client-side.
        const clientFilters = this.attrFilters.filter(f => f.op === 'exists' || f.op === '!exists');
        const displayLogs = clientFilters.length > 0
            ? this.logs.filter(log => this._matchesAttrFilters(log, clientFilters))
            : this.logs;

        const summaryHtml = this.renderPromptSummary();

        if (displayLogs.length === 0) {
            container.innerHTML = summaryHtml + '<div class="empty-state">No logs found</div>';
            return;
        }

        const useLlm = this.llmView && this.hasGenAiData;
        container.innerHTML = summaryHtml + displayLogs.map(log => this.renderLogEntry(log, useLlm)).join('');

        // Attach click handlers for expansion
        container.querySelectorAll('.log-entry').forEach((entry, index) => {
            entry.addEventListener('click', () => this.toggleLogExpansion(index));
        });

        // JSON expand/collapse toggle
        container.querySelectorAll('.json-toggle').forEach(toggle => {
            toggle.addEventListener('click', (e) => {
                e.stopPropagation();
                const wrapper = toggle.closest('.attribute-value-json, .log-body-json');
                const jsonBlock = wrapper.querySelector('.json-block');
                const rawBlock = wrapper.querySelector('.raw-block');
                const expanded = toggle.classList.contains('json-toggle-open');
                if (expanded) {
                    jsonBlock.classList.add('json-collapsed');
                    rawBlock.classList.add('json-collapsed');
                    toggle.classList.remove('json-toggle-open');
                } else {
                    const rawBtn = wrapper.querySelector('.json-raw-btn');
                    const showRaw = rawBtn && rawBtn.classList.contains('active');
                    jsonBlock.classList.toggle('json-collapsed', showRaw);
                    rawBlock.classList.toggle('json-collapsed', !showRaw);
                    toggle.classList.add('json-toggle-open');
                }
            });
        });

        // JSON raw/formatted switch
        container.querySelectorAll('.json-raw-btn').forEach(btn => {
            btn.addEventListener('click', (e) => {
                e.stopPropagation();
                const wrapper = btn.closest('.attribute-value-json, .log-body-json');
                const jsonBlock = wrapper.querySelector('.json-block');
                const rawBlock = wrapper.querySelector('.raw-block');
                const toggle = wrapper.querySelector('.json-toggle');
                const isExpanded = toggle.classList.contains('json-toggle-open');
                const isRaw = btn.classList.contains('active');
                btn.classList.toggle('active', !isRaw);
                if (isExpanded) {
                    jsonBlock.classList.toggle('json-collapsed', !isRaw);
                    rawBlock.classList.toggle('json-collapsed', isRaw);
                }
            });
        });

        // JSON in-value search (#47)
        container.querySelectorAll('.json-search').forEach(input => {
            input.addEventListener('input', (e) => {
                e.stopPropagation();
                const term = input.value.trim().toLowerCase();
                const treeBlock = input.closest('.attribute-value-json, .log-body-json').querySelector('.json-tree-block');
                if (!treeBlock) return;
                if (!term) {
                    // Clear all highlights/hidden
                    treeBlock.querySelectorAll('.json-tree-leaf, .json-tree-node').forEach(el => {
                        el.classList.remove('json-match', 'json-no-match');
                    });
                    treeBlock.querySelectorAll('.json-match-hl').forEach(el => {
                        el.outerHTML = el.textContent;
                    });
                    return;
                }
                // First pass: mark leaves
                treeBlock.querySelectorAll('.json-tree-leaf').forEach(leaf => {
                    const text = leaf.textContent.toLowerCase();
                    if (text.includes(term)) {
                        leaf.classList.add('json-match');
                        leaf.classList.remove('json-no-match');
                    } else {
                        leaf.classList.add('json-no-match');
                        leaf.classList.remove('json-match');
                    }
                });
                // Second pass: any node with a matching descendant is also shown
                treeBlock.querySelectorAll('.json-tree-node').forEach(node => {
                    const hasMatch = node.querySelector('.json-match');
                    if (hasMatch) {
                        node.classList.add('json-match');
                        node.classList.remove('json-no-match');
                        node.open = true;
                    } else {
                        node.classList.add('json-no-match');
                        node.classList.remove('json-match');
                    }
                });
            });
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

        // Detect claude_code.api_response_body logs and extract full LLM info from JSON body
        const llm = extractLlmUsage(log);

        let headerCols;
        if (useLlm) {
            const model = attrs['gen_ai.request.model'] || attrs['gen_ai.response.model'] || llm?.model || '—';
            const rawInput = attrs['gen_ai.usage.input_tokens'] ?? llm?.usage?.input_tokens;
            const rawOutput = attrs['gen_ai.usage.output_tokens'] ?? llm?.usage?.output_tokens;
            const inputTokens = rawInput != null ? Number(rawInput).toLocaleString() : '—';
            const outputTokens = rawOutput != null ? Number(rawOutput).toLocaleString() : '—';
            const finishReasonsRaw = attrs['gen_ai.response.finish_reasons'];
            const finishReason = finishReasonsRaw != null
                ? (Array.isArray(finishReasonsRaw) ? finishReasonsRaw.join(', ') : String(finishReasonsRaw))
                : (attrs['gen_ai.response.finish_reason'] || llm?.stop_reason || '—');
            headerCols = `
                    <span class="log-timestamp">${formatTs(timestamp)}</span>
                    <span class="log-severity ${severityClass}">${log.severity}</span>
                    <span class="log-col-model" title="${this.escapeHtml(model)}">${this.escapeHtml(String(model))}</span>
                    <span class="log-col-tokens">${this.escapeHtml(inputTokens)}</span>
                    <span class="log-col-tokens">${this.escapeHtml(outputTokens)}</span>
                    <span class="log-col-tokens">${this.escapeHtml(String(finishReason))}</span>
                    <span class="log-body-preview">${bodyPreview}</span>`;
        } else {
            // Show a compact session badge in the collapsed header when present —
            // avoids having to expand every row to identify which session it belongs to.
            const sessionBadge = attrs['session.id']
                ? `<span class="log-session-badge" title="Session: ${this.escapeHtml(attrs['session.id'])}" onclick="window.app.navigateToSessionReport('${this.escapeHtml(attrs['session.id'])}');event.stopPropagation();" style="cursor:pointer;">${this.escapeHtml(String(attrs['session.id']).slice(0, 8))}</span>`
                : '';
            headerCols = `
                    <span class="log-timestamp">${formatTs(timestamp)}</span>
                    <span class="log-severity ${severityClass}">${log.severity}</span>
                    ${sessionBadge}
                    <span class="log-body-preview">${bodyPreview}</span>
                    ${log.trace_id ? `<span class="log-trace-id" title="Trace ID">${this.escapeHtml(log.trace_id.substring(0, 8))}...</span>` : ''}`;
        }

        return `
            <div class="log-entry ${severityClass}" data-timestamp="${log.timestamp}">
                <div class="log-header">
                    ${headerCols}
                </div>
                <div class="log-details" style="display: none;">
                    ${this.renderBodyField(log.body)}
                    ${log.trace_id ? `<div class="log-field"><strong>Trace ID:</strong> <a class="trace-link" onclick="window.app.navigateToTrace('${this.escapeHtml(log.trace_id)}');return false;" href="#" title="View trace">${this.escapeHtml(log.trace_id)}</a></div>` : ''}
                    ${attrs['session.id'] ? `<div class="log-field"><strong>Session:</strong> <a class="trace-link" onclick="window.app.navigateToLogsBySession('${this.escapeHtml(attrs['session.id'])}');return false;" href="#" title="View all logs for this session">${this.escapeHtml(attrs['session.id'])}</a></div>` : ''}
                    ${attrs['prompt.id'] ? `<div class="log-field"><strong>Prompt:</strong> <a class="trace-link" onclick="window.app.navigateToLogsByPrompt('${this.escapeHtml(attrs['prompt.id'])}');return false;" href="#" title="View all logs for this prompt turn">${this.escapeHtml(attrs['prompt.id'])}</a></div>` : ''}
                    ${attrs['gen_ai.conversation.id'] ? `<div class="log-field"><strong>Conversation:</strong> <a class="trace-link" onclick="window.app.navigateToLogsByConversation('${this.escapeHtml(attrs['gen_ai.conversation.id'])}');return false;" href="#" title="View all logs for this conversation">${this.escapeHtml(attrs['gen_ai.conversation.id'])}</a></div>` : ''}
                    ${log.span_id ? `<div class="log-field"><strong>Span ID:</strong> ${log.span_id}</div>` : ''}
                    ${llm ? this.renderLlmUsage(llm) : ''}
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

    /**
     * Render an LLM usage detail panel for claude_code.api_response_body logs.
     * Shows cache-tier breakdown, server tool usage, stop reason, service tier, estimated cost.
     */
    renderLlmUsage(llm) {
        if (!llm) return '';
        const u = llm.usage ?? {};
        const fmt = n => n == null ? '—' : Number(n).toLocaleString();
        const rows = [];
        if (llm.model) rows.push(['Model', this.escapeHtml(llm.model)]);
        if (llm.stop_reason) rows.push(['Stop reason', this.escapeHtml(llm.stop_reason)]);
        if (llm.service_tier) rows.push(['Service tier', this.escapeHtml(llm.service_tier)]);
        rows.push(['Input tokens', fmt(u.input_tokens)]);
        rows.push(['Output tokens', fmt(u.output_tokens)]);
        const c5 = u.cache_creation?.ephemeral_5m_input_tokens;
        const c1 = u.cache_creation?.ephemeral_1h_input_tokens;
        if (u.cache_creation_input_tokens != null || c5 != null || c1 != null) {
            const parts = [];
            if (c5 != null) parts.push(`${fmt(c5)} @ 5m`);
            if (c1 != null) parts.push(`${fmt(c1)} @ 1h`);
            const label = parts.length > 0 ? parts.join(', ') : fmt(u.cache_creation_input_tokens);
            rows.push(['Cache write', label]);
        }
        if (u.cache_read_input_tokens != null) rows.push(['Cache read', fmt(u.cache_read_input_tokens)]);
        if (llm.web_search_requests || llm.web_fetch_requests) {
            const toolParts = [];
            if (llm.web_search_requests) toolParts.push(`${llm.web_search_requests} web_search`);
            if (llm.web_fetch_requests) toolParts.push(`${llm.web_fetch_requests} web_fetch`);
            rows.push(['Server tools', toolParts.join(', ')]);
        }
        // Per-span cost is rendered on the Usage page (server-computed with
        // LiteLLM-sourced rates). We avoid duplicating that math client-side.
        return `
            <div class="log-field llm-usage-panel">
                <strong>LLM Usage:</strong>
                <div class="llm-usage-grid">
                    ${rows.map(([k, v]) => `<div class="llm-usage-row"><span class="llm-usage-key">${k}</span><span class="llm-usage-val">${v}</span></div>`).join('')}
                </div>
            </div>
        `;
    }

    /**
     * Render a turn-summary banner aggregating all logs for the filtered prompt.
     * Shown above the log list only when filters.prompt_id is set.
     */
    renderPromptSummary() {
        if (!this.filters.prompt_id || this.logs.length === 0) return '';
        const logs = this.logs;
        const byBody = new Map();
        for (const l of logs) byBody.set(l.body, (byBody.get(l.body) || 0) + 1);
        const timestamps = logs.map(l => l.timestamp).filter(t => t != null);
        const tMin = Math.min(...timestamps);
        const tMax = Math.max(...timestamps);
        const durationMs = (tMax - tMin) / 1_000_000;

        // Aggregate LLM usage across api_response_body logs (usually one per prompt, but support multiple)
        let aggModel = null, aggStop = null, aggTier = null;
        const agg = { input: 0, output: 0, c5: 0, c1: 0, cc_other: 0, cread: 0, web_search: 0, web_fetch: 0 };
        for (const l of logs) {
            const llm = extractLlmUsage(l);
            if (!llm) continue;
            aggModel = aggModel || llm.model;
            aggStop = aggStop || llm.stop_reason;
            aggTier = aggTier || llm.service_tier;
            const u = llm.usage ?? {};
            agg.input += u.input_tokens ?? 0;
            agg.output += u.output_tokens ?? 0;
            const c5 = u.cache_creation?.ephemeral_5m_input_tokens ?? 0;
            const c1 = u.cache_creation?.ephemeral_1h_input_tokens ?? 0;
            agg.c5 += c5; agg.c1 += c1;
            agg.cc_other += Math.max((u.cache_creation_input_tokens ?? 0) - c5 - c1, 0);
            agg.cread += u.cache_read_input_tokens ?? 0;
            agg.web_search += llm.web_search_requests ?? 0;
            agg.web_fetch += llm.web_fetch_requests ?? 0;
        }

        const fmt = n => Number(n).toLocaleString();
        const row = (k, v) => `<div class="prompt-summary-row"><span class="prompt-summary-key">${k}</span><span class="prompt-summary-val">${v}</span></div>`;
        const bodyBreakdown = Array.from(byBody.entries())
            .sort((a, b) => b[1] - a[1])
            .map(([b, n]) => `${n}× ${this.escapeHtml(b)}`)
            .join(' · ');

        return `
            <div class="prompt-summary">
                <div class="prompt-summary-header">
                    <strong>Prompt turn:</strong> <code>${this.escapeHtml(this.filters.prompt_id)}</code>
                    <button class="btn btn-secondary btn-sm" onclick="window.app.views.logs.clearPromptFilter();">× Clear</button>
                </div>
                <div class="prompt-summary-grid">
                    ${aggModel ? row('Model', this.escapeHtml(aggModel)) : ''}
                    ${aggStop ? row('Stop', this.escapeHtml(aggStop)) : ''}
                    ${aggTier ? row('Tier', this.escapeHtml(aggTier)) : ''}
                    ${row('Logs', `${logs.length} (${this.escapeHtml(bodyBreakdown)})`)}
                    ${row('Duration', `${durationMs.toFixed(0)} ms`)}
                    ${aggModel ? row('Input', fmt(agg.input)) : ''}
                    ${aggModel ? row('Output', fmt(agg.output)) : ''}
                    ${aggModel && (agg.c5 || agg.c1 || agg.cc_other) ? row('Cache write', `${fmt(agg.c5)} @ 5m · ${fmt(agg.c1)} @ 1h${agg.cc_other ? ' · ' + fmt(agg.cc_other) + ' other' : ''}`) : ''}
                    ${aggModel && agg.cread ? row('Cache read', fmt(agg.cread)) : ''}
                    ${(agg.web_search || agg.web_fetch) ? row('Server tools', `${agg.web_search} web_search · ${agg.web_fetch} web_fetch`) : ''}
                </div>
            </div>
        `;
    }

    clearPromptFilter() {
        this.filters.prompt_id = null;
        this.currentPage = 0;
        this.loadLogs();
    }

    renderAttributeMap(attributes) {
        const SKIP_ATTRS = new Set(['duration_ms', 'otel.scope.name', 'otel.scope.version', 'session.id', 'prompt.id', 'gen_ai.conversation.id']);
        const entries = Object.entries(attributes).filter(([k]) => !SKIP_ATTRS.has(k));
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
            const collapsed = formatted.autoExpand ? '' : 'json-collapsed';
            const toggleOpen = formatted.autoExpand ? ' json-toggle-open' : '';
            const showSearch = formatted.pretty.length > 400;
            const searchHtml = showSearch
                ? `<input class="json-search" type="search" placeholder="Search JSON…" aria-label="Search within JSON value">`
                : '';
            return `
                <div class="attribute-value attribute-value-json">
                    <div class="json-header">
                        <span class="json-toggle${toggleOpen}">${this.escapeHtml(formatted.preview)}</span>
                        ${searchHtml}
                        <span class="json-raw-btn" title="Toggle raw/formatted">[raw]</span>
                    </div>
                    <div class="json-block json-tree-block ${collapsed}">${this.renderJsonTree(JSON.parse(value), 0)}</div>
                    <pre class="raw-block json-collapsed">${this.escapeHtml(String(value))}</pre>
                </div>
            `;
        }

        // Value starts with { or [ but failed to parse — likely truncated JSON
        if (typeof value === 'string') {
            const trimmed = value.trimStart();
            if (trimmed.startsWith('{') || trimmed.startsWith('[')) {
                const preview = trimmed.startsWith('{') ? '{…truncated}' : '[…truncated]';
                return `
                    <div class="attribute-value attribute-value-json">
                        <div class="json-header">
                            <span class="json-toggle">${this.escapeHtml(preview)}</span>
                            <span class="json-truncated-badge">truncated</span>
                            <span class="json-raw-btn" title="Toggle raw/formatted">[raw]</span>
                        </div>
                        <pre class="json-block json-collapsed"><code>${this.syntaxHighlightJson(this.repairAndFormatTruncatedJson(value))}</code></pre>
                        <pre class="raw-block json-collapsed">${this.escapeHtml(String(value))}</pre>
                    </div>
                `;
            }
        }

        return `<span class="attribute-value">${this.escapeHtml(String(value))}</span>`;
    }

    renderBodyField(body) {
        const formatted = this.tryFormatJsonString(body);
        if (formatted) {
            const collapsed = formatted.autoExpand ? '' : 'json-collapsed';
            const toggleOpen = formatted.autoExpand ? ' json-toggle-open' : '';
            const showSearch = formatted.pretty.length > 400;
            const searchHtml = showSearch
                ? `<input class="json-search" type="search" placeholder="Search JSON…" aria-label="Search within JSON body">`
                : '';
            return `
                <div class="log-body log-body-json">
                    <div class="json-header">
                        <span class="json-toggle${toggleOpen}">${this.escapeHtml(formatted.preview)}</span>
                        ${searchHtml}
                        <span class="json-raw-btn" title="Toggle raw/formatted">[raw]</span>
                    </div>
                    <div class="json-block json-tree-block ${collapsed}">${this.renderJsonTree(JSON.parse(body), 0)}</div>
                    <pre class="raw-block json-collapsed">${this.escapeHtml(body)}</pre>
                </div>
            `;
        }
        return `<div class="log-body">${this.escapeHtml(body)}</div>`;
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
            if (typeof parsed !== 'object' || parsed === null) {
                return null;
            }
            const pretty = JSON.stringify(parsed, null, 2);
            return {
                preview: this.describeJsonValue(parsed),
                pretty,
                autoExpand: pretty.length <= 400
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

    // Close open brackets/strings so the truncated value can be parsed.
    // Returns { pretty, wasRepaired } where pretty is the formatted string.
    repairAndFormatTruncatedJson(str) {
        // Strip known truncation marker added by the storage layer
        const markerIdx = str.indexOf('[TRUNCATED');
        let s = markerIdx >= 0 ? str.slice(0, markerIdx) : str;

        // Walk once to find open contexts (stack of '{' or '[', plus inString state)
        const stack = [];
        let inString = false;
        let escape = false;
        for (let i = 0; i < s.length; i++) {
            const ch = s[i];
            if (escape) { escape = false; continue; }
            if (inString) {
                if (ch === '\\') { escape = true; continue; }
                if (ch === '"') inString = false;
                continue;
            }
            if (ch === '"') { inString = true; continue; }
            if (ch === '{' || ch === '[') { stack.push(ch); continue; }
            if (ch === '}' || ch === ']') { stack.pop(); continue; }
        }

        // Close any open string, strip trailing comma, close open brackets
        if (inString) s += '"';
        s = s.trimEnd();
        while (s.endsWith(',') || s.endsWith(':')) s = s.slice(0, -1).trimEnd();
        for (let i = stack.length - 1; i >= 0; i--) {
            s += stack[i] === '{' ? '}' : ']';
        }

        try {
            const parsed = JSON.parse(s);
            return JSON.stringify(parsed, null, 2);
        } catch (_) {
            // Repair still failed — fall back to character-by-character indent
            return this.prettyPrintPartialJson(str);
        }
    }

    // Fallback formatter for when JSON repair fails.
    prettyPrintPartialJson(str) {
        let out = '';
        let indent = 0;
        let inString = false;
        let escape = false;
        const pad = () => '  '.repeat(indent);
        for (let i = 0; i < str.length; i++) {
            const ch = str[i];
            if (escape) { out += ch; escape = false; continue; }
            if (inString) {
                if (ch === '\\') escape = true;
                else if (ch === '"') inString = false;
                out += ch;
                continue;
            }
            switch (ch) {
                case '"': inString = true; out += ch; break;
                case '{': case '[': out += ch + '\n' + pad(++indent); break;
                case '}': case ']': out += '\n' + pad(--indent < 0 ? (indent = 0) : indent) + ch; break;
                case ',': out += ch + '\n' + pad(); break;
                case ':': out += ': '; break;
                case ' ': case '\t': case '\n': case '\r': break;
                default: out += ch;
            }
        }
        return out;
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
     * Render a parsed JSON value as a collapsible tree.
     * Objects and arrays become <details>/<summary> nodes; scalars are inline spans.
     * Depth is limited to 8 levels to avoid enormous trees from deeply-nested bodies.
     */
    renderJsonTree(value, depth) {
        const MAX_DEPTH = 8;
        if (depth > MAX_DEPTH) {
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
            return `<span class="json-scalar json-string">${this.escapeHtml(JSON.stringify(value))}</span>`;
        }
        if (Array.isArray(value)) {
            if (value.length === 0) return `<span class="json-scalar">[&thinsp;]</span>`;
            return `<div class="json-tree-array">${value.map((item, i) => {
                const isObj = item !== null && typeof item === 'object';
                if (isObj) {
                    const isOpen = depth < 2 ? ' open' : '';
                    return `<details class="json-tree-node"${isOpen}><summary class="json-tree-summary"><span class="json-tree-key json-tree-idx">[${i}]</span></summary><div class="json-tree-children">${this.renderJsonTree(item, depth + 1)}</div></details>`;
                }
                return `<div class="json-tree-leaf"><span class="json-tree-key json-tree-idx">[${i}]</span><span class="json-tree-sep">:&nbsp;</span>${this.renderJsonTree(item, depth + 1)}</div>`;
            }).join('')}</div>`;
        }
        // Plain object
        const keys = Object.keys(value);
        if (keys.length === 0) return `<span class="json-scalar">{&thinsp;}</span>`;
        return `<div class="json-tree-obj">${keys.map(k => {
            const v = value[k];
            const isObj = v !== null && typeof v === 'object';
            const keyHtml = `<span class="json-tree-key">${this.escapeHtml(k)}</span>`;
            if (isObj) {
                const isOpen = depth < 2 ? ' open' : '';
                return `<details class="json-tree-node"${isOpen}><summary class="json-tree-summary">${keyHtml}</summary><div class="json-tree-children">${this.renderJsonTree(v, depth + 1)}</div></details>`;
            }
            return `<div class="json-tree-leaf">${keyHtml}<span class="json-tree-sep">:&nbsp;</span>${this.renderJsonTree(v, depth + 1)}</div>`;
        }).join('')}</div>`;
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
            endTime: null,
            trace_id: null,
            session_id: null,
            prompt_id: null,
            conversation_id: null,
            model: null,
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
        const modelSel = document.getElementById('model-filter-logs');
        if (modelSel) modelSel.value = '';
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
    _matchesAttrFilters(log, filters = this.attrFilters) {
        const attrs = log.attributes || {};
        const resAttrs = (log.resource && log.resource.attributes) ? log.resource.attributes : {};
        for (const f of filters) {
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
     * Scan loaded logs for observed models (gen_ai.request.model and api_response_body.model)
     * and refresh the model-filter dropdown.
     */
    _updateObservedModels() {
        for (const log of this.logs) {
            const attrs = log.attributes || {};
            const req = attrs['gen_ai.request.model'];
            const resp = attrs['gen_ai.response.model'];
            if (req) this.observedModels.add(String(req));
            if (resp) this.observedModels.add(String(resp));
            if (log.body === 'claude_code.api_response_body' && attrs.body) {
                try {
                    const parsed = JSON.parse(attrs.body);
                    if (parsed && parsed.model) this.observedModels.add(String(parsed.model));
                } catch { /* ignore */ }
            }
        }
        const sel = document.getElementById('model-filter-logs');
        if (!sel) return;
        if (this.observedModels.size === 0) {
            sel.classList.add('hidden');
            return;
        }
        sel.classList.remove('hidden');
        const current = this.filters.model || '';
        const sorted = Array.from(this.observedModels).sort();
        sel.innerHTML = '<option value="">All models</option>' +
            sorted.map(m => `<option value="${this.escapeHtml(m)}"${m === current ? ' selected' : ''}>${this.escapeHtml(m)}</option>`).join('');
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
