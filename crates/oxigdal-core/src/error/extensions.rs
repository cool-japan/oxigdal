//! Error handling extensions and utilities

use super::types::OxiGdalError;

#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

/// Macro for precondition checking with automatic error creation
///
/// # Examples
///
/// ```ignore
/// use oxigdal_core::ensure;
/// use oxigdal_core::error::{OxiGdalError, Result};
///
/// fn process_data(size: usize) -> Result<()> {
///     ensure!(size > 0, InvalidParameter {
///         parameter: "size",
///         message: "size must be positive".to_string()
///     });
///     Ok(())
/// }
/// ```
#[macro_export]
macro_rules! ensure {
    ($cond:expr, $err:expr) => {
        if !($cond) {
            return Err($crate::error::OxiGdalError::$err.into());
        }
    };
    ($cond:expr, $err:ident { $($field:ident: $value:expr),* $(,)? }) => {
        if !($cond) {
            return Err($crate::error::OxiGdalError::$err {
                $($field: $value),*
            }.into());
        }
    };
}

/// Trait for adding context to errors
pub trait ResultExt<T> {
    /// Add context to an error
    fn context(self, msg: impl Into<String>) -> crate::error::Result<T>;

    /// Add context with a lazy message (only evaluated on error)
    fn with_context<F>(self, f: F) -> crate::error::Result<T>
    where
        F: FnOnce() -> String;
}

impl<T> ResultExt<T> for crate::error::Result<T> {
    fn context(self, msg: impl Into<String>) -> crate::error::Result<T> {
        self.map_err(|_| OxiGdalError::Internal {
            message: msg.into(),
        })
    }

    fn with_context<F>(self, f: F) -> crate::error::Result<T>
    where
        F: FnOnce() -> String,
    {
        self.map_err(|_| OxiGdalError::Internal { message: f() })
    }
}

/// Aggregate multiple errors into a single error
#[derive(Debug)]
pub struct ErrorAggregator {
    errors: Vec<OxiGdalError>,
}

impl ErrorAggregator {
    /// Create a new error aggregator
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    /// Add an error to the aggregator
    pub fn add(&mut self, error: OxiGdalError) {
        self.errors.push(error);
    }

    /// Add a result, collecting any errors
    pub fn add_result<T>(&mut self, result: crate::error::Result<T>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                self.add(error);
                None
            }
        }
    }

    /// Check if any errors were collected
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Get the number of errors collected
    pub fn count(&self) -> usize {
        self.errors.len()
    }

    /// Convert to a result, failing if any errors were collected
    pub fn into_result(self) -> crate::error::Result<()> {
        if self.errors.is_empty() {
            Ok(())
        } else {
            let count = self.errors.len();
            let first = &self.errors[0];
            Err(OxiGdalError::Internal {
                message: format!(
                    "Multiple errors occurred ({} total). First error: {}",
                    count, first
                ),
            })
        }
    }

    /// Get all collected errors
    pub fn into_errors(self) -> Vec<OxiGdalError> {
        self.errors
    }
}

impl Default for ErrorAggregator {
    fn default() -> Self {
        Self::new()
    }
}
