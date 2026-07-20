//! Adaptive algorithm selection and auto-tuning for GPUs.
//!
//! This module provides intelligent algorithm selection based on hardware
//! capabilities and workload characteristics with performance feedback loops.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use wgpu::{Device, DeviceType};

/// Adaptive algorithm selector
#[derive(Clone)]
pub struct AdaptiveSelector {
    /// The GPU device (used for future adaptive selection)
    #[allow(dead_code)]
    device: Arc<Device>,
    device_info: DeviceInfo,
    performance_history: Arc<RwLock<PerformanceHistory>>,
    /// Configuration for adaptive selection (used for future tuning)
    #[allow(dead_code)]
    config: AdaptiveConfig,
}

impl AdaptiveSelector {
    /// Create a new adaptive selector
    pub fn new(device: Arc<Device>, device_type: DeviceType) -> Self {
        let device_info = DeviceInfo::from_device(&device, device_type);

        Self {
            device,
            device_info,
            performance_history: Arc::new(RwLock::new(PerformanceHistory::new())),
            config: AdaptiveConfig::default(),
        }
    }

    /// Select best algorithm for a given operation
    pub fn select_algorithm(&self, operation: &str, workload: &WorkloadInfo) -> Algorithm {
        // Check if we have historical data
        let history = self.performance_history.read();
        if let Some(best) = history.get_best_algorithm(operation, workload) {
            return best;
        }
        drop(history);

        // No history, use heuristics based on hardware and workload
        self.select_by_heuristics(operation, workload)
    }

    /// Select algorithm using hardware heuristics
    fn select_by_heuristics(&self, operation: &str, workload: &WorkloadInfo) -> Algorithm {
        match operation {
            "matrix_multiply" => self.select_matmul_algorithm(workload),
            "convolution" => self.select_convolution_algorithm(workload),
            "reduction" => self.select_reduction_algorithm(workload),
            "sort" => self.select_sort_algorithm(workload),
            "fft" => self.select_fft_algorithm(workload),
            _ => Algorithm::default(),
        }
    }

    /// Select matrix multiplication algorithm
    fn select_matmul_algorithm(&self, workload: &WorkloadInfo) -> Algorithm {
        let size = workload.data_size;

        if size < 1024 * 1024 {
            // Small matrices: use naive algorithm
            Algorithm {
                name: "matmul_naive".to_string(),
                workgroup_size: (8, 8, 1),
                strategy: ExecutionStrategy::Direct,
                tuning_params: TuningParams::default(),
            }
        } else if size < 16 * 1024 * 1024 {
            // Medium matrices: use tiled algorithm
            Algorithm {
                name: "matmul_tiled".to_string(),
                workgroup_size: (16, 16, 1),
                strategy: ExecutionStrategy::Tiled { tile_size: 32 },
                tuning_params: TuningParams {
                    use_shared_memory: true,
                    unroll_factor: 4,
                    vectorize: true,
                },
            }
        } else {
            // Large matrices: use hierarchical algorithm
            Algorithm {
                name: "matmul_hierarchical".to_string(),
                workgroup_size: (16, 16, 1),
                strategy: ExecutionStrategy::Hierarchical { levels: 2 },
                tuning_params: TuningParams {
                    use_shared_memory: true,
                    unroll_factor: 8,
                    vectorize: true,
                },
            }
        }
    }

    /// Select convolution algorithm
    fn select_convolution_algorithm(&self, workload: &WorkloadInfo) -> Algorithm {
        let kernel_size = workload.dimensions.first().copied().unwrap_or(3);

        if kernel_size <= 3 {
            Algorithm {
                name: "conv_direct".to_string(),
                workgroup_size: (8, 8, 1),
                strategy: ExecutionStrategy::Direct,
                tuning_params: TuningParams {
                    use_shared_memory: true,
                    unroll_factor: 1,
                    vectorize: false,
                },
            }
        } else {
            Algorithm {
                name: "conv_im2col".to_string(),
                workgroup_size: (16, 16, 1),
                strategy: ExecutionStrategy::Transform,
                tuning_params: TuningParams {
                    use_shared_memory: true,
                    unroll_factor: 4,
                    vectorize: true,
                },
            }
        }
    }

    /// Select reduction algorithm
    fn select_reduction_algorithm(&self, workload: &WorkloadInfo) -> Algorithm {
        let compute_units = self.device_info.compute_units.unwrap_or(8);

        Algorithm {
            name: "reduce_hierarchical".to_string(),
            workgroup_size: (256, 1, 1),
            strategy: ExecutionStrategy::Hierarchical {
                levels: (workload.data_size as f32).log2().ceil() as usize / 8,
            },
            tuning_params: TuningParams {
                use_shared_memory: true,
                unroll_factor: (compute_units / 8).max(1),
                vectorize: true,
            },
        }
    }

    /// Select sorting algorithm
    fn select_sort_algorithm(&self, workload: &WorkloadInfo) -> Algorithm {
        if workload.data_size < 1024 {
            Algorithm {
                name: "sort_insertion".to_string(),
                workgroup_size: (64, 1, 1),
                strategy: ExecutionStrategy::Direct,
                tuning_params: TuningParams::default(),
            }
        } else if workload.data_size < 1024 * 1024 {
            Algorithm {
                name: "sort_bitonic".to_string(),
                workgroup_size: (128, 1, 1),
                strategy: ExecutionStrategy::Parallel,
                tuning_params: TuningParams {
                    use_shared_memory: true,
                    unroll_factor: 2,
                    vectorize: false,
                },
            }
        } else {
            Algorithm {
                name: "sort_radix".to_string(),
                workgroup_size: (256, 1, 1),
                strategy: ExecutionStrategy::Hierarchical { levels: 4 },
                tuning_params: TuningParams {
                    use_shared_memory: true,
                    unroll_factor: 4,
                    vectorize: true,
                },
            }
        }
    }

    /// Select FFT algorithm
    fn select_fft_algorithm(&self, workload: &WorkloadInfo) -> Algorithm {
        let size = workload.data_size;
        let is_power_of_2 = (size & (size - 1)) == 0;

        if is_power_of_2 {
            Algorithm {
                name: "fft_cooley_tukey".to_string(),
                workgroup_size: (256, 1, 1),
                strategy: ExecutionStrategy::Hierarchical {
                    levels: (size as f32).log2() as usize,
                },
                tuning_params: TuningParams {
                    use_shared_memory: true,
                    unroll_factor: 4,
                    vectorize: true,
                },
            }
        } else {
            Algorithm {
                name: "fft_bluestein".to_string(),
                workgroup_size: (128, 1, 1),
                strategy: ExecutionStrategy::Transform,
                tuning_params: TuningParams {
                    use_shared_memory: false,
                    unroll_factor: 2,
                    vectorize: false,
                },
            }
        }
    }

    /// Record performance for an algorithm
    pub fn record_performance(
        &self,
        operation: &str,
        workload: &WorkloadInfo,
        algorithm: &Algorithm,
        duration: Duration,
    ) {
        let mut history = self.performance_history.write();
        history.record(operation, workload, algorithm, duration);
    }

    /// Get performance statistics
    pub fn get_statistics(&self, operation: &str) -> Option<AlgorithmStats> {
        let history = self.performance_history.read();
        history.get_stats(operation)
    }
}

/// Device hardware information
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// Device type
    pub device_type: DeviceType,
    /// Estimated compute units
    pub compute_units: Option<u32>,
    /// Memory bandwidth (estimated, GB/s)
    pub memory_bandwidth: Option<f32>,
    /// Peak FLOPS (estimated)
    pub peak_flops: Option<f64>,
}

impl DeviceInfo {
    fn from_device(_device: &Device, device_type: DeviceType) -> Self {
        let compute_units = match device_type {
            DeviceType::DiscreteGpu => Some(64),
            DeviceType::IntegratedGpu => Some(16),
            _ => None,
        };

        Self {
            device_type,
            compute_units,
            memory_bandwidth: None,
            peak_flops: None,
        }
    }
}

/// Workload information
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct WorkloadInfo {
    /// Total data size in bytes
    pub data_size: u64,
    /// Data dimensions (e.g., [width, height] for 2D)
    pub dimensions: Vec<u32>,
    /// Data type size in bytes
    pub element_size: u32,
}

/// Algorithm configuration
#[derive(Debug, Clone)]
pub struct Algorithm {
    /// Algorithm name
    pub name: String,
    /// Workgroup size (x, y, z)
    pub workgroup_size: (u32, u32, u32),
    /// Execution strategy
    pub strategy: ExecutionStrategy,
    /// Tuning parameters
    pub tuning_params: TuningParams,
}

impl Default for Algorithm {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            workgroup_size: (8, 8, 1),
            strategy: ExecutionStrategy::Direct,
            tuning_params: TuningParams::default(),
        }
    }
}

/// Execution strategy
#[derive(Debug, Clone, Copy)]
pub enum ExecutionStrategy {
    /// Direct execution
    Direct,
    /// Tiled execution with specified tile size
    Tiled {
        /// Tile size for tiled execution
        tile_size: u32,
    },
    /// Hierarchical/recursive execution with specified levels
    Hierarchical {
        /// Number of hierarchical levels
        levels: usize,
    },
    /// Transform-based execution
    Transform,
    /// Parallel execution
    Parallel,
}

/// Algorithm tuning parameters
#[derive(Debug, Clone)]
pub struct TuningParams {
    /// Use shared/local memory
    pub use_shared_memory: bool,
    /// Loop unroll factor
    pub unroll_factor: u32,
    /// Enable vectorization
    pub vectorize: bool,
}

impl Default for TuningParams {
    fn default() -> Self {
        Self {
            use_shared_memory: false,
            unroll_factor: 1,
            vectorize: false,
        }
    }
}

/// Performance history tracker
struct PerformanceHistory {
    records: HashMap<String, Vec<PerformanceRecord>>,
    max_records_per_operation: usize,
}

impl PerformanceHistory {
    fn new() -> Self {
        Self {
            records: HashMap::new(),
            max_records_per_operation: 100,
        }
    }

    fn record(
        &mut self,
        operation: &str,
        workload: &WorkloadInfo,
        algorithm: &Algorithm,
        duration: Duration,
    ) {
        let record = PerformanceRecord {
            workload: workload.clone(),
            algorithm_name: algorithm.name.clone(),
            duration,
            timestamp: Instant::now(),
        };

        let records = self.records.entry(operation.to_string()).or_default();
        records.push(record);

        // Keep only recent records
        if records.len() > self.max_records_per_operation {
            records.remove(0);
        }
    }

    fn get_best_algorithm(&self, operation: &str, workload: &WorkloadInfo) -> Option<Algorithm> {
        let records = self.records.get(operation)?;

        // Find records with similar workload
        let mut similar: Vec<_> = records
            .iter()
            .filter(|r| Self::is_similar_workload(&r.workload, workload))
            .collect();

        if similar.is_empty() {
            return None;
        }

        // Sort by duration
        similar.sort_by_key(|r| r.duration);

        // Return the algorithm of the best performing record
        Some(Algorithm {
            name: similar[0].algorithm_name.clone(),
            ..Algorithm::default()
        })
    }

    fn is_similar_workload(w1: &WorkloadInfo, w2: &WorkloadInfo) -> bool {
        // Similar if data size within 20% and same dimensions count
        let size_ratio = (w1.data_size as f64) / (w2.data_size as f64);
        size_ratio > 0.8 && size_ratio < 1.2 && w1.dimensions.len() == w2.dimensions.len()
    }

    fn get_stats(&self, operation: &str) -> Option<AlgorithmStats> {
        let records = self.records.get(operation)?;

        if records.is_empty() {
            return None;
        }

        let total_duration: Duration = records.iter().map(|r| r.duration).sum();
        let count = records.len() as u32;

        Some(AlgorithmStats {
            total_executions: count,
            average_duration: total_duration / count,
            total_duration,
        })
    }
}

/// Performance record
#[derive(Debug, Clone)]
struct PerformanceRecord {
    workload: WorkloadInfo,
    algorithm_name: String,
    duration: Duration,
    /// Timestamp when the record was created (for future LRU eviction)
    #[allow(dead_code)]
    timestamp: Instant,
}

/// Algorithm performance statistics
#[derive(Debug, Clone)]
pub struct AlgorithmStats {
    /// Total executions
    pub total_executions: u32,
    /// Average duration
    pub average_duration: Duration,
    /// Total duration
    pub total_duration: Duration,
}

/// Adaptive configuration
#[derive(Debug, Clone)]
pub struct AdaptiveConfig {
    /// Enable auto-tuning
    pub auto_tune: bool,
    /// Minimum samples before trusting history
    pub min_samples: usize,
}

impl Default for AdaptiveConfig {
    fn default() -> Self {
        Self {
            auto_tune: true,
            min_samples: 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workload_similarity() {
        let w1 = WorkloadInfo {
            data_size: 1000,
            dimensions: vec![100, 10],
            element_size: 4,
        };

        let w2 = WorkloadInfo {
            data_size: 1100,
            dimensions: vec![110, 10],
            element_size: 4,
        };

        let w3 = WorkloadInfo {
            data_size: 2000,
            dimensions: vec![100, 20],
            element_size: 4,
        };

        assert!(PerformanceHistory::is_similar_workload(&w1, &w2));
        assert!(!PerformanceHistory::is_similar_workload(&w1, &w3));
    }

    #[test]
    fn test_algorithm_default() {
        let algo = Algorithm::default();
        assert_eq!(algo.name, "default");
        assert_eq!(algo.workgroup_size, (8, 8, 1));
    }
}
