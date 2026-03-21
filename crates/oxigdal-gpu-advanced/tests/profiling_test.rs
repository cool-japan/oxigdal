//! Tests for GPU profiling and metrics

use oxigdal_gpu_advanced::{GpuProfiler, ProfilingConfig};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn test_profiler_creation() {
    // This test requires actual GPU, may not work in CI
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });

    let adapter = match instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
    {
        Ok(adapter) => adapter,
        Err(e) => {
            println!("No GPU available, skipping test: {}", e);
            return;
        }
    };

    let (device, queue) = match adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("test_device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::Off,
        })
        .await
    {
        Ok((device, queue)) => (device, queue),
        Err(e) => {
            println!("Failed to request device: {}", e);
            return;
        }
    };

    let device = Arc::new(device);
    let queue = Arc::new(queue);

    let config = ProfilingConfig::default();
    let profiler = GpuProfiler::new(device, queue, config);

    assert!(profiler.is_ok());
}

#[tokio::test]
async fn test_profiling_session() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });

    let adapter = match instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
    {
        Ok(adapter) => adapter,
        Err(e) => {
            println!("No GPU available, skipping test: {}", e);
            return;
        }
    };

    let (device, queue) = match adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("test_device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::Off,
        })
        .await
    {
        Ok((device, queue)) => (device, queue),
        Err(e) => {
            println!("Failed to request device: {}", e);
            return;
        }
    };

    let device = Arc::new(device);
    let queue = Arc::new(queue);

    let config = ProfilingConfig::default();
    let profiler = GpuProfiler::new(device, queue, config).expect("Failed to create profiler");

    // Begin and end a profiling session
    let session = profiler.begin_profile("test_kernel");
    session.end(1024, 8);

    // Get metrics
    let metrics = profiler.get_metrics();
    assert_eq!(metrics.overall.total_kernels, 1);
}

#[test]
fn test_profiling_config() {
    let config = ProfilingConfig::default();
    assert!(config.detailed);
    assert_eq!(config.min_expected_bandwidth_gbs, 100.0);
    assert_eq!(config.max_kernel_duration, Duration::from_millis(100));
}

#[tokio::test]
async fn test_bottleneck_detection() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });

    let adapter = match instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
    {
        Ok(adapter) => adapter,
        Err(e) => {
            println!("No GPU available, skipping test: {}", e);
            return;
        }
    };

    let (device, queue) = match adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("test_device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::Off,
        })
        .await
    {
        Ok((device, queue)) => (device, queue),
        Err(e) => {
            println!("Failed to request device: {}", e);
            return;
        }
    };

    let device = Arc::new(device);
    let queue = Arc::new(queue);

    let config = ProfilingConfig {
        detailed: true,
        min_expected_bandwidth_gbs: 1000.0, // Unrealistically high
        max_kernel_duration: Duration::from_nanos(1), // Unrealistically low
        max_transfer_ratio: 0.01,
        track_power: false,
    };

    let profiler = GpuProfiler::new(device, queue, config).expect("Failed to create profiler");

    // Record some metrics
    profiler.record_kernel_execution("slow_kernel", Duration::from_millis(200), 1024, 8);
    profiler.record_memory_transfer(1024 * 1024, Duration::from_millis(100), true);

    // Detect bottlenecks
    let bottlenecks = profiler.detect_bottlenecks();
    assert!(!bottlenecks.is_empty());
}

#[tokio::test]
async fn test_profiling_report() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });

    let adapter = match instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
    {
        Ok(adapter) => adapter,
        Err(e) => {
            println!("No GPU available, skipping test: {}", e);
            return;
        }
    };

    let (device, queue) = match adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("test_device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::Off,
        })
        .await
    {
        Ok((device, queue)) => (device, queue),
        Err(e) => {
            println!("Failed to request device: {}", e);
            return;
        }
    };

    let device = Arc::new(device);
    let queue = Arc::new(queue);

    let config = ProfilingConfig::default();
    let profiler = GpuProfiler::new(device, queue, config).expect("Failed to create profiler");

    // Record some metrics
    profiler.record_kernel_execution("kernel1", Duration::from_millis(10), 1024, 8);
    profiler.record_kernel_execution("kernel2", Duration::from_millis(20), 2048, 16);
    profiler.record_memory_transfer(1024 * 1024, Duration::from_millis(5), true);

    // Generate report
    let report = profiler.generate_report();
    assert_eq!(report.summary.total_kernels, 2);
    assert_eq!(report.summary.total_transfers, 1);
    assert_eq!(report.kernel_details.len(), 2);

    // Print report for manual inspection
    report.print();
}
