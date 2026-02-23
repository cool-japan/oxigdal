//! DCAT (Data Catalog Vocabulary) metadata.
//!
//! This module implements the W3C DCAT v3 vocabulary for describing
//! data catalogs, datasets, and data services.
//!
//! # Overview
//!
//! DCAT is an RDF vocabulary designed to facilitate interoperability
//! between data catalogs. It provides:
//! - Dataset descriptions
//! - Distribution information
//! - Data service descriptions
//! - Catalog metadata
//!
//! # Examples
//!
//! ```no_run
//! use oxigdal_metadata::dcat::*;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let dataset = Dataset::builder()
//!     .title("Climate Data")
//!     .description("Global climate observations")
//!     .keyword("climate")
//!     .keyword("temperature")
//!     .build()?;
//! # Ok(())
//! # }
//! ```

use crate::error::{MetadataError, Result};
use serde::{Deserialize, Serialize};
use url::Url;

/// DCAT Catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    /// Catalog title (mandatory)
    pub title: Vec<LiteralString>,
    /// Catalog description (mandatory)
    pub description: Vec<LiteralString>,
    /// Publisher (mandatory)
    pub publisher: Agent,
    /// Datasets in catalog (mandatory)
    pub dataset: Vec<Dataset>,
    /// Catalog homepage
    pub homepage: Option<Url>,
    /// Language
    pub language: Vec<String>,
    /// License
    pub license: Option<Url>,
    /// Issued date
    pub issued: Option<chrono::DateTime<chrono::Utc>>,
    /// Modified date
    pub modified: Option<chrono::DateTime<chrono::Utc>>,
    /// Spatial coverage
    pub spatial: Vec<Location>,
    /// Temporal coverage
    pub temporal: Vec<PeriodOfTime>,
    /// Themes
    pub themes: Vec<String>,
    /// Catalog record
    pub record: Vec<CatalogRecord>,
    /// Data service
    pub service: Vec<DataService>,
}

/// DCAT Dataset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dataset {
    /// Dataset title (mandatory)
    pub title: Vec<LiteralString>,
    /// Dataset description (mandatory)
    pub description: Vec<LiteralString>,
    /// Identifier
    pub identifier: Vec<String>,
    /// Keywords
    pub keyword: Vec<String>,
    /// Theme
    pub theme: Vec<String>,
    /// Contact point
    pub contact_point: Vec<ContactPoint>,
    /// Publisher
    pub publisher: Option<Agent>,
    /// Creator
    pub creator: Option<Agent>,
    /// Issued date
    pub issued: Option<chrono::DateTime<chrono::Utc>>,
    /// Modified date
    pub modified: Option<chrono::DateTime<chrono::Utc>>,
    /// Language
    pub language: Vec<String>,
    /// Landing page
    pub landing_page: Vec<Url>,
    /// Access rights
    pub access_rights: Option<String>,
    /// Conforms to
    pub conforms_to: Vec<String>,
    /// Distributions
    pub distribution: Vec<Distribution>,
    /// Frequency
    pub accrual_periodicity: Option<String>,
    /// Spatial coverage
    pub spatial: Vec<Location>,
    /// Temporal coverage
    pub temporal: Vec<PeriodOfTime>,
    /// Version
    pub version: Option<String>,
    /// Version notes
    pub version_notes: Vec<String>,
    /// Qualified relation
    pub qualified_relation: Vec<Relationship>,
}

/// Builder for DCAT Dataset.
pub struct DatasetBuilder {
    title: Vec<LiteralString>,
    description: Vec<LiteralString>,
    identifier: Vec<String>,
    keyword: Vec<String>,
    theme: Vec<String>,
    contact_point: Vec<ContactPoint>,
    publisher: Option<Agent>,
    creator: Option<Agent>,
    issued: Option<chrono::DateTime<chrono::Utc>>,
    modified: Option<chrono::DateTime<chrono::Utc>>,
    language: Vec<String>,
    landing_page: Vec<Url>,
    access_rights: Option<String>,
    conforms_to: Vec<String>,
    distribution: Vec<Distribution>,
    accrual_periodicity: Option<String>,
    spatial: Vec<Location>,
    temporal: Vec<PeriodOfTime>,
    version: Option<String>,
    version_notes: Vec<String>,
    qualified_relation: Vec<Relationship>,
}

impl Dataset {
    /// Create a new builder.
    pub fn builder() -> DatasetBuilder {
        DatasetBuilder {
            title: Vec::new(),
            description: Vec::new(),
            identifier: Vec::new(),
            keyword: Vec::new(),
            theme: Vec::new(),
            contact_point: Vec::new(),
            publisher: None,
            creator: None,
            issued: None,
            modified: None,
            language: Vec::new(),
            landing_page: Vec::new(),
            access_rights: None,
            conforms_to: Vec::new(),
            distribution: Vec::new(),
            accrual_periodicity: None,
            spatial: Vec::new(),
            temporal: Vec::new(),
            version: None,
            version_notes: Vec::new(),
            qualified_relation: Vec::new(),
        }
    }
}

impl DatasetBuilder {
    /// Set the title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title.push(LiteralString {
            value: title.into(),
            language: None,
        });
        self
    }

    /// Set the description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description.push(LiteralString {
            value: description.into(),
            language: None,
        });
        self
    }

    /// Add a keyword.
    pub fn keyword(mut self, keyword: impl Into<String>) -> Self {
        self.keyword.push(keyword.into());
        self
    }

    /// Add a theme.
    pub fn theme(mut self, theme: impl Into<String>) -> Self {
        self.theme.push(theme.into());
        self
    }

    /// Set the publisher.
    pub fn publisher(mut self, publisher: Agent) -> Self {
        self.publisher = Some(publisher);
        self
    }

    /// Add a distribution.
    pub fn distribution(mut self, distribution: Distribution) -> Self {
        self.distribution.push(distribution);
        self
    }

    /// Build the dataset.
    pub fn build(self) -> Result<Dataset> {
        if self.title.is_empty() {
            return Err(MetadataError::MissingField("title".to_string()));
        }
        if self.description.is_empty() {
            return Err(MetadataError::MissingField("description".to_string()));
        }

        Ok(Dataset {
            title: self.title,
            description: self.description,
            identifier: self.identifier,
            keyword: self.keyword,
            theme: self.theme,
            contact_point: self.contact_point,
            publisher: self.publisher,
            creator: self.creator,
            issued: self.issued,
            modified: self.modified,
            language: self.language,
            landing_page: self.landing_page,
            access_rights: self.access_rights,
            conforms_to: self.conforms_to,
            distribution: self.distribution,
            accrual_periodicity: self.accrual_periodicity,
            spatial: self.spatial,
            temporal: self.temporal,
            version: self.version,
            version_notes: self.version_notes,
            qualified_relation: self.qualified_relation,
        })
    }
}

/// DCAT Distribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Distribution {
    /// Access URL (mandatory)
    pub access_url: Url,
    /// Download URL
    pub download_url: Option<Url>,
    /// Title
    pub title: Vec<LiteralString>,
    /// Description
    pub description: Vec<LiteralString>,
    /// Format
    pub format: Option<String>,
    /// Media type
    pub media_type: Option<String>,
    /// Byte size
    pub byte_size: Option<u64>,
    /// Checksum
    pub checksum: Option<Checksum>,
    /// Compression format
    pub compression_format: Option<String>,
    /// Package format
    pub package_format: Option<String>,
    /// Issued date
    pub issued: Option<chrono::DateTime<chrono::Utc>>,
    /// Modified date
    pub modified: Option<chrono::DateTime<chrono::Utc>>,
    /// License
    pub license: Option<Url>,
    /// Rights
    pub rights: Option<String>,
    /// Access service
    pub access_service: Vec<DataService>,
}

/// DCAT Data Service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataService {
    /// Service title (mandatory)
    pub title: Vec<LiteralString>,
    /// Endpoint URL (mandatory)
    pub endpoint_url: Vec<Url>,
    /// Endpoint description
    pub endpoint_description: Vec<Url>,
    /// Service description
    pub description: Vec<LiteralString>,
    /// Service type
    pub service_type: Option<String>,
    /// Serves dataset
    pub serves_dataset: Vec<Dataset>,
    /// Contact point
    pub contact_point: Vec<ContactPoint>,
    /// License
    pub license: Option<Url>,
    /// Access rights
    pub access_rights: Option<String>,
}

/// DCAT Catalog Record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogRecord {
    /// Title
    pub title: Vec<LiteralString>,
    /// Description
    pub description: Vec<LiteralString>,
    /// Issued date
    pub issued: Option<chrono::DateTime<chrono::Utc>>,
    /// Modified date (mandatory)
    pub modified: chrono::DateTime<chrono::Utc>,
    /// Primary topic
    pub primary_topic: Option<Dataset>,
    /// Conforms to
    pub conforms_to: Vec<String>,
}

/// Agent (publisher, creator, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    /// Name
    pub name: String,
    /// Type
    pub agent_type: Option<AgentType>,
}

impl Agent {
    /// Create a new agent.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            agent_type: None,
        }
    }

    /// Create an organization agent.
    pub fn organization(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            agent_type: Some(AgentType::Organization),
        }
    }

    /// Create a person agent.
    pub fn person(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            agent_type: Some(AgentType::Person),
        }
    }
}

/// Agent type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AgentType {
    /// Organization
    Organization,
    /// Person
    Person,
}

/// Contact point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactPoint {
    /// Name
    pub name: Option<String>,
    /// Email
    pub email: Option<String>,
    /// URL
    pub url: Option<Url>,
}

/// Literal string with optional language tag.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiteralString {
    /// Value
    pub value: String,
    /// Language code
    pub language: Option<String>,
}

impl LiteralString {
    /// Create a new literal string.
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            language: None,
        }
    }

    /// Create with language tag.
    pub fn with_language(value: impl Into<String>, language: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            language: Some(language.into()),
        }
    }
}

/// Location.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    /// Geometry (WKT, GeoJSON, etc.)
    pub geometry: Option<String>,
    /// Bounding box
    pub bbox: Option<BoundingBox>,
    /// Centroid
    pub centroid: Option<Point>,
}

/// Bounding box.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BoundingBox {
    /// West bound
    pub west: f64,
    /// East bound
    pub east: f64,
    /// South bound
    pub south: f64,
    /// North bound
    pub north: f64,
}

/// Point.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Point {
    /// Longitude
    pub lon: f64,
    /// Latitude
    pub lat: f64,
}

/// Period of time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeriodOfTime {
    /// Start date
    pub start_date: Option<chrono::DateTime<chrono::Utc>>,
    /// End date
    pub end_date: Option<chrono::DateTime<chrono::Utc>>,
}

/// Checksum.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checksum {
    /// Algorithm
    pub algorithm: String,
    /// Checksum value
    pub checksum_value: String,
}

/// Relationship.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    /// Relation
    pub relation: Url,
    /// Had role
    pub had_role: Option<String>,
}

impl Dataset {
    /// Export to JSON-LD format.
    pub fn to_jsonld(&self) -> Result<String> {
        // Add JSON-LD context
        let mut map = serde_json::json!({
            "@context": "https://www.w3.org/ns/dcat",
            "@type": "Dataset",
        });

        let dataset_json =
            serde_json::to_value(self).map_err(|e| MetadataError::JsonError(e.to_string()))?;

        if let serde_json::Value::Object(obj) = dataset_json {
            if let serde_json::Value::Object(ref mut context_map) = map {
                context_map.extend(obj);
            }
        }

        serde_json::to_string_pretty(&map).map_err(|e| MetadataError::JsonError(e.to_string()))
    }

    /// Export to JSON format.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(&self).map_err(|e| MetadataError::JsonError(e.to_string()))
    }

    /// Import from JSON format.
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| MetadataError::JsonError(e.to_string()))
    }
}

impl Catalog {
    /// Export to JSON-LD format.
    pub fn to_jsonld(&self) -> Result<String> {
        let mut map = serde_json::json!({
            "@context": "https://www.w3.org/ns/dcat",
            "@type": "Catalog",
        });

        let catalog_json =
            serde_json::to_value(self).map_err(|e| MetadataError::JsonError(e.to_string()))?;

        if let serde_json::Value::Object(obj) = catalog_json {
            if let serde_json::Value::Object(ref mut context_map) = map {
                context_map.extend(obj);
            }
        }

        serde_json::to_string_pretty(&map).map_err(|e| MetadataError::JsonError(e.to_string()))
    }

    /// Export to JSON format.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(&self).map_err(|e| MetadataError::JsonError(e.to_string()))
    }

    /// Import from JSON format.
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| MetadataError::JsonError(e.to_string()))
    }
}
