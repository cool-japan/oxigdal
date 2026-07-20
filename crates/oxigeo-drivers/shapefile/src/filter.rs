//! Attribute filtering for Shapefile features.
//!
//! This module provides:
//! - [`FieldFilter`] – a structured filter that matches features whose named
//!   attribute satisfies a comparison against a [`FilterValue`].
//! - [`FieldFilterOp`] – the comparison operators supported.
//! - [`FilterValue`] – the right-hand side of a comparison (String, Integer,
//!   Float, or Bool).
//!
//! All types are cheaply cloneable and `Send + Sync`, so they can be used in
//! parallel read scenarios.

use crate::reader::ShapefileFeature;
use oxigeo_core::vector::FieldValue;

// ── Comparison operators ───────────────────────────────────────────────────────

/// Operator for comparing a feature attribute to a [`FilterValue`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldFilterOp {
    /// Equal to.
    Eq,
    /// Not equal to.
    Ne,
    /// Greater than (numeric comparison).
    Gt,
    /// Less than (numeric comparison).
    Lt,
    /// Greater than or equal to (numeric comparison).
    Gte,
    /// Less than or equal to (numeric comparison).
    Lte,
    /// String contains the value as a substring (string types only).
    Contains,
    /// String starts with the value as a prefix (string types only).
    StartsWith,
}

// ── Filter value ──────────────────────────────────────────────────────────────

/// The right-hand side of an attribute filter comparison.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterValue {
    /// A UTF-8 string value.
    String(String),
    /// A 64-bit signed integer value.
    Integer(i64),
    /// A 64-bit floating-point value.
    Float(f64),
    /// A boolean value.
    Bool(bool),
}

impl FilterValue {
    /// Converts the filter value to `f64` for numeric comparisons.
    ///
    /// Returns `None` if the value is not numeric (i.e. String or Bool).
    fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Integer(i) => Some(*i as f64),
            Self::Float(f) => Some(*f),
            Self::String(_) | Self::Bool(_) => None,
        }
    }
}

// ── Field filter ──────────────────────────────────────────────────────────────

/// Structured filter that tests a single named attribute of a
/// [`ShapefileFeature`] against a [`FilterValue`] using a [`FieldFilterOp`].
///
/// # Example
///
/// ```rust,no_run
/// use oxigeo_shapefile::filter::{FieldFilter, FieldFilterOp, FilterValue};
///
/// // Match features where "NAME" == "Paris"
/// let filter = FieldFilter {
///     field: "NAME".to_string(),
///     op: FieldFilterOp::Eq,
///     value: FilterValue::String("Paris".to_string()),
/// };
/// ```
#[derive(Debug, Clone)]
pub struct FieldFilter {
    /// The attribute field name to test.
    pub field: String,
    /// The comparison operator.
    pub op: FieldFilterOp,
    /// The right-hand side value.
    pub value: FilterValue,
}

impl FieldFilter {
    /// Returns `true` if `feature` satisfies this filter.
    ///
    /// Rules:
    /// - If the field is absent from the feature's attributes, returns `false`.
    /// - `Eq`/`Ne`: value-level equality for all types.  Float equality is
    ///   strict (`==` on `f64`); callers should avoid using `Eq`/`Ne` with
    ///   floating-point values.
    /// - `Gt`/`Lt`/`Gte`/`Lte`: numeric comparison; both the attribute and the
    ///   filter value are cast to `f64`.  Non-numeric types on either side
    ///   return `false`.
    /// - `Contains`/`StartsWith`: substring / prefix match on string types
    ///   only; any other type returns `false`.
    pub fn matches(&self, feature: &ShapefileFeature) -> bool {
        let Some(attr) = feature.attributes.get(&self.field) else {
            return false;
        };

        match self.op {
            FieldFilterOp::Eq => self.eq_match(attr),
            FieldFilterOp::Ne => !self.eq_match(attr),
            FieldFilterOp::Gt => self.numeric_cmp(attr).is_some_and(|o| o > 0),
            FieldFilterOp::Lt => self.numeric_cmp(attr).is_some_and(|o| o < 0),
            FieldFilterOp::Gte => self.numeric_cmp(attr).is_some_and(|o| o >= 0),
            FieldFilterOp::Lte => self.numeric_cmp(attr).is_some_and(|o| o <= 0),
            FieldFilterOp::Contains => self.string_contains(attr),
            FieldFilterOp::StartsWith => self.string_starts_with(attr),
        }
    }

    // ── Helpers ────────────────────────────────────────────────────────────────

    /// Value-equality check between `attr` and `self.value`.
    fn eq_match(&self, attr: &FieldValue) -> bool {
        match (attr, &self.value) {
            (FieldValue::String(a), FilterValue::String(v)) => a == v,
            (FieldValue::Integer(a), FilterValue::Integer(v)) => a == v,
            (FieldValue::Float(a), FilterValue::Float(v)) => a == v,
            (FieldValue::Bool(a), FilterValue::Bool(v)) => a == v,
            // Allow cross-type numeric comparisons (Integer attribute vs Float filter).
            (FieldValue::Integer(a), FilterValue::Float(v)) => (*a as f64) == *v,
            (FieldValue::Float(a), FilterValue::Integer(v)) => *a == (*v as f64),
            _ => false,
        }
    }

    /// Returns a numeric comparison result (`-1`, `0`, `1`) between the
    /// attribute and the filter value, or `None` if either side is not numeric.
    ///
    /// Negative means `attr < value`, zero means equal, positive means `attr > value`.
    fn numeric_cmp(&self, attr: &FieldValue) -> Option<i8> {
        let lhs: f64 = match attr {
            FieldValue::Integer(i) => *i as f64,
            FieldValue::Float(f) => *f,
            _ => return None,
        };
        let rhs = self.value.as_f64()?;
        if lhs < rhs {
            Some(-1)
        } else if lhs > rhs {
            Some(1)
        } else {
            Some(0)
        }
    }

    /// Returns `true` if the string attribute contains the filter value as a
    /// substring.  Non-string attribute types always return `false`.
    fn string_contains(&self, attr: &FieldValue) -> bool {
        let FilterValue::String(needle) = &self.value else {
            return false;
        };
        match attr {
            FieldValue::String(haystack) => haystack.contains(needle.as_str()),
            _ => false,
        }
    }

    /// Returns `true` if the string attribute starts with the filter value as a
    /// prefix.  Non-string attribute types always return `false`.
    fn string_starts_with(&self, attr: &FieldValue) -> bool {
        let FilterValue::String(prefix) = &self.value else {
            return false;
        };
        match attr {
            FieldValue::String(haystack) => haystack.starts_with(prefix.as_str()),
            _ => false,
        }
    }
}

// ── Unit tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::ShapefileFeature;
    use std::collections::HashMap;

    fn make_feature(attrs: Vec<(&str, FieldValue)>) -> ShapefileFeature {
        let mut attributes = HashMap::new();
        for (k, v) in attrs {
            attributes.insert(k.to_string(), v);
        }
        ShapefileFeature::new(1, None, attributes)
    }

    #[test]
    fn test_string_eq() {
        let filter = FieldFilter {
            field: "NAME".to_string(),
            op: FieldFilterOp::Eq,
            value: FilterValue::String("Paris".to_string()),
        };
        let yes = make_feature(vec![("NAME", FieldValue::String("Paris".to_string()))]);
        let no = make_feature(vec![("NAME", FieldValue::String("London".to_string()))]);
        assert!(filter.matches(&yes));
        assert!(!filter.matches(&no));
    }

    #[test]
    fn test_integer_ne() {
        let filter = FieldFilter {
            field: "VAL".to_string(),
            op: FieldFilterOp::Ne,
            value: FilterValue::Integer(0),
        };
        let yes = make_feature(vec![("VAL", FieldValue::Integer(5))]);
        let no = make_feature(vec![("VAL", FieldValue::Integer(0))]);
        assert!(filter.matches(&yes));
        assert!(!filter.matches(&no));
    }

    #[test]
    fn test_float_gt() {
        let filter = FieldFilter {
            field: "SCORE".to_string(),
            op: FieldFilterOp::Gt,
            value: FilterValue::Float(5.0),
        };
        let yes = make_feature(vec![("SCORE", FieldValue::Float(6.0))]);
        let no = make_feature(vec![("SCORE", FieldValue::Float(4.0))]);
        let equal = make_feature(vec![("SCORE", FieldValue::Float(5.0))]);
        assert!(filter.matches(&yes));
        assert!(!filter.matches(&no));
        assert!(!filter.matches(&equal));
    }

    #[test]
    fn test_contains() {
        let filter = FieldFilter {
            field: "NAME".to_string(),
            op: FieldFilterOp::Contains,
            value: FilterValue::String("oint".to_string()),
        };
        let yes = make_feature(vec![("NAME", FieldValue::String("Point A".to_string()))]);
        let no = make_feature(vec![("NAME", FieldValue::String("Region B".to_string()))]);
        assert!(filter.matches(&yes));
        assert!(!filter.matches(&no));
    }

    #[test]
    fn test_missing_field_returns_false() {
        let filter = FieldFilter {
            field: "NONEXISTENT".to_string(),
            op: FieldFilterOp::Eq,
            value: FilterValue::String("x".to_string()),
        };
        let feature = make_feature(vec![("NAME", FieldValue::String("y".to_string()))]);
        assert!(!filter.matches(&feature));
    }
}
