// Main application entry point

import { api } from './api.js';

/**
 * Main application class
 */
class App {
    constructor() {
        this.currentView = 'logs';
        this.connectionCheckInterval = null;
        this.init();
    }

    /**
     * Initialize the application
     */
    init() {
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
    }

    /**
     * Check connection to backend
     */
    async checkConnection() {
        const indicator = document.getElementById('status-indicator');
        const text = document.getElementById('status-text');

        try {
            await api.getHealth();
            indicator.classList.remove('disconnected');
            text.textContent = 'Connected';
        } catch (error) {
            indicator.classList.add('disconnected');
            text.textContent = 'Disconnected';
            console.error('Connection check failed:', error);
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
