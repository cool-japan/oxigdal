//! STAC Catalog representation.

use crate::{
    error::{Result, StacError},
    item::Link,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A STAC Catalog is a simple grouping of STAC Items and/or other STAC Catalogs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Catalog {
    /// Type must be "Catalog".
    #[serde(rename = "type")]
    pub type_: String,

    /// STAC version.
    #[serde(rename = "stac_version")]
    pub stac_version: String,

    /// List of extensions used in the catalog.
    #[serde(skip_serializing_if = "Option::is_none", rename = "stac_extensions")]
    pub stac_extensions: Option<Vec<String>>,

    /// Unique identifier for the catalog.
    pub id: String,

    /// Title of the catalog.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Description of the catalog.
    pub description: String,

    /// Links to other resources.
    pub links: Vec<Link>,

    /// Additional fields for extensions.
    #[serde(flatten)]
    pub additional_fields: HashMap<String, serde_json::Value>,
}

impl Catalog {
    /// Creates a new STAC Catalog.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique identifier for the catalog
    /// * `description` - Description of the catalog
    ///
    /// # Returns
    ///
    /// A new Catalog instance with default STAC version 1.0.0
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            type_: "Catalog".to_string(),
            stac_version: "1.0.0".to_string(),
            stac_extensions: None,
            id: id.into(),
            title: None,
            description: description.into(),
            links: Vec::new(),
            additional_fields: HashMap::new(),
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
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Adds a link to the catalog.
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

    /// Gets the self link if available.
    ///
    /// # Returns
    ///
    /// Reference to the self link if it exists
    pub fn get_self_link(&self) -> Option<&Link> {
        self.find_links("self").next()
    }

    /// Gets the root link if available.
    ///
    /// # Returns
    ///
    /// Reference to the root link if it exists
    pub fn get_root_link(&self) -> Option<&Link> {
        self.find_links("root").next()
    }

    /// Gets all child links (items and catalogs).
    ///
    /// # Returns
    ///
    /// Iterator over child links
    pub fn get_child_links(&self) -> impl Iterator<Item = &Link> {
        self.links
            .iter()
            .filter(|link| link.rel == "child" || link.rel == "item")
    }

    /// Validates the catalog according to STAC specification.
    ///
    /// # Returns
    ///
    /// `Ok(())` if valid, otherwise an error
    pub fn validate(&self) -> Result<()> {
        // Check type
        if self.type_ != "Catalog" {
            return Err(StacError::InvalidType {
                expected: "Catalog".to_string(),
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

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catalog_new() {
        let catalog = Catalog::new("test-catalog", "Test description");
        assert_eq!(catalog.id, "test-catalog");
        assert_eq!(catalog.description, "Test description");
        assert_eq!(catalog.stac_version, "1.0.0");
        assert_eq!(catalog.type_, "Catalog");
    }

    #[test]
    fn test_catalog_with_title() {
        let catalog = Catalog::new("test-catalog", "Test description").with_title("Test Title");
        assert_eq!(catalog.title, Some("Test Title".to_string()));
    }

    #[test]
    fn test_catalog_add_link() {
        let link = Link::new("https://example.com", "self");
        let catalog = Catalog::new("test-catalog", "Test description").add_link(link.clone());
        assert_eq!(catalog.links.len(), 1);
        assert_eq!(catalog.links[0], link);
    }

    #[test]
    fn test_catalog_find_links() {
        let link1 = Link::new("https://example.com/self", "self");
        let link2 = Link::new("https://example.com/child", "child");
        let catalog = Catalog::new("test-catalog", "Test description")
            .add_link(link1.clone())
            .add_link(link2);

        let self_links: Vec<_> = catalog.find_links("self").collect();
        assert_eq!(self_links.len(), 1);
        assert_eq!(self_links[0], &link1);
    }

    #[test]
    fn test_catalog_get_self_link() {
        let link = Link::new("https://example.com", "self");
        let catalog = Catalog::new("test-catalog", "Test description").add_link(link.clone());
        assert_eq!(catalog.get_self_link(), Some(&link));
    }

    #[test]
    fn test_catalog_validate() {
        let catalog = Catalog::new("test-catalog", "Test description");
        assert!(catalog.validate().is_ok());
    }

    #[test]
    fn test_catalog_validate_empty_id() {
        let catalog = Catalog::new("", "Test description");
        assert!(catalog.validate().is_err());
    }

    #[test]
    fn test_catalog_validate_empty_description() {
        let catalog = Catalog::new("test-catalog", "");
        assert!(catalog.validate().is_err());
    }
}
