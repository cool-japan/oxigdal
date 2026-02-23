//! STAC Item representation.

use crate::{
    asset::Asset,
    error::{Result, StacError},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A STAC Item is a GeoJSON Feature with additional STAC-specific properties.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Item {
    /// Type must be "Feature" for GeoJSON compliance.
    #[serde(rename = "type")]
    pub type_: String,

    /// STAC version.
    #[serde(rename = "stac_version")]
    pub stac_version: String,

    /// List of extensions used in the item.
    #[serde(skip_serializing_if = "Option::is_none", rename = "stac_extensions")]
    pub stac_extensions: Option<Vec<String>>,

    /// Unique identifier for the item.
    pub id: String,

    /// Geometry of the item (GeoJSON geometry object).
    pub geometry: Option<geojson::Geometry>,

    /// Bounding box of the asset [west, south, east, north].
    pub bbox: Option<Vec<f64>>,

    /// Item properties.
    pub properties: ItemProperties,

    /// Links to other resources.
    pub links: Vec<Link>,

    /// Assets associated with this item.
    pub assets: HashMap<String, Asset>,

    /// Collection ID this item belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,
}

/// Properties of a STAC Item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ItemProperties {
    /// Date and time of the asset, in UTC.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datetime: Option<DateTime<Utc>>,

    /// Start date and time of the asset, in UTC.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_datetime: Option<DateTime<Utc>>,

    /// End date and time of the asset, in UTC.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_datetime: Option<DateTime<Utc>>,

    /// Creation date and time of the item, in UTC.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<DateTime<Utc>>,

    /// Last update date and time of the item, in UTC.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<DateTime<Utc>>,

    /// Title of the item.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Description of the item.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Additional properties (including extension properties).
    #[serde(flatten)]
    pub additional_fields: HashMap<String, serde_json::Value>,
}

/// A link to another resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Link {
    /// URI to the linked resource.
    pub href: String,

    /// Relationship type.
    pub rel: String,

    /// Media type of the linked resource.
    #[serde(skip_serializing_if = "Option::is_none", rename = "type")]
    pub media_type: Option<String>,

    /// Title for the link.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Additional fields.
    #[serde(flatten)]
    pub additional_fields: HashMap<String, serde_json::Value>,
}

impl Item {
    /// Creates a new STAC Item.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique identifier for the item
    ///
    /// # Returns
    ///
    /// A new Item instance with default STAC version 1.0.0
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            type_: "Feature".to_string(),
            stac_version: "1.0.0".to_string(),
            stac_extensions: None,
            id: id.into(),
            geometry: None,
            bbox: None,
            properties: ItemProperties {
                datetime: None,
                start_datetime: None,
                end_datetime: None,
                created: None,
                updated: None,
                title: None,
                description: None,
                additional_fields: HashMap::new(),
            },
            links: Vec::new(),
            assets: HashMap::new(),
            collection: None,
        }
    }

    /// Sets the geometry of the item.
    ///
    /// # Arguments
    ///
    /// * `geometry` - GeoJSON geometry
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_geometry(mut self, geometry: geojson::Geometry) -> Self {
        self.geometry = Some(geometry);
        self
    }

    /// Sets the bounding box of the item.
    ///
    /// # Arguments
    ///
    /// * `bbox` - Bounding box [west, south, east, north]
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_bbox(mut self, bbox: Vec<f64>) -> Self {
        self.bbox = Some(bbox);
        self
    }

    /// Sets the datetime of the item.
    ///
    /// # Arguments
    ///
    /// * `datetime` - Date and time in UTC
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_datetime(mut self, datetime: DateTime<Utc>) -> Self {
        self.properties.datetime = Some(datetime);
        self
    }

    /// Sets the datetime range of the item.
    ///
    /// # Arguments
    ///
    /// * `start` - Start date and time in UTC
    /// * `end` - End date and time in UTC
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_datetime_range(mut self, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        self.properties.datetime = None;
        self.properties.start_datetime = Some(start);
        self.properties.end_datetime = Some(end);
        self
    }

    /// Adds an asset to the item.
    ///
    /// # Arguments
    ///
    /// * `key` - Asset key
    /// * `asset` - Asset to add
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn add_asset(mut self, key: impl Into<String>, asset: Asset) -> Self {
        self.assets.insert(key.into(), asset);
        self
    }

    /// Adds a link to the item.
    ///
    /// # Arguments
    ///
    /// * `link` - Link to add
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn add_link(mut self, link: Link) -> Self {
        self.links.push(link);
        self
    }

    /// Sets the collection ID.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - Collection identifier
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_collection(mut self, collection_id: impl Into<String>) -> Self {
        self.collection = Some(collection_id.into());
        self
    }

    /// Adds a STAC extension.
    ///
    /// # Arguments
    ///
    /// * `extension` - Extension schema URI
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn add_extension(mut self, extension: impl Into<String>) -> Self {
        match &mut self.stac_extensions {
            Some(extensions) => extensions.push(extension.into()),
            None => self.stac_extensions = Some(vec![extension.into()]),
        }
        self
    }

    /// Gets an asset by key.
    ///
    /// # Arguments
    ///
    /// * `key` - Asset key
    ///
    /// # Returns
    ///
    /// Reference to the asset if it exists
    pub fn get_asset(&self, key: &str) -> Option<&Asset> {
        self.assets.get(key)
    }

    /// Gets a mutable reference to an asset by key.
    ///
    /// # Arguments
    ///
    /// * `key` - Asset key
    ///
    /// # Returns
    ///
    /// Mutable reference to the asset if it exists
    pub fn get_asset_mut(&mut self, key: &str) -> Option<&mut Asset> {
        self.assets.get_mut(key)
    }

    /// Finds links by relationship type.
    ///
    /// # Arguments
    ///
    /// * `rel` - Relationship type
    ///
    /// # Returns
    ///
    /// Iterator over matching links
    pub fn find_links(&self, rel: &str) -> impl Iterator<Item = &Link> {
        self.links.iter().filter(move |link| link.rel == rel)
    }

    /// Validates the item according to STAC specification.
    ///
    /// # Returns
    ///
    /// `Ok(())` if valid, otherwise an error
    pub fn validate(&self) -> Result<()> {
        // Check type
        if self.type_ != "Feature" {
            return Err(StacError::InvalidType {
                expected: "Feature".to_string(),
                found: self.type_.clone(),
            });
        }

        // Check STAC version
        if self.stac_version != "1.0.0" {
            return Err(StacError::InvalidVersion(self.stac_version.clone()));
        }

        // Check ID
        if self.id.is_empty() {
            return Err(StacError::MissingField("id".to_string()));
        }

        // Check datetime requirements
        if self.properties.datetime.is_none()
            && (self.properties.start_datetime.is_none() || self.properties.end_datetime.is_none())
        {
            return Err(StacError::MissingField(
                "datetime or start_datetime/end_datetime".to_string(),
            ));
        }

        // Validate bbox if present
        if let Some(bbox) = &self.bbox {
            if bbox.len() != 4 && bbox.len() != 6 {
                return Err(StacError::InvalidBbox(format!(
                    "bbox must have 4 or 6 elements, found {}",
                    bbox.len()
                )));
            }
        }

        // Validate assets
        for (key, asset) in &self.assets {
            asset.validate().map_err(|e| StacError::InvalidFieldValue {
                field: format!("assets.{}", key),
                reason: e.to_string(),
            })?;
        }

        Ok(())
    }
}

impl Link {
    /// Creates a new link.
    ///
    /// # Arguments
    ///
    /// * `href` - URI to the linked resource
    /// * `rel` - Relationship type
    ///
    /// # Returns
    ///
    /// A new Link instance
    pub fn new(href: impl Into<String>, rel: impl Into<String>) -> Self {
        Self {
            href: href.into(),
            rel: rel.into(),
            media_type: None,
            title: None,
            additional_fields: HashMap::new(),
        }
    }

    /// Sets the media type of the link.
    ///
    /// # Arguments
    ///
    /// * `media_type` - Media type (MIME type)
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_type(mut self, media_type: impl Into<String>) -> Self {
        self.media_type = Some(media_type.into());
        self
    }

    /// Sets the title of the link.
    ///
    /// # Arguments
    ///
    /// * `title` - Title for the link
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// Common link relationship types.
pub mod link_rel {
    /// Link to self.
    pub const SELF: &str = "self";

    /// Link to root catalog.
    pub const ROOT: &str = "root";

    /// Link to parent catalog.
    pub const PARENT: &str = "parent";

    /// Link to collection.
    pub const COLLECTION: &str = "collection";

    /// Link to derived data.
    pub const DERIVED_FROM: &str = "derived_from";

    /// Link to license.
    pub const LICENSE: &str = "license";

    /// Alternate representation.
    pub const ALTERNATE: &str = "alternate";

    /// Preview link.
    pub const PREVIEW: &str = "preview";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_item_new() {
        let item = Item::new("test-item");
        assert_eq!(item.id, "test-item");
        assert_eq!(item.stac_version, "1.0.0");
        assert_eq!(item.type_, "Feature");
    }

    #[test]
    fn test_item_with_datetime() {
        let now = Utc::now();
        let item = Item::new("test-item").with_datetime(now);
        assert_eq!(item.properties.datetime, Some(now));
    }

    #[test]
    fn test_item_add_asset() {
        let asset = Asset::new("https://example.com/image.tif");
        let item = Item::new("test-item").add_asset("data", asset.clone());
        assert_eq!(item.get_asset("data"), Some(&asset));
    }

    #[test]
    fn test_item_add_link() {
        let link = Link::new("https://example.com", link_rel::SELF);
        let item = Item::new("test-item").add_link(link.clone());
        assert_eq!(item.links.len(), 1);
        assert_eq!(item.links[0], link);
    }

    #[test]
    fn test_item_validate_missing_datetime() {
        let item = Item::new("test-item");
        assert!(item.validate().is_err());
    }

    #[test]
    fn test_item_validate_with_datetime() {
        let item = Item::new("test-item").with_datetime(Utc::now());
        assert!(item.validate().is_ok());
    }

    #[test]
    fn test_item_validate_with_datetime_range() {
        let now = Utc::now();
        let item = Item::new("test-item").with_datetime_range(now, now);
        assert!(item.validate().is_ok());
    }

    #[test]
    fn test_link_builder() {
        let link = Link::new("https://example.com", link_rel::SELF)
            .with_type("application/json")
            .with_title("Self link");

        assert_eq!(link.href, "https://example.com");
        assert_eq!(link.rel, link_rel::SELF);
        assert_eq!(link.media_type, Some("application/json".to_string()));
        assert_eq!(link.title, Some("Self link".to_string()));
    }
}
