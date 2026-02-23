//! Request transformation utilities.

use super::{ContentType, TransformEngine};
use crate::error::Result;
use std::collections::HashMap;

/// HTTP request representation.
#[derive(Debug, Clone)]
pub struct Request {
    /// Request method
    pub method: String,
    /// Request path
    pub path: String,
    /// Request headers
    pub headers: HashMap<String, String>,
    /// Request body
    pub body: Vec<u8>,
    /// Query parameters
    pub query: HashMap<String, String>,
}

impl Request {
    /// Creates a new request.
    pub fn new(method: String, path: String) -> Self {
        Self {
            method,
            path,
            headers: HashMap::new(),
            body: Vec::new(),
            query: HashMap::new(),
        }
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

    /// Gets a query parameter.
    pub fn query_param(&self, name: &str) -> Option<&String> {
        self.query.get(name)
    }

    /// Sets a query parameter.
    pub fn set_query_param(&mut self, name: String, value: String) {
        self.query.insert(name, value);
    }
}

/// Request transformer.
pub struct RequestTransformer {
    engine: TransformEngine,
}

impl RequestTransformer {
    /// Creates a new request transformer.
    pub fn new(engine: TransformEngine) -> Self {
        Self { engine }
    }

    /// Transforms a request.
    pub fn transform(&self, mut request: Request) -> Result<Request> {
        // Transform headers
        self.engine.transform_request_headers(
            &request.path,
            &request.method,
            &mut request.headers,
        )?;

        // Transform body if needed
        if !request.body.is_empty() {
            if let Some(content_type) = request.content_type() {
                request.body = self.engine.transform_request_body(
                    &request.path,
                    &request.method,
                    request.body,
                    content_type,
                )?;
            }
        }

        Ok(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_creation() {
        let req = Request::new("GET".to_string(), "/api/test".to_string());
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/api/test");
        assert!(req.headers.is_empty());
        assert!(req.body.is_empty());
    }

    #[test]
    fn test_content_type() {
        let mut req = Request::new("POST".to_string(), "/api/test".to_string());
        req.set_content_type(ContentType::Json);

        assert_eq!(req.content_type(), Some(ContentType::Json));
        assert_eq!(
            req.header("Content-Type"),
            Some(&"application/json".to_string())
        );
    }

    #[test]
    fn test_headers() {
        let mut req = Request::new("GET".to_string(), "/api/test".to_string());
        req.set_header("X-Custom".to_string(), "value".to_string());

        assert_eq!(req.header("X-Custom"), Some(&"value".to_string()));
    }

    #[test]
    fn test_query_params() {
        let mut req = Request::new("GET".to_string(), "/api/test".to_string());
        req.set_query_param("page".to_string(), "1".to_string());

        assert_eq!(req.query_param("page"), Some(&"1".to_string()));
    }
}
