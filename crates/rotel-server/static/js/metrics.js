// Metrics view implementation
class MetricsView {
    constructor(apiClient) {
        this.apiClient = apiClient;
        this.metrics = [];
        this.metricNames = [];
        this.selectedMetric = null;
        this.interval = 60; // 1 minute buckets
        this.autoRefreshInterval = null;
        this.timeWindowHours = 1;  // how many hours to show
        this.timeOffsetWindows = 0; // how many windows back in time (0 = now)
        this.resourceFilter = '';
    }

    async render() {
        const container = document.getElementById('metrics-view');
        container.innerHTML = `
            <div class="view-header">
                <h2>Metrics</h2>
                <div class="view-actions">
                    <button id="refresh-metrics" class="btn btn-primary">Refresh</button>
                    <button id="export-metrics" class="btn btn-secondary">Export</button>
                    <label class="auto-refresh-toggle">
                        <input type="checkbox" id="auto-refresh-metrics" checked>
                        Auto-refresh (30s)
                    </label>
                </div>
            </div>
            <div class="filters">
                <datalist id="metrics-resource-keys-list"></datalist>
                <input type="text" id="metrics-resource-filter" placeholder="Resource filter (e.g., service.name=my-service)" class="filter-input" list="metrics-resource-keys-list">
                <button id="apply-metrics-resource-filter" class="btn btn-primary">Apply</button>
                <button id="clear-metrics-resource-filter" class="btn btn-secondary">Clear</button>
            </div>
            <div class="metrics-layout">
                <div class="metrics-sidebar" id="metrics-sidebar">
                    <div class="empty-state">Loading...</div>
                </div>
                <div id="metrics-h-handle" class="layout-drag-handle-v"></div>
                <div class="metrics-detail" id="metrics-detail">
                    <div class="empty-state">Select a metric to view</div>
                </div>
            </div>
        `;

        this.attachEventListeners();
        await this.loadMetrics();
        this.startAutoRefresh();
    }

    async loadMetrics() {
        try {
            const params = { limit: 500 };
            if (this.resourceFilter) params.resource = this.resourceFilter;
            this.metrics = await this.apiClient.getMetrics(params);
            this.metricNames = [...new Set(this.metrics.map(m => m.name))].sort();

            this.renderSidebar();

            if (!this.selectedMetric && this.metricNames.length > 0) {
                this.selectedMetric = this.metricNames[0];
            }
            if (this.selectedMetric) {
                await this.renderDetail(this.selectedMetric);
            }
        } catch (error) {
            console.error('Failed to load metrics:', error);
            document.getElementById('metrics-sidebar').innerHTML =
                '<div class="empty-state">Failed to load metrics</div>';
        }
    }

    renderSidebar() {
        const sidebar = document.getElementById('metrics-sidebar');
        if (this.metricNames.length === 0) {
            sidebar.innerHTML = '<div class="empty-state">No metrics yet</div>';
            return;
        }

        const searchVal = (document.getElementById('metrics-search')?.value ?? '').toLowerCase();
        const filteredNames = searchVal
            ? this.metricNames.filter(n => n.toLowerCase().includes(searchVal))
            : this.metricNames;

        // Keep search box, replace items below it
        const existingSearch = sidebar.querySelector('.metrics-sidebar-search');
        if (!existingSearch) {
            sidebar.innerHTML = `<input type="text" id="metrics-search" class="metrics-sidebar-search" placeholder="Filter metrics...">`;
            document.getElementById('metrics-search').addEventListener('input', () => this.renderSidebar());
        }

        // Remove old items
        sidebar.querySelectorAll('.metric-sidebar-item').forEach(el => el.remove());

        filteredNames.forEach(name => {
            const points = this.metrics.filter(m => m.name === name);
            const latest = points.reduce((a, b) => a.timestamp > b.timestamp ? a : b, points[0]);
            const valueStr = this.formatValue(latest);
            const isSelected = name === this.selectedMetric;

            const item = document.createElement('div');
            item.className = `metric-sidebar-item${isSelected ? ' selected' : ''}`;
            item.dataset.metric = name;
            item.innerHTML = `
                <div class="metric-sidebar-name">${this.escapeHtml(name)}</div>
                <div class="metric-sidebar-meta">
                    <span class="metric-type-badge metric-type-${latest.metric_type}">${latest.metric_type}</span>
                    <span class="metric-sidebar-value">${valueStr}</span>
                </div>
            `;
            item.addEventListener('click', () => {
                this.selectedMetric = name;
                sidebar.querySelectorAll('.metric-sidebar-item').forEach(i => i.classList.remove('selected'));
                item.classList.add('selected');
                this.renderDetail(name);
            });
            sidebar.appendChild(item);
        });

    }

    async renderDetail(metricName) {
        const detail = document.getElementById('metrics-detail');
        detail.innerHTML = `
            <div class="metric-detail-header">
                <h3>${this.escapeHtml(metricName)}</h3>
                <div class="metric-detail-controls">
                    <label>Bucket:
                        <select id="interval-select" class="filter-select">
                            <option value="10">10s</option>
                            <option value="60" selected>1 min</option>
                            <option value="300">5 min</option>
                            <option value="3600">1 hr</option>
                        </select>
                    </label>
                    <label>Window:
                        <select id="window-select" class="filter-select">
                            <option value="0.25">15 min</option>
                            <option value="1" selected>1 hr</option>
                            <option value="6">6 hr</option>
                            <option value="24">24 hr</option>
                        </select>
                    </label>
                </div>
            </div>
            <div class="metric-chart-area">
                <div class="chart-time-nav">
                    <span id="chart-time-range">—</span>
                    <div class="chart-time-nav-controls">
                        <button class="btn-icon" id="chart-prev" title="Earlier">&#8592;</button>
                        <button class="btn-icon" id="chart-now" title="Jump to now">Now</button>
                        <button class="btn-icon" id="chart-next" title="Later">&#8594;</button>
                    </div>
                </div>
                <canvas id="metrics-chart"></canvas>
            </div>
            <div class="metric-data-table">
                <h4>Data points</h4>
                <div id="metric-data-rows"></div>
            </div>
        `;

        document.getElementById('interval-select').addEventListener('change', (e) => {
            this.interval = parseInt(e.target.value);
            this.loadTimeseries(metricName);
        });
        document.getElementById('window-select').addEventListener('change', (e) => {
            this.timeWindowHours = parseFloat(e.target.value);
            this.timeOffsetWindows = 0;
            this.refreshCurrentValue(metricName);
            this.loadTimeseries(metricName);
        });
        document.getElementById('chart-prev').addEventListener('click', () => {
            this.timeOffsetWindows++;
            this.loadTimeseries(metricName);
        });
        document.getElementById('chart-next').addEventListener('click', () => {
            if (this.timeOffsetWindows > 0) this.timeOffsetWindows--;
            this.loadTimeseries(metricName);
        });
        document.getElementById('chart-now').addEventListener('click', () => {
            this.timeOffsetWindows = 0;
            this.loadTimeseries(metricName);
        });

        this.renderCurrentValue(metricName);
        this.renderDataTable(metricName);
        await this.loadTimeseries(metricName);
    }

    refreshCurrentValue(metricName) {
        const heroEl = document.getElementById('metric-hero-value');
        if (heroEl) heroEl.remove();
        const bucketContainer = document.getElementById('histogram-bucket-container');
        if (bucketContainer) bucketContainer.remove();
        this.renderCurrentValue(metricName);
    }

    renderCurrentValue(metricName) {
        const points = this.metrics.filter(m => m.name === metricName);
        if (points.length === 0) return;
        const latest = points.reduce((a, b) => a.timestamp > b.timestamp ? a : b, points[0]);
        const type = latest.metric_type;
        const v = latest.value;
        const detail = document.getElementById('metrics-detail');
        const header = detail.querySelector('.metric-detail-header');

        if (type === 'histogram' || type === 'summary') {
            if (v !== null && typeof v === 'object') {
                const avg = (v.count > 0 && v.sum !== undefined) ? v.sum / v.count : 0;
                const unit = latest.unit || '';
                const heroEl = document.createElement('div');
                heroEl.id = 'metric-hero-value';
                heroEl.style.cssText = 'display:flex;align-items:baseline;gap:0.5rem;padding:0.5rem 0 0.75rem;border-bottom:1px solid var(--border-color);margin-bottom:0.5rem;flex-wrap:wrap;';
                heroEl.innerHTML = `
                    <span style="font-size:2.5rem;font-weight:700;font-family:monospace;color:#818cf8;line-height:1;">${this.formatChartValue(avg)}</span>
                    <span style="font-size:1rem;color:var(--text-secondary);">avg${unit ? ' ' + unit : ''}</span>
                    <span style="font-size:0.85rem;color:var(--text-secondary);padding-left:0.5rem;">${v.count !== undefined ? v.count.toLocaleString() + ' obs' : ''}</span>
                    <span style="font-size:0.75rem;color:var(--text-secondary);margin-left:auto;">${new Date(latest.timestamp / 1_000_000).toLocaleString()}</span>
                `;
                if (header) header.after(heroEl);

                // Bucket distribution chart for histograms
                if (type === 'histogram' && v.buckets && v.buckets.length > 0) {
                    const bucketContainer = document.createElement('div');
                    bucketContainer.id = 'histogram-bucket-container';
                    bucketContainer.style.cssText = 'padding:0.5rem 0 0.75rem;border-bottom:1px solid var(--border-color);margin-bottom:0.5rem;';
                    bucketContainer.innerHTML = `
                        <div style="font-size:0.7rem;color:var(--text-secondary);text-transform:uppercase;letter-spacing:0.05em;margin-bottom:0.4rem;">Bucket Distribution</div>
                        <canvas id="histogram-bucket-chart"></canvas>
                    `;
                    heroEl.after(bucketContainer);
                    requestAnimationFrame(() => this.renderHistogramBucketChart(v.buckets, v.count, unit));
                }
            }
            return;
        }

        // Counter / gauge: show current value as hero
        if (type !== 'counter' && type !== 'gauge') return;

        const formatted = this.formatValue(latest);
        const unit = latest.unit ? ` ${latest.unit}` : '';
        const heroEl = document.createElement('div');
        heroEl.id = 'metric-hero-value';
        heroEl.style.cssText = 'display:flex;align-items:baseline;gap:0.5rem;padding:0.5rem 0 0.75rem;border-bottom:1px solid var(--border-color);margin-bottom:0.5rem;';
        heroEl.innerHTML = `
            <span style="font-size:2.5rem;font-weight:700;font-family:monospace;color:#818cf8;line-height:1;">${formatted}</span>
            ${unit ? `<span style="font-size:1rem;color:var(--text-secondary);">${unit}</span>` : ''}
            <span style="font-size:0.75rem;color:var(--text-secondary);margin-left:auto;">${new Date(latest.timestamp / 1_000_000).toLocaleString()}</span>
        `;
        if (header) header.after(heroEl);
    }

    renderHistogramBucketChart(buckets, totalCount, _unit) {
        const canvas = document.getElementById('histogram-bucket-chart');
        if (!canvas) return;

        // Sort by upper_bound
        const sorted = [...buckets].sort((a, b) => a.upper_bound - b.upper_bound);

        // Determine if cumulative: if last finite bucket count ≈ totalCount, it's cumulative
        const finiteBuckets = sorted.filter(b => b.upper_bound !== null && Number.isFinite(b.upper_bound));
        const lastFiniteCount = finiteBuckets.length > 0 ? finiteBuckets[finiteBuckets.length - 1].count : 0;
        let perBucket;
        if (totalCount > 0 && Math.abs(lastFiniteCount - totalCount) < 2) {
            // Cumulative → convert to per-bucket deltas
            perBucket = sorted.map((b, i) => ({
                upper_bound: b.upper_bound,
                count: i === 0 ? b.count : Math.max(0, b.count - sorted[i - 1].count),
            }));
        } else {
            perBucket = sorted;
        }

        // Build display bars (include +Inf as ">max" label, skip zero-count)
        const bars = perBucket
            .filter(b => b.count > 0)
            .map(b => {
                const ub = b.upper_bound;
                const label = (ub === null || ub === undefined || !Number.isFinite(ub))
                    ? '+Inf'
                    : (ub >= 1000 ? (ub / 1000).toFixed(1) + 'K' : String(ub));
                return { label, count: b.count };
            });

        if (bars.length === 0) return;

        const dpr = window.devicePixelRatio || 1;
        const rect = canvas.parentElement.getBoundingClientRect();
        const cssW = Math.max((rect.width || 500) - 16, 200);
        const cssH = 140;
        canvas.style.width = cssW + 'px';
        canvas.style.height = cssH + 'px';
        canvas.width = cssW * dpr;
        canvas.height = cssH * dpr;
        const ctx = canvas.getContext('2d');
        ctx.scale(dpr, dpr);
        ctx.clearRect(0, 0, cssW, cssH);

        const maxCount = Math.max(...bars.map(b => b.count));
        const padL = 44, padR = 8, padT = 8, padB = 28;
        const w = cssW - padL - padR;
        const h = cssH - padT - padB;
        const barW = Math.max(4, (w / bars.length) - 2);

        // Grid lines
        ctx.strokeStyle = '#1e1e1e';
        ctx.lineWidth = 1;
        for (let i = 0; i <= 4; i++) {
            const y = padT + (i / 4) * h;
            ctx.beginPath();
            ctx.moveTo(padL, y);
            ctx.lineTo(padL + w, y);
            ctx.stroke();
        }

        // Y-axis labels
        ctx.fillStyle = '#64748b';
        ctx.font = '10px monospace';
        ctx.textAlign = 'right';
        for (let i = 0; i <= 4; i++) {
            const v = Math.round(maxCount * (1 - i / 4));
            ctx.fillText(v, padL - 4, padT + (i / 4) * h + 4);
        }

        // Bars
        bars.forEach((b, i) => {
            const barH = (b.count / maxCount) * h;
            const x = padL + i * (w / bars.length);
            const y = padT + h - barH;
            const grad = ctx.createLinearGradient(0, y, 0, y + barH);
            grad.addColorStop(0, '#818cf8');
            grad.addColorStop(1, 'rgba(129,140,248,0.25)');
            ctx.fillStyle = grad;
            ctx.fillRect(x + 1, y, barW, barH);

            // X label (upper_bound)
            ctx.fillStyle = '#64748b';
            ctx.font = '9px monospace';
            ctx.textAlign = 'center';
            ctx.fillText(b.label, x + barW / 2 + 1, cssH - 6);
        });
    }

    timeWindow() {
        const windowMs = this.timeWindowHours * 3600 * 1000;
        const endMs = Date.now() - this.timeOffsetWindows * windowMs;
        const startMs = endMs - windowMs;
        return {
            start_time: startMs * 1_000_000,   // nanoseconds
            end_time: endMs * 1_000_000,
        };
    }

    renderDataTable(metricName) {
        const points = this.metrics
            .filter(m => m.name === metricName)
            .sort((a, b) => b.timestamp - a.timestamp);

        const rows = document.getElementById('metric-data-rows');
        if (!rows) return;

        if (points.length === 0) {
            rows.innerHTML = '<div class="empty-state">No data points</div>';
            return;
        }

        rows.innerHTML = `
            <table class="data-table">
                <thead><tr>
                    <th>Time</th>
                    <th>Value</th>
                    <th>Labels</th>
                </tr></thead>
                <tbody>
                    ${points.map(p => `
                        <tr>
                            <td class="data-cell-time">${this.formatTimestamp(p.timestamp)}</td>
                            <td class="data-cell-value">${this.formatValue(p)}${p.unit ? ' ' + p.unit : ''}</td>
                            <td class="data-cell-labels">${this.formatLabels(p.attributes)}</td>
                        </tr>
                    `).join('')}
                </tbody>
            </table>
        `;
    }

    // Only show discriminating attributes — skip IDs, otel internals, and resource-level keys
    formatLabels(attributes) {
        const skipKeys = new Set([
            'session.id', 'user.id', 'trace.id', 'span.id',
            'otel.scope.name', 'otel.scope.version',
            'service.name', 'service.version',
            'os.type', 'os.version', 'host.arch',
        ]);
        const entries = Object.entries(attributes)
            .filter(([k]) => !skipKeys.has(k) && !k.endsWith('.id'))
            .filter(([, v]) => String(v).length <= 40);

        if (entries.length === 0) return '<span class="no-labels">—</span>';
        return entries.map(([k, v]) =>
            `<span class="label-tag">${this.escapeHtml(k)}: <strong>${this.escapeHtml(String(v))}</strong></span>`
        ).join('');
    }

    async loadTimeseries(metricName) {
        try {
            const { start_time, end_time } = this.timeWindow();
            const buckets = await this.apiClient.getMetricTimeseries(metricName, {
                step: this.interval,
                start_time,
                end_time,
            });
            this._lastBuckets = buckets;
            this.renderChart(metricName, buckets);
            this.attachResizeObserver();
            // Update time range label
            const label = document.getElementById('chart-time-range');
            if (label) {
                const fmt = t => new Date(t / 1_000_000).toLocaleTimeString();
                label.textContent = `${fmt(start_time)} – ${fmt(end_time)}`;
            }
            // Disable "next" button when at now
            const nextBtn = document.getElementById('chart-next');
            if (nextBtn) nextBtn.disabled = this.timeOffsetWindows === 0;
        } catch (error) {
            console.error('Failed to load timeseries:', error);
        }
    }

    renderChart(_metricName, buckets) {
        const canvas = document.getElementById('metrics-chart');
        if (!canvas) return;
        const ctx = canvas.getContext('2d');

        // Size canvas to its container, accounting for device pixel ratio
        const dpr = window.devicePixelRatio || 1;
        const rect = canvas.parentElement.getBoundingClientRect();
        const cssW = (rect.width || 700) - 16; // subtract padding
        const cssH = 260;
        canvas.style.width = cssW + 'px';
        canvas.style.height = cssH + 'px';
        canvas.width = cssW * dpr;
        canvas.height = cssH * dpr;
        ctx.scale(dpr, dpr);
        // store css dims for drawing
        const drawW = cssW;
        const drawH = cssH;
        ctx.clearRect(0, 0, drawW, drawH);

        if (!buckets || buckets.length === 0) {
            ctx.font = '14px sans-serif';
            ctx.fillStyle = '#64748b';
            ctx.textAlign = 'center';
            ctx.fillText('No timeseries data', drawW / 2, drawH / 2);
            return;
        }

        // With a single point, show value prominently
        if (buckets.length === 1) {
            ctx.font = 'bold 36px sans-serif';
            ctx.fillStyle = '#818cf8';
            ctx.textAlign = 'center';
            ctx.fillText(this.formatChartValue(buckets[0].value), drawW / 2, drawH / 2);
            const t = new Date(buckets[0].timestamp / 1000000).toLocaleString();
            ctx.font = '12px sans-serif';
            ctx.fillStyle = '#64748b';
            ctx.fillText(t, drawW / 2, drawH / 2 + 28);
            return;
        }

        const padL = 60, padR = 16, padT = 16, padB = 32;
        const w = drawW - padL - padR;
        const h = drawH - padT - padB;

        const values = buckets.map(b => b.value);
        const minV = Math.min(...values);
        const maxV = Math.max(...values);
        const rangeV = maxV - minV || 1;

        // Grid lines
        ctx.strokeStyle = '#1e1e1e';
        ctx.lineWidth = 1;
        for (let i = 0; i <= 4; i++) {
            const y = padT + (i / 4) * h;
            ctx.beginPath();
            ctx.moveTo(padL, y);
            ctx.lineTo(padL + w, y);
            ctx.stroke();
        }

        // Y-axis labels
        ctx.fillStyle = '#64748b';
        ctx.font = '11px monospace';
        ctx.textAlign = 'right';
        for (let i = 0; i <= 4; i++) {
            const v = maxV - (rangeV * i / 4);
            const y = padT + (i / 4) * h;
            ctx.fillText(this.formatChartValue(v), padL - 4, y + 4);
        }

        // Area fill under line
        ctx.beginPath();
        buckets.forEach((b, i) => {
            const x = padL + (i / (buckets.length - 1)) * w;
            const y = padT + ((maxV - b.value) / rangeV) * h;
            i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y);
        });
        ctx.lineTo(padL + w, padT + h);
        ctx.lineTo(padL, padT + h);
        ctx.closePath();
        const grad = ctx.createLinearGradient(0, padT, 0, padT + h);
        grad.addColorStop(0, 'rgba(74, 222, 128, 0.25)');
        grad.addColorStop(1, 'rgba(74, 222, 128, 0.02)');
        ctx.fillStyle = grad;
        ctx.fill();

        // Line
        ctx.strokeStyle = '#4ade80';
        ctx.lineWidth = 2;
        ctx.beginPath();
        buckets.forEach((b, i) => {
            const x = padL + (i / (buckets.length - 1)) * w;
            const y = padT + ((maxV - b.value) / rangeV) * h;
            i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y);
        });
        ctx.stroke();

        // Dots
        ctx.fillStyle = '#4ade80';
        buckets.forEach((b, i) => {
            const x = padL + (i / (buckets.length - 1)) * w;
            const y = padT + ((maxV - b.value) / rangeV) * h;
            ctx.beginPath();
            ctx.arc(x, y, 3, 0, Math.PI * 2);
            ctx.fill();
        });

        // X-axis labels
        ctx.fillStyle = '#64748b';
        ctx.font = '10px monospace';
        ctx.textAlign = 'center';
        const labelCount = Math.min(6, buckets.length);
        for (let i = 0; i < labelCount; i++) {
            const idx = Math.round(i * (buckets.length - 1) / (labelCount - 1));
            const x = padL + (idx / (buckets.length - 1)) * w;
            const t = new Date(buckets[idx].timestamp / 1000000).toLocaleTimeString();
            ctx.fillText(t, x, drawH - 4);
        }
    }

    // MetricValue is untagged: Gauge/Counter → plain number, Histogram/Summary → {sum, count, buckets/quantiles}
    formatValue(metric) {
        const v = metric.value;
        if (typeof v === 'number') {
            return v.toLocaleString(undefined, { maximumFractionDigits: 2 });
        }
        // Histogram or Summary: {sum, count, ...}
        if (v !== null && typeof v === 'object') {
            if (v.count > 0 && v.sum !== undefined) {
                const avg = v.sum / v.count;
                return `avg ${avg.toLocaleString(undefined, { maximumFractionDigits: 2 })}`;
            }
            if (v.count !== undefined) return `${v.count} obs`;
        }
        return '?';
    }

    formatChartValue(v) {
        if (v >= 1_000_000_000) return (v / 1_000_000_000).toFixed(2) + 'B';
        if (v >= 1_000_000) return (v / 1_000_000).toFixed(2) + 'M';
        if (v >= 1_000) return (v / 1_000).toFixed(1) + 'K';
        if (v >= 10) return v.toFixed(1);
        return v.toFixed(3);
    }

    formatTimestamp(nanos) {
        return new Date(nanos / 1000000).toLocaleString();
    }

    async exportMetrics() {
        try {
            const blob = await this.apiClient.exportMetrics({ format: 'json' });
            const url = window.URL.createObjectURL(blob);
            const a = document.createElement('a');
            a.href = url;
            a.download = 'metrics.json';
            document.body.appendChild(a);
            a.click();
            window.URL.revokeObjectURL(url);
            document.body.removeChild(a);
        } catch (error) {
            console.error('Failed to export metrics:', error);
        }
    }

    attachEventListeners() {
        document.getElementById('refresh-metrics').addEventListener('click', () => this.loadMetrics());
        document.getElementById('export-metrics').addEventListener('click', () => this.exportMetrics());
        document.getElementById('auto-refresh-metrics').addEventListener('change', (e) => {
            e.target.checked ? this.startAutoRefresh() : this.stopAutoRefresh();
        });
        document.getElementById('apply-metrics-resource-filter').addEventListener('click', () => {
            this.resourceFilter = document.getElementById('metrics-resource-filter').value;
            this.loadMetrics();
        });
        document.getElementById('clear-metrics-resource-filter').addEventListener('click', () => {
            this.resourceFilter = '';
            document.getElementById('metrics-resource-filter').value = '';
            this.loadMetrics();
        });
        this.attachHorizontalDragResize(
            document.getElementById('metrics-sidebar'),
            document.getElementById('metrics-h-handle')
        );
        this.loadResourceKeys();
    }

    async loadResourceKeys() {
        try {
            const response = await this.apiClient.getResourceKeys('metrics');
            const datalist = document.getElementById('metrics-resource-keys-list');
            if (!datalist) return;
            datalist.innerHTML = response.keys
                .map(k => `<option value="${k}=">`)
                .join('');
        } catch (_error) {
            // Non-critical; silently ignore
        }
    }

    attachHorizontalDragResize(leftPanel, handle) {
        if (!leftPanel || !handle) return;
        let startX, startW;
        handle.addEventListener('mousedown', e => {
            startX = e.clientX;
            startW = leftPanel.offsetWidth;
            handle.classList.add('dragging');
            const onMove = e => {
                const newW = Math.max(160, Math.min(500, startW + (e.clientX - startX)));
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

    startAutoRefresh() {
        this.stopAutoRefresh();
        this.autoRefreshInterval = setInterval(() => this.loadMetrics(), 30000);
    }

    stopAutoRefresh() {
        if (this.autoRefreshInterval) {
            clearInterval(this.autoRefreshInterval);
            this.autoRefreshInterval = null;
        }
    }

    attachResizeObserver() {
        if (this._resizeObserver) this._resizeObserver.disconnect();
        const chartArea = document.querySelector('.metric-chart-area');
        if (!chartArea) return;
        this._resizeObserver = new ResizeObserver(() => {
            if (this.selectedMetric && this._lastBuckets) {
                this.renderChart(this.selectedMetric, this._lastBuckets);
            }
        });
        this._resizeObserver.observe(chartArea);
    }

    escapeHtml(text) {
        const div = document.createElement('div');
        div.textContent = String(text);
        return div.innerHTML;
    }

    destroy() {
        this.stopAutoRefresh();
        if (this._resizeObserver) {
            this._resizeObserver.disconnect();
            this._resizeObserver = null;
        }
    }
}

// Export for use in app.js
window.MetricsView = MetricsView;
