//! AWS CloudWatch exporter.

use super::{Metric, MetricExporter};
use crate::error::Result;

/// CloudWatch exporter.
pub struct CloudWatchExporter {
    namespace: String,
}

impl CloudWatchExporter {
    /// Create a new CloudWatch exporter.
    pub fn new(namespace: String) -> Self {
        Self { namespace }
    }

    /// Get the namespace.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }
}

impl MetricExporter for CloudWatchExporter {
    fn export(&self, _metrics: &[Metric]) -> Result<()> {
        // Placeholder for CloudWatch integration
        // Would use AWS SDK to put metrics
        Ok(())
    }

    fn flush(&self) -> Result<()> {
        Ok(())
    }

    fn name(&self) -> &str {
        "cloudwatch"
    }
}
