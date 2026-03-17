//! GPU profiling and performance metrics.
//!
//! This module provides comprehensive GPU profiling capabilities including:
//! - Kernel execution time measurement
//! - Memory bandwidth tracking
//! - GPU utilization metrics
//! - Performance bottleneck detection
//! - Power consumption monitoring (when available)

use crate::error::Result;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use wgpu::{Device, Queue};

/// GPU profiling manager
#[derive(Clone)]
pub struct GpuProfiler {
    /// Device for GPU timestamp queries (reserved for GPU profiling)
    #[allow(dead_code)]
    device: Arc<Device>,
    /// Queue for GPU command submission (reserved for GPU profiling)
    #[allow(dead_code)]
    queue: Arc<Queue>,
    metrics: Arc<RwLock<ProfilingMetrics>>,
    config: ProfilingConfig,
    /// Query sets for GPU timestamp queries (reserved for GPU profiling)
    #[allow(dead_code)]
    query_sets: Arc<RwLock<Vec<wgpu::QuerySet>>>,
    timestamp_period: f32,
}

impl GpuProfiler {
    /// Create a new GPU profiler
    pub fn new(device: Arc<Device>, queue: Arc<Queue>, config: ProfilingConfig) -> Result<Self> {
        // Get timestamp period for accurate timing
        let timestamp_period = queue.get_timestamp_period();

        Ok(Self {
            device,
            queue,
            metrics: Arc::new(RwLock::new(ProfilingMetrics::default())),
            config,
            query_sets: Arc::new(RwLock::new(Vec::new())),
            timestamp_period,
        })
    }

    /// Start profiling a kernel execution
    pub fn begin_profile(&self, label: &str) -> ProfileSession {
        let start = Instant::now();
        ProfileSession {
            label: label.to_string(),
            start,
            profiler: self.clone(),
            gpu_start_query: None,
            gpu_end_query: None,
        }
    }

    /// Record kernel execution metrics
    pub fn record_kernel_execution(
        &self,
        label: &str,
        duration: Duration,
        memory_bytes: u64,
        compute_units: u32,
    ) {
        let mut metrics = self.metrics.write();
        metrics.record_kernel(label, duration, memory_bytes, compute_units);
    }

    /// Record memory transfer
    pub fn record_memory_transfer(&self, bytes: u64, duration: Duration, host_to_device: bool) {
        let mut metrics = self.metrics.write();
        metrics.record_transfer(bytes, duration, host_to_device);
    }

    /// Get current metrics
    pub fn get_metrics(&self) -> ProfilingMetrics {
        self.metrics.read().clone()
    }

    /// Generate profiling report
    pub fn generate_report(&self) -> ProfilingReport {
        let metrics = self.metrics.read();
        ProfilingReport::from_metrics(&metrics)
    }

    /// Reset all metrics
    pub fn reset(&self) {
        let mut metrics = self.metrics.write();
        *metrics = ProfilingMetrics::default();
    }

    /// Get timestamp period in nanoseconds
    pub fn timestamp_period(&self) -> f32 {
        self.timestamp_period
    }

    /// Detect performance bottlenecks
    pub fn detect_bottlenecks(&self) -> Vec<PerformanceBottleneck> {
        let metrics = self.metrics.read();
        let mut bottlenecks = Vec::new();

        // Check memory bandwidth
        if let Some(bandwidth_gbs) = metrics.average_memory_bandwidth_gbs() {
            if bandwidth_gbs < self.config.min_expected_bandwidth_gbs {
                bottlenecks.push(PerformanceBottleneck {
                    kind: BottleneckKind::MemoryBandwidth,
                    severity: BottleneckSeverity::High,
                    description: format!(
                        "Memory bandwidth {:.2} GB/s is below expected {:.2} GB/s",
                        bandwidth_gbs, self.config.min_expected_bandwidth_gbs
                    ),
                    suggestion: "Consider batching transfers or using compression".to_string(),
                });
            }
        }

        // Check kernel efficiency
        for (label, stats) in &metrics.kernel_stats {
            if let Some(avg_duration) = stats.average_duration() {
                if avg_duration > self.config.max_kernel_duration {
                    bottlenecks.push(PerformanceBottleneck {
                        kind: BottleneckKind::KernelExecution,
                        severity: BottleneckSeverity::Medium,
                        description: format!(
                            "Kernel '{}' average duration {:?} exceeds threshold {:?}",
                            label, avg_duration, self.config.max_kernel_duration
                        ),
                        suggestion: "Consider optimizing shader or reducing workload".to_string(),
                    });
                }
            }
        }

        // Check transfer overhead
        let total_time = metrics.total_duration();
        let transfer_time = metrics.total_transfer_duration();
        if total_time > Duration::ZERO {
            let transfer_ratio = transfer_time.as_secs_f64() / total_time.as_secs_f64();
            if transfer_ratio > self.config.max_transfer_ratio {
                bottlenecks.push(PerformanceBottleneck {
                    kind: BottleneckKind::TransferOverhead,
                    severity: BottleneckSeverity::High,
                    description: format!(
                        "Memory transfer overhead {:.1}% exceeds threshold {:.1}%",
                        transfer_ratio * 100.0,
                        self.config.max_transfer_ratio * 100.0
                    ),
                    suggestion: "Reduce data transfers or overlap with computation".to_string(),
                });
            }
        }

        bottlenecks
    }
}

/// Profile session for a single operation
pub struct ProfileSession {
    label: String,
    start: Instant,
    profiler: GpuProfiler,
    /// GPU start query index (reserved for GPU timestamp queries)
    #[allow(dead_code)]
    gpu_start_query: Option<u32>,
    /// GPU end query index (reserved for GPU timestamp queries)
    #[allow(dead_code)]
    gpu_end_query: Option<u32>,
}

impl ProfileSession {
    /// End profiling and record metrics
    pub fn end(self, memory_bytes: u64, compute_units: u32) {
        let duration = self.start.elapsed();
        self.profiler
            .record_kernel_execution(&self.label, duration, memory_bytes, compute_units);
    }

    /// End with custom duration (for GPU timestamp queries)
    pub fn end_with_duration(self, duration: Duration, memory_bytes: u64, compute_units: u32) {
        self.profiler
            .record_kernel_execution(&self.label, duration, memory_bytes, compute_units);
    }
}

/// Profiling configuration
#[derive(Debug, Clone)]
pub struct ProfilingConfig {
    /// Enable detailed profiling
    pub detailed: bool,
    /// Minimum expected memory bandwidth in GB/s
    pub min_expected_bandwidth_gbs: f64,
    /// Maximum acceptable kernel duration
    pub max_kernel_duration: Duration,
    /// Maximum acceptable transfer overhead ratio (0.0 - 1.0)
    pub max_transfer_ratio: f64,
    /// Enable power consumption tracking (if available)
    pub track_power: bool,
}

impl Default for ProfilingConfig {
    fn default() -> Self {
        Self {
            detailed: true,
            min_expected_bandwidth_gbs: 100.0,
            max_kernel_duration: Duration::from_millis(100),
            max_transfer_ratio: 0.3,
            track_power: false,
        }
    }
}

/// Aggregated profiling metrics
#[derive(Debug, Clone, Default)]
pub struct ProfilingMetrics {
    /// Per-kernel statistics
    pub kernel_stats: HashMap<String, KernelStats>,
    /// Memory transfer statistics
    pub transfer_stats: TransferStats,
    /// Overall metrics
    pub overall: OverallMetrics,
}

impl ProfilingMetrics {
    /// Record a kernel execution
    fn record_kernel(
        &mut self,
        label: &str,
        duration: Duration,
        memory_bytes: u64,
        compute_units: u32,
    ) {
        let stats = self.kernel_stats.entry(label.to_string()).or_default();
        stats.record(duration, memory_bytes, compute_units);
        self.overall.total_kernel_time += duration;
        self.overall.total_kernels += 1;
    }

    /// Record a memory transfer
    fn record_transfer(&mut self, bytes: u64, duration: Duration, host_to_device: bool) {
        if host_to_device {
            self.transfer_stats.host_to_device.record(bytes, duration);
        } else {
            self.transfer_stats.device_to_host.record(bytes, duration);
        }
        self.overall.total_transfer_time += duration;
        self.overall.total_transfers += 1;
        self.overall.total_bytes_transferred += bytes;
    }

    /// Calculate average memory bandwidth in GB/s
    fn average_memory_bandwidth_gbs(&self) -> Option<f64> {
        let total_bytes = self.overall.total_bytes_transferred;
        let total_time = self.overall.total_transfer_time;

        if total_time > Duration::ZERO && total_bytes > 0 {
            let bytes_per_sec = total_bytes as f64 / total_time.as_secs_f64();
            Some(bytes_per_sec / 1_000_000_000.0)
        } else {
            None
        }
    }

    /// Get total duration
    fn total_duration(&self) -> Duration {
        self.overall.total_kernel_time + self.overall.total_transfer_time
    }

    /// Get total transfer duration
    fn total_transfer_duration(&self) -> Duration {
        self.overall.total_transfer_time
    }
}

/// Statistics for a specific kernel
#[derive(Debug, Clone, Default)]
pub struct KernelStats {
    /// Number of executions
    pub executions: u64,
    /// Total execution time
    pub total_duration: Duration,
    /// Minimum execution time
    pub min_duration: Option<Duration>,
    /// Maximum execution time
    pub max_duration: Option<Duration>,
    /// Total memory accessed
    pub total_memory_bytes: u64,
    /// Total compute units used
    pub total_compute_units: u64,
}

impl KernelStats {
    fn record(&mut self, duration: Duration, memory_bytes: u64, compute_units: u32) {
        self.executions += 1;
        self.total_duration += duration;
        self.total_memory_bytes += memory_bytes;
        self.total_compute_units += compute_units as u64;

        self.min_duration = Some(
            self.min_duration
                .map(|min| min.min(duration))
                .unwrap_or(duration),
        );
        self.max_duration = Some(
            self.max_duration
                .map(|max| max.max(duration))
                .unwrap_or(duration),
        );
    }

    /// Calculate average duration
    pub fn average_duration(&self) -> Option<Duration> {
        if self.executions > 0 {
            Some(self.total_duration / self.executions as u32)
        } else {
            None
        }
    }

    /// Calculate bandwidth in GB/s
    pub fn bandwidth_gbs(&self) -> Option<f64> {
        if self.total_duration > Duration::ZERO && self.total_memory_bytes > 0 {
            let bytes_per_sec = self.total_memory_bytes as f64 / self.total_duration.as_secs_f64();
            Some(bytes_per_sec / 1_000_000_000.0)
        } else {
            None
        }
    }
}

/// Memory transfer statistics
#[derive(Debug, Clone, Default)]
pub struct TransferStats {
    /// Host to device transfers
    pub host_to_device: DirectionalTransferStats,
    /// Device to host transfers
    pub device_to_host: DirectionalTransferStats,
}

/// Directional transfer statistics
#[derive(Debug, Clone, Default)]
pub struct DirectionalTransferStats {
    /// Number of transfers
    pub count: u64,
    /// Total bytes transferred
    pub total_bytes: u64,
    /// Total transfer time
    pub total_duration: Duration,
    /// Minimum transfer time
    pub min_duration: Option<Duration>,
    /// Maximum transfer time
    pub max_duration: Option<Duration>,
}

impl DirectionalTransferStats {
    fn record(&mut self, bytes: u64, duration: Duration) {
        self.count += 1;
        self.total_bytes += bytes;
        self.total_duration += duration;

        self.min_duration = Some(
            self.min_duration
                .map(|min| min.min(duration))
                .unwrap_or(duration),
        );
        self.max_duration = Some(
            self.max_duration
                .map(|max| max.max(duration))
                .unwrap_or(duration),
        );
    }

    /// Calculate average bandwidth in GB/s
    pub fn bandwidth_gbs(&self) -> Option<f64> {
        if self.total_duration > Duration::ZERO && self.total_bytes > 0 {
            let bytes_per_sec = self.total_bytes as f64 / self.total_duration.as_secs_f64();
            Some(bytes_per_sec / 1_000_000_000.0)
        } else {
            None
        }
    }
}

/// Overall metrics
#[derive(Debug, Clone, Default)]
pub struct OverallMetrics {
    /// Total kernel execution time
    pub total_kernel_time: Duration,
    /// Total memory transfer time
    pub total_transfer_time: Duration,
    /// Total number of kernels executed
    pub total_kernels: u64,
    /// Total number of transfers
    pub total_transfers: u64,
    /// Total bytes transferred
    pub total_bytes_transferred: u64,
}

/// Profiling report
#[derive(Debug, Clone)]
pub struct ProfilingReport {
    /// Summary statistics
    pub summary: ReportSummary,
    /// Per-kernel details
    pub kernel_details: Vec<KernelDetail>,
    /// Transfer details
    pub transfer_details: TransferDetail,
    /// Detected bottlenecks
    pub bottlenecks: Vec<PerformanceBottleneck>,
}

impl ProfilingReport {
    fn from_metrics(metrics: &ProfilingMetrics) -> Self {
        let mut kernel_details = Vec::new();
        for (label, stats) in &metrics.kernel_stats {
            kernel_details.push(KernelDetail {
                name: label.clone(),
                executions: stats.executions,
                total_time: stats.total_duration,
                avg_time: stats.average_duration().unwrap_or_default(),
                min_time: stats.min_duration.unwrap_or_default(),
                max_time: stats.max_duration.unwrap_or_default(),
                bandwidth_gbs: stats.bandwidth_gbs(),
            });
        }

        // Sort by total time descending
        kernel_details.sort_by_key(|x| std::cmp::Reverse(x.total_time));

        Self {
            summary: ReportSummary {
                total_duration: metrics.total_duration(),
                kernel_time: metrics.overall.total_kernel_time,
                transfer_time: metrics.overall.total_transfer_time,
                total_kernels: metrics.overall.total_kernels,
                total_transfers: metrics.overall.total_transfers,
                average_bandwidth_gbs: metrics.average_memory_bandwidth_gbs(),
            },
            kernel_details,
            transfer_details: TransferDetail {
                host_to_device_count: metrics.transfer_stats.host_to_device.count,
                host_to_device_bytes: metrics.transfer_stats.host_to_device.total_bytes,
                host_to_device_bandwidth: metrics.transfer_stats.host_to_device.bandwidth_gbs(),
                device_to_host_count: metrics.transfer_stats.device_to_host.count,
                device_to_host_bytes: metrics.transfer_stats.device_to_host.total_bytes,
                device_to_host_bandwidth: metrics.transfer_stats.device_to_host.bandwidth_gbs(),
            },
            bottlenecks: Vec::new(),
        }
    }

    /// Print report to stdout
    pub fn print(&self) {
        println!("=== GPU Profiling Report ===");
        println!("\nSummary:");
        println!("  Total Duration: {:?}", self.summary.total_duration);
        println!(
            "  Kernel Time: {:?} ({:.1}%)",
            self.summary.kernel_time,
            100.0 * self.summary.kernel_time.as_secs_f64()
                / self.summary.total_duration.as_secs_f64()
        );
        println!(
            "  Transfer Time: {:?} ({:.1}%)",
            self.summary.transfer_time,
            100.0 * self.summary.transfer_time.as_secs_f64()
                / self.summary.total_duration.as_secs_f64()
        );
        println!("  Total Kernels: {}", self.summary.total_kernels);
        println!("  Total Transfers: {}", self.summary.total_transfers);
        if let Some(bw) = self.summary.average_bandwidth_gbs {
            println!("  Average Bandwidth: {:.2} GB/s", bw);
        }

        println!("\nTop Kernels by Time:");
        for detail in self.kernel_details.iter().take(10) {
            println!(
                "  {} ({} execs): {:?} total, {:?} avg",
                detail.name, detail.executions, detail.total_time, detail.avg_time
            );
            if let Some(bw) = detail.bandwidth_gbs {
                println!("    Bandwidth: {:.2} GB/s", bw);
            }
        }

        println!("\nMemory Transfers:");
        println!(
            "  Host->Device: {} transfers, {} bytes ({:.2} GB/s)",
            self.transfer_details.host_to_device_count,
            self.transfer_details.host_to_device_bytes,
            self.transfer_details
                .host_to_device_bandwidth
                .unwrap_or(0.0)
        );
        println!(
            "  Device->Host: {} transfers, {} bytes ({:.2} GB/s)",
            self.transfer_details.device_to_host_count,
            self.transfer_details.device_to_host_bytes,
            self.transfer_details
                .device_to_host_bandwidth
                .unwrap_or(0.0)
        );

        if !self.bottlenecks.is_empty() {
            println!("\nPerformance Bottlenecks:");
            for bottleneck in &self.bottlenecks {
                println!(
                    "  [{:?}] {:?}: {}",
                    bottleneck.severity, bottleneck.kind, bottleneck.description
                );
                println!("    Suggestion: {}", bottleneck.suggestion);
            }
        }
    }
}

/// Report summary
#[derive(Debug, Clone)]
pub struct ReportSummary {
    /// Total profiling duration
    pub total_duration: Duration,
    /// Total kernel execution time
    pub kernel_time: Duration,
    /// Total transfer time
    pub transfer_time: Duration,
    /// Total number of kernels
    pub total_kernels: u64,
    /// Total number of transfers
    pub total_transfers: u64,
    /// Average bandwidth
    pub average_bandwidth_gbs: Option<f64>,
}

/// Kernel detail in report
#[derive(Debug, Clone)]
pub struct KernelDetail {
    /// Kernel name
    pub name: String,
    /// Number of executions
    pub executions: u64,
    /// Total time
    pub total_time: Duration,
    /// Average time
    pub avg_time: Duration,
    /// Minimum time
    pub min_time: Duration,
    /// Maximum time
    pub max_time: Duration,
    /// Bandwidth in GB/s
    pub bandwidth_gbs: Option<f64>,
}

/// Transfer detail in report
#[derive(Debug, Clone)]
pub struct TransferDetail {
    /// Host to device count
    pub host_to_device_count: u64,
    /// Host to device bytes
    pub host_to_device_bytes: u64,
    /// Host to device bandwidth
    pub host_to_device_bandwidth: Option<f64>,
    /// Device to host count
    pub device_to_host_count: u64,
    /// Device to host bytes
    pub device_to_host_bytes: u64,
    /// Device to host bandwidth
    pub device_to_host_bandwidth: Option<f64>,
}

/// Performance bottleneck
#[derive(Debug, Clone)]
pub struct PerformanceBottleneck {
    /// Bottleneck kind
    pub kind: BottleneckKind,
    /// Severity
    pub severity: BottleneckSeverity,
    /// Description
    pub description: String,
    /// Suggestion for improvement
    pub suggestion: String,
}

/// Bottleneck kind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottleneckKind {
    /// Memory bandwidth bottleneck
    MemoryBandwidth,
    /// Kernel execution bottleneck
    KernelExecution,
    /// Transfer overhead bottleneck
    TransferOverhead,
    /// Synchronization bottleneck
    Synchronization,
}

/// Bottleneck severity
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BottleneckSeverity {
    /// Low severity
    Low,
    /// Medium severity
    Medium,
    /// High severity
    High,
    /// Critical severity
    Critical,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kernel_stats() {
        let mut stats = KernelStats::default();
        stats.record(Duration::from_millis(10), 1024, 8);
        stats.record(Duration::from_millis(20), 2048, 16);

        assert_eq!(stats.executions, 2);
        assert_eq!(stats.total_memory_bytes, 3072);
        assert_eq!(stats.total_compute_units, 24);
        assert_eq!(stats.min_duration, Some(Duration::from_millis(10)));
        assert_eq!(stats.max_duration, Some(Duration::from_millis(20)));
    }

    #[test]
    fn test_transfer_stats() {
        let mut stats = DirectionalTransferStats::default();
        stats.record(1024, Duration::from_micros(10));
        stats.record(2048, Duration::from_micros(20));

        assert_eq!(stats.count, 2);
        assert_eq!(stats.total_bytes, 3072);
        assert!(stats.bandwidth_gbs().is_some());
    }
}
