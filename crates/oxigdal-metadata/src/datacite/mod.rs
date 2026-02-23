//! DataCite metadata for research data DOIs.
//!
//! This module implements the DataCite Metadata Schema for publication
//! and citation of research data and other resources.
//!
//! # Overview
//!
//! DataCite provides a standard for describing research data with
//! Digital Object Identifiers (DOIs). The schema includes:
//! - Resource identification
//! - Creators and contributors
//! - Publication information
//! - Subject and classification
//! - Rights and licensing
//!
//! # Examples
//!
//! ```no_run
//! use oxigdal_metadata::datacite::*;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let metadata = DataCiteMetadata::builder()
//!     .identifier("10.5281/zenodo.123456", IdentifierType::Doi)
//!     .title("Research Dataset")
//!     .creator(Creator::new("Smith, John"))
//!     .publisher("Zenodo")
//!     .publication_year(2024)
//!     .resource_type(ResourceTypeGeneral::Dataset)
//!     .build()?;
//! # Ok(())
//! # }
//! ```

use crate::error::{MetadataError, Result};
use serde::{Deserialize, Serialize};

/// DataCite metadata record (Schema 4.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataCiteMetadata {
    /// Identifier (mandatory)
    pub identifier: Identifier,
    /// Creators (mandatory)
    pub creators: Vec<Creator>,
    /// Titles (mandatory)
    pub titles: Vec<Title>,
    /// Publisher (mandatory)
    pub publisher: String,
    /// Publication year (mandatory)
    pub publication_year: u16,
    /// Resource type (mandatory)
    pub resource_type: ResourceType,
    /// Subjects (recommended)
    pub subjects: Vec<Subject>,
    /// Contributors (optional)
    pub contributors: Vec<Contributor>,
    /// Dates (recommended)
    pub dates: Vec<Date>,
    /// Language (optional)
    pub language: Option<String>,
    /// Alternate identifiers (optional)
    pub alternate_identifiers: Vec<AlternateIdentifier>,
    /// Related identifiers (recommended)
    pub related_identifiers: Vec<RelatedIdentifier>,
    /// Sizes (optional)
    pub sizes: Vec<String>,
    /// Formats (optional)
    pub formats: Vec<String>,
    /// Version (optional)
    pub version: Option<String>,
    /// Rights (optional)
    pub rights_list: Vec<Rights>,
    /// Descriptions (recommended)
    pub descriptions: Vec<Description>,
    /// Geo locations (recommended)
    pub geo_locations: Vec<GeoLocation>,
    /// Funding references (optional)
    pub funding_references: Vec<FundingReference>,
}

/// Builder for DataCite metadata.
pub struct DataCiteBuilder {
    identifier: Option<Identifier>,
    creators: Vec<Creator>,
    titles: Vec<Title>,
    publisher: Option<String>,
    publication_year: Option<u16>,
    resource_type: Option<ResourceType>,
    subjects: Vec<Subject>,
    contributors: Vec<Contributor>,
    dates: Vec<Date>,
    language: Option<String>,
    alternate_identifiers: Vec<AlternateIdentifier>,
    related_identifiers: Vec<RelatedIdentifier>,
    sizes: Vec<String>,
    formats: Vec<String>,
    version: Option<String>,
    rights_list: Vec<Rights>,
    descriptions: Vec<Description>,
    geo_locations: Vec<GeoLocation>,
    funding_references: Vec<FundingReference>,
}

impl DataCiteMetadata {
    /// Create a new builder.
    pub fn builder() -> DataCiteBuilder {
        DataCiteBuilder {
            identifier: None,
            creators: Vec::new(),
            titles: Vec::new(),
            publisher: None,
            publication_year: None,
            resource_type: None,
            subjects: Vec::new(),
            contributors: Vec::new(),
            dates: Vec::new(),
            language: None,
            alternate_identifiers: Vec::new(),
            related_identifiers: Vec::new(),
            sizes: Vec::new(),
            formats: Vec::new(),
            version: None,
            rights_list: Vec::new(),
            descriptions: Vec::new(),
            geo_locations: Vec::new(),
            funding_references: Vec::new(),
        }
    }
}

impl DataCiteBuilder {
    /// Set the identifier.
    pub fn identifier(mut self, identifier: impl Into<String>, id_type: IdentifierType) -> Self {
        self.identifier = Some(Identifier {
            identifier: identifier.into(),
            identifier_type: id_type,
        });
        self
    }

    /// Add a creator.
    pub fn creator(mut self, creator: Creator) -> Self {
        self.creators.push(creator);
        self
    }

    /// Add a title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.titles.push(Title {
            title: title.into(),
            title_type: None,
            lang: None,
        });
        self
    }

    /// Set the publisher.
    pub fn publisher(mut self, publisher: impl Into<String>) -> Self {
        self.publisher = Some(publisher.into());
        self
    }

    /// Set the publication year.
    pub fn publication_year(mut self, year: u16) -> Self {
        self.publication_year = Some(year);
        self
    }

    /// Set the resource type.
    pub fn resource_type(mut self, resource_type: ResourceTypeGeneral) -> Self {
        self.resource_type = Some(ResourceType {
            resource_type_general: resource_type,
            resource_type: None,
        });
        self
    }

    /// Add a subject.
    pub fn subject(mut self, subject: Subject) -> Self {
        self.subjects.push(subject);
        self
    }

    /// Add a description.
    pub fn description(mut self, description: Description) -> Self {
        self.descriptions.push(description);
        self
    }

    /// Add rights information.
    pub fn rights(mut self, rights: Rights) -> Self {
        self.rights_list.push(rights);
        self
    }

    /// Build the metadata.
    pub fn build(self) -> Result<DataCiteMetadata> {
        let identifier = self
            .identifier
            .ok_or_else(|| MetadataError::MissingField("identifier".to_string()))?;

        if self.creators.is_empty() {
            return Err(MetadataError::MissingField("creators".to_string()));
        }

        if self.titles.is_empty() {
            return Err(MetadataError::MissingField("titles".to_string()));
        }

        let publisher = self
            .publisher
            .ok_or_else(|| MetadataError::MissingField("publisher".to_string()))?;

        let publication_year = self
            .publication_year
            .ok_or_else(|| MetadataError::MissingField("publication_year".to_string()))?;

        let resource_type = self
            .resource_type
            .ok_or_else(|| MetadataError::MissingField("resource_type".to_string()))?;

        Ok(DataCiteMetadata {
            identifier,
            creators: self.creators,
            titles: self.titles,
            publisher,
            publication_year,
            resource_type,
            subjects: self.subjects,
            contributors: self.contributors,
            dates: self.dates,
            language: self.language,
            alternate_identifiers: self.alternate_identifiers,
            related_identifiers: self.related_identifiers,
            sizes: self.sizes,
            formats: self.formats,
            version: self.version,
            rights_list: self.rights_list,
            descriptions: self.descriptions,
            geo_locations: self.geo_locations,
            funding_references: self.funding_references,
        })
    }
}

/// Identifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identifier {
    /// Identifier value
    pub identifier: String,
    /// Identifier type
    pub identifier_type: IdentifierType,
}

/// Identifier type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum IdentifierType {
    /// ARK
    ARK,
    /// arXiv
    ArXiv,
    /// Bibcode
    Bibcode,
    /// DOI
    #[serde(rename = "DOI")]
    Doi,
    /// EAN13
    EAN13,
    /// EISSN
    EISSN,
    /// Handle
    Handle,
    /// IGSN
    IGSN,
    /// ISBN
    ISBN,
    /// ISSN
    ISSN,
    /// ISTC
    ISTC,
    /// LISSN
    LISSN,
    /// LSID
    LSID,
    /// PMID
    PMID,
    /// PURL
    PURL,
    /// UPC
    UPC,
    /// URL
    URL,
    /// URN
    URN,
    /// Other
    Other,
}

/// Creator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Creator {
    /// Creator name (mandatory)
    pub name: String,
    /// Name type
    pub name_type: Option<NameType>,
    /// Given name
    pub given_name: Option<String>,
    /// Family name
    pub family_name: Option<String>,
    /// Name identifiers
    pub name_identifiers: Vec<NameIdentifier>,
    /// Affiliations
    pub affiliations: Vec<String>,
}

impl Creator {
    /// Create a new creator.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            name_type: Some(NameType::Personal),
            given_name: None,
            family_name: None,
            name_identifiers: Vec::new(),
            affiliations: Vec::new(),
        }
    }

    /// Create an organizational creator.
    pub fn organization(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            name_type: Some(NameType::Organizational),
            given_name: None,
            family_name: None,
            name_identifiers: Vec::new(),
            affiliations: Vec::new(),
        }
    }
}

/// Name type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum NameType {
    /// Organizational
    Organizational,
    /// Personal
    Personal,
}

/// Name identifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameIdentifier {
    /// Name identifier
    pub name_identifier: String,
    /// Name identifier scheme
    pub name_identifier_scheme: String,
    /// Scheme URI
    pub scheme_uri: Option<String>,
}

/// Title.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Title {
    /// Title text
    pub title: String,
    /// Title type
    pub title_type: Option<TitleType>,
    /// Language
    pub lang: Option<String>,
}

/// Title type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TitleType {
    /// Alternative title
    AlternativeTitle,
    /// Subtitle
    Subtitle,
    /// Translated title
    TranslatedTitle,
    /// Other
    Other,
}

/// Resource type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceType {
    /// Resource type general (mandatory)
    pub resource_type_general: ResourceTypeGeneral,
    /// Free text resource type
    pub resource_type: Option<String>,
}

/// Resource type general.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ResourceTypeGeneral {
    /// Audiovisual
    Audiovisual,
    /// Book
    Book,
    /// Book chapter
    BookChapter,
    /// Collection
    Collection,
    /// Computational notebook
    ComputationalNotebook,
    /// Conference paper
    ConferencePaper,
    /// Conference proceeding
    ConferenceProceeding,
    /// Data paper
    DataPaper,
    /// Dataset
    Dataset,
    /// Dissertation
    Dissertation,
    /// Event
    Event,
    /// Image
    Image,
    /// Interactive resource
    InteractiveResource,
    /// Journal
    Journal,
    /// Journal article
    JournalArticle,
    /// Model
    Model,
    /// Output management plan
    OutputManagementPlan,
    /// Peer review
    PeerReview,
    /// Physical object
    PhysicalObject,
    /// Preprint
    Preprint,
    /// Report
    Report,
    /// Service
    Service,
    /// Software
    Software,
    /// Sound
    Sound,
    /// Standard
    Standard,
    /// Text
    Text,
    /// Workflow
    Workflow,
    /// Other
    Other,
}

/// Subject.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subject {
    /// Subject text
    pub subject: String,
    /// Subject scheme
    pub subject_scheme: Option<String>,
    /// Scheme URI
    pub scheme_uri: Option<String>,
    /// Value URI
    pub value_uri: Option<String>,
    /// Classification code
    pub classification_code: Option<String>,
}

impl Subject {
    /// Create a new subject.
    pub fn new(subject: impl Into<String>) -> Self {
        Self {
            subject: subject.into(),
            subject_scheme: None,
            scheme_uri: None,
            value_uri: None,
            classification_code: None,
        }
    }
}

/// Contributor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contributor {
    /// Contributor name
    pub name: String,
    /// Contributor type
    pub contributor_type: ContributorType,
    /// Name type
    pub name_type: Option<NameType>,
    /// Given name
    pub given_name: Option<String>,
    /// Family name
    pub family_name: Option<String>,
    /// Name identifiers
    pub name_identifiers: Vec<NameIdentifier>,
    /// Affiliations
    pub affiliations: Vec<String>,
}

/// Contributor type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ContributorType {
    /// Contact person
    ContactPerson,
    /// Data collector
    DataCollector,
    /// Data curator
    DataCurator,
    /// Data manager
    DataManager,
    /// Distributor
    Distributor,
    /// Editor
    Editor,
    /// Hosting institution
    HostingInstitution,
    /// Producer
    Producer,
    /// Project leader
    ProjectLeader,
    /// Project manager
    ProjectManager,
    /// Project member
    ProjectMember,
    /// Registration agency
    RegistrationAgency,
    /// Registration authority
    RegistrationAuthority,
    /// Related person
    RelatedPerson,
    /// Researcher
    Researcher,
    /// Research group
    ResearchGroup,
    /// Rights holder
    RightsHolder,
    /// Sponsor
    Sponsor,
    /// Supervisor
    Supervisor,
    /// Work package leader
    WorkPackageLeader,
    /// Other
    Other,
}

/// Date.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Date {
    /// Date value
    pub date: String,
    /// Date type
    pub date_type: DateType,
    /// Date information
    pub date_information: Option<String>,
}

/// Date type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DateType {
    /// Accepted
    Accepted,
    /// Available
    Available,
    /// Collected
    Collected,
    /// Copyrighted
    Copyrighted,
    /// Created
    Created,
    /// Issued
    Issued,
    /// Submitted
    Submitted,
    /// Updated
    Updated,
    /// Valid
    Valid,
    /// Withdrawn
    Withdrawn,
    /// Other
    Other,
}

/// Alternate identifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlternateIdentifier {
    /// Alternate identifier
    pub alternate_identifier: String,
    /// Alternate identifier type
    pub alternate_identifier_type: String,
}

/// Related identifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedIdentifier {
    /// Related identifier
    pub related_identifier: String,
    /// Related identifier type
    pub related_identifier_type: IdentifierType,
    /// Relation type
    pub relation_type: RelationType,
    /// Related metadata scheme
    pub related_metadata_scheme: Option<String>,
    /// Scheme URI
    pub scheme_uri: Option<String>,
    /// Scheme type
    pub scheme_type: Option<String>,
    /// Resource type general
    pub resource_type_general: Option<ResourceTypeGeneral>,
}

/// Relation type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum RelationType {
    /// Is cited by
    IsCitedBy,
    /// Cites
    Cites,
    /// Is supplement to
    IsSupplementTo,
    /// Is supplemented by
    IsSupplementedBy,
    /// Is continued by
    IsContinuedBy,
    /// Continues
    Continues,
    /// Describes
    Describes,
    /// Is described by
    IsDescribedBy,
    /// Has metadata
    HasMetadata,
    /// Is metadata for
    IsMetadataFor,
    /// Has version
    HasVersion,
    /// Is version of
    IsVersionOf,
    /// Is new version of
    IsNewVersionOf,
    /// Is previous version of
    IsPreviousVersionOf,
    /// Is part of
    IsPartOf,
    /// Has part
    HasPart,
    /// Is referenced by
    IsReferencedBy,
    /// References
    References,
    /// Is documented by
    IsDocumentedBy,
    /// Documents
    Documents,
    /// Is compiled by
    IsCompiledBy,
    /// Compiles
    Compiles,
    /// Is variant form of
    IsVariantFormOf,
    /// Is original form of
    IsOriginalFormOf,
    /// Is identical to
    IsIdenticalTo,
    /// Is reviewed by
    IsReviewedBy,
    /// Reviews
    Reviews,
    /// Is derived from
    IsDerivedFrom,
    /// Is source of
    IsSourceOf,
    /// Is required by
    IsRequiredBy,
    /// Requires
    Requires,
    /// Obsoletes
    Obsoletes,
    /// Is obsoleted by
    IsObsoletedBy,
}

/// Rights.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rights {
    /// Rights text
    pub rights: Option<String>,
    /// Rights URI
    pub rights_uri: Option<String>,
    /// Rights identifier
    pub rights_identifier: Option<String>,
    /// Rights identifier scheme
    pub rights_identifier_scheme: Option<String>,
    /// Scheme URI
    pub scheme_uri: Option<String>,
}

/// Description.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Description {
    /// Description text
    pub description: String,
    /// Description type
    pub description_type: DescriptionType,
    /// Language
    pub lang: Option<String>,
}

/// Description type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DescriptionType {
    /// Abstract
    Abstract,
    /// Methods
    Methods,
    /// Series information
    SeriesInformation,
    /// Table of contents
    TableOfContents,
    /// Technical info
    TechnicalInfo,
    /// Other
    Other,
}

/// Geo location.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoLocation {
    /// Geo location place
    pub geo_location_place: Option<String>,
    /// Geo location point
    pub geo_location_point: Option<GeoLocationPoint>,
    /// Geo location box
    pub geo_location_box: Option<GeoLocationBox>,
    /// Geo location polygons
    pub geo_location_polygons: Vec<GeoLocationPolygon>,
}

/// Geo location point.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GeoLocationPoint {
    /// Point longitude
    pub point_longitude: f64,
    /// Point latitude
    pub point_latitude: f64,
}

/// Geo location box.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GeoLocationBox {
    /// West bound longitude
    pub west_bound_longitude: f64,
    /// East bound longitude
    pub east_bound_longitude: f64,
    /// South bound latitude
    pub south_bound_latitude: f64,
    /// North bound latitude
    pub north_bound_latitude: f64,
}

/// Geo location polygon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoLocationPolygon {
    /// Polygon points
    pub polygon_points: Vec<GeoLocationPoint>,
    /// Inpolygon point
    pub in_polygon_point: Option<GeoLocationPoint>,
}

/// Funding reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingReference {
    /// Funder name
    pub funder_name: String,
    /// Funder identifier
    pub funder_identifier: Option<String>,
    /// Funder identifier type
    pub funder_identifier_type: Option<String>,
    /// Award number
    pub award_number: Option<String>,
    /// Award URI
    pub award_uri: Option<String>,
    /// Award title
    pub award_title: Option<String>,
}

#[cfg(feature = "xml")]
impl DataCiteMetadata {
    /// Export to DataCite XML format.
    pub fn to_xml(&self) -> Result<String> {
        use quick_xml::se::to_string;
        to_string(&self).map_err(|e| MetadataError::XmlError(e.to_string()))
    }

    /// Import from DataCite XML format.
    pub fn from_xml(xml: &str) -> Result<Self> {
        use quick_xml::de::from_str;
        from_str(xml).map_err(|e| MetadataError::XmlError(e.to_string()))
    }
}

impl DataCiteMetadata {
    /// Export to JSON format.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(&self).map_err(|e| MetadataError::JsonError(e.to_string()))
    }

    /// Import from JSON format.
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| MetadataError::JsonError(e.to_string()))
    }
}
