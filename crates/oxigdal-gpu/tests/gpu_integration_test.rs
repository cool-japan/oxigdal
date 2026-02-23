//! Comprehensive integration tests for GPU operations.

use oxigdal_gpu::*;
use wgpu::BufferUsages;

// Helper function to create GPU context
async fn create_context() -> Option<GpuContext> {
    GpuContext::new().await.ok()
}

// Memory management tests
mod memory_tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_pool_allocation() {
        let Some(context) = create_context().await else {
            return;
        };

        let config = MemoryPoolConfig::default();
        let mut pool = MemoryPool::new(&context, config).expect("Failed to create pool");

        // Test allocation
        let alloc1 = pool.allocate(1024, 256).expect("Failed to allocate");
        assert_eq!(pool.stats().bytes_in_use, 1024);

        // Test multiple allocations
        let alloc2 = pool.allocate(2048, 256).expect("Failed to allocate");
        assert!(pool.stats().bytes_in_use >= 3072);

        // Test freeing
        pool.free(alloc1).expect("Failed to free");
        assert!(pool.stats().bytes_in_use >= 2048);

        pool.free(alloc2).expect("Failed to free");
    }

    #[tokio::test]
    async fn test_memory_pool_expansion() {
        let Some(context) = create_context().await else {
            return;
        };

        let config = MemoryPoolConfig {
            initial_size: 1024,
            max_size: 4096,
            ..Default::default()
        };

        let mut pool = MemoryPool::new(&context, config).expect("Failed to create pool");

        // Allocate more than initial size
        let alloc = pool.allocate(2048, 256).expect("Failed to allocate");

        assert!(pool.stats().total_allocated >= 2048);
        assert!(pool.stats().num_expansions > 0);

        pool.free(alloc).expect("Failed to free");
    }

    #[tokio::test]
    async fn test_staging_buffer_manager() {
        let Some(context) = create_context().await else {
            return;
        };

        let mut manager = StagingBufferManager::new(&context, 1024, 5);

        // Test upload buffer
        let upload = manager
            .get_upload_buffer()
            .expect("Failed to get upload buffer");
        manager.return_upload_buffer(upload);

        // Test download buffer
        let download = manager
            .get_download_buffer()
            .expect("Failed to get download buffer");
        manager.return_download_buffer(download);

        // Test reuse
        let upload2 = manager
            .get_upload_buffer()
            .expect("Failed to get upload buffer");
        manager.return_upload_buffer(upload2);
    }

    #[tokio::test]
    async fn test_vram_budget_manager() {
        let manager = VramBudgetManager::new(1024);

        let id1 = manager.allocate(512).expect("Failed to allocate");
        assert_eq!(manager.allocated(), 512);
        assert_eq!(manager.utilization(), 50.0);

        let id2 = manager.allocate(256).expect("Failed to allocate");
        assert_eq!(manager.allocated(), 768);

        // Should fail - exceeds budget
        assert!(manager.allocate(512).is_err());

        manager.free(id1).expect("Failed to free");
        assert_eq!(manager.allocated(), 256);

        manager.free(id2).expect("Failed to free");
        assert_eq!(manager.allocated(), 0);
    }
}

// Buffer tests
mod buffer_tests {
    use super::*;

    #[tokio::test]
    async fn test_gpu_buffer_creation() {
        let Some(context) = create_context().await else {
            return;
        };

        let buffer: GpuBuffer<f32> =
            GpuBuffer::new(&context, 1024, BufferUsages::STORAGE).expect("Failed to create buffer");

        assert_eq!(buffer.len(), 1024);
        assert!(!buffer.is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn test_gpu_buffer_write_read() {
        let Some(context) = create_context().await else {
            return;
        };

        let data: Vec<f32> = (0..100).map(|i| i as f32).collect();

        let buffer = GpuBuffer::from_data(
            &context,
            &data,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )
        .expect("Failed to create buffer");

        // Create staging buffer for reading
        let mut staging =
            GpuBuffer::staging(&context, 100).expect("Failed to create staging buffer");

        staging.copy_from(&buffer).expect("Failed to copy");

        let result = staging.read().await.expect("Failed to read");

        assert_eq!(result.len(), data.len());
        for (a, b) in result.iter().zip(data.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[tokio::test]
    async fn test_gpu_raster_buffer() {
        let Some(context) = create_context().await else {
            return;
        };

        let width = 64;
        let height = 64;
        let num_bands = 3;

        let raster: GpuRasterBuffer<f32> =
            GpuRasterBuffer::new(&context, width, height, num_bands, BufferUsages::STORAGE)
                .expect("Failed to create raster buffer");

        assert_eq!(raster.width(), width);
        assert_eq!(raster.height(), height);
        assert_eq!(raster.num_bands(), num_bands);
    }

    #[tokio::test]
    async fn test_raster_buffer_from_bands() {
        let Some(context) = create_context().await else {
            return;
        };

        let width = 32;
        let height = 32;
        let bands_data: Vec<Vec<f32>> = vec![
            vec![1.0; (width * height) as usize],
            vec![2.0; (width * height) as usize],
            vec![3.0; (width * height) as usize],
        ];

        let raster = GpuRasterBuffer::from_bands(
            &context,
            width,
            height,
            &bands_data,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )
        .expect("Failed to create raster from bands");

        assert_eq!(raster.num_bands(), 3);
    }
}

// Compute pipeline tests
mod pipeline_tests {
    use super::*;

    #[tokio::test]
    async fn test_compute_pipeline_creation() {
        let Some(context) = create_context().await else {
            return;
        };

        let data: Vec<f32> = vec![1.0; 100];

        let pipeline =
            ComputePipeline::from_data(&context, &data, 10, 10).expect("Failed to create pipeline");

        assert_eq!(pipeline.dimensions(), (10, 10));
    }

    #[tokio::test]
    #[ignore]
    async fn test_pipeline_scalar_operations() {
        let Some(context) = create_context().await else {
            return;
        };

        let data: Vec<f32> = vec![2.0; 100];

        let result = ComputePipeline::from_data(&context, &data, 10, 10)
            .expect("Failed to create pipeline")
            .add(3.0)
            .expect("Failed to add")
            .multiply(2.0)
            .expect("Failed to multiply")
            .read_blocking()
            .expect("Failed to read");

        // Should be (2.0 + 3.0) * 2.0 = 10.0
        for val in result {
            assert!((val - 10.0).abs() < 1e-5);
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_pipeline_unary_operations() {
        let Some(context) = create_context().await else {
            return;
        };

        let data: Vec<f32> = vec![4.0; 100];

        let result = ComputePipeline::from_data(&context, &data, 10, 10)
            .expect("Failed to create pipeline")
            .sqrt()
            .expect("Failed to sqrt")
            .read_blocking()
            .expect("Failed to read");

        // Should be sqrt(4.0) = 2.0
        for val in result {
            assert!((val - 2.0).abs() < 1e-5);
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_pipeline_chaining() {
        let Some(context) = create_context().await else {
            return;
        };

        let data: Vec<f32> = vec![1.0; 64 * 64];

        let result = ComputePipeline::from_data(&context, &data, 64, 64)
            .expect("Failed to create pipeline")
            .add(10.0)
            .and_then(|p| p.multiply(2.0))
            .and_then(|p| p.clamp(0.0, 100.0))
            .expect("Failed to chain operations")
            .read_blocking()
            .expect("Failed to read");

        // Should be clamp((1.0 + 10.0) * 2.0, 0.0, 100.0) = 22.0
        for val in result {
            assert!((val - 22.0).abs() < 1e-5);
        }
    }
}

// Multi-band pipeline tests
mod multiband_tests {
    use super::*;

    #[tokio::test]
    async fn test_multiband_pipeline_creation() {
        let Some(context) = create_context().await else {
            return;
        };

        let bands_data: Vec<Vec<f32>> =
            vec![vec![1.0; 64 * 64], vec![2.0; 64 * 64], vec![3.0; 64 * 64]];

        let raster = GpuRasterBuffer::from_bands(
            &context,
            64,
            64,
            &bands_data,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )
        .expect("Failed to create raster");

        let pipeline =
            MultibandPipeline::new(&context, &raster).expect("Failed to create multiband pipeline");

        assert_eq!(pipeline.num_bands(), 3);
    }

    #[tokio::test]
    #[ignore]
    async fn test_ndvi_computation() {
        let Some(context) = create_context().await else {
            return;
        };

        // Create test data: R, G, B, NIR
        let red = vec![50.0; 32 * 32];
        let green = vec![60.0; 32 * 32];
        let blue = vec![40.0; 32 * 32];
        let nir = vec![80.0; 32 * 32];

        let bands_data = vec![red, green, blue, nir];

        let raster = GpuRasterBuffer::from_bands(
            &context,
            32,
            32,
            &bands_data,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )
        .expect("Failed to create raster");

        let pipeline =
            MultibandPipeline::new(&context, &raster).expect("Failed to create pipeline");

        let ndvi_result = pipeline.ndvi().expect("Failed to compute NDVI");

        let ndvi_values = ndvi_result.read_blocking().expect("Failed to read NDVI");

        // NDVI = (NIR - Red) / (NIR + Red) = (80 - 50) / (80 + 50) = 30/130 ≈ 0.23
        for val in ndvi_values {
            assert!((val - 0.23_f32).abs() < 0.05_f32);
        }
    }
}

// Backend tests
mod backend_tests {
    use super::*;

    #[tokio::test]
    async fn test_backend_detection() {
        let Some(context) = create_context().await else {
            return;
        };

        let backend = context.backend();
        println!("Detected backend: {:?}", backend);

        let capabilities = backends::query_capabilities(backend);
        println!("Backend capabilities: {:?}", capabilities);
    }

    #[tokio::test]
    async fn test_adapter_info() {
        let Some(context) = create_context().await else {
            return;
        };

        let info = context.adapter_info();
        println!("GPU: {}", info.name);
        println!("Backend: {:?}", info.backend);
        println!("Device type: {:?}", info.device_type);
    }
}

// Error handling tests
mod error_tests {
    use super::*;

    #[tokio::test]
    async fn test_invalid_buffer_size() {
        let Some(context) = create_context().await else {
            return;
        };

        let data: Vec<f32> = vec![1.0; 100];

        // Wrong dimensions
        let result = ComputePipeline::from_data(&context, &data, 5, 5);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_vram_budget_exceeded() {
        let manager = VramBudgetManager::new(100);

        let result = manager.allocate(200);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_memory_pool_exhausted() {
        let Some(context) = create_context().await else {
            return;
        };

        let config = MemoryPoolConfig {
            initial_size: 1024,
            max_size: 2048,
            ..Default::default()
        };

        let mut pool = MemoryPool::new(&context, config).expect("Failed to create pool");

        // Allocate beyond max size
        let result = pool.allocate(4096, 256);
        assert!(result.is_err());
    }
}

// Integration tests
mod integration_tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_end_to_end_raster_processing() {
        let Some(context) = create_context().await else {
            return;
        };

        // Create test raster
        let width = 128;
        let height = 128;
        let data: Vec<f32> = (0..(width * height)).map(|i| i as f32).collect();

        // Process through pipeline
        let result = ComputePipeline::from_data(&context, &data, width as u32, height as u32)
            .expect("Failed to create pipeline")
            .add(100.0)
            .and_then(|p| p.multiply(0.5))
            .and_then(|p| p.clamp(0.0, 1000.0))
            .expect("Failed to process")
            .read_blocking()
            .expect("Failed to read result");

        assert_eq!(result.len(), (width * height) as usize);

        // Verify first few values
        for (i, val) in result.iter().take(10).enumerate() {
            let expected = ((i as f32 + 100.0) * 0.5).clamp(0.0, 1000.0);
            assert!((val - expected).abs() < 1e-3);
        }
    }

    #[tokio::test]
    async fn test_multiband_raster_workflow() {
        let Some(context) = create_context().await else {
            return;
        };

        let width = 64;
        let height = 64;
        let num_pixels = (width * height) as usize;

        // Create RGB bands
        let red = vec![100.0; num_pixels];
        let green = vec![150.0; num_pixels];
        let blue = vec![50.0; num_pixels];

        let bands_data = vec![red, green, blue];

        let raster = GpuRasterBuffer::from_bands(
            &context,
            width,
            height,
            &bands_data,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )
        .expect("Failed to create raster");

        // Process all bands
        let pipeline =
            MultibandPipeline::new(&context, &raster).expect("Failed to create pipeline");

        let result = pipeline
            .map(|band| band.multiply(1.5).and_then(|b| b.clamp(0.0, 255.0)))
            .expect("Failed to map bands");

        let processed_bands = result.finish();
        assert_eq!(processed_bands.len(), 3);
    }
}

// Performance tests
mod performance_tests {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    async fn test_large_raster_performance() {
        let Some(context) = create_context().await else {
            return;
        };

        let width = 2048;
        let height = 2048;
        let data: Vec<f32> = vec![1.0; (width * height) as usize];

        let start = Instant::now();

        let _result = ComputePipeline::from_data(&context, &data, width, height)
            .expect("Failed to create pipeline")
            .multiply(2.0)
            .and_then(|p| p.add(5.0))
            .expect("Failed to process");

        let elapsed = start.elapsed();
        println!("Processed {}x{} raster in {:?}", width, height, elapsed);

        // Should complete in reasonable time (< 1 second)
        assert!(elapsed.as_secs() < 1);
    }
}
