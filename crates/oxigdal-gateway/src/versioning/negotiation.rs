//! Version negotiation strategies.
//!
//! Handles content negotiation for API versioning through headers, URL paths, and query parameters.

use crate::error::{GatewayError, Result};
use crate::versioning::{ApiVersion, VersionContext, VersionRegistry};
use http::{HeaderMap, HeaderValue};

/// Version negotiation strategy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VersionStrategy {
    /// Version in URL path (e.g., /v1/resource).
    UrlPath,
    /// Version in Accept header (e.g., Accept: application/vnd.api+json;version=1).
    AcceptHeader,
    /// Version in custom header (e.g., API-Version: 1.0).
    CustomHeader,
    /// Version in query parameter (e.g., ?version=1.0).
    QueryParameter,
}

/// Version negotiator.
pub struct VersionNegotiator {
    registry: VersionRegistry,
    strategy: VersionStrategy,
    header_name: String,
    query_param: String,
}

impl VersionNegotiator {
    /// Creates a new version negotiator.
    pub fn new(registry: VersionRegistry, strategy: VersionStrategy) -> Self {
        Self {
            registry,
            strategy,
            header_name: "API-Version".to_string(),
            query_param: "version".to_string(),
        }
    }

    /// Sets custom header name.
    pub fn with_header_name(mut self, name: impl Into<String>) -> Self {
        self.header_name = name.into();
        self
    }

    /// Sets custom query parameter name.
    pub fn with_query_param(mut self, param: impl Into<String>) -> Self {
        self.query_param = param.into();
        self
    }

    /// Negotiates version from HTTP headers.
    pub fn negotiate_from_headers(&self, headers: &HeaderMap) -> Result<VersionContext> {
        match self.strategy {
            VersionStrategy::AcceptHeader => self.negotiate_from_accept_header(headers),
            VersionStrategy::CustomHeader => self.negotiate_from_custom_header(headers),
            _ => Ok(VersionContext::exact(
                self.registry.default_version().clone(),
            )),
        }
    }

    /// Negotiates version from URL path.
    pub fn negotiate_from_path(&self, path: &str) -> Result<VersionContext> {
        if self.strategy != VersionStrategy::UrlPath {
            return Ok(VersionContext::exact(
                self.registry.default_version().clone(),
            ));
        }

        // Extract version from path like "/v1/resource" or "/api/v2/resource"
        let parts: Vec<&str> = path.split('/').collect();

        for part in parts {
            if part.starts_with('v') || part.starts_with('V') {
                if let Ok(version) = part.parse::<ApiVersion>() {
                    return self.resolve_version(version);
                }
            }
        }

        Ok(VersionContext::exact(
            self.registry.default_version().clone(),
        ))
    }

    /// Negotiates version from query parameters.
    pub fn negotiate_from_query(&self, query: &str) -> Result<VersionContext> {
        if self.strategy != VersionStrategy::QueryParameter {
            return Ok(VersionContext::exact(
                self.registry.default_version().clone(),
            ));
        }

        let params: Vec<(&str, &str)> = query
            .split('&')
            .filter_map(|pair| {
                let mut parts = pair.split('=');
                Some((parts.next()?, parts.next()?))
            })
            .collect();

        for (key, value) in params {
            if key == self.query_param {
                if let Ok(version) = value.parse::<ApiVersion>() {
                    return self.resolve_version(version);
                }
            }
        }

        Ok(VersionContext::exact(
            self.registry.default_version().clone(),
        ))
    }

    fn negotiate_from_accept_header(&self, headers: &HeaderMap) -> Result<VersionContext> {
        let accept = headers
            .get(http::header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        // Parse Accept header like "application/vnd.api+json;version=1.0"
        if let Some(version_part) = accept
            .split(';')
            .find(|part| part.trim().starts_with("version="))
        {
            let version_str = version_part.trim().trim_start_matches("version=");
            if let Ok(version) = version_str.parse::<ApiVersion>() {
                return self.resolve_version(version);
            }
        }

        Ok(VersionContext::exact(
            self.registry.default_version().clone(),
        ))
    }

    fn negotiate_from_custom_header(&self, headers: &HeaderMap) -> Result<VersionContext> {
        let version_str = headers
            .get(&self.header_name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if version_str.is_empty() {
            return Ok(VersionContext::exact(
                self.registry.default_version().clone(),
            ));
        }

        let version = version_str.parse::<ApiVersion>()?;
        self.resolve_version(version)
    }

    fn resolve_version(&self, requested: ApiVersion) -> Result<VersionContext> {
        if self.registry.is_supported(&requested) {
            return Ok(VersionContext::exact(requested));
        }

        // Try to find closest compatible version
        if let Some(closest) = self.registry.find_closest(&requested) {
            return Ok(VersionContext::new(requested, closest.clone()));
        }

        Err(GatewayError::UnsupportedVersion {
            version: requested.to_string(),
            supported: self
                .registry
                .versions()
                .iter()
                .map(|v| v.to_string())
                .collect(),
        })
    }

    /// Creates version response headers.
    pub fn create_response_headers(&self, context: &VersionContext) -> HeaderMap {
        let mut headers = HeaderMap::new();

        // Add resolved version header
        if let Ok(value) = HeaderValue::from_str(&context.resolved.to_string()) {
            if let Ok(header_name) = http::HeaderName::from_bytes(self.header_name.as_bytes()) {
                headers.insert(header_name, value);
            }
        }

        // Add deprecation warning if negotiated
        if context.negotiated {
            if let Ok(value) = HeaderValue::from_str(&format!(
                "Requested version {} not available, using {}",
                context.requested, context.resolved
            )) {
                headers.insert("Warning", value);
            }
        }

        headers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_registry() -> VersionRegistry {
        let mut registry = VersionRegistry::new(ApiVersion::new(1, 0, 0));
        registry.add_version(ApiVersion::new(1, 1, 0));
        registry.add_version(ApiVersion::new(2, 0, 0));
        registry
    }

    #[test]
    fn test_negotiate_from_path() {
        let registry = create_test_registry();
        let negotiator = VersionNegotiator::new(registry, VersionStrategy::UrlPath);

        let context = negotiator.negotiate_from_path("/v1/resource").ok();
        assert!(context.is_some());
        assert_eq!(
            context.as_ref().map(|c| &c.resolved),
            Some(&ApiVersion::new(1, 0, 0))
        );

        let context2 = negotiator.negotiate_from_path("/api/v2/resource").ok();
        assert!(context2.is_some());
        assert_eq!(
            context2.as_ref().map(|c| &c.resolved),
            Some(&ApiVersion::new(2, 0, 0))
        );
    }

    #[test]
    fn test_negotiate_from_query() {
        let registry = create_test_registry();
        let negotiator = VersionNegotiator::new(registry, VersionStrategy::QueryParameter);

        let context = negotiator.negotiate_from_query("version=1.0.0").ok();
        assert!(context.is_some());
        assert_eq!(
            context.as_ref().map(|c| &c.resolved),
            Some(&ApiVersion::new(1, 0, 0))
        );
    }

    #[test]
    fn test_negotiate_from_custom_header() {
        let registry = create_test_registry();
        let negotiator = VersionNegotiator::new(registry, VersionStrategy::CustomHeader);

        let mut headers = HeaderMap::new();
        headers.insert("API-Version", HeaderValue::from_static("1.1.0"));

        let context = negotiator.negotiate_from_headers(&headers).ok();
        assert!(context.is_some());
        assert_eq!(
            context.as_ref().map(|c| &c.resolved),
            Some(&ApiVersion::new(1, 1, 0))
        );
    }

    #[test]
    fn test_unsupported_version() {
        let registry = create_test_registry();
        let negotiator = VersionNegotiator::new(registry, VersionStrategy::UrlPath);

        let result = negotiator.negotiate_from_path("/v5/resource");
        assert!(result.is_err());
    }
}
