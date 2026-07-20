//! Builder patterns for STAC objects.
//!
//! This module provides fluent builder APIs for creating STAC Catalogs,
//! Collections, and Items.

use crate::{
    asset::Asset,
    catalog::Catalog,
    collection::{Collection, Provider},
    error::Result,
    item::{Item, Link},
};
use chrono::{DateTime, Utc};

/// Builder for creating STAC Catalogs.
#[derive(Debug, Clone)]
pub struct CatalogBuilder {
    catalog: Catalog,
}

impl CatalogBuilder {
    /// Creates a new CatalogBuilder.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique identifier for the catalog
    /// * `description` - Description of the catalog
    ///
    /// # Returns
    ///
    /// A new CatalogBuilder instance
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            catalog: Catalog::new(id, description),
        }
    }

    /// Sets the title of the catalog.
    ///
    /// # Arguments
    ///
    /// * `title` - Title for the catalog
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.catalog = self.catalog.with_title(title);
        self
    }

    /// Adds a link to the catalog.
    ///
    /// # Arguments
    ///
    /// * `href` - URI to the linked resource
    /// * `rel` - Relationship type
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn link(mut self, href: impl Into<String>, rel: impl Into<String>) -> Self {
        self.catalog = self.catalog.add_link(Link::new(href, rel));
        self
    }

    /// Adds a self link to the catalog.
    ///
    /// # Arguments
    ///
    /// * `href` - URI to self
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn self_link(self, href: impl Into<String>) -> Self {
        self.link(href, "self")
    }

    /// Adds a root link to the catalog.
    ///
    /// # Arguments
    ///
    /// * `href` - URI to root catalog
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn root_link(self, href: impl Into<String>) -> Self {
        self.link(href, "root")
    }

    /// Adds a child link to the catalog.
    ///
    /// # Arguments
    ///
    /// * `href` - URI to child catalog or collection
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn child_link(self, href: impl Into<String>) -> Self {
        self.link(href, "child")
    }

    /// Adds an extension to the catalog.
    ///
    /// # Arguments
    ///
    /// * `extension` - Extension schema URI
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn extension(mut self, extension: impl Into<String>) -> Self {
        self.catalog = self.catalog.add_extension(extension);
        self
    }

    /// Builds the catalog.
    ///
    /// # Returns
    ///
    /// The constructed Catalog
    pub fn build(self) -> Result<Catalog> {
        self.catalog.validate()?;
        Ok(self.catalog)
    }
}

/// Builder for creating STAC Collections.
#[derive(Debug, Clone)]
pub struct CollectionBuilder {
    collection: Collection,
}

impl CollectionBuilder {
    /// Creates a new CollectionBuilder.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique identifier for the collection
    /// * `description` - Description of the collection
    /// * `license` - License identifier or URL
    ///
    /// # Returns
    ///
    /// A new CollectionBuilder instance
    pub fn new(
        id: impl Into<String>,
        description: impl Into<String>,
        license: impl Into<String>,
    ) -> Self {
        Self {
            collection: Collection::new(id, description, license),
        }
    }

    /// Sets the title of the collection.
    ///
    /// # Arguments
    ///
    /// * `title` - Title for the collection
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.collection = self.collection.with_title(title);
        self
    }

    /// Sets the keywords of the collection.
    ///
    /// # Arguments
    ///
    /// * `keywords` - Vector of keywords
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn keywords(mut self, keywords: Vec<String>) -> Self {
        self.collection = self.collection.with_keywords(keywords);
        self
    }

    /// Adds a provider to the collection.
    ///
    /// # Arguments
    ///
    /// * `name` - Provider name
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn provider(mut self, name: impl Into<String>) -> Self {
        self.collection = self.collection.add_provider(Provider::new(name));
        self
    }

    /// Sets the spatial extent of the collection.
    ///
    /// # Arguments
    ///
    /// * `west` - Western longitude
    /// * `south` - Southern latitude
    /// * `east` - Eastern longitude
    /// * `north` - Northern latitude
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn spatial_extent(mut self, west: f64, south: f64, east: f64, north: f64) -> Self {
        self.collection = self
            .collection
            .with_spatial_extent(vec![west, south, east, north]);
        self
    }

    /// Sets the temporal extent of the collection.
    ///
    /// # Arguments
    ///
    /// * `start` - Start datetime (None for open start)
    /// * `end` - End datetime (None for open end)
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn temporal_extent(
        mut self,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    ) -> Self {
        self.collection = self.collection.with_temporal_extent(start, end);
        self
    }

    /// Adds a link to the collection.
    ///
    /// # Arguments
    ///
    /// * `href` - URI to the linked resource
    /// * `rel` - Relationship type
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn link(mut self, href: impl Into<String>, rel: impl Into<String>) -> Self {
        self.collection = self.collection.add_link(Link::new(href, rel));
        self
    }

    /// Adds a self link to the collection.
    ///
    /// # Arguments
    ///
    /// * `href` - URI to self
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn self_link(self, href: impl Into<String>) -> Self {
        self.link(href, "self")
    }

    /// Adds an extension to the collection.
    ///
    /// # Arguments
    ///
    /// * `extension` - Extension schema URI
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn extension(mut self, extension: impl Into<String>) -> Self {
        self.collection = self.collection.add_extension(extension);
        self
    }

    /// Builds the collection.
    ///
    /// # Returns
    ///
    /// The constructed Collection
    pub fn build(self) -> Result<Collection> {
        self.collection.validate()?;
        Ok(self.collection)
    }
}

/// Builder for creating STAC Items.
#[derive(Debug, Clone)]
pub struct ItemBuilder {
    item: Item,
}

impl ItemBuilder {
    /// Creates a new ItemBuilder.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique identifier for the item
    ///
    /// # Returns
    ///
    /// A new ItemBuilder instance
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            item: Item::new(id),
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
    pub fn geometry(mut self, geometry: geojson::Geometry) -> Self {
        self.item = self.item.with_geometry(geometry);
        self
    }

    /// Sets the bounding box of the item.
    ///
    /// # Arguments
    ///
    /// * `west` - Western longitude
    /// * `south` - Southern latitude
    /// * `east` - Eastern longitude
    /// * `north` - Northern latitude
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn bbox(mut self, west: f64, south: f64, east: f64, north: f64) -> Self {
        self.item = self.item.with_bbox(vec![west, south, east, north]);
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
    pub fn datetime(mut self, datetime: DateTime<Utc>) -> Self {
        self.item = self.item.with_datetime(datetime);
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
    pub fn datetime_range(mut self, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        self.item = self.item.with_datetime_range(start, end);
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
    pub fn asset(mut self, key: impl Into<String>, asset: Asset) -> Self {
        self.item = self.item.add_asset(key, asset);
        self
    }

    /// Adds a simple asset with just an href.
    ///
    /// # Arguments
    ///
    /// * `key` - Asset key
    /// * `href` - URI to the asset
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn simple_asset(self, key: impl Into<String>, href: impl Into<String>) -> Self {
        self.asset(key, Asset::new(href))
    }

    /// Adds a link to the item.
    ///
    /// # Arguments
    ///
    /// * `href` - URI to the linked resource
    /// * `rel` - Relationship type
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn link(mut self, href: impl Into<String>, rel: impl Into<String>) -> Self {
        self.item = self.item.add_link(Link::new(href, rel));
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
    pub fn collection(mut self, collection_id: impl Into<String>) -> Self {
        self.item = self.item.with_collection(collection_id);
        self
    }

    /// Adds an extension to the item.
    ///
    /// # Arguments
    ///
    /// * `extension` - Extension schema URI
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn extension(mut self, extension: impl Into<String>) -> Self {
        self.item = self.item.add_extension(extension);
        self
    }

    /// Sets a property value.
    ///
    /// # Arguments
    ///
    /// * `key` - Property key
    /// * `value` - Property value
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn property(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.item
            .properties
            .additional_fields
            .insert(key.into(), value);
        self
    }

    /// Builds the item.
    ///
    /// # Returns
    ///
    /// The constructed Item
    pub fn build(self) -> Result<Item> {
        self.item.validate()?;
        Ok(self.item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catalog_builder() {
        let catalog = CatalogBuilder::new("test-catalog", "A test catalog")
            .title("Test Catalog")
            .self_link("https://example.com/catalog.json")
            .child_link("https://example.com/collection.json")
            .build();

        assert!(catalog.is_ok());
        let catalog = catalog.expect("Failed to build catalog");
        assert_eq!(catalog.id, "test-catalog");
        assert_eq!(catalog.title, Some("Test Catalog".to_string()));
        assert_eq!(catalog.links.len(), 2);
    }

    #[test]
    fn test_collection_builder() {
        let now = Utc::now();
        let collection = CollectionBuilder::new("test-collection", "A test collection", "MIT")
            .title("Test Collection")
            .keywords(vec!["test".to_string(), "example".to_string()])
            .provider("Test Provider")
            .spatial_extent(-180.0, -90.0, 180.0, 90.0)
            .temporal_extent(Some(now), None)
            .build();

        assert!(collection.is_ok());
        let collection = collection.expect("Failed to build collection");
        assert_eq!(collection.id, "test-collection");
        assert_eq!(collection.title, Some("Test Collection".to_string()));
    }

    #[test]
    fn test_item_builder() {
        let now = Utc::now();
        let geometry = geojson::Geometry::new_point([-122.0, 37.0]);

        let item = ItemBuilder::new("test-item")
            .geometry(geometry)
            .bbox(-122.5, 36.5, -121.5, 37.5)
            .datetime(now)
            .simple_asset("data", "https://example.com/data.tif")
            .collection("test-collection")
            .build();

        assert!(item.is_ok());
        let item = item.expect("Failed to build item");
        assert_eq!(item.id, "test-item");
        assert_eq!(item.assets.len(), 1);
        assert_eq!(item.collection, Some("test-collection".to_string()));
    }

    #[test]
    fn test_item_builder_with_properties() {
        let now = Utc::now();

        let item = ItemBuilder::new("test-item")
            .datetime(now)
            .property("cloud_cover", serde_json::json!(10.5))
            .property("platform", serde_json::json!("sentinel-2a"))
            .build();

        assert!(item.is_ok());
        let item = item.expect("Failed to build item");
        assert_eq!(
            item.properties.additional_fields.get("cloud_cover"),
            Some(&serde_json::json!(10.5))
        );
    }
}
