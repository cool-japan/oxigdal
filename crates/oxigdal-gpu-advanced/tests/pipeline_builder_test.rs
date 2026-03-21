//! Tests for GPU pipeline builder

use oxigdal_gpu_advanced::{PipelineBuilder, PipelineConfig, PipelineStage};
use std::sync::Arc;

const TEST_SHADER: &str = r#"
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    // Simple test shader
}
"#;

#[tokio::test]
async fn test_pipeline_stage_creation() {
    let stage = PipelineStage::new("test_stage", TEST_SHADER, "main")
        .with_workgroup_size(16, 16, 1)
        .with_bind_groups(2);

    assert_eq!(stage.label, "test_stage");
    assert_eq!(stage.workgroup_size, (16, 16, 1));
    assert_eq!(stage.bind_group_count, 2);
}

#[tokio::test]
async fn test_pipeline_builder() {
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

    // Create bind group layout
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("test_layout"),
        entries: &[],
    });

    let stage = PipelineStage::new("test_stage", TEST_SHADER, "main");

    let result = PipelineBuilder::new(device)
        .add_stage(stage)
        .add_bind_group_layout(layout)
        .build();

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_pipeline_validation() {
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

    // Try to build pipeline without stages
    let result = PipelineBuilder::new(device).build();
    assert!(result.is_err());
}

#[tokio::test]
async fn test_pipeline_with_dependencies() {
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

    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("test_layout"),
        entries: &[],
    });

    let stage1 = PipelineStage::new("stage1", TEST_SHADER, "main");
    let stage2 = PipelineStage::new("stage2", TEST_SHADER, "main").depends_on(0);

    let result = PipelineBuilder::new(device.clone())
        .add_stage(stage1)
        .add_stage(stage2)
        .add_bind_group_layout(layout)
        .build();

    assert!(result.is_ok());

    if let Ok(pipeline) = result {
        assert_eq!(pipeline.stage_count(), 2);
        println!("{}", pipeline.visualize());
    }
}

#[test]
fn test_pipeline_config() {
    let config = PipelineConfig::default();
    assert!(config.optimize);
    assert!(config.cache);
    assert_eq!(config.max_stages, 16);

    let custom_config = PipelineConfig {
        optimize: false,
        cache: false,
        max_stages: 8,
    };
    assert!(!custom_config.optimize);
}
