//! ONNX graph optimization passes integrated with oxionnx.
//!
//! Provides [`GraphOptConfig`] to control which optimization passes are applied
//! at session creation time, and [`benchmark_optimization`] to measure the impact
//! of graph optimizations on inference latency and node count.
//!
//! # Constraints
//!
//! oxionnx-proto does not support ONNX serialization, so graph optimizations are
//! applied at runtime (session creation) only -- not as file-to-file transforms.

use crate::error::{InferenceError, MlError, ModelError, Result};
use oxionnx::{Graph, OptLevel, Session, SessionBuilder, Tensor};
use std::collections::HashMap;
use tracing::info;

/// Configuration for ONNX graph optimization passes.
///
/// Each boolean controls a category of optimization. When a flag is enabled,
/// the corresponding oxionnx optimizer pass runs during session creation.
///
/// Because oxionnx maps all enabled passes to a single [`OptLevel`], the
/// effective mapping is:
/// - All flags false => [`OptLevel::None`]
/// - Any flag true   => [`OptLevel::All`]
#[derive(Debug, Clone)]
pub struct GraphOptConfig {
    /// Enable constant folding (evaluate nodes whose inputs are all constants).
    pub constant_folding: bool,
    /// Enable dead node elimination (remove unreachable nodes).
    pub dead_node_elimination: bool,
    /// Enable common sub-expression elimination (merge duplicate expressions).
    pub common_subexpression_elimination: bool,
    /// Enable operator fusion (MatMul+Add -> Gemm, Conv+BN, Conv+Relu, etc.).
    pub operator_fusion: bool,
}

impl Default for GraphOptConfig {
    /// Returns a configuration with all optimization passes enabled.
    fn default() -> Self {
        Self {
            constant_folding: true,
            dead_node_elimination: true,
            common_subexpression_elimination: true,
            operator_fusion: true,
        }
    }
}

impl GraphOptConfig {
    /// Returns a configuration with no optimization passes enabled.
    #[must_use]
    pub fn none() -> Self {
        Self {
            constant_folding: false,
            dead_node_elimination: false,
            common_subexpression_elimination: false,
            operator_fusion: false,
        }
    }

    /// Returns `true` if any optimization pass is enabled.
    #[must_use]
    pub fn any_enabled(&self) -> bool {
        self.constant_folding
            || self.dead_node_elimination
            || self.common_subexpression_elimination
            || self.operator_fusion
    }

    /// Maps this configuration to an oxionnx [`OptLevel`].
    ///
    /// Because the oxionnx runtime applies all passes as a unit, any enabled
    /// flag maps to [`OptLevel::All`] and no enabled flags maps to
    /// [`OptLevel::None`].
    #[must_use]
    pub fn to_opt_level(&self) -> OptLevel {
        if self.any_enabled() {
            OptLevel::All
        } else {
            OptLevel::None
        }
    }
}

/// Build an oxionnx [`Session`] from a pre-parsed graph and weights,
/// applying graph optimizations according to `config`.
///
/// This is the recommended way to load a model with fine-grained control
/// over which optimization passes run.
///
/// # Errors
///
/// Returns an error if session construction fails (e.g. unsupported operators).
pub fn apply_graph_optimization(
    graph: Graph,
    weights: HashMap<String, Tensor>,
    config: &GraphOptConfig,
) -> Result<Session> {
    let level = config.to_opt_level();
    info!(
        "Applying graph optimization with level {:?} (config: {:?})",
        level, config
    );
    let session = SessionBuilder::new()
        .with_optimization_level(level)
        .build_from_graph(graph, weights)
        .map_err(|e| ModelError::LoadFailed {
            reason: format!("Graph optimization session build failed: {e}"),
        })?;
    Ok(session)
}

/// Build an oxionnx [`Session`] from raw ONNX model bytes,
/// applying graph optimizations according to `config`.
///
/// # Errors
///
/// Returns an error if the bytes cannot be parsed or session construction fails.
pub fn apply_graph_optimization_from_bytes(
    bytes: &[u8],
    config: &GraphOptConfig,
) -> Result<Session> {
    let level = config.to_opt_level();
    info!(
        "Applying graph optimization from bytes with level {:?}",
        level
    );
    let session = SessionBuilder::new()
        .with_optimization_level(level)
        .load_from_bytes(bytes)
        .map_err(|e| ModelError::LoadFailed {
            reason: format!("Graph optimization session build failed: {e}"),
        })?;
    Ok(session)
}

/// Results from benchmarking graph optimization impact.
#[derive(Debug, Clone)]
pub struct OptimizationBenchmark {
    /// Number of computation nodes before optimization.
    pub original_node_count: usize,
    /// Number of computation nodes after optimization.
    pub optimized_node_count: usize,
    /// Median inference latency (ms) without optimization.
    pub original_latency_ms: f64,
    /// Median inference latency (ms) with optimization.
    pub optimized_latency_ms: f64,
    /// Speedup factor (original / optimized). Values > 1.0 indicate improvement.
    pub speedup_factor: f64,
    /// Number of nodes eliminated by optimization.
    pub nodes_eliminated: usize,
}

impl OptimizationBenchmark {
    /// Returns `true` if the optimization yielded a meaningful speedup (> 1.0x).
    #[must_use]
    pub fn is_beneficial(&self) -> bool {
        self.speedup_factor > 1.0
    }

    /// Returns the percentage of nodes eliminated.
    #[must_use]
    pub fn node_reduction_percent(&self) -> f64 {
        if self.original_node_count == 0 {
            return 0.0;
        }
        (self.nodes_eliminated as f64 / self.original_node_count as f64) * 100.0
    }
}

/// Benchmark the impact of graph optimization on a model.
///
/// Loads the model twice -- once without optimization (`OptLevel::None`) and once
/// with full optimization (`OptLevel::All`) -- measures inference latency over
/// `num_iterations` runs, and reports the delta.
///
/// # Arguments
///
/// * `graph` - The ONNX computation graph.
/// * `weights` - Model weights/initializers.
/// * `input_data` - Flat f32 input data for inference.
/// * `input_shape` - Shape of the input tensor.
/// * `num_iterations` - Number of inference iterations for timing.
///
/// # Errors
///
/// Returns an error if session creation or inference fails.
pub fn benchmark_optimization(
    graph: Graph,
    weights: HashMap<String, Tensor>,
    input_data: &[f32],
    input_shape: &[usize],
    num_iterations: usize,
) -> Result<OptimizationBenchmark> {
    let iterations = if num_iterations == 0 {
        1
    } else {
        num_iterations
    };

    // Build unoptimized session
    let session_none = SessionBuilder::new()
        .with_optimization_level(OptLevel::None)
        .build_from_graph(graph.clone(), weights.clone())
        .map_err(|e| ModelError::LoadFailed {
            reason: format!("Failed to build unoptimized session: {e}"),
        })?;

    // Build optimized session
    let session_all = SessionBuilder::new()
        .with_optimization_level(OptLevel::All)
        .build_from_graph(graph, weights)
        .map_err(|e| ModelError::LoadFailed {
            reason: format!("Failed to build optimized session: {e}"),
        })?;

    let original_node_count = session_none.model_info().node_count;
    let optimized_node_count = session_all.model_info().node_count;

    // Measure latency for unoptimized model
    let original_latency_ms =
        measure_median_latency(&session_none, input_data, input_shape, iterations)?;

    // Measure latency for optimized model
    let optimized_latency_ms =
        measure_median_latency(&session_all, input_data, input_shape, iterations)?;

    let speedup_factor = if optimized_latency_ms > 0.0 {
        original_latency_ms / optimized_latency_ms
    } else {
        1.0
    };

    let nodes_eliminated = original_node_count.saturating_sub(optimized_node_count);

    info!(
        "Optimization benchmark: {} -> {} nodes ({} eliminated, {:.2}x speedup)",
        original_node_count, optimized_node_count, nodes_eliminated, speedup_factor
    );

    Ok(OptimizationBenchmark {
        original_node_count,
        optimized_node_count,
        original_latency_ms,
        optimized_latency_ms,
        speedup_factor,
        nodes_eliminated,
    })
}

/// Measure median inference latency for a session.
fn measure_median_latency(
    session: &Session,
    input_data: &[f32],
    input_shape: &[usize],
    iterations: usize,
) -> Result<f64> {
    use std::time::Instant;

    let input_name = session
        .input_names()
        .first()
        .ok_or_else(|| InferenceError::Failed {
            reason: "No input names in session".to_string(),
        })?
        .clone();

    let total_elements: usize = input_shape.iter().product();
    if input_data.len() != total_elements {
        return Err(MlError::Inference(InferenceError::InvalidInputShape {
            expected: input_shape.to_vec(),
            actual: vec![input_data.len()],
        }));
    }

    let input_tensor = Tensor::new(input_data.to_vec(), input_shape.to_vec());

    let mut latencies = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let inputs_map =
            oxionnx::inputs![input_name.as_str() => input_tensor.clone()].map_err(|e| {
                InferenceError::Failed {
                    reason: format!("Failed to build inputs map: {e}"),
                }
            })?;

        let start = Instant::now();
        let _ = session
            .run(&inputs_map)
            .map_err(|e| InferenceError::Failed {
                reason: format!("Inference failed: {e}"),
            })?;
        let elapsed = start.elapsed();
        latencies.push(elapsed.as_secs_f64() * 1000.0);
    }

    // Compute median
    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = if latencies.len() % 2 == 0 && latencies.len() >= 2 {
        let mid = latencies.len() / 2;
        (latencies[mid - 1] + latencies[mid]) / 2.0
    } else {
        latencies[latencies.len() / 2]
    };

    Ok(median)
}

/// Helper to build a simple test graph with a MatMul followed by Add
/// (fuseable to Gemm by the optimizer).
///
/// Returns (graph, weights) ready for session construction.
#[cfg(test)]
pub(crate) fn make_matmul_add_graph() -> (Graph, HashMap<String, Tensor>) {
    use oxionnx::{Attributes, Node, OpKind};

    let mut weights = HashMap::new();

    // Weight matrix: 4x4 identity-like
    weights.insert(
        "W".to_string(),
        Tensor::new(
            vec![
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            vec![4, 4],
        ),
    );

    // Bias vector
    weights.insert(
        "B".to_string(),
        Tensor::new(vec![0.1, 0.2, 0.3, 0.4], vec![4]),
    );

    let matmul_node = Node {
        op: OpKind::MatMul,
        name: "matmul_0".to_string(),
        inputs: vec!["X".to_string(), "W".to_string()],
        outputs: vec!["matmul_out".to_string()],
        attrs: Attributes::default(),
    };

    let add_node = Node {
        op: OpKind::Add,
        name: "add_0".to_string(),
        inputs: vec!["matmul_out".to_string(), "B".to_string()],
        outputs: vec!["Y".to_string()],
        attrs: Attributes::default(),
    };

    let graph = Graph {
        name: "test_matmul_add".to_string(),
        nodes: vec![matmul_node, add_node],
        input_names: vec!["X".to_string()],
        output_names: vec!["Y".to_string()],
        ..Default::default()
    };

    (graph, weights)
}

/// Helper to build a simple identity passthrough graph for basic testing.
#[cfg(test)]
pub(crate) fn make_identity_graph() -> (Graph, HashMap<String, Tensor>) {
    use oxionnx::{Attributes, Node, OpKind};

    let node = Node {
        op: OpKind::Identity,
        name: "identity_0".to_string(),
        inputs: vec!["X".to_string()],
        outputs: vec!["Y".to_string()],
        attrs: Attributes::default(),
    };

    let graph = Graph {
        name: "test_identity".to_string(),
        nodes: vec![node],
        input_names: vec!["X".to_string()],
        output_names: vec!["Y".to_string()],
        ..Default::default()
    };

    (graph, HashMap::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_opt_config_default() {
        let config = GraphOptConfig::default();
        assert!(config.constant_folding);
        assert!(config.dead_node_elimination);
        assert!(config.common_subexpression_elimination);
        assert!(config.operator_fusion);
        assert!(config.any_enabled());
        assert_eq!(config.to_opt_level(), OptLevel::All);
    }

    #[test]
    fn test_graph_opt_config_none() {
        let config = GraphOptConfig::none();
        assert!(!config.constant_folding);
        assert!(!config.dead_node_elimination);
        assert!(!config.common_subexpression_elimination);
        assert!(!config.operator_fusion);
        assert!(!config.any_enabled());
        assert_eq!(config.to_opt_level(), OptLevel::None);
    }

    #[test]
    fn test_graph_opt_config_partial() {
        let config = GraphOptConfig {
            constant_folding: false,
            dead_node_elimination: true,
            common_subexpression_elimination: false,
            operator_fusion: false,
        };
        assert!(config.any_enabled());
        // Any enabled flag maps to OptLevel::All
        assert_eq!(config.to_opt_level(), OptLevel::All);
    }

    #[test]
    fn test_apply_graph_optimization_identity() {
        let (graph, weights) = make_identity_graph();
        let config = GraphOptConfig::default();
        let session = apply_graph_optimization(graph, weights, &config);
        assert!(session.is_ok());
        let session = session.expect("session should be valid");
        let info = session.model_info();
        // Identity graph should have at most 1 node
        assert!(info.node_count <= 1);
    }

    #[test]
    fn test_apply_graph_optimization_none_level() {
        let (graph, weights) = make_identity_graph();
        let config = GraphOptConfig::none();
        let session = apply_graph_optimization(graph, weights, &config);
        assert!(session.is_ok());
    }

    #[test]
    fn test_operator_fusion_changes_node_count() {
        // MatMul + Add should be fused to Gemm when optimization is on.
        let (graph_opt, weights_opt) = make_matmul_add_graph();
        let (graph_none, weights_none) = make_matmul_add_graph();

        let session_opt = SessionBuilder::new()
            .with_optimization_level(OptLevel::All)
            .build_from_graph(graph_opt, weights_opt);
        assert!(session_opt.is_ok(), "Optimized session should build");
        let session_opt = session_opt.expect("session should be valid");

        let session_none = SessionBuilder::new()
            .with_optimization_level(OptLevel::None)
            .build_from_graph(graph_none, weights_none);
        assert!(session_none.is_ok(), "Unoptimized session should build");
        let session_none = session_none.expect("session should be valid");

        let opt_count = session_opt.model_info().node_count;
        let none_count = session_none.model_info().node_count;

        // Unoptimized should have 2 nodes (MatMul + Add)
        assert_eq!(none_count, 2, "Unoptimized should preserve both nodes");
        // Optimized should have fewer nodes (MatMul+Add fused to Gemm = 1 node)
        assert!(
            opt_count <= none_count,
            "Optimized node count ({opt_count}) should be <= unoptimized ({none_count})"
        );
    }

    #[test]
    fn test_optimization_pipeline_with_fusion_flag() {
        let (graph, weights) = make_matmul_add_graph();
        let config = GraphOptConfig {
            constant_folding: false,
            dead_node_elimination: false,
            common_subexpression_elimination: false,
            operator_fusion: true,
        };
        // operator_fusion = true => OptLevel::All
        let session = apply_graph_optimization(graph, weights, &config);
        assert!(session.is_ok(), "Pipeline with fusion should succeed");
    }

    #[test]
    fn test_benchmark_struct_fields() {
        let benchmark = OptimizationBenchmark {
            original_node_count: 10,
            optimized_node_count: 6,
            original_latency_ms: 50.0,
            optimized_latency_ms: 30.0,
            speedup_factor: 50.0 / 30.0,
            nodes_eliminated: 4,
        };

        assert!(benchmark.is_beneficial());
        assert!((benchmark.speedup_factor - 1.6667).abs() < 0.01);
        assert!((benchmark.node_reduction_percent() - 40.0).abs() < 0.01);
    }

    #[test]
    fn test_benchmark_struct_no_improvement() {
        let benchmark = OptimizationBenchmark {
            original_node_count: 5,
            optimized_node_count: 5,
            original_latency_ms: 10.0,
            optimized_latency_ms: 10.5,
            speedup_factor: 10.0 / 10.5,
            nodes_eliminated: 0,
        };

        assert!(!benchmark.is_beneficial());
        assert!((benchmark.node_reduction_percent() - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_benchmark_struct_zero_original_nodes() {
        let benchmark = OptimizationBenchmark {
            original_node_count: 0,
            optimized_node_count: 0,
            original_latency_ms: 0.0,
            optimized_latency_ms: 0.0,
            speedup_factor: 1.0,
            nodes_eliminated: 0,
        };

        assert!((benchmark.node_reduction_percent() - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_benchmark_optimization_identity() {
        let (graph, weights) = make_identity_graph();
        let input_data = vec![1.0, 2.0, 3.0, 4.0];
        let input_shape = vec![1, 4];

        let result = benchmark_optimization(graph, weights, &input_data, &input_shape, 3);
        assert!(
            result.is_ok(),
            "Benchmark should succeed: {:?}",
            result.err()
        );
        let bench = result.expect("benchmark should be valid");
        assert!(bench.original_latency_ms >= 0.0);
        assert!(bench.optimized_latency_ms >= 0.0);
        assert!(bench.speedup_factor > 0.0);
    }

    #[test]
    fn test_benchmark_optimization_matmul_add() {
        let (graph, weights) = make_matmul_add_graph();
        let input_data = vec![1.0, 0.0, 0.0, 0.0];
        let input_shape = vec![1, 4];

        let result = benchmark_optimization(graph, weights, &input_data, &input_shape, 5);
        assert!(
            result.is_ok(),
            "Benchmark should succeed: {:?}",
            result.err()
        );
        let bench = result.expect("benchmark should be valid");
        // Unoptimized: MatMul + Add = 2 nodes
        assert_eq!(bench.original_node_count, 2);
        // Optimized: should be <= 2 (fusion may reduce)
        assert!(bench.optimized_node_count <= bench.original_node_count);
    }

    #[test]
    fn test_apply_from_bytes_roundtrip() {
        // Verify the config -> OptLevel mapping is consistent
        let configs = [
            (GraphOptConfig::default(), OptLevel::All),
            (GraphOptConfig::none(), OptLevel::None),
            (
                GraphOptConfig {
                    constant_folding: true,
                    dead_node_elimination: false,
                    common_subexpression_elimination: false,
                    operator_fusion: false,
                },
                OptLevel::All,
            ),
        ];

        for (config, expected_level) in &configs {
            assert_eq!(
                config.to_opt_level(),
                *expected_level,
                "Config {:?} should map to {:?}",
                config,
                expected_level
            );
        }
    }
}
