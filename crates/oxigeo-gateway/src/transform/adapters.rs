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

    /// Converts XML to JSON by actually parsing the XML document.
    ///
    /// Conversion convention (a compact, widely-used mapping):
    /// - Each element becomes a JSON object keyed by its tag name at the top level.
    /// - Attributes become `@name` members.
    /// - Text content of an element with attributes/children is stored under `#text`;
    ///   an element with only text maps directly to that text value.
    /// - Repeated child tags collapse into a JSON array preserving document order.
    ///
    /// A malformed document is a hard error rather than an opaque `{ "xml": "..." }` wrapper.
    fn xml_to_json(&self, data: &[u8]) -> Result<Vec<u8>> {
        let xml_str = std::str::from_utf8(data).map_err(|e| {
            GatewayError::TransformationError(format!("XML is not valid UTF-8: {e}"))
        })?;

        let value = XmlParser::new(xml_str).parse_document()?;
        let bytes = serde_json::to_vec(&value)?;
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

/// A minimal, dependency-free recursive-descent XML parser used to convert XML documents
/// into [`serde_json::Value`]. It supports elements, attributes, nested/repeated children,
/// text content, CDATA sections, comments, the XML prolog, `<!DOCTYPE>`, self-closing tags,
/// and the five predefined entities.
struct XmlParser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> XmlParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            bytes: input.as_bytes(),
            pos: 0,
        }
    }

    fn parse_document(&mut self) -> Result<serde_json::Value> {
        self.skip_misc()?;
        let (name, value) = self.parse_element()?;
        self.skip_misc().ok();
        let mut map = serde_json::Map::new();
        map.insert(name, value);
        Ok(serde_json::Value::Object(map))
    }

    /// Skips whitespace, the `<?xml?>` prolog, comments and `<!DOCTYPE>` declarations.
    fn skip_misc(&mut self) -> Result<()> {
        loop {
            self.skip_whitespace();
            if self.starts_with(b"<?") {
                self.consume_until(b"?>")?;
            } else if self.starts_with(b"<!--") {
                self.consume_until(b"-->")?;
            } else if self.starts_with(b"<!") {
                // DOCTYPE or similar; consume to the next '>'.
                self.consume_until(b">")?;
            } else {
                break;
            }
        }
        Ok(())
    }

    fn parse_element(&mut self) -> Result<(String, serde_json::Value)> {
        if !self.starts_with(b"<") {
            return Err(GatewayError::TransformationError(
                "expected '<' at start of XML element".to_string(),
            ));
        }
        self.pos += 1; // consume '<'

        let name = self.parse_name()?;
        let mut attributes: Vec<(String, String)> = Vec::new();

        // Parse attributes.
        loop {
            self.skip_whitespace();
            if self.starts_with(b"/>") {
                self.pos += 2;
                return Ok((
                    name,
                    Self::build_value(attributes, Vec::new(), String::new()),
                ));
            }
            if self.starts_with(b">") {
                self.pos += 1;
                break;
            }
            let attr_name = self.parse_name()?;
            self.skip_whitespace();
            if !self.starts_with(b"=") {
                return Err(GatewayError::TransformationError(format!(
                    "expected '=' after attribute '{attr_name}'"
                )));
            }
            self.pos += 1; // consume '='
            self.skip_whitespace();
            let attr_value = self.parse_quoted()?;
            attributes.push((attr_name, attr_value));
        }

        // Parse children and text until the matching close tag.
        let mut children: Vec<(String, serde_json::Value)> = Vec::new();
        let mut text = String::new();

        loop {
            if self.pos >= self.bytes.len() {
                return Err(GatewayError::TransformationError(format!(
                    "unexpected end of XML inside element '{name}'"
                )));
            }
            if self.starts_with(b"</") {
                self.pos += 2;
                let close_name = self.parse_name()?;
                self.skip_whitespace();
                if !self.starts_with(b">") {
                    return Err(GatewayError::TransformationError(format!(
                        "malformed closing tag for '{close_name}'"
                    )));
                }
                self.pos += 1;
                if close_name != name {
                    return Err(GatewayError::TransformationError(format!(
                        "mismatched closing tag: expected '</{name}>', found '</{close_name}>'"
                    )));
                }
                break;
            } else if self.starts_with(b"<!--") {
                self.consume_until(b"-->")?;
            } else if self.starts_with(b"<![CDATA[") {
                self.pos += b"<![CDATA[".len();
                let start = self.pos;
                let end = self.find(b"]]>").ok_or_else(|| {
                    GatewayError::TransformationError("unterminated CDATA section".to_string())
                })?;
                text.push_str(&String::from_utf8_lossy(&self.bytes[start..end]));
                self.pos = end + 3;
            } else if self.starts_with(b"<") {
                let (child_name, child_value) = self.parse_element()?;
                children.push((child_name, child_value));
            } else {
                // Text run up to the next '<'.
                let start = self.pos;
                while self.pos < self.bytes.len() && self.bytes[self.pos] != b'<' {
                    self.pos += 1;
                }
                let raw = String::from_utf8_lossy(&self.bytes[start..self.pos]);
                text.push_str(&Self::decode_entities(&raw));
            }
        }

        Ok((name, Self::build_value(attributes, children, text)))
    }

    /// Builds the JSON value for an element from its attributes, children and text.
    fn build_value(
        attributes: Vec<(String, String)>,
        children: Vec<(String, serde_json::Value)>,
        text: String,
    ) -> serde_json::Value {
        let trimmed = text.trim();

        if attributes.is_empty() && children.is_empty() {
            return serde_json::Value::String(trimmed.to_string());
        }

        let mut map = serde_json::Map::new();

        for (name, value) in attributes {
            map.insert(format!("@{name}"), serde_json::Value::String(value));
        }

        // Group children by tag, collapsing repeats into arrays (order preserved).
        for (child_name, child_value) in children {
            match map.get_mut(&child_name) {
                Some(serde_json::Value::Array(arr)) => arr.push(child_value),
                Some(existing) => {
                    let previous = existing.take();
                    map.insert(
                        child_name,
                        serde_json::Value::Array(vec![previous, child_value]),
                    );
                }
                None => {
                    map.insert(child_name, child_value);
                }
            }
        }

        if !trimmed.is_empty() {
            map.insert(
                "#text".to_string(),
                serde_json::Value::String(trimmed.to_string()),
            );
        }

        serde_json::Value::Object(map)
    }

    fn parse_name(&mut self) -> Result<String> {
        let start = self.pos;
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.' || b == b':' {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == start {
            return Err(GatewayError::TransformationError(
                "expected an XML name".to_string(),
            ));
        }
        Ok(String::from_utf8_lossy(&self.bytes[start..self.pos]).to_string())
    }

    fn parse_quoted(&mut self) -> Result<String> {
        if self.pos >= self.bytes.len() {
            return Err(GatewayError::TransformationError(
                "expected quoted attribute value".to_string(),
            ));
        }
        let quote = self.bytes[self.pos];
        if quote != b'"' && quote != b'\'' {
            return Err(GatewayError::TransformationError(
                "attribute value must be quoted".to_string(),
            ));
        }
        self.pos += 1;
        let start = self.pos;
        while self.pos < self.bytes.len() && self.bytes[self.pos] != quote {
            self.pos += 1;
        }
        if self.pos >= self.bytes.len() {
            return Err(GatewayError::TransformationError(
                "unterminated attribute value".to_string(),
            ));
        }
        let raw = String::from_utf8_lossy(&self.bytes[start..self.pos]);
        self.pos += 1; // consume closing quote
        Ok(Self::decode_entities(&raw))
    }

    fn decode_entities(input: &str) -> String {
        if !input.contains('&') {
            return input.to_string();
        }
        input
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&")
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn starts_with(&self, needle: &[u8]) -> bool {
        self.bytes[self.pos..].starts_with(needle)
    }

    fn find(&self, needle: &[u8]) -> Option<usize> {
        self.bytes[self.pos..]
            .windows(needle.len())
            .position(|w| w == needle)
            .map(|p| self.pos + p)
    }

    fn consume_until(&mut self, terminator: &[u8]) -> Result<()> {
        match self.find(terminator) {
            Some(idx) => {
                self.pos = idx + terminator.len();
                Ok(())
            }
            None => Err(GatewayError::TransformationError(format!(
                "unterminated XML section (expected '{}')",
                String::from_utf8_lossy(terminator)
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
    fn test_xml_to_json_real_parse() {
        let adapter = XmlAdapter::new();
        let xml = br#"<person id="7"><name>Alice</name><age>30</age></person>"#;
        let result = adapter
            .convert(xml, ContentType::Xml, ContentType::Json)
            .expect("convert");
        let value: serde_json::Value = serde_json::from_slice(&result).expect("valid json output");

        // Real structural conversion, NOT an opaque {"xml": "..."} wrapper.
        assert!(value.get("xml").is_none());
        let person = value.get("person").expect("person key");
        assert_eq!(person.get("@id").and_then(|v| v.as_str()), Some("7"));
        assert_eq!(person.get("name").and_then(|v| v.as_str()), Some("Alice"));
        assert_eq!(person.get("age").and_then(|v| v.as_str()), Some("30"));
    }

    #[test]
    fn test_xml_to_json_repeated_children_become_array() {
        let adapter = XmlAdapter::new();
        let xml = br#"<list><item>a</item><item>b</item><item>c</item></list>"#;
        let result = adapter
            .convert(xml, ContentType::Xml, ContentType::Json)
            .expect("convert");
        let value: serde_json::Value = serde_json::from_slice(&result).expect("json");
        let items = value
            .get("list")
            .and_then(|l| l.get("item"))
            .and_then(|i| i.as_array())
            .expect("item array");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].as_str(), Some("a"));
        assert_eq!(items[2].as_str(), Some("c"));
    }

    #[test]
    fn test_xml_to_json_self_closing_and_prolog() {
        let adapter = XmlAdapter::new();
        let xml = br#"<?xml version="1.0"?><root><empty/><v>x</v></root>"#;
        let result = adapter
            .convert(xml, ContentType::Xml, ContentType::Json)
            .expect("convert");
        let value: serde_json::Value = serde_json::from_slice(&result).expect("json");
        let root = value.get("root").expect("root");
        assert_eq!(root.get("empty").and_then(|v| v.as_str()), Some(""));
        assert_eq!(root.get("v").and_then(|v| v.as_str()), Some("x"));
    }

    #[test]
    fn test_xml_to_json_malformed_errors() {
        let adapter = XmlAdapter::new();
        // Mismatched close tag must be a hard error, not a silent wrapper.
        let xml = br#"<a><b>text</c></a>"#;
        let result = adapter.convert(xml, ContentType::Xml, ContentType::Json);
        assert!(result.is_err());
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
