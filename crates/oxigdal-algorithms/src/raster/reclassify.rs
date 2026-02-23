//! Raster reclassification

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;

/// Reclassification rule
#[derive(Debug, Clone, Copy)]
pub struct ReclassRule {
    /// Minimum value (inclusive)
    pub min: f64,
    /// Maximum value (inclusive)
    pub max: f64,
    /// New value for this range
    pub new_value: f64,
}

impl ReclassRule {
    /// Creates a new reclassification rule
    #[must_use]
    pub const fn new(min: f64, max: f64, new_value: f64) -> Self {
        Self {
            min,
            max,
            new_value,
        }
    }

    /// Checks if a value falls within this rule's range
    #[must_use]
    pub fn matches(&self, value: f64) -> bool {
        value >= self.min && value <= self.max
    }
}

/// Reclassifies a raster based on rules
pub fn reclassify(src: &RasterBuffer, rules: &[ReclassRule]) -> Result<RasterBuffer> {
    if rules.is_empty() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "rules",
            message: "At least one rule required".to_string(),
        });
    }

    let mut result = RasterBuffer::zeros(src.width(), src.height(), src.data_type());

    for y in 0..src.height() {
        for x in 0..src.width() {
            let val = src.get_pixel(x, y).map_err(AlgorithmError::Core)?;

            let new_val = rules
                .iter()
                .find(|rule| rule.matches(val))
                .map(|rule| rule.new_value)
                .unwrap_or(val); // Keep original if no match

            result
                .set_pixel(x, y, new_val)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_reclassify() {
        let mut src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        src.set_pixel(0, 0, 5.0).ok();
        src.set_pixel(1, 1, 15.0).ok();

        let rules = [
            ReclassRule::new(0.0, 10.0, 1.0),
            ReclassRule::new(10.0, 20.0, 2.0),
        ];

        let result = reclassify(&src, &rules);
        assert!(result.is_ok());
    }
}
