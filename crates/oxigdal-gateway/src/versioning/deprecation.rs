//! API deprecation policies and warnings.
//!
//! Manages version deprecation with sunset dates and warning headers.

use crate::versioning::ApiVersion;
use chrono::{DateTime, Utc};
use http::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Deprecation policy for an API version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeprecationPolicy {
    /// Version being deprecated.
    pub version: ApiVersion,
    /// Deprecation announcement date.
    pub deprecated_since: DateTime<Utc>,
    /// Sunset date (when version will be removed).
    pub sunset_date: Option<DateTime<Utc>>,
    /// Replacement version.
    pub replacement: Option<ApiVersion>,
    /// Deprecation message.
    pub message: String,
    /// Documentation URL.
    pub documentation_url: Option<String>,
}

impl DeprecationPolicy {
    /// Creates a new deprecation policy.
    pub fn new(version: ApiVersion, message: impl Into<String>) -> Self {
        Self {
            version,
            deprecated_since: Utc::now(),
            sunset_date: None,
            replacement: None,
            message: message.into(),
            documentation_url: None,
        }
    }

    /// Sets the sunset date.
    pub fn with_sunset_date(mut self, sunset_date: DateTime<Utc>) -> Self {
        self.sunset_date = Some(sunset_date);
        self
    }

    /// Sets the replacement version.
    pub fn with_replacement(mut self, replacement: ApiVersion) -> Self {
        self.replacement = Some(replacement);
        self
    }

    /// Sets the documentation URL.
    pub fn with_documentation(mut self, url: impl Into<String>) -> Self {
        self.documentation_url = Some(url.into());
        self
    }

    /// Checks if version is past sunset date.
    pub fn is_sunset(&self) -> bool {
        if let Some(sunset) = self.sunset_date {
            Utc::now() >= sunset
        } else {
            false
        }
    }

    /// Gets days until sunset.
    pub fn days_until_sunset(&self) -> Option<i64> {
        self.sunset_date.map(|sunset| {
            let duration = sunset.signed_duration_since(Utc::now());
            duration.num_days()
        })
    }

    /// Creates deprecation warning header.
    pub fn to_warning_header(&self) -> HeaderValue {
        let mut warning = format!("299 - \"API version {} is deprecated", self.version);

        if let Some(ref replacement) = self.replacement {
            warning.push_str(&format!(", use version {} instead", replacement));
        }

        if let Some(sunset) = self.sunset_date {
            warning.push_str(&format!(", sunset date: {}", sunset.format("%Y-%m-%d")));
        }

        warning.push('"');

        HeaderValue::from_str(&warning)
            .unwrap_or_else(|_| HeaderValue::from_static("299 - \"Deprecated\""))
    }

    /// Creates Sunset header (RFC 8594).
    pub fn to_sunset_header(&self) -> Option<HeaderValue> {
        self.sunset_date.map(|sunset| {
            HeaderValue::from_str(&sunset.to_rfc2822())
                .unwrap_or_else(|_| HeaderValue::from_static(""))
        })
    }
}

/// Deprecation warning to be included in responses.
#[derive(Debug, Clone)]
pub struct DeprecationWarning {
    /// Warning message.
    pub message: String,
    /// Sunset date if applicable.
    pub sunset_date: Option<DateTime<Utc>>,
    /// Documentation link.
    pub link: Option<String>,
}

impl DeprecationWarning {
    /// Creates HTTP headers for the warning.
    pub fn to_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();

        // Warning header
        if let Ok(value) = HeaderValue::from_str(&format!("299 - \"{}\"", self.message)) {
            headers.insert("Warning", value);
        }

        // Sunset header
        if let Some(sunset) = self.sunset_date {
            if let Ok(value) = HeaderValue::from_str(&sunset.to_rfc2822()) {
                headers.insert("Sunset", value);
            }
        }

        // Link header for documentation
        if let Some(ref link) = self.link {
            if let Ok(value) = HeaderValue::from_str(&format!("<{}>; rel=\"deprecation\"", link)) {
                headers.insert("Link", value);
            }
        }

        headers
    }
}

/// Deprecation manager tracking deprecated versions.
pub struct DeprecationManager {
    policies: HashMap<ApiVersion, DeprecationPolicy>,
}

impl DeprecationManager {
    /// Creates a new deprecation manager.
    pub fn new() -> Self {
        Self {
            policies: HashMap::new(),
        }
    }

    /// Adds a deprecation policy.
    pub fn add_policy(&mut self, policy: DeprecationPolicy) {
        self.policies.insert(policy.version.clone(), policy);
    }

    /// Gets deprecation policy for a version.
    pub fn get_policy(&self, version: &ApiVersion) -> Option<&DeprecationPolicy> {
        self.policies.get(version)
    }

    /// Checks if a version is deprecated.
    pub fn is_deprecated(&self, version: &ApiVersion) -> bool {
        self.policies.contains_key(version)
    }

    /// Checks if a version is sunset.
    pub fn is_sunset(&self, version: &ApiVersion) -> bool {
        self.policies
            .get(version)
            .map(|p| p.is_sunset())
            .unwrap_or(false)
    }

    /// Creates warning for a version.
    pub fn create_warning(&self, version: &ApiVersion) -> Option<DeprecationWarning> {
        self.policies.get(version).map(|policy| DeprecationWarning {
            message: policy.message.clone(),
            sunset_date: policy.sunset_date,
            link: policy.documentation_url.clone(),
        })
    }

    /// Gets all deprecated versions.
    pub fn deprecated_versions(&self) -> Vec<&ApiVersion> {
        self.policies.keys().collect()
    }

    /// Gets all sunset versions.
    pub fn sunset_versions(&self) -> Vec<&ApiVersion> {
        self.policies
            .values()
            .filter(|p| p.is_sunset())
            .map(|p| &p.version)
            .collect()
    }
}

impl Default for DeprecationManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_deprecation_policy() {
        let version = ApiVersion::new(1, 0, 0);
        let policy = DeprecationPolicy::new(version.clone(), "Version 1.0 is deprecated")
            .with_replacement(ApiVersion::new(2, 0, 0))
            .with_sunset_date(Utc::now() + Duration::days(90));

        assert_eq!(policy.version, version);
        assert!(policy.replacement.is_some());
        assert!(policy.sunset_date.is_some());
        assert!(!policy.is_sunset());
    }

    #[test]
    fn test_days_until_sunset() {
        let version = ApiVersion::new(1, 0, 0);
        let sunset = Utc::now() + Duration::days(30);
        let policy = DeprecationPolicy::new(version, "Deprecated").with_sunset_date(sunset);

        let days = policy.days_until_sunset();
        assert!(days.is_some());
        // Should be approximately 30 days
        assert!((days.unwrap_or(0) - 30).abs() <= 1);
    }

    #[test]
    fn test_deprecation_manager() {
        let mut manager = DeprecationManager::new();

        let v1 = ApiVersion::new(1, 0, 0);
        let policy = DeprecationPolicy::new(v1.clone(), "Version 1.0 is deprecated");
        manager.add_policy(policy);

        assert!(manager.is_deprecated(&v1));
        assert!(!manager.is_deprecated(&ApiVersion::new(2, 0, 0)));
    }

    #[test]
    fn test_deprecation_warning_headers() {
        let warning = DeprecationWarning {
            message: "This version is deprecated".to_string(),
            sunset_date: Some(Utc::now() + Duration::days(90)),
            link: Some("https://api.example.com/docs/deprecation".to_string()),
        };

        let headers = warning.to_headers();
        assert!(headers.contains_key("Warning"));
        assert!(headers.contains_key("Sunset"));
        assert!(headers.contains_key("Link"));
    }

    #[test]
    fn test_sunset_versions() {
        let mut manager = DeprecationManager::new();

        let v1 = ApiVersion::new(1, 0, 0);
        let past_sunset = DeprecationPolicy::new(v1.clone(), "Sunset")
            .with_sunset_date(Utc::now() - Duration::days(1));
        manager.add_policy(past_sunset);

        let v2 = ApiVersion::new(2, 0, 0);
        let future_sunset = DeprecationPolicy::new(v2.clone(), "Not yet sunset")
            .with_sunset_date(Utc::now() + Duration::days(30));
        manager.add_policy(future_sunset);

        assert!(manager.is_sunset(&v1));
        assert!(!manager.is_sunset(&v2));

        let sunset = manager.sunset_versions();
        assert_eq!(sunset.len(), 1);
        assert_eq!(sunset[0], &v1);
    }
}
