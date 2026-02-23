//! Core metadata elements for ISO 19115.

use super::*;
use crate::common::{BoundingBox, Keyword, TemporalExtent};
use serde::{Deserialize, Serialize};

/// Data identification information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataIdentification {
    /// Citation
    pub citation: Citation,
    /// Abstract
    pub abstract_text: String,
    /// Purpose
    pub purpose: Option<String>,
    /// Credit
    pub credit: Vec<String>,
    /// Status
    pub status: Vec<ProgressCode>,
    /// Point of contact
    pub point_of_contact: Vec<ResponsibleParty>,
    /// Resource maintenance
    pub resource_maintenance: Option<MaintenanceInformation>,
    /// Descriptive keywords
    pub keywords: Vec<Vec<Keyword>>,
    /// Resource constraints
    pub resource_constraints: Vec<Constraints>,
    /// Spatial representation type
    pub spatial_representation_type: Vec<SpatialRepresentationType>,
    /// Spatial resolution
    pub spatial_resolution: Vec<Resolution>,
    /// Language
    pub language: Vec<String>,
    /// Character set
    pub character_set: Vec<CharacterSet>,
    /// Topic category
    pub topic_category: Vec<TopicCategory>,
    /// Extent
    pub extent: Extent,
}

impl Default for DataIdentification {
    fn default() -> Self {
        Self {
            citation: Citation::default(),
            abstract_text: String::new(),
            purpose: None,
            credit: Vec::new(),
            status: Vec::new(),
            point_of_contact: Vec::new(),
            resource_maintenance: None,
            keywords: Vec::new(),
            resource_constraints: Vec::new(),
            spatial_representation_type: Vec::new(),
            spatial_resolution: Vec::new(),
            language: vec!["eng".to_string()],
            character_set: vec![CharacterSet::Utf8],
            topic_category: Vec::new(),
            extent: Extent::default(),
        }
    }
}

/// Progress code.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ProgressCode {
    /// Completed
    Completed,
    /// Historical archive
    HistoricalArchive,
    /// Obsolete
    Obsolete,
    /// Ongoing
    Ongoing,
    /// Planned
    Planned,
    /// Required
    Required,
    /// Under development
    UnderDevelopment,
}

/// Maintenance information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceInformation {
    /// Maintenance and update frequency
    pub maintenance_frequency: MaintenanceFrequency,
    /// Date of next update
    pub date_of_next_update: Option<chrono::DateTime<chrono::Utc>>,
    /// User defined maintenance frequency
    pub user_defined_frequency: Option<String>,
    /// Maintenance note
    pub maintenance_note: Vec<String>,
}

/// Maintenance frequency code.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MaintenanceFrequency {
    /// Continually
    Continual,
    /// Daily
    Daily,
    /// Weekly
    Weekly,
    /// Fortnightly
    Fortnightly,
    /// Monthly
    Monthly,
    /// Quarterly
    Quarterly,
    /// Biannually
    Biannually,
    /// Annually
    Annually,
    /// As needed
    AsNeeded,
    /// Irregular
    Irregular,
    /// Not planned
    NotPlanned,
    /// Unknown
    Unknown,
}

/// Resource constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Constraints {
    /// Legal constraints
    Legal(LegalConstraints),
    /// Security constraints
    Security(SecurityConstraints),
    /// General constraints
    General(GeneralConstraints),
}

/// Legal constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegalConstraints {
    /// Access constraints
    pub access_constraints: Vec<RestrictionCode>,
    /// Use constraints
    pub use_constraints: Vec<RestrictionCode>,
    /// Other constraints
    pub other_constraints: Vec<String>,
    /// Use limitation
    pub use_limitation: Vec<String>,
}

/// Security constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConstraints {
    /// Classification
    pub classification: ClassificationCode,
    /// User note
    pub user_note: Option<String>,
    /// Classification system
    pub classification_system: Option<String>,
    /// Handling description
    pub handling_description: Option<String>,
}

/// General constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConstraints {
    /// Use limitation
    pub use_limitation: Vec<String>,
}

/// Restriction code.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum RestrictionCode {
    /// Copyright
    Copyright,
    /// Patent
    Patent,
    /// Patent pending
    PatentPending,
    /// Trademark
    Trademark,
    /// License
    License,
    /// Intellectual property rights
    IntellectualPropertyRights,
    /// Restricted
    Restricted,
    /// Other restrictions
    OtherRestrictions,
}

/// Classification code.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ClassificationCode {
    /// Unclassified
    Unclassified,
    /// Restricted
    Restricted,
    /// Confidential
    Confidential,
    /// Secret
    Secret,
    /// Top secret
    TopSecret,
}

/// Spatial representation type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SpatialRepresentationType {
    /// Vector
    Vector,
    /// Grid
    Grid,
    /// Text table
    TextTable,
    /// TIN
    Tin,
    /// Stereo model
    StereoModel,
    /// Video
    Video,
}

/// Resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Resolution {
    /// Distance
    Distance(f64),
    /// Scale
    Scale(i32),
}

/// Topic category code.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TopicCategory {
    /// Farming
    Farming,
    /// Biota
    Biota,
    /// Boundaries
    Boundaries,
    /// Climatology meteorology atmosphere
    ClimatologyMeteorologyAtmosphere,
    /// Economy
    Economy,
    /// Elevation
    Elevation,
    /// Environment
    Environment,
    /// Geoscientific information
    GeoscientificInformation,
    /// Health
    Health,
    /// Imagery base maps earth cover
    ImageryBaseMapsEarthCover,
    /// Intelligence military
    IntelligenceMilitary,
    /// Inland waters
    InlandWaters,
    /// Location
    Location,
    /// Oceans
    Oceans,
    /// Planning cadastre
    PlanningCadastre,
    /// Society
    Society,
    /// Structure
    Structure,
    /// Transportation
    Transportation,
    /// Utilities communication
    UtilitiesCommunication,
}

/// Extent information.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Extent {
    /// Description
    pub description: Option<String>,
    /// Geographic extent
    pub geographic_extent: Option<BoundingBox>,
    /// Temporal extent
    pub temporal_extent: Option<TemporalExtent>,
    /// Vertical extent
    pub vertical_extent: Option<VerticalExtent>,
}

/// Vertical extent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerticalExtent {
    /// Minimum value
    pub minimum_value: f64,
    /// Maximum value
    pub maximum_value: f64,
    /// Unit of measure
    pub unit_of_measure: String,
    /// Vertical CRS
    pub vertical_crs: Option<String>,
}

#[cfg(feature = "xml")]
impl Iso19115Metadata {
    /// Export to ISO 19139 XML format.
    pub fn to_xml(&self) -> Result<String> {
        use quick_xml::se::to_string;
        to_string(&self).map_err(|e| MetadataError::XmlError(e.to_string()))
    }

    /// Import from ISO 19139 XML format.
    pub fn from_xml(xml: &str) -> Result<Self> {
        use quick_xml::de::from_str;
        from_str(xml).map_err(|e| MetadataError::XmlError(e.to_string()))
    }
}

impl Iso19115Metadata {
    /// Export to JSON format.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(&self).map_err(|e| MetadataError::JsonError(e.to_string()))
    }

    /// Import from JSON format.
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| MetadataError::JsonError(e.to_string()))
    }
}
