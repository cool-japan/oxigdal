//! Request/response transformation module.
//!
//! Provides comprehensive transformation capabilities for API requests and responses,
//! including format conversion, header manipulation, and content negotiation.

pub mod adapters;
pub mod request;
pub mod response;

use crate::error::{GatewayError, Result};
use std::collections::HashMap;

/// Content type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentType {
    /// JSON format
    Json,
    /// XML format
    Xml,
    /// Protocol Buffers
    Protobuf,
    /// MessagePack
    MessagePack,
    /// Plain text
    Text,
    /// Binary data
    Binary,
}

impl ContentType {
    /// Parses content type from MIME type string.
    pub fn from_mime(mime: &str) -> Option<Self> {
        match mime {
            "application/json" => Some(Self::Json),
            "application/xml" | "text/xml" => Some(Self::Xml),
            "application/protobuf" | "application/x-protobuf" => Some(Self::Protobuf),
            "application/msgpack" | "application/x-msgpack" => Some(Self::MessagePack),
            "text/plain" => Some(Self::Text),
            "application/octet-stream" => Some(Self::Binary),
            _ => None,
        }
    }

    /// Converts content type to MIME type string.
    pub fn to_mime(&self) -> &'static str {
        match self {
            Self::Json => "application/json",
            Self::Xml => "application/xml",
            Self::Protobuf => "application/protobuf",
            Self::MessagePack => "application/msgpack",
            Self::Text => "text/plain",
            Self::Binary => "application/octet-stream",
        }
    }
}

/// Transformation rule for request/response.
#[derive(Debug, Clone)]
pub struct TransformRule {
    /// Rule name
    pub name: String,
    /// Match pattern for path
    pub path_pattern: Option<String>,
    /// Match pattern for method
    pub method_pattern: Option<String>,
    /// Header transformations
    pub header_transforms: Vec<HeaderTransform>,
    /// Body transformation
    pub body_transform: Option<BodyTransform>,
}

/// Header transformation operation.
#[derive(Debug, Clone)]
pub enum HeaderTransform {
    /// Add a header
    Add {
        /// Header name
        name: String,
        /// Header value
        value: String,
    },
    /// Remove a header
    Remove {
        /// Header name
        name: String,
    },
    /// Rename a header
    Rename {
        /// Old header name
        from: String,
        /// New header name
        to: String,
    },
    /// Replace header value
    Replace {
        /// Header name
        name: String,
        /// New value
        value: String,
    },
}

/// Body transformation operation.
#[derive(Debug, Clone)]
pub enum BodyTransform {
    /// Convert format
    ConvertFormat {
        /// Source format
        from: ContentType,
        /// Target format
        to: ContentType,
    },
    /// Apply JSON path transformation
    JsonPath {
        /// JSON path expression
        path: String,
    },
    /// Apply template transformation
    Template {
        /// Template string
        template: String,
    },
}

/// Transformation engine.
pub struct TransformEngine {
    rules: Vec<TransformRule>,
    adapters: adapters::FormatAdapterRegistry,
}

impl TransformEngine {
    /// Creates a new transformation engine.
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            adapters: adapters::FormatAdapterRegistry::new(),
        }
    }

    /// Adds a transformation rule.
    pub fn add_rule(&mut self, rule: TransformRule) {
        self.rules.push(rule);
    }

    /// Removes a transformation rule by name.
    pub fn remove_rule(&mut self, name: &str) -> Option<TransformRule> {
        self.rules
            .iter()
            .position(|r| r.name == name)
            .map(|i| self.rules.remove(i))
    }

    /// Gets all transformation rules.
    pub fn rules(&self) -> &[TransformRule] {
        &self.rules
    }

    /// Transforms request headers.
    pub fn transform_request_headers(
        &self,
        path: &str,
        method: &str,
        headers: &mut HashMap<String, String>,
    ) -> Result<()> {
        for rule in &self.rules {
            if !self.matches_rule(rule, path, method) {
                continue;
            }

            for transform in &rule.header_transforms {
                match transform {
                    HeaderTransform::Add { name, value } => {
                        headers.insert(name.clone(), value.clone());
                    }
                    HeaderTransform::Remove { name } => {
                        headers.remove(name);
                    }
                    HeaderTransform::Rename { from, to } => {
                        if let Some(value) = headers.remove(from) {
                            headers.insert(to.clone(), value);
                        }
                    }
                    HeaderTransform::Replace { name, value } => {
                        headers.insert(name.clone(), value.clone());
                    }
                }
            }
        }

        Ok(())
    }

    /// Transforms request body.
    pub fn transform_request_body(
        &self,
        path: &str,
        method: &str,
        body: Vec<u8>,
        content_type: ContentType,
    ) -> Result<Vec<u8>> {
        for rule in &self.rules {
            if !self.matches_rule(rule, path, method) {
                continue;
            }

            if let Some(transform) = &rule.body_transform {
                return self.apply_body_transform(body, content_type, transform);
            }
        }

        Ok(body)
    }

    /// Checks if a rule matches the request.
    fn matches_rule(&self, rule: &TransformRule, path: &str, method: &str) -> bool {
        if let Some(pattern) = &rule.path_pattern {
            if !Self::matches_pattern(path, pattern) {
                return false;
            }
        }

        if let Some(pattern) = &rule.method_pattern {
            if method != pattern {
                return false;
            }
        }

        true
    }

    /// Checks if a string matches a pattern (simple glob-style).
    fn matches_pattern(s: &str, pattern: &str) -> bool {
        if pattern == "*" {
            return true;
        }

        if let Some(prefix) = pattern.strip_suffix('*') {
            return s.starts_with(prefix);
        }

        if let Some(suffix) = pattern.strip_prefix('*') {
            return s.ends_with(suffix);
        }

        s == pattern
    }

    /// Applies body transformation.
    fn apply_body_transform(
        &self,
        body: Vec<u8>,
        content_type: ContentType,
        transform: &BodyTransform,
    ) -> Result<Vec<u8>> {
        match transform {
            BodyTransform::ConvertFormat { from, to } => {
                if content_type != *from {
                    return Err(GatewayError::TransformationError(format!(
                        "Expected content type {:?}, got {:?}",
                        from, content_type
                    )));
                }

                self.adapters.convert(&body, *from, *to)
            }
            BodyTransform::JsonPath { path } => {
                if content_type != ContentType::Json {
                    return Err(GatewayError::TransformationError(
                        "JSONPath requires JSON content".to_string(),
                    ));
                }

                self.apply_json_path(&body, path)
            }
            BodyTransform::Template { template } => self.apply_template(&body, template),
        }
    }

    /// Applies JSON path transformation.
    fn apply_json_path(&self, body: &[u8], _path: &str) -> Result<Vec<u8>> {
        // Simplified implementation
        // In a real implementation, use a JSONPath library
        Ok(body.to_vec())
    }

    /// Applies template transformation.
    fn apply_template(&self, body: &[u8], template: &str) -> Result<Vec<u8>> {
        // Parse body as JSON
        let value: serde_json::Value = serde_json::from_slice(body)?;

        // Simple template replacement
        let mut result = template.to_string();
        if let serde_json::Value::Object(map) = value {
            for (key, val) in map {
                let placeholder = format!("{{{}}}", key);
                if let Some(s) = val.as_str() {
                    result = result.replace(&placeholder, s);
                }
            }
        }

        Ok(result.into_bytes())
    }
}

impl Default for TransformEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_type_from_mime() {
        assert_eq!(
            ContentType::from_mime("application/json"),
            Some(ContentType::Json)
        );
        assert_eq!(
            ContentType::from_mime("application/xml"),
            Some(ContentType::Xml)
        );
        assert_eq!(
            ContentType::from_mime("application/protobuf"),
            Some(ContentType::Protobuf)
        );
    }

    #[test]
    fn test_content_type_to_mime() {
        assert_eq!(ContentType::Json.to_mime(), "application/json");
        assert_eq!(ContentType::Xml.to_mime(), "application/xml");
        assert_eq!(ContentType::Protobuf.to_mime(), "application/protobuf");
    }

    #[test]
    fn test_transform_engine_creation() {
        let engine = TransformEngine::new();
        assert_eq!(engine.rules().len(), 0);
    }

    #[test]
    fn test_add_rule() {
        let mut engine = TransformEngine::new();
        let rule = TransformRule {
            name: "test_rule".to_string(),
            path_pattern: Some("/api/*".to_string()),
            method_pattern: Some("GET".to_string()),
            header_transforms: vec![],
            body_transform: None,
        };

        engine.add_rule(rule);
        assert_eq!(engine.rules().len(), 1);
    }

    #[test]
    fn test_remove_rule() {
        let mut engine = TransformEngine::new();
        let rule = TransformRule {
            name: "test_rule".to_string(),
            path_pattern: None,
            method_pattern: None,
            header_transforms: vec![],
            body_transform: None,
        };

        engine.add_rule(rule);
        assert_eq!(engine.rules().len(), 1);

        let removed = engine.remove_rule("test_rule");
        assert!(removed.is_some());
        assert_eq!(engine.rules().len(), 0);
    }

    #[test]
    fn test_header_transform() {
        let mut engine = TransformEngine::new();
        let rule = TransformRule {
            name: "add_header".to_string(),
            path_pattern: Some("/api/*".to_string()),
            method_pattern: None,
            header_transforms: vec![HeaderTransform::Add {
                name: "X-Custom-Header".to_string(),
                value: "test_value".to_string(),
            }],
            body_transform: None,
        };

        engine.add_rule(rule);

        let mut headers = HashMap::new();
        let result = engine.transform_request_headers("/api/test", "GET", &mut headers);

        assert!(result.is_ok());
        assert_eq!(
            headers.get("X-Custom-Header"),
            Some(&"test_value".to_string())
        );
    }

    #[test]
    fn test_pattern_matching() {
        assert!(TransformEngine::matches_pattern("/api/test", "/api/*"));
        assert!(TransformEngine::matches_pattern("/api/test", "*"));
        assert!(!TransformEngine::matches_pattern("/other/test", "/api/*"));
    }
}
