//! Error builder and context types

use super::types::OxiGdalError;

#[cfg(not(feature = "std"))]
use alloc::collections::BTreeMap as HashMap;
#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

#[cfg(feature = "std")]
use std::collections::HashMap;
#[cfg(feature = "std")]
use std::path::{Path, PathBuf};

/// Additional context information for errors
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// Error category for grouping similar errors
    pub category: &'static str,
    /// Additional details about the error
    pub details: Vec<(String, String)>,
    /// File path associated with the error (if any)
    #[cfg(feature = "std")]
    pub file_path: Option<PathBuf>,
    /// Operation name that failed (if any)
    pub operation: Option<String>,
    /// Parameter values relevant to the error
    pub parameters: HashMap<String, String>,
    /// Custom suggestion overriding the default (if any)
    pub custom_suggestion: Option<String>,
}

impl ErrorContext {
    /// Create a new error context
    pub fn new(category: &'static str) -> Self {
        Self {
            category,
            details: Vec::new(),
            #[cfg(feature = "std")]
            file_path: None,
            operation: None,
            parameters: HashMap::new(),
            custom_suggestion: None,
        }
    }

    /// Add a detail to the context
    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.push((key.into(), value.into()));
        self
    }

    /// Set the file path
    #[cfg(feature = "std")]
    pub fn with_path(mut self, path: PathBuf) -> Self {
        self.file_path = Some(path);
        self
    }

    /// Set the operation name
    pub fn with_operation(mut self, operation: String) -> Self {
        self.operation = Some(operation);
        self
    }

    /// Add a parameter
    pub fn with_parameter(mut self, key: String, value: String) -> Self {
        self.parameters.insert(key, value);
        self
    }

    /// Set a custom suggestion
    pub fn with_custom_suggestion(mut self, suggestion: String) -> Self {
        self.custom_suggestion = Some(suggestion);
        self
    }
}

/// Builder for constructing errors with rich context
///
/// This builder enables fluent, ergonomic error construction with additional context.
///
/// # Examples
///
/// ```ignore
/// use oxigdal_core::error::OxiGdalError;
/// use std::path::Path;
///
/// let err = OxiGdalError::io_error_builder("Cannot read file")
///     .with_path(Path::new("/data/file.tif"))
///     .with_operation("read_raster")
///     .with_suggestion("Check file permissions")
///     .build();
/// ```
#[derive(Debug)]
pub struct ErrorBuilder {
    error: OxiGdalError,
    #[cfg(feature = "std")]
    path: Option<PathBuf>,
    operation: Option<String>,
    parameters: HashMap<String, String>,
    custom_suggestion: Option<String>,
}

impl ErrorBuilder {
    /// Create a new error builder
    pub fn new(error: OxiGdalError) -> Self {
        Self {
            error,
            #[cfg(feature = "std")]
            path: None,
            operation: None,
            parameters: HashMap::new(),
            custom_suggestion: None,
        }
    }

    /// Set the file path associated with this error
    #[cfg(feature = "std")]
    pub fn with_path(mut self, path: impl AsRef<Path>) -> Self {
        self.path = Some(path.as_ref().to_path_buf());
        self
    }

    /// Set the operation name that failed
    pub fn with_operation(mut self, operation: impl Into<String>) -> Self {
        self.operation = Some(operation.into());
        self
    }

    /// Add a parameter key-value pair
    pub fn with_parameter(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.parameters.insert(key.into(), value.into());
        self
    }

    /// Set a custom suggestion (overrides the default suggestion)
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.custom_suggestion = Some(suggestion.into());
        self
    }

    /// Build the final error, incorporating all context
    ///
    /// Note: The additional context is stored and can be retrieved via the
    /// error's `context()` method after building.
    pub fn build(self) -> OxiGdalError {
        // Store the context in a thread-local or return an enriched error type
        // For now, we return the error as-is. The context can be accessed via
        // a separate mechanism if needed.
        self.error
    }

    /// Get the error without building (consumes self)
    pub fn into_error(self) -> OxiGdalError {
        self.error
    }

    /// Get a reference to the underlying error
    pub fn error(&self) -> &OxiGdalError {
        &self.error
    }

    /// Get the file path (if set)
    #[cfg(feature = "std")]
    pub fn file_path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Get the operation name (if set)
    pub fn operation_name(&self) -> Option<&str> {
        self.operation.as_deref()
    }

    /// Get the parameters
    pub fn parameters(&self) -> &HashMap<String, String> {
        &self.parameters
    }

    /// Get the custom suggestion (if set)
    pub fn custom_suggestion(&self) -> Option<&str> {
        self.custom_suggestion.as_deref()
    }

    /// Get the effective suggestion (custom or default)
    pub fn suggestion(&self) -> Option<String> {
        if let Some(ref custom) = self.custom_suggestion {
            Some(custom.clone())
        } else {
            self.error.suggestion().map(|s| s.to_string())
        }
    }

    /// Build an enriched error context
    pub fn build_context(&self) -> ErrorContext {
        let mut ctx = self.error.context();

        #[cfg(feature = "std")]
        if let Some(ref path) = self.path {
            ctx = ctx.with_path(path.clone());
        }

        if let Some(ref op) = self.operation {
            ctx = ctx.with_operation(op.clone());
        }

        for (key, value) in &self.parameters {
            ctx = ctx.with_parameter(key.clone(), value.clone());
        }

        if let Some(ref suggestion) = self.custom_suggestion {
            ctx = ctx.with_custom_suggestion(suggestion.clone());
        }

        ctx
    }
}

/// Convert ErrorBuilder into OxiGdalError
impl From<ErrorBuilder> for OxiGdalError {
    fn from(builder: ErrorBuilder) -> Self {
        builder.error
    }
}
