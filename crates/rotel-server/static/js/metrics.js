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
            <div class="metrics-layout">
                <div class="metrics-sidebar" id="metrics-sidebar">
                    <div class="empty-state">Loading...</div>
                </div>
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
            this.metrics = await this.apiClient.getMetrics({ limit: 500 });
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

        this.renderDataTable(metricName);
        await this.loadTimeseries(metricName);
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
            this.renderChart(metricName, buckets);
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

    renderChart(metricName, buckets) {
        const canvas = document.getElementById('metrics-chart');
        if (!canvas) return;
        const ctx = canvas.getContext('2d');

        canvas.width = canvas.offsetWidth || 700;
        canvas.height = 280;
        ctx.clearRect(0, 0, canvas.width, canvas.height);

        if (!buckets || buckets.length === 0) {
            ctx.font = '14px sans-serif';
            ctx.fillStyle = '#888';
            ctx.textAlign = 'center';
            ctx.fillText('No timeseries data', canvas.width / 2, canvas.height / 2);
            return;
        }

        // With a single point, just show the value
        if (buckets.length === 1) {
            ctx.font = 'bold 32px sans-serif';
            ctx.fillStyle = 'var(--text-primary, #333)';
            ctx.textAlign = 'center';
            ctx.fillText(buckets[0].value.toFixed(2), canvas.width / 2, canvas.height / 2);
            const t = new Date(buckets[0].timestamp / 1000000).toLocaleTimeString();
            ctx.font = '12px sans-serif';
            ctx.fillStyle = '#888';
            ctx.fillText(t, canvas.width / 2, canvas.height / 2 + 24);
            return;
        }

        const padL = 52, padR = 16, padT = 16, padB = 32;
        const w = canvas.width - padL - padR;
        const h = canvas.height - padT - padB;

        const values = buckets.map(b => b.value);
        const minV = Math.min(...values);
        const maxV = Math.max(...values);
        const rangeV = maxV - minV || 1;

        // Grid lines
        ctx.strokeStyle = '#333';
        ctx.lineWidth = 1;
        for (let i = 0; i <= 4; i++) {
            const y = padT + (i / 4) * h;
            ctx.beginPath();
            ctx.moveTo(padL, y);
            ctx.lineTo(padL + w, y);
            ctx.stroke();
        }

        // Y-axis labels
        ctx.fillStyle = '#888';
        ctx.font = '11px sans-serif';
        ctx.textAlign = 'right';
        for (let i = 0; i <= 4; i++) {
            const v = maxV - (rangeV * i / 4);
            const y = padT + (i / 4) * h;
            ctx.fillText(v.toFixed(1), padL - 4, y + 4);
        }

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
        ctx.fillStyle = '#888';
        ctx.font = '10px sans-serif';
        ctx.textAlign = 'center';
        const labelCount = Math.min(6, buckets.length);
        for (let i = 0; i < labelCount; i++) {
            const idx = Math.round(i * (buckets.length - 1) / (labelCount - 1));
            const x = padL + (idx / (buckets.length - 1)) * w;
            const t = new Date(buckets[idx].timestamp / 1000000).toLocaleTimeString();
            ctx.fillText(t, x, canvas.height - 4);
        }
    }

    formatValue(metric) {
        const v = metric.value;
        if (typeof v === 'number') return v.toLocaleString(undefined, { maximumFractionDigits: 2 });
        if (v.Gauge !== undefined) return v.Gauge.toLocaleString(undefined, { maximumFractionDigits: 2 });
        if (v.Counter !== undefined) return v.Counter.toLocaleString();
        if (v.Histogram) return `${v.Histogram.count} obs`;
        if (v.Summary) return `${v.Summary.count} obs`;
        return '?';
    }

    formatTimestamp(nanos) {
        return new Date(nanos / 1000000).toLocaleString();
    }

    attachEventListeners() {
        document.getElementById('refresh-metrics').addEventListener('click', () => this.loadMetrics());
        document.getElementById('export-metrics').addEventListener('click', () => {
            window.open('/api/metrics/export', '_blank');
        });
        document.getElementById('auto-refresh-metrics').addEventListener('change', (e) => {
            e.target.checked ? this.startAutoRefresh() : this.stopAutoRefresh();
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

    escapeHtml(text) {
        const div = document.createElement('div');
        div.textContent = String(text);
        return div.innerHTML;
    }

    destroy() {
        this.stopAutoRefresh();
    }
}

// Export for use in app.js
window.MetricsView = MetricsView;
