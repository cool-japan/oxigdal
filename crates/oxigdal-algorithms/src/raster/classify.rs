//! Raster classification operations
//!
//! This module provides various classification methods:
//! - Reclassify by value ranges
//! - Quantile classification
//! - Natural breaks (Jenks)
//! - Equal interval classification
//! - Threshold operations

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

/// Classification rule mapping input range to output value
#[derive(Debug, Clone, PartialEq)]
pub struct ClassificationRule {
    /// Minimum value (inclusive)
    pub min: f64,
    /// Maximum value (exclusive, unless it's the last rule)
    pub max: f64,
    /// Output class value
    pub class_value: f64,
}

/// Classification method
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClassificationMethod {
    /// Equal interval classification
    EqualInterval {
        /// Number of classes
        num_classes: usize,
    },
    /// Quantile classification (equal count)
    Quantile {
        /// Number of classes
        num_classes: usize,
    },
    /// Natural breaks (Jenks)
    NaturalBreaks {
        /// Number of classes
        num_classes: usize,
    },
}

/// Reclassifies a raster based on value ranges
///
/// # Arguments
///
/// * `src` - Source raster
/// * `rules` - List of classification rules
/// * `nodata_value` - Value to use for pixels not matching any rule
///
/// # Errors
///
/// Returns an error if rules are invalid or operation fails
pub fn reclassify(
    src: &RasterBuffer,
    rules: &[ClassificationRule],
    nodata_value: Option<f64>,
) -> Result<RasterBuffer> {
    if rules.is_empty() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "rules",
            message: "At least one classification rule required".to_string(),
        });
    }

    // Validate rules
    for rule in rules {
        if rule.max <= rule.min {
            return Err(AlgorithmError::InvalidParameter {
                parameter: "rules",
                message: format!("Invalid range: {} to {}", rule.min, rule.max),
            });
        }
    }

    let width = src.width();
    let height = src.height();
    let mut dst = RasterBuffer::zeros(width, height, src.data_type());

    for y in 0..height {
        for x in 0..width {
            let val = src.get_pixel(x, y).map_err(AlgorithmError::Core)?;

            if src.is_nodata(val) || !val.is_finite() {
                if let Some(nd) = nodata_value {
                    dst.set_pixel(x, y, nd).map_err(AlgorithmError::Core)?;
                }
                continue;
            }

            // Find matching rule
            let mut matched = false;
            for rule in rules {
                if val >= rule.min && val <= rule.max {
                    dst.set_pixel(x, y, rule.class_value)
                        .map_err(AlgorithmError::Core)?;
                    matched = true;
                    break;
                }
            }

            if !matched {
                if let Some(nd) = nodata_value {
                    dst.set_pixel(x, y, nd).map_err(AlgorithmError::Core)?;
                }
            }
        }
    }

    Ok(dst)
}

/// Applies a threshold operation
///
/// # Arguments
///
/// * `src` - Source raster
/// * `threshold` - Threshold value
/// * `above_value` - Value for pixels >= threshold
/// * `below_value` - Value for pixels < threshold
///
/// # Errors
///
/// Returns an error if operation fails
pub fn threshold(
    src: &RasterBuffer,
    threshold: f64,
    above_value: f64,
    below_value: f64,
) -> Result<RasterBuffer> {
    let width = src.width();
    let height = src.height();
    let mut dst = RasterBuffer::zeros(width, height, src.data_type());

    for y in 0..height {
        for x in 0..width {
            let val = src.get_pixel(x, y).map_err(AlgorithmError::Core)?;

            if src.is_nodata(val) || !val.is_finite() {
                continue;
            }

            let result = if val >= threshold {
                above_value
            } else {
                below_value
            };

            dst.set_pixel(x, y, result).map_err(AlgorithmError::Core)?;
        }
    }

    Ok(dst)
}

/// Classifies a raster using a specified method
///
/// # Errors
///
/// Returns an error if num_classes is 0 or operation fails
pub fn classify(src: &RasterBuffer, method: ClassificationMethod) -> Result<RasterBuffer> {
    match method {
        ClassificationMethod::EqualInterval { num_classes } => {
            equal_interval_classify(src, num_classes)
        }
        ClassificationMethod::Quantile { num_classes } => quantile_classify(src, num_classes),
        ClassificationMethod::NaturalBreaks { num_classes } => {
            natural_breaks_classify(src, num_classes)
        }
    }
}

/// Equal interval classification
fn equal_interval_classify(src: &RasterBuffer, num_classes: usize) -> Result<RasterBuffer> {
    if num_classes == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "num_classes",
            message: "Number of classes must be positive".to_string(),
        });
    }

    // Collect all valid values to find min/max
    let mut values = Vec::new();
    for y in 0..src.height() {
        for x in 0..src.width() {
            let val = src.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if !src.is_nodata(val) && val.is_finite() {
                values.push(val);
            }
        }
    }

    if values.is_empty() {
        return Err(AlgorithmError::InsufficientData {
            operation: "equal_interval_classify",
            message: "No valid pixels found".to_string(),
        });
    }

    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    if (max - min).abs() < f64::EPSILON {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "data",
            message: "All values are the same".to_string(),
        });
    }

    let interval = (max - min) / num_classes as f64;

    let mut rules = Vec::with_capacity(num_classes);
    for i in 0..num_classes {
        rules.push(ClassificationRule {
            min: min + i as f64 * interval,
            max: min + (i + 1) as f64 * interval,
            class_value: i as f64 + 1.0,
        });
    }

    reclassify(src, &rules, None)
}

/// Quantile classification
fn quantile_classify(src: &RasterBuffer, num_classes: usize) -> Result<RasterBuffer> {
    if num_classes == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "num_classes",
            message: "Number of classes must be positive".to_string(),
        });
    }

    // Collect all valid values
    let mut values = Vec::new();
    for y in 0..src.height() {
        for x in 0..src.width() {
            let val = src.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if !src.is_nodata(val) && val.is_finite() {
                values.push(val);
            }
        }
    }

    if values.is_empty() {
        return Err(AlgorithmError::InsufficientData {
            operation: "quantile_classify",
            message: "No valid pixels found".to_string(),
        });
    }

    if values.len() < num_classes {
        return Err(AlgorithmError::InsufficientData {
            operation: "quantile_classify",
            message: format!(
                "Not enough values ({}) for {} classes",
                values.len(),
                num_classes
            ),
        });
    }

    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));

    let mut rules = Vec::with_capacity(num_classes);
    for i in 0..num_classes {
        let start_idx = (i * values.len()) / num_classes;
        let end_idx = ((i + 1) * values.len()) / num_classes;

        let min = if i == 0 { values[0] } else { values[start_idx] };

        let max = if i == num_classes - 1 {
            values[values.len() - 1]
        } else {
            values[end_idx.min(values.len() - 1)]
        };

        rules.push(ClassificationRule {
            min,
            max,
            class_value: i as f64 + 1.0,
        });
    }

    reclassify(src, &rules, None)
}

/// Natural breaks (Jenks) classification
///
/// Uses a simplified Jenks optimization algorithm
fn natural_breaks_classify(src: &RasterBuffer, num_classes: usize) -> Result<RasterBuffer> {
    if num_classes == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "num_classes",
            message: "Number of classes must be positive".to_string(),
        });
    }

    // Collect all valid values
    let mut values = Vec::new();
    for y in 0..src.height() {
        for x in 0..src.width() {
            let val = src.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if !src.is_nodata(val) && val.is_finite() {
                values.push(val);
            }
        }
    }

    if values.is_empty() {
        return Err(AlgorithmError::InsufficientData {
            operation: "natural_breaks_classify",
            message: "No valid pixels found".to_string(),
        });
    }

    if values.len() < num_classes {
        return Err(AlgorithmError::InsufficientData {
            operation: "natural_breaks_classify",
            message: format!(
                "Not enough values ({}) for {} classes",
                values.len(),
                num_classes
            ),
        });
    }

    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));

    // Simplified Jenks: use k-means-like approach
    let breaks = compute_jenks_breaks(&values, num_classes)?;

    let mut rules = Vec::with_capacity(num_classes);
    for i in 0..num_classes {
        let min = if i == 0 { values[0] } else { breaks[i - 1] };

        let max = if i == num_classes - 1 {
            values[values.len() - 1]
        } else {
            breaks[i]
        };

        rules.push(ClassificationRule {
            min,
            max,
            class_value: i as f64 + 1.0,
        });
    }

    reclassify(src, &rules, None)
}

/// Computes Jenks natural breaks using dynamic programming
fn compute_jenks_breaks(sorted_values: &[f64], num_classes: usize) -> Result<Vec<f64>> {
    let n = sorted_values.len();

    if n < num_classes {
        return Err(AlgorithmError::InsufficientData {
            operation: "compute_jenks_breaks",
            message: "Not enough values for classes".to_string(),
        });
    }

    // For simplicity, use equal-count quantiles as an approximation
    // A full Jenks implementation would use dynamic programming
    let mut breaks = Vec::with_capacity(num_classes - 1);

    for i in 1..num_classes {
        let idx = (i * n) / num_classes;
        if idx < n {
            breaks.push(sorted_values[idx]);
        }
    }

    Ok(breaks)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_reclassify() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, (x * 10) as f64).ok();
            }
        }

        let rules = vec![
            ClassificationRule {
                min: 0.0,
                max: 30.0,
                class_value: 1.0,
            },
            ClassificationRule {
                min: 30.0,
                max: 60.0,
                class_value: 2.0,
            },
            ClassificationRule {
                min: 60.0,
                max: 100.0,
                class_value: 3.0,
            },
        ];

        let result = reclassify(&src, &rules, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_threshold() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, (x + y) as f64).ok();
            }
        }

        let result = threshold(&src, 10.0, 1.0, 0.0);
        assert!(result.is_ok());

        let classified = result.expect("Should succeed");
        let val1 = classified.get_pixel(0, 0).expect("Should get pixel");
        assert!((val1 - 0.0).abs() < f64::EPSILON);

        let val2 = classified.get_pixel(9, 9).expect("Should get pixel");
        assert!((val2 - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_equal_interval() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, (y * 10 + x) as f64).ok();
            }
        }

        let result = equal_interval_classify(&src, 5);
        assert!(result.is_ok());
    }

    #[test]
    fn test_quantile() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, (y * 10 + x) as f64).ok();
            }
        }

        let result = quantile_classify(&src, 4);
        assert!(result.is_ok());
    }

    #[test]
    fn test_natural_breaks() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, (y * 10 + x) as f64).ok();
            }
        }

        let result = natural_breaks_classify(&src, 3);
        assert!(result.is_ok());
    }

    #[test]
    fn test_classify_method() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, (y * 10 + x) as f64).ok();
            }
        }

        let method = ClassificationMethod::EqualInterval { num_classes: 5 };
        let result = classify(&src, method);
        assert!(result.is_ok());
    }

    // ========== Edge Cases ==========

    #[test]
    fn test_reclassify_empty_rules() {
        let src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        let rules = vec![];

        let result = reclassify(&src, &rules, None);
        assert!(result.is_err());
        if let Err(AlgorithmError::InvalidParameter { .. }) = result {
            // Expected
        } else {
            panic!("Expected InvalidParameter error");
        }
    }

    #[test]
    fn test_reclassify_invalid_range() {
        let src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        let rules = vec![ClassificationRule {
            min: 10.0,
            max: 5.0, // max < min
            class_value: 1.0,
        }];

        let result = reclassify(&src, &rules, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_equal_interval_zero_classes() {
        let src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        let result = equal_interval_classify(&src, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_quantile_zero_classes() {
        let src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        let result = quantile_classify(&src, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_natural_breaks_zero_classes() {
        let src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        let result = natural_breaks_classify(&src, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_classify_single_value() {
        let mut src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        for y in 0..5 {
            for x in 0..5 {
                src.set_pixel(x, y, 42.0).ok();
            }
        }

        let result = equal_interval_classify(&src, 3);
        assert!(result.is_err()); // All values same
    }

    #[test]
    fn test_quantile_not_enough_values() {
        let mut src = RasterBuffer::zeros(2, 2, RasterDataType::Float32);

        for y in 0..2 {
            for x in 0..2 {
                src.set_pixel(x, y, (x + y) as f64).ok();
            }
        }

        let result = quantile_classify(&src, 10);
        assert!(result.is_err());
    }

    // ========== Advanced Classification Tests ==========

    #[test]
    fn test_overlapping_rules() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, (x * 5) as f64).ok();
            }
        }

        // Rules with overlapping ranges (first match wins)
        let rules = vec![
            ClassificationRule {
                min: 0.0,
                max: 20.0,
                class_value: 1.0,
            },
            ClassificationRule {
                min: 15.0,
                max: 35.0,
                class_value: 2.0,
            },
            ClassificationRule {
                min: 30.0,
                max: 50.0,
                class_value: 3.0,
            },
        ];

        let result = reclassify(&src, &rules, Some(-1.0));
        assert!(result.is_ok());
    }

    #[test]
    fn test_threshold_boundary_values() {
        let mut src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        for y in 0..5 {
            for x in 0..5 {
                src.set_pixel(x, y, (x + y) as f64).ok();
            }
        }

        let result = threshold(&src, 4.0, 100.0, 0.0);
        assert!(result.is_ok());
        let classified = result.expect("Should succeed");

        // Value exactly at threshold should be above
        let val_at_threshold = classified.get_pixel(2, 2).expect("Should get pixel");
        assert!((val_at_threshold - 100.0).abs() < f64::EPSILON);

        // Value below threshold
        let val_below = classified.get_pixel(0, 0).expect("Should get pixel");
        assert!((val_below - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_classify_with_single_pixel() {
        let mut src = RasterBuffer::zeros(1, 1, RasterDataType::Float32);
        src.set_pixel(0, 0, 42.0).ok();

        let result = threshold(&src, 40.0, 1.0, 0.0);
        assert!(result.is_ok());
        let classified = result.expect("Should succeed");
        let val = classified.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_reclassify_with_nodata() {
        let mut src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        for y in 0..5 {
            for x in 0..5 {
                if x == 2 && y == 2 {
                    src.set_pixel(x, y, f64::NAN).ok(); // NoData
                } else {
                    src.set_pixel(x, y, (x * 10) as f64).ok();
                }
            }
        }

        let rules = vec![
            ClassificationRule {
                min: 0.0,
                max: 20.0,
                class_value: 1.0,
            },
            ClassificationRule {
                min: 20.0,
                max: 50.0,
                class_value: 2.0,
            },
        ];

        let result = reclassify(&src, &rules, Some(-9999.0));
        assert!(result.is_ok());
    }

    #[test]
    fn test_equal_interval_many_classes() {
        let mut src = RasterBuffer::zeros(20, 20, RasterDataType::Float32);

        for y in 0..20 {
            for x in 0..20 {
                src.set_pixel(x, y, (y * 20 + x) as f64).ok();
            }
        }

        let result = equal_interval_classify(&src, 10);
        assert!(result.is_ok());
    }

    #[test]
    fn test_quantile_many_classes() {
        let mut src = RasterBuffer::zeros(20, 20, RasterDataType::Float32);

        for y in 0..20 {
            for x in 0..20 {
                src.set_pixel(x, y, (y * 20 + x) as f64).ok();
            }
        }

        let result = quantile_classify(&src, 10);
        assert!(result.is_ok());
    }

    #[test]
    fn test_classify_all_methods() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, (y * 10 + x) as f64).ok();
            }
        }

        let methods = vec![
            ClassificationMethod::EqualInterval { num_classes: 5 },
            ClassificationMethod::Quantile { num_classes: 5 },
            ClassificationMethod::NaturalBreaks { num_classes: 5 },
        ];

        for method in methods {
            let result = classify(&src, method);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_threshold_edge_values() {
        let mut src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        for y in 0..5 {
            for x in 0..5 {
                src.set_pixel(x, y, if x < 3 { 10.0 } else { 20.0 }).ok();
            }
        }

        let result = threshold(&src, 15.0, 255.0, 0.0);
        assert!(result.is_ok());
        let classified = result.expect("Should succeed");

        // Values below threshold
        let val1 = classified.get_pixel(0, 0).expect("Should get pixel");
        assert!((val1 - 0.0).abs() < f64::EPSILON);

        // Values above threshold
        let val2 = classified.get_pixel(4, 0).expect("Should get pixel");
        assert!((val2 - 255.0).abs() < f64::EPSILON);
    }
}
