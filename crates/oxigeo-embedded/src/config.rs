//! Configuration for embedded systems
//!
//! Provides compile-time and runtime configuration options

use crate::error::{EmbeddedError, Result};

#[cfg(feature = "low-power")]
use crate::power::PowerMode;

// Stub PowerMode when feature is disabled
/// Power mode configuration for embedded systems
#[cfg(not(feature = "low-power"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PowerMode {
    /// Full performance, maximum power consumption
    HighPerformance = 0,
    /// Balanced performance and power
    Balanced = 1,
    /// Low power, reduced performance
    LowPower = 2,
    /// Ultra low power, minimal functionality
    UltraLowPower = 3,
}

/// System configuration
#[derive(Debug, Clone, Copy)]
pub struct SystemConfig {
    /// CPU frequency in MHz
    pub cpu_freq_mhz: u32,
    /// RAM size in bytes
    pub ram_size: usize,
    /// Flash size in bytes
    pub flash_size: usize,
    /// Default power mode
    pub power_mode: PowerMode,
    /// Enable debug logging
    pub debug_enabled: bool,
}

impl SystemConfig {
    /// Create a new system configuration
    pub const fn new() -> Self {
        Self {
            cpu_freq_mhz: 80,            // Common embedded frequency
            ram_size: 512 * 1024,        // 512KB
            flash_size: 4 * 1024 * 1024, // 4MB
            power_mode: PowerMode::Balanced,
            debug_enabled: cfg!(debug_assertions),
        }
    }

    /// Create configuration for high performance
    pub const fn high_performance() -> Self {
        Self {
            cpu_freq_mhz: 240,
            ram_size: 512 * 1024,
            flash_size: 4 * 1024 * 1024,
            power_mode: PowerMode::HighPerformance,
            debug_enabled: false,
        }
    }

    /// Create configuration for low power
    pub const fn low_power() -> Self {
        Self {
            cpu_freq_mhz: 40,
            ram_size: 512 * 1024,
            flash_size: 4 * 1024 * 1024,
            power_mode: PowerMode::LowPower,
            debug_enabled: false,
        }
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        if self.cpu_freq_mhz == 0 {
            return Err(EmbeddedError::InvalidParameter);
        }

        if self.ram_size == 0 {
            return Err(EmbeddedError::InvalidParameter);
        }

        Ok(())
    }
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Memory configuration
#[derive(Debug, Clone, Copy)]
pub struct MemoryConfig {
    /// Static pool size in bytes
    pub static_pool_size: usize,
    /// Block pool block size
    pub block_size: usize,
    /// Number of blocks in block pool
    pub num_blocks: usize,
    /// Enable memory guards (debug)
    pub enable_guards: bool,
}

impl MemoryConfig {
    /// Create a new memory configuration
    pub const fn new() -> Self {
        Self {
            static_pool_size: 64 * 1024, // 64KB
            block_size: 256,
            num_blocks: 256,
            enable_guards: cfg!(debug_assertions),
        }
    }

    /// Create configuration for minimal memory
    pub const fn minimal() -> Self {
        Self {
            static_pool_size: 4 * 1024, // 4KB
            block_size: 64,
            num_blocks: 64,
            enable_guards: false,
        }
    }

    /// Get total memory usage
    pub const fn total_size(&self) -> usize {
        self.static_pool_size + (self.block_size * self.num_blocks)
    }
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Real-time configuration
#[derive(Debug, Clone, Copy)]
pub struct RealtimeConfig {
    /// Enable real-time scheduling
    pub enabled: bool,
    /// Maximum number of tasks
    pub max_tasks: usize,
    /// Default task priority
    pub default_priority: u8,
    /// Enable deadline monitoring
    pub monitor_deadlines: bool,
}

impl RealtimeConfig {
    /// Create a new real-time configuration
    pub const fn new() -> Self {
        Self {
            enabled: false,
            max_tasks: 8,
            default_priority: 2,
            monitor_deadlines: false,
        }
    }

    /// Create configuration with real-time enabled
    pub const fn enabled() -> Self {
        Self {
            enabled: true,
            max_tasks: 16,
            default_priority: 2,
            monitor_deadlines: true,
        }
    }
}

impl Default for RealtimeConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Communication configuration
#[derive(Debug, Clone, Copy)]
pub struct CommConfig {
    /// UART baud rate
    pub uart_baud: u32,
    /// I2C frequency (Hz)
    pub i2c_freq: u32,
    /// SPI frequency (Hz)
    pub spi_freq: u32,
    /// Enable CRC checking
    pub enable_crc: bool,
}

impl CommConfig {
    /// Create a new communication configuration
    pub const fn new() -> Self {
        Self {
            uart_baud: 115200,
            i2c_freq: 100_000,   // 100kHz
            spi_freq: 1_000_000, // 1MHz
            enable_crc: false,
        }
    }

    /// Create high-speed configuration
    pub const fn high_speed() -> Self {
        Self {
            uart_baud: 921600,
            i2c_freq: 400_000,    // 400kHz (fast mode)
            spi_freq: 10_000_000, // 10MHz
            enable_crc: true,
        }
    }
}

impl Default for CommConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Complete embedded configuration
#[derive(Debug, Clone, Copy)]
pub struct EmbeddedConfig {
    /// System configuration
    pub system: SystemConfig,
    /// Memory configuration
    pub memory: MemoryConfig,
    /// Real-time configuration
    pub realtime: RealtimeConfig,
    /// Communication configuration
    pub comm: CommConfig,
}

impl EmbeddedConfig {
    /// Create a new embedded configuration
    pub const fn new() -> Self {
        Self {
            system: SystemConfig::new(),
            memory: MemoryConfig::new(),
            realtime: RealtimeConfig::new(),
            comm: CommConfig::new(),
        }
    }

    /// Create high-performance configuration
    pub const fn high_performance() -> Self {
        Self {
            system: SystemConfig::high_performance(),
            memory: MemoryConfig::new(),
            realtime: RealtimeConfig::enabled(),
            comm: CommConfig::high_speed(),
        }
    }

    /// Create low-power configuration
    pub const fn low_power() -> Self {
        Self {
            system: SystemConfig::low_power(),
            memory: MemoryConfig::minimal(),
            realtime: RealtimeConfig::new(),
            comm: CommConfig::new(),
        }
    }

    /// Validate the entire configuration
    pub fn validate(&self) -> Result<()> {
        self.system.validate()?;

        // Check memory doesn't exceed RAM
        if self.memory.total_size() > self.system.ram_size {
            return Err(EmbeddedError::BufferTooSmall {
                required: self.memory.total_size(),
                available: self.system.ram_size,
            });
        }

        Ok(())
    }
}

impl Default for EmbeddedConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration presets for common platforms
pub mod presets {
    use super::*;

    /// ESP32 preset
    pub const fn esp32() -> EmbeddedConfig {
        EmbeddedConfig {
            system: SystemConfig {
                cpu_freq_mhz: 240,
                ram_size: 520 * 1024, // 520KB
                flash_size: 4 * 1024 * 1024,
                power_mode: PowerMode::HighPerformance,
                debug_enabled: false,
            },
            memory: MemoryConfig {
                static_pool_size: 128 * 1024,
                block_size: 512,
                num_blocks: 128,
                enable_guards: false,
            },
            realtime: RealtimeConfig::enabled(),
            comm: CommConfig::high_speed(),
        }
    }

    /// ESP32-C3 preset (RISC-V)
    pub const fn esp32c3() -> EmbeddedConfig {
        EmbeddedConfig {
            system: SystemConfig {
                cpu_freq_mhz: 160,
                ram_size: 400 * 1024, // 400KB
                flash_size: 4 * 1024 * 1024,
                power_mode: PowerMode::Balanced,
                debug_enabled: false,
            },
            memory: MemoryConfig {
                static_pool_size: 64 * 1024,
                block_size: 256,
                num_blocks: 128,
                enable_guards: false,
            },
            realtime: RealtimeConfig::enabled(),
            comm: CommConfig::new(),
        }
    }

    /// ARM Cortex-M4 preset
    pub const fn cortex_m4() -> EmbeddedConfig {
        EmbeddedConfig {
            system: SystemConfig {
                cpu_freq_mhz: 168,
                ram_size: 192 * 1024,    // 192KB
                flash_size: 1024 * 1024, // 1MB
                power_mode: PowerMode::Balanced,
                debug_enabled: false,
            },
            memory: MemoryConfig {
                static_pool_size: 32 * 1024,
                block_size: 128,
                num_blocks: 128,
                enable_guards: false,
            },
            realtime: RealtimeConfig::enabled(),
            comm: CommConfig::new(),
        }
    }

    /// RISC-V preset (generic)
    pub const fn riscv() -> EmbeddedConfig {
        EmbeddedConfig {
            system: SystemConfig {
                cpu_freq_mhz: 100,
                ram_size: 128 * 1024,        // 128KB
                flash_size: 2 * 1024 * 1024, // 2MB
                power_mode: PowerMode::Balanced,
                debug_enabled: false,
            },
            memory: MemoryConfig {
                static_pool_size: 16 * 1024,
                block_size: 128,
                num_blocks: 64,
                enable_guards: false,
            },
            realtime: RealtimeConfig::new(),
            comm: CommConfig::new(),
        }
    }

    /// Ultra-low-power preset
    pub const fn ultra_low_power() -> EmbeddedConfig {
        EmbeddedConfig {
            system: SystemConfig {
                cpu_freq_mhz: 32,
                ram_size: 64 * 1024,    // 64KB
                flash_size: 512 * 1024, // 512KB
                power_mode: PowerMode::UltraLowPower,
                debug_enabled: false,
            },
            memory: MemoryConfig::minimal(),
            realtime: RealtimeConfig::new(),
            comm: CommConfig::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_config() {
        let config = SystemConfig::new();
        assert!(config.validate().is_ok());

        let hp_config = SystemConfig::high_performance();
        assert_eq!(hp_config.power_mode, PowerMode::HighPerformance);
    }

    #[test]
    fn test_memory_config() {
        let config = MemoryConfig::new();
        assert!(config.total_size() > 0);

        let minimal = MemoryConfig::minimal();
        assert!(minimal.total_size() < config.total_size());
    }

    #[test]
    fn test_embedded_config_validation() {
        let config = EmbeddedConfig::new();
        assert!(config.validate().is_ok());

        let hp_config = EmbeddedConfig::high_performance();
        assert!(hp_config.validate().is_ok());
    }

    #[test]
    fn test_presets() {
        let esp32_config = presets::esp32();
        assert!(esp32_config.validate().is_ok());
        assert_eq!(esp32_config.system.cpu_freq_mhz, 240);

        let cortex_config = presets::cortex_m4();
        assert!(cortex_config.validate().is_ok());
    }
}
