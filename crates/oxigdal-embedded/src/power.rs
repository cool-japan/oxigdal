//! Power management for embedded systems
//!
//! Provides utilities for managing power consumption in resource-constrained environments

use crate::error::{EmbeddedError, Result};
use core::sync::atomic::{AtomicU8, AtomicU16, Ordering};

/// Power mode levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
    /// Sleep mode, CPU halted
    Sleep = 4,
    /// Deep sleep, most peripherals off
    DeepSleep = 5,
}

impl PowerMode {
    /// Get power mode from u8
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::HighPerformance),
            1 => Some(Self::Balanced),
            2 => Some(Self::LowPower),
            3 => Some(Self::UltraLowPower),
            4 => Some(Self::Sleep),
            5 => Some(Self::DeepSleep),
            _ => None,
        }
    }

    /// Get CPU frequency scaling factor (1.0 = full speed)
    pub const fn cpu_freq_factor(&self) -> f32 {
        match self {
            Self::HighPerformance => 1.0,
            Self::Balanced => 0.75,
            Self::LowPower => 0.5,
            Self::UltraLowPower => 0.25,
            Self::Sleep => 0.0,
            Self::DeepSleep => 0.0,
        }
    }

    /// Check if mode allows CPU execution
    pub const fn allows_execution(&self) -> bool {
        !matches!(self, Self::Sleep | Self::DeepSleep)
    }
}

/// Power manager for controlling system power state
pub struct PowerManager {
    current_mode: AtomicU8,
}

impl PowerManager {
    /// Create a new power manager in high performance mode
    pub const fn new() -> Self {
        Self {
            current_mode: AtomicU8::new(PowerMode::HighPerformance as u8),
        }
    }

    /// Get current power mode
    pub fn current_mode(&self) -> PowerMode {
        let mode_u8 = self.current_mode.load(Ordering::Relaxed);
        PowerMode::from_u8(mode_u8).unwrap_or(PowerMode::HighPerformance)
    }

    /// Request power mode transition
    ///
    /// # Errors
    ///
    /// Returns error if transition is not allowed or fails
    pub fn request_mode(&self, mode: PowerMode) -> Result<()> {
        let current = self.current_mode();

        // Validate transition
        if !self.is_transition_allowed(current, mode) {
            return Err(EmbeddedError::PowerModeTransitionFailed);
        }

        // Store new mode
        self.current_mode.store(mode as u8, Ordering::Release);

        // Perform hardware transition (platform-specific)
        self.apply_mode(mode)?;

        Ok(())
    }

    /// Check if a power mode transition is allowed
    fn is_transition_allowed(&self, from: PowerMode, to: PowerMode) -> bool {
        // Allow any transition for now
        // In real implementation, this would check hardware constraints
        let _ = from;
        let _ = to;
        true
    }

    /// Apply the power mode to hardware
    fn apply_mode(&self, mode: PowerMode) -> Result<()> {
        match mode {
            PowerMode::HighPerformance => self.apply_high_performance(),
            PowerMode::Balanced => self.apply_balanced(),
            PowerMode::LowPower => self.apply_low_power(),
            PowerMode::UltraLowPower => self.apply_ultra_low_power(),
            PowerMode::Sleep => self.apply_sleep(),
            PowerMode::DeepSleep => self.apply_deep_sleep(),
        }
    }

    fn apply_high_performance(&self) -> Result<()> {
        // Set CPU to maximum frequency
        // Enable all peripherals
        // Disable power gating
        Ok(())
    }

    fn apply_balanced(&self) -> Result<()> {
        // Set CPU to moderate frequency
        // Enable dynamic frequency scaling
        Ok(())
    }

    fn apply_low_power(&self) -> Result<()> {
        // Reduce CPU frequency
        // Disable unused peripherals
        // Enable aggressive power gating
        Ok(())
    }

    fn apply_ultra_low_power(&self) -> Result<()> {
        // Minimum CPU frequency
        // Disable all non-essential peripherals
        // Maximum power saving
        Ok(())
    }

    fn apply_sleep(&self) -> Result<()> {
        // Halt CPU, wait for interrupt
        #[cfg(feature = "esp32")]
        {
            // Would call actual sleep implementation
        }

        #[cfg(feature = "riscv")]
        {
            use crate::target::riscv::power;
            power::wait_for_interrupt()?;
        }

        Ok(())
    }

    fn apply_deep_sleep(&self) -> Result<()> {
        // Enter deep sleep mode
        // Most peripherals powered off
        // Only wake on specific triggers
        Ok(())
    }
}

impl Default for PowerManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Power consumption estimator
pub struct PowerEstimator {
    current_ma: u32,
    voltage_mv: u32,
}

impl PowerEstimator {
    /// Create a new power estimator
    pub const fn new(voltage_mv: u32) -> Self {
        Self {
            current_ma: 0,
            voltage_mv,
        }
    }

    /// Set current consumption in milliamps
    pub fn set_current(&mut self, current_ma: u32) {
        self.current_ma = current_ma;
    }

    /// Get current consumption in milliamps
    pub const fn current_ma(&self) -> u32 {
        self.current_ma
    }

    /// Calculate power consumption in milliwatts
    pub const fn power_mw(&self) -> u32 {
        (self.current_ma as u64 * self.voltage_mv as u64 / 1000) as u32
    }

    /// Estimate battery life in hours
    pub fn battery_life_hours(&self, battery_mah: u32) -> f32 {
        if self.current_ma == 0 {
            return f32::INFINITY;
        }

        battery_mah as f32 / self.current_ma as f32
    }

    /// Estimate energy consumption in millijoules per operation
    pub fn energy_per_op(&self, operation_time_us: u32) -> f32 {
        let power_w = self.power_mw() as f32 / 1000.0;
        let time_s = operation_time_us as f32 / 1_000_000.0;
        power_w * time_s * 1000.0 // Convert to mJ
    }
}

/// Wake source for sleep modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeSource {
    /// Timer wake
    Timer,
    /// GPIO wake
    Gpio,
    /// UART wake
    Uart,
    /// Touch sensor wake
    Touch,
    /// Any wake source
    Any,
}

/// Sleep configuration
pub struct SleepConfig {
    /// Wakeup sources
    pub wake_sources: heapless::Vec<WakeSource, 8>,
    /// Sleep duration in microseconds (for timer wake)
    pub duration_us: Option<u64>,
    /// Enable RTC during sleep
    pub keep_rtc: bool,
}

impl Default for SleepConfig {
    fn default() -> Self {
        Self {
            wake_sources: heapless::Vec::new(),
            duration_us: None,
            keep_rtc: true,
        }
    }
}

impl SleepConfig {
    /// Create a new sleep configuration
    pub const fn new() -> Self {
        Self {
            wake_sources: heapless::Vec::new(),
            duration_us: None,
            keep_rtc: true,
        }
    }

    /// Add a wake source
    pub fn add_wake_source(&mut self, source: WakeSource) -> Result<()> {
        self.wake_sources
            .push(source)
            .map_err(|_| EmbeddedError::BufferTooSmall {
                required: 1,
                available: 0,
            })
    }

    /// Set timer wake duration
    pub fn set_timer_wake(&mut self, duration_us: u64) -> Result<()> {
        self.duration_us = Some(duration_us);
        self.add_wake_source(WakeSource::Timer)
    }
}

/// Voltage regulator control
pub struct VoltageRegulator {
    voltage_mv: AtomicU16, // Actual voltage in millivolts
}

impl VoltageRegulator {
    /// Create a new voltage regulator at nominal voltage
    pub const fn new(nominal_voltage_mv: u16) -> Self {
        Self {
            voltage_mv: AtomicU16::new(nominal_voltage_mv),
        }
    }

    /// Get current voltage in millivolts
    pub fn voltage_mv(&self) -> u16 {
        self.voltage_mv.load(Ordering::Relaxed)
    }

    /// Set voltage in millivolts
    ///
    /// # Errors
    ///
    /// Returns error if voltage is out of safe range
    pub fn set_voltage(&self, voltage_mv: u16) -> Result<()> {
        // Validate voltage range (0.5V to 5.0V)
        if !(500..=5000).contains(&voltage_mv) {
            return Err(EmbeddedError::InvalidParameter);
        }

        self.voltage_mv.store(voltage_mv, Ordering::Release);

        // Apply voltage change to hardware
        // This would interface with voltage regulator hardware
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_power_mode_ordering() {
        assert!(PowerMode::HighPerformance < PowerMode::LowPower);
        assert!(PowerMode::LowPower < PowerMode::Sleep);
    }

    #[test]
    fn test_power_manager() {
        let pm = PowerManager::new();
        assert_eq!(pm.current_mode(), PowerMode::HighPerformance);

        pm.request_mode(PowerMode::LowPower)
            .expect("mode change failed");
        assert_eq!(pm.current_mode(), PowerMode::LowPower);
    }

    #[test]
    fn test_power_estimator() {
        let mut estimator = PowerEstimator::new(3300); // 3.3V
        estimator.set_current(100); // 100mA

        assert_eq!(estimator.power_mw(), 330); // 330mW

        let battery_life = estimator.battery_life_hours(1000); // 1000mAh battery
        assert_eq!(battery_life, 10.0); // 10 hours
    }

    #[test]
    fn test_sleep_config() {
        let mut config = SleepConfig::new();
        config
            .add_wake_source(WakeSource::Gpio)
            .expect("add failed");
        config.set_timer_wake(1_000_000).expect("set timer failed");

        assert_eq!(config.wake_sources.len(), 2);
    }

    #[test]
    fn test_voltage_regulator() {
        let regulator = VoltageRegulator::new(3300);
        assert_eq!(regulator.voltage_mv(), 3300);

        regulator.set_voltage(1800).expect("voltage change failed");
        assert_eq!(regulator.voltage_mv(), 1800);

        // Test invalid voltage
        assert!(regulator.set_voltage(6000).is_err());
    }
}
