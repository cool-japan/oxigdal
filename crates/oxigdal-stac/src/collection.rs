//! STAC Collection representation.

use crate::{
    error::{Result, StacError},
    item::Link,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A STAC Collection provides additional metadata about a set of Items.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Collection {
    /// Type must be "Collection".
    #[serde(rename = "type")]
    pub type_: String,

    /// STAC version.
    #[serde(rename = "stac_version")]
    pub stac_version: String,

    /// List of extensions used in the collection.
    #[serde(skip_serializing_if = "Option::is_none", rename = "stac_extensions")]
    pub stac_extensions: Option<Vec<String>>,

    /// Unique identifier for the collection.
    pub id: String,

    /// Title of the collection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Description of the collection.
    pub description: String,

    /// Keywords describing the collection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Vec<String>>,

    /// License of the collection (SPDX license identifier or URL).
    pub license: String,

    /// Providers of the collection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub providers: Option<Vec<Provider>>,

    /// Extent of the collection.
    pub extent: Extent,

    /// Summaries of properties in the collection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summaries: Option<HashMap<String, serde_json::Value>>,

    /// Links to other resources.
    pub links: Vec<Link>,

    /// Assets associated with this collection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assets: Option<HashMap<String, crate::asset::Asset>>,

    /// Additional fields for extensions.
    #[serde(flatten)]
    pub additional_fields: HashMap<String, serde_json::Value>,
}

/// Provider of data in a STAC Collection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Provider {
    /// Name of the provider.
    pub name: String,

    /// Description of the provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Roles of the provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roles: Option<Vec<String>>,

    /// URL of the provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Extent of a STAC Collection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Extent {
    /// Spatial extent.
    pub spatial: SpatialExtent,

    /// Temporal extent.
    pub temporal: TemporalExtent,
}

/// Spatial extent of a STAC Collection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpatialExtent {
    /// Bounding boxes of the collection [west, south, east, north].
    pub bbox: Vec<Vec<f64>>,
}

/// Temporal extent of a STAC Collection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemporalExtent {
    /// Time intervals of the collection [[start, end], ...].
    /// `null` can be used for open-ended intervals.
    pub interval: Vec<Vec<Option<DateTime<Utc>>>>,
}

impl Collection {
    /// Creates a new STAC Collection.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique identifier for the collection
    /// * `description` - Description of the collection
    /// * `license` - License identifier or URL
    ///
    /// # Returns
    ///
    /// A new Collection instance with default STAC version 1.0.0
    pub fn new(
        id: impl Into<String>,
        description: impl Into<String>,
        license: impl Into<String>,
    ) -> Self {
        Self {
            type_: "Collection".to_string(),
            stac_version: "1.0.0".to_string(),
            stac_extensions: None,
            id: id.into(),
            title: None,
            description: description.into(),
            keywords: None,
            license: license.into(),
            providers: None,
            extent: Extent {
                spatial: SpatialExtent { bbox: vec![] },
                temporal: TemporalExtent { interval: vec![] },
            },
            summaries: None,
            links: Vec::new(),
            assets: None,
            additional_fields: HashMap::new(),
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
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
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
    pub fn with_keywords(mut self, keywords: Vec<String>) -> Self {
        self.keywords = Some(keywords);
        self
    }

    /// Adds a provider to the collection.
    ///
    /// # Arguments
    ///
    /// * `provider` - Provider to add
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn add_provider(mut self, provider: Provider) -> Self {
        match &mut self.providers {
            Some(providers) => providers.push(provider),
            None => self.providers = Some(vec![provider]),
        }
        self
    }

    /// Sets the spatial extent of the collection.
    ///
    /// # Arguments
    ///
    /// * `bbox` - Bounding box [west, south, east, north]
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_spatial_extent(mut self, bbox: Vec<f64>) -> Self {
        self.extent.spatial.bbox = vec![bbox];
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
    pub fn with_temporal_extent(
        mut self,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    ) -> Self {
        self.extent.temporal.interval = vec![vec![start, end]];
        self
    }

    /// Adds a link to the collection.
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

    /// Adds a summary field.
    ///
    /// # Arguments
    ///
    /// * `key` - Summary field name
    /// * `value` - Summary value
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn add_summary(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        match &mut self.summaries {
            Some(summaries) => {
                summaries.insert(key.into(), value);
            }
            None => {
                let mut summaries = HashMap::new();
                summaries.insert(key.into(), value);
                self.summaries = Some(summaries);
            }
        }
        self
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

    /// Validates the collection according to STAC specification.
    ///
    /// # Returns
    ///
    /// `Ok(())` if valid, otherwise an error
    pub fn validate(&self) -> Result<()> {
        // Check type
        if self.type_ != "Collection" {
            return Err(StacError::InvalidType {
                expected: "Collection".to_string(),
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

        // Check description
        if self.description.is_empty() {
            return Err(StacError::MissingField("description".to_string()));
        }

        // Check license
        if self.license.is_empty() {
            return Err(StacError::MissingField("license".to_string()));
        }

        // Validate spatial extent
        for bbox in &self.extent.spatial.bbox {
            if bbox.len() != 4 && bbox.len() != 6 {
                return Err(StacError::InvalidBbox(format!(
                    "bbox must have 4 or 6 elements, found {}",
                    bbox.len()
                )));
            }
        }

        Ok(())
    }
}

impl Provider {
    /// Creates a new provider.
    ///
    /// # Arguments
    ///
    /// * `name` - Provider name
    ///
    /// # Returns
    ///
    /// A new Provider instance
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            roles: None,
            url: None,
        }
    }

    /// Sets the description of the provider.
    ///
    /// # Arguments
    ///
    /// * `description` - Description of the provider
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the roles of the provider.
    ///
    /// # Arguments
    ///
    /// * `roles` - Vector of role identifiers
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_roles(mut self, roles: Vec<String>) -> Self {
        self.roles = Some(roles);
        self
    }

    /// Sets the URL of the provider.
    ///
    /// # Arguments
    ///
    /// * `url` - Provider URL
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }
}

/// Common provider roles.
pub mod provider_roles {
    /// Licensor role.
    pub const LICENSOR: &str = "licensor";

    /// Producer role.
    pub const PRODUCER: &str = "producer";

    /// Processor role.
    pub const PROCESSOR: &str = "processor";

    /// Host role.
    pub const HOST: &str = "host";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collection_new() {
        let collection = Collection::new("test-collection", "Test description", "MIT");
        assert_eq!(collection.id, "test-collection");
        assert_eq!(collection.description, "Test description");
        assert_eq!(collection.license, "MIT");
        assert_eq!(collection.stac_version, "1.0.0");
        assert_eq!(collection.type_, "Collection");
    }

    #[test]
    fn test_collection_with_title() {
        let collection =
            Collection::new("test-collection", "Test description", "MIT").with_title("Test Title");
        assert_eq!(collection.title, Some("Test Title".to_string()));
    }

    #[test]
    fn test_collection_add_provider() {
        let provider = Provider::new("Test Provider");
        let collection = Collection::new("test-collection", "Test description", "MIT")
            .add_provider(provider.clone());
        assert_eq!(collection.providers, Some(vec![provider]));
    }

    #[test]
    fn test_collection_with_spatial_extent() {
        let collection = Collection::new("test-collection", "Test description", "MIT")
            .with_spatial_extent(vec![-180.0, -90.0, 180.0, 90.0]);
        assert_eq!(
            collection.extent.spatial.bbox,
            vec![vec![-180.0, -90.0, 180.0, 90.0]]
        );
    }

    #[test]
    fn test_collection_with_temporal_extent() {
        let start = Utc::now();
        let end = Utc::now();
        let collection = Collection::new("test-collection", "Test description", "MIT")
            .with_temporal_extent(Some(start), Some(end));
        assert_eq!(
            collection.extent.temporal.interval,
            vec![vec![Some(start), Some(end)]]
        );
    }

    #[test]
    fn test_collection_validate() {
        let collection = Collection::new("test-collection", "Test description", "MIT")
            .with_spatial_extent(vec![-180.0, -90.0, 180.0, 90.0]);
        assert!(collection.validate().is_ok());
    }

    #[test]
    fn test_provider_builder() {
        let provider = Provider::new("Test Provider")
            .with_description("A test provider")
            .with_roles(vec![provider_roles::PRODUCER.to_string()])
            .with_url("https://example.com");

        assert_eq!(provider.name, "Test Provider");
        assert_eq!(provider.description, Some("A test provider".to_string()));
        assert_eq!(
            provider.roles,
            Some(vec![provider_roles::PRODUCER.to_string()])
        );
        assert_eq!(provider.url, Some("https://example.com".to_string()));
    }
}
