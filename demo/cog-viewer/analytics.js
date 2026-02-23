/**
 * Privacy-Respecting Analytics and Monitoring
 *
 * Features:
 * - Performance monitoring
 * - Error tracking
 * - Usage statistics (opt-in)
 * - No personal data collection
 * - No cookies
 * - No tracking across sites
 */

class Analytics {
    constructor() {
        this.enabled = false;
        this.performanceMetrics = [];
        this.errors = [];
        this.events = [];
        this.maxStoredItems = 100;

        // Check if user has opted in
        this.enabled = this.checkOptIn();

        if (this.enabled) {
            this.initializeMonitoring();
        }
    }

    /**
     * Check if analytics is opted in
     */
    checkOptIn() {
        try {
            const optIn = localStorage.getItem('analytics_opt_in');
            return optIn === 'true';
        } catch {
            return false;
        }
    }

    /**
     * Opt in to analytics
     */
    optIn() {
        try {
            localStorage.setItem('analytics_opt_in', 'true');
            this.enabled = true;
            this.initializeMonitoring();
            console.log('Analytics enabled');
        } catch (error) {
            console.error('Failed to enable analytics:', error);
        }
    }

    /**
     * Opt out of analytics
     */
    optOut() {
        try {
            localStorage.removeItem('analytics_opt_in');
            this.enabled = false;
            this.performanceMetrics = [];
            this.errors = [];
            this.events = [];
            console.log('Analytics disabled');
        } catch (error) {
            console.error('Failed to disable analytics:', error);
        }
    }

    /**
     * Initialize monitoring
     */
    initializeMonitoring() {
        // Performance monitoring
        this.monitorPerformance();

        // Error tracking
        this.monitorErrors();

        // Page visibility
        this.monitorVisibility();

        // Network information
        this.monitorNetwork();
    }

    /**
     * Monitor performance metrics
     */
    monitorPerformance() {
        // Web Vitals
        if ('PerformanceObserver' in window) {
            try {
                // Largest Contentful Paint (LCP)
                const lcpObserver = new PerformanceObserver((list) => {
                    const entries = list.getEntries();
                    const lastEntry = entries[entries.length - 1];
                    this.trackMetric('LCP', lastEntry.renderTime || lastEntry.loadTime);
                });
                lcpObserver.observe({ entryTypes: ['largest-contentful-paint'] });

                // First Input Delay (FID)
                const fidObserver = new PerformanceObserver((list) => {
                    const entries = list.getEntries();
                    entries.forEach(entry => {
                        this.trackMetric('FID', entry.processingStart - entry.startTime);
                    });
                });
                fidObserver.observe({ entryTypes: ['first-input'] });

                // Cumulative Layout Shift (CLS)
                let clsValue = 0;
                const clsObserver = new PerformanceObserver((list) => {
                    const entries = list.getEntries();
                    entries.forEach(entry => {
                        if (!entry.hadRecentInput) {
                            clsValue += entry.value;
                        }
                    });
                    this.trackMetric('CLS', clsValue);
                });
                clsObserver.observe({ entryTypes: ['layout-shift'] });
            } catch (error) {
                console.warn('Performance monitoring failed:', error);
            }
        }

        // Page load timing
        window.addEventListener('load', () => {
            setTimeout(() => {
                const perfData = performance.timing;
                const pageLoadTime = perfData.loadEventEnd - perfData.navigationStart;
                const connectTime = perfData.responseEnd - perfData.requestStart;
                const renderTime = perfData.domComplete - perfData.domLoading;

                this.trackMetric('PageLoad', pageLoadTime);
                this.trackMetric('Connect', connectTime);
                this.trackMetric('Render', renderTime);
            }, 0);
        });
    }

    /**
     * Monitor errors
     */
    monitorErrors() {
        // Global error handler
        window.addEventListener('error', (event) => {
            this.trackError({
                type: 'error',
                message: event.message,
                filename: event.filename,
                lineno: event.lineno,
                colno: event.colno,
                timestamp: Date.now(),
            });
        });

        // Unhandled promise rejections
        window.addEventListener('unhandledrejection', (event) => {
            this.trackError({
                type: 'unhandledRejection',
                reason: event.reason?.message || event.reason,
                timestamp: Date.now(),
            });
        });

        // WASM errors
        window.addEventListener('wasmError', (event) => {
            this.trackError({
                type: 'wasm',
                message: event.detail?.message || 'WASM error',
                timestamp: Date.now(),
            });
        });
    }

    /**
     * Monitor page visibility
     */
    monitorVisibility() {
        let hiddenTime = null;

        document.addEventListener('visibilitychange', () => {
            if (document.hidden) {
                hiddenTime = Date.now();
            } else {
                if (hiddenTime) {
                    const timeHidden = Date.now() - hiddenTime;
                    this.trackEvent('PageVisibility', { timeHidden });
                    hiddenTime = null;
                }
            }
        });
    }

    /**
     * Monitor network information
     */
    monitorNetwork() {
        if ('connection' in navigator) {
            const connection = navigator.connection;

            const trackConnection = () => {
                this.trackEvent('NetworkInfo', {
                    effectiveType: connection.effectiveType,
                    downlink: connection.downlink,
                    rtt: connection.rtt,
                    saveData: connection.saveData,
                });
            };

            trackConnection();
            connection.addEventListener('change', trackConnection);
        }
    }

    /**
     * Track custom metric
     */
    trackMetric(name, value) {
        if (!this.enabled) return;

        const metric = {
            name,
            value,
            timestamp: Date.now(),
        };

        this.performanceMetrics.push(metric);

        // Limit stored metrics
        if (this.performanceMetrics.length > this.maxStoredItems) {
            this.performanceMetrics.shift();
        }

        console.log(`[Analytics] Metric: ${name} = ${value}`);
    }

    /**
     * Track error
     */
    trackError(error) {
        if (!this.enabled) return;

        this.errors.push(error);

        // Limit stored errors
        if (this.errors.length > this.maxStoredItems) {
            this.errors.shift();
        }

        console.log('[Analytics] Error tracked:', error);
    }

    /**
     * Track event
     */
    trackEvent(name, data = {}) {
        if (!this.enabled) return;

        const event = {
            name,
            data,
            timestamp: Date.now(),
        };

        this.events.push(event);

        // Limit stored events
        if (this.events.length > this.maxStoredItems) {
            this.events.shift();
        }

        console.log(`[Analytics] Event: ${name}`, data);
    }

    /**
     * Track COG load
     */
    trackCogLoad(url, metadata, loadTime) {
        this.trackEvent('CogLoad', {
            url: this.anonymizeUrl(url),
            width: metadata.width,
            height: metadata.height,
            bandCount: metadata.bandCount,
            loadTime,
        });
    }

    /**
     * Track tile load
     */
    trackTileLoad(coords, loadTime, cached) {
        this.trackEvent('TileLoad', {
            level: coords.level,
            loadTime,
            cached,
        });
    }

    /**
     * Track user interaction
     */
    trackInteraction(action, details = {}) {
        this.trackEvent('UserInteraction', {
            action,
            ...details,
        });
    }

    /**
     * Anonymize URL (remove sensitive parts)
     */
    anonymizeUrl(url) {
        try {
            const urlObj = new URL(url);
            // Return just the hostname
            return urlObj.hostname;
        } catch {
            return 'unknown';
        }
    }

    /**
     * Get performance summary
     */
    getPerformanceSummary() {
        if (!this.enabled) return null;

        const summary = {};

        this.performanceMetrics.forEach(metric => {
            if (!summary[metric.name]) {
                summary[metric.name] = {
                    count: 0,
                    total: 0,
                    min: Infinity,
                    max: -Infinity,
                };
            }

            summary[metric.name].count++;
            summary[metric.name].total += metric.value;
            summary[metric.name].min = Math.min(summary[metric.name].min, metric.value);
            summary[metric.name].max = Math.max(summary[metric.name].max, metric.value);
        });

        // Calculate averages
        Object.keys(summary).forEach(key => {
            summary[key].average = summary[key].total / summary[key].count;
        });

        return summary;
    }

    /**
     * Get error summary
     */
    getErrorSummary() {
        if (!this.enabled) return null;

        const summary = {
            totalErrors: this.errors.length,
            byType: {},
        };

        this.errors.forEach(error => {
            const type = error.type || 'unknown';
            summary.byType[type] = (summary.byType[type] || 0) + 1;
        });

        return summary;
    }

    /**
     * Export analytics data
     */
    exportData() {
        if (!this.enabled) return null;

        return {
            enabled: this.enabled,
            performanceMetrics: this.performanceMetrics,
            performanceSummary: this.getPerformanceSummary(),
            errors: this.errors,
            errorSummary: this.getErrorSummary(),
            events: this.events,
            exportedAt: Date.now(),
        };
    }

    /**
     * Clear all data
     */
    clearData() {
        this.performanceMetrics = [];
        this.errors = [];
        this.events = [];
        console.log('[Analytics] Data cleared');
    }
}

// Create global analytics instance
const analytics = new Analytics();

// Export for use in other modules
export default analytics;
