//! CQL2 minimal parser and evaluator.

use super::error::FeaturesError;

/// A CQL2 scalar value
#[derive(Debug, Clone, PartialEq)]
pub enum CqlValue {
    /// Text string
    String(String),
    /// Numeric value
    Number(f64),
    /// Boolean
    Bool(bool),
}

/// A CQL2 expression node
#[derive(Debug, Clone, PartialEq)]
pub enum CqlExpr {
    /// Equality comparison
    Eq {
        /// Property name
        property: String,
        /// Comparison value
        value: CqlValue,
    },
    /// Less-than comparison
    Lt {
        /// Property name
        property: String,
        /// Threshold
        value: f64,
    },
    /// Less-than-or-equal comparison
    Lte {
        /// Property name
        property: String,
        /// Threshold
        value: f64,
    },
    /// Greater-than comparison
    Gt {
        /// Property name
        property: String,
        /// Threshold
        value: f64,
    },
    /// Greater-than-or-equal comparison
    Gte {
        /// Property name
        property: String,
        /// Threshold
        value: f64,
    },
    /// SQL LIKE pattern match (`%` and `_` wildcards)
    Like {
        /// Property name
        property: String,
        /// Pattern string
        pattern: String,
    },
    /// BETWEEN range check (inclusive)
    Between {
        /// Property name
        property: String,
        /// Lower bound
        low: f64,
        /// Upper bound
        high: f64,
    },
    /// Logical AND of two expressions
    And(Box<CqlExpr>, Box<CqlExpr>),
    /// Logical OR of two expressions
    Or(Box<CqlExpr>, Box<CqlExpr>),
    /// Logical NOT of an expression
    Not(Box<CqlExpr>),
}

/// Minimal CQL2-text parser and evaluator
pub struct CqlParser;

impl CqlParser {
    /// Parse a simple CQL2-text expression string into a `CqlExpr`.
    ///
    /// Supported forms (case-insensitive keywords):
    /// - `name = 'London'`
    /// - `population > 1000000`
    /// - `name LIKE '%city%'`
    /// - `age BETWEEN 18 AND 65`
    /// - `a > 5 AND b < 10`
    /// - `NOT (a > 5)`
    pub fn parse(input: &str) -> Result<CqlExpr, FeaturesError> {
        let trimmed = input.trim();
        Self::parse_or(trimmed)
    }

    // ── recursive descent ───────────────────────────────────────────────────

    fn parse_or(input: &str) -> Result<CqlExpr, FeaturesError> {
        // Split on " OR " (case-insensitive, top level)
        if let Some(idx) = Self::find_keyword_boundary(input, " OR ") {
            let left = Self::parse_and(&input[..idx])?;
            let right = Self::parse_or(&input[idx + 4..])?;
            return Ok(CqlExpr::Or(Box::new(left), Box::new(right)));
        }
        Self::parse_and(input)
    }

    fn parse_and(input: &str) -> Result<CqlExpr, FeaturesError> {
        // Split on " AND " but NOT inside "BETWEEN … AND …"
        if let Some(idx) = Self::find_and_not_between(input) {
            let left = Self::parse_not(&input[..idx])?;
            let right = Self::parse_and(&input[idx + 5..])?;
            return Ok(CqlExpr::And(Box::new(left), Box::new(right)));
        }
        Self::parse_not(input)
    }

    fn parse_not(input: &str) -> Result<CqlExpr, FeaturesError> {
        let s = input.trim();
        let upper = s.to_ascii_uppercase();
        if upper.starts_with("NOT ") {
            let inner = s[4..].trim();
            // strip optional parentheses
            let inner = Self::strip_parens(inner);
            return Ok(CqlExpr::Not(Box::new(Self::parse_or(inner)?)));
        }
        // Strip surrounding parentheses then retry
        if s.starts_with('(') && s.ends_with(')') {
            let inner = &s[1..s.len() - 1];
            return Self::parse_or(inner.trim());
        }
        Self::parse_atom(s)
    }

    fn parse_atom(input: &str) -> Result<CqlExpr, FeaturesError> {
        let s = input.trim();
        let upper = s.to_ascii_uppercase();

        // BETWEEN
        if let Some(between_idx) = upper.find(" BETWEEN ") {
            let property = s[..between_idx].trim().to_string();
            let rest = &s[between_idx + 9..];
            let upper_rest = rest.to_ascii_uppercase();
            if let Some(and_idx) = upper_rest.find(" AND ") {
                let low: f64 = rest[..and_idx].trim().parse().map_err(|_| {
                    FeaturesError::CqlParseError(format!(
                        "BETWEEN low bound not numeric: {}",
                        &rest[..and_idx]
                    ))
                })?;
                let high: f64 = rest[and_idx + 5..].trim().parse().map_err(|_| {
                    FeaturesError::CqlParseError(format!(
                        "BETWEEN high bound not numeric: {}",
                        &rest[and_idx + 5..]
                    ))
                })?;
                return Ok(CqlExpr::Between {
                    property,
                    low,
                    high,
                });
            }
        }

        // LIKE
        if let Some(like_idx) = upper.find(" LIKE ") {
            let property = s[..like_idx].trim().to_string();
            let pattern_raw = s[like_idx + 6..].trim();
            let pattern = Self::unquote(pattern_raw)?;
            return Ok(CqlExpr::Like { property, pattern });
        }

        // Comparison operators — longest first to avoid ambiguity
        for (op_str, builder) in &[
            (
                ">=",
                Self::build_gte as fn(&str, &str) -> Result<CqlExpr, FeaturesError>,
            ),
            ("<=", Self::build_lte),
            ("!=", Self::build_neq_placeholder),
            (">", Self::build_gt),
            ("<", Self::build_lt),
            ("=", Self::build_eq),
        ] {
            if let Some(op_idx) = s.find(op_str) {
                let property = s[..op_idx].trim().to_string();
                let value_str = s[op_idx + op_str.len()..].trim();
                return builder(&property, value_str);
            }
        }

        Err(FeaturesError::CqlParseError(format!(
            "Cannot parse atom: {s}"
        )))
    }

    // ── operator builders ────────────────────────────────────────────────────

    fn build_eq(property: &str, value_str: &str) -> Result<CqlExpr, FeaturesError> {
        let value = Self::parse_value(value_str)?;
        Ok(CqlExpr::Eq {
            property: property.to_string(),
            value,
        })
    }

    fn build_lt(property: &str, value_str: &str) -> Result<CqlExpr, FeaturesError> {
        let v: f64 = value_str.parse().map_err(|_| {
            FeaturesError::CqlParseError(format!("Expected number after '<': {value_str}"))
        })?;
        Ok(CqlExpr::Lt {
            property: property.to_string(),
            value: v,
        })
    }

    fn build_lte(property: &str, value_str: &str) -> Result<CqlExpr, FeaturesError> {
        let v: f64 = value_str.parse().map_err(|_| {
            FeaturesError::CqlParseError(format!("Expected number after '<=': {value_str}"))
        })?;
        Ok(CqlExpr::Lte {
            property: property.to_string(),
            value: v,
        })
    }

    fn build_gt(property: &str, value_str: &str) -> Result<CqlExpr, FeaturesError> {
        let v: f64 = value_str.parse().map_err(|_| {
            FeaturesError::CqlParseError(format!("Expected number after '>': {value_str}"))
        })?;
        Ok(CqlExpr::Gt {
            property: property.to_string(),
            value: v,
        })
    }

    fn build_gte(property: &str, value_str: &str) -> Result<CqlExpr, FeaturesError> {
        let v: f64 = value_str.parse().map_err(|_| {
            FeaturesError::CqlParseError(format!("Expected number after '>=': {value_str}"))
        })?;
        Ok(CqlExpr::Gte {
            property: property.to_string(),
            value: v,
        })
    }

    fn build_neq_placeholder(_property: &str, _value_str: &str) -> Result<CqlExpr, FeaturesError> {
        Err(FeaturesError::CqlParseError(
            "!= operator is not yet supported".to_string(),
        ))
    }

    // ── value parsing helpers ────────────────────────────────────────────────

    fn parse_value(s: &str) -> Result<CqlValue, FeaturesError> {
        let s = s.trim();
        // Quoted string
        if (s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')) {
            return Ok(CqlValue::String(Self::unquote(s)?));
        }
        // Boolean
        match s.to_ascii_uppercase().as_str() {
            "TRUE" => return Ok(CqlValue::Bool(true)),
            "FALSE" => return Ok(CqlValue::Bool(false)),
            _ => {}
        }
        // Number
        if let Ok(n) = s.parse::<f64>() {
            return Ok(CqlValue::Number(n));
        }
        // Bare string (unquoted identifier value)
        Ok(CqlValue::String(s.to_string()))
    }

    fn unquote(s: &str) -> Result<String, FeaturesError> {
        let s = s.trim();
        if (s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')) {
            Ok(s[1..s.len() - 1].to_string())
        } else {
            Ok(s.to_string())
        }
    }

    fn strip_parens(s: &str) -> &str {
        let s = s.trim();
        if s.starts_with('(') && s.ends_with(')') {
            &s[1..s.len() - 1]
        } else {
            s
        }
    }

    /// Find the byte offset of the first top-level occurrence of `keyword`
    /// (case-insensitive), ignoring occurrences inside parentheses.
    fn find_keyword_boundary(input: &str, keyword: &str) -> Option<usize> {
        let upper = input.to_ascii_uppercase();
        let kw_upper = keyword.to_ascii_uppercase();
        let mut depth = 0usize;
        let bytes = input.as_bytes();
        let kw_len = kw_upper.len();
        let kw_bytes = kw_upper.as_bytes();

        let mut i = 0;
        while i + kw_len <= bytes.len() {
            match bytes[i] {
                b'(' => {
                    depth += 1;
                    i += 1;
                }
                b')' => {
                    depth = depth.saturating_sub(1);
                    i += 1;
                }
                b'\'' | b'"' => {
                    // skip quoted string
                    let quote = bytes[i];
                    i += 1;
                    while i < bytes.len() && bytes[i] != quote {
                        i += 1;
                    }
                    i += 1; // closing quote
                }
                _ => {
                    if depth == 0 && upper.as_bytes()[i..].starts_with(kw_bytes) {
                        return Some(i);
                    }
                    i += 1;
                }
            }
        }
        None
    }

    /// Find ` AND ` that is NOT part of a `BETWEEN … AND …` construct.
    fn find_and_not_between(input: &str) -> Option<usize> {
        let upper = input.to_ascii_uppercase();
        let mut search_start = 0;

        while let Some(rel) = upper[search_start..].find(" AND ") {
            let abs = search_start + rel;
            // Check if this AND is part of BETWEEN … AND
            let prefix = &upper[..abs];
            // Find nearest BETWEEN before abs that is not already paired
            if Self::is_between_and(prefix) {
                search_start = abs + 5;
                continue;
            }
            return Some(abs);
        }
        None
    }

    /// Heuristic: is the upcoming " AND " completing a BETWEEN expression?
    ///
    /// We look at whether the token before this AND has a BETWEEN in the same
    /// clause that hasn't been closed yet.
    fn is_between_and(prefix: &str) -> bool {
        // Count BETWEEN occurrences and AND occurrences in the prefix.
        // If betweens > ands already consumed, this AND closes a BETWEEN.
        let p = prefix.to_ascii_uppercase();
        let between_count = p.matches(" BETWEEN ").count();
        // Count ANDs that are already present in the prefix (closing previous BETWEENs)
        let and_count = p.matches(" AND ").count();
        between_count > and_count
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Evaluator
    // ─────────────────────────────────────────────────────────────────────────

    /// Evaluate a `CqlExpr` against a JSON properties object.
    ///
    /// Returns `true` if the expression holds for these properties.
    pub fn evaluate(expr: &CqlExpr, properties: &serde_json::Value) -> bool {
        match expr {
            CqlExpr::Eq { property, value } => {
                let prop = Self::get_prop(properties, property);
                match (value, &prop) {
                    (CqlValue::String(s), serde_json::Value::String(ps)) => s == ps,
                    (CqlValue::Number(n), serde_json::Value::Number(pn)) => {
                        pn.as_f64().is_some_and(|v| (v - n).abs() < f64::EPSILON)
                    }
                    (CqlValue::Bool(b), serde_json::Value::Bool(pb)) => b == pb,
                    _ => false,
                }
            }

            CqlExpr::Lt { property, value } => {
                Self::numeric_prop(properties, property).is_some_and(|v| v < *value)
            }
            CqlExpr::Lte { property, value } => {
                Self::numeric_prop(properties, property).is_some_and(|v| v <= *value)
            }
            CqlExpr::Gt { property, value } => {
                Self::numeric_prop(properties, property).is_some_and(|v| v > *value)
            }
            CqlExpr::Gte { property, value } => {
                Self::numeric_prop(properties, property).is_some_and(|v| v >= *value)
            }

            CqlExpr::Like { property, pattern } => {
                if let serde_json::Value::String(s) = Self::get_prop(properties, property) {
                    Self::like_match(&s, pattern)
                } else {
                    false
                }
            }

            CqlExpr::Between {
                property,
                low,
                high,
            } => Self::numeric_prop(properties, property).is_some_and(|v| v >= *low && v <= *high),

            CqlExpr::And(a, b) => Self::evaluate(a, properties) && Self::evaluate(b, properties),
            CqlExpr::Or(a, b) => Self::evaluate(a, properties) || Self::evaluate(b, properties),
            CqlExpr::Not(inner) => !Self::evaluate(inner, properties),
        }
    }

    fn get_prop(properties: &serde_json::Value, key: &str) -> serde_json::Value {
        match properties {
            serde_json::Value::Object(map) => {
                map.get(key).cloned().unwrap_or(serde_json::Value::Null)
            }
            _ => serde_json::Value::Null,
        }
    }

    fn numeric_prop(properties: &serde_json::Value, key: &str) -> Option<f64> {
        match Self::get_prop(properties, key) {
            serde_json::Value::Number(n) => n.as_f64(),
            _ => None,
        }
    }

    /// SQL LIKE matching with `%` (any sequence) and `_` (any single char).
    fn like_match(value: &str, pattern: &str) -> bool {
        Self::like_recursive(value.as_bytes(), pattern.as_bytes())
    }

    fn like_recursive(value: &[u8], pattern: &[u8]) -> bool {
        if pattern.is_empty() {
            return value.is_empty();
        }
        match pattern[0] {
            b'%' => {
                // Match zero or more characters
                for i in 0..=value.len() {
                    if Self::like_recursive(&value[i..], &pattern[1..]) {
                        return true;
                    }
                }
                false
            }
            b'_' => {
                // Match exactly one character
                if value.is_empty() {
                    false
                } else {
                    Self::like_recursive(&value[1..], &pattern[1..])
                }
            }
            ch => {
                if value.is_empty() || value[0] != ch {
                    false
                } else {
                    Self::like_recursive(&value[1..], &pattern[1..])
                }
            }
        }
    }
}
