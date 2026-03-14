//! STAC Timestamps Extension v1.1.0
//!
//! Provides additional datetime fields: `published`, `expires`, and `unpublished`.
//! <https://github.com/stac-extensions/timestamps>

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::Extension;
use crate::error::{Result, StacError};

/// Schema URI for the Timestamps extension.
pub const SCHEMA_URI: &str = "https://stac-extensions.github.io/timestamps/v1.1.0/schema.json";

/// Timestamps extension properties for a STAC item or collection.
///
/// These fields augment the core `created`/`updated` fields with lifecycle
/// metadata that is useful for publishing pipelines.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TimestampsExtension {
    /// Date and time the item was published (made available to users).
    #[serde(rename = "published", skip_serializing_if = "Option::is_none")]
    pub published: Option<DateTime<Utc>>,

    /// Date and time after which the item should no longer be used.
    #[serde(rename = "expires", skip_serializing_if = "Option::is_none")]
    pub expires: Option<DateTime<Utc>>,

    /// Date and time the item was unpublished (removed from the catalog).
    #[serde(rename = "unpublished", skip_serializing_if = "Option::is_none")]
    pub unpublished: Option<DateTime<Utc>>,
}

impl TimestampsExtension {
    /// Creates a new, empty [`TimestampsExtension`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the `published` timestamp.
    pub fn with_published(mut self, dt: DateTime<Utc>) -> Self {
        self.published = Some(dt);
        self
    }

    /// Sets the `expires` timestamp.
    pub fn with_expires(mut self, dt: DateTime<Utc>) -> Self {
        self.expires = Some(dt);
        self
    }

    /// Sets the `unpublished` timestamp.
    pub fn with_unpublished(mut self, dt: DateTime<Utc>) -> Self {
        self.unpublished = Some(dt);
        self
    }

    /// Returns `true` if the item has expired relative to `now`.
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires.map(|e| now > e).unwrap_or(false)
    }

    /// Returns `true` if the item has been published relative to `now`.
    pub fn is_published(&self, now: DateTime<Utc>) -> bool {
        self.published.map(|p| now >= p).unwrap_or(false)
    }

    /// Validates that `published` ≤ `expires` when both are set.
    pub fn validate(&self) -> Result<()> {
        if let (Some(pub_dt), Some(exp_dt)) = (self.published, self.expires) {
            if pub_dt > exp_dt {
                return Err(StacError::InvalidExtension {
                    extension: "timestamps".to_string(),
                    reason: format!(
                        "published ({}) must not be after expires ({})",
                        pub_dt, exp_dt
                    ),
                });
            }
        }
        Ok(())
    }
}

impl Extension for TimestampsExtension {
    fn schema_uri() -> &'static str {
        SCHEMA_URI
    }

    fn validate(&self) -> Result<()> {
        self.validate()
    }

    fn to_value(&self) -> Result<serde_json::Value> {
        serde_json::to_value(self).map_err(|e| StacError::Serialization(e.to_string()))
    }

    fn from_value(value: &serde_json::Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|e| StacError::Deserialization(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dt(year: i32, month: u32, day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, 0, 0, 0).unwrap()
    }

    #[test]
    fn test_timestamps_extension_new() {
        let ts = TimestampsExtension::new();
        assert!(ts.published.is_none());
        assert!(ts.expires.is_none());
        assert!(ts.unpublished.is_none());
    }

    #[test]
    fn test_timestamps_builder() {
        let pub_dt = dt(2023, 1, 1);
        let exp_dt = dt(2024, 1, 1);
        let ts = TimestampsExtension::new()
            .with_published(pub_dt)
            .with_expires(exp_dt);
        assert_eq!(ts.published, Some(pub_dt));
        assert_eq!(ts.expires, Some(exp_dt));
    }

    #[test]
    fn test_is_expired() {
        let exp = dt(2020, 6, 1);
        let ts = TimestampsExtension::new().with_expires(exp);
        assert!(ts.is_expired(dt(2021, 1, 1)));
        assert!(!ts.is_expired(dt(2019, 1, 1)));
    }

    #[test]
    fn test_is_published() {
        let pub_dt = dt(2023, 3, 1);
        let ts = TimestampsExtension::new().with_published(pub_dt);
        assert!(ts.is_published(dt(2023, 4, 1)));
        assert!(!ts.is_published(dt(2023, 2, 1)));
    }

    #[test]
    fn test_validate_ok() {
        let ts = TimestampsExtension::new()
            .with_published(dt(2023, 1, 1))
            .with_expires(dt(2024, 1, 1));
        assert!(ts.validate().is_ok());
    }

    #[test]
    fn test_validate_published_after_expires_fails() {
        let ts = TimestampsExtension::new()
            .with_published(dt(2025, 1, 1))
            .with_expires(dt(2024, 1, 1));
        assert!(ts.validate().is_err());
    }

    #[test]
    fn test_serialization_roundtrip() {
        let pub_dt = dt(2023, 6, 15);
        let ts = TimestampsExtension::new().with_published(pub_dt);
        let json = serde_json::to_string(&ts).expect("serialize");
        let back: TimestampsExtension = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(ts, back);
    }

    #[test]
    fn test_schema_uri() {
        assert!(TimestampsExtension::schema_uri().contains("timestamps"));
    }
}
