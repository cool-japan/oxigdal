//! STAC version enumeration and parsing.
//!
//! Provides a typed representation of the STAC specification version, supporting
//! both 1.0.0 and the newer 1.1.0 release (2024-09).

use crate::error::StacError;
use serde::{Deserialize, Serialize};

/// Supported STAC specification versions.
///
/// The STAC 1.1.0 release added `assets` on Collections and the `bands`
/// shorthand on individual assets.  Both versions are accepted by
/// [`Collection::validate`](crate::collection::Collection::validate) and
/// [`Item::validate`](crate::item::Item::validate).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum StacVersion {
    /// STAC specification version 1.0.0.
    #[default]
    V1_0_0,
    /// STAC specification version 1.1.0 (released 2024-09).
    V1_1_0,
}

impl StacVersion {
    /// Returns the canonical version string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::V1_0_0 => "1.0.0",
            Self::V1_1_0 => "1.1.0",
        }
    }

    /// Parses a version string into a [`StacVersion`].
    ///
    /// # Errors
    ///
    /// Returns [`StacError::InvalidVersion`] for any unrecognised version string.
    pub fn parse(s: &str) -> Result<Self, StacError> {
        match s {
            "1.0.0" => Ok(Self::V1_0_0),
            "1.1.0" => Ok(Self::V1_1_0),
            other => Err(StacError::InvalidVersion(other.to_string())),
        }
    }

    /// Returns `true` if the version is at least 1.1.0.
    pub fn is_v1_1_or_later(&self) -> bool {
        matches!(self, Self::V1_1_0)
    }
}

impl std::fmt::Display for StacVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<&str> for StacVersion {
    type Error = StacError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::parse(s)
    }
}

impl TryFrom<String> for StacVersion {
    type Error = StacError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::parse(&s)
    }
}

impl From<StacVersion> for String {
    fn from(v: StacVersion) -> String {
        v.as_str().to_string()
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_accepts_1_0_0() {
        assert_eq!(
            StacVersion::parse("1.0.0").expect("1.0.0 is valid"),
            StacVersion::V1_0_0
        );
    }

    #[test]
    fn test_parse_accepts_1_1_0() {
        assert_eq!(
            StacVersion::parse("1.1.0").expect("1.1.0 is valid"),
            StacVersion::V1_1_0
        );
    }

    #[test]
    fn test_parse_rejects_unknown() {
        assert!(StacVersion::parse("2.0.0").is_err());
    }

    #[test]
    fn test_as_str_roundtrip() {
        assert_eq!(StacVersion::V1_0_0.as_str(), "1.0.0");
        assert_eq!(StacVersion::V1_1_0.as_str(), "1.1.0");
    }

    #[test]
    fn test_default_is_v1_0_0() {
        assert_eq!(StacVersion::default(), StacVersion::V1_0_0);
    }

    #[test]
    fn test_is_v1_1_or_later() {
        assert!(!StacVersion::V1_0_0.is_v1_1_or_later());
        assert!(StacVersion::V1_1_0.is_v1_1_or_later());
    }

    #[test]
    fn test_display() {
        assert_eq!(StacVersion::V1_0_0.to_string(), "1.0.0");
        assert_eq!(StacVersion::V1_1_0.to_string(), "1.1.0");
    }

    #[test]
    fn test_from_stac_version_into_string() {
        let s: String = StacVersion::V1_1_0.into();
        assert_eq!(s, "1.1.0");
    }

    #[test]
    fn test_serde_roundtrip() {
        let v = StacVersion::V1_1_0;
        let json = serde_json::to_string(&v).expect("serialization succeeds");
        let back: StacVersion = serde_json::from_str(&json).expect("deserialization succeeds");
        assert_eq!(v, back);
    }
}
