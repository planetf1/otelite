// Metrics view implementation
class MetricsView {
    constructor(apiClient) {
        this.apiClient = apiClient;
        this.container = document.getElementById('metrics-view');
        this.metrics = [];
        this.metricNames = [];
        this.selectedMetric = null;
        this.aggregation = 'avg';
        this.interval = 60; // 1 minute buckets
        this.autoRefreshInterval = null;
    }

    async render() {
        this.container.innerHTML = `
            <div class="metrics-header">
                <h2>Metrics</h2>
                <div class="metrics-controls">
                    <select id="metric-name-select" class="metric-select">
                        <option value="">Select a metric...</option>
                    </select>
                    <select id="aggregation-select" class="metric-select">
                        <option value="avg">Average</option>
                        <option value="sum">Sum</option>
                        <option value="min">Minimum</option>
                        <option value="max">Maximum</option>
                    </select>
                    <select id="interval-select" class="metric-select">
                        <option value="10">10 seconds</option>
                        <option value="60" selected>1 minute</option>
                        <option value="300">5 minutes</option>
                        <option value="600">10 minutes</option>
                        <option value="3600">1 hour</option>
                    </select>
                    <button id="refresh-metrics" class="btn btn-primary">Refresh</button>
                    <button id="export-metrics" class="btn btn-secondary">Export</button>
                    <label class="auto-refresh-label">
                        <input type="checkbox" id="auto-refresh-metrics" checked>
                        Auto-refresh (30s)
                    </label>
                </div>
            </div>
            <div class="metrics-content">
                <div class="metrics-chart-container">
                    <canvas id="metrics-chart"></canvas>
                </div>
                <div class="metrics-list-container">
                    <h3>Recent Metrics</h3>
                    <div id="metrics-list" class="metrics-list"></div>
                </div>
            </div>
        `;

        await this.loadMetricNames();
        this.attachEventListeners();
        await this.loadMetrics();
        this.startAutoRefresh();
    }

    async loadMetricNames() {
        try {
            const names = await this.apiClient.getMetricNames();
            this.metricNames = names;

            const select = document.getElementById('metric-name-select');
            names.forEach(name => {
                const option = document.createElement('option');
                option.value = name;
                option.textContent = name;
                select.appendChild(option);
            });

            // Select first metric by default
            if (names.length > 0) {
                this.selectedMetric = names[0];
                select.value = names[0];
            }
        } catch (error) {
            console.error('Failed to load metric names:', error);
        }
    }

    async loadMetrics() {
        try {
            // Load recent metrics
            const metrics = await this.apiClient.getMetrics({
                limit: 100
            });
            this.metrics = metrics;
            this.renderMetricsList();

            // Load aggregated data for chart if metric selected
            if (this.selectedMetric) {
                await this.loadAggregatedData();
            }
        } catch (error) {
            console.error('Failed to load metrics:', error);
            this.showError('Failed to load metrics');
        }
    }

    async loadAggregatedData() {
        if (!this.selectedMetric) return;

        try {
            // Use the new timeseries endpoint
            const buckets = await this.apiClient.getMetricTimeseries(this.selectedMetric, {
                step: this.interval
            });

            // Wrap in data structure expected by renderChart
            const data = {
                name: this.selectedMetric,
                function: this.aggregation,
                buckets: buckets
            };

            this.renderChart(data);
        } catch (error) {
            console.error('Failed to load aggregated data:', error);
        }
    }

    renderMetricsList() {
        const listContainer = document.getElementById('metrics-list');

        if (this.metrics.length === 0) {
            listContainer.innerHTML = '<div class="empty-state">No metrics found</div>';
            return;
        }

        listContainer.innerHTML = this.metrics.map(metric => `
            <div class="metric-item">
                <div class="metric-header">
                    <span class="metric-name">${this.escapeHtml(metric.name)}</span>
                    <span class="metric-type metric-type-${metric.metric_type}">${metric.metric_type}</span>
                    <span class="metric-timestamp">${this.formatTimestamp(metric.timestamp)}</span>
                </div>
                <div class="metric-value">
                    ${this.formatMetricValue(metric)}
                </div>
                ${metric.unit ? `<div class="metric-unit">${this.escapeHtml(metric.unit)}</div>` : ''}
                ${Object.keys(metric.attributes).length > 0 ? `
                    <div class="metric-attributes">
                        ${Object.entries(metric.attributes).map(([key, value]) =>
                            `<span class="attribute-tag">${this.escapeHtml(key)}: ${this.escapeHtml(value)}</span>`
                        ).join('')}
                    </div>
                ` : ''}
            </div>
        `).join('');
    }

    formatMetricValue(metric) {
        const value = metric.value;

        if (typeof value === 'number') {
            return value.toFixed(2);
        }

        if (value.Gauge !== undefined) {
            return value.Gauge.toFixed(2);
        }

        if (value.Counter !== undefined) {
            return value.Counter.toString();
        }

        if (value.Histogram) {
            return `Count: ${value.Histogram.count}, Sum: ${value.Histogram.sum.toFixed(2)}`;
        }

        if (value.Summary) {
            return `Count: ${value.Summary.count}, Sum: ${value.Summary.sum.toFixed(2)}`;
        }

        return 'N/A';
    }

    renderChart(data) {
        const canvas = document.getElementById('metrics-chart');
        const ctx = canvas.getContext('2d');

        // Clear previous chart
        ctx.clearRect(0, 0, canvas.width, canvas.height);

        if (!data.buckets || data.buckets.length === 0) {
            ctx.font = '16px sans-serif';
            ctx.fillStyle = '#666';
            ctx.textAlign = 'center';
            ctx.fillText('No data available', canvas.width / 2, canvas.height / 2);
            return;
        }

        // Set canvas size
        canvas.width = canvas.offsetWidth;
        canvas.height = 300;

        const padding = 40;
        const chartWidth = canvas.width - 2 * padding;
        const chartHeight = canvas.height - 2 * padding;

        // Find min/max values
        const values = data.buckets.map(b => b.value);
        const minValue = Math.min(...values);
        const maxValue = Math.max(...values);
        const valueRange = maxValue - minValue || 1;

        // Draw axes
        ctx.strokeStyle = '#ddd';
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(padding, padding);
        ctx.lineTo(padding, canvas.height - padding);
        ctx.lineTo(canvas.width - padding, canvas.height - padding);
        ctx.stroke();

        // Draw data points and lines
        ctx.strokeStyle = '#4CAF50';
        ctx.fillStyle = '#4CAF50';
        ctx.lineWidth = 2;
        ctx.beginPath();

        data.buckets.forEach((bucket, index) => {
            const x = padding + (index / (data.buckets.length - 1)) * chartWidth;
            const y = canvas.height - padding - ((bucket.value - minValue) / valueRange) * chartHeight;

            if (index === 0) {
                ctx.moveTo(x, y);
            } else {
                ctx.lineTo(x, y);
            }

            // Draw point
            ctx.fillRect(x - 3, y - 3, 6, 6);
        });

        ctx.stroke();

        // Draw labels
        ctx.fillStyle = '#666';
        ctx.font = '12px sans-serif';
        ctx.textAlign = 'center';

        // X-axis labels (timestamps)
        const labelCount = Math.min(5, data.buckets.length);
        for (let i = 0; i < labelCount; i++) {
            const bucketIndex = Math.floor(i * (data.buckets.length - 1) / (labelCount - 1));
            const bucket = data.buckets[bucketIndex];
            const x = padding + (bucketIndex / (data.buckets.length - 1)) * chartWidth;
            const time = new Date(bucket.timestamp / 1000000); // Convert from nanoseconds
            ctx.fillText(time.toLocaleTimeString(), x, canvas.height - padding + 20);
        }

        // Y-axis labels (values)
        ctx.textAlign = 'right';
        for (let i = 0; i <= 4; i++) {
            const value = minValue + (valueRange * i / 4);
            const y = canvas.height - padding - (i / 4) * chartHeight;
            ctx.fillText(value.toFixed(2), padding - 10, y + 4);
        }

        // Chart title
        ctx.fillStyle = '#333';
        ctx.font = 'bold 14px sans-serif';
        ctx.textAlign = 'center';
        ctx.fillText(
            `${this.selectedMetric} (${this.aggregation})`,
            canvas.width / 2,
            20
        );
    }

    attachEventListeners() {
        // Metric selection
        document.getElementById('metric-name-select').addEventListener('change', (e) => {
            this.selectedMetric = e.target.value;
            this.loadAggregatedData();
        });

        // Aggregation selection
        document.getElementById('aggregation-select').addEventListener('change', (e) => {
            this.aggregation = e.target.value;
            this.loadAggregatedData();
        });

        // Interval selection
        document.getElementById('interval-select').addEventListener('change', (e) => {
            this.interval = parseInt(e.target.value);
            this.loadAggregatedData();
        });

        // Refresh button
        document.getElementById('refresh-metrics').addEventListener('click', () => {
            this.loadMetrics();
        });

        // Export button
        document.getElementById('export-metrics').addEventListener('click', () => {
            this.exportMetrics();
        });

        // Auto-refresh toggle
        document.getElementById('auto-refresh-metrics').addEventListener('change', (e) => {
            if (e.target.checked) {
                this.startAutoRefresh();
            } else {
                this.stopAutoRefresh();
            }
        });
    }

    startAutoRefresh() {
        this.stopAutoRefresh();
        this.autoRefreshInterval = setInterval(() => {
            this.loadMetrics();
        }, 30000); // 30 seconds
    }

    stopAutoRefresh() {
        if (this.autoRefreshInterval) {
            clearInterval(this.autoRefreshInterval);
            this.autoRefreshInterval = null;
        }
    }

    async exportMetrics() {
        try {
            const params = new URLSearchParams();
            if (this.selectedMetric) {
                params.append('name', this.selectedMetric);
            }

            const url = `/api/metrics/export?${params.toString()}`;
            window.open(url, '_blank');
        } catch (error) {
            console.error('Failed to export metrics:', error);
            this.showError('Failed to export metrics');
        }
    }

    showError(message) {
        const listContainer = document.getElementById('metrics-list');
        listContainer.innerHTML = `<div class="error-state">${this.escapeHtml(message)}</div>`;
    }

    formatTimestamp(nanos) {
        const date = new Date(nanos / 1000000); // Convert from nanoseconds to milliseconds
        return date.toLocaleString();
    }

    escapeHtml(text) {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }

    destroy() {
        this.stopAutoRefresh();
        if (this.animationFrameId) {
            cancelAnimationFrame(this.animationFrameId);
        }
        if (this.container) {
            this.container.innerHTML = '';
        }
    }
}

// Export for use in app.js
window.MetricsView = MetricsView;

// Made with Bob
