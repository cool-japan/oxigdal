//! Power management for embedded systems
//!
//! Provides utilities for managing power consumption in resource-constrained
//! environments.
//!
//! # Real power transitions require a BSP-provided controller
//!
//! CPU-frequency scaling and peripheral clock/power gating are SoC-vendor
//! specific (STM32 `RCC`/`PWR`, nRF `CLOCK`/`POWER`, ESP32 RTC/clock, RISC-V
//! PMU, …) and cannot be expressed portably in a `no_std` crate. This module
//! therefore does **not** fabricate register writes. Instead it defines the
//! [`PowerController`] trait as the extension point a board-support package
//! (BSP) or HAL implements, and [`PowerManager`] delegates every transition to
//! the installed controller.
//!
//! A [`PowerManager`] built with [`PowerManager::new`] holds a
//! [`NoController`]: it records the requested mode (a software bookkeeping
//! layer) but performs **no** hardware action for the active modes. Install a
//! real controller with [`PowerManager::with_controller`] to get actual
//! transitions, and use [`PowerManager::request_mode_strict`] when a silent
//! no-op would be a correctness bug.

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

/// Board-support extension point for **real** power-state control.
///
/// Dynamic CPU-frequency scaling and peripheral clock/power gating are
/// inherently SoC-vendor-specific: they are driven by device registers such as
/// the STM32 `RCC`/`PWR` blocks, the nRF `CLOCK`/`POWER` peripherals, the ESP32
/// RTC/clock subsystem, or a RISC-V PMU. None of that can be expressed portably
/// at the ARM/RISC-V ISA level, so this crate does **not** fabricate register
/// pokes. Instead, a board-support package (BSP) or HAL implements this trait
/// and hands it to [`PowerManager`], which then delegates every transition to
/// it.
///
/// All methods take `&self`: hardware power controllers manipulate
/// memory-mapped registers (volatile writes to fixed addresses) or hold their
/// own interior-mutable/atomic state, so shared-reference access is the
/// idiomatic and `static`-friendly shape for embedded singletons.
///
/// # Honesty contract
///
/// Implementing this trait is an assertion that [`set_power_mode`] actually
/// reaches silicon. A controller that cannot perform a transition must return a
/// typed error (never `Ok(())` for a change it did not make). The only
/// sanctioned no-op is [`NoController`], which reports
/// [`is_hardware`] `== false` so the manager and its callers can tell that no
/// real transition occurred.
///
/// [`set_power_mode`]: PowerController::set_power_mode
/// [`is_hardware`]: PowerController::is_hardware
pub trait PowerController {
    /// Apply `mode` to the hardware.
    ///
    /// This is the primary hook the BSP must implement. It is expected to
    /// reprogram clock trees, switch voltage-scaling levels, and gate/ungate
    /// peripherals as appropriate for the target mode. For the sleep modes the
    /// manager additionally issues the ISA-level `WFI` after this returns, so an
    /// implementation only needs to configure wake sources / deep power states
    /// here.
    ///
    /// # Errors
    ///
    /// Returns an [`EmbeddedError`] if the transition cannot be performed. An
    /// implementation must **not** return `Ok(())` for a change it did not
    /// actually apply.
    fn set_power_mode(&self, mode: PowerMode) -> Result<()>;

    /// Optional finer-grained hook: apply only the CPU-frequency scaling for
    /// `mode` (see [`PowerMode::cpu_freq_factor`]).
    ///
    /// Provided so a BSP can structure [`set_power_mode`](Self::set_power_mode)
    /// as `scale_cpu` + `gate_peripherals` if convenient. The default does
    /// nothing and returns `Ok(())`; overriding it has no effect unless the
    /// implementation's `set_power_mode` calls it.
    ///
    /// # Errors
    ///
    /// Returns an [`EmbeddedError`] if the clock reconfiguration fails.
    fn scale_cpu(&self, mode: PowerMode) -> Result<()> {
        let _ = mode;
        Ok(())
    }

    /// Optional finer-grained hook: gate or ungate peripheral clocks/power for
    /// `mode`.
    ///
    /// See [`scale_cpu`](Self::scale_cpu) for how the default is intended to be
    /// used.
    ///
    /// # Errors
    ///
    /// Returns an [`EmbeddedError`] if the peripheral gating fails.
    fn gate_peripherals(&self, mode: PowerMode) -> Result<()> {
        let _ = mode;
        Ok(())
    }

    /// Whether this controller drives real silicon.
    ///
    /// Defaults to `true`. Only host/test stubs — notably
    /// [`NoController`] — override this to `false` so that
    /// [`PowerManager::has_hardware_controller`] and
    /// [`PowerManager::request_mode_strict`] can distinguish a genuine hardware
    /// backend from a bookkeeping-only placeholder.
    fn is_hardware(&self) -> bool {
        true
    }
}

/// The default, host/test [`PowerController`] that performs **no** hardware
/// action.
///
/// A [`PowerManager`] built with [`PowerManager::new`] holds a `NoController`.
/// Its [`set_power_mode`](PowerController::set_power_mode) is an explicit,
/// documented no-op that returns `Ok(())` — the [`PowerManager`] still records
/// the requested mode, so the manager behaves as a software bookkeeping layer,
/// but **no clock tree is reprogrammed and no peripheral is gated**. Because it
/// reports [`is_hardware`](PowerController::is_hardware) `== false`, callers that
/// require real transitions can detect its presence via
/// [`PowerManager::has_hardware_controller`] or fail loudly with
/// [`PowerManager::request_mode_strict`].
///
/// Install a real BSP-provided [`PowerController`] with
/// [`PowerManager::with_controller`] to get actual power transitions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoController;

impl PowerController for NoController {
    /// No-op: records nothing at the hardware level and reports success so the
    /// owning [`PowerManager`] can act as a bookkeeping layer. See the type
    /// docs for the honesty contract.
    fn set_power_mode(&self, mode: PowerMode) -> Result<()> {
        let _ = mode;
        Ok(())
    }

    fn is_hardware(&self) -> bool {
        false
    }
}

/// Power manager for controlling system power state.
///
/// # Hardware support (read this)
///
/// Real power transitions — dynamic CPU-frequency scaling and peripheral
/// clock/power gating for `HighPerformance` / `Balanced` / `LowPower` /
/// `UltraLowPower` / `DeepSleep` — are SoC-vendor-specific and **require a
/// BSP-provided [`PowerController`]**. This manager never fabricates
/// vendor-register writes; it delegates every transition to the installed
/// controller.
///
/// The type parameter records which controller is installed:
///
/// - [`PowerManager::new`] builds a `PowerManager<`[`NoController`]`>`: a
///   bookkeeping-only manager. Transitions are validated and the requested mode
///   is recorded (so [`current_mode`](Self::current_mode) always reflects the
///   last successful request), but **no hardware is touched** for the four
///   active modes. This is honest by construction — the type is
///   `PowerManager<NoController>` and [`has_hardware_controller`] returns
///   `false`.
/// - [`PowerManager::with_controller`] builds a `PowerManager<C>` around a real
///   BSP controller; [`request_mode`](Self::request_mode) then delegates to
///   [`PowerController::set_power_mode`] and propagates its result.
///
/// ## Sleep modes
///
/// For [`PowerMode::Sleep`] and [`PowerMode::DeepSleep`] the manager, *after*
/// delegating to the controller, additionally issues the ISA-level `WFI`
/// (wait-for-interrupt) on `arm`/`aarch64` and `riscv` targets. `WFI` really
/// halts the core in a low-power state until a wake event and is target-generic
/// (not SoC-specific), so it lives here rather than in the controller. On other
/// targets it is a no-op.
///
/// ## Requiring real hardware
///
/// Callers that must not proceed without silicon-level control should use
/// [`request_mode_strict`](Self::request_mode_strict), which returns
/// [`EmbeddedError::NoPowerController`] when only a [`NoController`] is
/// installed instead of silently recording a mode that never reached hardware.
///
/// [`has_hardware_controller`]: Self::has_hardware_controller
/// [`current_mode`]: Self::current_mode
pub struct PowerManager<C: PowerController = NoController> {
    current_mode: AtomicU8,
    controller: C,
}

impl PowerManager<NoController> {
    /// Create a new bookkeeping-only power manager in high performance mode.
    ///
    /// The manager holds a [`NoController`], so transitions are recorded but no
    /// hardware is reconfigured. Use [`with_controller`](Self::with_controller)
    /// to install a BSP-provided [`PowerController`] for real transitions.
    pub const fn new() -> Self {
        Self {
            current_mode: AtomicU8::new(PowerMode::HighPerformance as u8),
            controller: NoController,
        }
    }
}

impl<C: PowerController> PowerManager<C> {
    /// Create a power manager backed by a BSP-provided [`PowerController`],
    /// starting in high performance mode.
    ///
    /// Every subsequent transition delegates to `controller`.
    pub const fn with_controller(controller: C) -> Self {
        Self {
            current_mode: AtomicU8::new(PowerMode::HighPerformance as u8),
            controller,
        }
    }

    /// Borrow the installed power controller.
    pub const fn controller(&self) -> &C {
        &self.controller
    }

    /// Whether a real hardware [`PowerController`] is installed.
    ///
    /// Returns `false` for the default [`NoController`] and `true` for any
    /// controller that does not override
    /// [`PowerController::is_hardware`] to `false`.
    pub fn has_hardware_controller(&self) -> bool {
        self.controller.is_hardware()
    }

    /// Get current power mode.
    pub fn current_mode(&self) -> PowerMode {
        let mode_u8 = self.current_mode.load(Ordering::Relaxed);
        PowerMode::from_u8(mode_u8).unwrap_or(PowerMode::HighPerformance)
    }

    /// Request a power mode transition.
    ///
    /// The requested mode is validated and recorded, then applied through the
    /// installed [`PowerController`]. With the default [`NoController`] the mode
    /// is recorded but **no hardware is reconfigured** (see the type-level
    /// docs); with a BSP controller the transition is delegated to
    /// [`PowerController::set_power_mode`] and its result is propagated. For
    /// sleep modes the ISA-level `WFI` is issued afterwards on `arm`/`riscv`.
    ///
    /// If you must not proceed without real hardware control, use
    /// [`request_mode_strict`](Self::request_mode_strict) instead.
    ///
    /// # Errors
    ///
    /// Returns [`EmbeddedError::PowerModeTransitionFailed`] if the transition is
    /// not allowed, or any error surfaced by the installed controller.
    pub fn request_mode(&self, mode: PowerMode) -> Result<()> {
        let current = self.current_mode();

        // Validate transition
        if !self.is_transition_allowed(current, mode) {
            return Err(EmbeddedError::PowerModeTransitionFailed);
        }

        // Record the requested mode before touching hardware so `current_mode`
        // reflects the intent even if the controller fails partway through.
        self.current_mode.store(mode as u8, Ordering::Release);

        // Delegate the actual transition to the installed controller.
        self.apply_mode(mode)
    }

    /// Request a power mode transition, failing loudly if there is no real
    /// hardware controller.
    ///
    /// Identical to [`request_mode`](Self::request_mode) except that it returns
    /// [`EmbeddedError::NoPowerController`] — **without** recording the mode or
    /// touching hardware — when only a bookkeeping [`NoController`] is
    /// installed. Use this when a silent no-op would be a correctness bug (for
    /// example, before entering a mode your application depends on for thermal
    /// or battery limits).
    ///
    /// # Errors
    ///
    /// Returns [`EmbeddedError::NoPowerController`] if no hardware controller is
    /// installed, otherwise behaves like
    /// [`request_mode`](Self::request_mode).
    pub fn request_mode_strict(&self, mode: PowerMode) -> Result<()> {
        if !self.controller.is_hardware() {
            return Err(EmbeddedError::NoPowerController);
        }
        self.request_mode(mode)
    }

    /// Check if a power mode transition is allowed.
    fn is_transition_allowed(&self, from: PowerMode, to: PowerMode) -> bool {
        // Allow any transition for now. Real hardware constraints (e.g. illegal
        // direct jumps between deep states) are the controller's concern and are
        // surfaced through `PowerController::set_power_mode`.
        let _ = from;
        let _ = to;
        true
    }

    /// Apply the power mode via the controller, plus the ISA-level `WFI` for
    /// sleep modes.
    fn apply_mode(&self, mode: PowerMode) -> Result<()> {
        // Primary hardware hook: SoC-specific frequency scaling / peripheral
        // gating / deep-power configuration is the controller's responsibility.
        self.controller.set_power_mode(mode)?;

        // Sleep modes additionally halt the core at the ISA level. This is
        // target-generic (not SoC-specific), so it belongs here rather than in
        // the controller, and it runs after the controller has configured wake
        // sources / deep-power state.
        match mode {
            PowerMode::Sleep | PowerMode::DeepSleep => self.enter_wait_for_interrupt(),
            _ => Ok(()),
        }
    }

    /// Halt the CPU until a wake event using the ISA-level `WFI` instruction.
    ///
    /// Real, ISA-level power reduction on bare-metal `arm`/`aarch64`/`riscv`
    /// targets; a no-op on hosted targets where `WFI` from user space can trap.
    fn enter_wait_for_interrupt(&self) -> Result<()> {
        #[cfg(feature = "riscv")]
        {
            use crate::target::riscv::power;
            power::wait_for_interrupt()?;
        }

        #[cfg(feature = "arm")]
        {
            use crate::target::arm::power;
            power::wait_for_interrupt()?;
        }

        Ok(())
    }
}

impl Default for PowerManager<NoController> {
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
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

    /// A mock hardware controller that records every delegated call so tests can
    /// assert the manager delegates correctly. Uses atomics for `&self` interior
    /// mutability, mirroring how a real register-backed controller behaves.
    struct MockController {
        calls: AtomicUsize,
        last_mode: AtomicU8,
        fail: AtomicBool,
    }

    impl MockController {
        const fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                last_mode: AtomicU8::new(0xFF),
                fail: AtomicBool::new(false),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::Relaxed)
        }

        fn last_mode(&self) -> Option<PowerMode> {
            PowerMode::from_u8(self.last_mode.load(Ordering::Relaxed))
        }
    }

    impl PowerController for MockController {
        fn set_power_mode(&self, mode: PowerMode) -> Result<()> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.last_mode.store(mode as u8, Ordering::Relaxed);
            if self.fail.load(Ordering::Relaxed) {
                return Err(EmbeddedError::HardwareError);
            }
            Ok(())
        }
    }

    #[test]
    fn test_default_manager_has_no_hardware_controller() {
        let pm = PowerManager::new();
        assert!(!pm.has_hardware_controller());
        // Bookkeeping still works: the mode is recorded even without hardware.
        pm.request_mode(PowerMode::LowPower)
            .expect("bookkeeping transition should succeed");
        assert_eq!(pm.current_mode(), PowerMode::LowPower);
    }

    #[test]
    fn test_no_controller_strict_fails_loud() {
        let pm = PowerManager::new();
        // Without a hardware controller the strict API must fail loudly and must
        // NOT record the requested mode.
        let err = pm
            .request_mode_strict(PowerMode::UltraLowPower)
            .expect_err("strict transition without controller must fail");
        assert_eq!(err, EmbeddedError::NoPowerController);
        assert_eq!(pm.current_mode(), PowerMode::HighPerformance);
    }

    #[test]
    fn test_noop_controller_is_explicit() {
        let controller = NoController;
        assert!(!controller.is_hardware());
        assert!(controller.set_power_mode(PowerMode::Balanced).is_ok());
    }

    #[test]
    fn test_manager_delegates_to_controller() {
        let pm = PowerManager::with_controller(MockController::new());
        assert!(pm.has_hardware_controller());

        pm.request_mode(PowerMode::Balanced)
            .expect("delegated transition should succeed");
        assert_eq!(pm.current_mode(), PowerMode::Balanced);
        assert_eq!(pm.controller().calls(), 1);
        assert_eq!(pm.controller().last_mode(), Some(PowerMode::Balanced));

        pm.request_mode(PowerMode::UltraLowPower)
            .expect("second transition should succeed");
        assert_eq!(pm.controller().calls(), 2);
        assert_eq!(pm.controller().last_mode(), Some(PowerMode::UltraLowPower));
    }

    #[test]
    fn test_manager_delegates_sleep_modes() {
        let pm = PowerManager::with_controller(MockController::new());
        // Sleep/DeepSleep also delegate to the controller (which configures wake
        // sources / deep-power state) before the ISA-level WFI.
        pm.request_mode(PowerMode::Sleep)
            .expect("sleep transition should succeed");
        assert_eq!(pm.controller().last_mode(), Some(PowerMode::Sleep));

        pm.request_mode(PowerMode::DeepSleep)
            .expect("deep sleep transition should succeed");
        assert_eq!(pm.controller().last_mode(), Some(PowerMode::DeepSleep));
        assert_eq!(pm.controller().calls(), 2);
    }

    #[test]
    fn test_manager_propagates_controller_error() {
        let pm = PowerManager::with_controller(MockController::new());
        pm.controller().fail.store(true, Ordering::Relaxed);

        let err = pm
            .request_mode(PowerMode::LowPower)
            .expect_err("controller failure must propagate");
        assert_eq!(err, EmbeddedError::HardwareError);
        // The controller was still invoked.
        assert_eq!(pm.controller().calls(), 1);
    }

    #[test]
    fn test_strict_delegates_with_hardware_controller() {
        let pm = PowerManager::with_controller(MockController::new());
        pm.request_mode_strict(PowerMode::LowPower)
            .expect("strict transition with controller should succeed");
        assert_eq!(pm.current_mode(), PowerMode::LowPower);
        assert_eq!(pm.controller().calls(), 1);
    }

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
    fn test_power_manager_sleep_modes() {
        // Sleep / DeepSleep transitions must succeed and update the tracked
        // mode. Under the arm/riscv features these route through the ISA-level
        // WFI hooks (host stubs return Ok), exercising the wiring.
        let pm = PowerManager::new();

        pm.request_mode(PowerMode::Sleep)
            .expect("sleep transition failed");
        assert_eq!(pm.current_mode(), PowerMode::Sleep);

        pm.request_mode(PowerMode::DeepSleep)
            .expect("deep sleep transition failed");
        assert_eq!(pm.current_mode(), PowerMode::DeepSleep);

        pm.request_mode(PowerMode::HighPerformance)
            .expect("wake transition failed");
        assert_eq!(pm.current_mode(), PowerMode::HighPerformance);
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
