/**
 * Traces View - Display and interact with distributed traces
 */

function formatTs(date) {
    const p = n => String(n).padStart(2, '0');
    const ms = String(date.getMilliseconds()).padStart(3, '0');
    return `${date.getFullYear()}-${p(date.getMonth()+1)}-${p(date.getDate())} ` +
           `${p(date.getHours())}:${p(date.getMinutes())}:${p(date.getSeconds())}.${ms}`;
}

class TracesView {
    constructor(apiClient) {
        this.apiClient = apiClient;
        this.traces = [];
        this.selectedTrace = null;
        this.filters = {
            traceId: '',
            service: '',
            resource: '',
            search: '',
            startTime: null,
            endTime: null,
            sessionId: null,
            conversation_id: null,
            model: null
        };
        this.attrFilters = [];
        this.observedModels = new Set();
        this.observedSpanKinds = new Set();
        // Default time window: 1 day. Loading "all time" was the dominant
        // contributor to slow Traces page mounts.
        const now = new Date();
        this.trWindowHours = 24;
        this.trEnd = now;
        this.trStart = new Date(now.getTime() - this.trWindowHours * 3600000);
        this.currentPage = 0;
        this.pageSize = 50;
        this.autoRefresh = false;
        this.refreshInterval = null;
        this.collapsedSpans = new Set();
        this.zoomLevel = 1.0;
        // Agent-framework recognizer definitions, fetched lazily from the
        // server (see /api/genai/agent_framework_defs). Populated on first
        // render via _ensureAgentFrameworkDefs().
        this.agentFrameworkDefs = null;
    }

    async _ensureAgentFrameworkDefs() {
        if (this.agentFrameworkDefs !== null) return;
        try {
            this.agentFrameworkDefs = await this.apiClient.getAgentFrameworkDefs();
        } catch {
            // Fall back to empty list — framework sections simply won't render.
            this.agentFrameworkDefs = [];
        }
    }

    /**
     * Render the traces view
     */
    render() {
        const container = document.getElementById('traces-view');
        container.innerHTML = `
            ${this._renderTipsPanel()}
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
                <datalist id="traces-resource-keys-list"></datalist>
                <input type="text" id="traces-resource-filter" placeholder="Resource filter (e.g., service.name=my-service)" class="filter-input" list="traces-resource-keys-list">
                <div class="time-range-bar">
                    <button class="btn-icon" id="tr-prev-traces" title="Previous window">&#8592;</button>
                    <input type="text" id="tr-start-traces" class="filter-input tr-datetime" placeholder="YYYY-MM-DD HH:MM" autocomplete="off">
                    <span class="tr-sep">–</span>
                    <input type="text" id="tr-end-traces" class="filter-input tr-datetime" placeholder="YYYY-MM-DD HH:MM" autocomplete="off">
                    <button class="btn-icon" id="tr-next-traces" title="Next window">&#8594;</button>
                    <button class="btn-icon" id="tr-now-traces" title="Jump to now">Now</button>
                    <select id="tr-preset-traces" class="filter-select tr-preset">
                        <option value="">All time</option>
                        <option value="0.25">15 min</option>
                        <option value="1">1 hr</option>
                        <option value="6">6 hr</option>
                        <option value="24" selected>24 hr</option>
                        <option value="168">7 days</option>
                        <option value="720">30 days</option>
                    </select>
                </div>
                <button id="apply-trace-filters" class="btn btn-primary">Apply Filters</button>
                <button id="clear-trace-filters" class="btn btn-secondary">Clear</button>
            </div>

            <div class="attr-filter-bar" id="attr-filter-bar-traces">
                <span class="quick-filter-label">Quick:</span>
                <button id="quick-filter-errors-traces" class="btn btn-secondary btn-sm">Errors only</button>
                <button id="quick-filter-slow-traces" class="btn btn-secondary btn-sm" title="Show traces taking longer than 30 seconds">Slow (&gt;30s)</button>
                <select id="model-filter-traces" class="filter-select hidden">
                    <option value="">All models</option>
                </select>
                <span id="span-kind-chips-traces" class="span-kind-chips hidden"></span>
                <span style="width:1px;height:1.2em;background:var(--border-color);display:inline-block;margin:0 0.3rem;"></span>
                <input type="text" id="attr-key-traces" placeholder="attribute key" class="filter-input attr-filter-key" list="attr-keys-traces-list">
                <datalist id="attr-keys-traces-list"></datalist>
                <select id="attr-op-traces" class="filter-select attr-filter-op">
                    <option value="=">=</option>
                    <option value="!=">&#8800;</option>
                    <option value="exists">exists</option>
                    <option value="!exists">!exists</option>
                </select>
                <input type="text" id="attr-val-traces" placeholder="value" class="filter-input attr-filter-val">
                <button id="add-attr-filter-traces" class="btn btn-primary btn-sm">+ Add</button>
                <div id="attr-chips-traces" class="attr-chips"></div>
            </div>

            <div id="session-banner" style="display:none;"></div>

            <div class="traces-container">
                <div id="traces-list" class="traces-list"></div>
                <div id="traces-h-handle" class="layout-drag-handle-v"></div>
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
        this._syncDateInputs('traces');
        this.loadTraces();
        this.loadResourceKeys();
    }

    /**
     * Build the collapsible tips panel HTML for the traces view.
     */
    _renderTipsPanel() {
        // Collapsed by default on every load; no persistence.
        return `
            <details class="tips-panel" id="tips-panel-traces">
                <summary>Tips</summary>
                <div class="tips-panel-body">
                    <ul>
                        <li>Click <code>session.id</code> in any span's attributes to filter traces by session</li>
                        <li>LLM spans are split into Request / Response / Usage / Tool / Agent sections — gen_ai.* attrs are organised, not dumped</li>
                        <li>Chat transcripts render as role-coloured bubbles when the instrumentation emits <code>gen_ai.*.message</code> events or OpenInference <code>llm.*_messages</code> attributes</li>
                        <li>Use the Model dropdown or span-kind chips (if present) to narrow results server-side</li>
                    </ul>
                </div>
            </details>
        `;
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
        this.attachHorizontalDragResize(
            document.getElementById('traces-list'),
            document.getElementById('traces-h-handle')
        );

        // Time-range bar
        document.getElementById('tr-preset-traces').addEventListener('change', (e) => {
            const hours = e.target.value ? parseFloat(e.target.value) : null;
            if (hours !== null) {
                const now = new Date();
                this.trEnd = now;
                this.trStart = new Date(now.getTime() - hours * 3600000);
                this.trWindowHours = hours;
                this._syncDateInputs('traces');
            } else {
                this.trStart = null;
                this.trEnd = null;
                this.trWindowHours = null;
                this._syncDateInputs('traces');
            }
            this.currentPage = 0;
            this.loadTraces();
        });

        document.getElementById('tr-start-traces').addEventListener('change', () => this._onDateInputChange('traces'));
        document.getElementById('tr-end-traces').addEventListener('change', () => this._onDateInputChange('traces'));

        document.getElementById('tr-prev-traces').addEventListener('click', () => {
            const window = (this.trWindowHours || 1) * 3600000;
            const end = (this.trEnd || new Date()).getTime() - window;
            const start = (this.trStart ? this.trStart.getTime() : end - window) - window;
            this.trEnd = new Date(end);
            this.trStart = new Date(start);
            this._syncDateInputs('traces');
            document.getElementById('tr-preset-traces').value = '';
            this.currentPage = 0;
            this.loadTraces();
        });

        document.getElementById('tr-next-traces').addEventListener('click', () => {
            const now = Date.now();
            const window = (this.trWindowHours || 1) * 3600000;
            let end = (this.trEnd || new Date()).getTime() + window;
            if (end > now) end = now;
            this.trEnd = new Date(end);
            this.trStart = new Date(end - window);
            this._syncDateInputs('traces');
            document.getElementById('tr-preset-traces').value = '';
            this.currentPage = 0;
            this.loadTraces();
        });

        document.getElementById('tr-now-traces').addEventListener('click', () => {
            const now = new Date();
            const window = (this.trWindowHours || 1) * 3600000;
            this.trEnd = now;
            this.trStart = new Date(now.getTime() - window);
            this._syncDateInputs('traces');
            document.getElementById('tr-preset-traces').value = '';
            this.currentPage = 0;
            this.loadTraces();
        });

        // Attribute filter bar
        document.getElementById('add-attr-filter-traces').addEventListener('click', () => this._addAttrFilter());
        document.getElementById('attr-val-traces').addEventListener('keydown', (e) => {
            if (e.key === 'Enter') this._addAttrFilter();
        });
        document.getElementById('quick-filter-slow-traces').addEventListener('click', () => {
            // Client-side filter: keep only traces whose duration exceeds 30 s.
            // We re-use the synthetic 'error' filter mechanism — add a 'slow' key
            // that the client filter matches on duration.
            this.attrFilters.push({ key: 'slow', op: 'exists', value: '' });
            this._renderAttrChips();
            this.renderTraces();
        });

        document.getElementById('quick-filter-errors-traces').addEventListener('click', () => {
            this.attrFilters.push({ key: 'error', op: '=', value: 'true' });
            this._renderAttrChips();
            this.renderTraces();
        });

        const modelSel = document.getElementById('model-filter-traces');
        if (modelSel) {
            modelSel.addEventListener('change', (e) => {
                this.filters.model = e.target.value || null;
                this.currentPage = 0;
                this.loadTraces();
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
    // traces. Visual only — trStart/trEnd are not mutated.
    _prefillDateInputsFromTraces(traces) {
        if (this.trStart !== null || this.trEnd !== null) return;
        const startEl = document.getElementById('tr-start-traces');
        const endEl = document.getElementById('tr-end-traces');
        if (!startEl || !endEl) return;
        if (!Array.isArray(traces) || traces.length === 0) {
            startEl.value = '';
            endEl.value = '';
            return;
        }
        let minStart = Infinity, maxEnd = -Infinity;
        for (const t of traces) {
            if (typeof t.start_time !== 'number') continue;
            if (t.start_time < minStart) minStart = t.start_time;
            const end = t.start_time + (typeof t.duration === 'number' ? t.duration : 0);
            if (end > maxEnd) maxEnd = end;
        }
        if (!isFinite(minStart) || !isFinite(maxEnd)) return;
        startEl.value = this._toDatetimeLocal(new Date(minStart / 1_000_000));
        endEl.value = this._toDatetimeLocal(new Date(maxEnd / 1_000_000));
    }

    _toDatetimeLocal(date) {
        const pad = n => String(n).padStart(2, '0');
        return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
    }

    _parseDatetimeInput(str) {
        if (!str) return null;
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
        this.loadTraces();
    }

    attachHorizontalDragResize(leftPanel, handle) {
        if (!leftPanel || !handle) return;
        let startX, startW;
        handle.addEventListener('mousedown', e => {
            startX = e.clientX;
            startW = leftPanel.offsetWidth;
            handle.classList.add('dragging');
            const onMove = e => {
                const newW = Math.max(180, Math.min(600, startW + (e.clientX - startX)));
                leftPanel.style.width = newW + 'px';
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
     * Populate the resource-keys datalist for typeahead
     */
    async loadResourceKeys() {
        try {
            const response = await this.apiClient.getResourceKeys('spans');
            const datalist = document.getElementById('traces-resource-keys-list');
            if (!datalist) return;
            datalist.innerHTML = response.keys
                .map(k => `<option value="${k}=">`)
                .join('');
        } catch (_error) {
            // Non-critical; silently ignore
        }
    }

    /**
     * Load traces from API
     */
    async loadTraces() {
        document.getElementById('traces-list').innerHTML =
            '<div class="loading-state"><span class="spinner-sm"></span> Loading…</div>';
        try {
            // Load recognizer defs in parallel with the first trace query.
            this._ensureAgentFrameworkDefs();

            const params = {
                limit: this.pageSize,
                offset: this.currentPage * this.pageSize,
                ...this.filters
            };
            if (this.filters.sessionId) params.session_id = this.filters.sessionId;

            if (this.trStart !== null) {
                params.start_time = this.trStart.getTime() * 1_000_000;
                params.end_time = (this.trEnd || new Date()).getTime() * 1_000_000;
            }

            // Server only supports '=' and '!=' for attrs. Forward those; keep exists/!exists client-side.
            // 'error' stays synthetic client-side.
            const serverAttrs = this.attrFilters.filter(f =>
                (f.op === '=' || f.op === '!=') && f.key !== 'error' && f.key !== 'slow'
            );
            if (serverAttrs.length > 0) {
                params.attrs = JSON.stringify(serverAttrs);
            }

            const response = await this.apiClient.getTraces(params);
            this.traces = response.traces;
            this._updateObservedModels();
            this._updateObservedSpanKinds();
            this.renderTraces();
            this.updatePagination(response.total);
            this._prefillDateInputsFromTraces(this.traces);
        } catch (error) {
            console.error('Failed to load traces:', error);
            this.showError(`Failed to load traces: ${error.message}`);
        }
    }

    /**
     * Render traces list
     */
    renderTraces() {
        const container = document.getElementById('traces-list');

        // Populate attr key autocomplete from loaded data
        this._updateAttrKeyDatalist();

        // Server applies '=' and '!=' (except synthetic 'error'/'slow'). Remaining filters apply client-side.
        const clientFilters = this.attrFilters.filter(f =>
            f.op === 'exists' || f.op === '!exists' || f.key === 'error' || f.key === 'slow'
        );
        const displayTraces = clientFilters.length > 0
            ? this.traces.filter(trace => this._matchesAttrFilters(trace, clientFilters))
            : this.traces;

        if (displayTraces.length === 0) {
            container.innerHTML = '<div class="empty-state">No traces found</div>';
            return;
        }

        container.innerHTML = displayTraces.map(trace => this.renderTraceEntry(trace)).join('');

        // Attach click handlers
        container.querySelectorAll('.trace-entry').forEach((entry, index) => {
            entry.addEventListener('click', () => this.selectTrace(displayTraces[index].trace_id));
        });

        this._updateSessionBanner();
    }

    _updateSessionBanner() {
        const banner = document.getElementById('session-banner');
        if (!banner) return;
        if (!this.filters.sessionId) {
            banner.style.display = 'none';
            return;
        }
        banner.style.cssText = `
            display: flex; align-items: center; gap: 0.75rem;
            padding: 0.5rem 0.75rem;
            background: var(--bg-secondary);
            border: 1px solid var(--border-color);
            border-radius: 6px;
            margin-bottom: 0.5rem;
            font-size: 0.85rem;
        `;
        banner.innerHTML = `
            <span style="color:var(--text-secondary);">Filtered by session:</span>
            <code style="color:var(--accent-color);word-break:break-all;">${this.escapeHtml(this.filters.sessionId)}</code>
            <button id="session-report-btn" class="btn btn-secondary btn-sm">Session Report</button>
        `;
        document.getElementById('session-report-btn').addEventListener('click', () => {
            this.openSessionDiagnoseModal(this.filters.sessionId);
        });
    }

    async openSessionDiagnoseModal(sessionId) {
        const existing = document.getElementById('session-diagnose-modal');
        if (existing) existing.remove();

        const modal = document.createElement('div');
        modal.id = 'session-diagnose-modal';
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
        box.innerHTML = `
            <div style="display:flex;align-items:center;justify-content:space-between;padding:1rem 1.25rem;border-bottom:1px solid var(--border-color);flex-shrink:0;">
                <h3 style="font-size:1rem;font-weight:600;">Session Report</h3>
                <button id="close-session-diagnose" class="btn btn-secondary btn-sm">× Close</button>
            </div>
            <div id="session-diagnose-body" style="flex:1;overflow-y:auto;padding:1.25rem;">
                <div style="text-align:center;color:var(--text-secondary);padding:2rem;">Loading…</div>
            </div>
        `;
        modal.appendChild(box);
        document.body.appendChild(modal);

        document.getElementById('close-session-diagnose').addEventListener('click', () => modal.remove());
        modal.addEventListener('click', e => { if (e.target === modal) modal.remove(); });

        try {
            const data = await this.apiClient.getSessionDiagnose(sessionId);
            // Session context (spans/logs/metrics, issue #134) is
            // best-effort: the report renders even if the fetch fails.
            // It is bounded to the session's own period (±5 min:
            // session.created can precede the first LLM interaction by up
            // to ~0.25s, measured) so storage never pays for a
            // full-history scan.
            let context = null;
            const starts = (data.interactions || []).map(i => i.start_time_ns).filter(n => Number.isFinite(n));
            if (starts.length) {
                const marginNs = 5 * 60 * 1_000_000_000;
                const startNs = Math.min(...starts) - marginNs;
                const endNs = Math.max(...(data.interactions || []).map(
                    i => i.start_time_ns + (i.duration_ms || 0) * 1_000_000,
                )) + marginNs;
                context = await this.apiClient
                    .getSessionContext(sessionId, { start_time: startNs, end_time: endNs })
                    .catch(() => null);
            }
            document.getElementById('session-diagnose-body').innerHTML =
                this._renderSessionDiagnose(data) + this._renderSessionContext(sessionId, context);
        } catch (e) {
            document.getElementById('session-diagnose-body').innerHTML =
                `<div style="color:var(--error-color);padding:1rem;">Failed to load session report: ${this.escapeHtml(e.message)}</div>`;
        }
    }

    /**
     * Session Context section (issue #134): the full span/log/metric cohort
     * behind one session — beyond the LLM interactions in the diagnose
     * report — with cross-links into the filtered Traces and Logs views.
     * @param {string} sessionId
     * @param {?object} ctx - SessionContextResponse (null when unavailable)
     */
    _renderSessionContext(sessionId, ctx) {
        if (!ctx) return '';
        const esc = s => this.escapeHtml(String(s ?? ''));
        const fmtNs = ts => new Date(ts / 1e6).toLocaleTimeString([], { hour12: false }) +
            '.' + String(Math.floor(ts / 1000) % 1000).padStart(3, '0');
        const fmtDur = ns => ns >= 1_000_000 ? `${(ns / 1e6).toFixed(2)}s` : `${Math.round(ns / 1000)}ms`;
        const s = ctx.session || {};
        const spanRows = (ctx.spans || []).slice(0, 50).map(sp => `
            <tr style="border-bottom:1px solid var(--border-color);font-size:0.8rem;">
                <td style="padding:0.3rem 0.5rem;color:var(--text-secondary);">${fmtNs(sp.start_time)}</td>
                <td style="padding:0.3rem 0.5rem;font-family:monospace;">${esc(sp.name)}</td>
                <td style="padding:0.3rem 0.5rem;">${esc(sp.model)}</td>
                <td style="padding:0.3rem 0.5rem;">${fmtDur(sp.duration_ns)}</td>
            </tr>`).join('');
        const logRows = (ctx.logs || []).slice(0, 50).map(lg => `
            <tr style="border-bottom:1px solid var(--border-color);font-size:0.8rem;">
                <td style="padding:0.3rem 0.5rem;color:var(--text-secondary);">${fmtNs(lg.timestamp)}</td>
                <td style="padding:0.3rem 0.5rem;color:${(lg.severity || '').startsWith('ERROR') || (lg.severity || '').startsWith('FATAL') ? 'var(--error-color)' : (lg.severity || '').startsWith('WARN') ? 'var(--warning-color)' : 'inherit'};">${esc(lg.severity)}</td>
                <td style="padding:0.3rem 0.5rem;font-family:monospace;">${esc(lg.body)}</td>
            </tr>`).join('');
        const metricChips = (ctx.metrics || []).map(m => `
            <div class="sd-chip" title="${esc(m.name)} · ${m.count} points">
                <div class="sd-chip-val">${esc(m.name.replace('opencode.', '').replace('codex.', ''))}</div>
                <div class="sd-chip-lbl">${m.count} pts · sum ${m.sum != null ? Number(m.sum).toFixed(1) : '—'} · max ${m.max != null ? Number(m.max).toFixed(1) : '—'}</div>
            </div>`).join('');
        const timelineRows = (ctx.timeline || []).slice(0, 30).map(ev => `
            <div style="display:flex;gap:0.6rem;font-size:0.78rem;padding:0.15rem 0;">
                <span style="color:var(--text-secondary);min-width:9rem;">${fmtNs(ev.ts)}</span>
                <span style="min-width:2.5rem;color:${ev.kind === 'span' ? 'var(--accent-color)' : 'var(--text-secondary)'};">${esc(ev.kind)}</span>
                <span style="font-family:monospace;word-break:break-all;">${esc(ev.label)}</span>
            </div>`).join('');

        return `
            <details style="margin-top:1.5rem;" open>
                <summary style="cursor:pointer;font-weight:600;font-size:0.95rem;margin-bottom:0.75rem;">
                    Session context — agent: ${esc(s.agent) || 'unknown'}
                    ${s.project_id ? ` · project: ${esc(s.project_id)}` : ''}
                    · span coverage: ${esc(s.span_coverage)}
                </summary>
                <div style="display:flex;gap:0.75rem;flex-wrap:wrap;margin-bottom:1rem;">
                    <button class="btn btn-secondary btn-sm" onclick="window.app.navigateToTracesBySession('${esc(sessionId)}');return false;">Spans → Traces view</button>
                    <button class="btn btn-secondary btn-sm" onclick="window.app.navigateToLogsBySession('${esc(sessionId)}');return false;">Logs → Logs view</button>
                </div>
                <div style="display:flex;gap:1.5rem;flex-wrap:wrap;">
                    <div style="flex:1;min-width:320px;">
                        <div style="font-size:0.85rem;color:var(--text-secondary);margin-bottom:0.35rem;">Spans (${(ctx.spans || []).length} of ${ctx.spans_total})</div>
                        <table style="width:100%;border-collapse:collapse;">
                            <tr><th></th><th style="text-align:left;padding:0 0.5rem;">Span</th><th style="text-align:left;padding:0 0.5rem;">Model</th><th style="text-align:left;padding:0 0.5rem;">Duration</th></tr>
                            ${spanRows}
                        </table>
                    </div>
                    <div style="flex:1;min-width:320px;">
                        <div style="font-size:0.85rem;color:var(--text-secondary);margin-bottom:0.35rem;">Logs (${(ctx.logs || []).length} of ${ctx.logs_total})</div>
                        <table style="width:100%;border-collapse:collapse;">
                            <tr><th></th><th style="text-align:left;padding:0 0.5rem;">Severity</th><th style="text-align:left;padding:0 0.5rem;">Body</th></tr>
                            ${logRows}
                        </table>
                    </div>
                </div>
                ${metricChips ? `<div style="display:flex;gap:0.75rem;flex-wrap:wrap;margin-top:1rem;">${metricChips}</div>` : ''}
                ${timelineRows ? `<div style="margin-top:1rem;font-size:0.85rem;color:var(--text-secondary);margin-bottom:0.35rem;">Timeline (first ${(ctx.timeline || []).length})</div>${timelineRows}` : ''}
            </details>`;
    }

    _renderSessionDiagnose(data) {
        const fmt = n => n == null ? '—' : n >= 1_000_000 ? `${(n/1_000_000).toFixed(1)}M` : n >= 1000 ? `${(n/1000).toFixed(1)}K` : String(n);
        const fmtDur = ms => ms >= 60000 ? `${Math.floor(ms/60000)}m${String(Math.floor((ms%60000)/1000)).padStart(2,'0')}s` : `${(ms/1000).toFixed(1)}s`;
        const fmtTtft = v => v == null ? '—' : `${v.toFixed(1)}s`;

        // ── Derive session-level stats for the summary panel ──────────────────
        const totalDurationMs = data.interactions.reduce((s, i) => s + (i.duration_ms || 0), 0);
        const maxDurationMs   = Math.max(...data.interactions.map(i => i.duration_ms || 0));
        const slowCount       = data.interactions.filter(i => i.duration_ms > 30_000).length;
        const coldCount       = data.interactions.filter(i =>
            (i.cache_read_tokens || 0) === 0 && (i.cache_creation_tokens || 0) > 50_000
        ).length;
        const totalOutput     = data.interactions.reduce((s, i) => s + (i.output_tokens || 0), 0);
        const totalInput      = data.interactions.reduce((s, i) => s + (i.input_tokens  || 0), 0);
        const p95DurMs        = (() => {
            const sorted = [...data.interactions].map(i => i.duration_ms || 0).sort((a,b) => a-b);
            return sorted[Math.floor(sorted.length * 0.95)] ?? 0;
        })();
        const hasSummaryData  = data.interactions.some(i => i.input_tokens != null || i.output_tokens != null);

        let html = `
            <div style="margin-bottom:1rem;">
                <div style="font-size:0.85rem;color:var(--text-secondary);margin-bottom:0.25rem;">Session ID</div>
                <code style="word-break:break-all;">${this.escapeHtml(data.session_id)}</code>
            </div>
            <div style="display:flex;gap:2rem;flex-wrap:wrap;margin-bottom:1rem;font-size:0.9rem;">
                <div><span style="color:var(--text-secondary);">Model: </span>${this.escapeHtml(data.models.join(', ') || '(unknown)')}</div>
                <div><span style="color:var(--text-secondary);">Interactions: </span>${data.total_interactions}</div>
                <div><span style="color:var(--text-secondary);">Period: </span>${this.escapeHtml(data.start_time)} – ${this.escapeHtml(data.end_time)}</div>
                ${data.error_count > 0 ? `<div><span style="color:var(--error-color);">Errors: ${data.error_count}</span></div>` : ''}
                ${data.stall_count > 0 ? `<div><span style="color:var(--warning-color);">Stalls: ${data.stall_count}</span></div>` : ''}
            </div>`;

        // ── Slowness summary panel ────────────────────────────────────────────
        // Only render if we have meaningful data (non-trivial session).
        if (data.interactions.length > 1) {
            const chips = [];
            chips.push(`<div class="sd-chip"><div class="sd-chip-val">${fmtDur(totalDurationMs)}</div><div class="sd-chip-lbl">Total LLM time</div></div>`);
            chips.push(`<div class="sd-chip"><div class="sd-chip-val">${fmtDur(maxDurationMs)}</div><div class="sd-chip-lbl">Slowest turn</div></div>`);
            chips.push(`<div class="sd-chip${p95DurMs > 30_000 ? ' sd-chip-warn' : ''}"><div class="sd-chip-val">${fmtDur(p95DurMs)}</div><div class="sd-chip-lbl">p95 turn</div></div>`);
            if (slowCount > 0)
                chips.push(`<div class="sd-chip sd-chip-warn"><div class="sd-chip-val">${slowCount}</div><div class="sd-chip-lbl">Turns >30s</div></div>`);
            if (coldCount > 0)
                chips.push(`<div class="sd-chip sd-chip-warn"><div class="sd-chip-val">${coldCount}</div><div class="sd-chip-lbl">Cold starts</div></div>`);
            if (hasSummaryData && totalOutput > 0)
                chips.push(`<div class="sd-chip${totalOutput > 50_000 ? ' sd-chip-warn' : ''}"><div class="sd-chip-val">${fmt(totalOutput)}</div><div class="sd-chip-lbl">Total output tok</div></div>`);
            if (hasSummaryData && totalInput > 0)
                chips.push(`<div class="sd-chip"><div class="sd-chip-val">${fmt(totalInput)}</div><div class="sd-chip-lbl">Total input tok</div></div>`);

            html += `<div class="sd-chips">${chips.join('')}</div>`;

            // ── Actionable findings ─────────────────────────────────────────
            const findings = [];
            if (coldCount > 0)
                findings.push(`❄ ${coldCount} cold-start turn${coldCount>1?'s':''} rebuilt full context with no cache reads — likely session or context resets.`);
            if (slowCount > 0 && totalOutput > 0) {
                const avgOutSlow = data.interactions
                    .filter(i => i.duration_ms > 30_000 && i.output_tokens)
                    .reduce((s, i) => s + (i.output_tokens||0), 0);
                if (avgOutSlow > 5_000)
                    findings.push(`📝 Slow turns are generating ${fmt(avgOutSlow)} output tokens total — generation time dominates latency. Consider capping max_tokens.`);
            }
            if (p95DurMs > 60_000)
                findings.push(`⏱ p95 turn time is ${fmtDur(p95DurMs)} — most turns are slow, not just outliers.`);
            if (findings.length > 0)
                html += `<div class="sd-findings">${findings.map(f => `<div class="sd-finding">${this.escapeHtml(f)}</div>`).join('')}</div>`;
        }

        // ── Interaction table ─────────────────────────────────────────────────
        // Duration bar scale: max across all interactions.
        const durBarMax = Math.max(...data.interactions.map(i => i.duration_ms || 0), 1);

        html += `
            <div style="overflow-x:auto;margin-bottom:1.25rem;">
                <table style="width:100%;border-collapse:collapse;font-size:0.82rem;">
                    <thead>
                        <tr style="border-bottom:1px solid var(--border-color);color:var(--text-secondary);">
                            <th style="padding:0.3rem 0.5rem;text-align:right;">#</th>
                            <th style="padding:0.3rem 0.5rem;text-align:left;">Time</th>
                            <th style="padding:0.3rem 0.5rem;text-align:right;">Input</th>
                            <th style="padding:0.3rem 0.5rem;text-align:right;" title="New tokens written to prompt cache this turn">Cache+</th>
                            <th style="padding:0.3rem 0.5rem;text-align:right;" title="Tokens served from prompt cache">Cached</th>
                            <th style="padding:0.3rem 0.5rem;text-align:center;" title="COLD=no cache reads, full rebuild. WARMING=partial. HOT=>80% cached.">Cache</th>
                            <th style="padding:0.3rem 0.5rem;text-align:right;">TTFT</th>
                            <th style="padding:0.3rem 0.5rem;" title="Duration with proportional bar">Duration ▼</th>
                            <th style="padding:0.3rem 0.5rem;text-align:right;">Output</th>
                            <th style="padding:0.3rem 0.5rem;text-align:left;">Status</th>
                            <th style="padding:0.3rem 0.5rem;text-align:left;">Trace</th>
                        </tr>
                    </thead>
                    <tbody>
                        ${data.interactions.map(ia => {
                            const statusColor = ia.is_stall ? 'var(--warning-color)' : ia.is_error ? 'var(--error-color)' : 'var(--success-color)';
                            const statusText  = ia.is_stall ? 'STALL' : ia.is_error ? 'ERROR' : 'OK';
                            const traceShort  = ia.trace_id.substring(0, 12);
                            const cachePlusStyle = (ia.cache_creation_tokens || 0) > 0 ? 'color:var(--warning-color);' : '';
                            const durMs       = ia.duration_ms || 0;
                            const barPct      = Math.max(2, (durMs / durBarMax) * 100);
                            const isSlow      = durMs > 30_000;
                            const rowBg       = isSlow ? 'background:rgba(245,158,11,0.05);' : '';

                            // Cache state badge
                            const read    = ia.cache_read_tokens    || 0;
                            const create  = ia.cache_creation_tokens || 0;
                            const inp     = ia.input_tokens          || 0;
                            const total   = read + create + inp;
                            let cacheBadge = '';
                            if (total > 0) {
                                if (read === 0 && create > 50_000) {
                                    cacheBadge = '<span class="cache-state-badge cache-state-cold" title="Full context rebuild — no cache reads">COLD</span>';
                                } else if (read / total >= 0.8) {
                                    cacheBadge = '<span class="cache-state-badge cache-state-hot" title=">80% of tokens from cache">HOT</span>';
                                } else if (read > 0) {
                                    cacheBadge = '<span class="cache-state-badge cache-state-warming" title="Partial cache hit">WARM</span>';
                                }
                            }

                            return `<tr style="border-bottom:1px solid var(--border-color);${rowBg}">
                                <td style="padding:0.3rem 0.5rem;text-align:right;">${ia.index}</td>
                                <td style="padding:0.3rem 0.5rem;">${this.escapeHtml(ia.time)}</td>
                                <td style="padding:0.3rem 0.5rem;text-align:right;">${fmt(ia.input_tokens)}</td>
                                <td style="padding:0.3rem 0.5rem;text-align:right;${cachePlusStyle}">${fmt(ia.cache_creation_tokens)}</td>
                                <td style="padding:0.3rem 0.5rem;text-align:right;">${fmt(ia.cache_read_tokens)}</td>
                                <td style="padding:0.3rem 0.5rem;text-align:center;">${cacheBadge}</td>
                                <td style="padding:0.3rem 0.5rem;text-align:right;">${fmtTtft(ia.ttft_secs)}</td>
                                <td style="padding:0.3rem 0.5rem;min-width:120px;">
                                    <div style="display:flex;align-items:center;gap:5px;">
                                        <div style="flex:1;background:var(--border-color);border-radius:2px;height:8px;min-width:40px;">
                                            <div style="width:${barPct.toFixed(1)}%;height:8px;border-radius:2px;background:${isSlow ? '#f59e0b' : 'var(--accent-color,#3b82d4)'};"></div>
                                        </div>
                                        <span style="white-space:nowrap;${isSlow ? 'color:#d97706;font-weight:600;' : ''}">${fmtDur(durMs)}</span>
                                    </div>
                                </td>
                                <td style="padding:0.3rem 0.5rem;text-align:right;">${fmt(ia.output_tokens)}</td>
                                <td style="padding:0.3rem 0.5rem;color:${statusColor};font-weight:600;">${statusText}</td>
                                <td style="padding:0.3rem 0.5rem;font-family:monospace;font-size:0.78rem;">
                                    <a class="trace-link" onclick="window.app.views.traces.selectTrace('${this.escapeHtml(ia.trace_id)}');document.getElementById('session-diagnose-modal')?.remove();return false;" href="#">${traceShort}</a>
                                </td>
                            </tr>`;
                        }).join('')}
                    </tbody>
                </table>
            </div>`;

        // Context growth
        if (data.context_growth) {
            const cg = data.context_growth;
            html += `
                <div style="margin-bottom:1.25rem;padding:0.75rem;background:var(--bg-primary);border-radius:6px;font-size:0.87rem;">
                    <strong>Context growth:</strong>
                    ${fmt(cg.first_tokens)} → ${fmt(cg.last_tokens)} tokens across ${cg.interaction_count} interactions
                    <span style="color:var(--text-secondary);"> (peak: ${fmt(cg.peak_tokens)})</span>
                </div>`;
        }

        // Stall warnings + timeout suggestion
        const stalls = data.interactions.filter(i => i.is_stall);
        if (stalls.length > 0) {
            const longestMs = Math.max(...stalls.map(i => i.duration_ms));
            const suggestedTimeout = Math.max(Math.floor(longestMs / 1000) + 200, 500);
            const model = data.models[0] || 'this model';
            html += `<div style="padding:0.75rem;background:var(--bg-primary);border-left:3px solid var(--warning-color);border-radius:0 6px 6px 0;font-size:0.87rem;">
                <strong style="color:var(--warning-color);">⚠ ${stalls.length} streaming stall(s) detected</strong>
                ${stalls.map(ia => `<div style="margin-top:0.35rem;color:var(--text-secondary);">
                    Interaction #${ia.index}: ${fmtDur(ia.duration_ms)} duration${ia.input_tokens ? `, ~${fmt(ia.input_tokens)} tokens` : ''}
                </div>`).join('')}
                <div style="margin-top:0.5rem;">
                    <strong>Suggestion:</strong> raise the stream-idle timeout on the proxy/load-balancer
                    to at least <strong>${suggestedTimeout}s</strong> for the <code>${this.escapeHtml(model)}</code> route.
                    <span style="color:var(--text-secondary);">(longest stall: ${fmtDur(longestMs)}; a 300s hop-level timeout is a common cause)</span>
                </div>
            </div>`;
        }

        // Escalation block
        const errored = data.interactions.filter(i => i.is_error);
        if (errored.length > 0) {
            const responseIds = errored.filter(i => i.response_id).map(i => i.response_id).join(', ');
            const promptIds = errored.filter(i => i.prompt_id).map(i => i.prompt_id).join(', ');
            const traceIds = errored.map(i => i.trace_id.substring(0, 16)).join(', ');
            const bodySizes = errored.filter(i => i.body_length).map(i => `${i.body_length.toLocaleString()} bytes (~${Math.round(i.body_length/4000)}K tokens)`).join(', ');
            html += `
                <div style="margin-top:1.25rem;padding:0.75rem;background:var(--bg-primary);border-radius:6px;font-size:0.82rem;font-family:monospace;">
                    <div style="font-weight:600;margin-bottom:0.5rem;font-family:sans-serif;">Escalation info</div>
                    <div>Session:&nbsp;&nbsp;&nbsp;&nbsp;&nbsp; ${this.escapeHtml(data.session_id)}</div>
                    ${data.models.length ? `<div>Model:&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp; ${this.escapeHtml(data.models.join(', '))}</div>` : ''}
                    ${bodySizes ? `<div>Body size:&nbsp;&nbsp;&nbsp;&nbsp; ${this.escapeHtml(bodySizes)}</div>` : ''}
                    ${promptIds ? `<div>Prompt IDs:&nbsp;&nbsp;&nbsp; ${this.escapeHtml(promptIds)}</div>` : ''}
                    ${responseIds ? `<div>Response IDs:&nbsp; ${this.escapeHtml(responseIds)}</div>` : ''}
                    <div>Trace IDs:&nbsp;&nbsp;&nbsp;&nbsp; ${this.escapeHtml(traceIds)}</div>
                </div>`;
        }

        return html;
    }

    /**
     * Render a single trace entry
     */
    renderTraceEntry(trace) {
        const startTime = new Date(trace.start_time / 1000000);
        const durationMs = trace.duration / 1000000;
        const duration = durationMs >= 60000
            ? `${Math.floor(durationMs/60000)}m${String(Math.floor((durationMs%60000)/1000)).padStart(2,'0')}s`
            : `${durationMs.toFixed(0)}ms`;
        const errorClass = trace.has_errors ? 'trace-error' : '';
        const isSlowTrace = durationMs > 30_000;
        const durationClass = isSlowTrace ? 'trace-duration trace-duration-slow' : 'trace-duration';

        // Extract session/model tags if present on the root span metadata.
        const sessionId = trace.session_id || null;
        const model = trace.model || null;
        const sessionBadge = sessionId
            ? `<span class="trace-tag trace-tag-session" title="Session: ${this.escapeHtml(sessionId)}">session</span>`
            : '';
        const modelBadge = model
            ? `<span class="trace-tag trace-tag-model" title="${this.escapeHtml(model)}">${this.escapeHtml(model.replace(/^.*\//,'').substring(0,24))}</span>`
            : '';

        return `
            <div class="trace-entry ${errorClass}" data-trace-id="${trace.trace_id}">
                <div class="trace-header">
                    <span class="trace-time">${formatTs(startTime)}</span>
                    <span class="trace-name">${this.escapeHtml(trace.root_span_name)}</span>
                    ${sessionBadge}${modelBadge}
                    <span class="${durationClass}">${duration}</span>
                    <span class="trace-spans">${trace.span_count} spans</span>
                    ${trace.has_errors ? '<span class="trace-error-badge">ERROR</span>' : ''}
                </div>
                <div class="trace-meta">
                    <span class="trace-id-short" title="${trace.trace_id}">${trace.trace_id.substring(0, 16)}…</span>
                    ${trace.service_names.length > 0 ? `<span class="trace-services">${trace.service_names.join(', ')}</span>` : ''}
                </div>
            </div>
        `;
    }

    /**
     * Select and display trace details
     */
    async selectTrace(traceId, spanId) {
        try {
            // Mark the selected entry
            document.querySelectorAll('.trace-entry').forEach(el => {
                el.classList.toggle('selected', el.dataset.traceId === traceId);
            });
            const trace = await this.apiClient.getTrace(traceId);
            this.selectedTrace = trace;
            this.collapsedSpans = new Set();
            this._updateObservedSpanKinds();
            this.renderTraceDetail(trace);
            if (spanId) {
                this._focusSpanRow(spanId);
            }
        } catch (error) {
            console.error('Failed to load trace details:', error);
            this.showError('Failed to load trace details');
        }
    }

    /**
     * Scroll a span row into view and flash-highlight it (deep-link target,
     * e.g. jumping to the retried llm_request span from the analytics view).
     */
    _focusSpanRow(spanId) {
        requestAnimationFrame(() => {
            const row = document.querySelector(`.span-row[data-row-span-id="${spanId}"]`);
            if (!row) return;
            row.scrollIntoView({ block: 'center' });
            row.classList.add('span-row-flash');
            setTimeout(() => row.classList.remove('span-row-flash'), 3000);
        });
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

        const zoomPct = Math.round(this.zoomLevel * 100);
        container.innerHTML = `
            <div class="trace-detail-body">
                <div class="trace-detail-header">
                    <h3>${this.escapeHtml(trace.root_span_name ?? 'Trace Details')}</h3>
                    <div style="display:flex;align-items:center;gap:0.5rem;flex-wrap:wrap;">
                        <span class="trace-duration">${duration}ms · ${trace.span_count} spans</span>
                        ${(() => {
                            const sessionId = trace.spans?.reduce((found, s) => found || s.attributes?.['session.id'], null);
                            return sessionId ? `<button class="btn btn-primary btn-sm" onclick="window.app.views.traces.openSessionDiagnoseModal('${this.escapeHtml(sessionId)}');return false;">Session Report</button>` : '';
                        })()}
                        <button id="expand-all-spans" class="btn btn-secondary btn-sm">Expand all</button>
                        <button id="collapse-all-spans" class="btn btn-secondary btn-sm">Collapse all</button>
                        <span style="width:1px;height:1.2em;background:var(--border-color);display:inline-block;margin:0 0.2rem;"></span>
                        <button id="zoom-out-spans" class="btn btn-secondary btn-sm" title="Zoom out">−</button>
                        <span id="zoom-level-label" style="font-size:0.8rem;min-width:2.8rem;text-align:center;color:var(--text-secondary)">${zoomPct}%</span>
                        <button id="zoom-in-spans" class="btn btn-secondary btn-sm" title="Zoom in">+</button>
                        <button id="zoom-reset-spans" class="btn btn-secondary btn-sm" title="Reset zoom">1:1</button>
                    </div>
                </div>
                <div class="trace-info">
                    <div class="trace-info-item"><strong>Trace ID:</strong> <a class="trace-link" onclick="window.app.navigateToLogs('${this.escapeHtml(trace.trace_id)}');return false;" href="#" title="View logs for this trace"><code>${this.escapeHtml(trace.trace_id)}</code></a></div>
                    <div class="trace-info-item"><strong>Start:</strong> ${formatTs(startTime)}</div>
                    ${trace.service_names.length > 0 ? `<div class="trace-info-item"><strong>Services:</strong> ${this.escapeHtml(trace.service_names.join(', '))}</div>` : ''}
                    ${(() => {
                        const firstSpan = trace.spans?.[0];
                        const res = firstSpan?.resource?.attributes ?? {};
                        const items = ['service.version','host.arch','os.type','os.version'].filter(k => res[k]).map(k => `${k}=${res[k]}`).join(' · ');
                        return items ? `<div class="trace-info-item"><strong>Host:</strong> ${this.escapeHtml(items)}</div>` : '';
                    })()}
                </div>
                <div class="trace-waterfall">
                    <div class="span-kind-legend">
                        <span class="span-kind-legend-item"><span class="span-kind-dot" style="background:#3b82f6"></span>server</span>
                        <span class="span-kind-legend-item"><span class="span-kind-dot" style="background:#a855f7"></span>client</span>
                        <span class="span-kind-legend-item"><span class="span-kind-dot" style="background:#6366f1"></span>internal</span>
                        <span class="span-kind-legend-item"><span class="span-kind-dot" style="background:#f59e0b"></span>producer</span>
                        <span class="span-kind-legend-item"><span class="span-kind-dot" style="background:#22c55e"></span>consumer</span>
                        <span class="span-kind-legend-item"><span class="span-kind-dot" style="background:#ef4444"></span>error</span>
                        <span style="margin-left:auto;font-size:0.72rem;color:var(--text-secondary)">Click any row for details</span>
                    </div>
                    <div class="waterfall-zoom-content" style="width:${zoomPct}%;min-width:100%;">
                        <div class="waterfall-spans">
                            ${this.renderSpanTree(spanTree, trace.start_time, trace.duration)}
                        </div>
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
     * Collect the set of span IDs that have at least one child.
     */
    collectParentIds(spans, result = new Set()) {
        for (const span of spans) {
            if (span.children.length > 0) {
                result.add(span.span_id);
                this.collectParentIds(span.children, result);
            }
        }
        return result;
    }

    /**
     * Collect all non-leaf span IDs (spans with children) in the tree.
     */
    collectAllNonLeafIds(spans, result = new Set()) {
        for (const span of spans) {
            if (span.children.length > 0) {
                result.add(span.span_id);
                this.collectAllNonLeafIds(span.children, result);
            }
        }
        return result;
    }

    /**
     * Count total descendants of a span node.
     */
    countDescendants(span) {
        let count = 0;
        for (const child of span.children) {
            count += 1 + this.countDescendants(child);
        }
        return count;
    }

    /**
     * Build a compact inline GenAI badge string for waterfall rows.
     * Returns an HTML string or empty string if not a GenAI span.
     */
    buildGenAiWaterfallBadge(span) {
        const attrs = span.attributes || {};
        const info = this.extractGenAiInfo(attrs);
        if (!info) return '';

        if (info.isToolCall) {
            const toolLabel = info.toolName ? this.escapeHtml(info.toolName) : 'tool';
            return `<span class="genai-waterfall-badge genai-wb-tool">\uD83D\uDD27 ${toolLabel}</span>`;
        }

        const model = info.responseModel || info.model;
        const hasTokens = info.inputTokens !== null || info.outputTokens !== null;

        // Cache state pill alongside tokens.
        let cachePill = '';
        if (hasTokens && info.cacheReadTokens != null) {
            const total = (info.inputTokens ?? 0) + (info.cacheReadTokens ?? 0) + (info.cacheCreationTokens ?? 0);
            if (total > 0) {
                if (info.cacheReadTokens === 0 && (info.cacheCreationTokens ?? 0) > 50_000) {
                    cachePill = '<span class="cache-state-badge cache-state-cold">COLD</span>';
                } else if (info.cacheReadTokens / total >= 0.8) {
                    cachePill = '<span class="cache-state-badge cache-state-hot">HOT</span>';
                } else if (info.cacheReadTokens > 0) {
                    cachePill = '<span class="cache-state-badge cache-state-warming">WARM</span>';
                }
            }
        }

        let badgeText = '';
        if (model && hasTokens) {
            const inTok = (info.inputTokens ?? 0).toLocaleString();
            const outTok = (info.outputTokens ?? 0).toLocaleString();
            badgeText = `${this.escapeHtml(model)} \u00b7 ${inTok}\u2192${outTok}`;
        } else if (model) {
            badgeText = this.escapeHtml(model);
        } else if (hasTokens) {
            const inTok = (info.inputTokens ?? 0).toLocaleString();
            const outTok = (info.outputTokens ?? 0).toLocaleString();
            badgeText = `${inTok}\u2192${outTok}`;
        }

        if (!badgeText) return cachePill;
        return `<span class="genai-waterfall-badge genai-wb-llm">${badgeText}</span>${cachePill}`;
    }

    /**
     * Render span tree as waterfall
     */
    renderSpanTree(spans, traceStart, traceDuration, depth = 0, parentIds = null) {
        if (parentIds === null) {
            parentIds = this.collectParentIds(spans);
        }
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
            const hasChildren = span.children.length > 0;
            const descCount = hasChildren ? this.countDescendants(span) : 0;
            const toggleBtn = hasChildren
                ? `<span class="span-toggle" data-toggle-id="${span.span_id}" title="Collapse/expand">&#9660;</span>`
                : `<span class="span-toggle-spacer"></span>`;
            const collapsedCountEl = hasChildren
                ? `<span class="collapsed-count" data-count-id="${span.span_id}" style="display:none">(+${descCount})</span>`
                : '';
            const genAiBadge = this.buildGenAiWaterfallBadge(span);

            // Retried LLM call: the span carries one gen_ai.request.attempt
            // event per attempt. The first attempt failed before any complete
            // data (e.g. a dropped stream); the retry succeeded, so the span
            // status is OK and would otherwise look unremarkable.
            const attemptEvents = (span.events || []).filter(e => e && e.name === 'gen_ai.request.attempt');
            const retryBadge = attemptEvents.length > 1
                ? `<span class="span-retry-badge" title="Retried: ${attemptEvents.length} attempts — the first failed before any complete data was received; the retry succeeded. Open the span to see the attempt events.">↻${attemptEvents.length}</span>`
                : '';

            const spanDurMs = span.duration / 1_000_000;
            const isSlowSpan = spanDurMs > 5_000;
            const slowBadge = isSlowSpan
                ? `<span class="span-slow-badge" title="${spanDurMs >= 60000 ? Math.floor(spanDurMs/60000)+'m' : duration+'ms'} — slow span">slow</span>`
                : '';
            return `
                <div class="span-row${isSlowSpan ? ' span-row-slow' : ''}" data-row-span-id="${span.span_id}" style="padding-left: ${depth * 16 + 4}px;">
                    <div class="span-info">
                        ${toggleBtn}
                        <span class="span-name ${hasError ? 'span-error' : ''}" title="${this.escapeHtml(span.name)}">${this.escapeHtml(span.name)}${collapsedCountEl}</span>
                        ${genAiBadge}
                        ${retryBadge}
                        <span class="span-kind">${this.escapeHtml(kindLabel)}</span>
                        ${slowBadge}
                        <span class="span-duration${isSlowSpan ? ' span-duration-slow' : ''}">${duration}ms</span>
                    </div>
                    <div class="span-bar-container">
                        <div class="span-bar ${hasError ? 'span-bar-error' : kindClass}"
                             style="left: ${startOffset}%; width: ${width}%;"
                             data-span-id="${span.span_id}"
                             title="${this.escapeHtml(span.name)}: ${duration}ms">
                        </div>
                    </div>
                </div>
                ${hasChildren ? this.renderSpanTree(span.children, traceStart, traceDuration, depth + 1, parentIds) : ''}
            `;
        }).join('');
    }

    renderSpanAttributes(attributes) {
        const SKIP_ATTRS = new Set([
            'duration_ms', 'otel.scope.name', 'otel.scope.version',
            'llm.input_messages', 'llm.output_messages',
        ]);
        const entries = Object.entries(attributes ?? {})
            .filter(([k]) => !SKIP_ATTRS.has(k) && !k.startsWith('gen_ai.'));
        if (entries.length === 0) {
            return '';
        }

        return `
            <div class="span-attributes">
                ${entries.map(([key, value]) => {
                    if (key === 'session.id') {
                        const sv = this.escapeHtml(String(value));
                        return `<div class="attribute-item"><span class="attribute-key">session.id</span><span class="attribute-value"><a class="trace-link" onclick="window.app.navigateToTracesBySession('${sv}');return false;" href="#">${sv}</a> <button class="btn btn-secondary btn-sm" style="margin-left:0.4rem;padding:0.1rem 0.4rem;font-size:0.78rem;" onclick="window.app.views.traces.openSessionDiagnoseModal('${sv}');return false;">Session Report</button></span></div>`;
                    }
                    return `
                    <div class="attribute-item">
                        <span class="attribute-key">${this.escapeHtml(key)}</span>
                        ${this.renderAttributeValue(value)}
                    </div>
                `;
                }).join('')}
            </div>
        `;
    }

    /**
     * Render GenAI attribute sections (request / response / usage / tool / agent + standalone)
     * before the generic attribute list. Returns '' if no gen_ai.* attrs present.
     */
    /**
     * Render framework-specific "Agent orchestration" sections for spans that
     * carry attributes from agentic frameworks.
     *
     * The framework vocabulary (which prefixes/keys/markers belong to each
     * framework) is defined server-side in `otelite_core::agent_frameworks` and
     * fetched via `/api/genai/agent_framework_defs`. This keeps the
     * definitions in one place — the CLI/TUI use the same Rust source directly.
     *
     * Presence-gated: if none of a framework's markers (or prefix-matching
     * keys) are present, the section is omitted.
     */
    renderAgentFrameworkSections(attributes) {
        if (!attributes) return '';
        const defs = this.agentFrameworkDefs;
        if (!Array.isArray(defs) || defs.length === 0) return '';

        const attrKeys = Object.keys(attributes);

        const matchesKey = (fw, key) =>
            (fw.prefixes || []).some(p => key.startsWith(p)) ||
            (fw.literal_keys || []).includes(key);

        const isPresent = fw => {
            let prefixHit = false;
            for (const k of attrKeys) {
                if ((fw.marker_keys || []).includes(k)) return true;
                if ((fw.prefixes || []).some(p => k.startsWith(p))) prefixHit = true;
            }
            return prefixHit;
        };

        const renderRow = (k, v) =>
            `<div class="genai-section-row"><span class="genai-section-key">${this.escapeHtml(
                k,
            )}</span><span class="genai-section-val">${this.escapeHtml(String(v))}</span></div>`;

        const cards = defs
            .map(fw => {
                if (!isPresent(fw)) return '';
                const entries = Object.entries(attributes).filter(([k]) => matchesKey(fw, k));
                if (entries.length === 0) return '';
                return `<div class="genai-section"><h6>${this.escapeHtml(fw.name)} orchestration</h6><div class="genai-section-grid">${entries
                    .map(([k, v]) => renderRow(k, v))
                    .join('')}</div></div>`;
            })
            .filter(Boolean);

        return cards.join('');
    }

    /** Has any recognized agent-framework attribute — used to filter the
     *  generic attribute list so framework-grouped keys don't appear twice. */
    _isAgentFrameworkKey(key) {
        const defs = this.agentFrameworkDefs;
        if (!Array.isArray(defs)) return false;
        return defs.some(
            fw =>
                (fw.prefixes || []).some(p => key.startsWith(p)) ||
                (fw.literal_keys || []).includes(key),
        );
    }

    renderGenAiSections(attributes) {
        if (!attributes) return '';
        const hasGenAi = Object.keys(attributes).some(k => k.startsWith('gen_ai.'));
        if (!hasGenAi) return '';

        const pick = (prefix, exclude = []) => Object.entries(attributes)
            .filter(([k]) => k.startsWith(prefix) && !exclude.some(e => k.startsWith(e)))
            .map(([k, v]) => [k.slice(prefix.length), v]);

        const sections = [
            { title: 'Request', prefix: 'gen_ai.request.' },
            { title: 'Response', prefix: 'gen_ai.response.' },
            { title: 'Usage', prefix: 'gen_ai.usage.' },
            { title: 'Tool', prefix: 'gen_ai.tool.' },
            { title: 'Agent', prefix: 'gen_ai.agent.' },
        ];

        const renderRow = (k, v) => {
            // Intercept conversation id to add clickable link
            if (k === 'conversation.id') {
                return `<div class="genai-section-row"><span class="genai-section-key">${this.escapeHtml(k)}</span><span class="genai-section-val"><a class="trace-link" onclick="window.app.navigateToTracesByConversation('${this.escapeHtml(String(v))}');return false;" href="#">${this.escapeHtml(String(v))}</a></span></div>`;
            }
            return `<div class="genai-section-row"><span class="genai-section-key">${this.escapeHtml(k)}</span><span class="genai-section-val">${this.escapeHtml(String(v))}</span></div>`;
        };

        const sectionsHtml = sections.map(s => {
            const entries = pick(s.prefix);
            if (entries.length === 0) return '';
            return `<div class="genai-section"><h6>GenAI ${s.title}</h6><div class="genai-section-grid">${entries.map(([k, v]) => renderRow(k, v)).join('')}</div></div>`;
        }).join('');

        // Standalone top-level gen_ai.* keys (operation.name, provider.name, system, conversation.id, agent.id)
        const op = attributes['gen_ai.operation.name'];
        const system = attributes['gen_ai.provider.name'] || attributes['gen_ai.system'];
        const conv = attributes['gen_ai.conversation.id'];
        const agentId = attributes['gen_ai.agent.id'];
        const standalone = [];
        if (op) standalone.push(['operation', op]);
        if (system) standalone.push(['system', system]);
        if (conv) standalone.push(['conversation.id', conv]);
        if (agentId && !Object.keys(attributes).some(k => k.startsWith('gen_ai.agent.'))) {
            // only add here if not covered by the Agent section
            standalone.push(['agent.id', agentId]);
        }
        const standaloneHtml = standalone.length > 0
            ? `<div class="genai-section"><h6>GenAI</h6><div class="genai-section-grid">${standalone.map(([k, v]) => renderRow(k, v)).join('')}</div></div>`
            : '';

        return standaloneHtml + sectionsHtml;
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
            sessionId: attributes['session.id'] || null,
            system: attributes['gen_ai.provider.name'] || attributes['gen_ai.system'],
            model: attributes['gen_ai.request.model'],
            responseModel: attributes['gen_ai.response.model'] || null,
            operation: attributes['gen_ai.operation.name'],
            inputTokens: attributes['gen_ai.usage.input_tokens'] ? parseInt(attributes['gen_ai.usage.input_tokens']) : null,
            outputTokens: attributes['gen_ai.usage.output_tokens'] ? parseInt(attributes['gen_ai.usage.output_tokens']) : null,
            totalTokens: attributes['gen_ai.usage.total_tokens'] ? parseInt(attributes['gen_ai.usage.total_tokens']) : null,
            cacheCreationTokens: attributes['gen_ai.usage.cache_creation.input_tokens'] ? parseInt(attributes['gen_ai.usage.cache_creation.input_tokens']) : null,
            cacheReadTokens: attributes['gen_ai.usage.cache_read.input_tokens'] ? parseInt(attributes['gen_ai.usage.cache_read.input_tokens']) : null,
            temperature: attributes['gen_ai.request.temperature'] ? parseFloat(attributes['gen_ai.request.temperature']) : null,
            maxTokens: attributes['gen_ai.request.max_tokens'] ? parseInt(attributes['gen_ai.request.max_tokens']) : null,
            finishReasons: this.parseFinishReasons(attributes['gen_ai.response.finish_reasons']),
            responseId: attributes['gen_ai.response.id'] || null,
            toolName: attributes['gen_ai.tool.name'] || null,
            toolCallId: attributes['gen_ai.tool.call.id'] || null,
            toolType: attributes['gen_ai.tool.type'] || null,
            topP: attributes['gen_ai.request.top_p'] ? parseFloat(attributes['gen_ai.request.top_p']) : null,
            seed: attributes['gen_ai.request.seed'] ? parseInt(attributes['gen_ai.request.seed']) : null,
            isToolCall: false
        };

        // Calculate total tokens if not provided
        if (!info.totalTokens && info.inputTokens && info.outputTokens) {
            info.totalTokens = info.inputTokens + info.outputTokens;
        }

        info.isToolCall = !!(info.operation === 'execute_tool' || info.toolName);

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
                    ${info.sessionId ? `<button class="btn btn-secondary btn-sm" style="margin-left:auto;padding:0.2rem 0.6rem;font-size:0.8rem;" onclick="window.app.views.traces.openSessionDiagnoseModal('${this.escapeHtml(info.sessionId)}');return false;">Session Report</button>` : ''}
                </div>
                <div class="genai-details">
                    ${info.model ? `<div class="genai-detail-item"><strong>Model:</strong> ${this.escapeHtml(info.model)}</div>` : ''}
                    ${info.responseModel && info.responseModel !== info.model ? `<div class="genai-detail-item"><strong>Actual model:</strong> ${this.escapeHtml(info.responseModel)}</div>` : ''}
                    ${info.operation ? `<div class="genai-detail-item"><strong>Operation:</strong> ${this.escapeHtml(info.operation)}</div>` : ''}
                    ${tokenUsage ? `<div class="genai-detail-item"><strong>Tokens:</strong> <span class="genai-tokens">${tokenUsage}</span></div>` : ''}
                    ${info.cacheCreationTokens ? `<div class="genai-detail-item"><strong>Cache creation:</strong> ${info.cacheCreationTokens.toLocaleString()}</div>` : ''}
                    ${info.cacheReadTokens ? `<div class="genai-detail-item"><strong>Cache read:</strong> ${info.cacheReadTokens.toLocaleString()}</div>` : ''}
                    ${info.temperature !== null ? `<div class="genai-detail-item"><strong>Temperature:</strong> ${info.temperature.toFixed(2)}</div>` : ''}
                    ${info.maxTokens ? `<div class="genai-detail-item"><strong>Max Tokens:</strong> ${info.maxTokens.toLocaleString()}</div>` : ''}
                    ${info.finishReasons.length > 0 ? `<div class="genai-detail-item"><strong>Finish Reasons:</strong> ${info.finishReasons.join(', ')}</div>` : ''}
                    ${info.responseId ? `<div class="genai-detail-item genai-debug"><strong>Response ID:</strong> <code>${this.escapeHtml(info.responseId)}</code></div>` : ''}
                    ${info.toolName ? `<div class="genai-detail-item"><strong>🔧 Tool:</strong> <span class="genai-tool-name">${this.escapeHtml(info.toolName)}</span></div>` : ''}
                    ${info.toolCallId ? `<div class="genai-detail-item genai-debug"><strong>Tool call ID:</strong> <code>${this.escapeHtml(info.toolCallId)}</code></div>` : ''}
                    ${info.topP !== null ? `<div class="genai-detail-item"><strong>Top-p:</strong> ${info.topP}</div>` : ''}
                    ${info.seed !== null ? `<div class="genai-detail-item"><strong>Seed:</strong> ${info.seed}</div>` : ''}
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
        this.filters.resource = document.getElementById('traces-resource-filter').value;
        this.filters.search = document.getElementById('search-traces').value;
        // Time range state is managed live by the time-range-bar controls
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
            resource: '',
            search: '',
            startTime: null,
            endTime: null,
            sessionId: null,
            conversation_id: null,
            model: null
        };
        this.attrFilters = [];
        this._renderAttrChips();
        this.trStart = null;
        this.trEnd = null;
        this.trWindowHours = null;
        document.getElementById('trace-id-filter').value = '';
        document.getElementById('service-filter').value = '';
        document.getElementById('traces-resource-filter').value = '';
        document.getElementById('search-traces').value = '';
        document.getElementById('tr-preset-traces').value = '';
        const modelSel = document.getElementById('model-filter-traces');
        if (modelSel) modelSel.value = '';
        this._syncDateInputs('traces');
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
     * Collect all descendant span IDs of the given span_id using the tree structure.
     * Walks the rendered tree via data-row-span-id and the original span children map.
     */
    _gatherDescendantIds(spanId, spanNodeMap, result = new Set()) {
        const node = spanNodeMap.get(spanId);
        if (!node) return result;
        for (const child of node.children) {
            result.add(child.span_id);
            this._gatherDescendantIds(child.span_id, spanNodeMap, result);
        }
        return result;
    }

    /**
     * Build a map from span_id -> span node (with children) from the trace tree.
     */
    _buildSpanNodeMap(spanTree, map = new Map()) {
        for (const node of spanTree) {
            map.set(node.span_id, node);
            if (node.children.length > 0) {
                this._buildSpanNodeMap(node.children, map);
            }
        }
        return map;
    }

    /**
     * Update DOM visibility for all rows that are descendants of collapsed spans.
     * Also updates toggle arrows and collapsed-count badges.
     */
    _applyCollapseState(spanNodeMap) {
        // For each non-leaf span, decide visibility of its subtree
        // First, show all rows
        document.querySelectorAll('.span-row').forEach(row => {
            row.style.display = '';
        });
        // For each collapsed span, hide all descendants
        for (const collapsedId of this.collapsedSpans) {
            const descendants = this._gatherDescendantIds(collapsedId, spanNodeMap);
            for (const descId of descendants) {
                const row = document.querySelector(`.span-row[data-row-span-id="${descId}"]`);
                if (row) row.style.display = 'none';
            }
            // Update toggle arrow
            const toggle = document.querySelector(`.span-toggle[data-toggle-id="${collapsedId}"]`);
            if (toggle) toggle.innerHTML = '&#9654;';
            // Show collapsed count badge
            const badge = document.querySelector(`.collapsed-count[data-count-id="${collapsedId}"]`);
            if (badge) badge.style.display = '';
        }
        // For expanded spans, make sure arrows point down and badge is hidden
        document.querySelectorAll('.span-toggle[data-toggle-id]').forEach(toggle => {
            const id = toggle.getAttribute('data-toggle-id');
            if (!this.collapsedSpans.has(id)) {
                toggle.innerHTML = '&#9660;';
                const badge = document.querySelector(`.collapsed-count[data-count-id="${id}"]`);
                if (badge) badge.style.display = 'none';
            }
        });
    }

    /**
     * Attach click handlers to span bars and span toggle buttons
     */
    attachSpanClickHandlers(trace) {
        const spanTree = this.buildSpanTree(trace.spans);
        const spanNodeMap = this._buildSpanNodeMap(spanTree);

        // Entire span row is clickable (not just the bar)
        document.querySelectorAll('.span-row').forEach(row => {
            row.addEventListener('click', (e) => {
                // Let toggle button handle its own click
                if (e.target.closest('.span-toggle')) return;
                const spanId = row.getAttribute('data-row-span-id');
                const span = trace.spans.find(s => s.span_id === spanId);
                if (span) this.showSpanDetail(span, trace);
            });
        });

        // Toggle button click → collapse/expand subtree
        document.querySelectorAll('.span-toggle[data-toggle-id]').forEach(toggle => {
            toggle.addEventListener('click', (e) => {
                e.stopPropagation();
                const spanId = toggle.getAttribute('data-toggle-id');
                if (this.collapsedSpans.has(spanId)) {
                    this.collapsedSpans.delete(spanId);
                } else {
                    this.collapsedSpans.add(spanId);
                }
                this._applyCollapseState(spanNodeMap);
            });
        });

        // Expand all
        const expandBtn = document.getElementById('expand-all-spans');
        if (expandBtn) {
            expandBtn.addEventListener('click', () => {
                this.collapsedSpans.clear();
                this._applyCollapseState(spanNodeMap);
            });
        }

        // Collapse all
        const collapseBtn = document.getElementById('collapse-all-spans');
        if (collapseBtn) {
            collapseBtn.addEventListener('click', () => {
                const allNonLeaf = this.collectAllNonLeafIds(spanTree);
                this.collapsedSpans = allNonLeaf;
                this._applyCollapseState(spanNodeMap);
            });
        }

        // Zoom controls
        const zoomIn = document.getElementById('zoom-in-spans');
        if (zoomIn) {
            zoomIn.addEventListener('click', () => {
                this.zoomLevel = Math.min(5, this.zoomLevel + 0.5);
                this.renderTraceDetail(trace);
            });
        }
        const zoomOut = document.getElementById('zoom-out-spans');
        if (zoomOut) {
            zoomOut.addEventListener('click', () => {
                this.zoomLevel = Math.max(1, this.zoomLevel - 0.5);
                this.renderTraceDetail(trace);
            });
        }
        const zoomReset = document.getElementById('zoom-reset-spans');
        if (zoomReset) {
            zoomReset.addEventListener('click', () => {
                this.zoomLevel = 1.0;
                this.renderTraceDetail(trace);
            });
        }
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
                    <div class="span-attr-row"><span class="span-attr-key">start</span><span class="span-attr-val">${formatTs(startTime)}</span></div>
                    <div class="span-attr-row"><span class="span-attr-key">status</span><span class="span-attr-val">${span.status?.code ?? 'Unset'}${span.status?.message ? ': ' + this.escapeHtml(span.status.message) : ''}</span></div>
                    <div class="span-attr-row"><span class="span-attr-key">span_id</span><span class="span-attr-val">${span.span_id}</span></div>
                    <div class="span-attr-row"><span class="span-attr-key">trace_id</span>
                        <span class="span-attr-val">
                            <a class="trace-link" onclick="window.app.navigateToLogs('${this.escapeHtml(span.trace_id)}');return false;" href="#" title="View all logs for this trace">${this.escapeHtml(span.trace_id)}</a>
                        </span>
                    </div>
                    ${parent ? `<div class="span-attr-row"><span class="span-attr-key">parent</span><span class="span-attr-val">${this.escapeHtml(parent.name)}</span></div>` : ''}
                    ${children.length > 0 ? `<div class="span-attr-row" style="grid-column:1/-1"><span class="span-attr-key">children</span><span class="span-attr-val">${children.map(c => this.escapeHtml(c.name)).join(', ')}</span></div>` : ''}
                </div>
            </div>

            ${genaiInfo ? `<div class="span-detail-section">${this.renderGenAiInfo(genaiInfo)}</div>` : ''}

            ${(() => {
                const sectionsHtml = this.renderGenAiSections(span.attributes || {});
                return sectionsHtml ? `<div class="span-detail-section">${sectionsHtml}</div>` : '';
            })()}

            ${(() => {
                const agentHtml = this.renderAgentFrameworkSections(span.attributes || {});
                return agentHtml ? `<div class="span-detail-section">${agentHtml}</div>` : '';
            })()}

            ${(() => {
                // Exclude keys rendered by GenAI or agent-framework sections
                // above, so the generic list doesn't duplicate them.
                const nonGenAi = attrEntries.filter(
                    ([k]) => !k.startsWith('gen_ai.') && !this._isAgentFrameworkKey(k),
                );
                if (nonGenAi.length === 0) return '';
                return `
                <div class="span-detail-section">
                    <h5>Attributes (${nonGenAi.length})</h5>
                    <div class="span-attrs-grid">
                        ${nonGenAi.map(([k, v]) => {
                            const isLong = String(v).length > 80;
                            if (k === 'session.id') {
                                const sv = this.escapeHtml(String(v));
                                return `<div class="span-attr-row" style="grid-column:1/-1">
                                    <span class="span-attr-key">session.id</span>
                                    <span class="span-attr-val">
                                        <a class="trace-link" onclick="window.app.navigateToTracesBySession('${sv}');return false;" href="#">${sv}</a>
                                        <button class="btn btn-secondary btn-sm" style="margin-left:0.5rem;padding:0.15rem 0.5rem;font-size:0.78rem;" onclick="window.app.views.traces.openSessionDiagnoseModal('${sv}');return false;">Session Report</button>
                                    </span>
                                </div>`;
                            }
                            return `<div class="span-attr-row${isLong ? ' ' : ''}" ${isLong ? 'style="grid-column:1/-1"' : ''}>
                                <span class="span-attr-key">${this.escapeHtml(k)}</span>
                                <span class="span-attr-val${isLong ? ' long' : ''}">${this.escapeHtml(String(v))}</span>
                            </div>`;
                        }).join('')}
                    </div>
                </div>`;
            })()}

            ${span.events && span.events.length > 0 ? `
                <div class="span-detail-section">
                    <h5>Events (${span.events.length})</h5>
                    <div class="span-events-timeline">${this.renderSpanEvents(span.events, span.start_time)}</div>
                </div>
            ` : ''}

            ${this.renderLlmMessagesFromAttributes(span.attributes || {})}
        `;
    }

    /**
     * Render OpenInference-style llm.input_messages / llm.output_messages as chat bubbles.
     * The attributes may arrive as JSON-encoded strings or as arrays.
     */
    renderLlmMessagesFromAttributes(attributes) {
        if (!attributes) return '';
        const inRaw = attributes['llm.input_messages'];
        const outRaw = attributes['llm.output_messages'];
        if (inRaw == null && outRaw == null) return '';

        const parseList = (raw) => {
            if (raw == null) return [];
            if (Array.isArray(raw)) return raw;
            if (typeof raw === 'string') {
                try {
                    const parsed = JSON.parse(raw);
                    return Array.isArray(parsed) ? parsed : [];
                } catch { return []; }
            }
            return [];
        };

        const inputs = parseList(inRaw);
        const outputs = parseList(outRaw);
        if (inputs.length === 0 && outputs.length === 0) return '';

        const roleClass = (role) => {
            const r = String(role || '').toLowerCase();
            if (r === 'user' || r === 'assistant' || r === 'system' || r === 'tool') return r;
            return 'choice';
        };

        const renderBubble = (msg) => {
            const role = msg?.['message.role'] ?? msg?.role ?? 'unknown';
            const content = msg?.['message.content'] ?? msg?.content ?? '';
            const toolCalls = msg?.['message.tool_calls'] ?? msg?.tool_calls;
            const contentStr = typeof content === 'string' ? content : JSON.stringify(content, null, 2);
            let toolCallsHtml = '';
            if (Array.isArray(toolCalls) && toolCalls.length > 0) {
                const items = toolCalls.map(tc => {
                    const name = tc?.['tool_call.function.name'] ?? tc?.function?.name ?? tc?.name ?? 'tool';
                    const args = tc?.['tool_call.function.arguments'] ?? tc?.function?.arguments ?? tc?.arguments ?? '';
                    const argsStr = typeof args === 'string' ? args : JSON.stringify(args, null, 2);
                    return `<li><code>${this.escapeHtml(String(name))}</code><pre class="json-block"><code>${this.escapeHtml(argsStr)}</code></pre></li>`;
                }).join('');
                toolCallsHtml = `<div class="chat-bubble-toolcalls"><strong>Tool calls:</strong><ul>${items}</ul></div>`;
            }
            return `
                <div class="chat-bubble chat-bubble-${roleClass(role)}">
                    <div class="chat-bubble-header">
                        <span class="chat-bubble-role">${this.escapeHtml(String(role))}</span>
                    </div>
                    <div class="chat-bubble-content">${this.escapeHtml(contentStr)}</div>
                    ${toolCallsHtml}
                </div>
            `;
        };

        const inputHtml = inputs.map(renderBubble).join('');
        const outputHtml = outputs.map(renderBubble).join('');

        return `
            <div class="span-detail-section">
                <h5>Chat (from llm.*_messages)</h5>
                <div class="span-events-timeline">
                    ${inputHtml}${outputHtml}
                </div>
            </div>
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

        const { kindLabel, hasError } = context;
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
            const eventTime = new Date(event.timestamp / 1000000);
            const offsetMs = ((event.timestamp - spanStartTime) / 1000000).toFixed(2);

            const chatMatch = event.name && event.name.match(/^gen_ai\.(user|assistant|system|tool)\.message$/);
            const isChoice = event.name === 'gen_ai.choice';
            if (chatMatch || isChoice) {
                const role = chatMatch ? chatMatch[1] : 'choice';
                const attrs = event.attributes || {};
                let content = attrs.content ?? attrs['gen_ai.event.content'] ?? attrs.message;
                if (content === undefined || content === null) {
                    content = JSON.stringify(attrs, null, 2);
                }
                const contentStr = typeof content === 'string' ? content : JSON.stringify(content, null, 2);
                return `
                    <div class="chat-bubble chat-bubble-${role}">
                        <div class="chat-bubble-header">
                            <span class="chat-bubble-role">${this.escapeHtml(role)}</span>
                            <span class="chat-bubble-time">${formatTs(eventTime)} (+${offsetMs}ms)</span>
                        </div>
                        <div class="chat-bubble-content">${this.escapeHtml(contentStr)}</div>
                    </div>
                `;
            }

            return `
                <div class="span-event">
                    <div class="span-event-header">
                        <span class="span-event-name">${this.escapeHtml(event.name)}</span>
                        <span class="span-event-time">${formatTs(eventTime)} (+${offsetMs}ms)</span>
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
     * Add an attribute filter from the input row
     */
    _addAttrFilter() {
        const key = document.getElementById('attr-key-traces').value.trim();
        const op = document.getElementById('attr-op-traces').value;
        const value = document.getElementById('attr-val-traces').value.trim();
        if (!key) return;
        if ((op === '=' || op === '!=') && value === '') return;
        this.attrFilters.push({ key, op, value });
        document.getElementById('attr-key-traces').value = '';
        document.getElementById('attr-val-traces').value = '';
        document.getElementById('attr-op-traces').value = '=';
        this._renderAttrChips();
        this.renderTraces();
    }

    /**
     * Render the attribute filter chips
     */
    _renderAttrChips() {
        const container = document.getElementById('attr-chips-traces');
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
                this.renderTraces();
            });
        });
    }

    /**
     * Populate the attr key datalist from currently loaded traces.
     * Traces in list view may not carry attributes, but we still populate
     * from any available data.
     */
    _updateAttrKeyDatalist() {
        const datalist = document.getElementById('attr-keys-traces-list');
        if (!datalist) return;
        const keys = new Set();
        // 'error' is always a useful synthetic key for has_errors
        keys.add('error');
        for (const trace of this.traces) {
            if (trace.attributes) {
                for (const k of Object.keys(trace.attributes)) keys.add(k);
            }
        }
        datalist.innerHTML = Array.from(keys).map(k => `<option value="${this.escapeHtml(k)}">`).join('');
    }

    /**
     * Scan loaded traces for observed models and refresh the model-filter dropdown.
     * Trace list payload typically carries trace.models or trace.attributes; fall back to both.
     */
    _updateObservedModels() {
        for (const trace of this.traces) {
            if (Array.isArray(trace.models)) {
                for (const m of trace.models) if (m) this.observedModels.add(String(m));
            }
            const attrs = trace.attributes || {};
            if (attrs['gen_ai.request.model']) this.observedModels.add(String(attrs['gen_ai.request.model']));
            if (attrs['gen_ai.response.model']) this.observedModels.add(String(attrs['gen_ai.response.model']));
        }
        const sel = document.getElementById('model-filter-traces');
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
     * Scan loaded traces (and currently selected trace's spans) for OpenInference
     * span-kind attributes, and render toggle chips for the supported set.
     */
    _updateObservedSpanKinds() {
        const KNOWN = ['LLM', 'CHAIN', 'RETRIEVER', 'EMBEDDING', 'TOOL', 'AGENT', 'RERANKER', 'GUARDRAIL'];
        for (const trace of this.traces) {
            const attrs = trace.attributes || {};
            const k = attrs['openinference.span.kind'];
            if (k) this.observedSpanKinds.add(String(k).toUpperCase());
        }
        if (this.selectedTrace && Array.isArray(this.selectedTrace.spans)) {
            for (const span of this.selectedTrace.spans) {
                const k = span.attributes && span.attributes['openinference.span.kind'];
                if (k) this.observedSpanKinds.add(String(k).toUpperCase());
            }
        }
        const container = document.getElementById('span-kind-chips-traces');
        if (!container) return;
        if (this.observedSpanKinds.size === 0) {
            container.classList.add('hidden');
            container.innerHTML = '';
            return;
        }
        container.classList.remove('hidden');
        const activeKinds = new Set(
            this.attrFilters
                .filter(f => f.key === 'openinference.span.kind' && f.op === '=')
                .map(f => String(f.value).toUpperCase())
        );
        const chips = KNOWN
            .filter(k => this.observedSpanKinds.has(k))
            .map(k => {
                const active = activeKinds.has(k) ? ' active' : '';
                return `<button class="span-kind-chip${active}" data-kind="${this.escapeHtml(k)}">${this.escapeHtml(k)}</button>`;
            })
            .join('');
        container.innerHTML = chips;
        container.querySelectorAll('.span-kind-chip').forEach(btn => {
            btn.addEventListener('click', () => {
                const kind = btn.getAttribute('data-kind');
                const idx = this.attrFilters.findIndex(f =>
                    f.key === 'openinference.span.kind' && f.op === '=' &&
                    String(f.value).toUpperCase() === String(kind).toUpperCase()
                );
                if (idx >= 0) {
                    this.attrFilters.splice(idx, 1);
                } else {
                    this.attrFilters.push({ key: 'openinference.span.kind', op: '=', value: kind });
                }
                this._renderAttrChips();
                this.currentPage = 0;
                this.loadTraces();
            });
        });
    }

    /**
     * Test a single trace entry against all active attrFilters.
     * Handles the synthetic 'error' key via has_errors, and falls back to
     * trace.attributes for other keys.
     */
    _matchesAttrFilters(trace, filters = this.attrFilters) {
        const attrs = trace.attributes || {};
        for (const f of filters) {
            // Synthetic 'error' key maps to has_errors boolean
            if (f.key === 'slow') {
                // Synthetic: trace duration > 30 s
                const durationMs = (trace.duration || 0) / 1_000_000;
                const isSlow = durationMs > 30_000;
                if (f.op === 'exists' && !isSlow) return false;
                if (f.op === '!exists' && isSlow) return false;
                continue;
            }
            if (f.key === 'error') {
                const traceErrStr = String(trace.has_errors);
                switch (f.op) {
                    case '=':
                        if (traceErrStr !== f.value) return false;
                        break;
                    case '!=':
                        if (traceErrStr === f.value) return false;
                        break;
                    case 'exists':
                        // has_errors always exists
                        break;
                    case '!exists':
                        return false;
                }
                continue;
            }
            const val = f.key in attrs ? attrs[f.key] : undefined;
            switch (f.op) {
                case '=':
                    if (String(val) !== f.value) return false;
                    break;
                case '!=':
                    if (String(val) === f.value) return false;
                    break;
                case 'exists':
                    if (!(f.key in attrs)) return false;
                    break;
                case '!exists':
                    if (f.key in attrs) return false;
                    break;
            }
        }
        return true;
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
