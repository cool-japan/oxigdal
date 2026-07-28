//! DirectML operator-graph modeling and **simulation** layer.
//!
//! # Scope and honesty
//!
//! This module is a **pure-Rust simulation** of a DirectML-style operator graph.
//! It lets callers *model* an ML operator graph (nodes, edges, tensor
//! descriptors, memory budgeting) and run real, host-side graph transformations
//! such as operator fusion. It does **not** perform any real
//! DirectML / Direct3D 12 work: a genuine DirectML backend requires COM / D3D12
//! FFI bindings (C/C++), which fall outside OxiGeo's Pure-Rust default scope and
//! would have to live behind a dedicated, non-default C-FFI feature.
//!
//! Consequently the numeric *execution* entry points here return an honest
//! [`GpuError::UnsupportedOperation`] rather than silently pretending work was
//! done. The graph-building, fusion, tensor-layout, and memory-accounting APIs
//! are fully functional pure-Rust logic and can be used to prepare/validate a
//! graph before handing it to a real backend elsewhere.
//!
//! This whole module is gated behind the non-default `directml` cargo feature.

use crate::error::{GpuError, GpuResult};
use std::collections::HashMap;
use tracing::{debug, info};

/// DirectML device configuration.
#[derive(Debug, Clone)]
pub struct DirectMLConfig {
    /// Enable DirectML acceleration.
    pub enabled: bool,
    /// Device index to use.
    pub device_index: u32,
    /// Enable graph optimization.
    pub optimize_graph: bool,
    /// Enable operator fusion.
    pub enable_fusion: bool,
}

impl Default for DirectMLConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            device_index: 0,
            optimize_graph: true,
            enable_fusion: true,
        }
    }
}

/// DirectML operator-graph model (simulation, not a real DirectML device).
pub struct DirectMLDevice {
    config: DirectMLConfig,
    operators: HashMap<String, DirectMLOperator>,
}

impl DirectMLDevice {
    /// Create a new DirectML **simulation** context.
    ///
    /// This never talks to a real DirectML runtime; it always succeeds so the
    /// pure-Rust graph-modeling APIs are usable on any platform.
    ///
    /// # Errors
    ///
    /// Currently infallible, but returns [`GpuResult`] for forward
    /// compatibility with a future real backend.
    pub fn new(config: DirectMLConfig) -> GpuResult<Self> {
        info!(
            "Initializing DirectML simulation context (device index {})",
            config.device_index
        );

        Ok(Self {
            config,
            operators: HashMap::new(),
        })
    }

    /// The configuration this simulation context was created with.
    pub fn config(&self) -> &DirectMLConfig {
        &self.config
    }

    /// Whether the DirectML **simulation** layer is available.
    ///
    /// This always returns `true` — it reports availability of the pure-Rust
    /// graph-modeling layer, **not** the presence of a real DirectML/D3D12
    /// runtime. Use [`is_hardware_accelerated`](Self::is_hardware_accelerated)
    /// to check for genuine hardware acceleration (always `false` here).
    pub fn is_available() -> bool {
        true
    }

    /// Whether a real, hardware-accelerated DirectML backend is in use.
    ///
    /// Always `false`: this module is a pure-Rust simulation. A real backend
    /// would require D3D12/COM FFI behind a dedicated non-default feature.
    pub fn is_hardware_accelerated() -> bool {
        false
    }

    /// Register (model) an operator in the graph.
    pub fn create_operator(&mut self, name: String, op_type: DirectMLOperatorType) -> u32 {
        let id = self.operators.len() as u32;

        self.operators.insert(
            name.clone(),
            DirectMLOperator {
                id,
                name: name.clone(),
                op_type,
            },
        );

        debug!("Registered DirectML operator '{}' ({:?})", name, op_type);

        id
    }

    /// Attempt to execute a modeled operator.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::InvalidKernelParams`] if the operator name is not
    /// registered, and otherwise [`GpuError::UnsupportedOperation`]: numeric
    /// execution requires a real DirectML/D3D12 runtime (C FFI), which is
    /// outside this pure-Rust module's scope. This is an honest typed error, not
    /// a silent no-op that pretends work was performed.
    pub fn execute_operator(&self, name: &str) -> GpuResult<()> {
        let operator = self.operators.get(name).ok_or_else(|| {
            GpuError::invalid_kernel_params(format!("operator '{name}' not found"))
        })?;

        Err(GpuError::unsupported_operation(format!(
            "DirectML numeric execution of operator '{}' ({:?}) requires a real \
             DirectML/D3D12 runtime (C FFI), which is outside OxiGeo's Pure-Rust \
             scope; this module only models and optimizes the operator graph",
            operator.name, operator.op_type
        )))
    }
}

#[derive(Debug, Clone)]
struct DirectMLOperator {
    id: u32,
    name: String,
    op_type: DirectMLOperatorType,
}

/// DirectML operator types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectMLOperatorType {
    /// Convolution operator.
    Convolution,
    /// Matrix multiplication.
    Gemm,
    /// Activation function.
    Activation,
    /// Pooling operator.
    Pooling,
    /// Normalization operator.
    Normalization,
    /// Element-wise operator.
    ElementWise,
    /// Reduction operator.
    Reduction,
}

/// DirectML tensor descriptor.
#[derive(Debug, Clone)]
pub struct TensorDescriptor {
    /// Tensor data type.
    pub data_type: TensorDataType,
    /// Tensor dimensions.
    pub dimensions: Vec<u32>,
    /// Tensor strides.
    pub strides: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorDataType {
    /// 32-bit float.
    Float32,
    /// 16-bit float.
    Float16,
    /// 32-bit integer.
    Int32,
    /// 8-bit unsigned integer.
    UInt8,
}

impl TensorDescriptor {
    /// Create a new tensor descriptor.
    pub fn new(data_type: TensorDataType, dimensions: Vec<u32>) -> Self {
        let strides = Self::calculate_strides(&dimensions);

        Self {
            data_type,
            dimensions,
            strides,
        }
    }

    /// Get total element count.
    pub fn element_count(&self) -> u64 {
        self.dimensions.iter().map(|&d| d as u64).product()
    }

    /// Get tensor size in bytes.
    pub fn size_bytes(&self) -> u64 {
        self.element_count() * self.data_type.size_bytes() as u64
    }

    fn calculate_strides(dimensions: &[u32]) -> Vec<u32> {
        let mut strides = vec![1; dimensions.len()];

        for i in (0..dimensions.len().saturating_sub(1)).rev() {
            strides[i] = strides[i + 1] * dimensions[i + 1];
        }

        strides
    }
}

impl TensorDataType {
    /// Get size of data type in bytes.
    pub fn size_bytes(&self) -> usize {
        match self {
            Self::Float32 => 4,
            Self::Float16 => 2,
            Self::Int32 => 4,
            Self::UInt8 => 1,
        }
    }
}

/// DirectML operator graph builder.
pub struct OperatorGraphBuilder {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    next_node_id: u32,
}

#[derive(Debug, Clone)]
struct GraphNode {
    id: u32,
    operator: DirectMLOperatorType,
    inputs: Vec<u32>,
    outputs: Vec<u32>,
}

#[derive(Debug, Clone)]
struct GraphEdge {
    src_node: u32,
    dst_node: u32,
    tensor_id: u32,
}

impl OperatorGraphBuilder {
    /// Create a new graph builder.
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            next_node_id: 0,
        }
    }

    /// Add a node to the graph.
    pub fn add_node(&mut self, operator: DirectMLOperatorType) -> u32 {
        let id = self.next_node_id;
        self.next_node_id += 1;

        self.nodes.push(GraphNode {
            id,
            operator,
            inputs: Vec::new(),
            outputs: Vec::new(),
        });

        debug!("Added graph node {} ({:?})", id, operator);

        id
    }

    /// Connect two nodes.
    ///
    /// # Errors
    ///
    /// Returns an error if nodes not found.
    pub fn connect(&mut self, src: u32, dst: u32, tensor_id: u32) -> GpuResult<()> {
        // Verify nodes exist
        if !self.nodes.iter().any(|n| n.id == src) {
            return Err(GpuError::internal("Source node not found"));
        }

        if !self.nodes.iter().any(|n| n.id == dst) {
            return Err(GpuError::internal("Destination node not found"));
        }

        self.edges.push(GraphEdge {
            src_node: src,
            dst_node: dst,
            tensor_id,
        });

        debug!("Connected node {} -> {} (tensor {})", src, dst, tensor_id);

        Ok(())
    }

    /// Build the execution graph.
    pub fn build(self) -> OperatorGraph {
        OperatorGraph {
            nodes: self.nodes,
            edges: self.edges,
        }
    }

    /// Get number of nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get number of edges.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

impl Default for OperatorGraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Compiled operator graph.
#[derive(Debug, Clone)]
pub struct OperatorGraph {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

impl OperatorGraph {
    /// Attempt to execute the graph.
    ///
    /// # Errors
    ///
    /// Always returns [`GpuError::UnsupportedOperation`]: numeric graph
    /// execution requires a real DirectML/D3D12 runtime (C FFI) that is outside
    /// this pure-Rust module's scope. This is an honest typed error rather than
    /// a no-op that falsely reports success.
    pub fn execute(&self) -> GpuResult<()> {
        Err(GpuError::unsupported_operation(format!(
            "DirectML numeric execution of a {}-node graph requires a real \
             DirectML/D3D12 runtime (C FFI), which is outside OxiGeo's Pure-Rust \
             scope; use the graph-modeling and fusion APIs to prepare a graph for \
             a real backend instead",
            self.nodes.len()
        )))
    }

    /// Optimize the graph in place by fusing compatible adjacent operators.
    ///
    /// Returns the number of fusions applied. This is a real pure-Rust graph
    /// transformation (see [`OperatorFusionOptimizer::fuse`]).
    pub fn optimize(&mut self) -> usize {
        OperatorFusionOptimizer::fuse(self)
    }

    /// Number of nodes currently in the graph.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of edges currently in the graph.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

/// DirectML memory allocator.
pub struct DirectMLMemoryAllocator {
    total_memory: u64,
    allocated: u64,
    allocations: HashMap<u32, MemoryAllocation>,
    next_id: u32,
}

#[derive(Debug, Clone)]
struct MemoryAllocation {
    id: u32,
    size: u64,
    alignment: u64,
}

impl DirectMLMemoryAllocator {
    /// Create a new memory allocator.
    pub fn new(total_memory: u64) -> Self {
        Self {
            total_memory,
            allocated: 0,
            allocations: HashMap::new(),
            next_id: 0,
        }
    }

    /// Allocate memory.
    ///
    /// # Errors
    ///
    /// Returns an error if allocation exceeds available memory.
    pub fn allocate(&mut self, size: u64, alignment: u64) -> GpuResult<u32> {
        let aligned_size = Self::align(size, alignment);

        if self.allocated + aligned_size > self.total_memory {
            return Err(GpuError::out_of_memory(
                aligned_size,
                self.total_memory - self.allocated,
            ));
        }

        let id = self.next_id;
        self.next_id += 1;

        self.allocations.insert(
            id,
            MemoryAllocation {
                id,
                size: aligned_size,
                alignment,
            },
        );

        self.allocated += aligned_size;

        debug!(
            "Allocated {} bytes (aligned to {})",
            aligned_size, alignment
        );

        Ok(id)
    }

    /// Free memory.
    ///
    /// # Errors
    ///
    /// Returns an error if allocation not found.
    pub fn free(&mut self, id: u32) -> GpuResult<()> {
        let alloc = self
            .allocations
            .remove(&id)
            .ok_or_else(|| GpuError::invalid_buffer("Allocation not found"))?;

        self.allocated = self.allocated.saturating_sub(alloc.size);

        debug!("Freed {} bytes", alloc.size);

        Ok(())
    }

    /// Get memory statistics.
    pub fn stats(&self) -> (u64, u64, u64) {
        (
            self.allocated,
            self.total_memory,
            self.total_memory - self.allocated,
        )
    }

    fn align(size: u64, alignment: u64) -> u64 {
        ((size + alignment - 1) / alignment) * alignment
    }
}

/// DirectML execution engine.
pub struct DirectMLExecutionEngine {
    device: DirectMLDevice,
    graph: Option<OperatorGraph>,
    memory: DirectMLMemoryAllocator,
}

impl DirectMLExecutionEngine {
    /// Create a new execution engine.
    ///
    /// # Errors
    ///
    /// Returns an error if DirectML initialization fails.
    pub fn new(config: DirectMLConfig) -> GpuResult<Self> {
        let device = DirectMLDevice::new(config)?;
        let memory = DirectMLMemoryAllocator::new(4 * 1024 * 1024 * 1024); // 4 GB

        Ok(Self {
            device,
            graph: None,
            memory,
        })
    }

    /// Set the operator graph.
    pub fn set_graph(&mut self, graph: OperatorGraph) {
        self.graph = Some(graph);
    }

    /// Execute the current graph.
    ///
    /// # Errors
    ///
    /// Returns an error if no graph is set or execution fails.
    pub fn execute(&self) -> GpuResult<()> {
        let graph = self
            .graph
            .as_ref()
            .ok_or_else(|| GpuError::internal("No graph set"))?;

        graph.execute()
    }

    /// Get memory statistics.
    pub fn memory_stats(&self) -> (u64, u64, u64) {
        self.memory.stats()
    }
}

/// DirectML operator fusion optimizer.
pub struct OperatorFusionOptimizer;

impl OperatorFusionOptimizer {
    /// Fuse compatible adjacent operators in a graph, in place.
    ///
    /// Repeatedly finds a producer→consumer edge whose operator pair is fusible
    /// ([`can_fuse`](Self::can_fuse)) and whose consumer has this producer as
    /// its only input, then merges the consumer into the producer: the fusing
    /// edge is dropped, the consumer's outgoing edges are re-sourced from the
    /// producer, and the consumer node is removed. Returns the number of
    /// fusions applied. Terminates because every fusion removes exactly one
    /// node.
    pub fn fuse(graph: &mut OperatorGraph) -> usize {
        let mut fusions = 0usize;

        loop {
            // Locate a fusible edge whose destination has a single producer.
            let candidate = graph.edges.iter().enumerate().find_map(|(idx, edge)| {
                let src = graph.nodes.iter().find(|n| n.id == edge.src_node)?;
                let dst = graph.nodes.iter().find(|n| n.id == edge.dst_node)?;
                if !Self::can_fuse(src.operator, dst.operator) {
                    return None;
                }
                let incoming = graph.edges.iter().filter(|e| e.dst_node == dst.id).count();
                if incoming != 1 {
                    return None;
                }
                Some((idx, edge.src_node, edge.dst_node))
            });

            let (edge_idx, src_id, dst_id) = match candidate {
                Some(c) => c,
                None => break,
            };

            // Drop the fusing edge and re-source the consumer's outputs.
            graph.edges.remove(edge_idx);
            for edge in graph.edges.iter_mut() {
                if edge.src_node == dst_id {
                    edge.src_node = src_id;
                }
            }
            // Guard against any accidental self-loops.
            graph.edges.retain(|e| e.src_node != e.dst_node);
            // Remove the fused-away consumer node.
            graph.nodes.retain(|n| n.id != dst_id);

            fusions += 1;
        }

        debug!("Fused {} operator pair(s) in graph", fusions);
        fusions
    }

    /// Check if two operators can be fused.
    pub fn can_fuse(op1: DirectMLOperatorType, op2: DirectMLOperatorType) -> bool {
        matches!(
            (op1, op2),
            (
                DirectMLOperatorType::Convolution,
                DirectMLOperatorType::Activation
            ) | (DirectMLOperatorType::Gemm, DirectMLOperatorType::Activation)
                | (
                    DirectMLOperatorType::ElementWise,
                    DirectMLOperatorType::ElementWise
                )
        )
    }
}

/// Portable single-lane fallback definitions for wave/subgroup intrinsics.
pub struct WaveOperations;

impl WaveOperations {
    /// Generate WGSL for **portable single-lane** wave-intrinsic fallbacks.
    ///
    /// # Honesty
    ///
    /// WGSL has no portable cross-lane (subgroup/wave) operations without the
    /// optional subgroups hardware feature, and DirectML's HLSL wave intrinsics
    /// are not available here. The functions below are therefore the
    /// mathematically-correct **single-lane** (wave size = 1) specializations:
    /// with one active lane, the lane index is `0`, all lanes trivially agree,
    /// and the *exclusive* prefix sum has no predecessors and is `0`. They are
    /// correct fallbacks — not a pretend cross-lane reduction. A real
    /// cross-lane implementation requires `wgpu::Features::SUBGROUP` and
    /// subgroup builtins, which this simulation module does not emit.
    pub fn wave_intrinsics_shader() -> &'static str {
        r#"
// Portable single-lane (wave size = 1) fallback definitions.
// These are correct for one active lane; real cross-lane behavior needs
// hardware subgroup intrinsics (wgpu Features::SUBGROUP), not emitted here.

fn wave_get_lane_count() -> u32 {
    // Single-lane fallback: one active lane.
    return 1u;
}

fn wave_get_lane_index() -> u32 {
    // With a single lane, the only lane index is 0.
    return 0u;
}

fn wave_active_all_equal(value: f32) -> bool {
    // One lane trivially agrees with itself.
    return true;
}

fn wave_active_any(condition: bool) -> bool {
    // With one lane, "any" equals that lane's condition.
    return condition;
}

fn wave_active_all(condition: bool) -> bool {
    // With one lane, "all" equals that lane's condition.
    return condition;
}

fn wave_prefix_sum(value: f32) -> f32 {
    // Exclusive prefix sum: no preceding lanes, so the result is 0.
    return 0.0;
}

fn wave_read_lane_at(value: f32, lane_index: u32) -> f32 {
    // Only lane 0 exists; reading it yields this lane's own value.
    return value;
}
"#
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_directml_config() {
        let config = DirectMLConfig::default();
        assert!(config.enabled);
        assert!(config.optimize_graph);
        assert!(config.enable_fusion);
    }

    #[test]
    fn test_tensor_descriptor() {
        let desc = TensorDescriptor::new(TensorDataType::Float32, vec![1, 3, 224, 224]);

        assert_eq!(desc.element_count(), 3 * 224 * 224);
        assert_eq!(desc.size_bytes(), 3 * 224 * 224 * 4);
    }

    #[test]
    fn test_operator_graph_builder() {
        let mut builder = OperatorGraphBuilder::new();

        let conv = builder.add_node(DirectMLOperatorType::Convolution);
        let act = builder.add_node(DirectMLOperatorType::Activation);

        builder.connect(conv, act, 0).expect("Failed to connect");

        assert_eq!(builder.node_count(), 2);
        assert_eq!(builder.edge_count(), 1);

        let _graph = builder.build();
    }

    #[test]
    fn test_memory_allocator() {
        let mut allocator = DirectMLMemoryAllocator::new(1024 * 1024);

        let id1 = allocator.allocate(1024, 256).expect("Failed to allocate");
        let id2 = allocator.allocate(2048, 256).expect("Failed to allocate");

        let (used, total, available) = allocator.stats();
        assert!(used > 0);
        assert_eq!(total, 1024 * 1024);
        assert!(available < total);

        allocator.free(id1).expect("Failed to free");
        allocator.free(id2).expect("Failed to free");

        let (used, _, _) = allocator.stats();
        assert_eq!(used, 0);
    }

    #[test]
    fn test_operator_fusion() {
        assert!(OperatorFusionOptimizer::can_fuse(
            DirectMLOperatorType::Convolution,
            DirectMLOperatorType::Activation
        ));

        assert!(!OperatorFusionOptimizer::can_fuse(
            DirectMLOperatorType::Convolution,
            DirectMLOperatorType::Pooling
        ));
    }

    #[test]
    fn test_real_fusion_merges_fusible_pair() {
        // Conv -> Activation is fusible: fusion should merge the pair.
        let mut builder = OperatorGraphBuilder::new();
        let conv = builder.add_node(DirectMLOperatorType::Convolution);
        let act = builder.add_node(DirectMLOperatorType::Activation);
        builder.connect(conv, act, 0).expect("connect");
        let mut graph = builder.build();

        assert_eq!(graph.node_count(), 2);
        let fused = OperatorFusionOptimizer::fuse(&mut graph);
        assert_eq!(fused, 1, "one Conv+Activation fusion expected");
        assert_eq!(graph.node_count(), 1, "consumer node must be removed");
        assert_eq!(graph.edge_count(), 0, "fusing edge must be removed");
    }

    #[test]
    fn test_real_fusion_chains_and_reconnects() {
        // Conv -> Activation -> (Pooling). Conv+Activation fuse; the fused
        // node's edge to Pooling must be preserved (re-sourced from Conv).
        let mut builder = OperatorGraphBuilder::new();
        let conv = builder.add_node(DirectMLOperatorType::Convolution);
        let act = builder.add_node(DirectMLOperatorType::Activation);
        let pool = builder.add_node(DirectMLOperatorType::Pooling);
        builder.connect(conv, act, 0).expect("connect");
        builder.connect(act, pool, 1).expect("connect");
        let mut graph = builder.build();

        let fused = OperatorFusionOptimizer::fuse(&mut graph);
        assert_eq!(fused, 1);
        assert_eq!(graph.node_count(), 2, "Conv(+Act) and Pooling remain");
        // The surviving edge must now originate from the Conv node.
        assert_eq!(graph.edge_count(), 1);
    }

    #[test]
    fn test_non_fusible_graph_unchanged() {
        let mut builder = OperatorGraphBuilder::new();
        let conv = builder.add_node(DirectMLOperatorType::Convolution);
        let pool = builder.add_node(DirectMLOperatorType::Pooling);
        builder.connect(conv, pool, 0).expect("connect");
        let mut graph = builder.build();

        let fused = OperatorFusionOptimizer::fuse(&mut graph);
        assert_eq!(fused, 0, "Conv->Pooling is not fusible");
        assert_eq!(graph.node_count(), 2);
    }

    #[test]
    fn test_execute_operator_returns_honest_error() {
        // Executing a modeled operator must return a typed error, never a silent
        // Ok(()) that pretends the operator ran.
        let mut device = DirectMLDevice::new(DirectMLConfig::default()).expect("sim context");
        device.create_operator("conv1".to_string(), DirectMLOperatorType::Convolution);

        // Unknown operator -> error.
        assert!(device.execute_operator("does_not_exist").is_err());
        // Known operator -> honest unsupported-operation error.
        assert!(device.execute_operator("conv1").is_err());
    }

    #[test]
    fn test_graph_execute_returns_honest_error() {
        let builder = OperatorGraphBuilder::new();
        let graph = builder.build();
        assert!(
            graph.execute().is_err(),
            "numeric graph execution must return a typed error, not a fake Ok"
        );
    }

    #[test]
    fn test_is_not_hardware_accelerated() {
        assert!(DirectMLDevice::is_available(), "simulation layer available");
        assert!(
            !DirectMLDevice::is_hardware_accelerated(),
            "must not claim real hardware acceleration"
        );
    }
}
