//! FGDC (Federal Geographic Data Committee) metadata standard.
//!
//! This module implements the FGDC-STD-001-1998 Content Standard for
//! Digital Geospatial Metadata (CSDGM).
//!
//! # Overview
//!
//! The FGDC metadata standard is widely used in the United States for
//! documenting geospatial data. It provides a comprehensive structure
//! for describing data identification, quality, organization, spatial
//! reference, and distribution.
//!
//! # Examples
//!
//! ```no_run
//! use oxigdal_metadata::fgdc::*;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let metadata = FgdcMetadata::builder()
//!     .title("Digital Elevation Model")
//!     .abstract_text("30-meter resolution DEM")
//!     .purpose("Terrain analysis and visualization")
//!     .build()?;
//! # Ok(())
//! # }
//! ```

use crate::common::{BoundingBox, ContactInfo};
use crate::error::{MetadataError, Result};
use serde::{Deserialize, Serialize};

/// Complete FGDC metadata record.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FgdcMetadata {
    /// Identification information
    pub idinfo: IdentificationInfo,
    /// Data quality information
    pub dataqual: Option<DataQualityInfo>,
    /// Spatial data organization information
    pub spdoinfo: Option<SpatialDataOrganizationInfo>,
    /// Spatial reference information
    pub spref: Option<SpatialReferenceInfo>,
    /// Entity and attribute information
    pub eainfo: Option<EntityAttributeInfo>,
    /// Distribution information
    pub distinfo: Option<DistributionInfo>,
    /// Metadata reference information
    pub metainfo: MetadataReferenceInfo,
}

/// Builder for FGDC metadata.
pub struct FgdcBuilder {
    metadata: FgdcMetadata,
}

impl FgdcMetadata {
    /// Create a new builder.
    pub fn builder() -> FgdcBuilder {
        FgdcBuilder {
            metadata: Self::default(),
        }
    }
}

impl FgdcBuilder {
    /// Set the title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.metadata.idinfo.citation.citeinfo.title = title.into();
        self
    }

    /// Set the abstract.
    pub fn abstract_text(mut self, abstract_text: impl Into<String>) -> Self {
        self.metadata.idinfo.descript.abstract_text = abstract_text.into();
        self
    }

    /// Set the purpose.
    pub fn purpose(mut self, purpose: impl Into<String>) -> Self {
        self.metadata.idinfo.descript.purpose = Some(purpose.into());
        self
    }

    /// Set the bounding box.
    pub fn bbox(mut self, bbox: BoundingBox) -> Self {
        self.metadata.idinfo.spdom.bounding = bbox;
        self
    }

    /// Add keywords.
    pub fn keywords(mut self, theme: impl Into<String>, keywords: Vec<impl Into<String>>) -> Self {
        let kw = Keywords {
            theme: Some(theme.into()),
            theme_key: keywords.into_iter().map(|k| k.into()).collect(),
            place: Vec::new(),
            temporal: Vec::new(),
        };
        self.metadata.idinfo.keywords.push(kw);
        self
    }

    /// Build the metadata.
    pub fn build(self) -> Result<FgdcMetadata> {
        if self.metadata.idinfo.citation.citeinfo.title.is_empty() {
            return Err(MetadataError::MissingField("title".to_string()));
        }
        Ok(self.metadata)
    }
}

/// Identification information.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IdentificationInfo {
    /// Citation
    pub citation: Citation,
    /// Description
    pub descript: Description,
    /// Time period
    pub timeperd: TimePeriod,
    /// Status
    pub status: Status,
    /// Spatial domain
    pub spdom: SpatialDomain,
    /// Keywords
    pub keywords: Vec<Keywords>,
    /// Access constraints
    pub accconst: Option<String>,
    /// Use constraints
    pub useconst: Option<String>,
    /// Point of contact
    pub ptcontac: Option<Contact>,
    /// Browse graphic
    pub browse: Vec<BrowseGraphic>,
    /// Native data set environment
    pub native: Option<String>,
}

/// Citation information.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Citation {
    /// Citation info
    pub citeinfo: CiteInfo,
}

/// Citation info.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CiteInfo {
    /// Originator
    pub origin: Vec<String>,
    /// Publication date
    pub pubdate: Option<String>,
    /// Title
    pub title: String,
    /// Edition
    pub edition: Option<String>,
    /// Geospatial data presentation form
    pub geoform: Option<String>,
    /// Series information
    pub serinfo: Option<SeriesInfo>,
    /// Publication information
    pub pubinfo: Option<PublicationInfo>,
    /// Online linkage
    pub onlink: Vec<String>,
}

/// Series information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesInfo {
    /// Series name
    pub sername: String,
    /// Issue identification
    pub issue: Option<String>,
}

/// Publication information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicationInfo {
    /// Publication place
    pub pubplace: String,
    /// Publisher
    pub publish: String,
}

/// Description.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Description {
    /// Abstract
    pub abstract_text: String,
    /// Purpose
    pub purpose: Option<String>,
    /// Supplemental information
    pub supplinf: Option<String>,
}

/// Time period.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimePeriod {
    /// Time period info
    pub timeinfo: TimeInfo,
    /// Current reference
    pub current: String,
}

impl Default for TimePeriod {
    fn default() -> Self {
        Self {
            timeinfo: TimeInfo::Single {
                caldate: chrono::Utc::now().format("%Y%m%d").to_string(),
            },
            current: "ground condition".to_string(),
        }
    }
}

/// Time information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TimeInfo {
    /// Single date/time
    Single {
        /// Calendar date
        caldate: String,
    },
    /// Multiple dates/times
    Multiple {
        /// Calendar dates
        caldate: Vec<String>,
    },
    /// Range of dates/times
    Range {
        /// Beginning date
        begdate: String,
        /// Ending date
        enddate: String,
    },
}

/// Status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    /// Progress
    pub progress: Progress,
    /// Maintenance and update frequency
    pub update: MaintenanceFrequency,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            progress: Progress::Complete,
            update: MaintenanceFrequency::AsNeeded,
        }
    }
}

/// Progress code.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Progress {
    /// Complete
    Complete,
    /// In work
    InWork,
    /// Planned
    Planned,
}

/// Maintenance frequency.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MaintenanceFrequency {
    /// Continually
    Continually,
    /// Daily
    Daily,
    /// Weekly
    Weekly,
    /// Monthly
    Monthly,
    /// Annually
    Annually,
    /// Unknown
    Unknown,
    /// As needed
    AsNeeded,
    /// Irregular
    Irregular,
    /// None planned
    NonePlanned,
}

/// Spatial domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpatialDomain {
    /// Bounding coordinates
    pub bounding: BoundingBox,
}

impl Default for SpatialDomain {
    fn default() -> Self {
        Self {
            bounding: BoundingBox::new(-180.0, 180.0, -90.0, 90.0),
        }
    }
}

/// Keywords.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Keywords {
    /// Theme
    pub theme: Option<String>,
    /// Theme keywords
    pub theme_key: Vec<String>,
    /// Place keywords
    pub place: Vec<String>,
    /// Temporal keywords
    pub temporal: Vec<String>,
}

/// Contact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    /// Contact information
    pub cntinfo: ContactInfo,
}

/// Browse graphic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseGraphic {
    /// Browse file name
    pub browsen: String,
    /// Browse file description
    pub browsed: String,
    /// Browse file type
    pub browset: String,
}

/// Data quality information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataQualityInfo {
    /// Attribute accuracy
    pub attracc: Option<AttributeAccuracy>,
    /// Logical consistency
    pub logic: Option<String>,
    /// Completeness
    pub complete: Option<String>,
    /// Positional accuracy
    pub posacc: Option<PositionalAccuracy>,
    /// Lineage
    pub lineage: Lineage,
}

/// Attribute accuracy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeAccuracy {
    /// Attribute accuracy report
    pub attraccr: String,
}

/// Positional accuracy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionalAccuracy {
    /// Horizontal positional accuracy
    pub horizpa: Option<HorizontalPositionalAccuracy>,
    /// Vertical positional accuracy
    pub vertacc: Option<VerticalPositionalAccuracy>,
}

/// Horizontal positional accuracy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HorizontalPositionalAccuracy {
    /// Horizontal positional accuracy report
    pub horizpar: String,
}

/// Vertical positional accuracy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerticalPositionalAccuracy {
    /// Vertical positional accuracy report
    pub vertaccr: String,
}

/// Lineage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lineage {
    /// Source information
    pub srcinfo: Vec<SourceInfo>,
    /// Process step
    pub procstep: Vec<ProcessStep>,
}

/// Source information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceInfo {
    /// Source citation
    pub srccite: Citation,
    /// Source scale denominator
    pub srcscale: Option<i32>,
    /// Type of source media
    pub typesrc: Option<String>,
    /// Source contribution
    pub srccontr: String,
}

/// Process step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessStep {
    /// Process description
    pub procdesc: String,
    /// Process date
    pub procdate: String,
}

/// Spatial data organization information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpatialDataOrganizationInfo {
    /// Indirect spatial reference
    pub indspref: Option<String>,
    /// Direct spatial reference method
    pub direct: DirectSpatialReferenceMethod,
}

/// Direct spatial reference method.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DirectSpatialReferenceMethod {
    /// Point
    Point,
    /// Vector
    Vector,
    /// Raster
    Raster,
}

/// Spatial reference information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpatialReferenceInfo {
    /// Horizontal coordinate system definition
    pub horizsys: Option<HorizontalCoordinateSystem>,
    /// Vertical coordinate system definition
    pub vertdef: Option<VerticalCoordinateSystem>,
}

/// Horizontal coordinate system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HorizontalCoordinateSystem {
    /// Geographic
    Geographic {
        /// Latitude resolution
        latres: f64,
        /// Longitude resolution
        longres: f64,
        /// Geodetic model
        geodetic: GeodeticModel,
    },
    /// Planar
    Planar {
        /// Map projection
        mapproj: Option<String>,
        /// Grid coordinate system
        gridsys: Option<String>,
    },
}

/// Geodetic model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeodeticModel {
    /// Horizontal datum name
    pub horizdn: String,
    /// Ellipsoid name
    pub ellips: String,
    /// Semi-major axis
    pub semiaxis: f64,
    /// Denominator of flattening ratio
    pub denflat: f64,
}

/// Vertical coordinate system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerticalCoordinateSystem {
    /// Altitude system definition
    pub altsys: AltitudeSystem,
}

/// Altitude system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AltitudeSystem {
    /// Altitude datum name
    pub altdatum: String,
    /// Altitude resolution
    pub altres: f64,
    /// Altitude units
    pub altunits: String,
    /// Altitude encoding method
    pub altenc: String,
}

/// Entity and attribute information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityAttributeInfo {
    /// Detailed description
    pub detailed: Vec<DetailedDescription>,
    /// Overview description
    pub overview: Option<OverviewDescription>,
}

/// Detailed description.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetailedDescription {
    /// Entity type
    pub enttyp: EntityType,
    /// Attributes
    pub attr: Vec<Attribute>,
}

/// Entity type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityType {
    /// Entity type label
    pub enttypl: String,
    /// Entity type definition
    pub enttypd: String,
    /// Entity type definition source
    pub enttypds: String,
}

/// Attribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attribute {
    /// Attribute label
    pub attrlabl: String,
    /// Attribute definition
    pub attrdef: String,
    /// Attribute definition source
    pub attrdefs: String,
}

/// Overview description.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverviewDescription {
    /// Entity and attribute overview
    pub eaover: String,
    /// Entity and attribute detail citation
    pub eadetcit: String,
}

/// Distribution information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributionInfo {
    /// Distributor
    pub distrib: Distributor,
    /// Standard order process
    pub stdorder: Vec<StandardOrderProcess>,
}

/// Distributor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Distributor {
    /// Contact information
    pub cntinfo: ContactInfo,
}

/// Standard order process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandardOrderProcess {
    /// Non-digital form
    pub nondig: Option<String>,
    /// Digital form
    pub digform: Vec<DigitalForm>,
    /// Fees
    pub fees: Option<String>,
}

/// Digital form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DigitalForm {
    /// Digital transfer information
    pub digtinfo: DigitalTransferInfo,
    /// Digital transfer option
    pub digtopt: DigitalTransferOption,
}

/// Digital transfer information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DigitalTransferInfo {
    /// Format name
    pub formname: String,
    /// Format version number
    pub formvern: Option<String>,
    /// Transfer size
    pub transize: Option<f64>,
}

/// Digital transfer option.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DigitalTransferOption {
    /// Online option
    pub onlinopt: Vec<OnlineOption>,
}

/// Online option.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineOption {
    /// Computer contact information
    pub computer: ComputerContactInfo,
}

/// Computer contact information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerContactInfo {
    /// Network address
    pub networka: NetworkAddress,
}

/// Network address.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkAddress {
    /// Network resource name
    pub networkr: String,
}

/// Metadata reference information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataReferenceInfo {
    /// Metadata date
    pub metd: String,
    /// Metadata contact
    pub metc: Contact,
    /// Metadata standard name
    pub metstdn: String,
    /// Metadata standard version
    pub metstdv: String,
}

impl Default for MetadataReferenceInfo {
    fn default() -> Self {
        Self {
            metd: chrono::Utc::now().format("%Y%m%d").to_string(),
            metc: Contact {
                cntinfo: ContactInfo {
                    individual_name: None,
                    organization_name: None,
                    position_name: None,
                    email: None,
                    phone: None,
                    address: None,
                    online_resource: None,
                },
            },
            metstdn: "FGDC Content Standard for Digital Geospatial Metadata".to_string(),
            metstdv: "FGDC-STD-001-1998".to_string(),
        }
    }
}

#[cfg(feature = "xml")]
impl FgdcMetadata {
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

impl FgdcMetadata {
    /// Export to JSON format.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(&self).map_err(|e| MetadataError::JsonError(e.to_string()))
    }

    /// Import from JSON format.
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| MetadataError::JsonError(e.to_string()))
    }
}
