//! SLO objective definitions and management.

use super::{ErrorBudget, Slo, TimeWindow};
use chrono::Duration;

/// Availability SLO builder.
pub struct AvailabilitySlo;

impl AvailabilitySlo {
    /// Create a 99.9% availability SLO over 30 days.
    pub fn three_nines() -> Slo {
        Slo {
            name: "availability_99.9".to_string(),
            description: "99.9% availability over 30 days".to_string(),
            target: 99.9,
            sli_query: "sum(rate(http_requests_total{status!~\"5..\"}[5m])) / sum(rate(http_requests_total[5m]))".to_string(),
            time_window: TimeWindow::Rolling(Duration::days(30)),
            error_budget: ErrorBudget::from_target(99.9),
        }
    }

    /// Create a 99.99% availability SLO over 30 days.
    pub fn four_nines() -> Slo {
        Slo {
            name: "availability_99.99".to_string(),
            description: "99.99% availability over 30 days".to_string(),
            target: 99.99,
            sli_query: "sum(rate(http_requests_total{status!~\"5..\"}[5m])) / sum(rate(http_requests_total[5m]))".to_string(),
            time_window: TimeWindow::Rolling(Duration::days(30)),
            error_budget: ErrorBudget::from_target(99.99),
        }
    }
}

/// Latency SLO builder.
pub struct LatencySlo;

impl LatencySlo {
    /// Create a latency SLO for 95th percentile under 100ms.
    pub fn p95_100ms() -> Slo {
        Slo {
            name: "latency_p95_100ms".to_string(),
            description: "95% of requests complete within 100ms".to_string(),
            target: 95.0,
            sli_query:
                "histogram_quantile(0.95, rate(http_request_duration_seconds_bucket[5m])) < 0.1"
                    .to_string(),
            time_window: TimeWindow::Rolling(Duration::days(7)),
            error_budget: ErrorBudget::from_target(95.0),
        }
    }

    /// Create a latency SLO for 99th percentile under 500ms.
    pub fn p99_500ms() -> Slo {
        Slo {
            name: "latency_p99_500ms".to_string(),
            description: "99% of requests complete within 500ms".to_string(),
            target: 99.0,
            sli_query:
                "histogram_quantile(0.99, rate(http_request_duration_seconds_bucket[5m])) < 0.5"
                    .to_string(),
            time_window: TimeWindow::Rolling(Duration::days(7)),
            error_budget: ErrorBudget::from_target(99.0),
        }
    }
}

/// Throughput SLO builder.
pub struct ThroughputSlo;

impl ThroughputSlo {
    /// Create a throughput SLO for minimum RPS.
    pub fn min_rps(rps: f64) -> Slo {
        Slo {
            name: format!("throughput_min_{}rps", rps),
            description: format!("Minimum {} requests per second", rps),
            target: 99.0,
            sli_query: format!("rate(http_requests_total[1m]) >= {}", rps),
            time_window: TimeWindow::Rolling(Duration::hours(24)),
            error_budget: ErrorBudget::from_target(99.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_availability_slos() {
        let slo = AvailabilitySlo::three_nines();
        assert_eq!(slo.target, 99.9);

        let slo = AvailabilitySlo::four_nines();
        assert_eq!(slo.target, 99.99);
    }

    #[test]
    fn test_latency_slos() {
        let slo = LatencySlo::p95_100ms();
        assert_eq!(slo.target, 95.0);

        let slo = LatencySlo::p99_500ms();
        assert_eq!(slo.target, 99.0);
    }
}
