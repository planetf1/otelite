// Sessions view — list of GenAI sessions seen in the time window.
//
// Click a row to open the existing Session Report modal (rendered by
// traces.js → openSessionDiagnoseModal). The list itself is a thin
// summary; the modal stays the canonical detail view.

class SessionsView {
    constructor(apiClient) {
        this.api = apiClient;
        // Default time window: 1 day. Sessions stretching past that show
        // their full activity once the user widens the window.
        const now = new Date();
        this.trWindowHours = 24;
        this.trEnd = now;
        this.trStart = new Date(now.getTime() - this.trWindowHours * 3600000);
        this.refreshInterval = null;
        // Global filter bar state (#135) — persisted in the URL hash query
        this.filters = parseHashQuery();
        this.appliedUnion = new Set();
        this._bar = null;
    }

    async render() {
        const container = document.getElementById('sessions-container');
        if (!container) return;

        container.innerHTML = `
            <div class="filters">
                <div class="time-range-bar">
                    <label class="filter-label">Window:</label>
                    <select id="tr-preset-sessions" class="filter-select tr-preset">
                        <option value="1">1 hr</option>
                        <option value="6">6 hr</option>
                        <option value="24" selected>24 hr</option>
                        <option value="168">7 days</option>
                        <option value="720">30 days</option>
                    </select>
                    <button id="sessions-refresh" class="btn btn-secondary btn-sm">Refresh</button>
                </div>
                <span class="filter-hint">Click a row to open the Session Report.</span>
                <div id="sessions-filter-bar" class="filter-bar-row"></div>
            </div>
            <div id="sessions-cost-panel"></div>
            <div id="sessions-list"></div>
        `;

        document.getElementById('tr-preset-sessions').addEventListener('change', (e) => {
            const hours = parseFloat(e.target.value);
            const now = new Date();
            this.trWindowHours = hours;
            this.trEnd = now;
            this.trStart = new Date(now.getTime() - hours * 3600000);
            this._loadAndRender();
        });

        document.getElementById('sessions-refresh').addEventListener('click', () => this._loadAndRender());

        this._initFilterBar();
        this._hookFilterEcho();

        await this._loadAndRender();
    }

    _initFilterBar() {
        const mount = document.getElementById('sessions-filter-bar');
        if (!mount) return;
        this.api.globalFilters = this.filters;
        this._bar = renderFilterBar(mount, this.filters, {
            onChange: (state) => {
                this.filters = { ...state };
                writeHashQuery(this.filters);
                this._loadAndRender();
            },
        });
        this._bar.grey([...this.appliedUnion]);
    }

    /**
     * Record `filters_applied` echoed by each genai/sessions response so the
     * bar can grey out dimensions no loaded endpoint honours (#135).
     */
    _hookFilterEcho() {
        const inner = this.api.get.bind(this.api);
        this.api.get = async (endpoint, params) => {
            const result = await inner(endpoint, params);
            if (this.api.lastFiltersApplied) {
                for (const d of this.api.lastFiltersApplied) this.appliedUnion.add(d);
                if (this._bar) this._bar.grey([...this.appliedUnion]);
            }
            return result;
        };
    }

    async _loadAndRender() {
        const list = document.getElementById('sessions-list');
        const panel = document.getElementById('sessions-cost-panel');
        list.innerHTML = '<div class="loading-state"><span class="spinner-sm"></span> Loading…</div>';
        try {
            const params = {
                start_time: this.trStart.getTime() * 1_000_000,
                end_time: this.trEnd.getTime() * 1_000_000,
                limit: 200,
            };
            const [resp, costs, dist] = await Promise.all([
                this.api.getSessions(params),
                this.api.getSessionCosts({ ...params, limit: 500 }).catch(() => null),
                this.api.getSessionCostDistribution({
                    start_time: params.start_time,
                    end_time: params.end_time,
                    buckets: 20,
                }).catch(() => null),
            ]);
            this._renderCostPanel(costs, dist);
            const costBySid = new Map((costs?.sessions || []).map(s => [s.session_id, s]));
            this._renderList(resp.sessions || [], costBySid);
        } catch (err) {
            list.innerHTML = `<div class="error-message">Failed to load sessions: ${this._escape(err.message)}</div>`;
        }
    }

    _renderCostPanel(costs, dist) {
        const panel = document.getElementById('sessions-cost-panel');
        if (!panel) return;
        let html = '';
        if (dist && Array.isArray(dist.buckets) && dist.buckets.length) {
            const buckets = dist.buckets;
            const maxCount = buckets.reduce((m, b) => Math.max(m, b.count), 0);
            const total = buckets.reduce((s, b) => s + b.count, 0);
            const width = 100;
            const chartHeight = 100;
            const barGap = 0.5;
            const barWidth = Math.max((width - barGap * (buckets.length - 1)) / buckets.length, 0.1);
            const fmtUsd = v => v <= 0 ? '$0' : v < 0.01 ? `$${v.toFixed(4)}` : `$${v.toFixed(2)}`;
            const bars = buckets.map((b, i) => {
                const h = maxCount > 0 ? (b.count / maxCount) * chartHeight : 0;
                const title = `${fmtUsd(b.min_usd)} – ${fmtUsd(b.max_usd)}: ${b.count} session${b.count === 1 ? '' : 's'}`;
                return `<rect class="cost-chart-bar" x="${(i * (barWidth + barGap)).toFixed(3)}" y="${(chartHeight - h).toFixed(3)}" width="${barWidth.toFixed(3)}" height="${h.toFixed(3)}"><title>${this._escape(title)}</title></rect>`;
            }).join('');
            html += `
                <h3>Session cost distribution</h3>
                <p class="section-hint">${total} costed session${total === 1 ? '' : 's'} in the window (log-spaced buckets, first bucket = zero-cost).</p>
                <div class="cost-chart">
                    <svg class="cost-chart-svg" viewBox="0 0 ${width} ${chartHeight}" preserveAspectRatio="none">
                        ${bars}
                    </svg>
                    <div class="cost-chart-axis-labels">
                        <span class="cost-chart-axis-left">${this._escape(fmtUsd(buckets[0].min_usd))}</span>
                        <span class="cost-chart-axis-mid">${this._escape(fmtUsd(buckets[Math.floor(buckets.length / 2)].max_usd))}</span>
                        <span class="cost-chart-axis-right">${this._escape(fmtUsd(buckets[buckets.length - 1].max_usd))}</span>
                    </div>
                </div>`;
        }
        if (costs && costs.anomaly_rule && (costs.median_cost_usd != null)) {
            html += `<p class="section-hint">Anomaly rule: ${this._escape(costs.anomaly_rule)} — median of positive-cost sessions: ${this._escape(`$${costs.median_cost_usd.toFixed(4)}`)}.</p>`;
        }
        panel.innerHTML = html;
    }

    _renderList(sessions, costBySid) {
        const list = document.getElementById('sessions-list');
        if (sessions.length === 0) {
            list.innerHTML = '<div class="empty-state"><p>No sessions in this window</p><p class="empty-state-hint">Sessions are GenAI traces tagged with a <code>session.id</code> attribute. Widen the window or check that your instrumentation emits session ids.</p></div>';
            return;
        }

        const fmtNum = n => Number(n).toLocaleString();
        const fmtTime = ns => {
            const d = new Date(ns / 1_000_000);
            const p = n => String(n).padStart(2, '0');
            return `${d.getFullYear()}-${p(d.getMonth()+1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
        };
        // Duration between first and last event in the session.
        const fmtDur = ns => {
            const ms = ns / 1_000_000;
            if (ms < 60_000) return `${(ms / 1000).toFixed(0)}s`;
            return `${Math.floor(ms / 60_000)}m${String(Math.floor((ms % 60_000) / 1000)).padStart(2, '0')}s`;
        };

        const rows = sessions.map(s => {
            const errClass = s.error_count > 0 ? ' has-errors' : '';
            const errCell = s.error_count > 0
                ? `<span class="cell-error">${s.error_count}</span>`
                : '<span class="cell-muted">0</span>';
            const models = s.models.length === 0
                ? '<span class="cell-muted">—</span>'
                : this._escape(s.models.join(', '));
            const durNs = s.last_seen_ns - s.first_seen_ns;
            const durCell = durNs > 0 ? fmtDur(durNs) : '<span class="cell-muted">—</span>';
            const cost = costBySid ? costBySid.get(s.session_id) : null;
            let costCell = '<span class="cell-muted">—</span>';
            let qualityBadge = '';
            if (cost && cost.cost_usd != null) {
                const estNote = cost.cost_source === 'estimated' ? ' (est.)' : '';
                const badge = cost.anomaly ? ' <span class="anomaly-badge" title="cost exceeds 3× the median session cost">⚠</span>' : '';
                costCell = `$${Number(cost.cost_usd).toFixed(2)}${estNote}${badge}`;
            }
            if (cost && cost.quality) {
                const qMap = {
                    clean:    ['quality-clean',    '✓'],
                    degraded: ['quality-degraded', '⚠'],
                    errored:  ['quality-errored',  '✗'],
                };
                const [cls, icon] = qMap[cost.quality] || ['quality-clean', '✓'];
                qualityBadge = `<span class="quality-badge ${cls}" title="${cost.quality}">${icon}</span>`;
            }
            const sid = this._escape(s.session_id);
            // Cross-nav buttons — stop propagation so the row click (Session Report) doesn't also fire.
            const navBtns = `
                <span class="session-nav-btns">
                    <button class="btn btn-secondary btn-xs session-nav-traces" data-sid="${sid}" title="View traces for this session">Traces</button>
                    <button class="btn btn-secondary btn-xs session-nav-logs" data-sid="${sid}" title="View logs for this session">Logs</button>
                </span>`;
            return `
                <tr class="session-row${errClass}" data-session-id="${sid}">
                    <td class="session-id-cell"><code>${sid}</code>${navBtns}</td>
                    <td>${models}</td>
                    <td class="num-cell">${fmtNum(s.interaction_count)}</td>
                    <td class="num-cell">${fmtNum(s.total_input_tokens)}</td>
                    <td class="num-cell">${fmtNum(s.total_output_tokens)}</td>
                    <td class="num-cell">${costCell}</td>
                    <td class="num-cell">${qualityBadge}</td>
                    <td class="num-cell">${errCell}</td>
                    <td class="num-cell">${durCell}</td>
                    <td class="time-cell">${fmtTime(s.first_seen_ns)}</td>
                    <td class="time-cell">${fmtTime(s.last_seen_ns)}</td>
                </tr>
            `;
        }).join('');

        list.innerHTML = `
            <table class="sessions-table">
                <thead>
                    <tr>
                        <th>Session ID</th>
                        <th>Models</th>
                        <th class="num-cell">Interactions</th>
                        <th class="num-cell">Input tokens</th>
                        <th class="num-cell">Output tokens</th>
                        <th class="num-cell">Cost</th>
                        <th class="num-cell">Quality</th>
                        <th class="num-cell">Errors</th>
                        <th class="num-cell">Duration</th>
                        <th>First seen</th>
                        <th>Last seen</th>
                    </tr>
                </thead>
                <tbody>${rows}</tbody>
            </table>
        `;

        list.querySelectorAll('.session-row').forEach(row => {
            row.addEventListener('click', (e) => {
                // Ignore clicks on the nav buttons.
                if (e.target.closest('.session-nav-btns')) return;
                const sid = row.dataset.sessionId;
                if (window.app && window.app.views && window.app.views.traces &&
                    typeof window.app.views.traces.openSessionDiagnoseModal === 'function') {
                    // Make sure traces view is rendered so its modal helpers exist.
                    if (!window.app.renderedViews.has('traces')) {
                        window.app.views.traces.render();
                        window.app.renderedViews.add('traces');
                    }
                    window.app.views.traces.openSessionDiagnoseModal(sid);
                } else {
                    // Fallback: navigate to traces tab pre-filtered by session.
                    window.app.navigateToTracesBySession(sid);
                }
            });
        });

        // Cross-nav button handlers.
        list.querySelectorAll('.session-nav-traces').forEach(btn => {
            btn.addEventListener('click', (e) => {
                e.stopPropagation();
                window.app.navigateToTracesBySession(btn.dataset.sid);
            });
        });
        list.querySelectorAll('.session-nav-logs').forEach(btn => {
            btn.addEventListener('click', (e) => {
                e.stopPropagation();
                window.app.navigateToLogsBySession(btn.dataset.sid);
            });
        });
    }

    _escape(s) {
        const div = document.createElement('div');
        div.textContent = String(s);
        return div.innerHTML;
    }
}

window.SessionsView = SessionsView;
export { SessionsView };
