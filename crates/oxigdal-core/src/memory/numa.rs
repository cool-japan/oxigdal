//! NUMA-Aware Memory Allocation
//!
//! This module provides NUMA (Non-Uniform Memory Access) aware memory allocation:
//! - NUMA node detection
//! - Local allocation preference
//! - NUMA interleaving for large buffers
//! - Migration hints
//! - NUMA metrics (local vs remote access)

// Unsafe code is necessary for NUMA allocations
#![allow(unsafe_code)]

use crate::error::{OxiGdalError, Result};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

// mbind and MPOL_* constants for Linux NUMA support
#[cfg(target_os = "linux")]
const MPOL_BIND: libc::c_int = 2;
#[cfg(target_os = "linux")]
const MPOL_INTERLEAVE: libc::c_int = 3;
#[cfg(target_os = "linux")]
const MPOL_PREFERRED: libc::c_int = 1;

// System call number for mbind (varies by architecture)
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const SYS_MBIND: libc::c_long = 237;

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const SYS_MBIND: libc::c_long = 235;

#[cfg(all(
    target_os = "linux",
    not(any(target_arch = "x86_64", target_arch = "aarch64"))
))]
const SYS_MBIND: libc::c_long = 0; // Unsupported, will fail at runtime

// Wrapper for mbind syscall
#[cfg(target_os = "linux")]
unsafe fn mbind(
    addr: *mut libc::c_void,
    len: libc::size_t,
    mode: libc::c_int,
    nodemask: *const libc::c_ulong,
    maxnode: libc::c_ulong,
    flags: libc::c_uint,
) -> libc::c_long {
    // SAFETY: mbind is a valid Linux system call. The caller must ensure
    // that addr and nodemask point to valid memory regions.
    unsafe { libc::syscall(SYS_MBIND, addr, len, mode, nodemask, maxnode, flags) }
}

/// NUMA node identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NumaNode(pub i32);

impl NumaNode {
    /// Any NUMA node
    pub const ANY: Self = Self(-1);

    /// Create a NUMA node
    #[must_use]
    pub fn new(id: i32) -> Self {
        Self(id)
    }

    /// Get node ID
    #[must_use]
    pub fn id(&self) -> i32 {
        self.0
    }
}

/// NUMA allocation policy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumaPolicy {
    /// Default policy (typically local)
    Default,
    /// Bind to specific node
    Bind(NumaNode),
    /// Interleave across all nodes
    Interleave,
    /// Prefer specific node but allow others
    Prefer(NumaNode),
}

/// NUMA configuration
#[derive(Debug, Clone)]
pub struct NumaConfig {
    /// Allocation policy
    pub policy: NumaPolicy,
    /// Enable NUMA awareness
    pub enabled: bool,
    /// Track statistics
    pub track_stats: bool,
}

impl Default for NumaConfig {
    fn default() -> Self {
        Self {
            policy: NumaPolicy::Default,
            enabled: is_numa_available(),
            track_stats: true,
        }
    }
}

impl NumaConfig {
    /// Create new configuration
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set policy
    #[must_use]
    pub fn with_policy(mut self, policy: NumaPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Enable NUMA awareness
    #[must_use]
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Enable statistics tracking
    #[must_use]
    pub fn with_stats(mut self, track: bool) -> Self {
        self.track_stats = track;
        self
    }
}

/// NUMA statistics
#[derive(Debug, Default)]
pub struct NumaStats {
    /// Local allocations
    pub local_allocations: AtomicU64,
    /// Remote allocations
    pub remote_allocations: AtomicU64,
    /// Interleaved allocations
    pub interleaved_allocations: AtomicU64,
    /// Migration operations
    pub migrations: AtomicU64,
    /// Total bytes allocated per node
    pub bytes_per_node: Vec<AtomicU64>,
}

impl NumaStats {
    /// Create new statistics
    #[must_use]
    pub fn new(num_nodes: usize) -> Self {
        let mut bytes_per_node = Vec::new();
        for _ in 0..num_nodes {
            bytes_per_node.push(AtomicU64::new(0));
        }

        Self {
            local_allocations: AtomicU64::new(0),
            remote_allocations: AtomicU64::new(0),
            interleaved_allocations: AtomicU64::new(0),
            migrations: AtomicU64::new(0),
            bytes_per_node,
        }
    }

    /// Record a local allocation
    pub fn record_local(&self) {
        self.local_allocations.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a remote allocation
    pub fn record_remote(&self) {
        self.remote_allocations.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an interleaved allocation
    pub fn record_interleaved(&self) {
        self.interleaved_allocations.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a migration
    pub fn record_migration(&self) {
        self.migrations.fetch_add(1, Ordering::Relaxed);
    }

    /// Record bytes allocated on a node
    pub fn record_bytes(&self, node: usize, bytes: u64) {
        if node < self.bytes_per_node.len() {
            self.bytes_per_node[node].fetch_add(bytes, Ordering::Relaxed);
        }
    }

    /// Get local allocation percentage
    pub fn local_percentage(&self) -> f64 {
        let local = self.local_allocations.load(Ordering::Relaxed);
        let remote = self.remote_allocations.load(Ordering::Relaxed);
        let total = local + remote;

        if total == 0 {
            0.0
        } else {
            (local as f64 / total as f64) * 100.0
        }
    }
}

/// Check if NUMA is available on this system
#[must_use]
pub fn is_numa_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        // Check if /sys/devices/system/node exists
        std::path::Path::new("/sys/devices/system/node").exists()
    }

    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Get number of NUMA nodes
pub fn get_numa_node_count() -> Result<usize> {
    #[cfg(target_os = "linux")]
    {
        let mut count = 0;
        let node_dir = std::path::Path::new("/sys/devices/system/node");

        if !node_dir.exists() {
            return Ok(1); // No NUMA, single node
        }

        let entries =
            std::fs::read_dir(node_dir).map_err(|e| OxiGdalError::io_error(e.to_string()))?;

        for entry in entries {
            let entry = entry.map_err(|e| OxiGdalError::io_error(e.to_string()))?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str.starts_with("node") && name_str[4..].parse::<u32>().is_ok() {
                count += 1;
            }
        }

        Ok(if count > 0 { count } else { 1 })
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(1)
    }
}

/// Get current NUMA node for the calling thread
pub fn get_current_node() -> Result<NumaNode> {
    #[cfg(target_os = "linux")]
    {
        let cpu = unsafe { libc::sched_getcpu() };
        if cpu < 0 {
            // Cannot determine CPU, fall back to node 0
            return Ok(NumaNode(0));
        }

        // Read NUMA node from sysfs (may not exist in containers)
        let path = format!("/sys/devices/system/cpu/cpu{}/node", cpu);
        let node_dirs = match std::fs::read_dir(&path) {
            Ok(dirs) => dirs,
            Err(_) => {
                // sysfs NUMA info unavailable (e.g., containerized environment)
                return Ok(NumaNode(0));
            }
        };

        for entry in node_dirs {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str.starts_with("node") {
                if let Ok(node_id) = name_str[4..].parse::<i32>() {
                    return Ok(NumaNode(node_id));
                }
            }
        }

        Ok(NumaNode(0))
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(NumaNode(0))
    }
}

/// NUMA-aware allocator
pub struct NumaAllocator {
    /// Configuration
    config: NumaConfig,
    /// Statistics
    stats: Arc<NumaStats>,
}

impl NumaAllocator {
    /// Create a new NUMA allocator
    pub fn new() -> Result<Self> {
        Self::with_config(NumaConfig::default())
    }

    /// Create with custom configuration
    pub fn with_config(config: NumaConfig) -> Result<Self> {
        let num_nodes = get_numa_node_count()?;
        Ok(Self {
            config,
            stats: Arc::new(NumaStats::new(num_nodes)),
        })
    }

    /// Allocate memory with NUMA awareness
    pub fn allocate(&self, size: usize) -> Result<*mut u8> {
        if self.config.enabled {
            self.allocate_numa(size)
        } else {
            // NUMA not enabled, use standard allocation
            let layout = std::alloc::Layout::from_size_align(size, 16)
                .map_err(|e| OxiGdalError::allocation_error(e.to_string()))?;

            unsafe {
                let ptr = std::alloc::alloc(layout);
                if ptr.is_null() {
                    return Err(OxiGdalError::allocation_error(
                        "Allocation failed".to_string(),
                    ));
                }
                Ok(ptr)
            }
        }
    }

    /// Allocate with NUMA policy
    fn allocate_numa(&self, size: usize) -> Result<*mut u8> {
        #[cfg(target_os = "linux")]
        {
            use std::ptr::null_mut;

            let ptr = match self.config.policy {
                NumaPolicy::Default => {
                    self.stats.record_local();
                    unsafe {
                        libc::mmap(
                            null_mut(),
                            size,
                            libc::PROT_READ | libc::PROT_WRITE,
                            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                            -1,
                            0,
                        )
                    }
                }
                NumaPolicy::Bind(node) => {
                    if self.config.track_stats {
                        let current = get_current_node()?;
                        if current == node {
                            self.stats.record_local();
                        } else {
                            self.stats.record_remote();
                        }
                    }

                    unsafe {
                        let ptr = libc::mmap(
                            null_mut(),
                            size,
                            libc::PROT_READ | libc::PROT_WRITE,
                            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                            -1,
                            0,
                        );

                        if ptr != libc::MAP_FAILED {
                            // Apply NUMA binding
                            let node_mask: u64 = 1 << node.id();
                            mbind(
                                ptr,
                                size,
                                MPOL_BIND,
                                &node_mask as *const u64 as *const libc::c_ulong,
                                64,
                                0,
                            );
                        }

                        ptr
                    }
                }
                NumaPolicy::Interleave => {
                    self.stats.record_interleaved();
                    unsafe {
                        let ptr = libc::mmap(
                            null_mut(),
                            size,
                            libc::PROT_READ | libc::PROT_WRITE,
                            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                            -1,
                            0,
                        );

                        if ptr != libc::MAP_FAILED {
                            mbind(ptr, size, MPOL_INTERLEAVE, null_mut(), 0, 0);
                        }

                        ptr
                    }
                }
                NumaPolicy::Prefer(node) => {
                    if self.config.track_stats {
                        let current = get_current_node()?;
                        if current == node {
                            self.stats.record_local();
                        } else {
                            self.stats.record_remote();
                        }
                    }

                    unsafe {
                        let ptr = libc::mmap(
                            null_mut(),
                            size,
                            libc::PROT_READ | libc::PROT_WRITE,
                            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                            -1,
                            0,
                        );

                        if ptr != libc::MAP_FAILED {
                            let node_mask: u64 = 1 << node.id();
                            mbind(
                                ptr,
                                size,
                                MPOL_PREFERRED,
                                &node_mask as *const u64 as *const libc::c_ulong,
                                64,
                                0,
                            );
                        }

                        ptr
                    }
                }
            };

            if ptr == libc::MAP_FAILED {
                return Err(OxiGdalError::allocation_error(
                    "NUMA allocation failed".to_string(),
                ));
            }

            Ok(ptr as *mut u8)
        }

        #[cfg(not(target_os = "linux"))]
        {
            // Fallback to standard allocation
            let layout = std::alloc::Layout::from_size_align(size, 16)
                .map_err(|e| OxiGdalError::allocation_error(e.to_string()))?;

            unsafe {
                let ptr = std::alloc::alloc(layout);
                if ptr.is_null() {
                    return Err(OxiGdalError::allocation_error(
                        "Allocation failed".to_string(),
                    ));
                }
                Ok(ptr)
            }
        }
    }

    /// Deallocate memory
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - `ptr` was allocated by this allocator
    /// - `size` matches the original allocation size
    /// - `ptr` has not been deallocated previously
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    pub fn deallocate(&self, ptr: *mut u8, size: usize) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            if self.config.enabled {
                unsafe {
                    libc::munmap(ptr as *mut libc::c_void, size);
                }
                return Ok(());
            }
        }

        // Standard deallocation
        unsafe {
            let layout = std::alloc::Layout::from_size_align_unchecked(size, 16);
            std::alloc::dealloc(ptr, layout);
        }

        Ok(())
    }

    /// Get statistics
    #[must_use]
    pub fn stats(&self) -> Arc<NumaStats> {
        Arc::clone(&self.stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_numa_detection() {
        let available = is_numa_available();
        println!("NUMA available: {}", available);

        let node_count = get_numa_node_count().expect("Failed to get NUMA node count in test");
        println!("NUMA nodes: {}", node_count);
        assert!(node_count >= 1);
    }

    #[test]
    fn test_current_node() {
        let node = get_current_node().expect("Failed to get current NUMA node in test");
        println!("Current NUMA node: {}", node.id());
        assert!(node.id() >= 0);
    }

    #[test]
    fn test_numa_allocator() {
        let allocator = NumaAllocator::new().expect("Failed to create NUMA allocator in test");
        let ptr = allocator
            .allocate(4096)
            .expect("Failed to allocate 4096 bytes with NUMA allocator in test");
        assert!(!ptr.is_null());

        allocator
            .deallocate(ptr, 4096)
            .expect("Failed to deallocate NUMA memory in test");
    }

    #[test]
    fn test_numa_stats() {
        let stats = NumaStats::new(4);
        stats.record_local();
        stats.record_local();
        stats.record_remote();

        assert_eq!(stats.local_allocations.load(Ordering::Relaxed), 2);
        assert_eq!(stats.remote_allocations.load(Ordering::Relaxed), 1);
        assert!((stats.local_percentage() - 66.66).abs() < 0.1);
    }
}
