// GenAI token usage view

import { api } from './api.js';

class UsageView {
    constructor(apiClient) {
        this.api = apiClient;
        this.refreshInterval = null;
    }

    async render() {
        const container = document.getElementById('usage-container');
        if (!container) return;

        try {
            const data = await this.api.getTokenUsage();
            container.innerHTML = this._buildHtml(data);
        } catch (err) {
            container.innerHTML = `<div class="empty-state"><p>Failed to load usage data</p><p class="empty-state-hint">${err.message}</p></div>`;
        }

        // Start auto-refresh if not already running
        if (!this.refreshInterval) {
            this.refreshInterval = setInterval(() => this.render(), 30000);
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

// Made with Bob
