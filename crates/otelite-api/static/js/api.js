// API client for Otelite dashboard

const API_BASE = '/api';

/**
 * API client class
 */
class ApiClient {
    constructor() {
        this.baseUrl = API_BASE;
    }

    /**
     * Make a GET request
     */
    async get(endpoint, params = {}) {
        const url = new URL(`${this.baseUrl}${endpoint}`, window.location.origin);

        // Add query parameters
        Object.keys(params).forEach(key => {
            if (params[key] !== null && params[key] !== undefined) {
                url.searchParams.append(key, params[key]);
            }
        });

        try {
            const response = await fetch(url);

            if (!response.ok) {
                throw new Error(`HTTP ${response.status}: ${response.statusText}`);
            }

            return await response.json();
        } catch (error) {
            console.error(`API GET ${endpoint} failed:`, error);
            throw error;
        }
    }

    /**
     * Fetch logs with optional filters
     */
    async getLogs(filters = {}) {
        return this.get('/logs', filters);
    }

    /**
     * Fetch a single log by ID
     */
    async getLog(id) {
        return this.get(`/logs/${id}`);
    }

    /**
     * Export logs — returns a Blob for the caller to download
     */
    async exportLogs(params = {}) {
        const url = new URL(`${this.baseUrl}/logs/export`, window.location.origin);
        Object.keys(params).forEach(key => {
            if (params[key] !== null && params[key] !== undefined) {
                url.searchParams.append(key, params[key]);
            }
        });
        const response = await fetch(url);
        if (!response.ok) {
            throw new Error(`HTTP ${response.status}: ${response.statusText}`);
        }
        return response.blob();
    }

    /**
     * Fetch traces with optional filters
     */
    async getTraces(filters = {}) {
        return this.get('/traces', filters);
    }

    /**
     * Fetch a single trace by ID
     */
    async getTrace(traceId) {
        return this.get(`/traces/${traceId}`);
    }

    /**
     * Export traces — returns a Blob for the caller to download
     */
    async exportTraces(params = {}) {
        const url = new URL(`${this.baseUrl}/traces/export`, window.location.origin);
        Object.keys(params).forEach(key => {
            if (params[key] !== null && params[key] !== undefined) {
                url.searchParams.append(key, params[key]);
            }
        });
        const response = await fetch(url);
        if (!response.ok) {
            throw new Error(`HTTP ${response.status}: ${response.statusText}`);
        }
        return response.blob();
    }

    /**
     * Fetch metrics with optional filters
     */
    async getMetrics(filters = {}) {
        return this.get('/metrics', filters);
    }

    /**
     * Fetch list of metric names
     */
    async getMetricNames() {
        return this.get('/metrics/names');
    }

    /**
     * Fetch aggregated metrics
     */
    async getAggregatedMetrics(params = {}) {
        return this.get('/metrics/aggregate', params);
    }

    /**
     * Fetch time-series data for a specific metric
     */
    async getMetricTimeseries(name, params = {}) {
        return this.get(`/metrics/${encodeURIComponent(name)}/timeseries`, params);
    }

    /**
     * Export metrics — returns a Blob for the caller to download
     */
    async exportMetrics(params = {}) {
        const url = new URL(`${this.baseUrl}/metrics/export`, window.location.origin);
        Object.keys(params).forEach(key => {
            if (params[key] !== null && params[key] !== undefined) {
                url.searchParams.append(key, params[key]);
            }
        });
        const response = await fetch(url);
        if (!response.ok) {
            throw new Error(`HTTP ${response.status}: ${response.statusText}`);
        }
        return response.blob();
    }

    /**
     * Fetch distinct resource attribute keys for a signal type.
     * signal: "logs", "spans", or "metrics"
     */
    async getResourceKeys(signal) {
        return this.get('/resource-keys', { signal });
    }

    /**
     * Fetch GenAI token usage statistics
     */
    async getTokenUsage(params = {}) {
        return this.get('/genai/usage', params);
    }

    /**
     * Check health status
     */
    async getHealth() {
        return this.get('/health');
    }

    /**
     * Fetch storage statistics
     */
    async getStats() {
        return this.get('/stats');
    }
}

// Export singleton instance
export const api = new ApiClient();
