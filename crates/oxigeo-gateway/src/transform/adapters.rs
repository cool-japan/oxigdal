//! Format adapters for content transformation.

use super::ContentType;
use crate::error::{GatewayError, Result};
use std::collections::HashMap;

/// Format adapter trait.
pub trait FormatAdapter: Send + Sync {
    /// Converts data from one format to another.
    fn convert(&self, data: &[u8], from: ContentType, to: ContentType) -> Result<Vec<u8>>;
}

/// JSON adapter.
pub struct JsonAdapter;

impl JsonAdapter {
    /// Creates a new JSON adapter.
    pub fn new() -> Self {
        Self
    }

    /// Converts JSON to XML.
    fn json_to_xml(&self, data: &[u8]) -> Result<Vec<u8>> {
        let value: serde_json::Value = serde_json::from_slice(data)?;
        let xml = self.value_to_xml(&value, "root");
        Ok(xml.into_bytes())
    }

    /// Converts JSON value to XML string.
    fn value_to_xml(&self, value: &serde_json::Value, tag: &str) -> String {
        match value {
            serde_json::Value::Null => format!("<{} />", tag),
            serde_json::Value::Bool(b) => format!("<{}>{}</{}>", tag, b, tag),
            serde_json::Value::Number(n) => format!("<{}>{}</{}>", tag, n, tag),
            serde_json::Value::String(s) => format!("<{}>{}</{}>", tag, s, tag),
            serde_json::Value::Array(arr) => {
                let items: Vec<String> = arr.iter().map(|v| self.value_to_xml(v, "item")).collect();
                format!("<{}>{}</{}>", tag, items.join(""), tag)
            }
            serde_json::Value::Object(map) => {
                let items: Vec<String> = map.iter().map(|(k, v)| self.value_to_xml(v, k)).collect();
                format!("<{}>{}</{}>", tag, items.join(""), tag)
            }
        }
    }

    /// Converts JSON to plain text.
    fn json_to_text(&self, data: &[u8]) -> Result<Vec<u8>> {
        let value: serde_json::Value = serde_json::from_slice(data)?;
        let text = serde_json::to_string_pretty(&value)?;
        Ok(text.into_bytes())
    }
}

impl Default for JsonAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl FormatAdapter for JsonAdapter {
    fn convert(&self, data: &[u8], from: ContentType, to: ContentType) -> Result<Vec<u8>> {
        if from != ContentType::Json {
            return Err(GatewayError::TransformationError(
                "JsonAdapter only supports JSON source".to_string(),
            ));
        }

        match to {
            ContentType::Json => Ok(data.to_vec()),
            ContentType::Xml => self.json_to_xml(data),
            ContentType::Text => self.json_to_text(data),
            _ => Err(GatewayError::TransformationError(format!(
                "Unsupported conversion from JSON to {:?}",
                to
            ))),
        }
    }
}

/// XML adapter.
pub struct XmlAdapter;

impl XmlAdapter {
    /// Creates a new XML adapter.
    pub fn new() -> Self {
        Self
    }

    /// Converts XML to JSON (simplified).
    fn xml_to_json(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Simplified implementation
        // In a real implementation, use an XML parser library
        let xml_str = String::from_utf8_lossy(data);

        // Mock conversion
        let json = serde_json::json!({
            "xml": xml_str.to_string()
        });

        let bytes = serde_json::to_vec(&json)?;
        Ok(bytes)
    }

    /// Converts XML to text.
    fn xml_to_text(&self, data: &[u8]) -> Result<Vec<u8>> {
        Ok(data.to_vec())
    }
}

impl Default for XmlAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl FormatAdapter for XmlAdapter {
    fn convert(&self, data: &[u8], from: ContentType, to: ContentType) -> Result<Vec<u8>> {
        if from != ContentType::Xml {
            return Err(GatewayError::TransformationError(
                "XmlAdapter only supports XML source".to_string(),
            ));
        }

        match to {
            ContentType::Xml => Ok(data.to_vec()),
            ContentType::Json => self.xml_to_json(data),
            ContentType::Text => self.xml_to_text(data),
            _ => Err(GatewayError::TransformationError(format!(
                "Unsupported conversion from XML to {:?}",
                to
            ))),
        }
    }
}

/// Binary adapter.
pub struct BinaryAdapter;

impl BinaryAdapter {
    /// Creates a new binary adapter.
    pub fn new() -> Self {
        Self
    }

    /// Converts binary to base64-encoded JSON.
    fn binary_to_json(&self, data: &[u8]) -> Result<Vec<u8>> {
        let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, data);

        let json = serde_json::json!({
            "data": encoded,
            "encoding": "base64"
        });

        let bytes = serde_json::to_vec(&json)?;
        Ok(bytes)
    }

    /// Converts binary to hex-encoded text.
    fn binary_to_text(&self, data: &[u8]) -> Result<Vec<u8>> {
        let hex: String = data.iter().map(|b| format!("{:02x}", b)).collect();

        Ok(hex.into_bytes())
    }
}

impl Default for BinaryAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl FormatAdapter for BinaryAdapter {
    fn convert(&self, data: &[u8], from: ContentType, to: ContentType) -> Result<Vec<u8>> {
        if from != ContentType::Binary {
            return Err(GatewayError::TransformationError(
                "BinaryAdapter only supports binary source".to_string(),
            ));
        }

        match to {
            ContentType::Binary => Ok(data.to_vec()),
            ContentType::Json => self.binary_to_json(data),
            ContentType::Text => self.binary_to_text(data),
            _ => Err(GatewayError::TransformationError(format!(
                "Unsupported conversion from Binary to {:?}",
                to
            ))),
        }
    }
}

/// Format adapter registry.
pub struct FormatAdapterRegistry {
    adapters: HashMap<ContentType, Box<dyn FormatAdapter>>,
}

impl FormatAdapterRegistry {
    /// Creates a new format adapter registry with default adapters.
    pub fn new() -> Self {
        let mut adapters: HashMap<ContentType, Box<dyn FormatAdapter>> = HashMap::new();
        adapters.insert(ContentType::Json, Box::new(JsonAdapter::new()));
        adapters.insert(ContentType::Xml, Box::new(XmlAdapter::new()));
        adapters.insert(ContentType::Binary, Box::new(BinaryAdapter::new()));

        Self { adapters }
    }

    /// Registers a custom format adapter.
    pub fn register(&mut self, content_type: ContentType, adapter: Box<dyn FormatAdapter>) {
        self.adapters.insert(content_type, adapter);
    }

    /// Converts data between formats.
    pub fn convert(&self, data: &[u8], from: ContentType, to: ContentType) -> Result<Vec<u8>> {
        if from == to {
            return Ok(data.to_vec());
        }

        let adapter = self.adapters.get(&from).ok_or_else(|| {
            GatewayError::TransformationError(format!("No adapter for {:?}", from))
        })?;

        adapter.convert(data, from, to)
    }
}

impl Default for FormatAdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_to_xml() {
        let adapter = JsonAdapter::new();
        let json_data = br#"{"name":"test","value":123}"#;

        let result = adapter.convert(json_data, ContentType::Json, ContentType::Xml);
        assert!(result.is_ok());

        let xml = result.ok();
        assert!(xml.is_some());
        let xml = xml.unwrap_or_default();
        let xml_str = String::from_utf8_lossy(&xml);
        assert!(xml_str.contains("<name>test</name>"));
        assert!(xml_str.contains("<value>123</value>"));
    }

    #[test]
    fn test_json_to_text() {
        let adapter = JsonAdapter::new();
        let json_data = br#"{"name":"test"}"#;

        let result = adapter.convert(json_data, ContentType::Json, ContentType::Text);
        assert!(result.is_ok());

        let text = result.ok();
        assert!(text.is_some());
        let text = text.unwrap_or_default();
        let text_str = String::from_utf8_lossy(&text);
        assert!(text_str.contains("name"));
        assert!(text_str.contains("test"));
    }

    #[test]
    fn test_binary_to_json() {
        let adapter = BinaryAdapter::new();
        let binary_data = b"hello world";

        let result = adapter.convert(binary_data, ContentType::Binary, ContentType::Json);
        assert!(result.is_ok());

        let json = result.ok();
        assert!(json.is_some());
        let json = json.unwrap_or_default();
        let json_str = String::from_utf8_lossy(&json);
        assert!(json_str.contains("data"));
        assert!(json_str.contains("encoding"));
    }

    #[test]
    fn test_binary_to_text() {
        let adapter = BinaryAdapter::new();
        let binary_data = b"\x01\x02\x03";

        let result = adapter.convert(binary_data, ContentType::Binary, ContentType::Text);
        assert!(result.is_ok());

        let text = result.ok();
        assert!(text.is_some());
        let text = text.unwrap_or_default();
        let text_str = String::from_utf8_lossy(&text);
        assert_eq!(text_str, "010203");
    }

    #[test]
    fn test_registry() {
        let registry = FormatAdapterRegistry::new();

        let json_data = br#"{"test":true}"#;
        let result = registry.convert(json_data, ContentType::Json, ContentType::Xml);
        assert!(result.is_ok());
    }

    #[test]
    fn test_registry_same_format() {
        let registry = FormatAdapterRegistry::new();

        let json_data = br#"{"test":true}"#;
        let result = registry.convert(json_data, ContentType::Json, ContentType::Json);
        assert!(result.is_ok());

        let data = result.ok();
        assert!(data.is_some());
        let data = data.unwrap_or_default();
        assert_eq!(data, json_data);
    }
}
