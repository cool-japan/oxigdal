//! ISO 19115 Geographic Information - Metadata standard.
//!
//! This module implements the ISO 19115:2014 metadata standard for geographic information.
//!
//! # Overview
//!
//! ISO 19115 defines a comprehensive schema for describing geographic information and services.
//! It includes metadata for datasets, services, applications, and other resources.
//!
//! # Examples
//!
//! ```no_run
//! use oxigdal_metadata::iso19115::*;
//! use oxigdal_metadata::common::*;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let metadata = Iso19115Metadata::builder()
//!     .title("Sentinel-2 Level-2A")
//!     .abstract_text("Sentinel-2 atmospherically corrected imagery")
//!     .keywords(vec!["satellite", "sentinel-2", "optical"])
//!     .bbox(BoundingBox::new(-10.0, 5.0, 35.0, 45.0))
//!     .build()?;
//! # Ok(())
//! # }
//! ```

pub mod core;
pub mod reference_system;
pub mod spatial_representation;

pub use self::core::*;
pub use reference_system::*;
pub use spatial_representation::*;

use crate::common::{BoundingBox, ContactInfo, Keyword, TemporalExtent};
use crate::error::{MetadataError, Result};
use serde::{Deserialize, Serialize};

/// Complete ISO 19115 metadata record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Iso19115Metadata {
    /// Metadata identifier
    pub file_identifier: Option<String>,
    /// Metadata language
    pub language: Option<String>,
    /// Metadata character set
    pub character_set: Option<CharacterSet>,
    /// Metadata hierarchy level
    pub hierarchy_level: HierarchyLevel,
    /// Metadata contact
    pub contact: Vec<ResponsibleParty>,
    /// Metadata date stamp
    pub date_stamp: chrono::DateTime<chrono::Utc>,
    /// Metadata standard name
    pub metadata_standard_name: String,
    /// Metadata standard version
    pub metadata_standard_version: String,
    /// Identification info
    pub identification_info: Vec<DataIdentification>,
    /// Distribution info
    pub distribution_info: Option<Distribution>,
    /// Data quality info
    pub data_quality_info: Option<DataQuality>,
    /// Reference system info
    pub reference_system_info: Vec<ReferenceSystem>,
}

impl Default for Iso19115Metadata {
    fn default() -> Self {
        Self {
            file_identifier: None,
            language: Some("eng".to_string()),
            character_set: Some(CharacterSet::Utf8),
            hierarchy_level: HierarchyLevel::Dataset,
            contact: Vec::new(),
            date_stamp: chrono::Utc::now(),
            metadata_standard_name: "ISO 19115:2014".to_string(),
            metadata_standard_version: "2014".to_string(),
            identification_info: Vec::new(),
            distribution_info: None,
            data_quality_info: None,
            reference_system_info: Vec::new(),
        }
    }
}

/// Builder for ISO 19115 metadata.
pub struct Iso19115Builder {
    metadata: Iso19115Metadata,
}

impl Iso19115Metadata {
    /// Create a new builder.
    pub fn builder() -> Iso19115Builder {
        Iso19115Builder {
            metadata: Self::default(),
        }
    }
}

impl Iso19115Builder {
    /// Set the title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        if self.metadata.identification_info.is_empty() {
            self.metadata
                .identification_info
                .push(DataIdentification::default());
        }
        self.metadata.identification_info[0].citation.title = title.into();
        self
    }

    /// Set the abstract.
    pub fn abstract_text(mut self, abstract_text: impl Into<String>) -> Self {
        if self.metadata.identification_info.is_empty() {
            self.metadata
                .identification_info
                .push(DataIdentification::default());
        }
        self.metadata.identification_info[0].abstract_text = abstract_text.into();
        self
    }

    /// Add keywords.
    pub fn keywords(mut self, keywords: Vec<impl Into<String>>) -> Self {
        if self.metadata.identification_info.is_empty() {
            self.metadata
                .identification_info
                .push(DataIdentification::default());
        }
        let kw = keywords
            .into_iter()
            .map(|k| Keyword {
                keyword: k.into(),
                thesaurus: None,
            })
            .collect();
        self.metadata.identification_info[0].keywords.push(kw);
        self
    }

    /// Set the bounding box.
    pub fn bbox(mut self, bbox: BoundingBox) -> Self {
        if self.metadata.identification_info.is_empty() {
            self.metadata
                .identification_info
                .push(DataIdentification::default());
        }
        self.metadata.identification_info[0]
            .extent
            .geographic_extent = Some(bbox);
        self
    }

    /// Set the temporal extent.
    pub fn temporal_extent(mut self, extent: TemporalExtent) -> Self {
        if self.metadata.identification_info.is_empty() {
            self.metadata
                .identification_info
                .push(DataIdentification::default());
        }
        self.metadata.identification_info[0].extent.temporal_extent = Some(extent);
        self
    }

    /// Add a contact.
    pub fn contact(mut self, contact: ResponsibleParty) -> Self {
        self.metadata.contact.push(contact);
        self
    }

    /// Set the file identifier.
    pub fn file_identifier(mut self, id: impl Into<String>) -> Self {
        self.metadata.file_identifier = Some(id.into());
        self
    }

    /// Build the metadata.
    pub fn build(self) -> Result<Iso19115Metadata> {
        if self.metadata.identification_info.is_empty() {
            return Err(MetadataError::MissingField(
                "identification_info".to_string(),
            ));
        }
        Ok(self.metadata)
    }
}

/// Character set enumeration.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CharacterSet {
    /// UTF-8
    Utf8,
    /// ISO 8859-1
    Iso8859_1,
    /// UTF-16
    Utf16,
}

/// Metadata hierarchy level.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum HierarchyLevel {
    /// Dataset
    Dataset,
    /// Series
    Series,
    /// Service
    Service,
    /// Application
    Application,
}

/// Responsible party information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsibleParty {
    /// Individual name
    pub individual_name: Option<String>,
    /// Organization name
    pub organization_name: Option<String>,
    /// Position name
    pub position_name: Option<String>,
    /// Contact info
    pub contact_info: Option<ContactInfo>,
    /// Role
    pub role: Role,
}

/// Role code.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Role {
    /// Resource provider
    ResourceProvider,
    /// Custodian
    Custodian,
    /// Owner
    Owner,
    /// User
    User,
    /// Distributor
    Distributor,
    /// Originator
    Originator,
    /// Point of contact
    PointOfContact,
    /// Principal investigator
    PrincipalInvestigator,
    /// Processor
    Processor,
    /// Publisher
    Publisher,
    /// Author
    Author,
}

/// Citation information.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Citation {
    /// Title
    pub title: String,
    /// Alternate title
    pub alternate_title: Option<String>,
    /// Date
    pub date: Vec<CitationDate>,
    /// Edition
    pub edition: Option<String>,
    /// Identifier
    pub identifier: Vec<String>,
    /// Cited responsible party
    pub cited_responsible_party: Vec<ResponsibleParty>,
}

/// Citation date.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CitationDate {
    /// Date
    pub date: chrono::DateTime<chrono::Utc>,
    /// Date type
    pub date_type: DateType,
}

/// Date type code.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DateType {
    /// Creation date
    Creation,
    /// Publication date
    Publication,
    /// Revision date
    Revision,
}

/// Distribution information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Distribution {
    /// Distribution format
    pub format: Vec<Format>,
    /// Distributor
    pub distributor: Vec<Distributor>,
    /// Transfer options
    pub transfer_options: Vec<TransferOptions>,
}

/// Format information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Format {
    /// Format name
    pub name: String,
    /// Version
    pub version: String,
}

/// Distributor information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Distributor {
    /// Distributor contact
    pub distributor_contact: ResponsibleParty,
    /// Distribution order process
    pub distribution_order_process: Vec<String>,
}

/// Transfer options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferOptions {
    /// Online resource
    pub online: Vec<OnlineResource>,
    /// Transfer size (MB)
    pub transfer_size: Option<f64>,
}

/// Online resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineResource {
    /// Linkage URL
    pub linkage: String,
    /// Protocol
    pub protocol: Option<String>,
    /// Name
    pub name: Option<String>,
    /// Description
    pub description: Option<String>,
    /// Function
    pub function: OnlineFunction,
}

/// Online function code.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum OnlineFunction {
    /// Download
    Download,
    /// Information
    Information,
    /// Offline access
    OfflineAccess,
    /// Order
    Order,
    /// Search
    Search,
}

/// Data quality information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataQuality {
    /// Scope
    pub scope: Scope,
    /// Lineage
    pub lineage: Option<Lineage>,
    /// Report
    pub report: Vec<QualityReport>,
}

/// Scope information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scope {
    /// Level
    pub level: HierarchyLevel,
}

/// Lineage information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lineage {
    /// Statement
    pub statement: String,
    /// Process step
    pub process_step: Vec<ProcessStep>,
    /// Source
    pub source: Vec<Source>,
}

/// Process step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessStep {
    /// Description
    pub description: String,
    /// Date/time
    pub date_time: Option<chrono::DateTime<chrono::Utc>>,
    /// Processor
    pub processor: Vec<ResponsibleParty>,
}

/// Source information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    /// Description
    pub description: String,
    /// Scale denominator
    pub scale_denominator: Option<i32>,
    /// Source citation
    pub source_citation: Option<Citation>,
}

/// Quality report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityReport {
    /// Measure identification
    pub measure_identification: String,
    /// Result
    pub result: String,
}
