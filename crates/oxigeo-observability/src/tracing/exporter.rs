//! Trace exporters (Jaeger, Zipkin, OTLP).

use crate::error::{ObservabilityError, Result};

/// Trace exporter configuration.
#[derive(Debug, Clone)]
pub enum ExporterConfig {
    /// Jaeger exporter.
    Jaeger {
        /// The Jaeger collector endpoint URL.
        endpoint: String,
    },

    /// Zipkin exporter.
    Zipkin {
        /// The Zipkin collector endpoint URL.
        endpoint: String,
    },

    /// OTLP exporter.
    Otlp {
        /// The OTLP collector endpoint URL.
        endpoint: String,
    },

    /// Stdout exporter (for development).
    Stdout,
}

impl ExporterConfig {
    /// Create Jaeger exporter configuration.
    pub fn jaeger(endpoint: impl Into<String>) -> Self {
        Self::Jaeger {
            endpoint: endpoint.into(),
        }
    }

    /// Create Zipkin exporter configuration.
    pub fn zipkin(endpoint: impl Into<String>) -> Self {
        Self::Zipkin {
            endpoint: endpoint.into(),
        }
    }

    /// Create OTLP exporter configuration.
    pub fn otlp(endpoint: impl Into<String>) -> Self {
        Self::Otlp {
            endpoint: endpoint.into(),
        }
    }

    /// Validate the exporter configuration.
    pub fn validate(&self) -> Result<()> {
        match self {
            ExporterConfig::Jaeger { endpoint } => {
                if endpoint.is_empty() {
                    return Err(ObservabilityError::InvalidConfig(
                        "Jaeger endpoint cannot be empty".to_string(),
                    ));
                }
            }
            ExporterConfig::Zipkin { endpoint } => {
                if endpoint.is_empty() {
                    return Err(ObservabilityError::InvalidConfig(
                        "Zipkin endpoint cannot be empty".to_string(),
                    ));
                }
            }
            ExporterConfig::Otlp { endpoint } => {
                if endpoint.is_empty() {
                    return Err(ObservabilityError::InvalidConfig(
                        "OTLP endpoint cannot be empty".to_string(),
                    ));
                }
            }
            ExporterConfig::Stdout => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exporter_config() {
        let config = ExporterConfig::jaeger("localhost:6831");
        assert!(config.validate().is_ok());

        let config = ExporterConfig::jaeger("");
        assert!(config.validate().is_err());
    }
}
