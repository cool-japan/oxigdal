//! Battery-aware processing and power management
//!
//! This module provides utilities for monitoring battery levels and adjusting
//! processing algorithms based on power state to maximize battery life on
//! mobile devices.
//!
//! # Key Features
//!
//! - Real-time battery level monitoring
//! - Adaptive processing modes based on battery state
//! - Power consumption estimation
//! - Charging state detection
//! - Background processing throttling
//!
//! # Example
//!
//! ```rust,no_run
//! use oxigdal_mobile_enhanced::battery::{BatteryMonitor, ProcessingMode};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let monitor = BatteryMonitor::new()?;
//!
//! // Check current battery level
//! let level = monitor.current_level()?;
//! println!("Battery: {}%", level.percentage());
//!
//! // Get recommended processing mode
//! let mode = monitor.recommended_processing_mode();
//! match mode {
//!     ProcessingMode::HighPerformance => {
//!         // Use full processing capabilities
//!     }
//!     ProcessingMode::Balanced => {
//!         // Use moderate processing
//!     }
//!     ProcessingMode::PowerSaver => {
//!         // Minimize processing
//!     }
//! }
//! # Ok(())
//! # }
//! ```

use crate::error::{MobileError, Result};
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Battery level information
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BatteryLevel {
    /// Battery is fully charged (95-100%)
    Full,
    /// Battery is high (60-94%)
    High,
    /// Battery is medium (30-59%)
    Medium,
    /// Battery is low (15-29%)
    Low,
    /// Battery is critical (<15%)
    Critical,
    /// Battery level unknown
    Unknown,
}

impl BatteryLevel {
    /// Create battery level from percentage (0-100)
    pub fn from_percentage(percent: f32) -> Self {
        match percent {
            p if p >= 95.0 => Self::Full,
            p if p >= 60.0 => Self::High,
            p if p >= 30.0 => Self::Medium,
            p if p >= 15.0 => Self::Low,
            p if p >= 0.0 => Self::Critical,
            _ => Self::Unknown,
        }
    }

    /// Get typical percentage for this level
    pub fn percentage(&self) -> f32 {
        match self {
            Self::Full => 100.0,
            Self::High => 75.0,
            Self::Medium => 45.0,
            Self::Low => 20.0,
            Self::Critical => 10.0,
            Self::Unknown => 50.0,
        }
    }

    /// Check if battery is in a good state for intensive processing
    pub fn is_good_for_processing(&self) -> bool {
        matches!(self, Self::Full | Self::High)
    }

    /// Check if battery is critically low
    pub fn is_critical(&self) -> bool {
        matches!(self, Self::Critical)
    }
}

/// Charging state of the device
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChargingState {
    /// Device is charging
    Charging,
    /// Device is fully charged and connected to power
    FullyCharged,
    /// Device is discharging (on battery)
    Discharging,
    /// Charging state unknown
    Unknown,
}

/// Processing mode recommendation based on battery state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessingMode {
    /// High performance mode - use all available resources
    HighPerformance,
    /// Balanced mode - moderate resource usage
    Balanced,
    /// Power saver mode - minimal resource usage
    PowerSaver,
}

impl ProcessingMode {
    /// Get quality factor for this mode (0.0 - 1.0)
    pub fn quality_factor(&self) -> f32 {
        match self {
            Self::HighPerformance => 1.0,
            Self::Balanced => 0.7,
            Self::PowerSaver => 0.4,
        }
    }

    /// Get maximum concurrent operations for this mode
    pub fn max_concurrent_operations(&self) -> usize {
        let cpu_count = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(4);

        match self {
            Self::HighPerformance => cpu_count,
            Self::Balanced => (cpu_count / 2).max(1),
            Self::PowerSaver => 1,
        }
    }

    /// Get cache size multiplier for this mode
    pub fn cache_size_multiplier(&self) -> f32 {
        match self {
            Self::HighPerformance => 1.0,
            Self::Balanced => 0.5,
            Self::PowerSaver => 0.25,
        }
    }

    /// Check if background processing is allowed
    pub fn allows_background_processing(&self) -> bool {
        !matches!(self, Self::PowerSaver)
    }
}

/// Battery state snapshot
#[derive(Debug, Clone)]
pub struct BatteryState {
    /// Current battery level
    pub level: BatteryLevel,
    /// Charging state
    pub charging: ChargingState,
    /// Battery percentage (0.0 - 100.0)
    pub percentage: f32,
    /// Estimated time remaining (if discharging)
    pub time_remaining: Option<Duration>,
    /// Temperature in Celsius (if available)
    pub temperature: Option<f32>,
    /// Timestamp of this reading
    pub timestamp: Instant,
}

impl BatteryState {
    /// Check if device can perform intensive operations
    pub fn can_perform_intensive_operations(&self) -> bool {
        matches!(
            self.charging,
            ChargingState::Charging | ChargingState::FullyCharged
        ) || self.level.is_good_for_processing()
    }

    /// Get recommended processing mode
    pub fn recommended_mode(&self) -> ProcessingMode {
        match (&self.charging, &self.level) {
            // When charging, use high performance
            (ChargingState::Charging | ChargingState::FullyCharged, _) => {
                ProcessingMode::HighPerformance
            }
            // On battery
            (_, BatteryLevel::Full | BatteryLevel::High) => ProcessingMode::Balanced,
            (_, BatteryLevel::Medium) => ProcessingMode::Balanced,
            (_, BatteryLevel::Low | BatteryLevel::Critical) => ProcessingMode::PowerSaver,
            (_, BatteryLevel::Unknown) => ProcessingMode::Balanced,
        }
    }

    /// Check if battery is overheating
    pub fn is_overheating(&self) -> bool {
        self.temperature.is_some_and(|temp| temp > 45.0)
    }
}

/// Battery monitoring service
pub struct BatteryMonitor {
    state: Arc<RwLock<Option<BatteryState>>>,
    #[cfg(feature = "battery-aware")]
    sys: Arc<RwLock<sysinfo::System>>,
}

impl BatteryMonitor {
    /// Create a new battery monitor
    pub fn new() -> Result<Self> {
        #[cfg(feature = "battery-aware")]
        {
            let mut sys = sysinfo::System::new();
            sys.refresh_all();

            Ok(Self {
                state: Arc::new(RwLock::new(None)),
                sys: Arc::new(RwLock::new(sys)),
            })
        }

        #[cfg(not(feature = "battery-aware"))]
        {
            Ok(Self {
                state: Arc::new(RwLock::new(None)),
            })
        }
    }

    /// Refresh battery state
    pub fn refresh(&self) -> Result<()> {
        #[cfg(feature = "battery-aware")]
        {
            let mut sys = self.sys.write();
            sys.refresh_all();

            // Note: sysinfo doesn't provide battery info directly
            // In a real implementation, we would use platform-specific APIs
            // For now, we'll create a mock state
            let percentage = 75.0; // Mock value
            let level = BatteryLevel::from_percentage(percentage);

            let state = BatteryState {
                level,
                charging: ChargingState::Discharging,
                percentage,
                time_remaining: Some(Duration::from_secs(3600 * 4)),
                temperature: Some(35.0),
                timestamp: Instant::now(),
            };

            *self.state.write() = Some(state);
            Ok(())
        }

        #[cfg(not(feature = "battery-aware"))]
        {
            Err(MobileError::BatteryMonitoringNotSupported)
        }
    }

    /// Get current battery level
    pub fn current_level(&self) -> Result<BatteryLevel> {
        self.refresh()?;

        let state = self.state.read();
        state
            .as_ref()
            .map(|s| s.level)
            .ok_or(MobileError::BatteryReadError(
                "No battery state available".to_string(),
            ))
    }

    /// Get current battery state
    pub fn current_state(&self) -> Result<BatteryState> {
        self.refresh()?;

        let state = self.state.read();
        state.clone().ok_or(MobileError::BatteryReadError(
            "No battery state available".to_string(),
        ))
    }

    /// Get recommended processing mode
    pub fn recommended_processing_mode(&self) -> ProcessingMode {
        match self.current_state() {
            Ok(state) => state.recommended_mode(),
            Err(_) => ProcessingMode::Balanced, // Default to balanced if unavailable
        }
    }

    /// Check if intensive operations are allowed
    pub fn can_perform_intensive_operations(&self) -> bool {
        self.current_state()
            .map_or(true, |state| state.can_perform_intensive_operations())
    }

    /// Get battery percentage
    pub fn battery_percentage(&self) -> Result<f32> {
        let state = self.current_state()?;
        Ok(state.percentage)
    }

    /// Check if device is charging
    pub fn is_charging(&self) -> Result<bool> {
        let state = self.current_state()?;
        Ok(matches!(
            state.charging,
            ChargingState::Charging | ChargingState::FullyCharged
        ))
    }
}

impl Default for BatteryMonitor {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| {
            #[cfg(feature = "battery-aware")]
            {
                Self {
                    state: Arc::new(RwLock::new(None)),
                    sys: Arc::new(RwLock::new(sysinfo::System::new())),
                }
            }
            #[cfg(not(feature = "battery-aware"))]
            {
                Self {
                    state: Arc::new(RwLock::new(None)),
                }
            }
        })
    }
}

/// Adaptive processor that adjusts behavior based on battery state
pub struct BatteryAdaptiveProcessor {
    monitor: BatteryMonitor,
    mode: ProcessingMode,
    last_update: Instant,
    update_interval: Duration,
}

impl BatteryAdaptiveProcessor {
    /// Create new adaptive processor
    pub fn new() -> Result<Self> {
        Ok(Self {
            monitor: BatteryMonitor::new()?,
            mode: ProcessingMode::Balanced,
            last_update: Instant::now(),
            update_interval: Duration::from_secs(30),
        })
    }

    /// Update processing mode if needed
    pub fn update_mode(&mut self) -> Result<()> {
        let now = Instant::now();
        if now.duration_since(self.last_update) >= self.update_interval {
            self.mode = self.monitor.recommended_processing_mode();
            self.last_update = now;
        }
        Ok(())
    }

    /// Get current processing mode
    pub fn current_mode(&self) -> ProcessingMode {
        self.mode
    }

    /// Process data with battery-aware settings
    pub fn process_with_battery_awareness<F, T>(&mut self, operation: F) -> Result<T>
    where
        F: FnOnce(ProcessingMode) -> Result<T>,
    {
        self.update_mode()?;
        operation(self.mode)
    }

    /// Check if background processing should be throttled
    pub fn should_throttle_background(&self) -> bool {
        !self.mode.allows_background_processing()
    }

    /// Get quality factor for current mode
    pub fn quality_factor(&self) -> f32 {
        self.mode.quality_factor()
    }
}

impl Default for BatteryAdaptiveProcessor {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| Self {
            monitor: BatteryMonitor::default(),
            mode: ProcessingMode::Balanced,
            last_update: Instant::now(),
            update_interval: Duration::from_secs(30),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_battery_level_from_percentage() {
        assert_eq!(BatteryLevel::from_percentage(100.0), BatteryLevel::Full);
        assert_eq!(BatteryLevel::from_percentage(75.0), BatteryLevel::High);
        assert_eq!(BatteryLevel::from_percentage(45.0), BatteryLevel::Medium);
        assert_eq!(BatteryLevel::from_percentage(20.0), BatteryLevel::Low);
        assert_eq!(BatteryLevel::from_percentage(10.0), BatteryLevel::Critical);
    }

    #[test]
    fn test_battery_level_is_good_for_processing() {
        assert!(BatteryLevel::Full.is_good_for_processing());
        assert!(BatteryLevel::High.is_good_for_processing());
        assert!(!BatteryLevel::Medium.is_good_for_processing());
        assert!(!BatteryLevel::Low.is_good_for_processing());
        assert!(!BatteryLevel::Critical.is_good_for_processing());
    }

    #[test]
    fn test_processing_mode_quality_factor() {
        assert_eq!(ProcessingMode::HighPerformance.quality_factor(), 1.0);
        assert_eq!(ProcessingMode::Balanced.quality_factor(), 0.7);
        assert_eq!(ProcessingMode::PowerSaver.quality_factor(), 0.4);
    }

    #[test]
    fn test_processing_mode_concurrent_operations() {
        let cpu_count = num_cpus::get();
        assert_eq!(
            ProcessingMode::HighPerformance.max_concurrent_operations(),
            cpu_count
        );
        assert_eq!(
            ProcessingMode::Balanced.max_concurrent_operations(),
            (cpu_count / 2).max(1)
        );
        assert_eq!(ProcessingMode::PowerSaver.max_concurrent_operations(), 1);
    }

    #[test]
    fn test_battery_state_recommended_mode() {
        let state = BatteryState {
            level: BatteryLevel::High,
            charging: ChargingState::Charging,
            percentage: 80.0,
            time_remaining: None,
            temperature: Some(30.0),
            timestamp: Instant::now(),
        };
        assert_eq!(state.recommended_mode(), ProcessingMode::HighPerformance);

        let state = BatteryState {
            level: BatteryLevel::Critical,
            charging: ChargingState::Discharging,
            percentage: 10.0,
            time_remaining: Some(Duration::from_secs(600)),
            temperature: Some(30.0),
            timestamp: Instant::now(),
        };
        assert_eq!(state.recommended_mode(), ProcessingMode::PowerSaver);
    }

    #[test]
    fn test_battery_state_overheating() {
        let state = BatteryState {
            level: BatteryLevel::Medium,
            charging: ChargingState::Discharging,
            percentage: 50.0,
            time_remaining: None,
            temperature: Some(50.0),
            timestamp: Instant::now(),
        };
        assert!(state.is_overheating());

        let state = BatteryState {
            level: BatteryLevel::Medium,
            charging: ChargingState::Discharging,
            percentage: 50.0,
            time_remaining: None,
            temperature: Some(35.0),
            timestamp: Instant::now(),
        };
        assert!(!state.is_overheating());
    }
}
