//! Integration tests for oxigdal-embedded
//!
//! Tests the interaction between different modules

#![cfg(test)]

use crate::buffer::FixedBuffer;
use crate::config::presets;
use crate::error::{EmbeddedError, Result};
use crate::memory_pool::{BlockPool, MemoryPool, StaticPool};
use crate::minimal::{MinimalBounds, MinimalCoordinate, MinimalFeature, FeatureType};
use crate::power::{PowerManager, PowerMode};
use crate::realtime::{Deadline, PeriodicTask, Priority, RealtimeScheduler};
use crate::sync::{AtomicCounter, Mutex, Semaphore};

#[test]
fn test_full_workflow() {
    // Configuration
    let config = presets::esp32();
    assert!(config.validate().is_ok());

    // Memory pool
    let pool = StaticPool::<4096>::new();
    let ptr = pool.allocate(256, 8).expect("allocation failed");
    assert!(pool.used() > 0);

    // Coordinates
    let coord1 = MinimalCoordinate::new(10.0, 20.0);
    let coord2 = MinimalCoordinate::new(13.0, 24.0);
    let distance = coord1.distance_to(&coord2);
    assert!(distance > 0.0);

    // Bounds checking
    let bounds = MinimalBounds::new(0.0, 0.0, 100.0, 100.0);
    assert!(bounds.contains(&coord1));
    assert_eq!(bounds.area(), 10000.0);

    // Feature creation
    let mut feature = MinimalFeature::<16>::new(FeatureType::Line);
    feature.add_point(coord1).expect("add failed");
    feature.add_point(coord2).expect("add failed");
    let length = feature.length();
    assert!(length > 0.0);
}

#[test]
fn test_memory_pool_workflow() {
    // Static pool
    let static_pool = StaticPool::<8192>::new();
    let ptr1 = static_pool.allocate(1024, 16).expect("allocation failed");
    let ptr2 = static_pool.allocate(2048, 32).expect("allocation failed");
    assert_ne!(ptr1, ptr2);

    // Block pool
    let block_pool = BlockPool::<128, 32>::new();
    let block1 = block_pool.allocate(64, 8).expect("allocation failed");
    let block2 = block_pool.allocate(64, 8).expect("allocation failed");
    assert_ne!(block1, block2);

    // Deallocate blocks
    unsafe {
        block_pool
            .deallocate(block1, 64, 8)
            .expect("deallocation failed");
        block_pool
            .deallocate(block2, 64, 8)
            .expect("deallocation failed");
    }

    assert_eq!(block_pool.used(), 0);
}

#[test]
fn test_power_realtime_integration() {
    // Power management
    let pm = PowerManager::new();
    pm.request_mode(PowerMode::Balanced)
        .expect("mode change failed");
    assert_eq!(pm.current_mode(), PowerMode::Balanced);

    // Real-time scheduler
    let scheduler = RealtimeScheduler::new(240);
    scheduler.init();

    let deadline = Deadline::hard(1000);
    scheduler
        .execute_with_deadline(deadline, || {
            // Fast operation
            let _ = 1 + 1;
        })
        .expect("deadline missed");
}

#[test]
fn test_synchronization_primitives() {
    // Atomic counter
    let counter = AtomicCounter::new(0);
    counter.increment();
    counter.increment();
    assert_eq!(counter.get(), 2);

    // Mutex
    let mutex = Mutex::new(vec![1, 2, 3]);
    {
        let guard = mutex.lock();
        assert_eq!(guard.len(), 3);
    }

    // Semaphore
    let sem = Semaphore::new(3);
    sem.try_acquire().expect("acquire failed");
    sem.try_acquire().expect("acquire failed");
    assert_eq!(sem.count(), 1);
    sem.release();
    assert_eq!(sem.count(), 2);
}

#[test]
fn test_buffer_operations() {
    // Fixed buffer
    let mut buffer = FixedBuffer::<u32, 16>::new();
    buffer.push(1).expect("push failed");
    buffer.push(2).expect("push failed");
    buffer.push(3).expect("push failed");

    assert_eq!(buffer.len(), 3);
    assert_eq!(*buffer.get(0).expect("get failed"), 1);
    assert_eq!(*buffer.get(2).expect("get failed"), 3);

    let value = buffer.pop().expect("pop failed");
    assert_eq!(value, 3);
    assert_eq!(buffer.len(), 2);
}

#[test]
fn test_minimal_geospatial_operations() {
    // Create a square polygon
    let mut polygon = MinimalFeature::<16>::new(FeatureType::Polygon);
    polygon
        .add_point(MinimalCoordinate::new(0.0, 0.0))
        .expect("add failed");
    polygon
        .add_point(MinimalCoordinate::new(10.0, 0.0))
        .expect("add failed");
    polygon
        .add_point(MinimalCoordinate::new(10.0, 10.0))
        .expect("add failed");
    polygon
        .add_point(MinimalCoordinate::new(0.0, 10.0))
        .expect("add failed");

    let area = polygon.area().expect("area calculation failed");
    assert!((area - 100.0).abs() < 0.1);

    let bounds = polygon.bounds().expect("bounds calculation failed");
    assert_eq!(bounds.width(), 10.0);
    assert_eq!(bounds.height(), 10.0);
}

#[test]
fn test_error_handling() {
    // Test various error conditions
    let pool = StaticPool::<128>::new();

    // Exhaust the pool
    let _ptr1 = pool.allocate(64, 8).expect("allocation failed");
    let _ptr2 = pool.allocate(64, 8).expect("allocation failed");

    // This should fail
    let result = pool.allocate(64, 8);
    assert!(matches!(result, Err(EmbeddedError::PoolExhausted)));

    // Test invalid parameter
    let result = pool.allocate(0, 8);
    assert!(matches!(result, Err(EmbeddedError::InvalidParameter)));

    // Test buffer overflow
    let mut buffer = FixedBuffer::<u32, 2>::new();
    buffer.push(1).expect("push failed");
    buffer.push(2).expect("push failed");
    let result = buffer.push(3);
    assert!(matches!(result, Err(EmbeddedError::BufferTooSmall { .. })));
}

#[test]
fn test_configuration_presets() {
    // Test various presets
    let esp32_config = presets::esp32();
    assert!(esp32_config.validate().is_ok());
    assert_eq!(esp32_config.system.cpu_freq_mhz, 240);

    let cortex_config = presets::cortex_m4();
    assert!(cortex_config.validate().is_ok());
    assert_eq!(cortex_config.system.cpu_freq_mhz, 168);

    let riscv_config = presets::riscv();
    assert!(riscv_config.validate().is_ok());
    assert_eq!(riscv_config.system.cpu_freq_mhz, 100);

    let ulp_config = presets::ultra_low_power();
    assert!(ulp_config.validate().is_ok());
    assert_eq!(ulp_config.system.power_mode, PowerMode::UltraLowPower);
}
