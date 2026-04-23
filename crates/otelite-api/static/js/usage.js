// GenAI token usage view

import { api } from './api.js';

class UsageView {
    constructor(apiClient) {
        this.api = apiClient;
        this.refreshInterval = null;
        this.trStart = null;
        this.trEnd = null;
        this.trWindowHours = null;
    }

    async render() {
        const container = document.getElementById('usage-container');
        if (!container) return;

        container.innerHTML = `
            <div class="view-header">
                <h2>GenAI Usage</h2>
            </div>
            <div class="filters">
                <div class="time-range-bar">
                    <button class="btn-icon" id="tr-prev-usage" title="Previous window">&#8592;</button>
                    <input type="text" id="tr-start-usage" class="filter-input tr-datetime" placeholder="YYYY-MM-DD HH:MM" autocomplete="off">
                    <span class="tr-sep">–</span>
                    <input type="text" id="tr-end-usage" class="filter-input tr-datetime" placeholder="YYYY-MM-DD HH:MM" autocomplete="off">
                    <button class="btn-icon" id="tr-next-usage" title="Next window">&#8594;</button>
                    <button class="btn-icon" id="tr-now-usage" title="Jump to now">Now</button>
                    <select id="tr-preset-usage" class="filter-select tr-preset">
                        <option value="">All time</option>
                        <option value="1">1 hr</option>
                        <option value="6">6 hr</option>
                        <option value="24">24 hr</option>
                        <option value="168">7 days</option>
                    </select>
                </div>
            </div>
            <div id="usage-data-container"></div>
        `;

        this._attachTimeRangeListeners();
        await this._loadAndRender();

        // Start auto-refresh if not already running
        if (!this.refreshInterval) {
            this.refreshInterval = setInterval(() => this._loadAndRender(), 30000);
        }
    }

    _attachTimeRangeListeners() {
        document.getElementById('tr-preset-usage').addEventListener('change', (e) => {
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
            this._loadAndRender();
        });

        document.getElementById('tr-start-usage').addEventListener('change', () => this._onDateInputChange());
        document.getElementById('tr-end-usage').addEventListener('change', () => this._onDateInputChange());

        document.getElementById('tr-prev-usage').addEventListener('click', () => {
            const windowMs = (this.trWindowHours || 1) * 3600000;
            const end = (this.trEnd || new Date()).getTime() - windowMs;
            const start = (this.trStart ? this.trStart.getTime() : end - windowMs) - windowMs;
            this.trEnd = new Date(end);
            this.trStart = new Date(start);
            this._syncDateInputs();
            document.getElementById('tr-preset-usage').value = '';
            this._loadAndRender();
        });

        document.getElementById('tr-next-usage').addEventListener('click', () => {
            const now = Date.now();
            const windowMs = (this.trWindowHours || 1) * 3600000;
            let end = (this.trEnd || new Date()).getTime() + windowMs;
            if (end > now) end = now;
            this.trEnd = new Date(end);
            this.trStart = new Date(end - windowMs);
            this._syncDateInputs();
            document.getElementById('tr-preset-usage').value = '';
            this._loadAndRender();
        });

        document.getElementById('tr-now-usage').addEventListener('click', () => {
            const now = new Date();
            const windowMs = (this.trWindowHours || 1) * 3600000;
            this.trEnd = now;
            this.trStart = new Date(now.getTime() - windowMs);
            this._syncDateInputs();
            document.getElementById('tr-preset-usage').value = '';
            this._loadAndRender();
        });
    }

    _syncDateInputs() {
        const startEl = document.getElementById('tr-start-usage');
        const endEl = document.getElementById('tr-end-usage');
        if (startEl) startEl.value = this.trStart ? this._toDatetimeLocal(this.trStart) : '';
        if (endEl) endEl.value = this.trEnd ? this._toDatetimeLocal(this.trEnd) : '';
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
        const startEl = document.getElementById('tr-start-usage');
        const endEl = document.getElementById('tr-end-usage');
        this.trStart = this._parseDatetimeInput(startEl ? startEl.value : '');
        this.trEnd = this._parseDatetimeInput(endEl ? endEl.value : '');
        if (this.trStart && this.trEnd) {
            this.trWindowHours = (this.trEnd.getTime() - this.trStart.getTime()) / 3600000;
        }
        const presetEl = document.getElementById('tr-preset-usage');
        if (presetEl) presetEl.value = '';
        this._loadAndRender();
    }

    async _loadAndRender() {
        const dataContainer = document.getElementById('usage-data-container');
        if (!dataContainer) return;

        try {
            const params = {};
            if (this.trStart !== null) {
                params.start_time = this.trStart.getTime() * 1_000_000;
                params.end_time = (this.trEnd || new Date()).getTime() * 1_000_000;
            }
            const data = await this.api.getTokenUsage(params);
            dataContainer.innerHTML = this._buildHtml(data);
        } catch (err) {
            dataContainer.innerHTML = `<div class="empty-state"><p>Failed to load usage data</p><p class="empty-state-hint">${err.message}</p></div>`;
        }
    }

    _buildHtml(data) {
        const { summary, by_model, by_system } = data;

        if (summary.total_requests === 0) {
            return `<div class="empty-state">
                <p>No GenAI data yet</p>
                <p class="empty-state-hint">
                    Instrument your LLM application with the OpenAI or Anthropic OTel SDK and point it at
                    <strong>http://localhost:4318</strong>. Token usage will appear here once spans with
                    <code>gen_ai.system</code> attributes arrive.
                </p>
            </div>`;
        }

        const fmt = n => Number(n).toLocaleString();

        const summaryCards = `
            <div class="usage-summary-cards">
                <div class="usage-card">
                    <div class="usage-card-label">Input tokens</div>
                    <div class="usage-card-value">${fmt(summary.total_input_tokens)}</div>
                </div>
                <div class="usage-card">
                    <div class="usage-card-label">Output tokens</div>
                    <div class="usage-card-value">${fmt(summary.total_output_tokens)}</div>
                </div>
                <div class="usage-card">
                    <div class="usage-card-label">Requests</div>
                    <div class="usage-card-value">${fmt(summary.total_requests)}</div>
                </div>
            </div>`;

        const modelRows = by_model.map(m => `
            <tr>
                <td>${this._esc(m.model)}</td>
                <td>${fmt(m.requests)}</td>
                <td>${fmt(m.input_tokens)}</td>
                <td>${fmt(m.output_tokens)}</td>
                <td>${fmt(m.input_tokens + m.output_tokens)}</td>
            </tr>`).join('');

        const modelTable = `
            <h3>By model</h3>
            <table class="data-table">
                <thead><tr>
                    <th>Model</th><th>Requests</th><th>Input tokens</th><th>Output tokens</th><th>Total tokens</th>
                </tr></thead>
                <tbody>${modelRows}</tbody>
            </table>`;

        const systemRows = by_system.map(s => `
            <tr>
                <td>${this._esc(s.system)}</td>
                <td>${fmt(s.requests)}</td>
                <td>${fmt(s.input_tokens + s.output_tokens)}</td>
            </tr>`).join('');

        const systemTable = `
            <h3>By provider</h3>
            <table class="data-table">
                <thead><tr>
                    <th>Provider</th><th>Requests</th><th>Total tokens</th>
                </tr></thead>
                <tbody>${systemRows}</tbody>
            </table>`;

        return summaryCards + modelTable + systemTable;
    }

    _esc(str) {
        return String(str)
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;');
    }
}

window.UsageView = UsageView;
