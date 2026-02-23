//! Reconnection strategy and configuration

use std::time::Duration;

/// Reconnection strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReconnectStrategy {
    /// Fixed delay between reconnection attempts
    Fixed,
    /// Exponential backoff with jitter
    #[default]
    ExponentialBackoff,
    /// Linear backoff
    LinearBackoff,
}

/// Reconnection options
#[derive(Debug, Clone)]
pub struct ReconnectOptions {
    /// Reconnection strategy
    pub strategy: ReconnectStrategy,
    /// Initial delay before first reconnection attempt
    pub initial_delay: Duration,
    /// Maximum delay between reconnection attempts
    pub max_delay: Duration,
    /// Maximum number of reconnection attempts (None = infinite)
    pub max_attempts: Option<usize>,
    /// Backoff multiplier for exponential strategy
    pub backoff_multiplier: f64,
    /// Backoff increment for linear strategy
    pub backoff_increment: Duration,
    /// Enable jitter for backoff strategies
    pub enable_jitter: bool,
}

impl Default for ReconnectOptions {
    fn default() -> Self {
        Self {
            strategy: ReconnectStrategy::default(),
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            max_attempts: None,
            backoff_multiplier: 2.0,
            backoff_increment: Duration::from_secs(5),
            enable_jitter: true,
        }
    }
}

impl ReconnectOptions {
    /// Create new reconnection options with fixed strategy
    pub fn fixed(delay: Duration) -> Self {
        Self {
            strategy: ReconnectStrategy::Fixed,
            initial_delay: delay,
            max_delay: delay,
            ..Default::default()
        }
    }

    /// Create new reconnection options with exponential backoff
    pub fn exponential_backoff(
        initial_delay: Duration,
        max_delay: Duration,
        multiplier: f64,
    ) -> Self {
        Self {
            strategy: ReconnectStrategy::ExponentialBackoff,
            initial_delay,
            max_delay,
            backoff_multiplier: multiplier,
            ..Default::default()
        }
    }

    /// Create new reconnection options with linear backoff
    pub fn linear_backoff(
        initial_delay: Duration,
        max_delay: Duration,
        increment: Duration,
    ) -> Self {
        Self {
            strategy: ReconnectStrategy::LinearBackoff,
            initial_delay,
            max_delay,
            backoff_increment: increment,
            ..Default::default()
        }
    }

    /// Set maximum number of reconnection attempts
    pub fn with_max_attempts(mut self, max_attempts: usize) -> Self {
        self.max_attempts = Some(max_attempts);
        self
    }

    /// Enable or disable jitter
    pub fn with_jitter(mut self, enable: bool) -> Self {
        self.enable_jitter = enable;
        self
    }

    /// Calculate delay for a given attempt number
    pub fn calculate_delay(&self, attempt: usize) -> Duration {
        let base_delay = match self.strategy {
            ReconnectStrategy::Fixed => self.initial_delay,
            ReconnectStrategy::ExponentialBackoff => {
                let multiplier = self.backoff_multiplier.powi(attempt as i32);
                let delay_secs = self.initial_delay.as_secs_f64() * multiplier;
                Duration::from_secs_f64(delay_secs.min(self.max_delay.as_secs_f64()))
            }
            ReconnectStrategy::LinearBackoff => {
                let delay_secs = self.initial_delay.as_secs()
                    + (self.backoff_increment.as_secs() * attempt as u64);
                Duration::from_secs(delay_secs.min(self.max_delay.as_secs()))
            }
        };

        if self.enable_jitter {
            apply_jitter(base_delay)
        } else {
            base_delay
        }
    }

    /// Check if should attempt reconnection
    pub fn should_reconnect(&self, attempt: usize) -> bool {
        if let Some(max) = self.max_attempts {
            attempt < max
        } else {
            true
        }
    }
}

/// Apply jitter to a duration (±25% random variation)
fn apply_jitter(duration: Duration) -> Duration {
    let secs = duration.as_secs_f64();
    // Simple pseudo-random jitter using current time
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);

    let jitter_factor = 0.75 + ((nanos % 1000) as f64 / 2000.0); // Range: 0.75 to 1.25
    let jittered_secs = secs * jitter_factor;

    Duration::from_secs_f64(jittered_secs)
}

/// Reconnection state tracker
#[derive(Debug)]
#[allow(dead_code)]
pub struct ReconnectState {
    /// Current attempt number
    pub attempt: usize,
    /// Total reconnections succeeded
    pub success_count: usize,
    /// Total reconnections failed
    pub failure_count: usize,
    /// Last reconnection time
    pub last_attempt: Option<std::time::Instant>,
}

impl Default for ReconnectState {
    fn default() -> Self {
        Self::new()
    }
}

// Public API for reconnection state tracking - reserved for future use by consumers
#[allow(dead_code)]
impl ReconnectState {
    /// Create new reconnection state
    pub fn new() -> Self {
        Self {
            attempt: 0,
            success_count: 0,
            failure_count: 0,
            last_attempt: None,
        }
    }

    /// Record a reconnection attempt
    pub fn record_attempt(&mut self) {
        self.attempt += 1;
        self.last_attempt = Some(std::time::Instant::now());
    }

    /// Record a successful reconnection
    pub fn record_success(&mut self) {
        self.success_count += 1;
        self.attempt = 0;
    }

    /// Record a failed reconnection
    pub fn record_failure(&mut self) {
        self.failure_count += 1;
    }

    /// Reset state
    pub fn reset(&mut self) {
        self.attempt = 0;
        self.last_attempt = None;
    }

    /// Get time since last attempt
    pub fn time_since_last_attempt(&self) -> Option<Duration> {
        self.last_attempt.map(|t| t.elapsed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_strategy() {
        let opts = ReconnectOptions::fixed(Duration::from_secs(5));
        assert_eq!(opts.strategy, ReconnectStrategy::Fixed);

        // Disable jitter for testing
        let opts = opts.with_jitter(false);
        assert_eq!(opts.calculate_delay(0), Duration::from_secs(5));
        assert_eq!(opts.calculate_delay(1), Duration::from_secs(5));
        assert_eq!(opts.calculate_delay(10), Duration::from_secs(5));
    }

    #[test]
    fn test_exponential_backoff() {
        let opts = ReconnectOptions::exponential_backoff(
            Duration::from_secs(1),
            Duration::from_secs(60),
            2.0,
        )
        .with_jitter(false);

        assert_eq!(opts.calculate_delay(0), Duration::from_secs(1)); // 1 * 2^0 = 1
        assert_eq!(opts.calculate_delay(1), Duration::from_secs(2)); // 1 * 2^1 = 2
        assert_eq!(opts.calculate_delay(2), Duration::from_secs(4)); // 1 * 2^2 = 4
        assert_eq!(opts.calculate_delay(3), Duration::from_secs(8)); // 1 * 2^3 = 8

        // Should cap at max_delay
        assert_eq!(opts.calculate_delay(10), Duration::from_secs(60));
    }

    #[test]
    fn test_linear_backoff() {
        let opts = ReconnectOptions::linear_backoff(
            Duration::from_secs(1),
            Duration::from_secs(30),
            Duration::from_secs(5),
        )
        .with_jitter(false);

        assert_eq!(opts.calculate_delay(0), Duration::from_secs(1)); // 1 + 5*0 = 1
        assert_eq!(opts.calculate_delay(1), Duration::from_secs(6)); // 1 + 5*1 = 6
        assert_eq!(opts.calculate_delay(2), Duration::from_secs(11)); // 1 + 5*2 = 11
        assert_eq!(opts.calculate_delay(3), Duration::from_secs(16)); // 1 + 5*3 = 16

        // Should cap at max_delay
        assert_eq!(opts.calculate_delay(10), Duration::from_secs(30));
    }

    #[test]
    fn test_max_attempts() {
        let opts = ReconnectOptions::default().with_max_attempts(3);

        assert!(opts.should_reconnect(0));
        assert!(opts.should_reconnect(1));
        assert!(opts.should_reconnect(2));
        assert!(!opts.should_reconnect(3));
        assert!(!opts.should_reconnect(4));
    }

    #[test]
    fn test_infinite_attempts() {
        let opts = ReconnectOptions::default();
        assert_eq!(opts.max_attempts, None);

        assert!(opts.should_reconnect(0));
        assert!(opts.should_reconnect(100));
        assert!(opts.should_reconnect(1000));
    }

    #[test]
    fn test_reconnect_state() {
        let mut state = ReconnectState::new();
        assert_eq!(state.attempt, 0);
        assert_eq!(state.success_count, 0);
        assert_eq!(state.failure_count, 0);

        state.record_attempt();
        assert_eq!(state.attempt, 1);

        state.record_failure();
        assert_eq!(state.failure_count, 1);

        state.record_attempt();
        assert_eq!(state.attempt, 2);

        state.record_success();
        assert_eq!(state.success_count, 1);
        assert_eq!(state.attempt, 0); // Reset on success

        state.reset();
        assert_eq!(state.attempt, 0);
    }

    #[test]
    fn test_jitter() {
        let opts = ReconnectOptions::fixed(Duration::from_secs(10)).with_jitter(true);

        // Jitter should produce values between 7.5s and 12.5s
        let delay = opts.calculate_delay(0);
        let delay_secs = delay.as_secs();
        assert!((7..=13).contains(&delay_secs));
    }
}
