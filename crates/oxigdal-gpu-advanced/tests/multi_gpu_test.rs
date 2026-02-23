//! Multi-GPU integration tests.

use oxigdal_gpu_advanced::{MultiGpuManager, SelectionStrategy};

#[tokio::test]
async fn test_multi_gpu_manager_creation() {
    let result = MultiGpuManager::new(SelectionStrategy::LeastLoaded).await;

    match result {
        Ok(manager) => {
            println!("Found {} GPU(s)", manager.gpu_count());
            assert!(manager.gpu_count() > 0);

            manager.print_gpu_info();
        }
        Err(e) => {
            println!("No GPU available (this is OK in CI): {}", e);
        }
    }
}

#[tokio::test]
async fn test_gpu_selection_strategies() {
    let strategies = vec![
        SelectionStrategy::RoundRobin,
        SelectionStrategy::LeastLoaded,
        SelectionStrategy::BestScore,
        SelectionStrategy::Affinity,
    ];

    for strategy in strategies {
        let result = MultiGpuManager::new(strategy).await;

        if let Ok(manager) = result {
            let gpu = manager.select_gpu();
            if let Ok(gpu) = gpu {
                println!("Strategy {:?} selected GPU: {}", strategy, gpu.info.name);
            }
        }
    }
}

#[tokio::test]
async fn test_gpu_workload_tracking() {
    let result = MultiGpuManager::new(SelectionStrategy::LeastLoaded).await;

    if let Ok(manager) = result {
        for gpu in manager.get_all_gpus() {
            gpu.set_workload(0.5);
            assert_eq!(gpu.get_workload(), 0.5);

            gpu.set_workload(1.2); // Should be clamped
            assert_eq!(gpu.get_workload(), 1.0);

            gpu.set_workload(-0.1); // Should be clamped
            assert_eq!(gpu.get_workload(), 0.0);
        }
    }
}

#[tokio::test]
async fn test_load_balancer_stats() {
    let result = MultiGpuManager::new(SelectionStrategy::BestScore).await;

    if let Ok(manager) = result {
        let balancer = manager.get_load_balancer();

        // Simulate some task assignments
        for _ in 0..10 {
            let gpu = manager.select_gpu();
            if let Ok(gpu) = gpu {
                balancer.task_started(gpu.info.index);
                balancer.task_completed(gpu.info.index, 1000);
            }
        }

        balancer.print_stats();
        let stats = balancer.get_stats();
        assert!(stats.tasks_per_device.iter().sum::<usize>() > 0);
    }
}

#[tokio::test]
async fn test_work_queue() {
    let result = MultiGpuManager::new(SelectionStrategy::RoundRobin).await;

    if let Ok(manager) = result {
        if manager.gpu_count() > 0 {
            let queue = manager.get_work_queue(0);

            if let Ok(queue) = queue {
                assert_eq!(queue.pending_count(), 0);
                assert!(queue.is_empty());
            }
        }
    }
}

#[tokio::test]
async fn test_gpu_memory_tracking() {
    let result = MultiGpuManager::new(SelectionStrategy::LeastLoaded).await;

    if let Ok(manager) = result {
        for gpu in manager.get_all_gpus() {
            let initial_usage = gpu.get_memory_usage();
            assert_eq!(initial_usage, 0);

            // Simulate allocation
            gpu.update_memory_usage(1024 * 1024); // 1 MB
            assert_eq!(gpu.get_memory_usage(), 1024 * 1024);

            // Simulate deallocation
            gpu.update_memory_usage(-(512 * 1024)); // -512 KB
            assert_eq!(gpu.get_memory_usage(), 512 * 1024);
        }
    }
}

#[tokio::test]
async fn test_device_capabilities() {
    let result = MultiGpuManager::new(SelectionStrategy::BestScore).await;

    if let Ok(manager) = result {
        for gpu in manager.get_all_gpus() {
            println!("\nGPU {}: {}", gpu.info.index, gpu.info.name);
            println!("  Max buffer size: {} bytes", gpu.info.max_buffer_size);
            println!(
                "  Max texture 2D: {}x{}",
                gpu.info.max_texture_dimension_2d, gpu.info.max_texture_dimension_2d
            );
            println!(
                "  Max workgroup size: {}x{}x{}",
                gpu.info.max_compute_workgroup_size_x,
                gpu.info.max_compute_workgroup_size_y,
                gpu.info.max_compute_workgroup_size_z
            );

            assert!(gpu.info.max_buffer_size > 0);
            assert!(gpu.info.max_texture_dimension_2d > 0);
        }
    }
}
