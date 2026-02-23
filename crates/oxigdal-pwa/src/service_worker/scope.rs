//! Service worker scope management and utilities.

use crate::error::{PwaError, Result};
use regex::Regex;
use url::Url;

/// Service worker scope manager.
pub struct ServiceWorkerScope {
    base_url: Url,
    scope_path: String,
}

impl ServiceWorkerScope {
    /// Create a new scope with a base URL and scope path.
    pub fn new(base_url: &str, scope_path: &str) -> Result<Self> {
        let base_url = Url::parse(base_url)?;
        Ok(Self {
            base_url,
            scope_path: scope_path.to_string(),
        })
    }

    /// Create a scope from the current window location.
    pub fn from_window() -> Result<Self> {
        let window = web_sys::window()
            .ok_or_else(|| PwaError::InvalidState("No window available".to_string()))?;

        let location = window.location();
        let href = location
            .href()
            .map_err(|_| PwaError::InvalidState("Failed to get window location".to_string()))?;

        Self::new(&href, "/")
    }

    /// Get the full scope URL.
    pub fn scope_url(&self) -> String {
        let mut url = self.base_url.clone();
        url.set_path(&self.scope_path);
        url.to_string()
    }

    /// Check if a URL is within this scope.
    pub fn is_in_scope(&self, url: &str) -> Result<bool> {
        let target_url = Url::parse(url)?;

        // Check if same origin
        if target_url.origin() != self.base_url.origin() {
            return Ok(false);
        }

        // Check if path starts with scope path
        let scope_path = self.normalize_path(&self.scope_path);
        let target_path = self.normalize_path(target_url.path());

        Ok(target_path.starts_with(&scope_path))
    }

    /// Normalize a path by ensuring it starts with / and ends without /
    fn normalize_path(&self, path: &str) -> String {
        let mut normalized = path.trim().to_string();

        // Ensure starts with /
        if !normalized.starts_with('/') {
            normalized.insert(0, '/');
        }

        // Remove trailing / unless it's the root
        if normalized.len() > 1 && normalized.ends_with('/') {
            normalized.pop();
        }

        normalized
    }

    /// Get the scope path.
    pub fn path(&self) -> &str {
        &self.scope_path
    }

    /// Get the base URL.
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// Set a new scope path.
    pub fn set_scope_path(&mut self, path: &str) {
        self.scope_path = self.normalize_path(path);
    }
}

/// Scope pattern matcher for advanced scope matching.
pub struct ScopePattern {
    pattern: Regex,
}

impl ScopePattern {
    /// Create a new scope pattern from a glob-like pattern.
    pub fn new(pattern: &str) -> Result<Self> {
        // Convert glob pattern to regex
        let regex_pattern = Self::glob_to_regex(pattern);
        let regex = Regex::new(&regex_pattern)
            .map_err(|e| PwaError::InvalidCacheStrategy(format!("Invalid pattern: {}", e)))?;

        Ok(Self { pattern: regex })
    }

    /// Check if a URL matches this pattern.
    pub fn matches(&self, url: &str) -> bool {
        self.pattern.is_match(url)
    }

    /// Convert a glob pattern to a regex pattern.
    fn glob_to_regex(glob: &str) -> String {
        let mut regex = String::from("^");

        for ch in glob.chars() {
            match ch {
                '*' => regex.push_str(".*"),
                '?' => regex.push('.'),
                '.' => regex.push_str("\\."),
                '/' => regex.push_str("\\/"),
                _ if ch.is_alphanumeric() || ch == '_' || ch == '-' => regex.push(ch),
                _ => {
                    regex.push('\\');
                    regex.push(ch);
                }
            }
        }

        regex.push('$');
        regex
    }
}

/// Scope matcher with multiple patterns.
pub struct ScopeMatcher {
    patterns: Vec<ScopePattern>,
}

impl ScopeMatcher {
    /// Create a new scope matcher.
    pub fn new() -> Self {
        Self {
            patterns: Vec::new(),
        }
    }

    /// Add a pattern to the matcher.
    pub fn add_pattern(&mut self, pattern: &str) -> Result<()> {
        let scope_pattern = ScopePattern::new(pattern)?;
        self.patterns.push(scope_pattern);
        Ok(())
    }

    /// Check if any pattern matches the URL.
    pub fn matches_any(&self, url: &str) -> bool {
        self.patterns.iter().any(|p| p.matches(url))
    }

    /// Check if all patterns match the URL.
    pub fn matches_all(&self, url: &str) -> bool {
        !self.patterns.is_empty() && self.patterns.iter().all(|p| p.matches(url))
    }

    /// Get the number of patterns.
    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }
}

impl Default for ScopeMatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if a URL is same-origin with the current page.
pub fn is_same_origin(url: &str) -> Result<bool> {
    let window = web_sys::window()
        .ok_or_else(|| PwaError::InvalidState("No window available".to_string()))?;

    let location = window.location();
    let origin = location
        .origin()
        .map_err(|_| PwaError::InvalidState("Failed to get origin".to_string()))?;

    let target_url = Url::parse(url)?;
    Ok(target_url.origin().ascii_serialization() == origin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_creation() -> Result<()> {
        let scope = ServiceWorkerScope::new("https://example.com", "/app")?;
        assert_eq!(scope.path(), "/app");
        assert_eq!(scope.scope_url(), "https://example.com/app");
        Ok(())
    }

    #[test]
    fn test_scope_is_in_scope() -> Result<()> {
        let scope = ServiceWorkerScope::new("https://example.com", "/app")?;

        assert!(scope.is_in_scope("https://example.com/app/page")?);
        assert!(scope.is_in_scope("https://example.com/app")?);
        assert!(!scope.is_in_scope("https://example.com/other")?);
        assert!(!scope.is_in_scope("https://other.com/app")?);

        Ok(())
    }

    #[test]
    fn test_path_normalization() {
        let scope = ServiceWorkerScope::new("https://example.com", "/app/")
            .expect("Failed to create scope");

        let normalized = scope.normalize_path("/app/");
        assert_eq!(normalized, "/app");

        let normalized = scope.normalize_path("app");
        assert_eq!(normalized, "/app");
    }

    #[test]
    fn test_scope_pattern() -> Result<()> {
        let pattern = ScopePattern::new("https://example.com/api/*")?;

        assert!(pattern.matches("https://example.com/api/users"));
        assert!(pattern.matches("https://example.com/api/posts"));
        assert!(!pattern.matches("https://example.com/app"));

        Ok(())
    }

    #[test]
    fn test_scope_matcher() -> Result<()> {
        let mut matcher = ScopeMatcher::new();
        matcher.add_pattern("https://example.com/api/*")?;
        matcher.add_pattern("https://example.com/static/*")?;

        assert!(matcher.matches_any("https://example.com/api/users"));
        assert!(matcher.matches_any("https://example.com/static/css/style.css"));
        assert!(!matcher.matches_any("https://example.com/app"));
        assert_eq!(matcher.pattern_count(), 2);

        Ok(())
    }

    #[test]
    fn test_glob_to_regex() {
        let regex = ScopePattern::glob_to_regex("/api/*/users");
        assert!(regex.contains(".*"));
        assert!(regex.starts_with('^'));
        assert!(regex.ends_with('$'));
    }
}
