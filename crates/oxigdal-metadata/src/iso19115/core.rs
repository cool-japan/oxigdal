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
    /// Export to ISO 19115-3 / ISO 19139 XML format with proper namespaces.
    ///
    /// Produces XML conforming to the ISO 19115-3 metadata standard,
    /// including `gmd`, `gco`, and `gml` namespace prefixes.
    pub fn to_xml(&self) -> Result<String> {
        use std::fmt::Write;

        let mut xml = String::with_capacity(4096);

        // XML declaration
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");

        // Root element with ISO 19139 namespaces
        xml.push_str("<gmd:MD_Metadata\n");
        xml.push_str("  xmlns:gmd=\"http://www.isotc211.org/2005/gmd\"\n");
        xml.push_str("  xmlns:gco=\"http://www.isotc211.org/2005/gco\"\n");
        xml.push_str("  xmlns:gml=\"http://www.opengis.net/gml/3.2\"\n");
        xml.push_str("  xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"");
        xml.push_str(">\n");

        // File identifier
        if let Some(ref id) = self.file_identifier {
            let _ = write!(
                xml,
                "  <gmd:fileIdentifier>\n    <gco:CharacterString>{}</gco:CharacterString>\n  </gmd:fileIdentifier>\n",
                xml_escape(id)
            );
        }

        // Language
        if let Some(ref lang) = self.language {
            let _ = write!(
                xml,
                "  <gmd:language>\n    <gco:CharacterString>{}</gco:CharacterString>\n  </gmd:language>\n",
                xml_escape(lang)
            );
        }

        // Character set
        if let Some(ref cs) = self.character_set {
            let cs_code = match cs {
                CharacterSet::Utf8 => "utf8",
                CharacterSet::Iso8859_1 => "8859part1",
                CharacterSet::Utf16 => "utf16",
            };
            let _ = write!(
                xml,
                "  <gmd:characterSet>\n    <gmd:MD_CharacterSetCode codeList=\"http://www.isotc211.org/2005/resources/Codelist/gmxCodelists.xml#MD_CharacterSetCode\" codeListValue=\"{cs_code}\">{cs_code}</gmd:MD_CharacterSetCode>\n  </gmd:characterSet>\n"
            );
        }

        // Hierarchy level
        let hl_code = match self.hierarchy_level {
            HierarchyLevel::Dataset => "dataset",
            HierarchyLevel::Series => "series",
            HierarchyLevel::Service => "service",
            HierarchyLevel::Application => "application",
        };
        let _ = write!(
            xml,
            "  <gmd:hierarchyLevel>\n    <gmd:MD_ScopeCode codeList=\"http://www.isotc211.org/2005/resources/Codelist/gmxCodelists.xml#MD_ScopeCode\" codeListValue=\"{hl_code}\">{hl_code}</gmd:MD_ScopeCode>\n  </gmd:hierarchyLevel>\n"
        );

        // Contact(s)
        for contact in &self.contact {
            xml.push_str("  <gmd:contact>\n");
            write_responsible_party(&mut xml, contact, "    ");
            xml.push_str("  </gmd:contact>\n");
        }

        // Date stamp
        let _ = write!(
            xml,
            "  <gmd:dateStamp>\n    <gco:DateTime>{}</gco:DateTime>\n  </gmd:dateStamp>\n",
            self.date_stamp.format("%Y-%m-%dT%H:%M:%SZ")
        );

        // Metadata standard
        let _ = write!(
            xml,
            "  <gmd:metadataStandardName>\n    <gco:CharacterString>{}</gco:CharacterString>\n  </gmd:metadataStandardName>\n",
            xml_escape(&self.metadata_standard_name)
        );
        let _ = write!(
            xml,
            "  <gmd:metadataStandardVersion>\n    <gco:CharacterString>{}</gco:CharacterString>\n  </gmd:metadataStandardVersion>\n",
            xml_escape(&self.metadata_standard_version)
        );

        // Reference system info
        for rs in &self.reference_system_info {
            xml.push_str("  <gmd:referenceSystemInfo>\n");
            xml.push_str("    <gmd:MD_ReferenceSystem>\n");
            if let Some(ref ident) = rs.reference_system_identifier {
                xml.push_str("      <gmd:referenceSystemIdentifier>\n");
                xml.push_str("        <gmd:RS_Identifier>\n");
                let _ = write!(
                    xml,
                    "          <gmd:code>\n            <gco:CharacterString>{}</gco:CharacterString>\n          </gmd:code>\n",
                    xml_escape(&ident.code)
                );
                if let Some(ref cs) = ident.code_space {
                    let _ = write!(
                        xml,
                        "          <gmd:codeSpace>\n            <gco:CharacterString>{}</gco:CharacterString>\n          </gmd:codeSpace>\n",
                        xml_escape(cs)
                    );
                }
                if let Some(ref v) = ident.version {
                    let _ = write!(
                        xml,
                        "          <gmd:version>\n            <gco:CharacterString>{}</gco:CharacterString>\n          </gmd:version>\n",
                        xml_escape(v)
                    );
                }
                xml.push_str("        </gmd:RS_Identifier>\n");
                xml.push_str("      </gmd:referenceSystemIdentifier>\n");
            }
            xml.push_str("    </gmd:MD_ReferenceSystem>\n");
            xml.push_str("  </gmd:referenceSystemInfo>\n");
        }

        // Identification info
        for ident in &self.identification_info {
            xml.push_str("  <gmd:identificationInfo>\n");
            xml.push_str("    <gmd:MD_DataIdentification>\n");

            // Citation
            xml.push_str("      <gmd:citation>\n");
            xml.push_str("        <gmd:CI_Citation>\n");
            let _ = write!(
                xml,
                "          <gmd:title>\n            <gco:CharacterString>{}</gco:CharacterString>\n          </gmd:title>\n",
                xml_escape(&ident.citation.title)
            );
            for cd in &ident.citation.date {
                xml.push_str("          <gmd:date>\n");
                xml.push_str("            <gmd:CI_Date>\n");
                let _ = write!(
                    xml,
                    "              <gmd:date>\n                <gco:DateTime>{}</gco:DateTime>\n              </gmd:date>\n",
                    cd.date.format("%Y-%m-%dT%H:%M:%SZ")
                );
                let dt_code = match cd.date_type {
                    DateType::Creation => "creation",
                    DateType::Publication => "publication",
                    DateType::Revision => "revision",
                };
                let _ = write!(
                    xml,
                    "              <gmd:dateType>\n                <gmd:CI_DateTypeCode codeListValue=\"{dt_code}\">{dt_code}</gmd:CI_DateTypeCode>\n              </gmd:dateType>\n"
                );
                xml.push_str("            </gmd:CI_Date>\n");
                xml.push_str("          </gmd:date>\n");
            }
            xml.push_str("        </gmd:CI_Citation>\n");
            xml.push_str("      </gmd:citation>\n");

            // Abstract
            let _ = write!(
                xml,
                "      <gmd:abstract>\n        <gco:CharacterString>{}</gco:CharacterString>\n      </gmd:abstract>\n",
                xml_escape(&ident.abstract_text)
            );

            // Keywords
            for keyword_group in &ident.keywords {
                xml.push_str("      <gmd:descriptiveKeywords>\n");
                xml.push_str("        <gmd:MD_Keywords>\n");
                for kw in keyword_group {
                    let _ = write!(
                        xml,
                        "          <gmd:keyword>\n            <gco:CharacterString>{}</gco:CharacterString>\n          </gmd:keyword>\n",
                        xml_escape(&kw.keyword)
                    );
                }
                xml.push_str("        </gmd:MD_Keywords>\n");
                xml.push_str("      </gmd:descriptiveKeywords>\n");
            }

            // Extent
            xml.push_str("      <gmd:extent>\n");
            xml.push_str("        <gmd:EX_Extent>\n");
            if let Some(ref bbox) = ident.extent.geographic_extent {
                xml.push_str("          <gmd:geographicElement>\n");
                xml.push_str("            <gmd:EX_GeographicBoundingBox>\n");
                let _ = write!(
                    xml,
                    "              <gmd:westBoundLongitude>\n                <gco:Decimal>{}</gco:Decimal>\n              </gmd:westBoundLongitude>\n",
                    bbox.west
                );
                let _ = write!(
                    xml,
                    "              <gmd:eastBoundLongitude>\n                <gco:Decimal>{}</gco:Decimal>\n              </gmd:eastBoundLongitude>\n",
                    bbox.east
                );
                let _ = write!(
                    xml,
                    "              <gmd:southBoundLatitude>\n                <gco:Decimal>{}</gco:Decimal>\n              </gmd:southBoundLatitude>\n",
                    bbox.south
                );
                let _ = write!(
                    xml,
                    "              <gmd:northBoundLatitude>\n                <gco:Decimal>{}</gco:Decimal>\n              </gmd:northBoundLatitude>\n",
                    bbox.north
                );
                xml.push_str("            </gmd:EX_GeographicBoundingBox>\n");
                xml.push_str("          </gmd:geographicElement>\n");
            }
            if let Some(ref temporal) = ident.extent.temporal_extent {
                xml.push_str("          <gmd:temporalElement>\n");
                xml.push_str("            <gmd:EX_TemporalExtent>\n");
                xml.push_str("              <gmd:extent>\n");
                xml.push_str("                <gml:TimePeriod>\n");
                if let Some(start) = temporal.start {
                    let _ = writeln!(
                        xml,
                        "                  <gml:beginPosition>{}</gml:beginPosition>",
                        start.format("%Y-%m-%dT%H:%M:%SZ")
                    );
                }
                if let Some(end) = temporal.end {
                    let _ = writeln!(
                        xml,
                        "                  <gml:endPosition>{}</gml:endPosition>",
                        end.format("%Y-%m-%dT%H:%M:%SZ")
                    );
                }
                xml.push_str("                </gml:TimePeriod>\n");
                xml.push_str("              </gmd:extent>\n");
                xml.push_str("            </gmd:EX_TemporalExtent>\n");
                xml.push_str("          </gmd:temporalElement>\n");
            }
            xml.push_str("        </gmd:EX_Extent>\n");
            xml.push_str("      </gmd:extent>\n");

            xml.push_str("    </gmd:MD_DataIdentification>\n");
            xml.push_str("  </gmd:identificationInfo>\n");
        }

        // Distribution info
        if let Some(ref dist) = self.distribution_info {
            xml.push_str("  <gmd:distributionInfo>\n");
            xml.push_str("    <gmd:MD_Distribution>\n");
            for fmt in &dist.format {
                let _ = write!(
                    xml,
                    "      <gmd:distributionFormat>\n        <gmd:MD_Format>\n          <gmd:name>\n            <gco:CharacterString>{}</gco:CharacterString>\n          </gmd:name>\n          <gmd:version>\n            <gco:CharacterString>{}</gco:CharacterString>\n          </gmd:version>\n        </gmd:MD_Format>\n      </gmd:distributionFormat>\n",
                    xml_escape(&fmt.name),
                    xml_escape(&fmt.version)
                );
            }
            for to in &dist.transfer_options {
                xml.push_str("      <gmd:transferOptions>\n");
                xml.push_str("        <gmd:MD_DigitalTransferOptions>\n");
                for online in &to.online {
                    xml.push_str("          <gmd:onLine>\n");
                    xml.push_str("            <gmd:CI_OnlineResource>\n");
                    let _ = write!(
                        xml,
                        "              <gmd:linkage>\n                <gmd:URL>{}</gmd:URL>\n              </gmd:linkage>\n",
                        xml_escape(&online.linkage)
                    );
                    if let Some(ref name) = online.name {
                        let _ = write!(
                            xml,
                            "              <gmd:name>\n                <gco:CharacterString>{}</gco:CharacterString>\n              </gmd:name>\n",
                            xml_escape(name)
                        );
                    }
                    xml.push_str("            </gmd:CI_OnlineResource>\n");
                    xml.push_str("          </gmd:onLine>\n");
                }
                xml.push_str("        </gmd:MD_DigitalTransferOptions>\n");
                xml.push_str("      </gmd:transferOptions>\n");
            }
            xml.push_str("    </gmd:MD_Distribution>\n");
            xml.push_str("  </gmd:distributionInfo>\n");
        }

        // Data quality info
        if let Some(ref dq) = self.data_quality_info {
            xml.push_str("  <gmd:dataQualityInfo>\n");
            xml.push_str("    <gmd:DQ_DataQuality>\n");
            let scope_code = match dq.scope.level {
                HierarchyLevel::Dataset => "dataset",
                HierarchyLevel::Series => "series",
                HierarchyLevel::Service => "service",
                HierarchyLevel::Application => "application",
            };
            let _ = write!(
                xml,
                "      <gmd:scope>\n        <gmd:DQ_Scope>\n          <gmd:level>\n            <gmd:MD_ScopeCode codeListValue=\"{scope_code}\">{scope_code}</gmd:MD_ScopeCode>\n          </gmd:level>\n        </gmd:DQ_Scope>\n      </gmd:scope>\n"
            );
            if let Some(ref lineage) = dq.lineage {
                xml.push_str("      <gmd:lineage>\n");
                xml.push_str("        <gmd:LI_Lineage>\n");
                let _ = write!(
                    xml,
                    "          <gmd:statement>\n            <gco:CharacterString>{}</gco:CharacterString>\n          </gmd:statement>\n",
                    xml_escape(&lineage.statement)
                );
                xml.push_str("        </gmd:LI_Lineage>\n");
                xml.push_str("      </gmd:lineage>\n");
            }
            xml.push_str("    </gmd:DQ_DataQuality>\n");
            xml.push_str("  </gmd:dataQualityInfo>\n");
        }

        xml.push_str("</gmd:MD_Metadata>\n");

        Ok(xml)
    }

    /// Import from ISO 19139 XML format.
    pub fn from_xml(xml: &str) -> Result<Self> {
        use quick_xml::de::from_str;
        from_str(xml).map_err(|e| MetadataError::XmlError(e.to_string()))
    }
}

/// Write a CI_ResponsibleParty element.
#[cfg(feature = "xml")]
fn write_responsible_party(xml: &mut String, party: &ResponsibleParty, indent: &str) {
    use std::fmt::Write;

    let _ = writeln!(xml, "{indent}<gmd:CI_ResponsibleParty>");
    if let Some(ref name) = party.individual_name {
        let _ = write!(
            xml,
            "{indent}  <gmd:individualName>\n{indent}    <gco:CharacterString>{}</gco:CharacterString>\n{indent}  </gmd:individualName>\n",
            xml_escape(name)
        );
    }
    if let Some(ref org) = party.organization_name {
        let _ = write!(
            xml,
            "{indent}  <gmd:organisationName>\n{indent}    <gco:CharacterString>{}</gco:CharacterString>\n{indent}  </gmd:organisationName>\n",
            xml_escape(org)
        );
    }
    let role_code = match party.role {
        Role::ResourceProvider => "resourceProvider",
        Role::Custodian => "custodian",
        Role::Owner => "owner",
        Role::User => "user",
        Role::Distributor => "distributor",
        Role::Originator => "originator",
        Role::PointOfContact => "pointOfContact",
        Role::PrincipalInvestigator => "principalInvestigator",
        Role::Processor => "processor",
        Role::Publisher => "publisher",
        Role::Author => "author",
    };
    let _ = write!(
        xml,
        "{indent}  <gmd:role>\n{indent}    <gmd:CI_RoleCode codeListValue=\"{role_code}\">{role_code}</gmd:CI_RoleCode>\n{indent}  </gmd:role>\n"
    );
    let _ = writeln!(xml, "{indent}</gmd:CI_ResponsibleParty>");
}

/// Escape XML special characters.
#[cfg(feature = "xml")]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "xml")]
    #[test]
    fn test_xml_serialization_basic() {
        let iso = Iso19115Metadata::builder()
            .file_identifier("test-001")
            .title("Global DEM")
            .abstract_text("A global digital elevation model")
            .build()
            .expect("build should succeed");

        let xml = iso.to_xml().expect("XML serialization should succeed");
        assert!(xml.starts_with("<?xml version=\"1.0\""));
        assert!(xml.contains("<gmd:MD_Metadata"));
        assert!(xml.contains("xmlns:gmd=\"http://www.isotc211.org/2005/gmd\""));
        assert!(xml.contains("xmlns:gco=\"http://www.isotc211.org/2005/gco\""));
        assert!(xml.contains("<gmd:fileIdentifier>"));
        assert!(xml.contains("<gco:CharacterString>test-001</gco:CharacterString>"));
        assert!(xml.contains("Global DEM"));
        assert!(xml.contains("A global digital elevation model"));
        assert!(xml.contains("</gmd:MD_Metadata>"));
    }

    #[cfg(feature = "xml")]
    #[test]
    fn test_xml_serialization_with_bbox() {
        let iso = Iso19115Metadata::builder()
            .title("Bounded Dataset")
            .bbox(BoundingBox::new(-180.0, -90.0, 180.0, 90.0))
            .build()
            .expect("build should succeed");

        let xml = iso.to_xml().expect("XML should serialize");
        assert!(xml.contains("<gmd:EX_GeographicBoundingBox>"));
        assert!(xml.contains("<gmd:westBoundLongitude>"));
        assert!(xml.contains("<gco:Decimal>-180</gco:Decimal>"));
        assert!(xml.contains("<gmd:northBoundLatitude>"));
        assert!(xml.contains("<gco:Decimal>90</gco:Decimal>"));
    }

    #[cfg(feature = "xml")]
    #[test]
    fn test_xml_serialization_with_keywords() {
        let iso = Iso19115Metadata::builder()
            .title("Keyword Test")
            .keywords(vec!["elevation", "terrain", "DEM"])
            .build()
            .expect("build should succeed");

        let xml = iso.to_xml().expect("XML should serialize");
        assert!(xml.contains("<gmd:descriptiveKeywords>"));
        assert!(xml.contains("elevation"));
        assert!(xml.contains("terrain"));
        assert!(xml.contains("DEM"));
    }

    #[cfg(feature = "xml")]
    #[test]
    fn test_xml_serialization_with_reference_system() {
        use crate::iso19115::reference_system::{Identifier, ReferenceSystem};

        let mut iso = Iso19115Metadata::builder()
            .title("CRS Test")
            .build()
            .expect("build should succeed");

        iso.reference_system_info.push(ReferenceSystem {
            reference_system_identifier: Some(Identifier::epsg(4326)),
            reference_system_type: None,
        });

        let xml = iso.to_xml().expect("XML should serialize");
        assert!(xml.contains("<gmd:referenceSystemInfo>"));
        assert!(xml.contains("<gmd:MD_ReferenceSystem>"));
        assert!(xml.contains("4326"));
        assert!(xml.contains("EPSG"));
    }

    #[cfg(feature = "xml")]
    #[test]
    fn test_xml_escape_special_chars() {
        let iso = Iso19115Metadata::builder()
            .title("Test <with> &special 'chars' \"quoted\"")
            .build()
            .expect("build should succeed");

        let xml = iso.to_xml().expect("XML should serialize");
        assert!(xml.contains("&lt;with&gt;"));
        assert!(xml.contains("&amp;special"));
        assert!(xml.contains("&apos;chars&apos;"));
        assert!(xml.contains("&quot;quoted&quot;"));
    }

    #[cfg(feature = "xml")]
    #[test]
    fn test_xml_serialization_contact() {
        let mut iso = Iso19115Metadata::builder()
            .title("Contact Test")
            .build()
            .expect("build should succeed");

        iso.contact.push(ResponsibleParty {
            individual_name: Some("John Doe".to_string()),
            organization_name: Some("ACME Corp".to_string()),
            position_name: None,
            contact_info: None,
            role: Role::PointOfContact,
        });

        let xml = iso.to_xml().expect("XML should serialize");
        assert!(xml.contains("<gmd:CI_ResponsibleParty>"));
        assert!(xml.contains("John Doe"));
        assert!(xml.contains("ACME Corp"));
        assert!(xml.contains("pointOfContact"));
    }

    #[test]
    fn test_json_round_trip() {
        let original = Iso19115Metadata::builder()
            .file_identifier("round-trip-01")
            .title("Round Trip Test")
            .abstract_text("Testing JSON serialization")
            .bbox(BoundingBox::new(-10.0, -5.0, 10.0, 5.0))
            .build()
            .expect("build should succeed");

        let json = original.to_json().expect("JSON serialize");
        let restored = Iso19115Metadata::from_json(&json).expect("JSON deserialize");
        assert_eq!(restored.file_identifier, original.file_identifier);
        assert_eq!(
            restored.identification_info[0].citation.title,
            "Round Trip Test"
        );
    }

    #[test]
    fn test_default_metadata() {
        let iso = Iso19115Metadata::default();
        assert!(iso.file_identifier.is_none());
        assert!(iso.identification_info.is_empty());
        assert_eq!(iso.metadata_standard_name, "ISO 19115:2014");
    }
}
