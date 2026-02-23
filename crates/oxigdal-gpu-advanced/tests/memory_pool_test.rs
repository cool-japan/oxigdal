//! Memory pool integration tests.

use oxigdal_gpu_advanced::MemoryPool;
use std::sync::Arc;
use wgpu::BufferUsages;

async fn create_test_device() -> Option<Arc<wgpu::Device>> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        })
        .await
        .ok()?;

    let (device, _queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("Test Device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::Off,
        })
        .await
        .ok()?;

    Some(Arc::new(device))
}

#[tokio::test]
async fn test_memory_pool_creation() {
    if let Some(device) = create_test_device().await {
        let pool_size = 1024 * 1024; // 1 MB
        let pool = MemoryPool::new(
            device,
            pool_size,
            BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
        );

        assert!(pool.is_ok());

        if let Ok(pool) = pool {
            assert_eq!(pool.get_available_memory(), pool_size);
            assert_eq!(pool.get_current_usage(), 0);
        }
    } else {
        println!("No GPU available, skipping test");
    }
}

#[tokio::test]
async fn test_memory_allocation() {
    if let Some(device) = create_test_device().await {
        let pool_size = 1024 * 1024; // 1 MB
        let pool = MemoryPool::new(
            device,
            pool_size,
            BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
        );

        if let Ok(pool) = pool {
            let pool = Arc::new(pool);

            // Allocate 256 KB
            let alloc = pool.allocate(256 * 1024, 256);
            assert!(alloc.is_ok());

            if let Ok(alloc) = alloc {
                assert_eq!(alloc.size(), 256 * 1024);
                assert!(alloc.offset() < pool_size);

                let stats = pool.get_stats();
                assert_eq!(stats.current_usage, 256 * 1024);
                assert!(stats.available < pool_size);
            }
        }
    } else {
        println!("No GPU available, skipping test");
    }
}

#[tokio::test]
async fn test_memory_deallocation() {
    if let Some(device) = create_test_device().await {
        let pool_size = 1024 * 1024; // 1 MB
        let pool = MemoryPool::new(
            device,
            pool_size,
            BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
        );

        if let Ok(pool) = pool {
            let pool = Arc::new(pool);

            {
                let _alloc = pool.allocate(256 * 1024, 256);
                let stats = pool.get_stats();
                assert_eq!(stats.current_usage, 256 * 1024);
            }

            // Allocation should be dropped and memory freed
            let stats = pool.get_stats();
            assert_eq!(stats.current_usage, 0);
            assert_eq!(stats.available, pool_size);
        }
    } else {
        println!("No GPU available, skipping test");
    }
}

#[tokio::test]
async fn test_multiple_allocations() {
    if let Some(device) = create_test_device().await {
        let pool_size = 1024 * 1024; // 1 MB
        let pool = MemoryPool::new(
            device,
            pool_size,
            BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
        );

        if let Ok(pool) = pool {
            let pool = Arc::new(pool);

            let alloc1 = pool.allocate(256 * 1024, 256);
            let alloc2 = pool.allocate(256 * 1024, 256);
            let alloc3 = pool.allocate(256 * 1024, 256);

            assert!(alloc1.is_ok());
            assert!(alloc2.is_ok());
            assert!(alloc3.is_ok());

            let stats = pool.get_stats();
            assert_eq!(stats.current_usage, 3 * 256 * 1024);
        }
    } else {
        println!("No GPU available, skipping test");
    }
}

#[tokio::test]
async fn test_memory_pool_stats() {
    if let Some(device) = create_test_device().await {
        let pool_size = 1024 * 1024; // 1 MB
        let pool = MemoryPool::new(
            device,
            pool_size,
            BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
        );

        if let Ok(pool) = pool {
            let pool = Arc::new(pool);

            let _alloc1 = pool.allocate(256 * 1024, 256);
            let _alloc2 = pool.allocate(128 * 1024, 256);

            pool.print_stats();

            let stats = pool.get_stats();
            assert_eq!(stats.allocation_count, 2);
            assert_eq!(stats.deallocation_count, 0);
            assert!(stats.current_usage > 0);
            assert!(stats.available > 0);
        }
    } else {
        println!("No GPU available, skipping test");
    }
}

#[tokio::test]
async fn test_allocation_failure() {
    if let Some(device) = create_test_device().await {
        let pool_size = 1024 * 1024; // 1 MB
        let pool = MemoryPool::new(
            device,
            pool_size,
            BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
        );

        if let Ok(pool) = pool {
            let pool = Arc::new(pool);

            // Try to allocate more than pool size
            let result = pool.allocate(2 * pool_size, 256);
            assert!(result.is_err());
        }
    } else {
        println!("No GPU available, skipping test");
    }
}
