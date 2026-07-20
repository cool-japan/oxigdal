//! JoinValue type and operations

/// A value that can be compared in join conditions.
#[derive(Debug, Clone, PartialEq)]
pub enum JoinValue {
    /// Null value.
    Null,
    /// Boolean value.
    Boolean(bool),
    /// Integer value (stored as i64 for uniformity).
    Integer(i64),
    /// Float value (stored as f64 for uniformity).
    Float(f64),
    /// String value.
    String(String),
}

impl JoinValue {
    /// Check if this value is null.
    pub fn is_null(&self) -> bool {
        matches!(self, JoinValue::Null)
    }

    /// Convert to hashable key for hash join.
    pub fn to_hash_key(&self) -> String {
        match self {
            JoinValue::Null => "__NULL__".to_string(),
            JoinValue::Boolean(b) => format!("b:{}", b),
            JoinValue::Integer(i) => format!("i:{}", i),
            JoinValue::Float(f) => format!("f:{:?}", f),
            JoinValue::String(s) => format!("s:{}", s),
        }
    }

    /// Compare with another value for equality.
    pub fn equals(&self, other: &JoinValue) -> Option<bool> {
        if self.is_null() || other.is_null() {
            return None; // NULL = NULL is undefined
        }

        match (self, other) {
            (JoinValue::Boolean(a), JoinValue::Boolean(b)) => Some(a == b),
            (JoinValue::Integer(a), JoinValue::Integer(b)) => Some(a == b),
            (JoinValue::Integer(a), JoinValue::Float(b)) => Some((*a as f64) == *b),
            (JoinValue::Float(a), JoinValue::Integer(b)) => Some(*a == (*b as f64)),
            (JoinValue::Float(a), JoinValue::Float(b)) => Some(a == b),
            (JoinValue::String(a), JoinValue::String(b)) => Some(a == b),
            _ => Some(false), // Different types are not equal
        }
    }

    /// Compare with another value.
    pub fn compare(&self, other: &JoinValue) -> Option<std::cmp::Ordering> {
        if self.is_null() || other.is_null() {
            return None;
        }

        match (self, other) {
            (JoinValue::Boolean(a), JoinValue::Boolean(b)) => Some(a.cmp(b)),
            (JoinValue::Integer(a), JoinValue::Integer(b)) => Some(a.cmp(b)),
            (JoinValue::Integer(a), JoinValue::Float(b)) => (*a as f64).partial_cmp(b),
            (JoinValue::Float(a), JoinValue::Integer(b)) => a.partial_cmp(&(*b as f64)),
            (JoinValue::Float(a), JoinValue::Float(b)) => a.partial_cmp(b),
            (JoinValue::String(a), JoinValue::String(b)) => Some(a.cmp(b)),
            _ => None, // Cannot compare different types
        }
    }

    /// Negate for arithmetic operations.
    pub fn negate(&self) -> Option<JoinValue> {
        match self {
            JoinValue::Integer(i) => Some(JoinValue::Integer(-i)),
            JoinValue::Float(f) => Some(JoinValue::Float(-f)),
            _ => None,
        }
    }

    /// Logical NOT.
    pub fn not(&self) -> Option<JoinValue> {
        match self {
            JoinValue::Boolean(b) => Some(JoinValue::Boolean(!b)),
            _ => None,
        }
    }

    /// Add two values.
    pub fn add(&self, other: &JoinValue) -> Option<JoinValue> {
        match (self, other) {
            // Checked arithmetic: overflow yields None (surfaced as an execution
            // error by the caller) instead of panicking or silently wrapping.
            (JoinValue::Integer(a), JoinValue::Integer(b)) => {
                a.checked_add(*b).map(JoinValue::Integer)
            }
            (JoinValue::Integer(a), JoinValue::Float(b)) => Some(JoinValue::Float(*a as f64 + b)),
            (JoinValue::Float(a), JoinValue::Integer(b)) => Some(JoinValue::Float(a + *b as f64)),
            (JoinValue::Float(a), JoinValue::Float(b)) => Some(JoinValue::Float(a + b)),
            (JoinValue::String(a), JoinValue::String(b)) => {
                Some(JoinValue::String(format!("{}{}", a, b)))
            }
            _ => None,
        }
    }

    /// Subtract two values.
    pub fn subtract(&self, other: &JoinValue) -> Option<JoinValue> {
        match (self, other) {
            (JoinValue::Integer(a), JoinValue::Integer(b)) => {
                a.checked_sub(*b).map(JoinValue::Integer)
            }
            (JoinValue::Integer(a), JoinValue::Float(b)) => Some(JoinValue::Float(*a as f64 - b)),
            (JoinValue::Float(a), JoinValue::Integer(b)) => Some(JoinValue::Float(a - *b as f64)),
            (JoinValue::Float(a), JoinValue::Float(b)) => Some(JoinValue::Float(a - b)),
            _ => None,
        }
    }

    /// Multiply two values.
    pub fn multiply(&self, other: &JoinValue) -> Option<JoinValue> {
        match (self, other) {
            (JoinValue::Integer(a), JoinValue::Integer(b)) => {
                a.checked_mul(*b).map(JoinValue::Integer)
            }
            (JoinValue::Integer(a), JoinValue::Float(b)) => Some(JoinValue::Float(*a as f64 * b)),
            (JoinValue::Float(a), JoinValue::Integer(b)) => Some(JoinValue::Float(a * *b as f64)),
            (JoinValue::Float(a), JoinValue::Float(b)) => Some(JoinValue::Float(a * b)),
            _ => None,
        }
    }

    /// Divide two values.
    pub fn divide(&self, other: &JoinValue) -> Option<JoinValue> {
        match (self, other) {
            (JoinValue::Integer(a), JoinValue::Integer(b)) if *b != 0 => {
                Some(JoinValue::Integer(a / b))
            }
            (JoinValue::Integer(a), JoinValue::Float(b)) if *b != 0.0 => {
                Some(JoinValue::Float(*a as f64 / b))
            }
            (JoinValue::Float(a), JoinValue::Integer(b)) if *b != 0 => {
                Some(JoinValue::Float(a / *b as f64))
            }
            (JoinValue::Float(a), JoinValue::Float(b)) if *b != 0.0 => {
                Some(JoinValue::Float(a / b))
            }
            _ => None,
        }
    }

    /// Modulo two values.
    pub fn modulo(&self, other: &JoinValue) -> Option<JoinValue> {
        match (self, other) {
            (JoinValue::Integer(a), JoinValue::Integer(b)) if *b != 0 => {
                Some(JoinValue::Integer(a % b))
            }
            (JoinValue::Integer(a), JoinValue::Float(b)) if *b != 0.0 => {
                Some(JoinValue::Float(*a as f64 % b))
            }
            (JoinValue::Float(a), JoinValue::Integer(b)) if *b != 0 => {
                Some(JoinValue::Float(a % *b as f64))
            }
            (JoinValue::Float(a), JoinValue::Float(b)) if *b != 0.0 => {
                Some(JoinValue::Float(a % b))
            }
            _ => None,
        }
    }

    /// Convert to boolean for logical operations.
    pub fn to_bool(&self) -> Option<bool> {
        match self {
            JoinValue::Boolean(b) => Some(*b),
            JoinValue::Null => None,
            _ => None,
        }
    }

    /// Check if string matches a case-sensitive `LIKE` pattern.
    pub fn matches_like(&self, pattern: &JoinValue) -> Option<bool> {
        match (self, pattern) {
            (JoinValue::String(s), JoinValue::String(p)) => Some(Self::like_match(s, p)),
            _ => None,
        }
    }

    /// Check if string matches a case-insensitive `ILIKE` pattern.
    pub fn matches_ilike(&self, pattern: &JoinValue) -> Option<bool> {
        match (self, pattern) {
            (JoinValue::String(s), JoinValue::String(p)) => {
                Some(crate::executor::like::like_match(s, p, true))
            }
            _ => None,
        }
    }

    /// Simple case-sensitive `LIKE` pattern matching (supports `%` and `_`).
    pub fn like_match(text: &str, pattern: &str) -> bool {
        crate::executor::like::like_match(text, pattern, false)
    }
}
