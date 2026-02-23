//! Error types for embedded OxiGDAL operations
//!
//! This module provides error types optimized for no_std environments with minimal memory overhead.

use core::fmt;

#[cfg(feature = "std")]
use std::error::Error as StdError;

/// Result type alias for embedded operations
pub type Result<T> = core::result::Result<T, EmbeddedError>;

/// Error types for embedded OxiGDAL operations
///
/// Designed to be lightweight and suitable for no_std environments
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EmbeddedError {
    /// Memory allocation failed
    AllocationFailed,

    /// Memory pool exhausted
    PoolExhausted,

    /// Buffer too small for operation
    BufferTooSmall {
        /// Required size
        required: usize,
        /// Available size
        available: usize,
    },

    /// Invalid buffer alignment
    InvalidAlignment {
        /// Required alignment
        required: usize,
        /// Actual alignment
        actual: usize,
    },

    /// Operation not supported in current configuration
    UnsupportedOperation,

    /// Invalid parameter provided
    InvalidParameter,

    /// Out of bounds access
    OutOfBounds {
        /// Index attempted
        index: usize,
        /// Maximum valid index
        max: usize,
    },

    /// Power mode transition failed
    PowerModeTransitionFailed,

    /// Real-time deadline missed
    DeadlineMissed {
        /// Actual time taken (microseconds)
        actual_us: u64,
        /// Deadline (microseconds)
        deadline_us: u64,
    },

    /// Resource busy
    ResourceBusy,

    /// Timeout occurred
    Timeout,

    /// Initialization failed
    InitializationFailed,

    /// Hardware error
    HardwareError,

    /// Invalid state for operation
    InvalidState,

    /// Data format error
    FormatError,

    /// Checksum mismatch
    ChecksumMismatch,

    /// Target-specific error
    TargetSpecific(u8),
}

impl EmbeddedError {
    /// Returns true if the error is recoverable
    pub const fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::BufferTooSmall { .. }
                | Self::ResourceBusy
                | Self::Timeout
                | Self::DeadlineMissed { .. }
        )
    }

    /// Returns true if the error is a resource exhaustion error
    pub const fn is_resource_exhaustion(&self) -> bool {
        matches!(
            self,
            Self::AllocationFailed | Self::PoolExhausted | Self::BufferTooSmall { .. }
        )
    }

    /// Returns true if the error is a validation error
    pub const fn is_validation_error(&self) -> bool {
        matches!(
            self,
            Self::InvalidParameter
                | Self::InvalidAlignment { .. }
                | Self::OutOfBounds { .. }
                | Self::FormatError
                | Self::ChecksumMismatch
        )
    }

    /// Returns the error code as a u32 for FFI boundaries
    pub const fn error_code(&self) -> u32 {
        match self {
            Self::AllocationFailed => 1,
            Self::PoolExhausted => 2,
            Self::BufferTooSmall { .. } => 3,
            Self::InvalidAlignment { .. } => 4,
            Self::UnsupportedOperation => 5,
            Self::InvalidParameter => 6,
            Self::OutOfBounds { .. } => 7,
            Self::PowerModeTransitionFailed => 8,
            Self::DeadlineMissed { .. } => 9,
            Self::ResourceBusy => 10,
            Self::Timeout => 11,
            Self::InitializationFailed => 12,
            Self::HardwareError => 13,
            Self::InvalidState => 14,
            Self::FormatError => 15,
            Self::ChecksumMismatch => 16,
            Self::TargetSpecific(_) => 100,
        }
    }
}

impl fmt::Display for EmbeddedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed => write!(f, "Memory allocation failed"),
            Self::PoolExhausted => write!(f, "Memory pool exhausted"),
            Self::BufferTooSmall {
                required,
                available,
            } => write!(
                f,
                "Buffer too small: required {} bytes, available {} bytes",
                required, available
            ),
            Self::InvalidAlignment { required, actual } => write!(
                f,
                "Invalid alignment: required {}, got {}",
                required, actual
            ),
            Self::UnsupportedOperation => write!(f, "Operation not supported"),
            Self::InvalidParameter => write!(f, "Invalid parameter"),
            Self::OutOfBounds { index, max } => {
                write!(f, "Out of bounds: index {} exceeds max {}", index, max)
            }
            Self::PowerModeTransitionFailed => write!(f, "Power mode transition failed"),
            Self::DeadlineMissed {
                actual_us,
                deadline_us,
            } => write!(
                f,
                "Real-time deadline missed: {} us > {} us",
                actual_us, deadline_us
            ),
            Self::ResourceBusy => write!(f, "Resource is busy"),
            Self::Timeout => write!(f, "Operation timed out"),
            Self::InitializationFailed => write!(f, "Initialization failed"),
            Self::HardwareError => write!(f, "Hardware error"),
            Self::InvalidState => write!(f, "Invalid state"),
            Self::FormatError => write!(f, "Data format error"),
            Self::ChecksumMismatch => write!(f, "Checksum mismatch"),
            Self::TargetSpecific(code) => write!(f, "Target-specific error: {}", code),
        }
    }
}

#[cfg(feature = "std")]
impl StdError for EmbeddedError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_categories() {
        let err = EmbeddedError::BufferTooSmall {
            required: 100,
            available: 50,
        };
        assert!(err.is_recoverable());
        assert!(err.is_resource_exhaustion());
        assert!(!err.is_validation_error());

        let err = EmbeddedError::InvalidParameter;
        assert!(!err.is_recoverable());
        assert!(!err.is_resource_exhaustion());
        assert!(err.is_validation_error());
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(EmbeddedError::AllocationFailed.error_code(), 1);
        assert_eq!(EmbeddedError::PoolExhausted.error_code(), 2);
        assert_eq!(EmbeddedError::TargetSpecific(42).error_code(), 100);
    }
}
