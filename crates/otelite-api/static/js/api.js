// API client for Otelite dashboard

const API_BASE = '/api';

/**
 * API client class
 */
class ApiClient {
    constructor() {
        this.baseUrl = API_BASE;
        // Global filter bar state (#135): { agent, model, provider, project, session }.
        // Injected into every genai/sessions list call; endpoints echo back
        // the subset they applied in `filters_applied`.
        this.globalFilters = {};
        // Last `filters_applied` seen on a genai/sessions response (union
        // bookkeeping lives in the view).
        this.lastFiltersApplied = null;
    }

    /**
     * Endpoints whose array payload is wrapped in { items, filters_applied } (#135)
     */
    static WRAPPED_ENDPOINTS = new Set([
        '/genai/cost_series', '/genai/top_spans', '/genai/top_sessions',
        '/genai/top_conversations', '/genai/finish_reasons', '/genai/latency_stats',
        '/genai/error_rate', '/genai/tool_usage', '/genai/truncation_rate',
        '/genai/latency_series', '/genai/calls_series', '/genai/latency_by_context',
        '/genai/error_types', '/genai/model_drift', '/genai/stop_reasons',
        '/genai/context_type_split', '/genai/tool_errors', '/genai/hour_of_day',
        '/genai/agent_framework_defs', '/genai/cache_hit_rate',
    ]);

    /**
     * Make a GET request
     */
    async get(endpoint, params = {}) {
        // Merge the global filter bar into genai + sessions endpoints;
        // every one of them accepts the five params (unsupported ones are
        // ignored server-side, never a 400).
        let merged = params;
        const isFiltered = endpoint.startsWith('/genai/') ||
            endpoint === '/sessions' || endpoint === '/sessions/costs';
        if (isFiltered) {
            merged = { ...this.globalFilters, ...params };
        }

        const url = new URL(`${this.baseUrl}${endpoint}`, window.location.origin);

        // Add query parameters
        Object.keys(merged).forEach(key => {
            if (merged[key] !== null && merged[key] !== undefined) {
                url.searchParams.append(key, merged[key]);
            }
        });

        try {
            const response = await fetch(url);

            if (!response.ok) {
                let detail = response.statusText;
                try {
                    const body = await response.json();
                    detail = body.error || body.message || response.statusText;
                } catch { /* body not JSON, keep statusText */ }
                throw new Error(`HTTP ${response.status}: ${detail}`);
            }

            const body = await response.json();

            // #135: wrapped array endpoints carry { items, filters_applied }
            this.lastFiltersApplied =
                (body && Array.isArray(body.filters_applied)) ? body.filters_applied : null;
            if (ApiClient.WRAPPED_ENDPOINTS.has(endpoint) &&
                body && Array.isArray(body.items)) {
                return body.items;
            }
            return body;
        } catch (error) {
            console.error('API GET failed:', endpoint, error);
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
     * Fetch GenAI cost time-series buckets
     */
    async getCostSeries(params = {}) {
        return this.get('/genai/cost_series', params);
    }

    /**
     * Fetch top-N most expensive GenAI spans
     */
    async getTopSpans(params = {}) {
        return this.get('/genai/top_spans', params);
    }

    /**
     * Fetch GenAI finish-reason distribution
     */
    async getFinishReasons(params = {}) {
        return this.get('/genai/finish_reasons', params);
    }

    async getLatencyStats(params = {}) { return this.get('/genai/latency_stats', params); }
    async getLatencyPercentiles(params = {}) { return this.get('/genai/latency_percentiles', { bucket_secs: 3600, metrics: 'duration,ttft', ...params }); }
    async getDistribution(params = {}) { return this.get('/genai/distributions', { buckets: 20, ...params }); }
    async getErrorRate(params = {}) { return this.get('/genai/error_rate', params); }
    async getToolUsage(params = {}) { return this.get('/genai/tool_usage', params); }
    async getRetryStats(params = {}) { return this.get('/genai/retry_stats', params); }
    async getRetrievalStats(params = {}) { return this.get('/genai/retrieval_stats', params); }
    async getPricingMetadata() { return this.get('/genai/pricing_metadata'); }
    async getAgentFrameworkDefs() { return this.get('/genai/agent_framework_defs'); }
    async getTopSessions(params = {}) { return this.get('/genai/top_sessions', params); }
    async getTopConversations(params = {}) { return this.get('/genai/top_conversations', params); }
    async getTruncationRate(params = {}) { return this.get('/genai/truncation_rate', params); }
    async getCacheHitRate(params = {}) { return this.get('/genai/cache_hit_rate', params); }
    async getCacheEconomics(params = {}) {
        return this.get('/genai/cache_hit_rate', { by_model: 1, ...params });
    }
    async getReasoningShare(params = {}) { return this.get('/genai/reasoning_share', params); }
    async getAgents(params = {}) { return this.get('/genai/agents', params); }
    async getProjects(params = {}) { return this.get('/genai/projects', params); }
    async getSessionCosts(params = {}) { return this.get('/sessions/costs', params); }
    async getSessionCostDistribution(params = {}) { return this.get('/sessions/cost-distribution', params); }
    async getRequestParamProfile(params = {}) { return this.get('/genai/request_param_profile', params); }
    async getConversationDepth(params = {}) { return this.get('/genai/conversation_depth', params); }
    async getCallsSeries(params = {}) { return this.get('/genai/calls_series', { bucket_secs: 3600, ...params }); }
    async getErrorTypes(params = {})  { return this.get('/genai/error_types', params); }
    async getModelDrift(params = {})  { return this.get('/genai/model_drift', params); }
    async getLatencyByContext(params = {}) { return this.get('/genai/latency_by_context', params); }
    async getLatencySeries(params = {}) { return this.get('/genai/latency_series', { bucket_secs: 3600, ...params }); }
    async getGenAiCapabilities(params = {}) { return this.get('/genai/capabilities', params); }
    async getToolApprovals(params = {}) { return this.get('/genai/tool_approvals', params); }
    async getStopReasons(params = {})   { return this.get('/genai/stop_reasons', params); }
    async getContextTypeSplit(params = {}) { return this.get('/genai/context_type_split', params); }
    async getToolErrors(params = {})    { return this.get('/genai/tool_errors', params); }
    async getHourOfDay(params = {})     { return this.get('/genai/hour_of_day', params); }
    async getAgentRoles(params = {})    { return this.get('/genai/agent_roles', params); }
    async getProviderMix(params = {})   { return this.get('/genai/provider_mix', params); }

    /**
     * Fetch session diagnose report
     */
    async getSessionContext(sessionId, params = {}) { return this.get(`/sessions/${sessionId}/context`, { limit: 500, ...params }); }
    async getSessionDiagnose(sessionId) {
        return this.get(`/sessions/${encodeURIComponent(sessionId)}/diagnose`);
    }

    /**
     * Fetch a list of recent sessions with summary stats.
     * Params: { start_time, end_time, limit }
     */
    async getSessions(params = {}) {
        return this.get('/sessions', params);
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
