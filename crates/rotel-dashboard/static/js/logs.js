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
        this.currentPage = 0;
        this.pageSize = 100;
        this.autoRefresh = false;
        this.refreshInterval = null;
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
                    <button id="export-logs" class="btn btn-secondary">Export</button>
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
                <input type="text" id="resource-filter" placeholder="Resource filter (e.g., service.name=my-service)" class="filter-input">
                <button id="apply-filters" class="btn btn-primary">Apply Filters</button>
                <button id="clear-filters" class="btn btn-secondary">Clear</button>
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
    }

    /**
     * Attach event listeners
     */
    attachEventListeners() {
        document.getElementById('refresh-logs').addEventListener('click', () => this.loadLogs());
        document.getElementById('export-logs').addEventListener('click', () => this.exportLogs());
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

            const response = await this.apiClient.getLogs(params);
            this.logs = response.logs;
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

        if (this.logs.length === 0) {
            container.innerHTML = '<div class="empty-state">No logs found</div>';
            return;
        }

        container.innerHTML = this.logs.map(log => this.renderLogEntry(log)).join('');

        // Attach click handlers for expansion
        container.querySelectorAll('.log-entry').forEach((entry, index) => {
            entry.addEventListener('click', () => this.toggleLogExpansion(index));
        });
    }

    /**
     * Render a single log entry
     */
    renderLogEntry(log) {
        const timestamp = new Date(log.timestamp / 1000000); // Convert nanoseconds to milliseconds
        const severityClass = `severity-${log.severity.toLowerCase()}`;

        return `
            <div class="log-entry ${severityClass}" data-timestamp="${log.timestamp}">
                <div class="log-header">
                    <span class="log-timestamp">${timestamp.toISOString()}</span>
                    <span class="log-severity ${severityClass}">${log.severity}</span>
                    <span class="log-body-preview">${this.escapeHtml(log.body.substring(0, 100))}${log.body.length > 100 ? '...' : ''}</span>
                    ${log.trace_id ? `<span class="log-trace-id" title="Trace ID">${log.trace_id.substring(0, 8)}...</span>` : ''}
                </div>
                <div class="log-details" style="display: none;">
                    <div class="log-body">${this.escapeHtml(log.body)}</div>
                    ${log.trace_id ? `<div class="log-field"><strong>Trace ID:</strong> ${log.trace_id}</div>` : ''}
                    ${log.span_id ? `<div class="log-field"><strong>Span ID:</strong> ${log.span_id}</div>` : ''}
                    ${Object.keys(log.attributes).length > 0 ? `
                        <div class="log-field">
                            <strong>Attributes:</strong>
                            ${this.renderAttributeMap(log.attributes)}
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
        document.getElementById('severity-filter').value = '';
        document.getElementById('resource-filter').value = '';
        document.getElementById('search-logs').value = '';
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
     * Export logs
     */
    async exportLogs() {
        try {
            const format = confirm('Export as JSON? (Cancel for CSV)') ? 'json' : 'csv';
            const params = {
                format,
                ...this.filters
            };

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
