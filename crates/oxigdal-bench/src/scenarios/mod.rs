//! Benchmark scenarios for various oxigdal operations.
//!
//! This module provides a framework for defining and executing benchmark scenarios
//! across different components of oxigdal.

use crate::error::{BenchError, Result};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[cfg(feature = "raster")]
pub mod raster;

#[cfg(feature = "vector")]
pub mod vector;

pub mod io;

#[cfg(feature = "cloud")]
pub mod cloud;

#[cfg(feature = "ml")]
pub mod ml;

/// Benchmark scenario trait.
///
/// All benchmark scenarios must implement this trait to be executable
/// within the benchmarking framework.
pub trait BenchmarkScenario {
    /// Returns the name of the scenario.
    fn name(&self) -> &str;

    /// Returns a description of the scenario.
    fn description(&self) -> &str;

    /// Sets up the scenario (e.g., creating test data).
    fn setup(&mut self) -> Result<()>;

    /// Executes the benchmark scenario.
    fn execute(&mut self) -> Result<()>;

    /// Cleans up after the scenario (e.g., removing test data).
    fn teardown(&mut self) -> Result<()>;

    /// Validates the results of the benchmark.
    fn validate(&self) -> Result<()> {
        Ok(())
    }

    /// Returns the expected duration range for this scenario.
    /// Useful for regression detection.
    fn expected_duration_range(&self) -> Option<(Duration, Duration)> {
        None
    }
}

/// Benchmark scenario result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScenarioResult {
    /// Scenario name.
    pub name: String,
    /// Setup duration.
    #[serde(with = "crate::profiler::duration_serde")]
    pub setup_duration: Duration,
    /// Execution duration.
    #[serde(with = "crate::profiler::duration_serde")]
    pub execution_duration: Duration,
    /// Teardown duration.
    #[serde(with = "crate::profiler::duration_serde")]
    pub teardown_duration: Duration,
    /// Total duration.
    #[serde(with = "crate::profiler::duration_serde")]
    pub total_duration: Duration,
    /// Whether the scenario succeeded.
    pub success: bool,
    /// Error message if failed.
    pub error_message: Option<String>,
    /// Additional metrics.
    pub metrics: HashMap<String, f64>,
}

/// Benchmark scenario runner.
pub struct ScenarioRunner {
    scenarios: Vec<Box<dyn BenchmarkScenario>>,
    results: Vec<ScenarioResult>,
}

impl ScenarioRunner {
    /// Creates a new scenario runner.
    pub fn new() -> Self {
        Self {
            scenarios: Vec::new(),
            results: Vec::new(),
        }
    }

    /// Adds a scenario to the runner.
    pub fn add_scenario<S: BenchmarkScenario + 'static>(&mut self, scenario: S) {
        self.scenarios.push(Box::new(scenario));
    }

    /// Runs all scenarios.
    pub fn run_all(&mut self) -> Result<()> {
        self.results.clear();

        let mut results = Vec::new();
        for scenario in &mut self.scenarios {
            let result = Self::run_scenario_static(scenario.as_mut());
            results.push(result);
        }

        self.results = results;

        Ok(())
    }

    /// Runs a single scenario.
    fn run_scenario_static(scenario: &mut dyn BenchmarkScenario) -> ScenarioResult {
        let name = scenario.name().to_string();
        let metrics = HashMap::new();

        // Setup
        let setup_start = Instant::now();
        let setup_result = scenario.setup();
        let setup_duration = setup_start.elapsed();

        if let Err(e) = setup_result {
            return ScenarioResult {
                name,
                setup_duration,
                execution_duration: Duration::ZERO,
                teardown_duration: Duration::ZERO,
                total_duration: setup_duration,
                success: false,
                error_message: Some(format!("Setup failed: {e}")),
                metrics,
            };
        }

        // Execute
        let execute_start = Instant::now();
        let execute_result = scenario.execute();
        let execution_duration = execute_start.elapsed();

        let success = execute_result.is_ok();
        let error_message = execute_result.err().map(|e| e.to_string());

        // Validate
        if success {
            if let Err(e) = scenario.validate() {
                return ScenarioResult {
                    name,
                    setup_duration,
                    execution_duration,
                    teardown_duration: Duration::ZERO,
                    total_duration: setup_duration + execution_duration,
                    success: false,
                    error_message: Some(format!("Validation failed: {e}")),
                    metrics,
                };
            }
        }

        // Teardown
        let teardown_start = Instant::now();
        let _ = scenario.teardown(); // Best effort cleanup
        let teardown_duration = teardown_start.elapsed();

        let total_duration = setup_duration + execution_duration + teardown_duration;

        ScenarioResult {
            name,
            setup_duration,
            execution_duration,
            teardown_duration,
            total_duration,
            success,
            error_message,
            metrics,
        }
    }

    /// Gets all scenario results.
    pub fn results(&self) -> &[ScenarioResult] {
        &self.results
    }

    /// Gets successful results only.
    pub fn successful_results(&self) -> Vec<&ScenarioResult> {
        self.results.iter().filter(|r| r.success).collect()
    }

    /// Gets failed results only.
    pub fn failed_results(&self) -> Vec<&ScenarioResult> {
        self.results.iter().filter(|r| !r.success).collect()
    }
}

impl Default for ScenarioRunner {
    fn default() -> Self {
        Self::new()
    }
}

/// Benchmark scenario builder.
pub struct ScenarioBuilder {
    name: String,
    description: String,
    setup_fn: Option<Box<dyn FnMut() -> Result<()>>>,
    execute_fn: Option<Box<dyn FnMut() -> Result<()>>>,
    teardown_fn: Option<Box<dyn FnMut() -> Result<()>>>,
    validate_fn: Option<Box<dyn Fn() -> Result<()>>>,
}

impl ScenarioBuilder {
    /// Creates a new scenario builder.
    pub fn new<S: Into<String>>(name: S) -> Self {
        let name = name.into();
        Self {
            name: name.clone(),
            description: name,
            setup_fn: None,
            execute_fn: None,
            teardown_fn: None,
            validate_fn: None,
        }
    }

    /// Sets the description.
    pub fn description<S: Into<String>>(mut self, description: S) -> Self {
        self.description = description.into();
        self
    }

    /// Sets the setup function.
    pub fn setup<F>(mut self, f: F) -> Self
    where
        F: FnMut() -> Result<()> + 'static,
    {
        self.setup_fn = Some(Box::new(f));
        self
    }

    /// Sets the execute function.
    pub fn execute<F>(mut self, f: F) -> Self
    where
        F: FnMut() -> Result<()> + 'static,
    {
        self.execute_fn = Some(Box::new(f));
        self
    }

    /// Sets the teardown function.
    pub fn teardown<F>(mut self, f: F) -> Self
    where
        F: FnMut() -> Result<()> + 'static,
    {
        self.teardown_fn = Some(Box::new(f));
        self
    }

    /// Sets the validate function.
    pub fn validate<F>(mut self, f: F) -> Self
    where
        F: Fn() -> Result<()> + 'static,
    {
        self.validate_fn = Some(Box::new(f));
        self
    }

    /// Builds the scenario.
    pub fn build(self) -> CustomScenario {
        CustomScenario {
            name: self.name,
            description: self.description,
            setup_fn: self.setup_fn,
            execute_fn: self.execute_fn,
            teardown_fn: self.teardown_fn,
            validate_fn: self.validate_fn,
        }
    }
}

/// Custom benchmark scenario created via builder.
pub struct CustomScenario {
    name: String,
    description: String,
    setup_fn: Option<Box<dyn FnMut() -> Result<()>>>,
    execute_fn: Option<Box<dyn FnMut() -> Result<()>>>,
    teardown_fn: Option<Box<dyn FnMut() -> Result<()>>>,
    validate_fn: Option<Box<dyn Fn() -> Result<()>>>,
}

impl BenchmarkScenario for CustomScenario {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn setup(&mut self) -> Result<()> {
        if let Some(ref mut f) = self.setup_fn {
            f()
        } else {
            Ok(())
        }
    }

    fn execute(&mut self) -> Result<()> {
        if let Some(ref mut f) = self.execute_fn {
            f()
        } else {
            Err(BenchError::benchmark_execution(
                "No execute function defined",
            ))
        }
    }

    fn teardown(&mut self) -> Result<()> {
        if let Some(ref mut f) = self.teardown_fn {
            f()
        } else {
            Ok(())
        }
    }

    fn validate(&self) -> Result<()> {
        if let Some(ref f) = self.validate_fn {
            f()
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scenario_builder() {
        let scenario = ScenarioBuilder::new("test")
            .description("Test scenario")
            .execute(|| Ok(()))
            .build();

        assert_eq!(scenario.name(), "test");
        assert_eq!(scenario.description(), "Test scenario");
    }

    #[test]
    fn test_scenario_runner() {
        let mut runner = ScenarioRunner::new();

        let scenario = ScenarioBuilder::new("test_success")
            .execute(|| Ok(()))
            .build();

        runner.add_scenario(scenario);

        let result = runner.run_all();
        assert!(result.is_ok());

        let results = runner.results();
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
    }

    #[test]
    fn test_failed_scenario() {
        let mut runner = ScenarioRunner::new();

        let scenario = ScenarioBuilder::new("test_failure")
            .execute(|| Err(BenchError::benchmark_execution("Intentional failure")))
            .build();

        runner.add_scenario(scenario);

        let result = runner.run_all();
        assert!(result.is_ok());

        let results = runner.results();
        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert!(results[0].error_message.is_some());
    }
}
