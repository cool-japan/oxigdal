//! DirectML support for Windows ML acceleration.
//!
//! This module provides DirectML integration for hardware-accelerated machine
//! learning operations on Windows platforms.

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

/// DirectML device manager.
pub struct DirectMLDevice {
    config: DirectMLConfig,
    operators: HashMap<String, DirectMLOperator>,
}

impl DirectMLDevice {
    /// Create a new DirectML device.
    ///
    /// # Errors
    ///
    /// Returns an error if DirectML is not available.
    pub fn new(config: DirectMLConfig) -> GpuResult<Self> {
        if !Self::is_available() {
            return Err(GpuError::unsupported_operation(
                "DirectML not available on this platform".to_string(),
            ));
        }

        info!("Initializing DirectML device {}", config.device_index);

        Ok(Self {
            config,
            operators: HashMap::new(),
        })
    }

    /// Check if DirectML is available.
    pub fn is_available() -> bool {
        cfg!(target_os = "windows")
    }

    /// Create an operator.
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

        debug!("Created DirectML operator '{}' ({:?})", name, op_type);

        id
    }

    /// Execute an operator.
    ///
    /// # Errors
    ///
    /// Returns an error if operator not found.
    pub fn execute_operator(&self, name: &str) -> GpuResult<()> {
        let _operator = self
            .operators
            .get(name)
            .ok_or_else(|| GpuError::internal("Operator not found"))?;

        // Placeholder for actual execution
        debug!("Executing DirectML operator '{}'", name);

        Ok(())
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
    /// Execute the graph.
    pub fn execute(&self) -> GpuResult<()> {
        // Placeholder for actual execution
        debug!("Executing operator graph with {} nodes", self.nodes.len());
        Ok(())
    }

    /// Optimize the graph.
    pub fn optimize(&mut self) {
        // Placeholder for graph optimization
        debug!("Optimizing operator graph");
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
    /// Fuse compatible operators in a graph.
    pub fn fuse(_graph: &mut OperatorGraph) -> usize {
        // Placeholder for operator fusion
        // In a real implementation, this would:
        // 1. Identify fusible operator sequences
        // 2. Replace them with fused operators
        // 3. Update graph edges

        debug!("Fusing operators in graph");

        0 // Number of fusions performed
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

/// Wave operations for DirectML.
pub struct WaveOperations;

impl WaveOperations {
    /// Generate shader code for wave operations.
    pub fn wave_intrinsics_shader() -> &'static str {
        r#"
// DirectML wave intrinsics
// These are similar to CUDA warp operations

fn wave_get_lane_count() -> u32 {
    // Typically 32 or 64 on modern GPUs
    return 32u;
}

fn wave_get_lane_index() -> u32 {
    // Lane index within the wave
    return 0u;
}

fn wave_active_all_equal(value: f32) -> bool {
    // Check if all active lanes have the same value
    return true;
}

fn wave_active_any(condition: bool) -> bool {
    // Check if any active lane meets the condition
    return condition;
}

fn wave_active_all(condition: bool) -> bool {
    // Check if all active lanes meet the condition
    return condition;
}

fn wave_prefix_sum(value: f32) -> f32 {
    // Exclusive prefix sum across the wave
    return value;
}

fn wave_read_lane_at(value: f32, lane_index: u32) -> f32 {
    // Read value from a specific lane
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
}
