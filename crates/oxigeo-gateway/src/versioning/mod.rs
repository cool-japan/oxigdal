//! API versioning framework.
//!
//! Provides comprehensive API versioning support with negotiation, migration helpers,
//! deprecation warnings, and backward compatibility.

pub mod deprecation;
pub mod migration;
pub mod negotiation;

use crate::error::{GatewayError, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

pub use deprecation::{DeprecationPolicy, DeprecationWarning};
pub use migration::{MigrationPath, VersionMigrator};
pub use negotiation::{VersionNegotiator, VersionStrategy};

/// API version representation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ApiVersion {
    /// Major version.
    pub major: u32,
    /// Minor version.
    pub minor: u32,
    /// Patch version.
    pub patch: u32,
}

impl ApiVersion {
    /// Creates a new API version.
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Creates a version from major.minor format.
    pub const fn from_major_minor(major: u32, minor: u32) -> Self {
        Self::new(major, minor, 0)
    }

    /// Checks if this version is compatible with another version.
    pub fn is_compatible_with(&self, other: &Self) -> bool {
        self.major == other.major && self.minor >= other.minor
    }

    /// Checks if this is a breaking change from another version.
    pub fn is_breaking_from(&self, other: &Self) -> bool {
        self.major > other.major
    }

    /// Gets the version string in "vX.Y.Z" format.
    pub fn to_string_prefixed(&self) -> String {
        format!("v{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for ApiVersion {
    type Err = GatewayError;

    fn from_str(s: &str) -> Result<Self> {
        let s = s.trim_start_matches('v').trim_start_matches('V');
        let parts: Vec<&str> = s.split('.').collect();

        match parts.as_slice() {
            [major, minor, patch] => Ok(Self::new(
                major.parse().map_err(|_| {
                    GatewayError::InvalidRequest("Invalid major version".to_string())
                })?,
                minor.parse().map_err(|_| {
                    GatewayError::InvalidRequest("Invalid minor version".to_string())
                })?,
                patch.parse().map_err(|_| {
                    GatewayError::InvalidRequest("Invalid patch version".to_string())
                })?,
            )),
            [major, minor] => Ok(Self::from_major_minor(
                major.parse().map_err(|_| {
                    GatewayError::InvalidRequest("Invalid major version".to_string())
                })?,
                minor.parse().map_err(|_| {
                    GatewayError::InvalidRequest("Invalid minor version".to_string())
                })?,
            )),
            [major] => Ok(Self::new(
                major.parse().map_err(|_| {
                    GatewayError::InvalidRequest("Invalid major version".to_string())
                })?,
                0,
                0,
            )),
            _ => Err(GatewayError::InvalidRequest(format!(
                "Invalid version format: {s}"
            ))),
        }
    }
}

impl PartialOrd for ApiVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ApiVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.major.cmp(&other.major) {
            Ordering::Equal => match self.minor.cmp(&other.minor) {
                Ordering::Equal => self.patch.cmp(&other.patch),
                other => other,
            },
            other => other,
        }
    }
}

/// Version registry managing available API versions.
#[derive(Debug, Clone)]
pub struct VersionRegistry {
    versions: Vec<ApiVersion>,
    default_version: ApiVersion,
    latest_version: ApiVersion,
}

impl VersionRegistry {
    /// Creates a new version registry.
    pub fn new(default_version: ApiVersion) -> Self {
        let latest = default_version.clone();
        Self {
            versions: vec![default_version.clone()],
            default_version,
            latest_version: latest,
        }
    }

    /// Adds a version to the registry.
    pub fn add_version(&mut self, version: ApiVersion) {
        if !self.versions.contains(&version) {
            self.versions.push(version.clone());
            self.versions.sort();

            // Update latest version
            if version > self.latest_version {
                self.latest_version = version;
            }
        }
    }

    /// Gets all registered versions.
    pub fn versions(&self) -> &[ApiVersion] {
        &self.versions
    }

    /// Gets the default version.
    pub fn default_version(&self) -> &ApiVersion {
        &self.default_version
    }

    /// Gets the latest version.
    pub fn latest_version(&self) -> &ApiVersion {
        &self.latest_version
    }

    /// Checks if a version is supported.
    pub fn is_supported(&self, version: &ApiVersion) -> bool {
        self.versions.contains(version)
    }

    /// Finds closest supported version.
    pub fn find_closest(&self, requested: &ApiVersion) -> Option<&ApiVersion> {
        self.versions
            .iter()
            .filter(|v| v.major == requested.major)
            .min_by_key(|v| {
                (v.minor as i64 - requested.minor as i64).abs()
                    + (v.patch as i64 - requested.patch as i64).abs()
            })
    }
}

/// Version context for requests.
#[derive(Debug, Clone)]
pub struct VersionContext {
    /// Requested version.
    pub requested: ApiVersion,
    /// Resolved version (may differ from requested).
    pub resolved: ApiVersion,
    /// Whether version was negotiated.
    pub negotiated: bool,
}

impl VersionContext {
    /// Creates a new version context.
    pub fn new(requested: ApiVersion, resolved: ApiVersion) -> Self {
        let negotiated = requested != resolved;
        Self {
            requested,
            resolved,
            negotiated,
        }
    }

    /// Creates context without negotiation.
    pub fn exact(version: ApiVersion) -> Self {
        Self {
            requested: version.clone(),
            resolved: version,
            negotiated: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_version_parsing() {
        let v1 = "1.0.0".parse::<ApiVersion>().ok();
        assert_eq!(v1, Some(ApiVersion::new(1, 0, 0)));

        let v2 = "v2.1.3".parse::<ApiVersion>().ok();
        assert_eq!(v2, Some(ApiVersion::new(2, 1, 3)));

        let v3 = "1.0".parse::<ApiVersion>().ok();
        assert_eq!(v3, Some(ApiVersion::from_major_minor(1, 0)));
    }

    #[test]
    fn test_api_version_comparison() {
        let v1 = ApiVersion::new(1, 0, 0);
        let v2 = ApiVersion::new(1, 1, 0);
        let v3 = ApiVersion::new(2, 0, 0);

        assert!(v1 < v2);
        assert!(v2 < v3);
        assert!(v1 < v3);
    }

    #[test]
    fn test_api_version_compatibility() {
        let v1_0 = ApiVersion::new(1, 0, 0);
        let v1_1 = ApiVersion::new(1, 1, 0);
        let v2_0 = ApiVersion::new(2, 0, 0);

        assert!(v1_1.is_compatible_with(&v1_0));
        assert!(!v1_0.is_compatible_with(&v1_1));
        assert!(!v2_0.is_compatible_with(&v1_0));
    }

    #[test]
    fn test_api_version_breaking() {
        let v1 = ApiVersion::new(1, 0, 0);
        let v2 = ApiVersion::new(2, 0, 0);

        assert!(v2.is_breaking_from(&v1));
        assert!(!v1.is_breaking_from(&v2));
    }

    #[test]
    fn test_version_registry() {
        let mut registry = VersionRegistry::new(ApiVersion::new(1, 0, 0));

        registry.add_version(ApiVersion::new(1, 1, 0));
        registry.add_version(ApiVersion::new(2, 0, 0));

        assert_eq!(registry.versions().len(), 3);
        assert!(registry.is_supported(&ApiVersion::new(1, 0, 0)));
        assert!(!registry.is_supported(&ApiVersion::new(3, 0, 0)));
    }

    #[test]
    fn test_version_registry_closest() {
        let mut registry = VersionRegistry::new(ApiVersion::new(1, 0, 0));
        registry.add_version(ApiVersion::new(1, 2, 0));
        registry.add_version(ApiVersion::new(1, 5, 0));

        let closest = registry.find_closest(&ApiVersion::new(1, 3, 0));
        assert_eq!(closest, Some(&ApiVersion::new(1, 2, 0)));
    }
}
