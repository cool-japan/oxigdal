//! Minimal usage example for oxigdal-embedded
//!
//! This example demonstrates basic usage of the embedded crate in a std environment.
//! For actual no_std usage, see the target-specific examples.

use oxigdal_embedded::config::presets;
use oxigdal_embedded::prelude::*;

// Global static memory pool
static POOL: StaticPool<4096> = StaticPool::new();

fn process_coordinates() -> Result<f32> {
    // Create coordinates
    let point1 = MinimalCoordinate::new(10.0, 20.0);
    let point2 = MinimalCoordinate::new(13.0, 24.0);

    // Calculate distance
    let distance = point1.distance_to(&point2);

    // Create bounding box
    let bounds = MinimalBounds::new(0.0, 0.0, 100.0, 100.0);

    // Check containment
    if bounds.contains(&point1) {
        Ok(distance)
    } else {
        Err(EmbeddedError::OutOfBounds { index: 0, max: 0 })
    }
}

fn demonstrate_memory_pool() -> Result<()> {
    // Allocate from static pool
    let _ptr1 = POOL.allocate(256, 8)?;
    let _ptr2 = POOL.allocate(512, 16)?;

    // Check pool usage
    let used = POOL.used();
    let _available = POOL.available();

    // Validate allocation
    if used == 0 {
        return Err(EmbeddedError::AllocationFailed);
    }

    Ok(())
}

#[cfg(feature = "low-power")]
fn demonstrate_power_management() -> Result<()> {
    use oxigdal_embedded::power::{PowerManager, PowerMode};

    let pm = PowerManager::new();

    // Start in balanced mode
    pm.request_mode(PowerMode::Balanced)?;

    // Switch to low power when idle
    pm.request_mode(PowerMode::LowPower)?;

    Ok(())
}

fn demonstrate_sync() -> Result<()> {
    // Atomic counter for statistics
    let counter = AtomicCounter::new(0);
    counter.increment();
    counter.increment();

    if counter.get() != 2 {
        return Err(EmbeddedError::InvalidState);
    }

    Ok(())
}

fn main() {
    // Load platform configuration
    let _config = presets::esp32();

    println!("Running minimal usage example");

    // Process some coordinates
    match process_coordinates() {
        Ok(distance) => {
            println!("Distance: {}", distance);
        }
        Err(e) => {
            eprintln!("Error processing coordinates: {:?}", e);
        }
    }

    // Demonstrate memory pool
    match demonstrate_memory_pool() {
        Ok(()) => println!("Memory pool demo succeeded"),
        Err(e) => eprintln!("Memory pool demo failed: {:?}", e),
    }

    // Demonstrate power management (only with low-power feature)
    #[cfg(feature = "low-power")]
    match demonstrate_power_management() {
        Ok(()) => println!("Power management demo succeeded"),
        Err(e) => eprintln!("Power management demo failed: {:?}", e),
    }

    // Demonstrate synchronization
    match demonstrate_sync() {
        Ok(()) => println!("Sync demo succeeded"),
        Err(e) => eprintln!("Sync demo failed: {:?}", e),
    }

    println!("All demonstrations completed");
}
