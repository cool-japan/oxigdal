//! OxiGDAL Benchmarking and Profiling Suite
//!
//! `oxigdal-bench` provides comprehensive performance profiling and benchmarking
//! capabilities for the OxiGDAL geospatial library ecosystem.
//!
//! # Features
//!
//! - **CPU Profiling**: Profile CPU usage with flamegraph generation
//! - **Memory Profiling**: Track memory usage and detect leaks
//! - **Benchmark Scenarios**: Predefined scenarios for common operations
//! - **Performance Comparison**: Compare performance across implementations
//! - **Regression Detection**: Detect performance regressions against baselines
//! - **Report Generation**: Generate HTML, JSON, CSV, and Markdown reports
//!
//! # Example
//!
//! ```rust,no_run
//! use oxigdal_bench::profiler::{CpuProfiler, CpuProfilerConfig};
//! use oxigdal_bench::scenarios::{ScenarioBuilder, ScenarioRunner};
//! use oxigdal_bench::report::{BenchmarkReport, ReportFormat};
//!
//! // Create a custom benchmark scenario
//! let scenario = ScenarioBuilder::new("my_benchmark")
//!     .description("Test raster processing")
//!     .execute(|| {
//!         // Your benchmark code here
//!         Ok(())
//!     })
//!     .build();
//!
//! // Run the scenario
//! let mut runner = ScenarioRunner::new();
//! runner.add_scenario(scenario);
//! runner.run_all().expect("Failed to run benchmarks");
//!
//! // Generate a report
//! let mut report = BenchmarkReport::new("My Benchmark Report");
//! report.add_scenario_results(runner.results().to_vec());
//! report.generate("report.html", ReportFormat::Html)
//!     .expect("Failed to generate report");
//! ```
//!
//! # Scenario Modules
//!
//! - `scenarios::raster`: Raster operation benchmarks (requires `raster` feature)
//! - `scenarios::vector`: Vector operation benchmarks (requires `vector` feature)
//! - [`scenarios::io`]: I/O performance benchmarks
//! - `scenarios::cloud`: Cloud storage benchmarks (requires `cloud` feature)
//! - `scenarios::ml`: ML inference benchmarks (requires `ml` feature)
//!
//! # Profiling
//!
//! The [`profiler`] module provides CPU and memory profiling utilities:
//!
//! ```rust,no_run
//! use oxigdal_bench::profiler::{profile_cpu, CpuProfilerConfig};
//!
//! let config = CpuProfilerConfig {
//!     frequency: 100,
//!     generate_flamegraph: true,
//!     ..Default::default()
//! };
//!
//! let (result, report) = profile_cpu(|| {
//!     // Code to profile
//!     42
//! }, config).expect("Profiling failed");
//!
//! println!("Profiling duration: {:?}", report.duration);
//! ```

#![deny(clippy::unwrap_used)]
#![deny(clippy::panic)]
#![warn(missing_docs)]
#![warn(clippy::expect_used)]
#![allow(unexpected_cfgs)]

// Re-export commonly used types
pub use error::{BenchError, Result};

// Core modules
pub mod comparison;
pub mod error;
pub mod profiler;
pub mod regression;
pub mod report;
pub mod scenarios;

// Version information
/// The version of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The name of this crate.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

// Prelude module for convenient imports
pub mod prelude {
    //! Prelude module with commonly used types and traits.

    pub use crate::comparison::{Comparison, ComparisonSuite, Implementation};
    pub use crate::error::{BenchError, Result};
    pub use crate::profiler::{
        CpuProfiler, CpuProfilerConfig, MemoryProfiler, MemoryProfilerConfig, SystemMonitor,
        profile_cpu, profile_memory,
    };
    pub use crate::regression::{
        Baseline, BaselineStore, RegressionConfig, RegressionDetector, RegressionReport,
        RegressionResult, RegressionSeverity,
    };
    pub use crate::report::{BenchmarkReport, ReportBuilder, ReportFormat};
    pub use crate::scenarios::{
        BenchmarkScenario, CustomScenario, ScenarioBuilder, ScenarioResult, ScenarioRunner,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
        assert_eq!(CRATE_NAME, "oxigdal-bench");
    }
}
