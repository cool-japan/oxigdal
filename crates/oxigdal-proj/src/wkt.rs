//! Well-Known Text (WKT) parser for CRS definitions.
//!
//! This module provides parsing capabilities for WKT (Well-Known Text) format CRS definitions.
//! WKT is a text markup language for representing coordinate reference systems and geometric objects.

use crate::error::{Error, Result};
#[cfg(not(feature = "std"))]
use alloc::collections::BTreeMap as HashMap;
#[cfg(not(feature = "std"))]
use alloc::format;
#[cfg(not(feature = "std"))]
use alloc::string::{String, ToString};
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::collections::HashMap;

/// WKT parser for coordinate reference systems.
pub struct WktParser {
    input: String,
    position: usize,
}

/// Parsed WKT node representing a CRS component.
#[derive(Debug, Clone, PartialEq)]
pub struct WktNode {
    /// Node type (e.g., "GEOGCS", "PROJCS", "DATUM")
    pub node_type: String,
    /// Node value (first string in brackets, if present)
    pub value: Option<String>,
    /// Child nodes
    pub children: Vec<WktNode>,
    /// Parameters (key-value pairs)
    pub parameters: HashMap<String, String>,
}

impl WktParser {
    /// Creates a new WKT parser.
    pub fn new<S: Into<String>>(input: S) -> Self {
        Self {
            input: input.into(),
            position: 0,
        }
    }

    /// Parses the WKT string.
    ///
    /// # Errors
    ///
    /// Returns an error if the WKT string is malformed.
    pub fn parse(&mut self) -> Result<WktNode> {
        self.skip_whitespace();
        self.parse_node()
    }

    /// Parses a WKT node.
    fn parse_node(&mut self) -> Result<WktNode> {
        // Parse node type
        let node_type = self.parse_identifier()?;

        self.skip_whitespace();

        // Expect opening bracket
        if !self.expect_char('[')? {
            return Err(Error::wkt_parse_error(
                self.position,
                format!("Expected '[' after {}", node_type),
            ));
        }

        self.skip_whitespace();

        // Parse node value (first string, if present)
        let value = if self.peek_char() == Some('"') {
            Some(self.parse_string()?)
        } else {
            None
        };

        let mut children = Vec::new();
        let mut parameters = HashMap::new();
        let mut first_item = value.is_none(); // Track if we're parsing first item after name

        // Parse children and parameters
        loop {
            self.skip_whitespace();

            // Check for closing bracket
            if self.peek_char() == Some(']') {
                self.advance();
                break;
            }

            // Expect comma separator (except before first item)
            if !first_item {
                if !self.expect_char(',')? {
                    return Err(Error::wkt_parse_error(
                        self.position,
                        "Expected ',' or ']'".to_string(),
                    ));
                }
                self.skip_whitespace();
            }
            first_item = false;

            // Try to parse as child node or parameter
            // Look ahead to determine if this is a node (IDENTIFIER[) or parameter
            let saved_pos = self.position;

            if self.is_identifier_start() {
                // Try to parse identifier
                let _ident_result = self.parse_identifier();
                if _ident_result.is_ok() {
                    self.skip_whitespace();

                    if self.peek_char() == Some('[') {
                        // This is a node - reset and parse as node
                        self.position = saved_pos;
                        children.push(self.parse_node()?);
                    } else {
                        // This is a parameter - reset and parse as parameter
                        self.position = saved_pos;
                        let (key, value) = self.parse_parameter()?;
                        parameters.insert(key, value);
                    }
                } else {
                    // Failed to parse identifier, try as parameter
                    self.position = saved_pos;
                    let (key, value) = self.parse_parameter()?;
                    parameters.insert(key, value);
                }
            } else {
                // Not an identifier start, parse as parameter (number or string)
                let (key, value) = self.parse_parameter()?;
                parameters.insert(key, value);
            }
        }

        Ok(WktNode {
            node_type,
            value,
            children,
            parameters,
        })
    }

    /// Parses an identifier (e.g., GEOGCS, DATUM).
    fn parse_identifier(&mut self) -> Result<String> {
        let mut ident = String::new();

        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ident.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        if ident.is_empty() {
            Err(Error::wkt_parse_error(
                self.position,
                "Expected identifier".to_string(),
            ))
        } else {
            Ok(ident)
        }
    }

    /// Parses a quoted string.
    fn parse_string(&mut self) -> Result<String> {
        if !self.expect_char('"')? {
            return Err(Error::wkt_parse_error(
                self.position,
                "Expected '\"'".to_string(),
            ));
        }

        let mut value = String::new();

        loop {
            match self.peek_char() {
                Some('"') => {
                    self.advance();
                    break;
                }
                Some('\\') => {
                    self.advance();
                    if let Some(ch) = self.peek_char() {
                        value.push(ch);
                        self.advance();
                    } else {
                        return Err(Error::wkt_parse_error(
                            self.position,
                            "Unexpected end of string".to_string(),
                        ));
                    }
                }
                Some(ch) => {
                    value.push(ch);
                    self.advance();
                }
                None => {
                    return Err(Error::wkt_parse_error(
                        self.position,
                        "Unterminated string".to_string(),
                    ));
                }
            }
        }

        Ok(value)
    }

    /// Parses a parameter (key-value pair or just value).
    fn parse_parameter(&mut self) -> Result<(String, String)> {
        // Try to parse as identifier=value or just value
        let saved_pos = self.position;

        if let Ok(ident) = self.parse_identifier() {
            self.skip_whitespace();
            if self.peek_char() == Some('=') {
                self.advance();
                self.skip_whitespace();
                let value = self.parse_value()?;
                return Ok((ident, value));
            }
        }

        // Reset and parse as just a value
        self.position = saved_pos;
        let value = self.parse_value()?;
        Ok((format!("param_{}", self.position), value))
    }

    /// Parses a value (string or number).
    fn parse_value(&mut self) -> Result<String> {
        self.skip_whitespace();

        if self.peek_char() == Some('"') {
            self.parse_string()
        } else {
            self.parse_number()
        }
    }

    /// Parses a number.
    fn parse_number(&mut self) -> Result<String> {
        let mut number = String::new();

        // Handle negative sign
        if self.peek_char() == Some('-') {
            number.push('-');
            self.advance();
        }

        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() || ch == '.' || ch == 'e' || ch == 'E' || ch == '+' || ch == '-'
            {
                number.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        if number.is_empty() || number == "-" {
            Err(Error::wkt_parse_error(
                self.position,
                "Expected number".to_string(),
            ))
        } else {
            Ok(number)
        }
    }

    /// Checks if the current character is the start of an identifier.
    fn is_identifier_start(&self) -> bool {
        matches!(self.peek_char(), Some(ch) if ch.is_ascii_alphabetic() || ch == '_')
    }

    /// Expects a specific character.
    fn expect_char(&mut self, expected: char) -> Result<bool> {
        if self.peek_char() == Some(expected) {
            self.advance();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Peeks at the current character without consuming it.
    fn peek_char(&self) -> Option<char> {
        self.input.chars().nth(self.position)
    }

    /// Advances to the next character.
    fn advance(&mut self) {
        if self.position < self.input.len() {
            self.position += 1;
        }
    }

    /// Skips whitespace characters.
    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }
}

impl WktNode {
    /// Finds a child node by type.
    pub fn find_child(&self, node_type: &str) -> Option<&WktNode> {
        self.children
            .iter()
            .find(|child| child.node_type == node_type)
    }

    /// Finds all child nodes of a given type.
    pub fn find_children(&self, node_type: &str) -> Vec<&WktNode> {
        self.children
            .iter()
            .filter(|child| child.node_type == node_type)
            .collect()
    }

    /// Gets a parameter value by key.
    pub fn get_parameter(&self, key: &str) -> Option<&str> {
        self.parameters.get(key).map(|s| s.as_str())
    }

    /// Converts the WKT node to a string representation.
    pub fn to_string_repr(&self) -> String {
        let mut result = self.node_type.clone();
        result.push('[');

        if let Some(value) = &self.value {
            result.push('"');
            result.push_str(value);
            result.push('"');

            if !self.children.is_empty() || !self.parameters.is_empty() {
                result.push(',');
            }
        }

        for (i, child) in self.children.iter().enumerate() {
            if i > 0 || self.value.is_some() {
                result.push(',');
            }
            result.push_str(&child.to_string_repr());
        }

        for (i, (key, value)) in self.parameters.iter().enumerate() {
            if i > 0 || !self.children.is_empty() || self.value.is_some() {
                result.push(',');
            }
            result.push_str(key);
            result.push('=');
            result.push_str(value);
        }

        result.push(']');
        result
    }
}

/// Parses a WKT string into a node structure.
///
/// # Arguments
///
/// * `wkt` - WKT string to parse
///
/// # Errors
///
/// Returns an error if the WKT string is malformed.
pub fn parse_wkt<S: Into<String>>(wkt: S) -> Result<WktNode> {
    let mut parser = WktParser::new(wkt);
    parser.parse()
}

// =============================================================================
// WKT version and error types
// =============================================================================

/// WKT format version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WktVersion {
    /// WKT 1 (uses PROJCS / GEOGCS keywords).
    Wkt1,
    /// WKT 2 (ISO 19162:2019, uses PROJCRS / GEOGCRS keywords).
    Wkt2,
    /// Version could not be determined.
    Unknown,
}

/// Error type for WKT parsing failures.
#[derive(Debug, Clone)]
pub struct WktError {
    /// Human-readable error message.
    pub message: String,
    /// Byte offset in the input string where the error was detected, if known.
    pub position: Option<usize>,
}

impl WktError {
    /// Construct a new `WktError`.
    pub fn new(message: impl Into<String>, position: Option<usize>) -> Self {
        Self {
            message: message.into(),
            position,
        }
    }
}

#[cfg(feature = "std")]
impl std::fmt::Display for WktError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.position {
            Some(pos) => write!(f, "WKT parse error at position {}: {}", pos, self.message),
            None => write!(f, "WKT parse error: {}", self.message),
        }
    }
}

#[cfg(not(feature = "std"))]
impl core::fmt::Display for WktError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.position {
            Some(pos) => write!(f, "WKT parse error at position {}: {}", pos, self.message),
            None => write!(f, "WKT parse error: {}", self.message),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for WktError {}

// =============================================================================
// Static / free-function methods on WktParser
// =============================================================================

impl WktParser {
    /// Detect the WKT version of the given string.
    ///
    /// - Returns `WktVersion::Wkt2` if the string contains WKT 2 keywords
    ///   (`PROJCRS`, `GEOGCRS`, `GEODCRS`, `ENGCRS`, `VERTCRS`, `COMPOUNDCRS`).
    /// - Returns `WktVersion::Wkt1` if it contains WKT 1 keywords
    ///   (`PROJCS`, `GEOGCS`, `GEOCCS`, `VERT_CS`, `COMPD_CS`).
    /// - Returns `WktVersion::Unknown` otherwise.
    pub fn detect_version(wkt: &str) -> WktVersion {
        let upper = wkt.to_uppercase();
        // WKT2 keywords (longer/newer forms come first to avoid false matches)
        if upper.contains("PROJCRS")
            || upper.contains("GEOGCRS")
            || upper.contains("GEODCRS")
            || upper.contains("ENGCRS")
            || upper.contains("VERTCRS")
            || upper.contains("COMPOUNDCRS")
        {
            return WktVersion::Wkt2;
        }
        // WKT1 keywords
        if upper.contains("PROJCS")
            || upper.contains("GEOGCS")
            || upper.contains("GEOCCS")
            || upper.contains("VERT_CS")
            || upper.contains("COMPD_CS")
        {
            return WktVersion::Wkt1;
        }
        WktVersion::Unknown
    }

    /// Extract the top-level name from a WKT string.
    ///
    /// For example:
    /// - `PROJCS["WGS 84 / UTM zone 32N",...]` → `Some("WGS 84 / UTM zone 32N")`
    /// - `GEOGCS["WGS 84",...]` → `Some("WGS 84")`
    pub fn extract_name(wkt: &str) -> Option<String> {
        // Find the first '[' after the leading keyword
        let bracket_pos = wkt.find('[')?;
        let after_bracket = &wkt[bracket_pos + 1..];

        // Find the first '"'
        let quote_start = after_bracket.find('"')?;
        let after_quote = &after_bracket[quote_start + 1..];

        // Find closing '"'
        let quote_end = after_quote.find('"')?;
        Some(after_quote[..quote_end].to_string())
    }

    /// Extract the EPSG code from a WKT string.
    ///
    /// Searches for:
    /// - `AUTHORITY["EPSG","<code>"]`  (WKT 1)
    /// - `ID["EPSG",<code>]`           (WKT 2)
    pub fn extract_epsg(wkt: &str) -> Option<i32> {
        // Try WKT1 form: AUTHORITY["EPSG","4326"]
        if let Some(idx) = wkt.find("AUTHORITY[\"EPSG\",\"") {
            let after = &wkt[idx + "AUTHORITY[\"EPSG\",\"".len()..];
            let end = after.find('"')?;
            let code_str = &after[..end];
            return code_str.parse::<i32>().ok();
        }
        // Try WKT2 form: ID["EPSG",4326]
        if let Some(idx) = wkt.find("ID[\"EPSG\",") {
            let after = &wkt[idx + "ID[\"EPSG\",".len()..];
            // code may be quoted or unquoted
            let after = after.trim_start();
            let after = after.trim_start_matches('"');
            let end = after.find(|c: char| !c.is_ascii_digit())?;
            let code_str = &after[..end];
            return code_str.parse::<i32>().ok();
        }
        None
    }

    /// Extract the unit name and conversion factor from a WKT string.
    ///
    /// Searches for `UNIT["<name>",<factor>]` or `LENGTHUNIT["<name>",<factor>]`.
    pub fn extract_unit(wkt: &str) -> Option<(String, f64)> {
        // Try UNIT[ first, then LENGTHUNIT[
        let search_terms = ["UNIT[\"", "LENGTHUNIT[\"", "ANGLEUNIT[\""];
        for term in &search_terms {
            if let Some(idx) = wkt.find(term) {
                let after = &wkt[idx + term.len()..];
                let name_end = after.find('"')?;
                let unit_name = after[..name_end].to_string();
                // skip past name,"
                let rest = &after[name_end + 1..];
                let comma_pos = rest.find(',')?;
                let rest_after_comma = rest[comma_pos + 1..].trim_start();
                // extract number up to ']' or ','
                let num_end = rest_after_comma
                    .find([']', ','])
                    .unwrap_or(rest_after_comma.len());
                let factor_str = rest_after_comma[..num_end].trim();
                if let Ok(factor) = factor_str.parse::<f64>() {
                    return Some((unit_name, factor));
                }
            }
        }
        None
    }

    /// Parse a WKT string into a `CrsDefinition` (best-effort).
    ///
    /// This performs syntactic analysis only; it does not validate geodetic parameters.
    #[cfg(feature = "std")]
    pub fn parse_crs(
        wkt: &str,
    ) -> core::result::Result<crate::crs_registry::CrsDefinition, WktError> {
        use crate::crs_registry::{AreaOfUse, CrsDefinition, CrsType, CrsUnit};

        if wkt.trim().is_empty() {
            return Err(WktError::new("WKT string is empty", Some(0)));
        }

        let name = match Self::extract_name(wkt) {
            Some(n) => n,
            None => return Err(WktError::new("Could not extract CRS name from WKT", None)),
        };

        let epsg_code = Self::extract_epsg(wkt);

        // Detect CRS type from leading keyword
        let upper = wkt.trim_start().to_uppercase();
        let crs_type = if upper.starts_with("PROJCRS") || upper.starts_with("PROJCS") {
            CrsType::Projected
        } else if upper.starts_with("GEOGCRS") || upper.starts_with("GEOGCS") {
            CrsType::Geographic2D
        } else if upper.starts_with("GEOCCS") || upper.starts_with("GEODCRS") {
            CrsType::Geocentric
        } else if upper.starts_with("VERT_CS") || upper.starts_with("VERTCRS") {
            CrsType::Vertical
        } else if upper.starts_with("COMPD_CS") || upper.starts_with("COMPOUNDCRS") {
            CrsType::Compound
        } else {
            CrsType::Geographic2D // fallback
        };

        // Attempt to extract datum name (simple heuristic: value of DATUM node)
        let datum = extract_datum_name(wkt).unwrap_or_default();

        // Attempt to extract unit
        let unit = match Self::extract_unit(wkt) {
            Some((_, factor)) if (factor - 1.0).abs() < f64::EPSILON => CrsUnit::Metre,
            Some((ref name_str, _)) if name_str.to_lowercase().contains("degree") => {
                CrsUnit::Degree
            }
            Some((ref name_str, _)) if name_str.to_lowercase().contains("foot") => {
                CrsUnit::FootIntl
            }
            _ => match crs_type {
                CrsType::Projected => CrsUnit::Metre,
                _ => CrsUnit::Degree,
            },
        };

        Ok(CrsDefinition {
            epsg_code,
            name: name.clone(),
            crs_type,
            datum,
            unit,
            proj_string: None,
            wkt_name: Some(name),
            area_of_use: None::<AreaOfUse>,
            deprecated: false,
        })
    }
}

/// Extract the datum name from a WKT string by finding `DATUM["<name>"`.
fn extract_datum_name(wkt: &str) -> Option<String> {
    let idx = wkt.find("DATUM[\"").or_else(|| wkt.find("DATUM [\""))?;
    let after = &wkt[idx..];
    let bracket_pos = after.find('[')?;
    let after_bracket = &after[bracket_pos + 1..];
    let quote_start = after_bracket.find('"')?;
    let after_quote = &after_bracket[quote_start + 1..];
    let quote_end = after_quote.find('"')?;
    Some(after_quote[..quote_end].to_string())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_geogcs() {
        let wkt = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]]]"#;
        let result = parse_wkt(wkt);
        assert!(result.is_ok());

        let node = result.expect("should parse");
        assert_eq!(node.node_type, "GEOGCS");
        assert_eq!(node.value, Some("WGS 84".to_string()));
        assert!(node.find_child("DATUM").is_some());
    }

    #[test]
    fn test_parse_projcs() {
        let wkt = r#"PROJCS["WGS 84 / UTM zone 33N",GEOGCS["WGS 84",DATUM["WGS_1984"]]]"#;
        let result = parse_wkt(wkt);
        assert!(result.is_ok());

        let node = result.expect("should parse");
        assert_eq!(node.node_type, "PROJCS");
        assert_eq!(node.value, Some("WGS 84 / UTM zone 33N".to_string()));
        assert!(node.find_child("GEOGCS").is_some());
    }

    #[test]
    fn test_parse_with_parameters() {
        let wkt = r#"SPHEROID["WGS 84",6378137,298.257223563]"#;
        let result = parse_wkt(wkt);
        assert!(result.is_ok());

        let node = result.expect("should parse");
        assert_eq!(node.node_type, "SPHEROID");
        assert_eq!(node.value, Some("WGS 84".to_string()));
    }

    #[test]
    fn test_parse_nested() {
        let wkt = r#"DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]]"#;
        let result = parse_wkt(wkt);
        assert!(result.is_ok());

        let node = result.expect("should parse");
        assert_eq!(node.node_type, "DATUM");
        assert_eq!(node.value, Some("WGS_1984".to_string()));

        let spheroid = node.find_child("SPHEROID");
        assert!(spheroid.is_some());
        let spheroid = spheroid.expect("should have spheroid");
        assert_eq!(spheroid.value, Some("WGS 84".to_string()));
    }

    #[test]
    fn test_parse_invalid_wkt() {
        // Missing closing bracket
        let result = parse_wkt(r#"GEOGCS["WGS 84""#);
        assert!(result.is_err());

        // Missing opening bracket
        let result = parse_wkt(r#"GEOGCS"WGS 84"]"#);
        assert!(result.is_err());

        // Empty string
        let result = parse_wkt("");
        assert!(result.is_err());
    }

    #[test]
    fn test_find_child() {
        let wkt = r#"GEOGCS["WGS 84",DATUM["WGS_1984"],PRIMEM["Greenwich",0]]"#;
        let node = parse_wkt(wkt).expect("should parse");

        assert!(node.find_child("DATUM").is_some());
        assert!(node.find_child("PRIMEM").is_some());
        assert!(node.find_child("NONEXISTENT").is_none());
    }

    #[test]
    fn test_find_children() {
        let wkt = r#"COMPD_CS["name",GEOGCS["WGS 84"],VERT_CS["height"]]"#;
        let node = parse_wkt(wkt).expect("should parse");

        let geogcs = node.find_children("GEOGCS");
        assert_eq!(geogcs.len(), 1);

        let vert_cs = node.find_children("VERT_CS");
        assert_eq!(vert_cs.len(), 1);
    }

    #[test]
    fn test_node_to_string() {
        let wkt = r#"SPHEROID["WGS 84",6378137,298.257223563]"#;
        let node = parse_wkt(wkt).expect("should parse");
        let result = node.to_string_repr();

        // Should contain the essential components
        assert!(result.contains("SPHEROID"));
        assert!(result.contains("WGS 84"));
    }
}
