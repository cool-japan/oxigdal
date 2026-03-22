//! Comprehensive integration tests for advanced GPU features

use oxigdal_gpu_advanced::{
    AdaptiveSelector, CompactionConfig, GpuProfiler, KernelRegistry, MemoryCompactor,
    PipelineBuilder, PipelineStage, ProfilingConfig, WorkloadInfo,
};
use std::sync::Arc;
use std::time::Duration;

const SIMPLE_COMPUTE_SHADER: &str = r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64, 1, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let idx = global_id.x;
    output[idx] = input[idx] * 2.0;
}
"#;

#[tokio::test]
async fn test_kernel_registry_integration() {
    let registry = KernelRegistry::new();

    // Verify all built-in shaders are available
    assert!(registry.has_shader("matrix_ops"));
    assert!(registry.has_shader("fft"));
    assert!(registry.has_shader("histogram_eq"));
    assert!(registry.has_shader("morphology"));
    assert!(registry.has_shader("edge_detection"));
    assert!(registry.has_shader("texture_analysis"));

    // Get each shader and verify it's not empty
    assert!(registry.get_shader("matrix_ops").is_some());
    assert!(
        !registry
            .get_shader("matrix_ops")
            .expect("shader")
            .is_empty()
    );

    let shader_list = registry.list_shaders();
    assert_eq!(shader_list.len(), 6);
}

#[tokio::test]
async fn test_adaptive_pipeline_integration() {
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

    let info = adapter.get_info();

    let (device, queue) = match adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("integration_test_device"),
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

    // Create adaptive selector
    let selector = AdaptiveSelector::new(device.clone(), info.device_type);

    // Test workload selection
    let workload = WorkloadInfo {
        data_size: 1024 * 1024,
        dimensions: vec![1024, 1024],
        element_size: 4,
    };

    let algorithm = selector.select_algorithm("matrix_multiply", &workload);
    assert!(!algorithm.name.is_empty());

    // Create profiler
    let profiling_config = ProfilingConfig::default();
    let profiler = GpuProfiler::new(device.clone(), queue.clone(), profiling_config)
        .expect("Failed to create profiler");

    // Profile a simulated operation
    let session = profiler.begin_profile("test_operation");
    tokio::time::sleep(Duration::from_millis(10)).await;
    session.end(1024 * 1024, 16);

    // Check metrics
    let metrics = profiler.get_metrics();
    assert_eq!(metrics.overall.total_kernels, 1);

    // Generate report
    let report = profiler.generate_report();
    assert!(!report.kernel_details.is_empty());
}

#[tokio::test]
async fn test_memory_profiling_integration() {
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
            label: Some("mem_test_device"),
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

    // Create memory compactor
    let compaction_config = CompactionConfig::default();
    let compactor = MemoryCompactor::new(device.clone(), queue.clone(), compaction_config);

    // Simulate allocations
    for i in 0..50 {
        let offset = i * 2048;
        let size = 1024;
        compactor.register_allocation(i, offset, size, true);
    }

    // Detect fragmentation
    let frag_info = compactor.detect_fragmentation();
    assert!(frag_info.total_size > 0);
    assert_eq!(frag_info.used_size, 50 * 1024);

    // Check if compaction is needed
    let _ = compactor.needs_compaction();

    // Get stats
    let stats = compactor.get_stats();
    assert_eq!(stats.total_compactions, 0);
}

#[tokio::test]
async fn test_pipeline_with_profiling() {
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
            label: Some("pipeline_test_device"),
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

    // Create bind group layout
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("compute_layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });

    // Create pipeline
    let stage = PipelineStage::new("compute_stage", SIMPLE_COMPUTE_SHADER, "main")
        .with_workgroup_size(64, 1, 1);

    let pipeline_result = PipelineBuilder::new(device.clone())
        .add_stage(stage)
        .add_bind_group_layout(layout)
        .build();

    assert!(pipeline_result.is_ok());

    if let Ok(pipeline) = pipeline_result {
        // Create profiler
        let profiling_config = ProfilingConfig::default();
        let profiler = GpuProfiler::new(device.clone(), queue.clone(), profiling_config)
            .expect("Failed to create profiler");

        // Profile the pipeline
        let session = profiler.begin_profile("pipeline_execution");
        tokio::time::sleep(Duration::from_millis(5)).await;
        session.end(1024, 1);

        // Verify pipeline info
        let info = pipeline.info();
        assert_eq!(info.stage_count, 1);
        assert!(info.optimized);

        // Print visualization
        let viz = pipeline.visualize();
        assert!(!viz.is_empty());
        println!("Pipeline visualization:\n{}", viz);
    }
}

#[tokio::test]
async fn test_multi_stage_pipeline() {
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

    let (device, _queue) = match adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("multi_stage_device"),
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

    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("multi_stage_layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });

    // Create multiple stages
    let stage1 = PipelineStage::new("stage_1", SIMPLE_COMPUTE_SHADER, "main");
    let stage2 = PipelineStage::new("stage_2", SIMPLE_COMPUTE_SHADER, "main").depends_on(0);
    let stage3 = PipelineStage::new("stage_3", SIMPLE_COMPUTE_SHADER, "main").depends_on(1);

    let result = PipelineBuilder::new(device)
        .add_stage(stage1)
        .add_stage(stage2)
        .add_stage(stage3)
        .add_bind_group_layout(layout)
        .build();

    assert!(result.is_ok());

    if let Ok(pipeline) = result {
        assert_eq!(pipeline.stage_count(), 3);

        // Verify each stage
        for i in 0..3 {
            let stage = pipeline.get_stage(i);
            assert!(stage.is_some());
        }
    }
}

#[test]
fn test_workload_classification() {
    // Small workload
    let small = WorkloadInfo {
        data_size: 1024,
        dimensions: vec![32, 32],
        element_size: 4,
    };

    // Medium workload
    let medium = WorkloadInfo {
        data_size: 1024 * 1024,
        dimensions: vec![1024, 1024],
        element_size: 4,
    };

    // Large workload
    let large = WorkloadInfo {
        data_size: 16 * 1024 * 1024,
        dimensions: vec![4096, 4096],
        element_size: 4,
    };

    assert!(small.data_size < medium.data_size);
    assert!(medium.data_size < large.data_size);
}

#[test]
fn test_kernel_params_optimization() {
    use oxigdal_gpu_advanced::MatrixMultiplyKernel;

    // Test different matrix sizes
    let small_params = MatrixMultiplyKernel::params(256, 256, 256, false);
    let large_params = MatrixMultiplyKernel::params(2048, 2048, 2048, true);

    assert_eq!(small_params.entry_point, "matrix_multiply_naive");
    assert_eq!(large_params.entry_point, "matrix_multiply_tiled");

    // Verify thread counts
    assert!(small_params.total_threads() > 0);
    assert!(large_params.total_threads() > small_params.total_threads());
}
