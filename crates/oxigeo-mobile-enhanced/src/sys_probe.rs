//! Real system introspection helpers shared by the platform-specific modules.
//!
//! These helpers query genuine operating-system memory statistics through
//! [`sysinfo`] (which itself uses the mach APIs on Apple platforms and
//! `/proc`/`sysinfo(2)` on Linux/Android). They deliberately **never**
//! fabricate values: when the host cannot supply a reading, an explicit
//! [`MobileError::MemoryPressureError`] is returned instead of a plausible
//! constant, so downstream memory-pressure safety logic cannot be silently
//! disabled.

use crate::error::{MobileError, Result};

/// A real snapshot of host/device physical memory, in bytes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PhysicalMemory {
    /// Total physical RAM.
    pub total: u64,
    /// Memory currently available for (re)use.
    pub available: u64,
    /// Memory currently in use.
    ///
    /// Only read on iOS (`ios::memory`); allowed dead on other platform
    /// feature combinations (e.g. `--features android` alone) since it is
    /// still a genuine real reading, just not yet consumed by every caller.
    #[cfg_attr(not(feature = "ios"), allow(dead_code))]
    pub used: u64,
    /// Resident memory of the current process (0 if the OS could not report it).
    pub process: u64,
}

/// Sample real physical memory from the operating system.
///
/// # Errors
///
/// Returns [`MobileError::MemoryPressureError`] if the platform does not expose
/// physical-memory statistics (reported as a total of zero bytes), rather than
/// returning fabricated data.
pub(crate) fn sample_physical_memory() -> Result<PhysicalMemory> {
    use sysinfo::{ProcessesToUpdate, System};

    let mut sys = System::new();
    sys.refresh_memory();

    let total = sys.total_memory();
    if total == 0 {
        return Err(MobileError::MemoryPressureError(
            "physical memory introspection is unavailable on this platform".to_string(),
        ));
    }

    let available = sys.available_memory();
    // `used_memory` can momentarily exceed the derived `total - available`
    // depending on the platform; clamp to keep percentages in [0, 100].
    let used = sys.used_memory().min(total);

    // Best-effort resident-set size of the current process. A failure here is
    // non-fatal: the aggregate device figures above are what drive pressure
    // classification, so an unknown per-process figure is reported as zero.
    let process = match sysinfo::get_current_pid() {
        Ok(pid) => {
            sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), false);
            sys.process(pid).map(sysinfo::Process::memory).unwrap_or(0)
        }
        Err(_) => 0,
    };

    Ok(PhysicalMemory {
        total,
        available,
        used,
        process,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn sample_returns_real_nonzero_memory_on_host() {
        // The CI/dev host is a supported platform (Linux/macOS/Windows), so a
        // real, self-consistent reading must come back.
        let mem = sample_physical_memory().expect("host must expose physical memory");
        assert!(mem.total > 0);
        assert!(mem.available <= mem.total);
        assert!(mem.used <= mem.total);
    }
}
