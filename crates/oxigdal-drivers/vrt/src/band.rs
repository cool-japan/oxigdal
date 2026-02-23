//! VRT virtual band configuration

use crate::error::{Result, VrtError};
use crate::source::VrtSource;
use oxigdal_core::types::{ColorInterpretation, NoDataValue, RasterDataType};
use serde::{Deserialize, Serialize};

/// VRT band configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VrtBand {
    /// Band number (1-based)
    pub band: usize,
    /// Data type
    pub data_type: RasterDataType,
    /// Color interpretation
    pub color_interp: ColorInterpretation,
    /// NoData value
    pub nodata: NoDataValue,
    /// Sources for this band
    pub sources: Vec<VrtSource>,
    /// Block size (tile dimensions)
    pub block_size: Option<(u32, u32)>,
    /// Pixel function (for on-the-fly computation)
    pub pixel_function: Option<PixelFunction>,
    /// Offset for scaling
    pub offset: Option<f64>,
    /// Scale factor
    pub scale: Option<f64>,
    /// Color table
    pub color_table: Option<ColorTable>,
}

impl VrtBand {
    /// Creates a new VRT band
    pub fn new(band: usize, data_type: RasterDataType) -> Self {
        Self {
            band,
            data_type,
            color_interp: ColorInterpretation::Undefined,
            nodata: NoDataValue::None,
            sources: Vec::new(),
            block_size: None,
            pixel_function: None,
            offset: None,
            scale: None,
            color_table: None,
        }
    }

    /// Creates a simple band with a single source
    pub fn simple(band: usize, data_type: RasterDataType, source: VrtSource) -> Self {
        Self {
            band,
            data_type,
            color_interp: ColorInterpretation::Undefined,
            nodata: source.nodata.unwrap_or(NoDataValue::None),
            sources: vec![source],
            block_size: None,
            pixel_function: None,
            offset: None,
            scale: None,
            color_table: None,
        }
    }

    /// Adds a source to this band
    pub fn add_source(&mut self, source: VrtSource) {
        self.sources.push(source);
    }

    /// Sets the color interpretation
    pub fn with_color_interp(mut self, color_interp: ColorInterpretation) -> Self {
        self.color_interp = color_interp;
        self
    }

    /// Sets the NoData value
    pub fn with_nodata(mut self, nodata: NoDataValue) -> Self {
        self.nodata = nodata;
        self
    }

    /// Sets the block size
    pub fn with_block_size(mut self, width: u32, height: u32) -> Self {
        self.block_size = Some((width, height));
        self
    }

    /// Sets the pixel function
    pub fn with_pixel_function(mut self, function: PixelFunction) -> Self {
        self.pixel_function = Some(function);
        self
    }

    /// Sets the offset and scale
    pub fn with_scaling(mut self, offset: f64, scale: f64) -> Self {
        self.offset = Some(offset);
        self.scale = Some(scale);
        self
    }

    /// Sets the color table
    pub fn with_color_table(mut self, color_table: ColorTable) -> Self {
        self.color_table = Some(color_table);
        self
    }

    /// Validates the band configuration
    ///
    /// # Errors
    /// Returns an error if the band is invalid
    pub fn validate(&self) -> Result<()> {
        if self.band == 0 {
            return Err(VrtError::invalid_band("Band number must be >= 1"));
        }

        if self.sources.is_empty() && self.pixel_function.is_none() {
            return Err(VrtError::invalid_band(
                "Band must have at least one source or a pixel function",
            ));
        }

        // Validate all sources
        for source in &self.sources {
            source.validate()?;
        }

        // Validate pixel function if present
        if let Some(ref func) = self.pixel_function {
            func.validate(&self.sources)?;
        }

        Ok(())
    }

    /// Checks if this band has multiple sources
    pub fn has_multiple_sources(&self) -> bool {
        self.sources.len() > 1
    }

    /// Checks if this band uses a pixel function
    pub fn uses_pixel_function(&self) -> bool {
        self.pixel_function.is_some()
    }

    /// Applies scaling to a value
    pub fn apply_scaling(&self, value: f64) -> f64 {
        let scaled = if let Some(scale) = self.scale {
            value * scale
        } else {
            value
        };

        if let Some(offset) = self.offset {
            scaled + offset
        } else {
            scaled
        }
    }
}

/// Pixel function for on-the-fly computation
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PixelFunction {
    /// Average of all sources
    Average,
    /// Minimum of all sources
    Min,
    /// Maximum of all sources
    Max,
    /// Sum of all sources
    Sum,
    /// First valid (non-NoData) value
    FirstValid,
    /// Last valid (non-NoData) value
    LastValid,
    /// Weighted average (requires weights)
    WeightedAverage {
        /// Weights for each source
        weights: Vec<f64>,
    },
    /// NDVI: (NIR - Red) / (NIR + Red)
    /// Requires exactly 2 sources: [Red, NIR]
    Ndvi,
    /// Enhanced Vegetation Index: 2.5 * (NIR - Red) / (NIR + 6*Red - 7.5*Blue + 1)
    /// Requires exactly 3 sources: [Red, NIR, Blue]
    Evi,
    /// Normalized Difference Water Index: (Green - NIR) / (Green + NIR)
    /// Requires exactly 2 sources: [Green, NIR]
    Ndwi,
    /// Band math expression
    /// Supports operations: +, -, *, /, sqrt, pow, abs, min, max
    /// Variables are named as B1, B2, B3, etc. corresponding to source bands
    BandMath {
        /// Expression string (e.g., "(B1 + B2) / 2", "sqrt(B1 * B2)")
        expression: String,
    },
    /// Lookup table transformation
    /// Maps input values to output values
    LookupTable {
        /// Lookup table: vec of (input_value, output_value) pairs
        table: Vec<(f64, f64)>,
        /// Interpolation method: "nearest", "linear"
        interpolation: String,
    },
    /// Conditional logic: if condition then value_if_true else value_if_false
    /// Condition format: "B1 > 0.5", "B1 >= B2", etc.
    Conditional {
        /// Condition expression
        condition: String,
        /// Value if condition is true (can be a constant or expression)
        value_if_true: String,
        /// Value if condition is false (can be a constant or expression)
        value_if_false: String,
    },
    /// Multiply source values
    Multiply,
    /// Divide first source by second (handles division by zero)
    Divide,
    /// Square root of source value
    SquareRoot,
    /// Absolute value of source value
    Absolute,
    /// Custom function (not yet implemented)
    Custom {
        /// Function name
        name: String,
    },
}

impl PixelFunction {
    /// Validates the pixel function against sources
    ///
    /// # Errors
    /// Returns an error if the function is invalid for the given sources
    pub fn validate(&self, sources: &[VrtSource]) -> Result<()> {
        match self {
            Self::WeightedAverage { weights } => {
                if weights.len() != sources.len() {
                    return Err(VrtError::invalid_band(format!(
                        "WeightedAverage requires {} weights, got {}",
                        sources.len(),
                        weights.len()
                    )));
                }

                // Check that weights sum to approximately 1.0
                let sum: f64 = weights.iter().sum();
                if (sum - 1.0).abs() > 0.001 {
                    return Err(VrtError::invalid_band(format!(
                        "Weights should sum to 1.0, got {}",
                        sum
                    )));
                }
            }
            Self::Ndvi | Self::Ndwi => {
                if sources.len() != 2 {
                    return Err(VrtError::invalid_band(format!(
                        "{:?} requires exactly 2 sources, got {}",
                        self,
                        sources.len()
                    )));
                }
            }
            Self::Evi => {
                if sources.len() != 3 {
                    return Err(VrtError::invalid_band(format!(
                        "EVI requires exactly 3 sources, got {}",
                        sources.len()
                    )));
                }
            }
            Self::BandMath { expression } => {
                if expression.trim().is_empty() {
                    return Err(VrtError::invalid_band(
                        "BandMath expression cannot be empty",
                    ));
                }
            }
            Self::LookupTable { table, .. } => {
                if table.is_empty() {
                    return Err(VrtError::invalid_band("LookupTable cannot be empty"));
                }
            }
            Self::Conditional {
                condition,
                value_if_true,
                value_if_false,
            } => {
                if condition.trim().is_empty() {
                    return Err(VrtError::invalid_band(
                        "Conditional condition cannot be empty",
                    ));
                }
                if value_if_true.trim().is_empty() || value_if_false.trim().is_empty() {
                    return Err(VrtError::invalid_band("Conditional values cannot be empty"));
                }
            }
            Self::Divide | Self::Multiply => {
                if sources.len() < 2 {
                    return Err(VrtError::invalid_band(format!(
                        "{:?} requires at least 2 sources, got {}",
                        self,
                        sources.len()
                    )));
                }
            }
            Self::SquareRoot | Self::Absolute => {
                if sources.is_empty() {
                    return Err(VrtError::invalid_band(format!(
                        "{:?} requires at least 1 source",
                        self
                    )));
                }
            }
            Self::Custom { name } => {
                return Err(VrtError::InvalidPixelFunction {
                    function: name.clone(),
                });
            }
            _ => {}
        }
        Ok(())
    }

    /// Applies the pixel function to a set of values
    ///
    /// # Errors
    /// Returns an error if the function cannot be applied
    pub fn apply(&self, values: &[Option<f64>]) -> Result<Option<f64>> {
        match self {
            Self::Average => {
                let valid: Vec<f64> = values.iter().filter_map(|v| *v).collect();
                if valid.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(valid.iter().sum::<f64>() / valid.len() as f64))
                }
            }
            Self::Min => Ok(values
                .iter()
                .filter_map(|v| *v)
                .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))),
            Self::Max => Ok(values
                .iter()
                .filter_map(|v| *v)
                .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))),
            Self::Sum => {
                let valid: Vec<f64> = values.iter().filter_map(|v| *v).collect();
                if valid.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(valid.iter().sum()))
                }
            }
            Self::FirstValid => Ok(values.iter().find_map(|v| *v)),
            Self::LastValid => Ok(values.iter().rev().find_map(|v| *v)),
            Self::WeightedAverage { weights } => {
                if weights.len() != values.len() {
                    return Err(VrtError::invalid_band("Weight count mismatch"));
                }

                let mut sum = 0.0;
                let mut weight_sum = 0.0;

                for (value, weight) in values.iter().zip(weights.iter()) {
                    if let Some(v) = value {
                        sum += v * weight;
                        weight_sum += weight;
                    }
                }

                if weight_sum > 0.0 {
                    Ok(Some(sum / weight_sum))
                } else {
                    Ok(None)
                }
            }
            Self::Ndvi => {
                // NDVI = (NIR - Red) / (NIR + Red)
                if values.len() != 2 {
                    return Err(VrtError::invalid_band("NDVI requires exactly 2 values"));
                }
                match (values[0], values[1]) {
                    (Some(red), Some(nir)) => {
                        let denominator = nir + red;
                        if denominator.abs() < f64::EPSILON {
                            Ok(None) // Avoid division by zero
                        } else {
                            Ok(Some((nir - red) / denominator))
                        }
                    }
                    _ => Ok(None),
                }
            }
            Self::Evi => {
                // EVI = 2.5 * (NIR - Red) / (NIR + 6*Red - 7.5*Blue + 1)
                if values.len() != 3 {
                    return Err(VrtError::invalid_band("EVI requires exactly 3 values"));
                }
                match (values[0], values[1], values[2]) {
                    (Some(red), Some(nir), Some(blue)) => {
                        let denominator = nir + 6.0 * red - 7.5 * blue + 1.0;
                        if denominator.abs() < f64::EPSILON {
                            Ok(None)
                        } else {
                            Ok(Some(2.5 * (nir - red) / denominator))
                        }
                    }
                    _ => Ok(None),
                }
            }
            Self::Ndwi => {
                // NDWI = (Green - NIR) / (Green + NIR)
                if values.len() != 2 {
                    return Err(VrtError::invalid_band("NDWI requires exactly 2 values"));
                }
                match (values[0], values[1]) {
                    (Some(green), Some(nir)) => {
                        let denominator = green + nir;
                        if denominator.abs() < f64::EPSILON {
                            Ok(None)
                        } else {
                            Ok(Some((green - nir) / denominator))
                        }
                    }
                    _ => Ok(None),
                }
            }
            Self::BandMath { expression } => Self::evaluate_expression(expression, values),
            Self::LookupTable {
                table,
                interpolation,
            } => {
                if values.is_empty() {
                    return Ok(None);
                }
                if let Some(value) = values[0] {
                    Self::apply_lookup_table(value, table, interpolation)
                } else {
                    Ok(None)
                }
            }
            Self::Conditional {
                condition,
                value_if_true,
                value_if_false,
            } => Self::evaluate_conditional(condition, value_if_true, value_if_false, values),
            Self::Multiply => {
                let valid: Vec<f64> = values.iter().filter_map(|v| *v).collect();
                if valid.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(valid.iter().product()))
                }
            }
            Self::Divide => {
                if values.len() < 2 {
                    return Err(VrtError::invalid_band("Divide requires at least 2 values"));
                }
                match (values[0], values[1]) {
                    (Some(numerator), Some(denominator)) => {
                        if denominator.abs() < f64::EPSILON {
                            Ok(None) // Avoid division by zero
                        } else {
                            Ok(Some(numerator / denominator))
                        }
                    }
                    _ => Ok(None),
                }
            }
            Self::SquareRoot => {
                if values.is_empty() {
                    return Ok(None);
                }
                values[0].map_or(Ok(None), |v| {
                    if v < 0.0 {
                        Ok(None) // Negative values have no real square root
                    } else {
                        Ok(Some(v.sqrt()))
                    }
                })
            }
            Self::Absolute => {
                if values.is_empty() {
                    return Ok(None);
                }
                Ok(values[0].map(|v| v.abs()))
            }
            Self::Custom { name } => Err(VrtError::InvalidPixelFunction {
                function: name.clone(),
            }),
        }
    }

    /// Evaluates a band math expression
    fn evaluate_expression(expression: &str, values: &[Option<f64>]) -> Result<Option<f64>> {
        // Simple expression evaluator for basic band math
        // Supports: +, -, *, /, sqrt, pow, abs, min, max
        // Variables: B1, B2, B3, etc.

        let mut expr = expression.to_string();

        // Replace band variables with actual values
        for (i, value) in values.iter().enumerate() {
            let var = format!("B{}", i + 1);
            if let Some(v) = value {
                expr = expr.replace(&var, &v.to_string());
            } else {
                return Ok(None); // If any band is NoData, result is NoData
            }
        }

        // Basic expression evaluation (simplified)
        // For production, consider using a proper expression parser like `evalexpr`
        match Self::simple_eval(&expr) {
            Ok(result) => Ok(Some(result)),
            Err(_) => Err(VrtError::invalid_band(format!(
                "Failed to evaluate expression: {}",
                expression
            ))),
        }
    }

    /// Simple expression evaluator (basic implementation)
    fn simple_eval(expr: &str) -> Result<f64> {
        let expr = expr.trim();

        // Try to parse as number first
        if let Ok(num) = expr.parse::<f64>() {
            return Ok(num);
        }

        // Handle sqrt
        if expr.starts_with("sqrt(") && expr.ends_with(')') {
            let inner = &expr[5..expr.len() - 1];
            let val = Self::simple_eval(inner)?;
            if val < 0.0 {
                return Err(VrtError::invalid_band("Square root of negative number"));
            }
            return Ok(val.sqrt());
        }

        // Handle abs
        if expr.starts_with("abs(") && expr.ends_with(')') {
            let inner = &expr[4..expr.len() - 1];
            let val = Self::simple_eval(inner)?;
            return Ok(val.abs());
        }

        // Handle parentheses
        if expr.starts_with('(') && expr.ends_with(')') {
            // Check if these are balanced outer parens
            let inner = &expr[1..expr.len() - 1];
            let mut depth = 0;
            let mut is_outer = true;
            for ch in inner.chars() {
                if ch == '(' {
                    depth += 1;
                } else if ch == ')' {
                    depth -= 1;
                    if depth < 0 {
                        is_outer = false;
                        break;
                    }
                }
            }
            if is_outer && depth == 0 {
                return Self::simple_eval(inner);
            }
        }

        // Handle binary operations (search from right to left for left-to-right evaluation)
        // Process + and - first (lower precedence), then * and / (higher precedence)
        for op in &['+', '-'] {
            let mut depth = 0;
            for (i, ch) in expr.char_indices().rev() {
                if ch == ')' {
                    depth += 1;
                } else if ch == '(' {
                    depth -= 1;
                } else if depth == 0 && ch == *op && i > 0 && i < expr.len() - 1 {
                    let left = Self::simple_eval(&expr[..i])?;
                    let right = Self::simple_eval(&expr[i + 1..])?;
                    return match op {
                        '+' => Ok(left + right),
                        '-' => Ok(left - right),
                        _ => unreachable!(),
                    };
                }
            }
        }

        for op in &['*', '/'] {
            let mut depth = 0;
            for (i, ch) in expr.char_indices().rev() {
                if ch == ')' {
                    depth += 1;
                } else if ch == '(' {
                    depth -= 1;
                } else if depth == 0 && ch == *op && i > 0 && i < expr.len() - 1 {
                    let left = Self::simple_eval(&expr[..i])?;
                    let right = Self::simple_eval(&expr[i + 1..])?;
                    return match op {
                        '*' => Ok(left * right),
                        '/' => {
                            if right.abs() < f64::EPSILON {
                                Err(VrtError::invalid_band("Division by zero"))
                            } else {
                                Ok(left / right)
                            }
                        }
                        _ => unreachable!(),
                    };
                }
            }
        }

        Err(VrtError::invalid_band(format!(
            "Cannot parse expression: {}",
            expr
        )))
    }

    /// Applies lookup table transformation
    fn apply_lookup_table(
        value: f64,
        table: &[(f64, f64)],
        interpolation: &str,
    ) -> Result<Option<f64>> {
        if table.is_empty() {
            return Ok(None);
        }

        match interpolation {
            "nearest" => {
                // Find nearest entry
                let mut best_idx = 0;
                let mut best_dist = (table[0].0 - value).abs();

                for (i, (input, _)) in table.iter().enumerate() {
                    let dist = (input - value).abs();
                    if dist < best_dist {
                        best_dist = dist;
                        best_idx = i;
                    }
                }

                Ok(Some(table[best_idx].1))
            }
            "linear" => {
                // Linear interpolation
                if value <= table[0].0 {
                    return Ok(Some(table[0].1));
                }
                if value >= table[table.len() - 1].0 {
                    return Ok(Some(table[table.len() - 1].1));
                }

                // Find surrounding points
                for i in 0..table.len() - 1 {
                    if value >= table[i].0 && value <= table[i + 1].0 {
                        let x0 = table[i].0;
                        let y0 = table[i].1;
                        let x1 = table[i + 1].0;
                        let y1 = table[i + 1].1;

                        let t = (value - x0) / (x1 - x0);
                        return Ok(Some(y0 + t * (y1 - y0)));
                    }
                }

                Ok(Some(table[0].1))
            }
            _ => Err(VrtError::invalid_band(format!(
                "Unknown interpolation method: {}",
                interpolation
            ))),
        }
    }

    /// Evaluates conditional expression
    fn evaluate_conditional(
        condition: &str,
        value_if_true: &str,
        value_if_false: &str,
        values: &[Option<f64>],
    ) -> Result<Option<f64>> {
        // Simple condition evaluator
        // Supports: >, <, >=, <=, ==, !=

        let cond_result = Self::evaluate_condition(condition, values)?;

        let target_expr = if cond_result {
            value_if_true
        } else {
            value_if_false
        };

        // Evaluate the target expression
        Self::evaluate_expression(target_expr, values)
    }

    /// Evaluates a boolean condition
    fn evaluate_condition(condition: &str, values: &[Option<f64>]) -> Result<bool> {
        let condition = condition.trim();

        // Try different comparison operators in order (longer ones first to avoid false matches)
        let operators = [">=", "<=", "==", "!=", ">", "<"];

        for op_str in &operators {
            if let Some(pos) = condition.find(op_str) {
                let left_expr = condition[..pos].trim();
                let right_expr = condition[pos + op_str.len()..].trim();

                let left_val = Self::evaluate_expression(left_expr, values)?
                    .ok_or_else(|| VrtError::invalid_band("Left side of condition is NoData"))?;

                let right_val = Self::evaluate_expression(right_expr, values)?
                    .ok_or_else(|| VrtError::invalid_band("Right side of condition is NoData"))?;

                let result = match *op_str {
                    ">=" => left_val >= right_val,
                    "<=" => left_val <= right_val,
                    ">" => left_val > right_val,
                    "<" => left_val < right_val,
                    "==" => (left_val - right_val).abs() < f64::EPSILON,
                    "!=" => (left_val - right_val).abs() >= f64::EPSILON,
                    _ => {
                        return Err(VrtError::invalid_band(format!(
                            "Unknown operator: {}",
                            op_str
                        )));
                    }
                };

                return Ok(result);
            }
        }

        Err(VrtError::invalid_band(format!(
            "Cannot parse condition: {}",
            condition
        )))
    }
}

/// Color table entry
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorEntry {
    /// Color value (index)
    pub value: u16,
    /// Red component (0-255)
    pub r: u8,
    /// Green component (0-255)
    pub g: u8,
    /// Blue component (0-255)
    pub b: u8,
    /// Alpha component (0-255)
    pub a: u8,
}

impl ColorEntry {
    /// Creates a new color entry
    pub const fn new(value: u16, r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { value, r, g, b, a }
    }

    /// Creates an opaque color entry
    pub const fn rgb(value: u16, r: u8, g: u8, b: u8) -> Self {
        Self::new(value, r, g, b, 255)
    }
}

/// Color table (palette)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorTable {
    /// Color entries
    pub entries: Vec<ColorEntry>,
}

impl ColorTable {
    /// Creates a new empty color table
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Creates a color table with entries
    pub fn with_entries(entries: Vec<ColorEntry>) -> Self {
        Self { entries }
    }

    /// Adds a color entry
    pub fn add_entry(&mut self, entry: ColorEntry) {
        self.entries.push(entry);
    }

    /// Gets a color entry by value
    pub fn get(&self, value: u16) -> Option<&ColorEntry> {
        self.entries.iter().find(|e| e.value == value)
    }
}

impl Default for ColorTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SourceFilename;

    #[test]
    fn test_vrt_band_creation() {
        let band = VrtBand::new(1, RasterDataType::UInt8);
        assert_eq!(band.band, 1);
        assert_eq!(band.data_type, RasterDataType::UInt8);
    }

    #[test]
    fn test_vrt_band_validation() {
        let source = VrtSource::new(SourceFilename::absolute("/test.tif"), 1);
        let band = VrtBand::simple(1, RasterDataType::UInt8, source);
        assert!(band.validate().is_ok());

        let invalid_band = VrtBand::new(0, RasterDataType::UInt8);
        assert!(invalid_band.validate().is_err());

        let no_source_band = VrtBand::new(1, RasterDataType::UInt8);
        assert!(no_source_band.validate().is_err());
    }

    #[test]
    fn test_pixel_function_average() {
        let func = PixelFunction::Average;
        let values = vec![Some(1.0), Some(2.0), Some(3.0)];
        let result = func.apply(&values);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), Some(2.0));

        let with_none = vec![Some(1.0), None, Some(3.0)];
        let result = func.apply(&with_none);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), Some(2.0));
    }

    #[test]
    fn test_pixel_function_first_valid() {
        let func = PixelFunction::FirstValid;
        let values = vec![None, Some(2.0), Some(3.0)];
        let result = func.apply(&values);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), Some(2.0));
    }

    #[test]
    fn test_pixel_function_weighted_average() {
        let func = PixelFunction::WeightedAverage {
            weights: vec![0.5, 0.3, 0.2],
        };
        let values = vec![Some(10.0), Some(20.0), Some(30.0)];
        let result = func.apply(&values);
        assert!(result.is_ok());
        // 10*0.5 + 20*0.3 + 30*0.2 = 5 + 6 + 6 = 17
        assert_eq!(result.ok().flatten(), Some(17.0));
    }

    #[test]
    fn test_band_scaling() {
        let band = VrtBand::new(1, RasterDataType::UInt8).with_scaling(10.0, 2.0);

        assert_eq!(band.apply_scaling(5.0), 20.0); // 5 * 2 + 10 = 20
    }

    #[test]
    fn test_color_table() {
        let mut table = ColorTable::new();
        table.add_entry(ColorEntry::rgb(0, 255, 0, 0));
        table.add_entry(ColorEntry::rgb(1, 0, 255, 0));
        table.add_entry(ColorEntry::rgb(2, 0, 0, 255));

        assert_eq!(table.entries.len(), 3);
        assert_eq!(table.get(1).map(|e| e.g), Some(255));
    }

    #[test]
    fn test_pixel_function_ndvi() {
        let func = PixelFunction::Ndvi;

        // Standard NDVI calculation
        let values = vec![Some(0.1), Some(0.5)]; // Red, NIR
        let result = func.apply(&values);
        assert!(result.is_ok());
        // NDVI = (0.5 - 0.1) / (0.5 + 0.1) = 0.4 / 0.6 = 0.666...
        assert!((result.ok().flatten().expect("Should have value") - 0.666666).abs() < 0.001);

        // With NoData
        let values_nodata = vec![Some(0.1), None];
        let result = func.apply(&values_nodata);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), None);

        // Zero sum (edge case)
        let values_zero = vec![Some(0.5), Some(-0.5)];
        let result = func.apply(&values_zero);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), None); // Should return None to avoid division by zero
    }

    #[test]
    fn test_pixel_function_evi() {
        let func = PixelFunction::Evi;

        // Standard EVI calculation
        let values = vec![Some(0.1), Some(0.5), Some(0.05)]; // Red, NIR, Blue
        let result = func.apply(&values);
        assert!(result.is_ok());
        // EVI = 2.5 * (0.5 - 0.1) / (0.5 + 6*0.1 - 7.5*0.05 + 1)
        // = 2.5 * 0.4 / (0.5 + 0.6 - 0.375 + 1)
        // = 1.0 / 1.725 = 0.5797...
        let expected = 1.0 / 1.725;
        assert!((result.ok().flatten().expect("Should have value") - expected).abs() < 0.001);
    }

    #[test]
    fn test_pixel_function_ndwi() {
        let func = PixelFunction::Ndwi;

        // Standard NDWI calculation
        let values = vec![Some(0.3), Some(0.2)]; // Green, NIR
        let result = func.apply(&values);
        assert!(result.is_ok());
        // NDWI = (0.3 - 0.2) / (0.3 + 0.2) = 0.1 / 0.5 = 0.2
        assert!((result.ok().flatten().expect("Should have value") - 0.2).abs() < 0.001);
    }

    #[test]
    fn test_pixel_function_band_math() {
        let func = PixelFunction::BandMath {
            expression: "(B1 + B2) / 2".to_string(),
        };

        let values = vec![Some(10.0), Some(20.0)];
        let result = func.apply(&values);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), Some(15.0));

        // Test with sqrt
        let func_sqrt = PixelFunction::BandMath {
            expression: "sqrt(B1)".to_string(),
        };
        let values_sqrt = vec![Some(16.0)];
        let result = func_sqrt.apply(&values_sqrt);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), Some(4.0));

        // Test with abs
        let func_abs = PixelFunction::BandMath {
            expression: "abs(B1)".to_string(),
        };
        let values_abs = vec![Some(-5.0)];
        let result = func_abs.apply(&values_abs);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), Some(5.0));
    }

    #[test]
    fn test_pixel_function_lookup_table_nearest() {
        let func = PixelFunction::LookupTable {
            table: vec![(0.0, 10.0), (0.5, 20.0), (1.0, 30.0)],
            interpolation: "nearest".to_string(),
        };

        // Exact match
        let values = vec![Some(0.5)];
        let result = func.apply(&values);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), Some(20.0));

        // Nearest neighbor
        let values = vec![Some(0.7)];
        let result = func.apply(&values);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), Some(20.0)); // Closest to 0.5
    }

    #[test]
    fn test_pixel_function_lookup_table_linear() {
        let func = PixelFunction::LookupTable {
            table: vec![(0.0, 10.0), (1.0, 30.0)],
            interpolation: "linear".to_string(),
        };

        // Interpolated value
        let values = vec![Some(0.5)];
        let result = func.apply(&values);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), Some(20.0)); // Linear interpolation: 10 + 0.5 * (30-10)

        // Edge case: below range
        let values = vec![Some(-1.0)];
        let result = func.apply(&values);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), Some(10.0));

        // Edge case: above range
        let values = vec![Some(2.0)];
        let result = func.apply(&values);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), Some(30.0));
    }

    #[test]
    fn test_pixel_function_conditional() {
        let func = PixelFunction::Conditional {
            condition: "B1 > 0.5".to_string(),
            value_if_true: "B1 * 2".to_string(),
            value_if_false: "B1 / 2".to_string(),
        };

        // True case
        let values_true = vec![Some(0.8)];
        let result = func.apply(&values_true);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), Some(1.6));

        // False case
        let values_false = vec![Some(0.3)];
        let result = func.apply(&values_false);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), Some(0.15));
    }

    #[test]
    fn test_pixel_function_multiply() {
        let func = PixelFunction::Multiply;

        let values = vec![Some(2.0), Some(3.0), Some(4.0)];
        let result = func.apply(&values);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), Some(24.0));

        // With NoData
        let values_nodata = vec![Some(2.0), None, Some(4.0)];
        let result = func.apply(&values_nodata);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), Some(8.0)); // Only multiplies valid values
    }

    #[test]
    fn test_pixel_function_divide() {
        let func = PixelFunction::Divide;

        let values = vec![Some(10.0), Some(2.0)];
        let result = func.apply(&values);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), Some(5.0));

        // Division by zero
        let values_zero = vec![Some(10.0), Some(0.0)];
        let result = func.apply(&values_zero);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), None);
    }

    #[test]
    fn test_pixel_function_square_root() {
        let func = PixelFunction::SquareRoot;

        let values = vec![Some(25.0)];
        let result = func.apply(&values);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), Some(5.0));

        // Negative value
        let values_neg = vec![Some(-4.0)];
        let result = func.apply(&values_neg);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), None);
    }

    #[test]
    fn test_pixel_function_absolute() {
        let func = PixelFunction::Absolute;

        let values_pos = vec![Some(5.0)];
        let result = func.apply(&values_pos);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), Some(5.0));

        let values_neg = vec![Some(-5.0)];
        let result = func.apply(&values_neg);
        assert!(result.is_ok());
        assert_eq!(result.ok().flatten(), Some(5.0));
    }

    #[test]
    fn test_pixel_function_validation() {
        // NDVI validation
        let ndvi_func = PixelFunction::Ndvi;
        let source = VrtSource::new(SourceFilename::absolute("/test.tif"), 1);
        let sources_valid = vec![source.clone(), source.clone()];
        assert!(ndvi_func.validate(&sources_valid).is_ok());

        let sources_invalid = vec![source.clone()];
        assert!(ndvi_func.validate(&sources_invalid).is_err());

        // BandMath validation
        let math_func = PixelFunction::BandMath {
            expression: "B1 + B2".to_string(),
        };
        assert!(math_func.validate(&sources_valid).is_ok());

        let empty_expr = PixelFunction::BandMath {
            expression: "".to_string(),
        };
        assert!(empty_expr.validate(&sources_valid).is_err());

        // LookupTable validation
        let lut_func = PixelFunction::LookupTable {
            table: vec![(0.0, 10.0)],
            interpolation: "linear".to_string(),
        };
        assert!(lut_func.validate(&sources_valid).is_ok());

        let empty_lut = PixelFunction::LookupTable {
            table: vec![],
            interpolation: "linear".to_string(),
        };
        assert!(empty_lut.validate(&sources_valid).is_err());
    }
}
