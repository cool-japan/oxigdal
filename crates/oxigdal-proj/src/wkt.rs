//! Well-Known Text (WKT) parser for CRS definitions.
//!
//! This module provides parsing capabilities for WKT (Well-Known Text) format CRS definitions.
//! WKT is a text markup language for representing coordinate reference systems and geometric objects.

use crate::error::{Error, Result};
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
