# oxigdal-embedded

Embedded systems support for OxiGDAL, providing no_std compatible geospatial processing for ARM, RISC-V, ESP32, and other embedded targets.

## Features

- **no_std Compatibility**: Full support for bare-metal embedded systems
- **Static Memory Pools**: Predictable allocation behavior without heap
- **Target-Specific Optimizations**: ARM, RISC-V, and ESP32 support
- **Power Management**: Low-power operation modes and sleep states
- **Real-Time Support**: Deadline scheduling and watchdog timers
- **Minimal Footprint**: Lightweight geospatial primitives for constrained environments
- **Pure Rust**: No C/Fortran dependencies (COOLJAPAN policy)

## Supported Targets

### ARM
- ARM Cortex-M (M0, M0+, M3, M4, M7)
- ARM Cortex-A
- NEON SIMD support
- Hardware AES and CRC

### RISC-V
- RV32I, RV64I
- Vector extension support
- Atomic operations
- Cache management

### ESP32
- ESP32 (Xtensa LX6)
- ESP32-S2, ESP32-S3 (Xtensa LX7)
- ESP32-C3, ESP32-C6, ESP32-H2 (RISC-V)
- WiFi power management
- Hardware crypto acceleration

## Modules

- **alloc_utils**: Custom allocators (bump, stack, arena)
- **memory_pool**: Static and block-based memory pools
- **target**: Target-specific optimizations
- **power**: Power management and voltage regulation
- **realtime**: Real-time scheduling and deadline management
- **minimal**: Minimal geospatial primitives
- **buffer**: Fixed-size and ring buffers
- **sync**: Synchronization primitives (mutex, semaphore, etc.)
- **config**: System configuration and presets

## Usage

### Basic Setup

```rust
#![no_std]

use oxigdal_embedded::prelude::*;
use oxigdal_embedded::config::presets;

// Use a platform preset
let config = presets::esp32();

// Create a static memory pool
static POOL: StaticPool<4096> = StaticPool::new();

fn main() {
    // Allocate from the pool
    let ptr = POOL.allocate(256, 8).expect("allocation failed");

    // Use minimal geospatial types
    let coord = MinimalCoordinate::new(10.5, 20.3);
    let bounds = MinimalBounds::new(0.0, 0.0, 100.0, 100.0);

    if bounds.contains(&coord) {
        // Process coordinate
    }
}
```

### Power Management

```rust
use oxigdal_embedded::power::{PowerManager, PowerMode};

let pm = PowerManager::new();

// Switch to low power mode
pm.request_mode(PowerMode::LowPower).expect("mode change failed");

// Estimate battery life
use oxigdal_embedded::power::PowerEstimator;

let mut estimator = PowerEstimator::new(3300); // 3.3V
estimator.set_current(100); // 100mA
let hours = estimator.battery_life_hours(2000); // 2000mAh battery
```

### Real-Time Scheduling

```rust
use oxigdal_embedded::realtime::{RealtimeScheduler, Deadline};

let scheduler = RealtimeScheduler::new(240); // 240MHz CPU
scheduler.init();

let deadline = Deadline::hard(1000); // 1ms deadline

scheduler.execute_with_deadline(deadline, || {
    // Critical task that must complete within 1ms
}).expect("deadline missed");
```

### Memory Pools

```rust
use oxigdal_embedded::memory_pool::{StaticPool, BlockPool, MemoryPool};

// Static bump allocator
let static_pool = StaticPool::<65536>::new();
let ptr = static_pool.allocate(1024, 16).expect("allocation failed");

// Block pool for frequent allocations
let block_pool = BlockPool::<256, 64>::new();
let block = block_pool.allocate(128, 8).expect("allocation failed");

// Deallocate
unsafe {
    block_pool.deallocate(block, 128, 8).expect("deallocation failed");
}
```

### Synchronization

```rust
use oxigdal_embedded::sync::{Mutex, Semaphore, AtomicCounter};

// Thread-safe counter
let counter = AtomicCounter::new(0);
counter.increment();

// Mutex for shared data
let mutex = Mutex::new(42);
{
    let mut guard = mutex.lock();
    *guard = 100;
}

// Semaphore for resource counting
let sem = Semaphore::new(5);
sem.try_acquire().expect("acquire failed");
sem.release();
```

## Configuration Presets

The crate provides pre-configured settings for common platforms:

```rust
use oxigdal_embedded::config::presets;

// ESP32 (240MHz, 520KB RAM)
let config = presets::esp32();

// ESP32-C3 RISC-V (160MHz, 400KB RAM)
let config = presets::esp32c3();

// ARM Cortex-M4 (168MHz, 192KB RAM)
let config = presets::cortex_m4();

// Generic RISC-V (100MHz, 128KB RAM)
let config = presets::riscv();

// Ultra-low-power (32MHz, 64KB RAM)
let config = presets::ultra_low_power();
```

## Features Flags

- `default`: Includes `alloc` feature
- `std`: Enable standard library (disables no_std)
- `alloc`: Enable allocator support
- `arm`: ARM-specific optimizations
- `riscv`: RISC-V-specific optimizations
- `esp32`: ESP32-specific features
- `low-power`: Power management features
- `realtime`: Real-time scheduling support
- `minimal`: Minimal feature set only

## Memory Requirements

### Minimal Configuration
- Code: ~20KB flash
- Static pool: 4KB RAM
- Block pool: 4KB RAM
- Total: ~28KB

### Standard Configuration
- Code: ~40KB flash
- Static pool: 64KB RAM
- Block pool: 64KB RAM
- Total: ~168KB

### High-Performance Configuration
- Code: ~60KB flash
- Static pool: 128KB RAM
- Block pool: 64KB RAM
- Total: ~252KB

## Performance

Benchmarks on ESP32 (240MHz):

- Memory allocation: ~50ns (static pool)
- Memory deallocation: ~10ns (block pool)
- Coordinate distance: ~200ns
- Bounds intersection: ~100ns
- Power mode switch: ~5μs

## Safety

All operations follow strict safety guidelines:

- No unwrap() calls (enforced by clippy)
- Bounds checking on all array accesses
- Validated memory alignment
- Lock-free atomic operations where possible
- Minimal unsafe code with documented safety invariants

## License

Licensed under Apache-2.0.

Copyright (c) 2024-2026 COOLJAPAN OU (Team Kitasan)

## Contributing

This crate is part of the OxiGDAL project. See the main repository for contribution guidelines.

## See Also

- [oxigdal-core](../oxigdal-core) - Core abstractions
- [oxigdal-mobile](../oxigdal-mobile) - Mobile platform support
- [oxigdal-wasm](../oxigdal-wasm) - WebAssembly support
