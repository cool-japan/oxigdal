//! INSPIRE (Infrastructure for Spatial Information in the European Community) metadata.
//!
//! This module implements metadata support for the EU INSPIRE Directive,
//! which builds upon ISO 19115 with additional requirements for European
//! spatial data infrastructures.
//!
//! # Overview
//!
//! INSPIRE metadata extends ISO 19115 with specific requirements for:
//! - Resource locators
//! - Unique resource identifiers
//! - Coupled resources
//! - Spatial data service type
//! - Conformity to INSPIRE implementing rules
//!
//! # Examples
//!
//! ```no_run
//! use oxigdal_metadata::inspire::*;
//! use oxigdal_metadata::common::*;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let metadata = InspireMetadata::builder()
//!     .title("Elevation Data")
//!     .resource_type(ResourceType::Dataset)
//!     .topic_category(InspireTheme::Elevation)
//!     .build()?;
//! # Ok(())
//! # }
//! ```

use crate::error::{MetadataError, Result};
use crate::iso19115::{Iso19115Metadata, ResponsibleParty, Role};
use serde::{Deserialize, Serialize};

/// INSPIRE metadata record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspireMetadata {
    /// Base ISO 19115 metadata
    pub base: Iso19115Metadata,
    /// Resource locator
    pub resource_locator: Vec<ResourceLocator>,
    /// Unique resource identifier
    pub unique_resource_identifier: Vec<UniqueResourceIdentifier>,
    /// Coupled resource
    pub coupled_resource: Vec<CoupledResource>,
    /// Spatial data service type
    pub spatial_data_service_type: Option<SpatialDataServiceType>,
    /// Conformity
    pub conformity: Vec<Conformity>,
    /// Conditions for access and use
    pub conditions_for_access_and_use: Vec<String>,
    /// Limitations on public access
    pub limitations_on_public_access: Vec<String>,
    /// Responsible organisation
    pub responsible_organisation: Vec<ResponsibleParty>,
    /// Metadata point of contact
    pub metadata_point_of_contact: ResponsibleParty,
    /// Metadata date
    pub metadata_date: chrono::DateTime<chrono::Utc>,
    /// Metadata language
    pub metadata_language: String,
}

/// Builder for INSPIRE metadata.
pub struct InspireBuilder {
    metadata: InspireMetadata,
}

impl InspireMetadata {
    /// Create a new builder.
    pub fn builder() -> InspireBuilder {
        InspireBuilder {
            metadata: Self::default(),
        }
    }
}

impl Default for InspireMetadata {
    fn default() -> Self {
        Self {
            base: Iso19115Metadata::default(),
            resource_locator: Vec::new(),
            unique_resource_identifier: Vec::new(),
            coupled_resource: Vec::new(),
            spatial_data_service_type: None,
            conformity: Vec::new(),
            conditions_for_access_and_use: Vec::new(),
            limitations_on_public_access: Vec::new(),
            responsible_organisation: Vec::new(),
            metadata_point_of_contact: ResponsibleParty {
                individual_name: None,
                organization_name: None,
                position_name: None,
                contact_info: None,
                role: Role::PointOfContact,
            },
            metadata_date: chrono::Utc::now(),
            metadata_language: "eng".to_string(),
        }
    }
}

impl InspireBuilder {
    /// Set the title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.metadata.base = Iso19115Metadata::builder()
            .title(title)
            .build()
            .unwrap_or_default();
        self
    }

    /// Set the resource type.
    pub fn resource_type(self, _resource_type: ResourceType) -> Self {
        // Resource type handling would go here
        self
    }

    /// Set the INSPIRE theme.
    pub fn topic_category(self, _theme: InspireTheme) -> Self {
        // Theme handling would go here
        self
    }

    /// Add a resource locator.
    pub fn resource_locator(mut self, locator: ResourceLocator) -> Self {
        self.metadata.resource_locator.push(locator);
        self
    }

    /// Add a unique resource identifier.
    pub fn unique_identifier(mut self, identifier: UniqueResourceIdentifier) -> Self {
        self.metadata.unique_resource_identifier.push(identifier);
        self
    }

    /// Add conformity information.
    pub fn conformity(mut self, conformity: Conformity) -> Self {
        self.metadata.conformity.push(conformity);
        self
    }

    /// Build the metadata.
    pub fn build(self) -> Result<InspireMetadata> {
        if self.metadata.resource_locator.is_empty() {
            return Err(MetadataError::ValidationError(
                "At least one resource locator is required for INSPIRE".to_string(),
            ));
        }
        Ok(self.metadata)
    }
}

/// Resource locator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLocator {
    /// URL
    pub url: String,
    /// Description
    pub description: Option<String>,
    /// Function
    pub function: ResourceLocatorFunction,
}

/// Resource locator function.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ResourceLocatorFunction {
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

/// Unique resource identifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniqueResourceIdentifier {
    /// Code
    pub code: String,
    /// Code space
    pub code_space: Option<String>,
}

impl UniqueResourceIdentifier {
    /// Create a new unique resource identifier.
    pub fn new(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            code_space: None,
        }
    }

    /// Create with code space.
    pub fn with_code_space(code: impl Into<String>, code_space: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            code_space: Some(code_space.into()),
        }
    }
}

/// Coupled resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoupledResource {
    /// Operation name
    pub operation_name: String,
    /// Identifier
    pub identifier: String,
}

/// Spatial data service type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SpatialDataServiceType {
    /// Discovery
    Discovery,
    /// View
    View,
    /// Download
    Download,
    /// Transformation
    Transformation,
    /// Invoke
    Invoke,
    /// Other
    Other,
}

/// Conformity information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conformity {
    /// Specification
    pub specification: Specification,
    /// Degree
    pub degree: ConformityDegree,
}

/// Specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Specification {
    /// Title
    pub title: String,
    /// Date
    pub date: chrono::DateTime<chrono::Utc>,
    /// Date type
    pub date_type: DateType,
}

/// Date type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DateType {
    /// Creation
    Creation,
    /// Publication
    Publication,
    /// Revision
    Revision,
}

/// Conformity degree.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ConformityDegree {
    /// Conformant
    Conformant,
    /// Not conformant
    NotConformant,
    /// Not evaluated
    NotEvaluated,
}

/// INSPIRE themes (Annex I, II, III).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum InspireTheme {
    // Annex I
    /// Coordinate reference systems
    CoordinateReferenceSystems,
    /// Geographical grid systems
    GeographicalGridSystems,
    /// Geographical names
    GeographicalNames,
    /// Administrative units
    AdministrativeUnits,
    /// Addresses
    Addresses,
    /// Cadastral parcels
    CadastralParcels,
    /// Transport networks
    TransportNetworks,
    /// Hydrography
    Hydrography,
    /// Protected sites
    ProtectedSites,

    // Annex II
    /// Elevation
    Elevation,
    /// Land cover
    LandCover,
    /// Orthoimagery
    Orthoimagery,
    /// Geology
    Geology,

    // Annex III
    /// Statistical units
    StatisticalUnits,
    /// Buildings
    Buildings,
    /// Soil
    Soil,
    /// Land use
    LandUse,
    /// Human health and safety
    HumanHealthAndSafety,
    /// Utility and governmental services
    UtilityAndGovernmentalServices,
    /// Environmental monitoring facilities
    EnvironmentalMonitoringFacilities,
    /// Production and industrial facilities
    ProductionAndIndustrialFacilities,
    /// Agricultural and aquaculture facilities
    AgriculturalAndAquacultureFacilities,
    /// Population distribution and demography
    PopulationDistributionAndDemography,
    /// Area management/restriction/regulation zones
    AreaManagementRestrictionRegulationZones,
    /// Natural risk zones
    NaturalRiskZones,
    /// Atmospheric conditions
    AtmosphericConditions,
    /// Meteorological geographical features
    MeteorologicalGeographicalFeatures,
    /// Oceanographic geographical features
    OceanographicGeographicalFeatures,
    /// Sea regions
    SeaRegions,
    /// Bio-geographical regions
    BioGeographicalRegions,
    /// Habitats and biotopes
    HabitatsAndBiotopes,
    /// Species distribution
    SpeciesDistribution,
    /// Energy resources
    EnergyResources,
    /// Mineral resources
    MineralResources,
}

/// Resource type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ResourceType {
    /// Dataset
    Dataset,
    /// Dataset series
    DatasetSeries,
    /// Service
    Service,
}

/// Keyword value with origin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspireKeyword {
    /// Keyword value
    pub value: String,
    /// Originating controlled vocabulary
    pub originating_controlled_vocabulary: Option<String>,
}

impl InspireMetadata {
    /// Validate INSPIRE metadata completeness.
    pub fn validate(&self) -> Result<ValidationReport> {
        let mut report = ValidationReport::default();

        // Check mandatory elements
        if self.resource_locator.is_empty() {
            report
                .errors
                .push("Resource locator is mandatory".to_string());
        }

        if self.unique_resource_identifier.is_empty() {
            report
                .errors
                .push("Unique resource identifier is mandatory".to_string());
        }

        if self.conformity.is_empty() {
            report
                .warnings
                .push("Conformity information is recommended".to_string());
        }

        if self.responsible_organisation.is_empty() {
            report
                .errors
                .push("Responsible organisation is mandatory".to_string());
        }

        if self.conditions_for_access_and_use.is_empty() {
            report
                .errors
                .push("Conditions for access and use are mandatory".to_string());
        }

        if self.limitations_on_public_access.is_empty() {
            report
                .errors
                .push("Limitations on public access are mandatory".to_string());
        }

        report.is_valid = report.errors.is_empty();
        Ok(report)
    }
}

/// Validation report.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValidationReport {
    /// Is valid
    pub is_valid: bool,
    /// Errors
    pub errors: Vec<String>,
    /// Warnings
    pub warnings: Vec<String>,
}

#[cfg(feature = "xml")]
impl InspireMetadata {
    /// Export to XML format.
    pub fn to_xml(&self) -> Result<String> {
        use quick_xml::se::to_string;
        to_string(&self).map_err(|e| MetadataError::XmlError(e.to_string()))
    }

    /// Import from XML format.
    pub fn from_xml(xml: &str) -> Result<Self> {
        use quick_xml::de::from_str;
        from_str(xml).map_err(|e| MetadataError::XmlError(e.to_string()))
    }
}

impl InspireMetadata {
    /// Export to JSON format.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(&self).map_err(|e| MetadataError::JsonError(e.to_string()))
    }

    /// Import from JSON format.
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| MetadataError::JsonError(e.to_string()))
    }
}
