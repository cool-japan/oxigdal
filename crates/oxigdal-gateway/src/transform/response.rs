//! Response transformation utilities.

use super::{ContentType, TransformEngine};
use crate::error::Result;
use std::collections::HashMap;

/// HTTP response representation.
#[derive(Debug, Clone)]
pub struct Response {
    /// Response status code
    pub status: u16,
    /// Response headers
    pub headers: HashMap<String, String>,
    /// Response body
    pub body: Vec<u8>,
}

impl Response {
    /// Creates a new response.
    pub fn new(status: u16) -> Self {
        Self {
            status,
            headers: HashMap::new(),
            body: Vec::new(),
        }
    }

    /// Creates a successful response.
    pub fn ok() -> Self {
        Self::new(200)
    }

    /// Creates an error response.
    pub fn error(status: u16, message: &str) -> Self {
        let mut response = Self::new(status);
        response.body = message.as_bytes().to_vec();
        response.set_content_type(ContentType::Text);
        response
    }

    /// Gets content type from headers.
    pub fn content_type(&self) -> Option<ContentType> {
        self.headers
            .get("content-type")
            .or_else(|| self.headers.get("Content-Type"))
            .and_then(|ct| ContentType::from_mime(ct))
    }

    /// Sets content type header.
    pub fn set_content_type(&mut self, content_type: ContentType) {
        self.headers.insert(
            "Content-Type".to_string(),
            content_type.to_mime().to_string(),
        );
    }

    /// Gets a header value.
    pub fn header(&self, name: &str) -> Option<&String> {
        self.headers.get(name)
    }

    /// Sets a header value.
    pub fn set_header(&mut self, name: String, value: String) {
        self.headers.insert(name, value);
    }

    /// Sets JSON body.
    pub fn with_json(mut self, value: &serde_json::Value) -> Result<Self> {
        self.body = serde_json::to_vec(value)?;
        self.set_content_type(ContentType::Json);
        Ok(self)
    }

    /// Sets text body.
    pub fn with_text(mut self, text: &str) -> Self {
        self.body = text.as_bytes().to_vec();
        self.set_content_type(ContentType::Text);
        self
    }

    /// Sets binary body.
    pub fn with_binary(mut self, data: Vec<u8>) -> Self {
        self.body = data;
        self.set_content_type(ContentType::Binary);
        self
    }
}

/// Response transformer.
pub struct ResponseTransformer {
    engine: TransformEngine,
}

impl ResponseTransformer {
    /// Creates a new response transformer.
    pub fn new(engine: TransformEngine) -> Self {
        Self { engine }
    }

    /// Transforms a response.
    pub fn transform(&self, response: Response, path: &str, method: &str) -> Result<Response> {
        let mut transformed = response;

        // Transform headers
        self.engine
            .transform_request_headers(path, method, &mut transformed.headers)?;

        // Transform body if needed
        if !transformed.body.is_empty() {
            if let Some(content_type) = transformed.content_type() {
                transformed.body = self.engine.transform_request_body(
                    path,
                    method,
                    transformed.body,
                    content_type,
                )?;
            }
        }

        Ok(transformed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_creation() {
        let resp = Response::new(200);
        assert_eq!(resp.status, 200);
        assert!(resp.headers.is_empty());
        assert!(resp.body.is_empty());
    }

    #[test]
    fn test_response_ok() {
        let resp = Response::ok();
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn test_response_error() {
        let resp = Response::error(500, "Internal error");
        assert_eq!(resp.status, 500);
        assert_eq!(String::from_utf8_lossy(&resp.body), "Internal error");
    }

    #[test]
    fn test_content_type() {
        let mut resp = Response::new(200);
        resp.set_content_type(ContentType::Json);

        assert_eq!(resp.content_type(), Some(ContentType::Json));
        assert_eq!(
            resp.header("Content-Type"),
            Some(&"application/json".to_string())
        );
    }

    #[test]
    fn test_with_json() {
        let resp = Response::ok();
        let json = serde_json::json!({"status": "ok"});
        let result = resp.with_json(&json);

        assert!(result.is_ok());
        let resp = result.ok();
        assert!(resp.is_some());
        let resp = resp.unwrap_or(Response::new(500));
        assert_eq!(resp.content_type(), Some(ContentType::Json));
        assert!(!resp.body.is_empty());
    }

    #[test]
    fn test_with_text() {
        let resp = Response::ok().with_text("Hello, world!");
        assert_eq!(resp.content_type(), Some(ContentType::Text));
        assert_eq!(String::from_utf8_lossy(&resp.body), "Hello, world!");
    }

    #[test]
    fn test_with_binary() {
        let data = vec![1, 2, 3, 4, 5];
        let resp = Response::ok().with_binary(data.clone());
        assert_eq!(resp.content_type(), Some(ContentType::Binary));
        assert_eq!(resp.body, data);
    }
}
