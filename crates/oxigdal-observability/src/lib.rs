//! OpenTelemetry-based observability, monitoring, and alerting for OxiGDAL.
//!
//! This crate provides comprehensive observability capabilities for the OxiGDAL
//! geospatial data processing library, including:
//!
//! - **OpenTelemetry Integration**: Distributed tracing, metrics, and logs
//! - **Custom Geospatial Metrics**: Raster, vector, I/O, cache, query, GPU, and cluster metrics
//! - **Distributed Tracing**: W3C Trace Context propagation across services
//! - **Real-time Dashboards**: Grafana and Prometheus dashboard templates
//! - **Anomaly Detection**: Statistical and ML-based anomaly detection
//! - **SLO/SLA Monitoring**: Service level objectives and error budget tracking
//! - **Alert Management**: Rule-based alerting with routing and escalation
//! - **Metric Exporters**: Prometheus, StatsD, InfluxDB, CloudWatch support
//!
//! # Example
//!
//! ```no_run
//! use oxigdal_observability::telemetry::{TelemetryConfig, init_with_config};
//!
//! # tokio_test::block_on(async {
//! // Initialize telemetry
//! let config = TelemetryConfig::new("oxigdal")
//!     .with_service_version("1.0.0")
//!     .with_jaeger_endpoint("localhost:6831")
//!     .with_sampling_rate(0.1);
//!
//! let provider = init_with_config(config).await.expect("Failed to initialize telemetry");
//! # })
//! ```

#![warn(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::panic)]

pub mod alerting;
pub mod anomaly;
pub mod config;
pub mod correlation;
pub mod dashboard;
pub mod dashboards;
pub mod error;
pub mod exporters;
pub mod health;
pub mod integration;
pub mod metrics;
pub mod profiling;
pub mod reporting;
pub mod slo;
pub mod telemetry;
pub mod tracing;

// Re-export commonly used types
pub use error::{ObservabilityError, Result};
pub use health::{HealthCheckManager, HealthStatus};
pub use profiling::{ProfileStats, Profiler};
pub use telemetry::{TelemetryConfig, TelemetryProvider};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_config() {
        let config = TelemetryConfig::new("test-service")
            .with_service_version("1.0.0")
            .with_sampling_rate(0.5);

        assert_eq!(config.service_name, "test-service");
        assert_eq!(config.service_version, "1.0.0");
        assert_eq!(config.sampling_rate, 0.5);
    }
}
