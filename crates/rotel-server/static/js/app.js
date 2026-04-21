// Main application entry point

import { api } from './api.js';

/**
 * Main application class
 */
class App {
    constructor() {
        this.currentView = 'logs';
        this.connectionCheckInterval = null;
        this.renderedViews = new Set();
        this.views = {};
        this.popoverOpen = false;
        this.lastHealthData = null;
        this.init();
    }

    /**
     * Initialize the application
     */
    init() {
        this.views = {
            logs: new window.LogsView(api),
            traces: new window.TracesView(api),
            metrics: new window.MetricsView(api),
            usage: new window.UsageView(api),
            // setup is static HTML — no view class needed
        };
        this.setupNavigation();
        this.setupConnectionMonitoring();
        this.loadInitialView();
    }

    /**
     * Setup navigation between views
     */
    setupNavigation() {
        const navButtons = document.querySelectorAll('.nav-btn');

        navButtons.forEach(btn => {
            btn.addEventListener('click', () => {
                const view = btn.dataset.view;
                this.switchView(view);
            });
        });
    }

    /**
     * Switch to a different view
     */
    switchView(viewName) {
        // Update navigation buttons
        document.querySelectorAll('.nav-btn').forEach(btn => {
            btn.classList.toggle('active', btn.dataset.view === viewName);
        });

        // Update views
        document.querySelectorAll('.view').forEach(view => {
            view.classList.toggle('active', view.id === `${viewName}-view`);
        });

        this.currentView = viewName;

        // Render the view on first visit; subsequent visits use the view's own auto-refresh
        if (this.views[viewName] && !this.renderedViews.has(viewName)) {
            this.renderedViews.add(viewName);
            this.views[viewName].render();
        }

        // Trigger view-specific initialization
        this.dispatchViewChange(viewName);
    }

    /**
     * Dispatch custom event for view change
     */
    dispatchViewChange(viewName) {
        const event = new CustomEvent('viewchange', { detail: { view: viewName } });
        window.dispatchEvent(event);
    }

    /**
     * Setup connection monitoring
     */
    setupConnectionMonitoring() {
        this.checkConnection();

        // Check connection every 5 seconds
        this.connectionCheckInterval = setInterval(() => {
            this.checkConnection();
        }, 5000);

        // Make the connection status clickable
        const statusWrapper = document.getElementById('status-wrapper');
        if (statusWrapper) {
            statusWrapper.addEventListener('click', (e) => {
                e.stopPropagation();
                this.togglePopover();
            });
        }

        // Close popover when clicking outside
        document.addEventListener('click', () => {
            if (this.popoverOpen) {
                this.closePopover();
            }
        });
    }

    /**
     * Check connection to backend
     */
    async checkConnection() {
        const indicator = document.getElementById('status-indicator');
        const text = document.getElementById('status-text');

        try {
            const health = await api.getHealth();
            this.lastHealthData = health;
            indicator.classList.remove('disconnected');
            text.textContent = 'Connected';
            if (this.popoverOpen) {
                this.refreshPopover();
            }
        } catch (error) {
            this.lastHealthData = null;
            indicator.classList.add('disconnected');
            text.textContent = 'Disconnected';
            console.error('Connection check failed:', error);
            if (this.popoverOpen) {
                this.closePopover();
            }
        }
    }

    /**
     * Format uptime seconds into human-readable string (e.g. "2h 14m")
     */
    formatUptime(seconds) {
        if (seconds < 60) return `${seconds}s`;
        const mins = Math.floor(seconds / 60) % 60;
        const hours = Math.floor(seconds / 3600) % 24;
        const days = Math.floor(seconds / 86400);
        const parts = [];
        if (days > 0) parts.push(`${days}d`);
        if (hours > 0) parts.push(`${hours}h`);
        if (mins > 0) parts.push(`${mins}m`);
        return parts.join(' ') || '0m';
    }

    /**
     * Toggle the status popover
     */
    async togglePopover() {
        if (this.popoverOpen) {
            this.closePopover();
        } else {
            await this.openPopover();
        }
    }

    /**
     * Open the status popover and populate it
     */
    async openPopover() {
        this.popoverOpen = true;
        const popover = document.getElementById('status-popover');
        if (!popover) return;
        popover.classList.add('visible');
        await this.refreshPopover();
    }

    /**
     * Refresh popover content with latest health + stats data
     */
    async refreshPopover() {
        const popover = document.getElementById('status-popover');
        if (!popover) return;

        const health = this.lastHealthData;
        if (!health) {
            popover.innerHTML = '<div class="popover-row popover-error">Server unreachable</div>';
            return;
        }

        let statsHtml = '<div class="popover-row">Loading counts…</div>';
        try {
            const stats = await api.getStats();
            statsHtml = `
                <div class="popover-row"><span class="popover-label">Logs</span><span class="popover-value">${stats.log_count.toLocaleString()}</span></div>
                <div class="popover-row"><span class="popover-label">Traces</span><span class="popover-value">${stats.span_count.toLocaleString()}</span></div>
                <div class="popover-row"><span class="popover-label">Metric points</span><span class="popover-value">${stats.metric_count.toLocaleString()}</span></div>`;
        } catch (_) {
            statsHtml = '<div class="popover-row popover-error">Could not load counts</div>';
        }

        popover.innerHTML = `
            <div class="popover-row"><span class="popover-label">Version</span><span class="popover-value">${health.version}</span></div>
            <div class="popover-row"><span class="popover-label">Uptime</span><span class="popover-value">${this.formatUptime(health.uptime_seconds)}</span></div>
            <div class="popover-divider"></div>
            ${statsHtml}
            <div class="popover-divider"></div>
            <div class="popover-row"><span class="popover-label">gRPC</span><span class="popover-value">:4317</span></div>
            <div class="popover-row"><span class="popover-label">HTTP</span><span class="popover-value">:4318</span></div>
            <div class="popover-divider"></div>
            <div class="popover-row popover-action-row"><button class="popover-danger-btn" onclick="app.clearAllData()">Clear all data</button></div>`;
    }

    /**
     * Close the status popover
     */
    closePopover() {
        this.popoverOpen = false;
        const popover = document.getElementById('status-popover');
        if (popover) {
            popover.classList.remove('visible');
        }
    }

    /**
     * Delete all telemetry data after user confirmation
     */
    async clearAllData() {
        if (!confirm('Delete all telemetry data? This cannot be undone.')) {
            return;
        }
        try {
            await fetch('/api/admin/purge', { method: 'POST' });
            await this.refreshPopover();
        } catch (_) {
            // refreshPopover will show the error state if the server is unreachable
        }
    }

    /**
     * Load initial view
     */
    loadInitialView() {
        this.switchView(this.currentView);
    }

    /**
     * Show loading overlay
     */
    showLoading() {
        document.getElementById('loading-overlay').classList.remove('hidden');
    }

    /**
     * Hide loading overlay
     */
    hideLoading() {
        document.getElementById('loading-overlay').classList.add('hidden');
    }
}

// Initialize app when DOM is ready
if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => {
        window.app = new App();
    });
} else {
    window.app = new App();
}

// Export for use in other modules
export { App };

// Made with Bob
