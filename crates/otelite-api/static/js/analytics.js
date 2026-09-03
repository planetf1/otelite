// GenAI analytics view
//
// Replaces the old "Usage" tab. The page is organised as 4 collapsed
// <details> accordion sections grouped by question:
//   Cost · Latency · Reliability · Behavior
// On initial load only a single cheap getTokenUsage summary call (plus the
// static pricing metadata) is made — every chart inside a section is fetched
// lazily on first expand and cached thereafter.
//
// Costs are computed server-side (see crates/otelite-core/src/pricing.rs).

function formatTs(date) {
    const p = n => String(n).padStart(2, '0');
    return `${date.getFullYear()}-${p(date.getMonth()+1)}-${p(date.getDate())} ` +
           `${p(date.getHours())}:${p(date.getMinutes())}:${p(date.getSeconds())}`;
}

/**
 * Build an x-axis label for a chart bucket timestamp (nanoseconds).
 * When the data spans more than one calendar day, prepend the date so the
 * axis is readable for multi-day windows with sub-day buckets.
 * @param {number} tsNs   - bucket timestamp in nanoseconds
 * @param {boolean} multiDay - true when the chart's data crosses a day boundary
 */
function chartAxisLabel(tsNs, multiDay) {
    const d = new Date(tsNs / 1_000_000);
    const p = n => String(n).padStart(2, '0');
    const time = `${p(d.getHours())}:${p(d.getMinutes())}`;
    if (multiDay) return `${p(d.getMonth()+1)}-${p(d.getDate())} ${time}`;
    return time;
}

class AnalyticsView {
    constructor(apiClient) {
        this.api = apiClient;
        this.refreshInterval = null;
        const now = new Date();
        this.trWindowHours = 24;
        this.trEnd = now;
        this.trStart = new Date(now.getTime() - this.trWindowHours * 3600000);
        this.topNSort = 'cost';
        // Global filter bar state (#135) — persisted in the URL hash query
        this.filters = parseHashQuery();
        this.appliedUnion = new Set();
        this._bar = null;
        // Brush-to-focus zoom state (#136). A window in the URL hash
        // (`#/analytics?start=…&end=…`) is a zoomed window shared from a
        // link; the window it was zoomed from is unknown, so clearing it
        // falls back to the default preset window.
        this._zoomed = false;
        this._zoomBase = null;
        const hashWin = parseHashWindow();
        if (hashWin) {
            this.trStart = new Date(hashWin.startMs);
            this.trEnd = new Date(hashWin.endMs);
            this.trWindowHours = null;
            this._zoomed = true;
        }
        // Loader registry — keyed by section id ('cost', 'latency', ...)
        this.sectionLoaders = {};
        // Sections that have rendered their content for the current params.
        this.loadedSections = new Set();
        // Track open state across re-renders
        this.openSections = new Set();
        this.lastSummary = null;
    }

    async render() {
        const container = document.getElementById('analytics-container');
        if (!container) return;

        container.innerHTML = `
            ${this._renderTipsPanel()}
            <div class="view-header">
                <h2>GenAI Analytics</h2>
            </div>
            <div class="filters">
                <div class="time-range-bar">
                    <button class="btn-icon" id="tr-prev-analytics" title="Previous window">&#8592;</button>
                    <input type="text" id="tr-start-analytics" class="filter-input tr-datetime" placeholder="YYYY-MM-DD HH:MM" autocomplete="off">
                    <span class="tr-sep">–</span>
                    <input type="text" id="tr-end-analytics" class="filter-input tr-datetime" placeholder="YYYY-MM-DD HH:MM" autocomplete="off">
                    <button class="btn-icon" id="tr-next-analytics" title="Next window">&#8594;</button>
                    <button class="btn-icon" id="tr-now-analytics" title="Jump to now">Now</button>
                    <select id="tr-preset-analytics" class="filter-select tr-preset">
                        <option value="">All time</option>
                        <option value="1">1 hr</option>
                        <option value="6">6 hr</option>
                        <option value="24" selected>24 hr</option>
                        <option value="168">7 days</option>
                    </select>
                <span id="analytics-zoom-chip" class="zoom-chip" hidden></span>
                </div>
                <div id="analytics-filter-bar"></div>
            </div>
            <div id="analytics-pricing-notice"></div>
            <div id="analytics-summary-cards"></div>
            <div id="analytics-empty-state"></div>
            <div id="analytics-sections">
                ${this._renderSectionShell('cost', 'Cost', 'Tokens spent · pricing · most expensive calls')}
                ${this._renderSectionShell('tool_failure_rates', 'Tool Failure Rates', 'Failure % per opencode tool — spot flaky or broken integrations at a glance')}
                ${this._renderSectionShell('daily_tool_mix', 'Daily Tool Mix', 'Claude Code / opencode / Codex activity per calendar day — when you use each tool')}
                ${this._renderSectionShell('roles', 'Agent Roles', 'Sub-agent attribution · cost & tokens per role · role × model routing matrix (opencode)')}
                ${this._renderSectionShell('providers', 'Provider Mix', 'Tokens & estimated cost by provider × model (opencode · codex · claude)')}
                ${this._renderSectionShell('latency', 'Latency', 'Response time · throughput · context size')}
                ${this._renderSectionShell('reliability', 'Reliability', 'Errors · retries · truncation · drift')}
                ${this._renderSectionShell('behavior', 'Behavior', 'Tool use · retrieval · request volume')}
                ${this._renderSectionShell('skill_activity', 'Skills Activity', 'Which Codex skills fire most — implicit injection counts by skill name')}
                ${this._renderSectionShell('capabilities', 'Telemetry Capabilities', 'Which metrics each emitter actually provides · availability & quality')}
                ${this._renderSectionShell('model_performance', 'Model Performance', 'Per-model duration · throughput · TTFT · error diagnosis vs preceding & rolling baselines')}
                ${this._renderSectionShell('effort', 'Effort Breakdown', 'Claude Code token usage by effort level (low/medium/high/xhigh) × model × type')}
                ${this._renderSectionShell('efficiency', 'Agent Efficiency', 'Tokens per commit · tokens per line of code · cross-agent comparison')}
                ${this._renderSectionShell('codex_ttft', 'Codex TTFT', 'First-token latency percentiles (p50/p90/p95) per model from histogram metrics')}
                ${this._renderSectionShell('project_rollup', 'Project Rollup', 'Token activity and turn counts per project across all agents')}
                ${this._renderSectionShell('mcp_health', 'MCP Health', 'Call success/error rates per MCP server and tool — spot flaky integrations')}
                ${this._renderSectionShell('guardian', 'Guardian Reviews', 'Risk levels · denial rate by action type · what the guardian is actually blocking')}
                ${this._renderSectionShell('multi_agent', 'Multi-Agent Topology', 'Sub-agent spawn and resume counts by role')}
                ${this._renderSectionShell('codex_turns', 'Codex Busy/Idle', 'Average busy vs idle time per turn by model and project')}
                ${this._renderSectionShell('session_model', 'Session × Model', 'Token and cost breakdown per (session, model) pair — spot opus spend in specific sessions')}
                ${this._renderSectionShell('speed_dist', 'Speed / Effort Mode', 'Distribution of the Claude Code speed attribute (normal / extended thinking) by model')}
                ${this._renderSectionShell('cross_tool_ttft', 'Cross-Tool TTFT', 'First-token latency by model across all tools (Claude Code, opencode, pi) from span attributes')}
                ${this._renderSectionShell('hook_overhead', 'Hook Overhead', 'Codex hook total and average invocation time per event type — how much latency hooks add')}
                ${this._renderSectionShell('reasoning_share', 'Reasoning Token Share', 'Thinking tokens as a percentage of output tokens per model — opencode + Codex')}
            </div>
        `;

        this._attachTimeRangeListeners();
        this._syncDateInputs();
        this._initFilterBar();
        this._hookFilterEcho();
        this._attachZoomEscListener();
        this._syncZoomChip();

        this._registerSectionLoaders();
        this._attachSectionToggleHandlers();

        await this._loadSummary();

        if (!this.refreshInterval) {
            this.refreshInterval = setInterval(() => this._refresh(), 30000);
        }
    }

    _renderSectionShell(id, title, hint) {
        const open = this.openSections.has(id);
        return `
            <details class="analytics-section" id="analytics-section-${id}"${open ? ' open' : ''}>
                <summary class="analytics-section-summary">
                    <span class="analytics-section-title">${title}</span>
                    <span class="analytics-section-hint">${hint}</span>
                    <span class="analytics-section-stat" id="analytics-section-stat-${id}">—</span>
                </summary>
                <div class="analytics-section-body" id="analytics-section-body-${id}">
                    <div class="empty-state-hint">Loading…</div>
                </div>
            </details>`;
    }

    _renderTipsPanel() {
        // Collapsed by default on every load; no persistence.
        return `
            <details class="tips-panel" id="tips-panel-analytics">
                <summary>💡 Tips &amp; shortcuts</summary>
                <div class="tips-panel-body">
                    <div class="tips-grid">
                        <div class="tips-col">
                            <strong>Layout</strong>
                            <ul>
                                <li>Sections lazy-load on first expand</li>
                                <li>Top-spans table is under <strong>Cost</strong> — sort dropdown switches view</li>
                            </ul>
                            <strong>Widgets</strong>
                            <ul>
                                <li>Drag across a time-series chart to zoom every section; <kbd>Esc</kbd> or the chip's Clear restores the window</li>
                                <li>Cost from LiteLLM pricing — unknown models show "—"</li>
                                <li>Bucket auto-scales with time window</li>
                                <li>Truncation gauge goes red on <code>finish_reason=max_tokens</code></li>
                                <li>Tool rows amber if success rate &lt; 90%</li>
                            </ul>
                        </div>
                        <div class="tips-col">
                            <strong>Recipes</strong>
                            <ul>
                                <li>Prompt cost → Logs → click <code>prompt.id</code></li>
                                <li>Session history → click <code>session.id</code> anywhere</li>
                                <li>Truncation? → Reliability → finish_reasons</li>
                                <li>Most expensive → Cost → top calls table</li>
                                <li>Failing tool? → Behavior → tool usage → success rate</li>
                                <li>Opus vs Sonnet speed → Latency → latency-by-model</li>
                                <li>Why is it slow? → Latency → 🔍 Latency diagnosis card</li>
                                <li>Cache savings → Cost → cache hit rate</li>
                            </ul>
                        </div>
                    </div>
                </div>
            </details>
        `;
    }

    _attachTimeRangeListeners() {
        document.getElementById('tr-preset-analytics').addEventListener('change', (e) => {
            const hours = e.target.value ? parseFloat(e.target.value) : null;
            if (hours !== null) {
                const now = new Date();
                this.trEnd = now;
                this.trStart = new Date(now.getTime() - hours * 3600000);
                this.trWindowHours = hours;
                this._syncDateInputs();
            } else {
                this.trStart = null;
                this.trEnd = null;
                this.trWindowHours = null;
                this._syncDateInputs();
            }
            this._refresh();
        });

        document.getElementById('tr-start-analytics').addEventListener('change', () => this._onDateInputChange());
        document.getElementById('tr-end-analytics').addEventListener('change', () => this._onDateInputChange());

        document.getElementById('tr-prev-analytics').addEventListener('click', () => {
            const windowMs = (this.trWindowHours || 1) * 3600000;
            const end = (this.trEnd || new Date()).getTime() - windowMs;
            const start = (this.trStart ? this.trStart.getTime() : end - windowMs) - windowMs;
            this.trEnd = new Date(end);
            this.trStart = new Date(start);
            this._syncDateInputs();
            document.getElementById('tr-preset-analytics').value = '';
            this._refresh();
        });

        document.getElementById('tr-next-analytics').addEventListener('click', () => {
            const now = Date.now();
            const windowMs = (this.trWindowHours || 1) * 3600000;
            let end = (this.trEnd || new Date()).getTime() + windowMs;
            if (end > now) end = now;
            this.trEnd = new Date(end);
            this.trStart = new Date(end - windowMs);
            this._syncDateInputs();
            document.getElementById('tr-preset-analytics').value = '';
            this._refresh();
        });

        document.getElementById('tr-now-analytics').addEventListener('click', () => {
            const now = new Date();
            const windowMs = (this.trWindowHours || 1) * 3600000;
            this.trEnd = now;
            this.trStart = new Date(now.getTime() - windowMs);
            this._syncDateInputs();
            document.getElementById('tr-preset-analytics').value = '';
            this._refresh();
        });
    }

    _syncDateInputs() {
        const startEl = document.getElementById('tr-start-analytics');
        const endEl = document.getElementById('tr-end-analytics');
        if (startEl) startEl.value = this.trStart ? this._toDatetimeLocal(this.trStart) : '';
        if (endEl) endEl.value = this.trEnd ? this._toDatetimeLocal(this.trEnd) : '';
    }

    _prefillDateInputsFromData(costSeries, bucketSecs) {
        if (this.trStart !== null || this.trEnd !== null) return;
        const startEl = document.getElementById('tr-start-analytics');
        const endEl = document.getElementById('tr-end-analytics');
        if (!startEl || !endEl) return;
        if (!Array.isArray(costSeries) || costSeries.length === 0) return;
        const timestamps = costSeries
            .map(r => r.timestamp)
            .filter(t => typeof t === 'number');
        if (timestamps.length === 0) return;
        const minMs = Math.min(...timestamps) / 1_000_000;
        const bucketMs = (bucketSecs || 3600) * 1000;
        const maxMs = Math.min(Math.max(...timestamps) / 1_000_000 + bucketMs, Date.now());
        startEl.value = this._toDatetimeLocal(new Date(minMs));
        endEl.value = this._toDatetimeLocal(new Date(maxMs));
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

    _onDateInputChange() {
        const startEl = document.getElementById('tr-start-analytics');
        const endEl = document.getElementById('tr-end-analytics');
        this.trStart = this._parseDatetimeInput(startEl ? startEl.value : '');
        this.trEnd = this._parseDatetimeInput(endEl ? endEl.value : '');
        if (this.trStart && this.trEnd) {
            this.trWindowHours = (this.trEnd.getTime() - this.trStart.getTime()) / 3600000;
        }
        const presetEl = document.getElementById('tr-preset-analytics');
        if (presetEl) presetEl.value = '';
        this._syncZoomChip();
        this._refresh();
    }

    _chooseBucket() {
        const hours = this.trWindowHours;
        if (hours == null) return 86400;
        if (hours <= 1) return 60;
        if (hours <= 6) return 300;
        if (hours <= 24) return 900;
        if (hours <= 168) return 3600;
        return 86400;
    }

    _baseParams() {
        const params = {};
        if (this.trStart !== null) {
            params.start_time = this.trStart.getTime() * 1_000_000;
            params.end_time = (this.trEnd || new Date()).getTime() * 1_000_000;
        }
        return params;
    }

    /**
     * Re-fetch summary and any currently-expanded section. Called when the
     * time window or model filter changes, or on the 30s auto-refresh.
     *
     * Loaded sections are updated in place: their existing content stays
     * visible (dimmed) until the new data arrives, so charts never blank
     * out during a refresh.
     */
    async _refresh() {
        await this._loadSummary();
        // Re-fire loaders for any open sections
        for (const id of Object.keys(this.sectionLoaders)) {
            const details = document.getElementById(`analytics-section-${id}`);
            if (details && details.open) {
                this.sectionLoaders[id]();
            }
        }
    }

    /**
     * Single eager call: getTokenUsage. Populates the header summary cards,
     * the per-section tiny stat in each <summary>, the model dropdown, and
     * the pricing-notice slot (a separate cheap fetch for static metadata).
     */
    async _loadSummary() {
        const summaryContainer = document.getElementById('analytics-summary-cards');
        const emptyEl = document.getElementById('analytics-empty-state');
        const sectionsEl = document.getElementById('analytics-sections');
        const noticeEl = document.getElementById('analytics-pricing-notice');
        if (!summaryContainer) return;

        try {
            const params = this._baseParams();
            const [summary, pricingMeta] = await Promise.all([
                this.api.getTokenUsage(params),
                this.api.getPricingMetadata().catch(() => null),
            ]);
            this.lastSummary = summary;

            if (noticeEl) {
                noticeEl.innerHTML = this._renderPricingNotice(pricingMeta);
            }

            if (!summary || !summary.summary || summary.summary.total_requests === 0) {
                summaryContainer.innerHTML = '';
                if (sectionsEl) sectionsEl.style.display = 'none';
                if (emptyEl) {
                    emptyEl.innerHTML = `<div class="empty-state">
                        <p>No GenAI data yet</p>
                        <p class="empty-state-hint">
                            Instrument your LLM application with the OpenAI or Anthropic OTel SDK and point it at
                            <strong>http://localhost:4318</strong>. Token usage will appear here once spans with
                            <code>gen_ai.system</code> attributes arrive.
                        </p>
                    </div>`;
                }
                this._populateModelDropdown([]);
                return;
            }

            if (sectionsEl) sectionsEl.style.display = '';
            if (emptyEl) emptyEl.innerHTML = '';

            summaryContainer.innerHTML = this._buildHeaderCards(summary);
            this._populateModelDropdown(summary.by_model || []);
            this._updateSectionStats(summary);
        } catch (err) {
            if (this.lastSummary && this.lastSummary.summary) {
                // Keep the previous cards; the data is merely stale.
                return;
            }
            summaryContainer.innerHTML = `<div class="empty-state"><p>Failed to load analytics summary</p><p class="empty-state-hint">${this._esc(err.message)}</p></div>`;
        }
    }

    _buildHeaderCards(data) {
        const { summary } = data;
        const fmt = n => Number(n).toLocaleString();
        const totalInput = summary.total_input_tokens ?? 0;
        const totalOutput = summary.total_output_tokens ?? 0;
        return `
            <div class="usage-summary-cards">
                <div class="usage-card">
                    <div class="usage-card-label">Requests</div>
                    <div class="usage-card-value">${fmt(summary.total_requests ?? 0)}</div>
                </div>
                <div class="usage-card">
                    <div class="usage-card-label">Input tokens</div>
                    <div class="usage-card-value">${fmt(totalInput)}</div>
                </div>
                <div class="usage-card">
                    <div class="usage-card-label">Output tokens</div>
                    <div class="usage-card-value">${fmt(totalOutput)}</div>
                </div>
                <div class="usage-card">
                    <div class="usage-card-label">Models</div>
                    <div class="usage-card-value">${fmt((data.by_model || []).length)}</div>
                </div>
            </div>`;
    }

    _updateSectionStats(data) {
        const { summary, by_model } = data;
        const fmt = n => Number(n).toLocaleString();
        const requests = summary.total_requests ?? 0;
        const totalTokens = (summary.total_input_tokens ?? 0) + (summary.total_output_tokens ?? 0);
        const modelCount = (by_model || []).length;

        const set = (id, html) => {
            const el = document.getElementById(`analytics-section-stat-${id}`);
            if (el) el.innerHTML = html;
        };
        set('cost', `${fmt(totalTokens)} tokens · ${fmt(requests)} req`);
        set('latency', `${fmt(requests)} req · ${fmt(modelCount)} model${modelCount === 1 ? '' : 's'}`);
        set('reliability', `${fmt(requests)} req`);
        set('behavior', `${fmt(requests)} req`);
    }

    _initFilterBar() {
        const mount = document.getElementById('analytics-filter-bar');
        if (!mount) return;
        this.api.globalFilters = this.filters;
        this._bar = renderFilterBar(mount, this.filters, {
            onChange: (state) => {
                this.filters = { ...state };
                this._writeUrlState();
                this._refresh();
            },
        });
        this._bar.grey([...this.appliedUnion]);
    }

    /**
     * Persist filters + zoomed window into the URL hash (#135 / #136).
     */
    _writeUrlState() {
        const win = this._zoomed
            ? { startMs: this.trStart.getTime(), endMs: this.trEnd.getTime() }
            : null;
        writeHashQuery(this.filters, win);
    }

    /**
     * Record `filters_applied` echoed by each genai response so the bar can
     * grey out dimensions no loaded endpoint honours (#135).
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

    // ── Brush-to-focus zoom (#136) ───────────────────────────────────────────

    /**
     * SVG data attributes that make a time-series chart brushable. The x-axis
     * spans [first bucket start, last bucket end]; the brush handler maps a
     * pixel fraction onto that range.
     */
    _brushAttrs(timestampsNs, bucketSecs) {
        if (!timestampsNs || timestampsNs.length === 0) return '';
        const n = timestampsNs.length;
        const first = timestampsNs[0];
        const last = timestampsNs[n - 1];
        const bucketNs = bucketSecs
            ? bucketSecs * 1_000_000_000
            : (n > 1 ? (last - first) / (n - 1) : 3_600_000_000_000);
        const startMs = first / 1_000_000;
        const endMs = last / 1_000_000 + bucketNs / 1_000_000;
        return `data-brushable="1" data-ts-start="${startMs}" data-ts-end="${endMs}"`;
    }

    /**
     * Mark a freshly rendered section body's time-series charts as brushable
     * and make sure the delegated brush listeners exist (bound once per view
     * — sections re-render on every refresh, so per-chart window listeners
     * would leak).
     */
    _enableBrushing(root) {
        if (!root) return;
        root.querySelectorAll('svg[data-brushable]').forEach(svg => {
            if (svg.dataset.brushBound) return;
            svg.dataset.brushBound = '1';
            svg.style.cursor = 'crosshair';
        });
        this._ensureBrushDelegation();
    }

    _ensureBrushDelegation() {
        if (this._brushDelegate) return;
        this._brushDelegate = true;
        this._brush = null; // { svg, startPx, overlay, dragging }

        const MIN_DRAG_PX = 8;    // below this a release is a plain click
        const MIN_SPAN_MS = 60_000; // degenerate windows are rejected
        const frac = (svg, px) => {
            const rect = svg.getBoundingClientRect();
            return Math.min(1, Math.max(0, (px - rect.left) / rect.width));
        };
        const fracToMs = (svg, f) => {
            const t0 = Number(svg.dataset.tsStart);
            const t1 = Number(svg.dataset.tsEnd);
            return t0 + f * (t1 - t0);
        };

        document.addEventListener('mousedown', e => {
            if (e.button !== 0) return;
            const svg = e.target.closest ? e.target.closest('svg[data-brushable]') : null;
            if (!svg) return;
            const overlay = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
            overlay.setAttribute('class', 'brush-overlay');
            overlay.setAttribute('y', '0');
            overlay.setAttribute('height', '100');
            const f = frac(svg, e.clientX);
            overlay.setAttribute('x', (f * 100).toFixed(3));
            overlay.setAttribute('width', '0');
            svg.appendChild(overlay);
            this._brush = { svg, startPx: e.clientX, overlay, dragging: true };
            e.preventDefault();
        });

        document.addEventListener('mousemove', e => {
            const b = this._brush;
            if (!b || !b.dragging) return;
            const x0 = frac(b.svg, Math.min(b.startPx, e.clientX));
            const x1 = frac(b.svg, Math.max(b.startPx, e.clientX));
            b.overlay.setAttribute('x', (x0 * 100).toFixed(3));
            b.overlay.setAttribute('width', ((x1 - x0) * 100).toFixed(3));
        });

        document.addEventListener('mouseup', e => {
            const b = this._brush;
            if (!b || !b.dragging) return;
            b.dragging = false;
            const { svg, startPx, overlay } = b;
            this._brush = null;
            if (overlay) overlay.remove();
            if (Math.abs(e.clientX - startPx) < MIN_DRAG_PX) return; // click: no zoom
            const f0 = frac(svg, Math.min(startPx, e.clientX));
            const f1 = frac(svg, Math.max(startPx, e.clientX));
            const a = fracToMs(svg, f0);
            const c = fracToMs(svg, f1);
            if (c - a < MIN_SPAN_MS) return; // too narrow: no zoom
            this._applyZoom(a, c);
        });
    }

    _attachZoomEscListener() {
        this._escHandler = e => {
            if (e.key !== 'Escape' || !this._zoomed) return;
            const view = document.getElementById('analytics-view');
            if (!view || !view.classList.contains('active')) return;
            const t = e.target;
            if (t && /^(INPUT|SELECT|TEXTAREA)$/.test(t.tagName)) return;
            this._clearZoom();
        };
        window.addEventListener('keydown', this._escHandler);
    }

    _syncZoomChip() {
        const chip = document.getElementById('analytics-zoom-chip');
        if (!chip) return;
        if (!this._zoomed) {
            chip.hidden = true;
            chip.innerHTML = '';
            return;
        }
        chip.hidden = false;
        chip.innerHTML =
            `Zoomed ${this._esc(this._toDatetimeLocal(this.trStart))} – ${this._esc(this._toDatetimeLocal(this.trEnd))} ` +
            '<button type="button" class="btn-icon zoom-chip-clear" title="Restore the previous window (Esc)">Clear</button>';
        chip.querySelector('.zoom-chip-clear').addEventListener('click', () => this._clearZoom());
    }

    _applyZoom(startMs, endMs) {
        this._zoomBase = {
            start: this.trStart,
            end: this.trEnd,
            hours: this.trWindowHours,
        };
        this.trStart = new Date(startMs);
        this.trEnd = new Date(endMs);
        this.trWindowHours = null;
        this._zoomed = true;
        const preset = document.getElementById('tr-preset-analytics');
        if (preset) preset.value = '';
        this._syncDateInputs();
        this._writeUrlState();
        this._syncZoomChip();
        this._refresh();
    }

    _clearZoom() {
        if (!this._zoomed) return;
        if (this._zoomBase) {
            this.trStart = this._zoomBase.start;
            this.trEnd = this._zoomBase.end;
            this.trWindowHours = this._zoomBase.hours;
        } else {
            // Zoomed in from a shared link: the original window is unknown,
            // fall back to the default 24-hour preset.
            const now = new Date();
            this.trWindowHours = 24;
            this.trEnd = now;
            this.trStart = new Date(now.getTime() - 24 * 3600000);
        }
        this._zoomed = false;
        this._zoomBase = null;
        const preset = document.getElementById('tr-preset-analytics');
        if (preset) preset.value = this.trWindowHours ? String(this.trWindowHours) : '';
        this._syncDateInputs();
        this._writeUrlState();
        this._syncZoomChip();
        this._refresh();
    }

    _populateModelDropdown(byModel) {
        // Rebuild the bar's model select now that we know the models in the
        // window; provider options come from the by_system breakdown.
        const mount = document.getElementById('analytics-filter-bar');
        if (!mount) return;
        const models = [...new Set(byModel.map(r => r.model).filter(Boolean))].sort();
        const bySystem = (this.lastSummary && this.lastSummary.by_system) || [];
        const providers = [...new Set(bySystem.map(r => r.system).filter(Boolean))].sort();
        this._bar = renderFilterBar(mount, this.filters, {
            modelOptions: models,
            providerOptions: providers,
            onChange: (state) => {
                this.filters = { ...state };
                this._writeUrlState();
                this._refresh();
            },
        });
        this._bar.grey([...this.appliedUnion]);
    }

    // ── Section lazy-loaders ─────────────────────────────────────────────────

    _registerSectionLoaders() {
        this.sectionLoaders = {
            cost: () => this._loadCostSection(),
            roles: () => this._loadRolesSection(),
            providers: () => this._loadProvidersSection(),
            latency: () => this._loadLatencySection(),
            reliability: () => this._loadReliabilitySection(),
            behavior: () => this._loadBehaviorSection(),
            capabilities: () => this._loadCapabilitiesSection(),
            model_performance: () => this._loadModelPerformanceSection(),
            effort: () => this._loadEffortSection(),
            efficiency: () => this._loadEfficiencySection(),
            codex_ttft: () => this._loadCodexTtftSection(),
            project_rollup: () => this._loadProjectRollupSection(),
            mcp_health: () => this._loadMcpHealthSection(),
            guardian: () => this._loadGuardianSection(),
            multi_agent: () => this._loadMultiAgentSection(),
            codex_turns: () => this._loadCodexTurnsSection(),
            session_model: () => this._loadSessionModelSection(),
            speed_dist: () => this._loadSpeedDistSection(),
            cross_tool_ttft: () => this._loadCrossToolTtftSection(),
            hook_overhead: () => this._loadHookOverheadSection(),
            reasoning_share: () => this._loadReasoningShareSection(),
            tool_failure_rates: () => this._loadToolFailureRatesSection(),
            daily_tool_mix: () => this._loadDailyToolMixSection(),
            skill_activity: () => this._loadSkillActivitySection(),
        };
    }

    _attachSectionToggleHandlers() {
        for (const id of Object.keys(this.sectionLoaders)) {
            const details = document.getElementById(`analytics-section-${id}`);
            if (!details) continue;
            details.addEventListener('toggle', () => {
                if (details.open) {
                    this.openSections.add(id);
                    if (!this.loadedSections.has(id)) {
                        this.sectionLoaders[id]();
                    }
                } else {
                    this.openSections.delete(id);
                }
            });
        }
    }

    _setSectionBody(id, html) {
        const body = document.getElementById(`analytics-section-body-${id}`);
        if (body) {
            body.classList.remove('updating');
            body.innerHTML = html;
            this._enableBrushing(body);
        }
    }

    _setSectionLoading(id) {
        const body = document.getElementById(`analytics-section-body-${id}`);
        if (!body) return;
        if (!this.loadedSections.has(id)) {
            // First load: nothing to show yet.
            body.innerHTML = `<div class="empty-state-hint">Loading…</div>`;
        } else {
            // Refresh: keep the previous content on screen and dim it so
            // the chart does not disappear while the refetch is in flight.
            body.classList.add('updating');
        }
    }

    _setSectionError(id, err) {
        const msg = `<div class="empty-state-hint">Failed to load: ${this._esc(err.message || String(err))}</div>`;
        const body = document.getElementById(`analytics-section-body-${id}`);
        if (body && this.loadedSections.has(id) && body.innerHTML.trim()) {
            // Refresh failed but we have previous data: keep it and flag
            // the staleness above it instead of wiping the chart.
            body.classList.remove('updating');
            body.insertAdjacentHTML('afterbegin', msg);
        } else {
            this._setSectionBody(id, msg);
        }
    }

    async _loadCostSection() {
        this._setSectionLoading('cost');
        try {
            const params = this._baseParams();
            const bucket = this._chooseBucket();
            const [costSeries, topSpans, cacheHitRate, cacheEconomics, reasoningShare,
                   retryStats, errorRate, contextTypeSplit, agentsRollup, projectsRollup] =
                await Promise.all([
                    this.api.getCostSeries({ ...params, bucket }),
                    this.api.getTopSpans({ ...params, limit: 20 }),
                    this.api.getCacheHitRate(params).catch(() => null),
                    this.api.getCacheEconomics({ ...params, bucket_secs: bucket }).catch(() => null),
                    this.api.getReasoningShare(params).catch(() => null),
                    this.api.getRetryStats(params).catch(() => null),
                    this.api.getErrorRate(params).catch(() => []),
                    this.api.getContextTypeSplit(params).catch(() => null),
                    this.api.getAgents({ ...params, bucket_secs: bucket }).catch(() => null),
                    this.api.getProjects(params).catch(() => null),
                ]);

            const summary = this.lastSummary || { summary: {} };
            const cacheRead = summary.summary?.total_cache_read_tokens ?? 0;
            const cacheCreate = summary.summary?.total_cache_creation_tokens ?? 0;
            const totalInput = summary.summary?.total_input_tokens ?? 0;
            const cacheDenom = cacheRead + cacheCreate + totalInput;
            const cachePct = cacheDenom > 0 ? (cacheRead / cacheDenom) * 100 : 0;

            const fmt = n => Number(n).toLocaleString();

            // #insight-4: surface zero-cache models prominently
            const zeroCacheModels = (() => {
                const models = cacheEconomics && Array.isArray(cacheEconomics.models)
                    ? cacheEconomics.models : [];
                return models
                    .filter(m => (m.cache_read_tokens || 0) === 0 &&
                                 (m.cache_write_tokens || 0) === 0)
                    .map(m => m.model);
            })();

            const cacheCard = `
                <div class="usage-summary-cards">
                    <div class="usage-gauge-card">
                        <div class="usage-card-label">Cache hit rate</div>
                        <div class="usage-card-value">${cachePct.toFixed(1)}%</div>
                        <div class="gauge-bar"><div class="gauge-fill" style="width:${cachePct.toFixed(2)}%"></div></div>
                        <div class="gauge-hint">${fmt(cacheRead)} / ${fmt(cacheDenom)} tokens served from cache</div>
                    </div>
                    ${this._buildRetryGauge(retryStats)}
                </div>
                ${zeroCacheModels.length ? `<p class="table-hint insight-alert">⚠ No caching observed for: ${zeroCacheModels.map(m => `<strong>${this._esc(m)}</strong>`).join(', ')} — these models send full context every turn.</p>` : ''}`;

            const html = [
                cacheCard,
                this._buildCostChart(costSeries || [], bucket),
                this._buildCacheEconomics(cacheEconomics, cacheHitRate || [], bucket),
                this._buildTopNSection(topSpans || [], errorRate || []),
                this._buildReasoningShare(reasoningShare),
                this._buildAgents(agentsRollup, bucket),
                this._buildProjects(projectsRollup),
                this._buildByModelByProvider(summary),
                this._buildContextTypeSplit(contextTypeSplit || []),
            ].filter(Boolean).join('');

            this._setSectionBody('cost', html);
            this._attachTopNDropdownHandler(params);
            this._prefillDateInputsFromData(costSeries, bucket);
            this.loadedSections.add('cost');
        } catch (err) {
            this._setSectionError('cost', err);
        }
    }

    _buildByModelByProvider(data) {
        if (!data || !data.by_model) return '';
        const fmt = n => Number(n).toLocaleString();
        const modelRows = (data.by_model || []).map(m => `
            <tr>
                <td>${this._esc(m.model)}</td>
                <td>${fmt(m.requests)}</td>
                <td>${fmt(m.input_tokens)}</td>
                <td>${fmt(m.output_tokens)}</td>
                <td>${fmt(m.input_tokens + m.output_tokens)}</td>
            </tr>`).join('');
        const systemRows = (data.by_system || []).map(s => `
            <tr>
                <td>${this._esc(s.system)}</td>
                <td>${fmt(s.requests)}</td>
                <td>${fmt(s.input_tokens + s.output_tokens)}</td>
            </tr>`).join('');
        return `
            <h3>By model</h3>
            <table class="data-table">
                <thead><tr>
                    <th>Model</th><th>Requests</th><th>Input tokens</th><th>Output tokens</th><th>Total tokens</th>
                </tr></thead>
                <tbody>${modelRows}</tbody>
            </table>
            <h3>By provider</h3>
            <table class="data-table">
                <thead><tr>
                    <th>Provider</th><th>Requests</th><th>Total tokens</th>
                </tr></thead>
                <tbody>${systemRows}</tbody>
            </table>`;
    }

    async _loadLatencySection() {
        this._setSectionLoading('latency');
        try {
            const params = this._baseParams();
            const bucket = this._chooseBucket();
            // Daily throughput needs an explicit window spanning more than one
            // local day (calendar-day bucketing, issue #144).
            let dailyThroughput = null;
            let dailyTz = null;
            if (params.start_time) {
                const days = (params.end_time - params.start_time) / (86_400 * 1_000_000_000);
                if (days >= 2) {
                    dailyTz = this._localTimezone() || 'UTC';
                    dailyThroughput = await this.api.getLatencyPercentiles({
                        ...params,
                        calendar_day: '1',
                        timezone: dailyTz,
                        metrics: 'duration',
                    }).catch(() => null);
                }
            }
            const [latencyStats, latencySeries, latencyByContext, conversationDepth, latencyPercentiles, durationDist] = await Promise.all([
                this.api.getLatencyStats(params),
                this.api.getLatencySeries(params).catch(() => null),
                this.api.getLatencyByContext(params).catch(() => null),
                this.api.getConversationDepth(params).catch(() => null),
                this.api.getLatencyPercentiles(params).catch(() => null),
                this.api.getDistribution({ metric: 'llm_duration', scale: 'log', ...params }).catch(() => null),
            ]);

            const convCard = this._buildConversationDepthCard(conversationDepth);
            const insightCards = this._buildLatencyInsightCards(latencyStats || []);
            const allCards = [convCard, insightCards].filter(Boolean).join('');
            const cards = allCards ? `<div class="usage-summary-cards">${allCards}</div>` : '';

            const html = [
                cards,
                this._buildLatencyTable(latencyStats || []),
                this._buildLatencySeriesChart(latencySeries || [], bucket),
                this._buildDailyThroughputTable(dailyThroughput, dailyTz),
                this._buildLatencyPercentilesChart(latencyPercentiles),
                this._buildDistributionChart('Request duration distribution', durationDist),
                this._buildLatencyByContext(latencyByContext || []),
            ].filter(Boolean).join('');

            this._setSectionBody('latency', html);
            this._bindLatencyCharts();
            this.loadedSections.add('latency');
        } catch (err) {
            this._setSectionError('latency', err);
        }
    }

    /**
     * Build insight cards for the Latency section header.
     *
     * Computes the ratio of TTFT to total duration per model. When the median
     * ratio is > 0.85 across models with TTFT data, the wait is almost entirely
     * Anthropic/provider-side inference time — not tooling, context size, or
     * local overhead. We surface this as a plain-language diagnosis card so
     * users don't have to interpret the numbers themselves.
     *
     * @param {Array} latencyStats - rows from /api/genai/latency_stats
     * @returns {string} HTML for one or more diagnosis cards, or '' if not enough data
     */
    _buildLatencyInsightCards(latencyStats) {
        const buffered = latencyStats.filter(s => s.ttft_degenerate);
        const bufferedCard = buffered.length === 0 ? '' : `
            <div class="usage-gauge-card latency-insight-card">
                <div class="usage-card-label">⚠️ Streaming diagnosis</div>
                <div class="usage-card-value">Buffered responses</div>
                <div class="gauge-hint">
                    ${buffered.map(s => {
                        const pct = Math.round((s.ttft_degenerate_count || 0) * 100 / s.ttft_count);
                        return `${this._esc(s.model || '—')}: ${pct}% of TTFT values were near full response duration`;
                    }).join('<br>')}
                </div>
            </div>`;

        // Only consider models with TTFT data and at least 5 calls
        const withTtft = latencyStats.filter(s =>
            !s.ttft_degenerate &&
            s.ttft_count > 0 &&
            s.ttft_p50_ms != null &&
            s.p50_ms != null &&
            s.p50_ms > 0 &&
            (s.count || 0) >= 5
        );
        if (withTtft.length === 0) return bufferedCard;

        // Compute TTFT/duration ratio at p50 per model
        const ratios = withTtft.map(s => ({
            model: s.model || '—',
            ratio: s.ttft_p50_ms / s.p50_ms,
            p50_ms: s.p50_ms,
            ttft_p50_ms: s.ttft_p50_ms,
            p95_ms: s.p95_ms,
            count: s.count,
        }));

        const medianRatio = ratios.slice().sort((a, b) => a.ratio - b.ratio)[Math.floor(ratios.length / 2)].ratio;

        if (medianRatio < 0.85) return bufferedCard;

        // Build per-model lines for the detail
        const modelLines = ratios.map(r => {
            const ratioStr = (r.ratio * 100).toFixed(0);
            const ttftStr = this._formatDuration(r.ttft_p50_ms);
            const totalStr = this._formatDuration(r.p50_ms);
            return `<li><strong>${this._esc(r.model)}</strong>: TTFT ${ttftStr} of ${totalStr} total (${ratioStr}% inference)</li>`;
        }).join('');

        const overallPct = (medianRatio * 100).toFixed(0);

        return bufferedCard + `
            <div class="usage-gauge-card latency-insight-card">
                <div class="usage-card-label">
                    🔍 Latency diagnosis
                </div>
                <div class="usage-card-value">${overallPct}% inference</div>
                <div class="gauge-bar">
                    <div class="gauge-fill" style="width:${Math.min(medianRatio * 100, 100).toFixed(1)}%"></div>
                </div>
                <div class="gauge-hint">
                    Time-to-first-token accounts for ~${overallPct}% of total response time.
                    The wait is almost entirely provider-side inference, not local tooling,
                    context size, or network overhead.
                </div>
                <details class="latency-insight-detail">
                    <summary>Per-model breakdown</summary>
                    <ul class="latency-insight-model-list">${modelLines}</ul>
                    <p class="latency-insight-tip">
                        💡 To reduce average latency, route lighter turns to a faster model
                        (e.g. Sonnet instead of Opus). Context size, tool count, and prompt
                        length are <em>not</em> the bottleneck here.
                    </p>
                </details>
            </div>`;
    }

    async _loadReliabilitySection() {
        this._setSectionLoading('reliability');
        try {
            const params = this._baseParams();
            const [finishReasons, errorRate, errorTypes, truncationRate, modelDrift,
                   stopReasons] = await Promise.all([
                this.api.getFinishReasons(params),
                this.api.getErrorRate(params),
                this.api.getErrorTypes(params).catch(() => null),
                this.api.getTruncationRate(params).catch(() => null),
                this.api.getModelDrift(params).catch(() => null),
                this.api.getStopReasons(params).catch(() => null),
            ]);

            const reasons = Array.isArray(finishReasons) ? finishReasons : [];
            const truncCount = reasons
                .filter(r => String(r.reason || '').toLowerCase() === 'max_tokens')
                .reduce((acc, r) => acc + (r.count || 0), 0);
            const totalCount = reasons.reduce((acc, r) => acc + (r.count || 0), 0);
            const truncPct = totalCount > 0 ? (truncCount / totalCount) * 100 : 0;
            const fmt = n => Number(n).toLocaleString();

            const truncCard = totalCount > 0 ? `
                <div class="usage-summary-cards">
                    <div class="usage-gauge-card">
                        <div class="usage-card-label">Truncation rate</div>
                        <div class="usage-card-value">${truncPct.toFixed(1)}%</div>
                        <div class="gauge-bar"><div class="gauge-fill ${truncPct > 0 ? 'gauge-fill-warning' : ''}" style="width:${truncPct.toFixed(2)}%"></div></div>
                        <div class="gauge-hint">${fmt(truncCount)} / ${fmt(totalCount)} responses hit max_tokens</div>
                    </div>
                </div>` : '';

            const html = [
                truncCard,
                this._buildFinishReasons(reasons),
                this._buildStopReasons(stopReasons || []),
                this._buildTruncationRate(truncationRate || []),
                this._buildErrorRate(errorRate || []),
                this._buildErrorTypes(errorTypes || []),
                this._buildModelDrift(modelDrift || []),
            ].filter(Boolean).join('');

            this._setSectionBody('reliability', html);
            this.loadedSections.add('reliability');
        } catch (err) {
            this._setSectionError('reliability', err);
        }
    }

    async _loadBehaviorSection() {
        this._setSectionLoading('behavior');
        try {
            const params = this._baseParams();
            const [toolUsage, retrievalStats, requestParamProfile, callsSeries,
                   toolApprovals, toolErrors, hourOfDay] = await Promise.all([
                this.api.getToolUsage(params),
                this.api.getRetrievalStats(params).catch(() => null),
                this.api.getRequestParamProfile(params).catch(() => null),
                this.api.getCallsSeries(params).catch(() => null),
                this.api.getToolApprovals(params).catch(() => null),
                this.api.getToolErrors(params).catch(() => null),
                this.api.getHourOfDay(params).catch(() => null),
            ]);

            const html = [
                this._buildCallsChart(callsSeries || []),
                this._buildHourOfDay(hourOfDay || []),
                this._buildToolUsage(toolUsage || []),
                this._buildToolApprovals(toolApprovals),
                this._buildToolErrors(toolErrors || []),
                this._buildRetrievalStats(retrievalStats),
                this._buildRequestParamProfile(requestParamProfile),
            ].filter(Boolean).join('');

            this._setSectionBody('behavior', html);
            this.loadedSections.add('behavior');
        } catch (err) {
            this._setSectionError('behavior', err);
        }
    }

    async _loadRolesSection() {
        this._setSectionLoading('roles');
        try {
            const params = this._baseParams();
            const response = await this.api.getAgentRoles(params);
            const roles = (response && response.roles) || [];
            const html = this._buildAgentRoles(response);
            this._setSectionBody('roles', html ||
                '<div class="empty-state-hint">No agent-role data in this window (opencode only).</div>');
            const statEl = document.getElementById('analytics-section-stat-roles');
            if (statEl) {
                statEl.textContent = roles.length
                    ? `${roles.length} role${roles.length === 1 ? '' : 's'}`
                    : '—';
            }
            this.loadedSections.add('roles');
        } catch (err) {
            this._setSectionError('roles', err);
        }
    }

    async _loadProvidersSection() {
        this._setSectionLoading('providers');
        try {
            const params = this._baseParams();
            const response = await this.api.getProviderMix(params);
            const providers = (response && response.providers) || [];
            const html = this._buildProviderMix(response);
            this._setSectionBody('providers', html ||
                '<div class="empty-state-hint">No provider × model data in this window.</div>');
            const statEl = document.getElementById('analytics-section-stat-providers');
            if (statEl) {
                statEl.textContent = providers.length
                    ? `${providers.length} provider${providers.length === 1 ? '' : 's'}`
                    : '—';
            }
            this.loadedSections.add('providers');
        } catch (err) {
            this._setSectionError('providers', err);
        }
    }

    // ── Renderers (preserved from the old usage view) ────────────────────────

    _buildRetrievalStats(stats) {
        if (!stats || !stats.total_retrievals) return '';
        const fmt = n => Number(n).toLocaleString();
        const avgDocs = stats.avg_documents_per_query != null
            ? Number(stats.avg_documents_per_query).toFixed(2)
            : '—';
        const avgScore = stats.avg_top_document_score != null
            ? Number(stats.avg_top_document_score).toFixed(3)
            : null;

        const summaryLine = `
            <div class="retrieval-summary">
                <span><strong>${fmt(stats.total_retrievals)}</strong> retrievals</span>
                <span>·</span>
                <span><strong>${avgDocs}</strong> avg docs / query</span>
                ${avgScore !== null ? `<span>·</span><span><strong>${avgScore}</strong> avg top-1 score</span>` : ''}
            </div>`;

        const topQueries = Array.isArray(stats.top_queries) ? stats.top_queries : [];
        const topTable = topQueries.length > 0 ? `
            <table class="data-table">
                <thead><tr>
                    <th>Query</th><th>Retrievals</th><th>Avg docs</th><th>Avg top score</th>
                </tr></thead>
                <tbody>${topQueries.map(q => {
                    const full = String(q.query ?? '');
                    const truncated = full.length > 80 ? full.slice(0, 80) + '…' : full;
                    const avgDocsQ = q.avg_documents != null ? Number(q.avg_documents).toFixed(2) : '—';
                    const avgScoreQ = q.avg_top_score != null ? Number(q.avg_top_score).toFixed(3) : '—';
                    return `
                        <tr>
                            <td title="${this._esc(full)}">${this._esc(truncated)}</td>
                            <td>${fmt(q.count || 0)}</td>
                            <td>${this._esc(avgDocsQ)}</td>
                            <td>${this._esc(avgScoreQ)}</td>
                        </tr>`;
                }).join('')}</tbody>
            </table>` : '';

        return `
            <h3>Retrieval (RAG) activity</h3>
            ${summaryLine}
            ${topTable}
        `;
    }

    _formatDuration(ms) {
        if (ms == null) return '—';
        return ms < 10000 ? `${Number(ms).toLocaleString()} ms` : `${(ms / 1000).toFixed(1)} s`;
    }

    _buildRetryGauge(retryStats) {
        if (!retryStats || !retryStats.total_llm_calls) return '';
        const rate = retryStats.retry_rate || 0;
        const pct = rate * 100;
        const fmt = n => Number(n).toLocaleString();
        return `
                <div class="usage-gauge-card">
                    <div class="usage-card-label">Retry rate</div>
                    <div class="usage-card-value">${pct.toFixed(1)}%</div>
                    <div class="gauge-bar"><div class="gauge-fill ${pct > 0 ? 'gauge-fill-warning' : ''}" style="width:${pct.toFixed(2)}%"></div></div>
                    <div class="gauge-hint">${fmt(retryStats.retried_calls || 0)} of ${fmt(retryStats.total_llm_calls)} calls retried (${fmt(retryStats.extra_attempts || 0)} extra attempts)</div>
                </div>`;
    }

    _formatTokensK(n) {
        if (n == null) return '—';
        if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
        return String(n);
    }

    _buildLatencyTable(latencyStats) {
        if (!latencyStats.length) {
            return `<h3>Latency by model</h3><div class="empty-state-hint">No latency data in this window.</div>`;
        }
        const fmt = n => Number(n).toLocaleString();
        const rows = latencyStats.map(s => {
            const buffered = s.ttft_degenerate
                ? `buffered (${Math.round((s.ttft_degenerate_count || 0) * 100 / s.ttft_count)}%)`
                : null;
            const ttftP50 = buffered
                ? buffered
                : (s.ttft_count > 0 ? this._formatDuration(s.ttft_p50_ms) : '—');
            const ttftP95 = buffered
                ? buffered
                : (s.ttft_count > 0 ? this._formatDuration(s.ttft_p95_ms) : '—');

            // p10/p50/p90 are the primary triple (#119): lower-tail,
            // median, upper-reference. † marks weak lower tails (n < 10).
            const tpsP10 = s.derived_tokens_per_sec_p10 != null ? Math.round(s.derived_tokens_per_sec_p10) : null;
            const tpsP50 = s.derived_tokens_per_sec_p50 != null ? Math.round(s.derived_tokens_per_sec_p50) : null;
            const tpsP90 = s.derived_tokens_per_sec_p90 != null ? Math.round(s.derived_tokens_per_sec_p90) : null;
            const tpsCell = (tpsP10 != null && tpsP50 != null && tpsP90 != null)
                ? `${tpsP10} / ${tpsP50} / ${tpsP90} tok/s`
                : '—';
            const tpN = s.throughput_sample_count || 0;
            const nCell = tpN > 0 ? (tpN < 10 ? `${tpN}†` : String(tpN)) : '—';

            const ctxP50 = this._formatTokensK(s.input_tokens_p50);
            const ctxP95 = this._formatTokensK(s.input_tokens_p95);
            const ctxP99 = this._formatTokensK(s.input_tokens_p99);
            const ctxCell = (s.input_tokens_p50 != null) ? `${ctxP50} / ${ctxP95} / ${ctxP99}` : '—';

            const ratioP50 = s.output_input_ratio_p50 != null ? `${Number(s.output_input_ratio_p50).toFixed(2)}×` : null;
            const ratioP95 = s.output_input_ratio_p95 != null ? `${Number(s.output_input_ratio_p95).toFixed(2)}×` : null;
            const ratioCell = (ratioP50 != null && ratioP95 != null) ? `${ratioP50} / ${ratioP95}` : '—';

            return `
                <tr>
                    <td>${this._esc(s.model || '—')}</td>
                    <td class="num">${fmt(s.count || 0)}</td>
                    <td class="num">${this._esc(this._formatDuration(s.avg_ms))}</td>
                    <td class="num">${this._esc(this._formatDuration(s.p50_ms))}</td>
                    <td class="num">${this._esc(this._formatDuration(s.p95_ms))}</td>
                    <td class="num">${this._esc(this._formatDuration(s.p99_ms))}</td>
                    <td class="num">${this._esc(ttftP50)}</td>
                    <td class="num">${this._esc(ttftP95)}</td>
                    <td class="num">${this._esc(tpsCell)}</td>
                    <td class="num">${this._esc(nCell)}</td>
                    <td class="num">${this._esc(ctxCell)}</td>
                    <td class="num">${this._esc(ratioCell)}</td>
                </tr>`;
        }).join('');
        return `
            <h3>Latency by model</h3>
            <p class="table-hint">TTFT is emitter-supplied. “Buffered” means most values were near complete request duration, so no stream was observed. Tok/s is derived end-to-end — span duration includes provider, queue and network time, not pure generation rate. † = fewer than 10 throughput samples, so the p10 is a weak estimate.</p>
            <table class="data-table latency-table">
                <thead><tr>
                    <th>Model</th><th>Calls</th><th>Avg</th><th>P50</th><th>P95</th><th>P99</th><th>TTFT P50</th><th>TTFT P95</th>
                    <th title="Derived end-to-end output throughput per call: output tokens / span duration (raw ns). Span duration includes provider, queue and network time — not pure generation throughput. Lower-tail / median / upper-reference.">Tok/s* (p10/p50/p90)</th>
                    <th title="Calls with positive output and duration — the throughput sample, distinct from Calls">N*</th>
                    <th>Context (p50/p95/p99)</th>
                    <th title="Output divided by uncached input, cache reads, and cache creation">Out/context ratio (p50/p95)</th>
                </tr></thead>
                <tbody>${rows}</tbody>
            </table>`;
    }

    /** Local IANA timezone name, or null when the browser doesn't expose one. */
    _localTimezone() {
        try {
            return Intl.DateTimeFormat().resolvedOptions().timeZone || null;
        } catch {
            return null;
        }
    }

    /**
     * Daily output-throughput table from the calendar-day percentile grid
     * (issue #119 slice #144). One row per day × model with calls, the
     * throughput-eligible sample and the p10/p50/p90 tok/s triple. Days with
     * no calls are omitted; null percentiles render as —.
     *
     * @param {?object} resp - /api/genai/latency_percentiles response in calendar_day mode
     * @param {?string} tz   - IANA timezone the buckets align to
     * @returns {string} HTML block ('' when there is nothing to show)
     */
    async _loadCapabilitiesSection() {
        this._setSectionLoading('capabilities');
        try {
            const params = this._baseParams();
            const resp = await this.api.getGenAiCapabilities(params).catch(() => null);
            this._setSectionBody('capabilities', this._buildCapabilitiesTable(resp));
            this.loadedSections.add('capabilities');
        } catch (err) {
            this._setSectionError('capabilities', err);
        }
    }

    /**
     * Model-performance diagnosis section (#121/#154). The current interval
     * is the page's time window; the preceding window is derived by the API
     * and the rolling baseline is six equal windows before it. Both the raw
     * comparison and the deterministic assessments come back in one object —
     * this section renders them, it never recomputes a percentage or
     * reclassifies.
     */
    async _loadModelPerformanceSection() {
        this._setSectionLoading('model_performance');
        try {
            const params = this._baseParams();
            if (!params.start_time || !params.end_time) {
                this._setSectionBody('model_performance', `
                    <h3>Model performance</h3>
                    <div class="empty-state-hint">Pick a time window (start and end) to diagnose model performance against the preceding interval.</div>`);
                this.loadedSections.add('model_performance');
                return;
            }
            const span = params.end_time - params.start_time;
            params.rolling_ns = span * 6;
            const tz = this._localTimezone();
            if (tz) params.timezone = tz;
            const resp = await this.api.getModelPerformance(params).catch(() => null);
            this._setSectionBody('model_performance', this._buildModelPerformanceTable(resp));
            this.loadedSections.add('model_performance');
        } catch (err) {
            this._setSectionError('model_performance', err);
        }
    }

    _mpFmtValue(v, metric) {
        if (v == null) return '—';
        if (metric === 'error_rate') return `${(v * 100).toFixed(1)}%`;
        if (metric === 'throughput') return `${Math.round(v)} tok/s`;
        return `${Math.round(v)} ms`; // duration, ttft
    }

    _mpFmtDelta(d, metric) {
        // Wording mirrors the CLI/TUI: "pct unavailable" when the percentage
        // is undefined (zero baseline), "n/a" when there is no baseline at all.
        if (!d) return 'n/a';
        const abs = metric === 'error_rate'
            ? `${(d.absolute * 100).toFixed(1)} pts`
            : `${d.absolute.toFixed(1)}`;
        const sign = d.absolute > 0 ? '+' : '';
        const rel = d.relative != null
            ? `(${(d.relative * 100).toFixed(2)}%)`
            : '(pct unavailable)';
        return `${sign}${abs} ${rel}`;
    }

    _mpClassCell(m) {
        let cls = '';
        if (m.class === 'typical_regression' || m.class === 'tail_regression') cls = ' class="bad"';
        else if (m.class === 'workload_shift_correlated' || m.class === 'error_associated' || m.class === 'mixed_evidence') cls = ' class="warn"';
        else if (m.class === 'insufficient_telemetry') cls = ' class="dim"';
        const notes = (m.notes || []).length ? ` title="${this._esc(m.notes.join(' · '))}"` : '';
        return `<td${cls}${notes}>${this._esc(m.class)}</td>`;
    }

    _mpConfCell(conf) {
        const cls = (conf === 'insufficient' || conf === 'low') ? ' class="warn"' : '';
        return `<td${cls}>${this._esc(conf)}</td>`;
    }

    _buildModelPerformanceTable(diag) {
        if (!diag || !(diag.identities || []).length) {
            return `<h3>Model performance</h3>
                <div class="empty-state-hint">No LLM request spans in this window.</div>`;
        }
        const fmtIso = (ns) => new Date(ns / 1e6).toISOString().replace('.000Z', 'Z');
        const win = (w) => w ? `[${fmtIso(w.start_time)} → ${fmtIso(w.end_time)})` : null;
        const meta = [
            `Current ${win(diag.current_window)}`,
            `Preceding ${win(diag.preceding_window)}`,
            `Rolling ${diag.rolling_window ? win(diag.rolling_window) : '— (disabled)'}`,
        ];
        if (diag.timezone) meta.push(`Timezone ${this._esc(diag.timezone)}`);
        if (diag.truncated) meta.push('bounded sample — older spans excluded');

        const assessments = diag.assessments || [];
        const body = assessments.map((a) => {
            const identity = [a.provider, a.model].filter(Boolean).join('/') || '(unknown)';
            // Route to the full evidence for this identity and interval.
            const evidenceParams = new URLSearchParams({
                start_time: String(diag.current_window.start_time),
                end_time: String(diag.current_window.end_time),
            });
            if (a.model) evidenceParams.set('model', a.model);
            if (a.provider) evidenceParams.set('provider', a.provider);
            const identityCell = (rowspan) => `
                <td${rowspan ? ` rowspan="${rowspan}"` : ''}>
                    ${this._esc(identity)}
                    <br><span class="dim">fp ${this._esc(a.emitter_fingerprint || '—')} · overall ${this._esc(a.overall_class)} (${this._esc(a.overall_confidence)})</span>
                    <br><a href="/api/genai/model-performance?${evidenceParams.toString()}" target="_blank" rel="noopener">full JSON</a>
                </td>`;
            return (a.metrics || []).map((m, j) => {
                const samples = m.eligible_rolling != null
                    ? `${m.eligible_current}/${m.eligible_preceding}/${m.eligible_rolling}`
                    : `${m.eligible_current}/${m.eligible_preceding}`;
                return `<tr>
                    ${j === 0 ? identityCell(a.metrics.length) : ''}
                    <td>${this._esc(m.metric)}</td>
                    <td class="num">${this._mpFmtValue(m.preceding_median, m.metric)}</td>
                    <td class="num">${this._mpFmtValue(m.current_median, m.metric)}</td>
                    <td class="num">${this._mpFmtDelta(m.median_delta_vs_preceding, m.metric)}</td>
                    <td class="num">${samples}</td>
                    ${this._mpClassCell(m)}
                    ${this._mpConfCell(m.confidence)}
                </tr>`;
            }).join('');
        }).join('');

        return `
            <h3>Model performance</h3>
            <p class="table-hint">${meta.join(' · ')}. Classes are deterministic from named thresholds — they are reported states, not opinions. Workload and error relationships are <em>correlation, not causation</em>. <span class="dim">pct unavailable</span> means the percentage is undefined (zero baseline) — it is never a measured zero.</p>
            <table class="data-table model-performance-table">
                <thead><tr>
                    <th>Provider / Model</th><th>Metric</th>
                    <th>Base p50 (preceding)</th><th>Now p50</th>
                    <th>Δ vs preceding</th><th>N cur/prev(/roll)</th>
                    <th>Class</th><th>Conf</th>
                </tr></thead>
                <tbody>${body}</tbody>
            </table>
            <p class="table-hint">classes: no_material_change · typical_regression · tail_regression · workload_shift_correlated · error_associated · mixed_evidence (both baselines are reported) · insufficient_telemetry — confidence: high · medium · low · insufficient. N is eligible sample counts (current/preceding/rolling when enabled); below 10 current samples every class is insufficient_telemetry.</p>`;
    }

    _capabilityCell(m) {
        if (!m) return '<td class="num">—</td>';
        const derivation = m.derivation && m.derivation !== 'native' ? `/${m.derivation}` : '';
        const counts = m.observed_count > 0
            ? `${m.valid_count}/${m.observed_count} obs`
            : `0/${m.eligible_count} elig`;
        let cls = '';
        if (m.quality === 'invalid' || m.quality === 'degenerate') cls = ' class="warn"';
        else if (m.availability === 'absent') cls = ' class="dim"';
        return `<td${cls} title="valid ${m.valid_count} / observed ${m.observed_count} / eligible ${m.eligible_count}; invalid ${m.invalid_count}">${m.availability}/${m.quality}${derivation} (${counts})</td>`;
    }

    _correlationCell(c) {
        if (!c || c.rule === 'none') return '<td class="num dim">—</td>';
        const rejected = (c.rejected_count + c.ambiguous_count) > 0 ? ' class="num warn"' : ' class="num"';
        return `<td${rejected} title="${this._esc(c.rule)}: ${c.matched_count} matched, ${c.unmatched_count} unmatched, ${c.rejected_count} rejected, ${c.ambiguous_count} ambiguous candidates">${c.matched_count}/${c.unmatched_count}/${c.rejected_count}/${c.ambiguous_count}</td>`;
    }

    _buildCapabilitiesTable(resp) {
        const reports = (resp && resp.reports) || [];
        if (!reports.length) {
            return `<h3>Telemetry capabilities</h3>
                <div class="empty-state-hint">No LLM request spans in this window.</div>`;
        }
        const meta = [];
        meta.push(`${resp.canonical_span_count} canonical request span${resp.canonical_span_count === 1 ? '' : 's'}`);
        if (resp.duplicate_span_count > 0) meta.push(`${resp.duplicate_span_count} duplicate OTLP deliveries collapsed`);
        if (resp.truncated) meta.push('bounded sample — older spans excluded');
        const body = reports.map(r => {
            const identity = [r.provider, r.model].filter(Boolean).join('/') || '(unknown)';
            return `<tr>
                <td>${this._esc(identity)}</td>
                <td>${this._esc(r.emitter)}</td>
                <td class="num">${r.request_count}</td>
                ${this._capabilityCell(r.input_tokens)}
                ${this._capabilityCell(r.output_tokens)}
                ${this._capabilityCell(r.cache_creation_tokens)}
                ${this._capabilityCell(r.cache_read_tokens)}
                ${this._capabilityCell(r.ttft)}
                ${this._correlationCell(r.correlation)}
            </tr>`;
        }).join('');
        return `
            <h3>Telemetry capabilities</h3>
            <p class="table-hint">${meta.join(' · ')}. Cells are availability/quality(/derivation) with valid/observed counts. <span class="dim">absent</span> means the metric is not provided — it is never a measured zero. Emitters without a verified token signature stay <em>unavailable</em> instead of guessed values.</p>
            <table class="data-table capabilities-table">
                <thead><tr>
                    <th>Provider / Model</th><th>Emitter</th><th>Requests</th>
                    <th>Input tokens</th><th>Output tokens</th>
                    <th>Cache write</th><th>Cache read</th><th>TTFT</th><th>Correlation</th>
                </tr></thead>
                <tbody>${body}</tbody>
            </table>
            <p class="table-hint">availability: available · sparse · absent — quality: reliable · invalid · degenerate · not_assessed — derivation (shown when not native): correlated · unavailable. Correlation: matched/unmatched/rejected/ambiguous candidates under the group's join rule (— when no rule applies).</p>`
            + this._unidentifiedSection(resp);
    }

    _unidentifiedSection(resp) {
        const unidentified = (resp && resp.unidentified) || [];
        if (!unidentified.length) return '';
        const rows = unidentified.map(u => `
            <tr>
                <td class="num">${u.span_count}</td>
                <td>${u.required_attributes.map(a => `<code>${this._esc(a)}</code>`).join(' + ')}</td>
            </tr>`).join('');
        return `
            <h4>Unidentified emitters</h4>
            <p class="table-hint">LLM-ish spans no verified emitter signature matched, grouped by the attribute names a signature would still require. Attribute names only — no values or identifiers are exposed.</p>
            <table class="data-table">
                <thead><tr><th>Spans</th><th>Required attributes</th></tr></thead>
                <tbody>${rows}</tbody>
            </table>`;
    }

    _buildDailyThroughputTable(resp, tz) {
        const series = resp && resp.metrics && resp.metrics.duration;
        const models = (series && series.models) || {};
        const rows = [];
        for (const [model, points] of Object.entries(models)) {
            for (const p of points) {
                if (!p || (p.count || 0) === 0) continue; // omit empty days
                const nStar = p.throughput_sample_count || 0;
                const nCell = nStar > 0 ? (nStar < 10 ? `${nStar}†` : String(nStar)) : '—';
                const t10 = p.throughput_p10_tok_s != null ? Math.round(p.throughput_p10_tok_s) : null;
                const t50 = p.throughput_p50_tok_s != null ? Math.round(p.throughput_p50_tok_s) : null;
                const t90 = p.throughput_p90_tok_s != null ? Math.round(p.throughput_p90_tok_s) : null;
                const tpsCell = (t10 != null && t50 != null && t90 != null)
                    ? `${t10} / ${t50} / ${t90}`
                    : '—';
                const day = chartAxisLabel(p.timestamp, true);
                rows.push({ day, model, n: p.count || 0, nStar: nCell, tps: tpsCell });
            }
        }
        rows.sort((a, b) => a.day.localeCompare(b.day) || a.model.localeCompare(b.model));
        if (rows.length === 0) {
            return `<h3>Output throughput by day${tz ? ` (${this._esc(tz)})` : ''}</h3>
                <div class="empty-state-hint">No throughput data in this window.</div>`;
        }
        const body = rows.map(r => `
            <tr>
                <td>${this._esc(r.day)}</td>
                <td>${this._esc(r.model)}</td>
                <td class="num">${r.n}</td>
                <td class="num">${r.nStar}</td>
                <td class="num">${r.tps}</td>
            </tr>`).join('');
        return `
            <h3>Output throughput by day${tz ? ` (${this._esc(tz)})` : ''}</h3>
            <p class="table-hint">Tok/s is derived end-to-end output throughput per call (output tokens ÷ span duration); span duration includes provider, queue and network time, so this is not a provider-reported generation rate. Days with no calls are omitted. † = fewer than 10 throughput samples.</p>
            <table class="data-table daily-throughput-table">
                <thead><tr>
                    <th>Day</th><th>Model</th><th>Calls</th>
                    <th title="Calls with positive output and duration — the throughput sample, distinct from Calls">N*</th>
                    <th title="Derived end-to-end output throughput per call: output tokens / span duration (raw ns). Span duration includes provider, queue and network time — not pure generation throughput. Lower-tail / median / upper-reference.">Tok/s* (p10/p50/p90)</th>
                </tr></thead>
                <tbody>${body}</tbody>
            </table>`;
    }

    _buildLatencySeriesChart(points, bucketSecs) {
        if (!Array.isArray(points) || !points.length) {
            return `<h3>Latency over time</h3><div class="empty-state-hint">No latency data in this window.</div>`;
        }

        const bucketMap = new Map();
        for (const p of points) {
            const ts = p.timestamp;
            const n = p.count || 1;
            const existing = bucketMap.get(ts) || {
                timestamp: ts, count: 0, sum_avg: 0, max_p95: 0,
                sum_ttft: 0, ttft_n: 0, details: [],
            };
            existing.count += n;
            existing.sum_avg += (p.avg_ms || 0) * n;
            existing.max_p95 = Math.max(existing.max_p95, p.p95_ms || 0);
            if (p.avg_ttft_ms != null && !p.ttft_degenerate) {
                existing.sum_ttft += p.avg_ttft_ms * n;
                existing.ttft_n   += n;
            }
            existing.details.push(p);
            bucketMap.set(ts, existing);
        }
        const buckets = Array.from(bucketMap.values())
            .sort((a, b) => a.timestamp - b.timestamp)
            .map(b => ({
                ...b,
                avg_ms:    b.count > 0 ? b.sum_avg / b.count : 0,
                avg_ttft:  b.ttft_n  > 0 ? b.sum_ttft / b.ttft_n : null,
            }));

        const maxVal = buckets.reduce((m, b) => Math.max(m, b.max_p95), 0);
        if (maxVal === 0) return `<h3>Latency over time</h3><div class="empty-state-hint">No latency data in this window.</div>`;

        const width = 100, barGap = 0.5, chartHeight = 100;
        const barWidth = Math.max((width - barGap * (buckets.length - 1)) / buckets.length, 0.1);
        // Centre of each bar on the x-axis (used for the TTFT polyline points).
        const barCentreX = i => i * (barWidth + barGap) + barWidth / 2;

        const bars = buckets.map((b, i) => {
            const x = i * (barWidth + barGap);
            const p95H = (b.max_p95 / maxVal) * chartHeight;
            const avgH = Math.min((b.avg_ms / maxVal) * chartHeight, p95H);
            const tsDate = new Date(b.timestamp / 1_000_000);
            const modelLines = b.details.map(d => {
                const ttftStr = d.ttft_degenerate
                    ? ` · buffered (${Math.round((d.ttft_degenerate_count || 0) * 100 / d.ttft_count)}%)`
                    : (d.avg_ttft_ms != null ? ` · ttft ${Math.round(d.avg_ttft_ms)}ms` : '');
                return `  ${d.model || d.name || '(all)'}: avg ${Math.round(d.avg_ms)}ms · p95 ${d.p95_ms}ms · ${d.count} calls${ttftStr}`;
            }).join('\n');
            const ttftStr = b.avg_ttft != null ? `\nttft avg ${Math.round(b.avg_ttft)}ms` : '';
            const tip = `${formatTs(tsDate)}\navg ${Math.round(b.avg_ms)}ms  p95 ${b.max_p95}ms${ttftStr}\n${b.count} calls\n${modelLines}`;
            const p95Rect = `<rect class="latency-chart-bar-p95" x="${x.toFixed(3)}" y="${(chartHeight - p95H).toFixed(3)}" width="${barWidth.toFixed(3)}" height="${p95H.toFixed(3)}"><title>${this._esc(tip)}</title></rect>`;
            const avgRect = avgH > 0
                ? `<rect class="latency-chart-bar-avg" x="${x.toFixed(3)}" y="${(chartHeight - avgH).toFixed(3)}" width="${barWidth.toFixed(3)}" height="${avgH.toFixed(3)}"><title>${this._esc(tip)}</title></rect>`
                : '';
            return p95Rect + avgRect;
        }).join('');

        // TTFT overlay polyline — only rendered when at least two buckets have data.
        // Uses its own y-scale so short TTFT values don't disappear at the bottom.
        const ttftBuckets = buckets.filter(b => b.avg_ttft != null);
        let ttftPolyline = '';
        if (ttftBuckets.length >= 2) {
            const maxTtft = ttftBuckets.reduce((m, b) => Math.max(m, b.avg_ttft), 0);
            if (maxTtft > 0) {
                const pts = buckets
                    .map((b, i) => {
                        if (b.avg_ttft == null) return null;
                        const cx = barCentreX(i).toFixed(3);
                        const cy = (chartHeight - (b.avg_ttft / maxTtft) * chartHeight).toFixed(3);
                        return `${cx},${cy}`;
                    })
                    .filter(Boolean)
                    .join(' ');
                ttftPolyline = `<polyline class="latency-ttft-line" points="${pts}" fill="none"/>`;
            }
        }

        const multiDay = buckets.length > 1 &&
            new Date(buckets[0].timestamp / 1_000_000).toDateString() !==
            new Date(buckets[buckets.length - 1].timestamp / 1_000_000).toDateString();
        const labelFor = i => chartAxisLabel(buckets[i].timestamp, multiDay);
        let axisHtml = '';
        if (buckets.length > 0) {
            const left = this._esc(labelFor(0));
            const mid = buckets.length > 2 ? this._esc(labelFor(Math.floor(buckets.length / 2))) : '';
            const right = buckets.length > 1 ? this._esc(labelFor(buckets.length - 1)) : '';
            axisHtml = `<div class="cost-chart-axis-labels">
                <span class="cost-chart-axis-left">${left}</span>
                <span class="cost-chart-axis-mid">${mid}</span>
                <span class="cost-chart-axis-right">${right}</span>
            </div>`;
        }
        const peakP95 = buckets.reduce((m, b) => Math.max(m, b.max_p95), 0);
        const ttftLegend = ttftPolyline
            ? `<span class="latency-ttft-legend">— TTFT avg (own scale)</span>`
            : '';
        const hint = ttftPolyline
            ? 'Solid bar = avg; faded = p95; orange line = TTFT avg (own y-scale). Hover for per-model breakdown.'
            : 'Solid bar = avg; faded extension = p95. Hover for per-model breakdown.';
        const brushAttrs = this._brushAttrs(buckets.map(b => b.timestamp), bucketSecs);
        return `
            <h3>Latency over time — peak p95 ${peakP95.toLocaleString()} ms ${ttftLegend}</h3>
            <p class="table-hint">${hint}</p>
            <div class="cost-chart">
                <svg class="cost-chart-svg" viewBox="0 0 ${width} ${chartHeight}" preserveAspectRatio="none" ${brushAttrs}>
                    ${bars}
                    ${ttftPolyline}
                </svg>
                ${axisHtml}
            </div>`;
    }

    _buildLatencyByContext(bins) {
        if (!bins || !bins.length) return '';
        const fmt = n => Number(n).toLocaleString();
        const rows = bins.map(b => {
            const ttft = b.ttft_degenerate
                ? `buffered (${Math.round((b.ttft_degenerate_count || 0) * 100 / b.ttft_count)}%)`
                : (b.avg_ttft_ms != null ? this._formatDuration(b.avg_ttft_ms) : '—');
            return `
                <tr>
                    <td>${this._esc(b.bin)}</td>
                    <td>${this._esc(b.model || '—')}</td>
                    <td>${fmt(b.count || 0)}</td>
                    <td>${this._esc(this._formatDuration(b.avg_ms))}</td>
                    <td>${this._esc(this._formatDuration(b.p95_ms))}</td>
                    <td>${this._esc(this._formatDuration(b.max_ms))}</td>
                    <td>${this._esc(ttft)}</td>
                </tr>`;
        }).join('');
        return `
            <h3>Latency by context size</h3>
            <p class="table-hint">Response time broken down by prompt token count × model. Buffered TTFT means no stream was observed.</p>
            <table class="data-table">
                <thead><tr>
                    <th>Context bin (input tokens)</th><th>Model</th><th>Calls</th>
                    <th>Avg</th><th>P95</th><th>Max</th><th>TTFT avg</th>
                </tr></thead>
                <tbody>${rows}</tbody>
            </table>`;
    }

    _buildErrorRate(errorRate) {
        if (!errorRate.length || errorRate.every(r => (r.errors || 0) === 0)) {
            return '';
        }
        const sorted = [...errorRate].sort((a, b) => (b.error_rate || 0) - (a.error_rate || 0));
        const rows = sorted.map(r => {
            const rate = r.error_rate || 0;
            const pct = rate * 100;
            const warning = rate > 0.1;
            return `
                <div class="finish-reason-row">
                    <div class="finish-reason-name">${this._esc(r.model || '—')}</div>
                    <div class="finish-reason-bar"><div class="finish-reason-fill ${warning ? 'warning' : ''}" style="width:${pct.toFixed(2)}%"></div></div>
                    <div class="finish-reason-count">${r.errors || 0}/${r.total || 0} (${pct.toFixed(1)}%)</div>
                </div>`;
        }).join('');
        return `
            <h3>Error rate by model</h3>
            <div class="finish-reasons-list error-rate-list">${rows}</div>`;
    }

    _buildToolUsage(toolUsage) {
        if (!toolUsage.length) {
            return `<h3>Tool usage</h3><div class="empty-state-hint">No tool-use spans in this window.</div>`;
        }
        const fmt = n => Number(n).toLocaleString();
        // Sort by total wall-clock time descending so the most expensive tools surface first.
        const sorted = [...toolUsage].sort((a, b) => (b.total_duration_ms || 0) - (a.total_duration_ms || 0));
        const maxTotalMs = sorted.reduce((m, t) => Math.max(m, t.total_duration_ms || 0), 1);
        const rows = sorted.map(t => {
            const count = t.count || 0;
            const succ = t.success_count || 0;
            const rate = count > 0 ? (succ / count) * 100 : 0;
            const warn = rate < 90;
            const totalMs = t.total_duration_ms || 0;
            const barPct = Math.max(2, (totalMs / maxTotalMs) * 100);
            const totalStr = totalMs >= 60000
                ? `${(totalMs / 60000).toFixed(1)} min`
                : totalMs >= 1000
                    ? `${(totalMs / 1000).toFixed(1)} s`
                    : `${fmt(totalMs)} ms`;
            const isHeavy = totalMs > 300_000; // >5 min total
            return `
                <tr class="${warn ? 'tool-usage-warn' : ''}">
                    <td>${this._esc(t.tool_name || '—')}</td>
                    <td>${fmt(count)}</td>
                    <td>${rate.toFixed(1)}%</td>
                    <td>${fmt(t.error_count || 0)}</td>
                    <td>${this._esc(this._formatDuration(t.avg_duration_ms))}</td>
                    <td class="${isHeavy ? 'tool-total-heavy' : ''}">
                        <div class="tool-total-cell">
                            <div class="tool-time-bar-track">
                                <div class="tool-time-bar-fill${isHeavy ? ' tool-time-bar-heavy' : ''}" style="width:${barPct.toFixed(1)}%"></div>
                            </div>
                            <span class="tool-total-label">${this._esc(totalStr)}</span>
                        </div>
                    </td>
                </tr>`;
        }).join('');
        return `
            <h3>Tool usage</h3>
            <p class="table-hint">Sorted by total wall-clock time. Amber rows have success rate &lt; 90%; red total bar = &gt;5 min aggregate.</p>
            <table class="data-table tool-usage-table">
                <thead><tr>
                    <th>Tool</th><th>Calls</th><th>Success rate</th><th>Errors</th><th>Avg duration</th><th>Total time ▼</th>
                </tr></thead>
                <tbody>${rows}</tbody>
            </table>`;
    }

    _buildErrorTypes(rows) {
        if (!rows || !rows.length) return '';
        const sorted = [...rows].sort((a, b) => (b.count || 0) - (a.count || 0));
        const bucketColors = {
            rate_limit: '#e74c3c',
            timeout: '#e67e22',
            context_length: '#f39c12',
            content_filter: '#9b59b6',
            auth: '#c0392b',
            server_error: '#e74c3c',
            unknown: '#95a5a6',
        };
        const tableRows = sorted.map(r => {
            const color = bucketColors[r.bucket] || '#95a5a6';
            return `
                <tr>
                    <td><span class="bucket-chip" style="background:${color};color:#fff;padding:2px 6px;border-radius:3px;font-size:0.85em">${this._esc(r.bucket)}</span></td>
                    <td title="${this._esc(r.error_type)}">${this._esc(r.error_type.length > 40 ? r.error_type.slice(0, 40) + '…' : r.error_type)}</td>
                    <td>${this._esc(r.model || '—')}</td>
                    <td>${r.count || 0}</td>
                </tr>`;
        }).join('');
        return `
            <h3>Error type breakdown</h3>
            <table class="data-table">
                <thead><tr>
                    <th>Bucket</th><th>Error Type</th><th>Model</th><th>Count</th>
                </tr></thead>
                <tbody>${tableRows}</tbody>
            </table>`;
    }

    _buildModelDrift(rows) {
        if (!rows || !rows.length) return '';
        const drifted = rows.filter(r => r.differs);
        if (!drifted.length) {
            return `<h3>Model drift</h3><p class="empty-state-hint">No model drift detected — request and response models match for all calls.</p>`;
        }
        const tableRows = drifted.map(r => `
            <tr class="drift-warning">
                <td>${this._esc(r.request_model || '—')}</td>
                <td>⚠ ${this._esc(r.response_model || '—')}</td>
                <td>${r.count || 0}</td>
            </tr>`).join('');
        return `
            <h3>Model drift — provider rerouted to a different model</h3>
            <table class="data-table">
                <thead><tr>
                    <th>Requested</th><th>Served</th><th>Count</th>
                </tr></thead>
                <tbody>${tableRows}</tbody>
            </table>`;
    }

    _buildCostChart(costSeries, bucketSecs) {
        if (!costSeries.length) {
            return `<h3>Cost over time</h3><div class="empty-state-hint">No cost data in this window.</div>`;
        }

        const bucketMap = new Map();
        for (const row of costSeries) {
            const ts = row.timestamp;
            const cost = row.cost ?? 0;
            const existing = bucketMap.get(ts) || { timestamp: ts, cost: 0, models: {} };
            existing.cost += cost;
            existing.models[row.model] = (existing.models[row.model] || 0) + cost;
            bucketMap.set(ts, existing);
        }
        const buckets = Array.from(bucketMap.values()).sort((a, b) => a.timestamp - b.timestamp);
        const total = buckets.reduce((a, b) => a + b.cost, 0);
        const maxCost = buckets.reduce((a, b) => Math.max(a, b.cost), 0);

        const width = 100;
        const barGap = 0.5;
        const barWidth = buckets.length > 0 ? Math.max((width - barGap * (buckets.length - 1)) / buckets.length, 0.1) : 0;
        const chartHeight = 100;

        const bars = buckets.map((b, i) => {
            const h = maxCost > 0 ? (b.cost / maxCost) * chartHeight : 0;
            const x = i * (barWidth + barGap);
            const y = chartHeight - h;
            const breakdown = Object.entries(b.models)
                .filter(([, v]) => v > 0)
                .map(([m, v]) => `${m}: $${v.toFixed(4)}`)
                .join('\n');
            const tsDate = new Date(b.timestamp / 1_000_000);
            const title = `${formatTs(tsDate)}\n$${b.cost.toFixed(4)}${breakdown ? `\n${breakdown}` : ''}`;
            return `<rect class="cost-chart-bar" x="${x.toFixed(3)}" y="${y.toFixed(3)}" width="${barWidth.toFixed(3)}" height="${h.toFixed(3)}"><title>${this._esc(title)}</title></rect>`;
        }).join('');

        const multiDay = buckets.length > 1 &&
            new Date(buckets[0].timestamp / 1_000_000).toDateString() !==
            new Date(buckets[buckets.length - 1].timestamp / 1_000_000).toDateString();
        const labelFor = i => chartAxisLabel(buckets[i].timestamp, multiDay);

        let axisHtml = '';
        if (buckets.length > 0) {
            const left = this._esc(labelFor(0));
            const mid = buckets.length > 2
                ? this._esc(labelFor(Math.floor(buckets.length / 2)))
                : '';
            const right = buckets.length > 1
                ? this._esc(labelFor(buckets.length - 1))
                : '';
            axisHtml = `
                <div class="cost-chart-axis-labels">
                    <span class="cost-chart-axis-left">${left}</span>
                    <span class="cost-chart-axis-mid">${mid}</span>
                    <span class="cost-chart-axis-right">${right}</span>
                </div>`;
        }

        const brushAttrs = this._brushAttrs(buckets.map(b => b.timestamp), bucketSecs);
        return `
            <h3>Cost over time — total $${total.toFixed(4)} across ${buckets.length} bucket${buckets.length === 1 ? '' : 's'}</h3>
            <div class="cost-chart">
                <svg class="cost-chart-svg" viewBox="0 0 ${width} ${chartHeight}" preserveAspectRatio="none" ${brushAttrs}>
                    ${bars}
                </svg>
                ${axisHtml}
            </div>`;
    }

    // ── Top-N section: tab-driven ─────────────────────────────────────────────

    _buildTopNSection(topSpans, errorRate) {
        const tabs = [
            { id: 'cost',      label: 'Most expensive' },
            { id: 'slow',      label: 'Slowest' },
            { id: 'truncated', label: 'Truncated' },
            { id: 'sessions',  label: 'Sessions' },
            { id: 'convs',     label: 'Conversations' },
            { id: 'verbose',   label: 'Highest output/context' },
            { id: 'cache',     label: 'Cache efficiency' },
            { id: 'errors',    label: 'Error runs' },
        ];

        // Ensure active tab is valid; fall back to 'cost'.
        if (!tabs.find(t => t.id === this.topNSort)) this.topNSort = 'cost';

        const tabButtons = tabs.map(t =>
            `<button class="top-n-tab${t.id === this.topNSort ? ' active' : ''}" data-tab="${t.id}">${t.label}</button>`
        ).join('');

        // Cache the eagerly-fetched data so switching back is free.
        this._topNCostCache = topSpans || [];
        this._topNErrorCache = errorRate || [];

        let initialContent = '';
        if (this.topNSort === 'cost') {
            initialContent = this._renderSpanTable(topSpans || [], { extraCol: 'cost', emptyMsg: 'No expensive calls in this window.' });
        } else if (this.topNSort === 'errors') {
            initialContent = this._renderErrorRunsTable(errorRate || []);
        } else {
            initialContent = `<div class="empty-state-hint">Loading…</div>`;
        }

        return `
            <div class="top-n-section">
                <h3>Top 20 calls</h3>
                <div class="top-n-tabs" id="top-n-tabs">${tabButtons}</div>
                <div id="top-n-content">${initialContent}</div>
            </div>`;
    }

    _attachTopNDropdownHandler(params) {
        const tabBar = document.getElementById('top-n-tabs');
        if (!tabBar) return;

        const switchTab = async (id) => {
            this.topNSort = id;

            // Update active styling.
            tabBar.querySelectorAll('.top-n-tab').forEach(btn => {
                btn.classList.toggle('active', btn.dataset.tab === id);
            });

            const content = document.getElementById('top-n-content');
            if (!content) return;

            // Return cached data for the two eagerly-fetched tabs.
            if (id === 'cost') {
                content.innerHTML = this._renderSpanTable(this._topNCostCache, { extraCol: 'cost', emptyMsg: 'No expensive calls in this window.' });
                return;
            }
            if (id === 'errors') {
                content.innerHTML = this._renderErrorRunsTable(this._topNErrorCache);
                return;
            }

            content.innerHTML = `<div class="empty-state-hint">Loading…</div>`;
            const fetchers = {
                slow:      p => this.api.getTopSpans({...p, sort_by: 'duration'}),
                truncated: p => this.api.getTopSpans({...p, truncated_only: true}),
                sessions:  p => this.api.getTopSessions(p),
                convs:     p => this.api.getTopConversations(p),
                verbose:   p => this.api.getTopSpans({...p, sort_by: 'output_input_ratio'}),
                cache:     p => this.api.getTopSpans({...p, sort_by: 'cache_efficiency'}),
            };
            try {
                const data = await fetchers[id]({ ...params, limit: 20 });
                let html;
                if (id === 'sessions') {
                    html = this._renderGroupTable(data || [], 'session_id', 'Session ID');
                } else if (id === 'convs') {
                    html = this._renderGroupTable(data || [], 'conversation_id', 'Conversation ID');
                } else {
                    const extraCol = {slow: 'duration', truncated: 'finish_reason', verbose: 'ratio', cache: 'cache_rate'}[id] || 'cost';
                    html = this._renderSpanTable(data || [], { extraCol, emptyMsg: 'No matching spans in this window.' });
                }
                content.innerHTML = html;
            } catch (err) {
                content.innerHTML = `<div class="empty-state-hint">Failed to load: ${this._esc(err.message)}</div>`;
            }
        };

        tabBar.addEventListener('click', e => {
            const btn = e.target.closest('.top-n-tab');
            if (btn && btn.dataset.tab) switchTab(btn.dataset.tab);
        });

        // If the active tab is not one of the eagerly-cached ones, load it now.
        if (this.topNSort !== 'cost' && this.topNSort !== 'errors') {
            switchTab(this.topNSort);
        }
    }

    /**
     * Compute a cache-state label for a span row.
     * COLD  — cache_read=0 and cache_creation>50K (full context rebuild)
     * WARMING — cache_read present but <50% of token budget
     * HOT   — cache_read>80% of (cache_read + cache_creation + input_tokens)
     */
    _cacheStateLabel(row) {
        const read   = row.cache_read_tokens     || 0;
        const create = row.cache_creation_tokens || 0;
        const input  = row.input_tokens          || 0;
        const total  = read + create + input;
        if (total === 0) return null;
        if (read === 0 && create > 50_000) return 'cold';
        const hitPct = read / total;
        if (hitPct >= 0.8) return 'hot';
        if (hitPct >= 0.3) return 'warming';
        if (read === 0) return null;   // small request, no cache signal
        return 'warming';
    }

    _renderSpanTable(spans, { extraCol, emptyMsg }) {
        if (!spans.length) return `<div class="empty-state-hint">${emptyMsg}</div>`;
        const fmt = n => Number(n).toLocaleString();
        const anySession = spans.some(r => r.session_id);
        // Show cache column whenever we have cache token data on any row
        const anyCacheData = spans.some(r => (r.cache_creation_tokens || 0) + (r.cache_read_tokens || 0) > 0);

        const extraHeader = {
            cost:         '<th>Cost</th>',
            duration:     '<th>Duration</th>',
            finish_reason:'<th>Finish reason</th>',
            ratio:        '<th>Out/context ratio</th>',
            cache_rate:   '<th>Cache hit%</th>',
        }[extraCol] || '';

        const rows = spans.map(row => {
            const cost = row.cost ?? null;
            const costStr = cost === null
                ? `<span title="${this._esc(row.cost_reason || 'no pricing match')}">—</span>`
                : `$${cost.toFixed(4)}`;
            const costClass = cost !== null && cost >= 0.01 ? 'top-spans-cost-high' : '';

            const timeStr = formatTs(new Date((row.start_time ?? 0) / 1_000_000));
            const sessionCell = row.session_id
                ? `<span class="top-spans-session-cell">
                    <a href="#" onclick="window.app.navigateToSessionReport('${this._esc(row.session_id)}'); return false;" title="Session Report: ${this._esc(row.session_id)}">${this._esc(String(row.session_id).slice(0, 8))}</a>
                    <a href="#" class="cell-nav-link" onclick="window.app.navigateToLogsBySession('${this._esc(row.session_id)}'); return false;" title="View logs for this session">logs</a>
                   </span>`
                : '—';
            const traceCell = row.trace_id
                ? `<a href="#" onclick="window.app.navigateToTrace('${this._esc(row.trace_id)}'); return false;" title="${this._esc(row.trace_id)}">${this._esc(String(row.trace_id).slice(0, 8))}</a>`
                : '—';

            // Cache state badge
            const cacheState = anyCacheData ? this._cacheStateLabel(row) : null;
            const cacheLabels = {
                cold:    ['COLD',    'cache-state-cold',    'Full context rebuild — no cache reads, high creation cost'],
                warming: ['WARMING', 'cache-state-warming', 'Partial cache hit — context still filling'],
                hot:     ['HOT',     'cache-state-hot',     '>80% of tokens served from cache'],
            };
            const cacheBadge = cacheState
                ? (() => {
                    const [label, cls, tip] = cacheLabels[cacheState];
                    const read   = fmt(row.cache_read_tokens     || 0);
                    const create = fmt(row.cache_creation_tokens || 0);
                    return `<td><span class="cache-state-badge ${cls}" title="${tip}&#10;read: ${read} · created: ${create}">${label}</span></td>`;
                })()
                : (anyCacheData ? '<td>—</td>' : '');

            let extraCell = '';
            if (extraCol === 'cost') {
                extraCell = `<td class="${costClass}">${costStr}</td>`;
            } else if (extraCol === 'duration') {
                const ms = Math.round((row.duration ?? 0) / 1_000_000);
                extraCell = `<td>${ms.toLocaleString()}ms</td>`;
            } else if (extraCol === 'finish_reason') {
                extraCell = `<td>${this._esc(row.finish_reason || '—')}</td>`;
            } else if (extraCol === 'ratio') {
                const inp = (row.input_tokens || 0) + (row.cache_read_tokens || 0) + (row.cache_creation_tokens || 0);
                const out = row.output_tokens || 0;
                const ratio = inp > 0 ? (out / inp).toFixed(2) : '—';
                extraCell = `<td>${ratio}</td>`;
            } else if (extraCol === 'cache_rate') {
                const inp = (row.input_tokens || 0) + (row.cache_read_tokens || 0);
                const pct = inp > 0 ? ((row.cache_read_tokens || 0) / inp * 100).toFixed(1) : '—';
                extraCell = `<td>${pct}%</td>`;
            }

            return `<tr>
                <td>${this._esc(timeStr)}</td>
                <td>${this._esc(row.model || '—')}</td>
                ${anySession ? `<td>${sessionCell}</td>` : ''}
                <td class="num">${fmt(row.input_tokens ?? 0)}</td>
                <td class="num">${fmt(row.output_tokens ?? 0)}</td>
                ${anyCacheData ? cacheBadge : ''}
                ${extraCell}
                <td>${traceCell}</td>
            </tr>`;
        }).join('');

        return `<table class="data-table">
            <thead><tr>
                <th>Time</th><th>Model</th>
                ${anySession ? '<th>Session</th>' : ''}
                <th>Input</th><th>Output</th>
                ${anyCacheData ? '<th title="COLD = no cache reads, full context rebuild. WARMING = partial hit. HOT = >80% from cache.">Cache</th>' : ''}
                ${extraHeader}
                <th>Trace</th>
            </tr></thead>
            <tbody>${rows}</tbody>
        </table>`;
    }

    _renderGroupTable(rows, idField, idLabel) {
        const fmt = n => Number(n).toLocaleString();
        if (!rows.length) return `<div class="empty-state-hint">No data in this window.</div>`;
        const tableRows = rows.map(r => {
            const cost = r.cost ?? null;
            const costStr = cost === null ? '—' : `$${cost.toFixed(4)}`;
            const id = String(r[idField] || '—');
            // Session IDs → Session Report modal; conversation IDs → traces filtered by conversation.
            const navFn = idField === 'session_id'
                ? `window.app.navigateToSessionReport('${this._esc(id)}')`
                : `window.app.navigateToTracesByConversation('${this._esc(id)}')`;
            const idCell = id === '—' ? id
                : `<a href="#" onclick="${navFn}; return false;" title="${this._esc(id)}">${this._esc(id.slice(0, 24))}${id.length > 24 ? '…' : ''}</a>`;
            return `<tr>
                <td>${idCell}</td>
                <td>${fmt(r.request_count ?? 0)}</td>
                <td>${fmt(r.input_tokens ?? 0)}</td>
                <td>${fmt(r.output_tokens ?? 0)}</td>
                <td>${costStr}</td>
            </tr>`;
        }).join('');
        return `<table class="data-table">
            <thead><tr>
                <th>${idLabel}</th><th>Requests</th><th>Input</th><th>Output</th><th>Cost (est.)</th>
            </tr></thead>
            <tbody>${tableRows}</tbody>
        </table>`;
    }

    _renderErrorRunsTable(errorRate) {
        const fmt = n => Number(n).toLocaleString();
        if (!errorRate.length) return `<div class="empty-state-hint">No error data in this window.</div>`;
        const rows = [...errorRate]
            .sort((a, b) => (b.error_rate ?? 0) - (a.error_rate ?? 0))
            .map(r => {
                const pct = ((r.error_rate ?? 0) * 100).toFixed(1);
                const cls = (r.error_rate ?? 0) > 0.1 ? 'top-spans-cost-high' : '';
                return `<tr>
                    <td>${this._esc(r.model || '—')}</td>
                    <td>${fmt(r.total_calls ?? 0)}</td>
                    <td>${fmt(r.error_count ?? 0)}</td>
                    <td class="${cls}">${pct}%</td>
                </tr>`;
            }).join('');
        return `<table class="data-table">
            <thead><tr>
                <th>Model</th><th>Calls</th><th>Errors</th><th>Error rate</th>
            </tr></thead>
            <tbody>${rows}</tbody>
        </table>`;
    }

    _buildFinishReasons(reasons) {
        if (!reasons.length) {
            return `<h3>Stop reasons</h3><div class="empty-state-hint">No finish-reason data in this window.</div>`;
        }
        const total = reasons.reduce((acc, r) => acc + (r.count || 0), 0);
        const sorted = [...reasons].sort((a, b) => (b.count || 0) - (a.count || 0));

        const LABELS = {
            end_turn:   'end_turn — completed normally',
            max_tokens: 'max_tokens — truncated (hit token limit)',
            length:     'length — truncated (hit token limit)',
            stop_sequence: 'stop_sequence — stopped by stop token',
            tool_use:   'tool_use — paused for tool call',
        };

        const truncatedCount = reasons
            .filter(r => ['max_tokens','length'].includes(String(r.reason).toLowerCase()))
            .reduce((acc, r) => acc + (r.count || 0), 0);
        const truncatedPct = total > 0 ? (truncatedCount / total * 100) : 0;
        const truncatedBanner = truncatedCount > 0
            ? `<div class="finish-reason-warning-banner">⚠ ${Number(truncatedCount).toLocaleString()} truncated responses (${truncatedPct.toFixed(1)}%) — context window hit limit</div>`
            : '';

        const rows = sorted.map(r => {
            const count = r.count || 0;
            const pct = total > 0 ? (count / total) * 100 : 0;
            const reason = String(r.reason || 'unknown');
            const warning = ['max_tokens','length'].includes(reason.toLowerCase());
            const label = LABELS[reason.toLowerCase()] || reason;
            return `
                <div class="finish-reason-row">
                    <div class="finish-reason-name${warning ? ' warning-text' : ''}">${this._esc(label)}</div>
                    <div class="finish-reason-bar"><div class="finish-reason-fill ${warning ? 'warning' : ''}" style="width:${pct.toFixed(2)}%"></div></div>
                    <div class="finish-reason-count">${Number(count).toLocaleString()} (${pct.toFixed(1)}%)</div>
                </div>`;
        }).join('');

        return `
            <h3>Stop reasons</h3>
            ${truncatedBanner}
            <div class="finish-reasons-list">${rows}</div>`;
    }

    _buildTruncationRate(rows) {
        const meaningful = rows.filter(r => (r.truncated || 0) > 0);
        if (!meaningful.length) return '';
        const fmt = n => Number(n).toLocaleString();
        const tableRows = rows.map(r => {
            const rate = (r.rate || 0) * 100;
            let colorClass = 'trunc-rate-green';
            if (rate >= 5) colorClass = 'trunc-rate-red';
            else if (rate >= 1) colorClass = 'trunc-rate-yellow';
            return `
                <tr>
                    <td>${this._esc(r.model || '—')}</td>
                    <td>${fmt(r.total || 0)}</td>
                    <td>${fmt(r.truncated || 0)}</td>
                    <td class="${colorClass}">${rate.toFixed(1)}%</td>
                </tr>`;
        }).join('');
        return `
            <h3>Truncation rate by model</h3>
            <table class="data-table">
                <thead><tr>
                    <th>Model</th><th>Total calls</th><th>Truncated</th><th>Rate</th>
                </tr></thead>
                <tbody>${tableRows}</tbody>
            </table>`;
    }

    _buildCacheHitRate(rows) {
        const meaningful = rows.filter(r => (r.total_cache_read_tokens || 0) > 0);
        if (!meaningful.length) return '';
        const fmt = n => Number(n).toLocaleString();
        const tableRows = rows.map(r => {
            const rate = (r.hit_rate || 0) * 100;
            let colorClass = 'cache-rate-grey';
            if (rate >= 20) colorClass = 'cache-rate-green';
            else if (rate >= 5) colorClass = 'cache-rate-yellow';
            return `
                <tr>
                    <td>${this._esc(r.model || '—')}</td>
                    <td>${fmt(r.total_input_tokens || 0)}</td>
                    <td>${fmt(r.total_cache_read_tokens || 0)}</td>
                    <td>${fmt(r.total_cache_creation_tokens || 0)}</td>
                    <td class="${colorClass}">${rate.toFixed(1)}%</td>
                </tr>`;
        }).join('');
        return `
            <h3>Cache hit rate by model</h3>
            <table class="data-table">
                <thead><tr>
                    <th>Model</th><th>Input tokens</th><th>Cache read</th><th>Cache created</th><th>Hit rate</th>
                </tr></thead>
                <tbody>${tableRows}</tbody>
            </table>`;
    }

    /**
     * Cache economics: per-model read/write split with estimated savings
     * (from the by_model=1 response) plus a read:write stacked bar over the
     * time series. Falls back to the legacy span-based hit-rate table when
     * the economics payload is unavailable.
     */
    _buildCacheEconomics(econ, legacyRows, bucketSecs) {
        const models = econ && Array.isArray(econ.models) ? econ.models : [];
        if (!models.length) return this._buildCacheHitRate(legacyRows || []);
        const fmt = n => Number(n).toLocaleString();
        const fmtUsd = v => v == null ? '—' : `$${Number(v).toFixed(2)}`;
        const fmtRatio = r => r == null ? '—' : `${Number(r).toFixed(1)}:1`;

        const totalRead = models.reduce((s, m) => s + (m.cache_read_tokens || 0), 0);
        const totalWrite = models.reduce((s, m) => s + (m.cache_write_tokens || 0), 0);
        const allKnown = models.every(m => m.savings_known);
        const totalSavings = models
            .filter(m => m.savings_known)
            .reduce((s, m) => s + (m.est_savings_usd || 0), 0);

        const modelRows = models.map(m => `
            <tr>
                <td>${this._esc(m.model)}</td>
                <td>${fmt(m.cache_read_tokens || 0)}</td>
                <td>${fmt(m.cache_write_tokens || 0)}</td>
                <td>${fmtRatio(m.read_write_ratio)}</td>
                <td>${m.hit_rate == null ? '—' : (m.hit_rate * 100).toFixed(1)}%</td>
                <td>${fmtUsd(m.est_savings_usd)}${m.savings_known ? '' : ' <span class="pm-savings-unknown" title="No known cache-read price for this model">?</span>'}</td>
            </tr>`).join('');

        const series = econ && Array.isArray(econ.series) ? econ.series : [];
        let chart = '';
        if (series.length > 1 && series.length <= 48) {
            const maxTotal = Math.max(...series.map(p => (p.cache_read || 0) + (p.cache_write || 0)), 1);
            const segs = series.map(p => {
                const read = p.cache_read || 0, write = p.cache_write || 0;
                const height = Math.max(2, Math.round(((read + write) / maxTotal) * 100));
                const readH = read + write > 0 ? Math.round((read / (read + write)) * height) : 0;
                const ts = new Date(p.timestamp / 1e6).toISOString().slice(5, 16).replace('T', ' ');
                return `
                    <div class="ce-col" title="${ts} — read ${fmt(read)}, write ${fmt(write)}">
                        <div class="ce-stack" style="height:${height}px">
                            <div class="ce-read" style="height:${readH}px"></div>
                            <div class="ce-write" style="height:${height - readH}px"></div>
                        </div>
                    </div>`;
            }).join('');
            chart = `
                <div class="ce-chart" title="Cache reads (blue) vs writes (amber) per ${bucketSecs}s bucket">
                    ${segs}
                </div>
                <div class="ce-legend">
                    <span><span class="ce-swatch ce-read"></span>cache read</span>
                    <span><span class="ce-swatch ce-write"></span>cache write</span>
                </div>`;
        }

        return `
            <h3>Cache economics by model</h3>
            <p class="section-hint">${fmt(totalRead)} tokens served from cache vs ${fmt(totalWrite)} written — estimated savings ${fmtUsd(totalSavings)}${allKnown ? '' : ' (partial: some models have no known cache-read price)'}</p>
            ${chart}
            <table class="data-table">
                <thead><tr>
                    <th>Model</th><th>Cache read</th><th>Cache write</th>
                    <th>Read:write</th><th>Hit rate</th><th>Est. savings</th>
                </tr></thead>
                <tbody>${modelRows}</tbody>
            </table>`;
    }

    _buildReasoningShare(data) {
        if (!data) return '';
        const models = Array.isArray(data.models) ? data.models : [];
        const effort = Array.isArray(data.effort) ? data.effort : [];
        if (!models.length) return '';
        const fmt = n => Number(n).toLocaleString();
        const fmtUsd = v => v == null ? '—' : `$${Number(v).toFixed(2)}`;
        const totalReasoning = models.reduce((s, m) => s + (m.reasoning_tokens || 0), 0);
        const totalOutput = models.reduce((s, m) => s + (m.output_tokens || 0), 0);
        const totalCost = models.reduce((s, m) => s + (m.cost_usd || 0), 0);

        const modelRows = models.map(m => {
            const share = m.share_pct == null ? 0 : m.share_pct;
            return `
                <tr>
                    <td>${this._esc(m.model)}</td>
                    <td>
                        <div class="rs-bar" title="${fmt(m.reasoning_tokens)} / ${fmt(m.output_tokens)} tokens">
                            <div class="rs-bar-fill" style="width:${Math.min(100, share).toFixed(2)}%"></div>
                        </div>
                        ${m.share_pct == null ? '<span class="rs-share">—</span>' : `<span class="rs-share">${m.share_pct.toFixed(1)}%</span>`}
                    </td>
                    <td>${fmt(m.reasoning_tokens || 0)}</td>
                    <td>${fmt(m.output_tokens || 0)}</td>
                    <td>${fmtUsd(m.cost_usd)}</td>
                </tr>`;
        }).join('');

        let effortHtml = '';
        if (effort.length) {
            const rows = effort.map(e => `
                <tr>
                    <td>${this._esc(e.effort)}</td>
                    <td>${fmt(e.calls || 0)}</td>
                    <td>${fmt(e.reasoning_tokens || 0)}</td>
                </tr>`).join('');
            effortHtml = `
                <h4>By reasoning effort (codex)</h4>
                <table class="data-table rs-effort">
                    <thead><tr><th>Effort</th><th>Calls</th><th>Reasoning tokens</th></tr></thead>
                    <tbody>${rows}</tbody>
                </table>`;
        }

        return `
            <h3>Reasoning share by model</h3>
            <p class="section-hint">${fmt(totalReasoning)} thinking tokens out of ${fmt(totalOutput)} output — estimated thinking cost ${fmtUsd(totalCost)} (reasoning tokens billed at the output rate)</p>
            <table class="data-table">
                <thead><tr>
                    <th>Model</th><th>Share of output</th><th>Reasoning</th><th>Output</th><th>Thinking cost</th>
                </tr></thead>
                <tbody>${modelRows}</tbody>
            </table>
            ${effortHtml}`;
    }

    _buildAgents(data, bucketSecs) {
        if (!data) return '';
        const agents = Array.isArray(data.agents) ? data.agents : [];
        if (!agents.length) return '';
        const fmt = n => Number(n || 0).toLocaleString();
        const fmtUsd = v => v == null ? '—' : `$${Number(v).toFixed(2)}`;

        const agentColors = { opencode: 'var(--accent-color, #4c9aff)', codex: '#f5a623', claude: '#c084fc' };
        const colorFor = a => agentColors[a] || '#888';

        const rows = agents.map(a => {
            const t = a.tokens || {};
            const total = (t.input || 0) + (t.output || 0) + (t.cache_read || 0)
                + (t.cache_write || 0) + (t.reasoning || 0);
            const costNote = a.cost_source === 'estimated' ? ' (est.)' : '';
            return `
                <tr>
                    <td><span class="agent-dot" style="background:${colorFor(a.agent)}"></span>${this._esc(a.agent)}</td>
                    <td class="num">${fmt(a.sessions)}</td>
                    <td class="num" title="${a.cost_source === 'actual' ? 'harness cost counter' : 'tokens × pricing table'}">${fmtUsd(a.cost_usd)}${costNote}</td>
                    <td class="num" title="in ${fmt(t.input || 0)} · out ${fmt(t.output || 0)} · cache-r ${fmt(t.cache_read || 0)} · cache-w ${fmt(t.cache_write || 0)} · reasoning ${fmt(t.reasoning || 0)}">${fmt(total)}</td>
                    <td class="num">${fmt(a.tool_calls)}</td>
                    <td class="num">${a.retries == null ? '—' : fmt(a.retries)}</td>
                </tr>`;
        }).join('');

        // Stacked cost chart: one bucket column, one segment per agent.
        const bucketMap = new Map();
        for (const a of agents) {
            for (const p of a.series || []) {
                if (p.cost_usd == null) continue;
                const b = bucketMap.get(p.ts) || { ts: p.ts, costs: {} };
                b.costs[a.agent] = (b.costs[a.agent] || 0) + p.cost_usd;
                bucketMap.set(p.ts, b);
            }
        }
        let chartHtml = '';
        const buckets = Array.from(bucketMap.values()).sort((x, y) => x.ts - y.ts);
        if (buckets.length) {
            const total = buckets.reduce((s, b) => s + Object.values(b.costs).reduce((x, y) => x + y, 0), 0);
            const maxCost = buckets.reduce((m, b) => Math.max(m, Object.values(b.costs).reduce((x, y) => x + y, 0)), 0);
            const width = 100, chartHeight = 100;
            const barGap = 0.5;
            const barWidth = Math.max((width - barGap * (buckets.length - 1)) / buckets.length, 0.1);
            const bars = buckets.map((b, i) => {
                let y = chartHeight;
                const segs = Object.entries(b.costs)
                    .filter(([, v]) => v > 0)
                    .map(([agent, v]) => {
                        const h = maxCost > 0 ? (v / maxCost) * chartHeight : 0;
                        y -= h;
                        const tsDate = new Date(b.ts / 1_000_000_000);
                        const title = `${formatTs(tsDate)}\n${agent}: $${v.toFixed(2)}`;
                        return `<rect class="agent-chart-bar" x="${(i * (barWidth + barGap)).toFixed(3)}" y="${y.toFixed(3)}" width="${barWidth.toFixed(3)}" height="${h.toFixed(3)}" fill="${colorFor(agent)}"><title>${this._esc(title)}</title></rect>`;
                    });
                return segs.join('');
            }).join('');
            const multiDay = buckets.length > 1 &&
                new Date(buckets[0].ts / 1_000_000_000).toDateString() !==
                new Date(buckets[buckets.length - 1].ts / 1_000_000_000).toDateString();
            const labelFor = i => chartAxisLabel(buckets[i].ts, multiDay);
            const legend = agents
                .map(a => `<span class="agent-legend-item"><span class="agent-dot" style="background:${colorFor(a.agent)}"></span>${this._esc(a.agent)}</span>`)
                .join('');
            chartHtml = `
                <h4>Cost over time by agent — total ${fmtUsd(total)}</h4>
                <div class="cost-chart">
                    <svg class="cost-chart-svg" viewBox="0 0 ${width} ${chartHeight}" preserveAspectRatio="none">
                        ${bars}
                    </svg>
                    <div class="cost-chart-axis-labels">
                        <span class="cost-chart-axis-left">${this._esc(labelFor(0))}</span>
                        <span class="cost-chart-axis-mid">${buckets.length > 2 ? this._esc(labelFor(Math.floor(buckets.length / 2))) : ''}</span>
                        <span class="cost-chart-axis-right">${buckets.length > 1 ? this._esc(labelFor(buckets.length - 1)) : ''}</span>
                    </div>
                    <div class="agent-legend">${legend}</div>
                </div>`;
        }

        return `
            <h3>Agents</h3>
            <p class="section-hint">Per-harness sessions, spend, tokens and tool activity. opencode cost is its own counter; codex/claude cost is estimated from tokens × pricing (their cost counters under-report).</p>
            <table class="data-table">
                <thead><tr>
                    <th>Agent</th><th>Sessions</th><th>Cost</th><th>Tokens</th><th>Tool calls</th><th>Retries</th>
                </tr></thead>
                <tbody>${rows}</tbody>
            </table>
            ${chartHtml}`;
    }

    /**
     * Bucketed latency percentiles (p50/p90/p95/p99) from
     * /api/genai/latency_percentiles. Two charts (duration, ttft), each with
     * a model dropdown that re-renders client-side from the fetched data.
     * p50 is a solid line, p95/p99 dashed.
     */
    _buildLatencyPercentilesChart(resp) {
        if (!resp || !resp.metrics) return '';
        const metricTitles = { duration: 'Request duration percentiles', ttft: 'Time to first token percentiles' };
        const charts = Object.keys(resp.metrics)
            .sort((a, b) => (a === 'duration' ? -1 : b === 'duration' ? 1 : 0))
            .map(metric => {
                const series = resp.metrics[metric];
                if (!series || (!series.all.length && !Object.keys(series.models || {}).length)) return '';
                const models = Object.keys(series.models || {}).sort();
                const options = models.map(m =>
                    `<option value="${this._esc(m)}">${this._esc(m)}</option>`
                ).join('');
                return `
                    <div class="latency-percentile-chart" data-metric="${metric}" data-analytics-percentiles="${this._esc(JSON.stringify(series))}">
                        <h4>${metricTitles[metric] || metric} — model: all</h4>
                        <p class="table-hint">Solid line = p50; dashed = p95, p99. Pick a model to filter the series.</p>
                        <select class="latency-percentile-model" aria-label="Model filter">
                            <option value="all">all</option>
                            ${options}
                        </select>
                        <div class="percentile-chart-body"></div>
                    </div>`;
            }).filter(Boolean).join('');
        if (!charts) return '';
        return `
            <h3>Latency percentiles</h3>
            ${charts}`;
    }

    /**
     * Bind the latency section's interactive charts. Script tags inside
     * innerHTML never execute, so all chart wiring happens here, after
     * the DOM update (see _setSectionBody('latency', ...)).
     */
    _bindLatencyCharts() {
        const body = document.getElementById('analytics-section-body-latency');
        if (!body) return;
        body.querySelectorAll('.latency-percentile-chart').forEach(el => {
            const render = model => {
                const series = JSON.parse(el.dataset.analyticsPercentiles);
                const points = model === 'all' ? (series.all || []) : ((series.models || {})[model] || []);
                el.querySelector('.percentile-chart-body').innerHTML = this._renderPercentileLines(points);
                this._enableBrushing(el);
                const title = el.querySelector('h4');
                const prefix = title.textContent.split(' — model:')[0];
                title.textContent = prefix + ' — model: ' + model;
            };
            el.querySelector('.latency-percentile-model').addEventListener('change', e => render(e.target.value));
            render('all');
        });
        body.querySelectorAll('.distribution-chart').forEach(el => this._bindDistributionScale(el));
    }

    /** Bind (or re-bind after a scale-toggle re-render) the scale-toggle listener on a distribution chart. */
    _bindDistributionScale(el) {
        const sel = el && el.querySelector('.distribution-scale');
        if (!sel) return;
        sel.addEventListener('change', async () => {
            try {
                const params = Object.assign({}, JSON.parse(el.dataset.distributionParams || '{}'));
                const resp = await this.api.getDistribution({
                    metric: el.dataset.distributionMetric,
                    scale: sel.value,
                    ...params,
                });
                const html = this._buildDistributionChart(el.dataset.distributionTitle || '', resp);
                if (html) {
                    el.outerHTML = html;
                    this._bindDistributionScale(el.parentElement.querySelector('.distribution-chart'));
                }
            } catch (e) { /* keep current chart on fetch error */ }
        });
    }

    /**
     * Generic distribution chart from /api/genai/distributions (issue #133):
     * vertical bars (log-spaced buckets render as equal-width decades), a
     * stats line, and a scale toggle that re-fetches.
     * @param {string} title - Section heading text
     * @param {?object} resp - DistributionResponse (may be null/empty)
     */
    _buildDistributionChart(title, resp) {
        if (!resp || !resp.buckets || !resp.buckets.length) return '';
        const metric = resp.metric;
        const titleEl = this._esc(title);
        const stats = resp.stats || {};
        const statsLine = stats.count
            ? `n=${stats.count} · min ${this._fmtDistValue(resp.unit, stats.min)} · p50 ${this._fmtDistValue(resp.unit, stats.p50)} · p95 ${this._fmtDistValue(resp.unit, stats.p95)} · p99 ${this._fmtDistValue(resp.unit, stats.p99)} · max ${this._fmtDistValue(resp.unit, stats.max)}`
            : 'no values in window';
        const width = 100, height = 60;
        const maxCount = Math.max(...resp.buckets.map(b => b.count), 1);
        const n = resp.buckets.length;
        const barW = width / n;
        const bars = resp.buckets.map((b, i) => {
            const h = b.count === 0 ? 0 : Math.max(2, (b.count / maxCount) * (height - 8));
            const x = i * barW + barW * 0.08;
            const w = barW * 0.84;
            const y = height - h;
            const tip = `${this._esc(this._fmtDistValue(resp.unit, b.min))}–${this._esc(this._fmtDistValue(resp.unit, b.max))}: ${b.count}`;
            return `<rect class="hist-bar" x="${x.toFixed(3)}" y="${y.toFixed(3)}" width="${w.toFixed(3)}" height="${h.toFixed(3)}" rx="0.4"><title>${tip}</title></rect>`;
        }).join('');
        const first = resp.buckets[0], last = resp.buckets[n - 1];
        return `
            <div class="distribution-chart" data-distribution-metric="${this._esc(metric)}" data-distribution-title="${titleEl}" data-distribution-params="${this._esc(JSON.stringify(this._baseParams()))}">
                <h4>${titleEl}
                    <select class="distribution-scale" aria-label="Bin scale" style="margin-left:0.5rem;font-size:0.7em">
                        <option value="linear"${resp.scale === 'linear' ? ' selected' : ''}>linear</option>
                        <option value="log"${resp.scale === 'log' ? ' selected' : ''}>log</option>
                    </select>
                </h4>
                <div class="cost-chart">
                    <svg class="cost-chart-svg" viewBox="0 0 ${width} ${height}" preserveAspectRatio="none">${bars}</svg>
                    <div class="cost-chart-axis-labels">
                        <span class="cost-chart-axis-left">${this._esc(this._fmtDistValue(resp.unit, first.min))}</span>
                        <span class="cost-chart-axis-mid"></span>
                        <span class="cost-chart-axis-right">${this._esc(this._fmtDistValue(resp.unit, last.max))}</span>
                    </div>
                </div>
                <p class="table-hint distribution-stats">${this._esc(statsLine)}</p>
            </div>`;
    }

    _fmtDistValue(unit, v) {
        if (v === null || v === undefined || Number.isNaN(v)) return '—';
        if (unit === 'usd') return v >= 0.01 ? `$${v.toFixed(2)}` : `$${v.toFixed(4)}`;
        if (unit === 'ms') return v >= 1000 ? `${(v / 1000).toFixed(2)}s` : `${Math.round(v)}ms`;
        if (unit === 'tokens') {
            if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
            if (v >= 1_000) return `${(v / 1_000).toFixed(1)}k`;
            return `${Math.round(v)}`;
        }
        return String(Math.round(v * 100) / 100);
    }

    /**
     * SVG line chart for one percentile series (p50 solid, p90/p95/p99 dashed).
     * @param {Array} points - LatencyPercentilePoint[] ascending by ts
     */
    _renderPercentileLines(points) {
        if (!points.length) return '<div class="empty-state-hint">No data for this model in this window.</div>';
        const width = 100, chartHeight = 100, barGap = 0.5;
        const n = points.length;
        const x = i => n === 1 ? width / 2 : i * ((width - barGap) / (n - 1));
        const max = Math.max(...points.flatMap(p => [p.p50_ms, p.p90_ms, p.p95_ms, p.p99_ms]), 1);
        const y = v => chartHeight - (v / max) * chartHeight;
        const line = key => points
            .map((p, i) => `${x(i).toFixed(3)},${y(p[key]).toFixed(3)}`)
            .join(' ');
        const tip = p => {
            const d = new Date(p.ts / 1_000_000_000);
            return `${formatTs(d)}\np50 ${Math.round(p.p50_ms)}ms\np90 ${Math.round(p.p90_ms)}ms\np95 ${Math.round(p.p95_ms)}ms\np99 ${Math.round(p.p99_ms)}ms\n${p.count} requests`;
        };
        const dots = points.map((p, i) => {
            const t = this._esc(tip(p));
            return `<circle cx="${x(i).toFixed(3)}" cy="${y(p.p99_ms).toFixed(3)}" r="0.8" fill="var(--text-color, #ccc)" opacity="0"><title>${t}</title></circle>
                    <circle cx="${x(i).toFixed(3)}" cy="${y(p.p99_ms).toFixed(3)}" r="1.2" fill="transparent" style="pointer-events:all"><title>${t}</title></circle>`;
        }).join('');
        const multiDay = n > 1 &&
            new Date(points[0].ts / 1_000_000_000).toDateString() !==
            new Date(points[n - 1].ts / 1_000_000_000).toDateString();
        const labelFor = i => chartAxisLabel(points[i].ts, multiDay);
        const brushAttrs = this._brushAttrs(points.map(p => p.ts), null);
        return `
            <div class="cost-chart">
                <svg class="cost-chart-svg" viewBox="0 0 ${width} ${chartHeight}" preserveAspectRatio="none" ${brushAttrs}>
                    <polyline class="percentile-line-p50" points="${line('p50_ms')}" fill="none"/>
                    <polyline class="percentile-line-p90" points="${line('p90_ms')}" fill="none"/>
                    <polyline class="percentile-line-p95" points="${line('p95_ms')}" fill="none"/>
                    <polyline class="percentile-line-p99" points="${line('p99_ms')}" fill="none"/>
                    ${dots}
                </svg>
                <div class="cost-chart-axis-labels">
                    <span class="cost-chart-axis-left">${this._esc(labelFor(0))}</span>
                    <span class="cost-chart-axis-mid">${n > 2 ? this._esc(labelFor(Math.floor(n / 2))) : ''}</span>
                    <span class="cost-chart-axis-right">${n > 1 ? this._esc(labelFor(n - 1)) : ''}</span>
                </div>
            </div>
            <div class="agent-legend">
                <span class="agent-legend-item"><span style="display:inline-block;width:14px;border-top:2px solid var(--accent-color, #4c9aff)"></span> p50</span>
                <span class="agent-legend-item"><span style="display:inline-block;width:14px;border-top:2px dashed #f5a623"></span> p90</span>
                <span class="agent-legend-item"><span style="display:inline-block;width:14px;border-top:2px dashed #e05d44"></span> p95</span>
                <span class="agent-legend-item"><span style="display:inline-block;width:14px;border-top:2px dashed #a06cd5"></span> p99</span>
            </div>`;
    }

    _buildProjects(data) {
        if (!data) return '';
        const projects = Array.isArray(data.projects) ? data.projects : [];
        if (!projects.length) return '';
        const fmt = n => Number(n || 0).toLocaleString();
        const fmtUsd = v => v == null ? '—' : `$${Number(v).toFixed(2)}`;
        const sourceNote = s =>
            s === 'actual' ? 'harness cost counter'
            : s === 'mixed' ? 'counter + tokens × pricing (disjoint harnesses)'
            : 'tokens × pricing table';
        const rows = projects.map(p => {
            const t = p.tokens || {};
            const total = (t.input || 0) + (t.output || 0) + (t.cache_read || 0)
                + (t.cache_write || 0) + (t.reasoning || 0);
            const top = (p.top_models || []);
            const topCell = top.length
                ? top.map(m => {
                    const mt = m.tokens || {};
                    const mtot = (mt.input || 0) + (mt.output || 0) + (mt.cache_read || 0)
                        + (mt.cache_write || 0) + (mt.reasoning || 0);
                    return `<span title="${top.length > 1 ? 'top 5 models' : 'only model'}">${this._esc(m.model)} (${fmt(mtot)})${m.cost_usd != null ? `, ${fmtUsd(m.cost_usd)}` : ''}</span>`;
                }).join('<br>')
                : '—';
            return `
                <tr>
                    <td title="${p.project_id === 'unattributed' ? 'codex/claude emit no project label today' : ''}">${this._esc(p.project_id)}</td>
                    <td class="num">${fmt(p.sessions)}</td>
                    <td class="num" title="${sourceNote(p.cost_source)}">${fmtUsd(p.cost_usd)}${p.cost_source && p.cost_source !== 'actual' ? ` <span class="section-hint">(${p.cost_source})</span>` : ''}</td>
                    <td class="num" title="in ${fmt(t.input || 0)} · out ${fmt(t.output || 0)} · cache-r ${fmt(t.cache_read || 0)} · cache-w ${fmt(t.cache_write || 0)} · reasoning ${fmt(t.reasoning || 0)}">${fmt(total)}</td>
                    <td>${topCell}</td>
                </tr>`;
        }).join('');

        return `
            <h3>Projects</h3>
            <p class="section-hint">Which project drove the bill. opencode attributes by its project.id label; codex/claude emit no project label today and are grouped under "unattributed" (the limitation, not a gap in the query).</p>
            <table class="data-table">
                <thead><tr>
                    <th>Project</th><th>Sessions</th><th>Cost</th><th>Tokens</th><th>Top models</th>
                </tr></thead>
                <tbody>${rows}</tbody>
            </table>`;
    }

    _buildRequestParamProfile(profile) {
        if (!profile) return '';
        const tempBuckets = Array.isArray(profile.temperature_buckets) ? profile.temperature_buckets : [];
        const maxTokBuckets = Array.isArray(profile.max_tokens_buckets) ? profile.max_tokens_buckets : [];
        const distinctTemps = new Set(tempBuckets.map(b => b.temperature)).size;
        const distinctMaxToks = new Set(maxTokBuckets.map(b => b.max_tokens)).size;
        if (distinctTemps <= 1 && distinctMaxToks <= 1) return '';

        const fmt = n => Number(n).toLocaleString();

        const tempRows = tempBuckets.map(b => `
            <tr>
                <td>${b.temperature == null ? '<em>not set</em>' : this._esc(String(b.temperature))}</td>
                <td>${fmt(b.count || 0)}</td>
            </tr>`).join('');

        const maxTokRows = maxTokBuckets.map(b => `
            <tr>
                <td>${b.max_tokens == null ? '<em>not set</em>' : this._esc(String(b.max_tokens))}</td>
                <td>${fmt(b.count || 0)}</td>
            </tr>`).join('');

        const tempTable = distinctTemps > 1 ? `
            <div class="param-profile-table">
                <h4>Temperature distribution</h4>
                <table class="data-table">
                    <thead><tr><th>Temperature</th><th>Count</th></tr></thead>
                    <tbody>${tempRows}</tbody>
                </table>
            </div>` : '';

        const maxTokTable = distinctMaxToks > 1 ? `
            <div class="param-profile-table">
                <h4>Max tokens distribution</h4>
                <table class="data-table">
                    <thead><tr><th>Max tokens</th><th>Count</th></tr></thead>
                    <tbody>${maxTokRows}</tbody>
                </table>
            </div>` : '';

        return `
            <h3>Request parameters</h3>
            <div class="param-profile-container">${tempTable}${maxTokTable}</div>`;
    }

    _buildConversationDepthCard(depth) {
        if (!depth || !depth.total_conversations) return '';
        const fmt = n => Number(n).toLocaleString();
        const avg = depth.avg_turns != null ? Number(depth.avg_turns).toFixed(1) : '—';
        return `
                <div class="usage-card">
                    <div class="usage-card-label">Conversations</div>
                    <div class="usage-card-value">${fmt(depth.total_conversations)}</div>
                    <div class="gauge-hint">avg ${avg} turns · p50 ${depth.p50_turns ?? '—'} · p95 ${depth.p95_turns ?? '—'}</div>
                </div>`;
    }

    _buildCallsChart(callsSeries) {
        if (!Array.isArray(callsSeries) || !callsSeries.length) {
            return `<h3>Request volume over time</h3><div class="empty-state-hint">No request data in this window.</div>`;
        }

        const bucketMap = new Map();
        for (const row of callsSeries) {
            const ts = row.timestamp;
            bucketMap.set(ts, (bucketMap.get(ts) || 0) + (row.requests || 0));
        }
        const buckets = Array.from(bucketMap.entries())
            .sort((a, b) => a[0] - b[0])
            .map(([timestamp, requests]) => ({ timestamp, requests }));

        const totalRequests = buckets.reduce((a, b) => a + b.requests, 0);
        const maxRequests = buckets.reduce((a, b) => Math.max(a, b.requests), 0);

        const width = 100;
        const barGap = 0.5;
        const barWidth = buckets.length > 0 ? Math.max((width - barGap * (buckets.length - 1)) / buckets.length, 0.1) : 0;
        const chartHeight = 100;

        const bars = buckets.map((b, i) => {
            const h = maxRequests > 0 ? (b.requests / maxRequests) * chartHeight : 0;
            const x = i * (barWidth + barGap);
            const y = chartHeight - h;
            const tsDate = new Date(b.timestamp / 1_000_000);
            const title = `${formatTs(tsDate)}\n${b.requests.toLocaleString()} requests`;
            return `<rect class="cost-chart-bar" x="${x.toFixed(3)}" y="${y.toFixed(3)}" width="${barWidth.toFixed(3)}" height="${h.toFixed(3)}"><title>${this._esc(title)}</title></rect>`;
        }).join('');

        const multiDay = buckets.length > 1 &&
            new Date(buckets[0].timestamp / 1_000_000).toDateString() !==
            new Date(buckets[buckets.length - 1].timestamp / 1_000_000).toDateString();
        const labelFor = i => chartAxisLabel(buckets[i].timestamp, multiDay);

        let axisHtml = '';
        if (buckets.length > 0) {
            const left = this._esc(labelFor(0));
            const mid = buckets.length > 2 ? this._esc(labelFor(Math.floor(buckets.length / 2))) : '';
            const right = buckets.length > 1 ? this._esc(labelFor(buckets.length - 1)) : '';
            axisHtml = `
                <div class="cost-chart-axis-labels">
                    <span class="cost-chart-axis-left">${left}</span>
                    <span class="cost-chart-axis-mid">${mid}</span>
                    <span class="cost-chart-axis-right">${right}</span>
                </div>`;
        }
        const brushAttrs = this._brushAttrs(
            buckets.map(b => b.timestamp),
            null,
        );

        return `
            <h3>Request volume over time — ${totalRequests.toLocaleString()} total across ${buckets.length} bucket${buckets.length === 1 ? '' : 's'}</h3>
            <div class="cost-chart">
                <svg class="cost-chart-svg" viewBox="0 0 ${width} ${chartHeight}" preserveAspectRatio="none" ${brushAttrs}>
                    ${bars}
                </svg>
                ${axisHtml}
            </div>`;
    }

    _buildToolApprovals(stats) {
        if (!stats || !stats.total) return '';
        const fmt = n => Number(n).toLocaleString();
        const autoRate = stats.total > 0 ? (stats.auto_accepted / stats.total * 100) : 0;
        const rejectRate = stats.total > 0 ? (stats.rejected / stats.total * 100) : 0;
        const gauge = `
            <div class="usage-summary-cards">
                <div class="usage-gauge-card">
                    <div class="usage-card-label">Auto-accept rate</div>
                    <div class="usage-card-value">${autoRate.toFixed(1)}%</div>
                    <div class="gauge-bar"><div class="gauge-fill" style="width:${autoRate.toFixed(2)}%"></div></div>
                    <div class="gauge-hint">${fmt(stats.auto_accepted)} auto · ${fmt(stats.user_accepted)} user · ${fmt(stats.rejected)} rejected · ${fmt(stats.unknown)} unknown</div>
                </div>
            </div>`;
        const topRows = (stats.top_rejected || []).map(e => `
            <tr>
                <td>${this._esc(e.tool_name || '—')}</td>
                <td>${fmt(e.count)}</td>
                <td class="${rejectRate > 5 ? 'tool-usage-warn' : ''}">${(e.count / stats.total * 100).toFixed(1)}%</td>
            </tr>`).join('');
        const topTable = topRows ? `
            <h4>Top rejected tools</h4>
            <table class="data-table">
                <thead><tr><th>Tool</th><th>Rejections</th><th>% of all decisions</th></tr></thead>
                <tbody>${topRows}</tbody>
            </table>` : '';
        return `
            <h3>Tool approval decisions</h3>
            ${gauge}
            ${topTable}`;
    }

    _buildToolErrors(rows) {
        if (!rows || !rows.length) return '';
        const fmt = n => Number(n).toLocaleString();
        const sorted = [...rows].sort((a, b) => (b.count || 0) - (a.count || 0));
        const tableRows = sorted.map(r => `
            <tr>
                <td>${this._esc(r.tool_name || '—')}</td>
                <td title="${this._esc(r.error_message || '')}">${this._esc((r.error_message || '').length > 80 ? r.error_message.slice(0, 80) + '…' : (r.error_message || '—'))}</td>
                <td>${fmt(r.count || 0)}</td>
            </tr>`).join('');
        return `
            <h3>Top tool errors</h3>
            <p class="table-hint">Failed tool executions grouped by tool and error message (first 120 chars).</p>
            <table class="data-table">
                <thead><tr><th>Tool</th><th>Error</th><th>Count</th></tr></thead>
                <tbody>${tableRows}</tbody>
            </table>`;
    }

    _buildHourOfDay(buckets) {
        if (!Array.isArray(buckets) || !buckets.length) return '';
        const maxLlm = buckets.reduce((m, b) => Math.max(m, b.llm_calls), 1);
        const maxTool = buckets.reduce((m, b) => Math.max(m, b.tool_calls), 1);
        const maxVal = Math.max(maxLlm, maxTool, 1);
        const rows = buckets.map(b => {
            const llmW = Math.max(1, Math.round((b.llm_calls / maxVal) * 80));
            const toolW = Math.max(0, Math.round((b.tool_calls / maxVal) * 80));
            return `
                <tr>
                    <td class="num">${String(b.hour).padStart(2, '0')}:00</td>
                    <td>
                        <div class="hour-bar-track">
                            <div class="hour-bar-llm" style="width:${llmW}px" title="${b.llm_calls} LLM calls"></div>
                        </div>
                    </td>
                    <td class="num">${b.llm_calls > 0 ? b.llm_calls.toLocaleString() : ''}</td>
                    <td>
                        <div class="hour-bar-track">
                            <div class="hour-bar-tool" style="width:${toolW}px" title="${b.tool_calls} tool calls"></div>
                        </div>
                    </td>
                    <td class="num">${b.tool_calls > 0 ? b.tool_calls.toLocaleString() : ''}</td>
                </tr>`;
        }).join('');
        return `
            <h3>Activity by hour of day (UTC)</h3>
            <p class="table-hint">Blue = LLM calls · Orange = tool executions. All-time distribution.</p>
            <table class="data-table hour-of-day-table">
                <thead><tr>
                    <th>Hour</th><th>LLM calls</th><th></th><th>Tool calls</th><th></th>
                </tr></thead>
                <tbody>${rows}</tbody>
            </table>`;
    }

    _buildStopReasons(rows) {
        // Filter out '(none)' rows where nothing meaningful is set
        const meaningful = (rows || []).filter(r => r.reason && r.reason !== '(none)');
        if (!meaningful.length) return '';
        const total = meaningful.reduce((acc, r) => acc + (r.count || 0), 0);
        const sorted = [...meaningful].sort((a, b) => (b.count || 0) - (a.count || 0));
        const barRows = sorted.map(r => {
            const pct = total > 0 ? (r.count / total * 100) : 0;
            return `
                <div class="finish-reason-row">
                    <div class="finish-reason-name">${this._esc(r.reason || '—')}</div>
                    <div class="finish-reason-bar"><div class="finish-reason-fill" style="width:${pct.toFixed(2)}%"></div></div>
                    <div class="finish-reason-count">${Number(r.count).toLocaleString()} (${pct.toFixed(1)}%)</div>
                </div>`;
        }).join('');
        return `
            <h3>Stop reasons (claude_code)</h3>
            <p class="table-hint">Claude Code <code>stop_reason</code> attribute — tool_use means the model paused to run a tool; end_turn means the model finished naturally.</p>
            <div class="finish-reasons-list">${barRows}</div>`;
    }

    _buildContextTypeSplit(rows) {
        if (!rows || !rows.length) return '';
        const fmt = n => Number(n).toLocaleString();
        const tableRows = rows.map(r => `
            <tr>
                <td>${this._esc(r.context || '—')}</td>
                <td class="num">${fmt(r.calls || 0)}</td>
                <td class="num">${fmt(r.input_tokens || 0)}</td>
                <td class="num">${fmt(r.output_tokens || 0)}</td>
                <td class="num">${r.avg_ms > 0 ? Math.round(r.avg_ms).toLocaleString() + ' ms' : '—'}</td>
            </tr>`).join('');
        return `
            <h3>Usage by request context</h3>
            <p class="table-hint">Grouped by <code>llm_request.context</code> — e.g. <em>interaction</em> (direct user message) vs <em>sub_agent</em> (background task).</p>
            <table class="data-table">
                <thead><tr>
                    <th>Context</th><th>Calls</th><th>Input tokens</th><th>Output tokens</th><th>Avg latency</th>
                </tr></thead>
                <tbody>${tableRows}</tbody>
            </table>`;
    }

    _buildAgentRoles(response) {
        const roles = (response && response.roles) || [];
        if (!roles.length) return '';
        const fmt = n => Number(n || 0).toLocaleString();
        const tokenTotal = t => (t ? (t.input || 0) + (t.output || 0) + (t.cache_read || 0)
            + (t.cache_write || 0) + (t.reasoning || 0) : 0);
        const fmtCost = c => c != null ? `$${Number(c).toFixed(4)}` : '—';

        const tableRows = roles.map(r => {
            const t = r.tokens || {};
            const top = (r.top_models || [])
                .map(m => `${this._esc(m.model)} (${fmt(tokenTotal(m.tokens))})`)
                .join('<br>');
            return `
                <tr>
                    <td>${this._esc(r.role)}</td>
                    <td class="num">${fmt(r.sessions)}</td>
                    <td class="num">${fmt(tokenTotal(t))}</td>
                    <td class="num">${fmt(t.input)} / ${fmt(t.output)}</td>
                    <td class="num">${fmt(t.cache_read)} / ${fmt(t.cache_write)}</td>
                    <td class="num">${fmt(t.reasoning)}</td>
                    <td class="num">${r.share_pct != null ? r.share_pct.toFixed(1) + '%' : '—'}</td>
                    <td class="num">${fmtCost(r.cost)}</td>
                    <td class="small">${top || '—'}</td>
                </tr>`;
        }).join('');

        const unknownNote = response.unknown_share_pct != null
            ? `<p class="table-hint">⚠ ${response.unknown_share_pct.toFixed(1)}% of tokens have no
              <code>agent</code> label (attribution gap).</p>` : '';

        // #insight-6: role × model routing matrix
        const allModels = [...new Set(
            roles.flatMap(r => (r.top_models || []).map(m => m.model))
        )].sort();
        let matrixHtml = '';
        if (allModels.length > 1) {
            const matrixRows = roles.map(r => {
                const modelMap = {};
                for (const m of (r.top_models || [])) {
                    modelMap[m.model] = tokenTotal(m.tokens);
                }
                const cells = allModels.map(model => {
                    const v = modelMap[model];
                    if (!v) return '<td class="dim">—</td>';
                    const totalForRole = tokenTotal(r.tokens || {}) || 1;
                    const pct = Math.round(v / totalForRole * 100);
                    return `<td class="num" title="${fmt(v)} tokens (${pct}%)">${pct}%</td>`;
                }).join('');
                return `<tr><td>${this._esc(r.role)}</td>${cells}</tr>`;
            }).join('');
            const headCells = allModels.map(m =>
                `<th class="small" title="${this._esc(m)}">${this._esc(m.length > 22 ? m.slice(-22) : m)}</th>`
            ).join('');
            matrixHtml = `
                <h3>Role × model routing matrix</h3>
                <p class="table-hint">Cell value = % of that role's tokens routed to the model. Shows which roles use which models.</p>
                <div class="table-scroll-x"><table class="data-table role-matrix-table">
                    <thead><tr><th>Role</th>${headCells}</tr></thead>
                    <tbody>${matrixRows}</tbody>
                </table></div>`;
        }

        return `
            <h3>Sub-agent role attribution</h3>
            <p class="table-hint">Grouped by the opencode <code>agent</code> label — which sub-agent
            (orchestrator, reviewer, executor, …) drove the spend. Cost is estimated from
            tokens × pricing; local/unpriced models show <em>—</em>. Claude Code and Codex do
            not emit a role label yet.</p>
            ${unknownNote}
            <table class="data-table">
                <thead><tr>
                    <th>Role</th><th>Sessions</th><th>Tokens</th><th>In / Out</th>
                    <th>Cache r / w</th><th>Reasoning</th><th>Share</th><th>Cost (est.)</th>
                    <th>Top models</th>
                </tr></thead>
                <tbody>${tableRows}</tbody>
            </table>
            ${matrixHtml}`;
    }

    _buildProviderMix(response) {
        const providers = (response && response.providers) || [];
        if (!providers.length) return '';
        const fmt = n => Number(n || 0).toLocaleString();
        const tokenTotal = t => (t ? (t.input || 0) + (t.output || 0) + (t.cache_read || 0)
            + (t.cache_write || 0) + (t.reasoning || 0) : 0);
        const fmtCost = c => c != null ? `$${Number(c).toFixed(2)}` : '—';
        const totalTokens = Number(response.total_tokens || 0);

        // Stacked token-share bar by provider (colour per provider).
        const palette = ['#4f8cff', '#34c98e', '#f5a623', '#c65ce0', '#e5534b',
            '#5ac8c8', '#8a94a6', '#d0b34e'];
        const barSegments = providers.map((p, i) => {
            const pct = totalTokens > 0
                ? (p.share_pct != null ? p.share_pct : 0)
                : 0;
            return `<div class="pm-bar-seg" title="${this._esc(p.provider)}: ${pct.toFixed(1)}%"
                style="width:${pct}%;background:${palette[i % palette.length]}"></div>`;
        }).join('');
        const legend = providers.map((p, i) => `
            <span class="pm-legend-item">
                <span class="pm-legend-swatch" style="background:${palette[i % palette.length]}"></span>
                ${this._esc(p.provider)}
                <span class="num">${p.share_pct != null ? p.share_pct.toFixed(1) : '0.0'}%</span>
                <span class="num small">${fmtCost(p.cost_usd)}</span>
            </span>`).join('');

        // Nested provider → model table.
        const rows = providers.map(p => {
            const modelRows = (p.models || []).map((m, i) => {
                const t = m.tokens || {};
                return `
                    <tr>
                        <td>${i === 0 ? this._esc(p.provider) : ''}</td>
                        <td>${this._esc(m.model)}</td>
                        <td class="num">${fmt(tokenTotal(t))}</td>
                        <td class="num">${fmt(t.input)} / ${fmt(t.output)}</td>
                        <td class="num">${fmt(t.cache_read)} / ${fmt(t.cache_write)}</td>
                        <td class="num">${fmt(t.reasoning)}</td>
                        <td class="num">${fmt(m.sessions)}</td>
                        <td class="num">${fmtCost(m.cost_usd)}</td>
                    </tr>`;
            }).join('');
            const pTokens = (p.models || []).reduce((s, m) => s + tokenTotal(m.tokens), 0);
            return modelRows + `
                <tr class="pm-provider-total">
                    <td colspan="2"><strong>${this._esc(p.provider)} total</strong></td>
                    <td class="num"><strong>${fmt(pTokens)}</strong></td>
                    <td colspan="4"></td>
                    <td class="num"></td>
                    <td class="num"><strong>${fmtCost(p.cost_usd)}</strong></td>
                </tr>`;
        }).join('');

        const methodNote = response.method === 'token-share-split'
            ? `<p class="table-hint">⚠ At least one model was served by several providers; its
               tokens and cost were split across them by each provider's share of that model's
               usage rows (<code>method: token-share-split</code>).</p>` : '';

        return `
            <h3>Provider × model mix</h3>
            <p class="table-hint">Which provider served which model, and the per-model cost share,
            across opencode, codex and claude_code. Cost is estimated from tokens × pricing
            (opencode's own cost counter arrives zero-valued); local/unpriced models show
            <em>—</em>. Codex emits no provider attribute, so its models are grouped under
            <code>(unknown)</code> rather than guessed.</p>
            ${methodNote}
            <div class="pm-bar">${barSegments}</div>
            <div class="pm-legend">${legend}</div>
            <table class="data-table">
                <thead><tr>
                    <th>Provider</th><th>Model</th><th>Tokens</th><th>In / Out</th>
                    <th>Cache r / w</th><th>Reasoning</th><th>Sessions</th><th>Cost (est.)</th>
                </tr></thead>
                <tbody>${rows}</tbody>
            </table>`;
    }

    _esc(str) {
        return String(str)
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#39;');
    }

    _renderPricingNotice(meta) {
        if (!meta) return '';
        const source = meta.source;
        const sourceLabel = source === 'litellm'
            ? `LiteLLM (${meta.entry_count.toLocaleString()} models)`
            : `hardcoded Claude fallback — last verified ${meta.fallback_last_verified}`;
        const freshness = source === 'litellm' && meta.last_fetched_unix_ms
            ? ` · fetched ${this._relativeTime(meta.last_fetched_unix_ms)}`
            : '';
        const staleWarning = source !== 'litellm' && meta.last_failed_unix_ms
            ? ` · <span class="pricing-disclaimer-warn">last LiteLLM fetch failed ${this._relativeTime(meta.last_failed_unix_ms)}</span>`
            : '';
        return `
            <div class="pricing-disclaimer" role="note">
                <strong>Pricing note:</strong> ${this._esc(meta.disclaimer)}
                <br>
                <span>Source: ${this._esc(sourceLabel)}${freshness}${staleWarning}</span>
                · <a href="${this._esc(meta.source_url)}" target="_blank" rel="noopener">${this._esc(meta.license)}</a>
            </div>`;
    }

    _relativeTime(unixMs) {
        const diffSec = (Date.now() - unixMs) / 1000;
        if (diffSec < 60) return 'just now';
        if (diffSec < 3600) return `${Math.round(diffSec / 60)} min ago`;
        if (diffSec < 86400) return `${Math.round(diffSec / 3600)} h ago`;
        return `${Math.round(diffSec / 86400)} d ago`;
    }

    // ── New insight section loaders (#157–#164) ──────────────────────────

    async _loadEffortSection() {
        this._setSectionLoading('effort');
        try {
            const data = await this.api.getEffortBreakdown(this._baseParams());
            const rows = data.rows || [];
            if (!rows.length) {
                this._setSectionBody('effort', '<div class="empty-state-hint">No effort-level data in this window. Try a wider time range.</div>');
                this.loadedSections.add('effort');
                return;
            }
            // Group by effort level for a summary table
            const byEffort = {};
            for (const r of rows) {
                if (!byEffort[r.effort]) byEffort[r.effort] = 0;
                if (r.token_type === 'input' || r.token_type === 'output') byEffort[r.effort] += r.tokens;
            }
            const effortOrder = ['low', 'medium', 'high', 'xhigh', '(none)'];
            const sortedEfforts = Object.keys(byEffort).sort((a, b) =>
                (effortOrder.indexOf(a) + 1 || 99) - (effortOrder.indexOf(b) + 1 || 99));
            const total = Object.values(byEffort).reduce((s, v) => s + v, 0);
            let html = '<div class="analytics-table-wrap"><table class="analytics-table"><thead><tr><th>Effort</th><th>Tokens (input+output)</th><th>Share</th></tr></thead><tbody>';
            for (const e of sortedEfforts) {
                const pct = total > 0 ? (byEffort[e] / total * 100).toFixed(1) : '0.0';
                html += `<tr><td>${this._esc(e)}</td><td>${Number(byEffort[e]).toLocaleString()}</td><td>${pct}%</td></tr>`;
            }
            html += '</tbody></table></div>';
            // Also add per-model table
            const modelTotals = {};
            for (const r of rows) {
                const key = r.model;
                if (!modelTotals[key]) modelTotals[key] = 0;
                if (r.token_type === 'input' || r.token_type === 'output') modelTotals[key] += r.tokens;
            }
            const topModels = Object.entries(modelTotals).sort((a, b) => b[1] - a[1]).slice(0, 10);
            html += '<h4 style="margin-top:1rem">By model</h4><div class="analytics-table-wrap"><table class="analytics-table"><thead><tr><th>Model</th><th>Tokens</th></tr></thead><tbody>';
            for (const [m, t] of topModels) html += `<tr><td>${this._esc(m)}</td><td>${Number(t).toLocaleString()}</td></tr>`;
            html += '</tbody></table></div>';
            this._setSectionBody('effort', html);
            this.loadedSections.add('effort');
        } catch (err) {
            this._setSectionError('effort', err);
        }
    }

    async _loadEfficiencySection() {
        this._setSectionLoading('efficiency');
        try {
            const data = await this.api.getEfficiencyStats(this._baseParams());
            const fmt = n => Number(n).toLocaleString();
            const fmtF = n => n != null ? n.toFixed(1) : '—';
            let html = `
                <div class="usage-summary-cards">
                    <div class="usage-gauge-card"><div class="usage-card-label">Total tokens</div><div class="usage-card-value">${fmt(data.total_tokens)}</div></div>
                    <div class="usage-gauge-card"><div class="usage-card-label">Commits</div><div class="usage-card-value">${fmt(data.total_commits)}</div></div>
                    <div class="usage-gauge-card"><div class="usage-card-label">Net lines added</div><div class="usage-card-value">${fmt(data.net_lines_added)}</div></div>
                    <div class="usage-gauge-card"><div class="usage-card-label">Tokens / commit</div><div class="usage-card-value">${fmtF(data.tokens_per_commit)}</div></div>
                    <div class="usage-gauge-card"><div class="usage-card-label">Tokens / LOC</div><div class="usage-card-value">${fmtF(data.tokens_per_loc)}</div></div>
                </div>`;
            if (data.by_agent && data.by_agent.length) {
                html += '<div class="analytics-table-wrap"><table class="analytics-table"><thead><tr><th>Agent</th><th>Tokens</th><th>Commits</th><th>Lines added</th><th>Lines removed</th></tr></thead><tbody>';
                for (const a of data.by_agent) {
                    html += `<tr><td>${this._esc(a.agent)}</td><td>${fmt(a.tokens)}</td><td>${fmt(a.commits)}</td><td>${fmt(a.lines_added)}</td><td>${fmt(a.lines_removed)}</td></tr>`;
                }
                html += '</tbody></table></div>';
            }
            this._setSectionBody('efficiency', html);
            this.loadedSections.add('efficiency');
        } catch (err) {
            this._setSectionError('efficiency', err);
        }
    }

    async _loadCodexTtftSection() {
        this._setSectionLoading('codex_ttft');
        try {
            const data = await this.api.getCodexTtft(this._baseParams());
            const models = data.models || [];
            if (!models.length) {
                this._setSectionBody('codex_ttft', '<div class="empty-state-hint">No Codex TTFT data in this window.</div>');
                this.loadedSections.add('codex_ttft');
                return;
            }
            const fmtMs = v => v != null ? `${v.toFixed(0)} ms` : '—';
            let html = '<div class="analytics-table-wrap"><table class="analytics-table"><thead><tr><th>Model</th><th>Samples</th><th>p50</th><th>p90</th><th>p95</th></tr></thead><tbody>';
            for (const m of models) {
                html += `<tr><td>${this._esc(m.model)}</td><td>${Number(m.count).toLocaleString()}</td><td>${fmtMs(m.p50_ms)}</td><td>${fmtMs(m.p90_ms)}</td><td>${fmtMs(m.p95_ms)}</td></tr>`;
            }
            html += '</tbody></table></div>';
            this._setSectionBody('codex_ttft', html);
            this.loadedSections.add('codex_ttft');
        } catch (err) {
            this._setSectionError('codex_ttft', err);
        }
    }

    async _loadProjectRollupSection() {
        this._setSectionLoading('project_rollup');
        try {
            const data = await this.api.getProjectRollup(this._baseParams());
            const projects = data.projects || [];
            if (!projects.length) {
                this._setSectionBody('project_rollup', '<div class="empty-state-hint">No project data in this window.</div>');
                this.loadedSections.add('project_rollup');
                return;
            }
            let html = '<div class="analytics-table-wrap"><table class="analytics-table"><thead><tr><th>Project</th><th>Agents</th><th>Tokens / turns</th></tr></thead><tbody>';
            for (const p of projects.slice(0, 30)) {
                const agents = (p.agents || []).join(', ');
                html += `<tr><td>${this._esc(p.project)}</td><td>${this._esc(agents)}</td><td>${Number(p.total_tokens).toLocaleString()}</td></tr>`;
            }
            html += '</tbody></table></div>';
            if (projects.length > 30) html += `<p class="empty-state-hint">Showing top 30 of ${projects.length} projects.</p>`;
            this._setSectionBody('project_rollup', html);
            this.loadedSections.add('project_rollup');
        } catch (err) {
            this._setSectionError('project_rollup', err);
        }
    }

    async _loadMcpHealthSection() {
        this._setSectionLoading('mcp_health');
        try {
            const data = await this.api.getMcpHealth(this._baseParams());
            const entries = data.entries || [];
            if (!entries.length) {
                this._setSectionBody('mcp_health', '<div class="empty-state-hint">No MCP call data in this window.</div>');
                this.loadedSections.add('mcp_health');
                return;
            }
            let html = '<div class="analytics-table-wrap"><table class="analytics-table"><thead><tr><th>Server</th><th>Tool</th><th>OK</th><th>Errors</th><th>Error rate</th></tr></thead><tbody>';
            for (const e of entries) {
                const pct = (e.error_rate * 100).toFixed(1);
                const cls = e.error_rate > 0.1 ? ' style="color:var(--error-color,#c0392b)"' : '';
                html += `<tr><td>${this._esc(e.server)}</td><td>${this._esc(e.tool)}</td><td>${Number(e.ok_calls).toLocaleString()}</td><td>${Number(e.error_calls).toLocaleString()}</td><td${cls}>${pct}%</td></tr>`;
            }
            html += '</tbody></table></div>';
            this._setSectionBody('mcp_health', html);
            this.loadedSections.add('mcp_health');
        } catch (err) {
            this._setSectionError('mcp_health', err);
        }
    }

    async _loadGuardianSection() {
        this._setSectionLoading('guardian');
        try {
            const data = await this.api.getGuardianStats(this._baseParams());
            if (!data.total_reviews) {
                this._setSectionBody('guardian', '<div class="empty-state-hint">No Guardian review data in this window.</div>');
                this.loadedSections.add('guardian');
                return;
            }
            const approvalPct = (data.approval_rate * 100).toFixed(1);
            const deniedCount = data.total_reviews - Math.round(data.approval_rate * data.total_reviews);
            let html = `
                <div class="usage-summary-cards">
                    <div class="usage-gauge-card"><div class="usage-card-label">Total reviews</div><div class="usage-card-value">${Number(data.total_reviews).toLocaleString()}</div></div>
                    <div class="usage-gauge-card"><div class="usage-card-label">Approval rate</div><div class="usage-card-value">${approvalPct}%</div><div class="gauge-bar"><div class="gauge-fill" style="width:${approvalPct}%"></div></div></div>
                    <div class="usage-gauge-card"><div class="usage-card-label">Actions blocked</div><div class="usage-card-value">${Number(deniedCount).toLocaleString()}</div><div class="gauge-hint">denied by Guardian</div></div>
                </div>`;
            // #insight-5: denial rate by risk level × action
            if (data.by_risk_level && data.by_risk_level.length) {
                html += '<h4 style="margin-top:1rem">Risk level breakdown</h4><div class="analytics-table-wrap"><table class="analytics-table"><thead><tr><th>Risk</th><th>Total</th><th>Denied</th><th>Deny %</th></tr></thead><tbody>';
                for (const r of data.by_risk_level) {
                    const denied = r.denied || 0;
                    const denyPct = r.count > 0 ? (denied / r.count * 100).toFixed(1) : '0.0';
                    const rowClass = denied > 0 ? ' class="row-warn"' : '';
                    html += `<tr${rowClass}><td>${this._esc(r.risk_level)}</td><td>${Number(r.count).toLocaleString()}</td><td>${Number(denied).toLocaleString()}</td><td>${denyPct}%</td></tr>`;
                }
                html += '</tbody></table></div>';
            }
            if (data.by_action && data.by_action.length) {
                html += '<h4 style="margin-top:1rem">Action breakdown</h4><div class="analytics-table-wrap"><table class="analytics-table"><thead><tr><th>Action</th><th>Total</th><th>Denied</th><th>Deny %</th></tr></thead><tbody>';
                for (const a of data.by_action) {
                    const denied = a.denied || 0;
                    const denyPct = a.count > 0 ? (denied / a.count * 100).toFixed(1) : '0.0';
                    const rowClass = denied > 0 ? ' class="row-warn"' : '';
                    html += `<tr${rowClass}><td>${this._esc(a.action)}</td><td>${Number(a.count).toLocaleString()}</td><td>${Number(denied).toLocaleString()}</td><td>${denyPct}%</td></tr>`;
                }
                html += '</tbody></table></div>';
                html += '<p class="table-hint">deny % = blocked ÷ total × 100. Rows with any blocks are highlighted.</p>';
            }
            this._setSectionBody('guardian', html);
            this.loadedSections.add('guardian');
        } catch (err) {
            this._setSectionError('guardian', err);
        }
    }

    async _loadMultiAgentSection() {
        this._setSectionLoading('multi_agent');
        try {
            const data = await this.api.getMultiAgentStats(this._baseParams());
            const roles = data.roles || [];
            if (!roles.length && !data.total_spawns) {
                this._setSectionBody('multi_agent', '<div class="empty-state-hint">No multi-agent spawn data in this window.</div>');
                this.loadedSections.add('multi_agent');
                return;
            }
            let html = `
                <div class="usage-summary-cards">
                    <div class="usage-gauge-card"><div class="usage-card-label">Total spawns</div><div class="usage-card-value">${Number(data.total_spawns).toLocaleString()}</div></div>
                    <div class="usage-gauge-card"><div class="usage-card-label">Total resumes</div><div class="usage-card-value">${Number(data.total_resumes).toLocaleString()}</div></div>
                </div>`;
            if (roles.length) {
                html += '<div class="analytics-table-wrap"><table class="analytics-table"><thead><tr><th>Role</th><th>Spawns</th><th>Resumes</th><th>Share</th></tr></thead><tbody>';
                for (const r of roles) {
                    html += `<tr><td>${this._esc(r.role)}</td><td>${Number(r.spawns).toLocaleString()}</td><td>${Number(r.resumes).toLocaleString()}</td><td>${r.share_pct.toFixed(1)}%</td></tr>`;
                }
                html += '</tbody></table></div>';
            }
            this._setSectionBody('multi_agent', html);
            this.loadedSections.add('multi_agent');
        } catch (err) {
            this._setSectionError('multi_agent', err);
        }
    }

    async _loadCodexTurnsSection() {
        this._setSectionLoading('codex_turns');
        try {
            const data = await this.api.getCodexTurnBreakdown(this._baseParams());
            const rows = data.rows || [];
            if (!rows.length) {
                this._setSectionBody('codex_turns', '<div class="empty-state-hint">No Codex turn data in this window.</div>');
                this.loadedSections.add('codex_turns');
                return;
            }
            const fmtMs = v => v != null ? `${v.toFixed(0)} ms` : '—';
            const fmtPct = v => v != null ? `${(v * 100).toFixed(1)}%` : '—';
            let html = '<div class="analytics-table-wrap"><table class="analytics-table"><thead><tr><th>Model</th><th>Project</th><th>Turns</th><th>Avg duration</th><th>Avg busy</th><th>Avg idle</th><th>Busy ratio</th></tr></thead><tbody>';
            for (const r of rows.slice(0, 30)) {
                html += `<tr><td>${this._esc(r.model)}</td><td>${this._esc(r.project)}</td><td>${Number(r.turn_count).toLocaleString()}</td><td>${fmtMs(r.avg_duration_ms)}</td><td>${fmtMs(r.avg_busy_ms)}</td><td>${fmtMs(r.avg_idle_ms)}</td><td>${fmtPct(r.busy_ratio)}</td></tr>`;
            }
            html += '</tbody></table></div>';
            if (rows.length > 30) html += `<p class="empty-state-hint">Showing top 30 of ${rows.length} rows.</p>`;
            this._setSectionBody('codex_turns', html);
            this.loadedSections.add('codex_turns');
        } catch (err) {
            this._setSectionError('codex_turns', err);
        }
    }

    async _loadSessionModelSection() {
        this._setSectionLoading('session_model');
        try {
            const data = await this.api.getSessionModelBreakdown(this._baseParams());
            const rows = data.rows || [];
            if (!rows.length) {
                this._setSectionBody('session_model', '<div class="empty-state-hint">No session × model data in this window.</div>');
                this.loadedSections.add('session_model');
                return;
            }
            const fmtTok = v => v != null ? Number(v).toLocaleString() : '—';
            const fmtCost = v => v != null ? `$${Number(v).toFixed(4)}` : '—';
            let html = '<div class="analytics-table-wrap"><table class="analytics-table"><thead><tr><th>Session</th><th>Model</th><th>Requests</th><th>Input tokens</th><th>Output tokens</th><th>Est. cost</th></tr></thead><tbody>';
            for (const r of rows.slice(0, 50)) {
                html += `<tr><td title="${this._esc(r.session_id)}">${this._esc(r.session_id.slice(0, 8))}</td><td>${this._esc(r.model)}</td><td>${Number(r.requests).toLocaleString()}</td><td>${fmtTok(r.input_tokens)}</td><td>${fmtTok(r.output_tokens)}</td><td>${fmtCost(r.cost)}</td></tr>`;
            }
            html += '</tbody></table></div>';
            if (rows.length > 50) html += `<p class="empty-state-hint">Showing top 50 of ${rows.length} rows (sorted by cost).</p>`;
            this._setSectionBody('session_model', html);
            this.loadedSections.add('session_model');
        } catch (err) {
            this._setSectionError('session_model', err);
        }
    }

    async _loadSpeedDistSection() {
        this._setSectionLoading('speed_dist');
        try {
            const data = await this.api.getSpeedDistribution(this._baseParams());
            const rows = data.rows || [];
            if (!rows.length) {
                this._setSectionBody('speed_dist', '<div class="empty-state-hint">No speed attribute data in this window. Only Claude Code spans carry the speed attribute.</div>');
                this.loadedSections.add('speed_dist');
                return;
            }
            const fmtTok = v => v != null ? Number(v).toLocaleString() : '—';
            let html = '<div class="analytics-table-wrap"><table class="analytics-table"><thead><tr><th>Speed / mode</th><th>Model</th><th>Requests</th><th>Input tokens</th><th>Output tokens</th></tr></thead><tbody>';
            for (const r of rows.slice(0, 40)) {
                const speed = r.speed != null ? this._esc(r.speed) : '<span class="dim">(not set)</span>';
                html += `<tr><td>${speed}</td><td>${this._esc(r.model)}</td><td>${Number(r.requests).toLocaleString()}</td><td>${fmtTok(r.input_tokens)}</td><td>${fmtTok(r.output_tokens)}</td></tr>`;
            }
            html += '</tbody></table></div>';
            if (rows.length > 40) html += `<p class="empty-state-hint">Showing top 40 of ${rows.length} rows.</p>`;
            this._setSectionBody('speed_dist', html);
            this.loadedSections.add('speed_dist');
        } catch (err) {
            this._setSectionError('speed_dist', err);
        }
    }

    async _loadCrossToolTtftSection() {
        this._setSectionLoading('cross_tool_ttft');
        try {
            const data = await this.api.getCrossToolTtft(this._timeParams());
            const rows = data.rows || [];
            if (!rows.length) {
                this._setSectionBody('cross_tool_ttft', '<div class="empty-state-hint">No TTFT span data in this window. Only Claude Code and opencode spans carry ttft_ms.</div>');
                this.loadedSections.add('cross_tool_ttft');
                return;
            }
            const fmtMs = v => v != null ? `${Math.round(v).toLocaleString()} ms` : '—';
            let html = '<div class="analytics-table-wrap"><table class="analytics-table"><thead><tr>' +
                '<th>Tool</th><th>Model</th><th>Count</th><th>Avg TTFT</th><th>Min</th><th>p90</th><th>Max</th>' +
                '</tr></thead><tbody>';
            for (const r of rows.slice(0, 60)) {
                html += `<tr>
                    <td>${this._esc(r.tool)}</td>
                    <td>${this._esc(r.model)}</td>
                    <td>${Number(r.count).toLocaleString()}</td>
                    <td>${fmtMs(r.avg_ms)}</td>
                    <td>${fmtMs(r.min_ms)}</td>
                    <td>${r.p90_ms != null ? fmtMs(r.p90_ms) : '<span class="dim">n/a</span>'}</td>
                    <td>${fmtMs(r.max_ms)}</td>
                </tr>`;
            }
            html += '</tbody></table></div>';
            if (rows.length > 60) html += `<p class="empty-state-hint">Showing top 60 of ${rows.length} rows.</p>`;
            this._setSectionBody('cross_tool_ttft', html);
            this.loadedSections.add('cross_tool_ttft');
        } catch (err) {
            this._setSectionError('cross_tool_ttft', err);
        }
    }

    async _loadHookOverheadSection() {
        this._setSectionLoading('hook_overhead');
        try {
            const data = await this.api.getHookOverhead(this._timeParams());
            const rows = data.rows || [];
            if (!rows.length) {
                this._setSectionBody('hook_overhead', '<div class="empty-state-hint">No Codex hook data in this window.</div>');
                this.loadedSections.add('hook_overhead');
                return;
            }
            const totalHours = (data.grand_total_ms / 3_600_000).toFixed(1);
            const fmtMs = v => v != null ? `${Math.round(v).toLocaleString()} ms` : '—';
            let html = `<p class="analytics-summary-line">Grand total hook time: <strong>${Number(data.grand_total_ms / 1000).toLocaleString(undefined, {maximumFractionDigits:0})} s</strong> (${totalHours} h)</p>`;
            html += '<div class="analytics-table-wrap"><table class="analytics-table"><thead><tr>' +
                '<th>Hook event</th><th>Invocations</th><th>Total time</th><th>Avg per call</th>' +
                '</tr></thead><tbody>';
            for (const r of rows) {
                html += `<tr>
                    <td>${this._esc(r.event)}</td>
                    <td>${Number(r.count).toLocaleString()}</td>
                    <td>${fmtMs(r.total_ms)}</td>
                    <td>${fmtMs(r.avg_ms)}</td>
                </tr>`;
            }
            html += '</tbody></table></div>';
            this._setSectionBody('hook_overhead', html);
            this.loadedSections.add('hook_overhead');
        } catch (err) {
            this._setSectionError('hook_overhead', err);
        }
    }

    async _loadReasoningShareSection() {
        this._setSectionLoading('reasoning_share');
        try {
            const data = await this.api.getReasoningShare(this._timeParams());
            const models = data.models || [];
            const effort = data.effort || [];
            if (!models.length && !effort.length) {
                this._setSectionBody('reasoning_share', '<div class="empty-state-hint">No reasoning/thinking token data in this window. Requires opencode or Codex with extended thinking enabled.</div>');
                this.loadedSections.add('reasoning_share');
                return;
            }
            const fmtTok = v => v != null ? Number(v).toLocaleString() : '—';
            let html = '';
            if (models.length) {
                html += '<h4 class="analytics-sub-heading">By model</h4>';
                html += '<div class="analytics-table-wrap"><table class="analytics-table"><thead><tr>' +
                    '<th>Model</th><th>Reasoning tokens</th><th>Output tokens</th><th>Share %</th><th>Est. cost</th>' +
                    '</tr></thead><tbody>';
                for (const m of models) {
                    const share = m.share_pct != null ? `${m.share_pct.toFixed(1)}%` : '—';
                    const cost = m.cost_usd != null ? `$${m.cost_usd.toFixed(4)}` : '—';
                    html += `<tr>
                        <td>${this._esc(m.model)}</td>
                        <td>${fmtTok(m.reasoning_tokens)}</td>
                        <td>${fmtTok(m.output_tokens)}</td>
                        <td>${share}</td>
                        <td>${cost}</td>
                    </tr>`;
                }
                html += '</tbody></table></div>';
            }
            if (effort.length) {
                html += '<h4 class="analytics-sub-heading">By effort level (Codex)</h4>';
                html += '<div class="analytics-table-wrap"><table class="analytics-table"><thead><tr>' +
                    '<th>Effort level</th><th>Reasoning tokens</th><th>Calls</th>' +
                    '</tr></thead><tbody>';
                for (const e of effort) {
                    html += `<tr>
                        <td>${this._esc(e.effort)}</td>
                        <td>${fmtTok(e.reasoning_tokens)}</td>
                        <td>${Number(e.calls).toLocaleString()}</td>
                    </tr>`;
                }
                html += '</tbody></table></div>';
            }
            this._setSectionBody('reasoning_share', html);
            this.loadedSections.add('reasoning_share');
        } catch (err) {
            this._setSectionError('reasoning_share', err);
        }
    }

    // ── Tool Failure Rates (#insight-1) ──────────────────────────────────────

    async _loadToolFailureRatesSection() {
        this._setSectionLoading('tool_failure_rates');
        try {
            const data = await this.api.getToolFailureRates(this._baseParams());
            const rows = (data && data.rows) || [];
            if (!rows.length) {
                this._setSectionBody('tool_failure_rates', '<div class="empty-state-hint">No opencode tool failure data in this window.</div>');
                this.loadedSections.add('tool_failure_rates');
                return;
            }
            const statEl = document.getElementById('analytics-section-stat-tool_failure_rates');
            if (statEl) statEl.textContent = `${rows.length} tool${rows.length === 1 ? '' : 's'} with failures`;
            const html = this._buildToolFailureRates(rows);
            this._setSectionBody('tool_failure_rates', html);
            this.loadedSections.add('tool_failure_rates');
        } catch (err) {
            this._setSectionError('tool_failure_rates', err);
        }
    }

    _buildToolFailureRates(rows) {
        const fmt = n => Number(n || 0).toLocaleString();
        const tableRows = rows.map(r => {
            const cls = r.fail_pct >= 50 ? 'row-error' : r.fail_pct >= 10 ? 'row-warn' : '';
            return `<tr class="${cls}">
                <td>${this._esc(r.tool)}</td>
                <td class="num">${fmt(r.total)}</td>
                <td class="num">${fmt(r.failures)}</td>
                <td class="num">${Number(r.fail_pct).toFixed(1)}%</td>
            </tr>`;
        }).join('');
        return `
            <h3>Tool failure rates (opencode)</h3>
            <p class="table-hint">Only tools with at least one failure are shown. ≥ 50% fail rate = <span class="badge-error">red</span>, 10–49% = <span class="badge-warn">amber</span>. Sorted by failure count descending.</p>
            <div class="table-scroll-x"><table class="data-table">
                <thead><tr>
                    <th>Tool</th><th>Total calls</th><th>Failures</th><th>Fail %</th>
                </tr></thead>
                <tbody>${tableRows}</tbody>
            </table></div>`;
    }

    // ── Daily Tool Mix (#insight-2) ───────────────────────────────────────────

    async _loadDailyToolMixSection() {
        this._setSectionLoading('daily_tool_mix');
        try {
            const data = await this.api.getDailyToolMix(this._baseParams());
            const rows = (data && data.rows) || [];
            const tools = (data && data.tools) || [];
            if (!rows.length) {
                this._setSectionBody('daily_tool_mix', '<div class="empty-state-hint">No tool activity data in this window.</div>');
                this.loadedSections.add('daily_tool_mix');
                return;
            }
            const html = this._buildDailyToolMix(rows, tools);
            this._setSectionBody('daily_tool_mix', html);
            this.loadedSections.add('daily_tool_mix');
        } catch (err) {
            this._setSectionError('daily_tool_mix', err);
        }
    }

    _buildDailyToolMix(rows, tools) {
        const dayMap = {};
        for (const r of rows) {
            if (!dayMap[r.day]) dayMap[r.day] = {};
            dayMap[r.day][r.tool] = r.datapoints;
        }
        const days = Object.keys(dayMap).sort();

        const toolColours = {
            claude_code: '#f5a623',
            opencode:    '#4f8cff',
            codex:       '#34c98e',
        };
        const defaultColours = ['#c65ce0', '#e5534b', '#5ac8c8'];
        const getColour = (tool, i) => toolColours[tool] || defaultColours[i % defaultColours.length];

        const maxTotal = Math.max(...days.map(d =>
            tools.reduce((s, t) => s + (dayMap[d][t] || 0), 0)
        ), 1);

        const bars = days.map(d => {
            const total = tools.reduce((s, t) => s + (dayMap[d][t] || 0), 0);
            const height = Math.max(4, Math.round((total / maxTotal) * 100));
            const segs = tools.map((tool, i) => {
                const v = dayMap[d][tool] || 0;
                const segH = total > 0 ? Math.round((v / total) * height) : 0;
                const colour = getColour(tool, i);
                return `<div style="height:${segH}px;background:${colour};width:100%" title="${this._esc(tool)}: ${Number(v).toLocaleString()}"></div>`;
            }).join('');
            const label = d.slice(5); // MM-DD
            return `<div class="ce-col" title="${this._esc(d)} — ${Number(total).toLocaleString()} datapoints">
                <div class="ce-stack" style="height:${height}px">${segs}</div>
                <div class="ce-label">${label}</div>
            </div>`;
        }).join('');

        const legend = tools.map((t, i) =>
            `<span><span class="ce-swatch" style="background:${getColour(t, i)}"></span>${this._esc(t)}</span>`
        ).join(' ');

        const headCells = ['Day', ...tools].map(h => `<th>${this._esc(h)}</th>`).join('');
        const tableRows = days.map(d => {
            const cells = tools.map(t => `<td class="num">${Number(dayMap[d][t] || 0).toLocaleString()}</td>`).join('');
            return `<tr><td>${this._esc(d)}</td>${cells}</tr>`;
        }).join('');

        return `
            <h3>Daily tool activity mix</h3>
            <p class="table-hint">Metric datapoints per tool per calendar day (UTC). Stacked bars show relative tool share each day.</p>
            <div class="ce-chart" style="align-items:flex-end">${bars}</div>
            <div class="ce-legend">${legend}</div>
            <div class="table-scroll-x"><table class="data-table">
                <thead><tr>${headCells}</tr></thead>
                <tbody>${tableRows}</tbody>
            </table></div>`;
    }

    // ── Skills Activity (#insight-3) ─────────────────────────────────────────

    async _loadSkillActivitySection() {
        this._setSectionLoading('skill_activity');
        try {
            const data = await this.api.getSkillActivity(this._baseParams());
            const rows = (data && data.rows) || [];
            if (!rows.length) {
                this._setSectionBody('skill_activity', '<div class="empty-state-hint">No Codex skill injection data in this window. Requires Codex with skills enabled.</div>');
                this.loadedSections.add('skill_activity');
                return;
            }
            const statEl = document.getElementById('analytics-section-stat-skill_activity');
            if (statEl) statEl.textContent = `${Number(data.total_injections || 0).toLocaleString()} injections`;
            const html = this._buildSkillActivity(rows, data.total_injections || 0);
            this._setSectionBody('skill_activity', html);
            this.loadedSections.add('skill_activity');
        } catch (err) {
            this._setSectionError('skill_activity', err);
        }
    }

    _buildSkillActivity(rows, totalInjections) {
        const fmt = n => Number(n || 0).toLocaleString();
        const tableRows = rows.map(r => {
            const sharePct = totalInjections > 0 ? (r.injections / totalInjections * 100).toFixed(1) : '0.0';
            return `<tr>
                <td>${this._esc(r.skill)}</td>
                <td>${this._esc(r.invoke_type)}</td>
                <td class="num">${fmt(r.injections)}</td>
                <td class="num">${sharePct}%</td>
            </tr>`;
        }).join('');
        return `
            <h3>Codex skill injections</h3>
            <p class="table-hint">${fmt(totalInjections)} total injections. Shows which skills are selected by the shadow model and injected into Codex threads. Use this to identify skills with real usage vs those sitting idle in your library.</p>
            <div class="table-scroll-x"><table class="data-table">
                <thead><tr>
                    <th>Skill</th><th>Invoke type</th><th>Injections</th><th>Share %</th>
                </tr></thead>
                <tbody>${tableRows}</tbody>
            </table></div>`;
    }
}

// Expose to the browser global; also export for the node --test parity
// tests (crates/otelite-api/tests/js/daily_throughput.test.mjs).
if (typeof window !== 'undefined') {
    window.AnalyticsView = AnalyticsView;
}
if (typeof module !== 'undefined' && module.exports) {
    module.exports = { AnalyticsView };
}
