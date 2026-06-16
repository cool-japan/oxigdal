//! Cross-standard metadata transformation.
//!
//! This module provides functionality to transform metadata between different
//! standards, including ISO 19115, FGDC, INSPIRE, STAC, DataCite, and DCAT.

use crate::common::{BoundingBox, Keyword, TemporalExtent};
use crate::datacite::{Creator, DataCiteMetadata, IdentifierType, ResourceTypeGeneral};
use crate::dcat::Dataset as DcatDataset;
use crate::error::{MetadataError, Result};
use crate::fgdc::{FgdcMetadata, Keywords, TimeInfo};
use crate::inspire::{InspireMetadata, ResourceLocator, ResourceLocatorFunction};
use crate::iso19115::{
    DataIdentification, Iso19115Metadata, OnlineFunction, ResponsibleParty, Role,
};
use chrono::Datelike;

/// Extract a DOI from a list of identifier strings.
/// Matches bare DOI (`10.NNNN/suffix`) or a `doi.org/` URL.
fn extract_doi(ids: &[String]) -> Option<String> {
    for id in ids {
        let s = id.trim();
        if s.starts_with("10.") && s.contains('/') {
            return Some(s.to_string());
        }
        if let Some(pos) = s.find("doi.org/") {
            let after = &s[pos + "doi.org/".len()..];
            if after.starts_with("10.") {
                return Some(after.to_string());
            }
        }
    }
    None
}

/// Map an ISO 19115 `OnlineFunction` variant to an INSPIRE `ResourceLocatorFunction`.
fn map_online_function(f: &OnlineFunction) -> ResourceLocatorFunction {
    match f {
        OnlineFunction::Download => ResourceLocatorFunction::Download,
        OnlineFunction::Information => ResourceLocatorFunction::Information,
        OnlineFunction::OfflineAccess => ResourceLocatorFunction::OfflineAccess,
        OnlineFunction::Order => ResourceLocatorFunction::Order,
        OnlineFunction::Search => ResourceLocatorFunction::Search,
    }
}

/// Transform ISO 19115 metadata to FGDC.
pub fn iso19115_to_fgdc(iso: &Iso19115Metadata) -> Result<FgdcMetadata> {
    let mut fgdc = FgdcMetadata::default();

    if let Some(ident) = iso.identification_info.first() {
        // Transform citation
        fgdc.idinfo.citation.citeinfo.title = ident.citation.title.clone();

        // Transform description
        fgdc.idinfo.descript.abstract_text = ident.abstract_text.clone();
        fgdc.idinfo.descript.purpose = ident.purpose.clone();

        // Transform keywords
        for keyword_group in &ident.keywords {
            let theme_keywords: Vec<String> =
                keyword_group.iter().map(|k| k.keyword.clone()).collect();

            if !theme_keywords.is_empty() {
                fgdc.idinfo.keywords.push(Keywords {
                    theme: Some("General".to_string()),
                    theme_key: theme_keywords,
                    place: Vec::new(),
                    temporal: Vec::new(),
                });
            }
        }

        // Transform bounding box
        if let Some(bbox) = ident.extent.geographic_extent {
            fgdc.idinfo.spdom.bounding = bbox;
        }

        // Transform temporal extent
        if let Some(ref temporal) = ident.extent.temporal_extent {
            if let (Some(start), Some(end)) = (temporal.start, temporal.end) {
                fgdc.idinfo.timeperd.timeinfo = TimeInfo::Range {
                    begdate: start.format("%Y%m%d").to_string(),
                    enddate: end.format("%Y%m%d").to_string(),
                };
            }
        }
    }

    // Transform metadata date
    fgdc.metainfo.metd = iso.date_stamp.format("%Y%m%d").to_string();

    Ok(fgdc)
}

/// Transform FGDC metadata to ISO 19115.
pub fn fgdc_to_iso19115(fgdc: &FgdcMetadata) -> Result<Iso19115Metadata> {
    let mut iso = Iso19115Metadata::default();
    let mut ident = DataIdentification::default();

    // Transform citation
    ident.citation.title = fgdc.idinfo.citation.citeinfo.title.clone();

    // Transform description
    ident.abstract_text = fgdc.idinfo.descript.abstract_text.clone();
    ident.purpose = fgdc.idinfo.descript.purpose.clone();

    // Transform keywords
    for keyword_group in &fgdc.idinfo.keywords {
        let keywords: Vec<Keyword> = keyword_group
            .theme_key
            .iter()
            .map(|k| Keyword {
                keyword: k.clone(),
                thesaurus: keyword_group.theme.clone(),
            })
            .collect();

        if !keywords.is_empty() {
            ident.keywords.push(keywords);
        }
    }

    // Transform bounding box
    ident.extent.geographic_extent = Some(fgdc.idinfo.spdom.bounding);

    // Transform temporal extent
    if let TimeInfo::Range {
        begdate: _,
        enddate: _,
    } = &fgdc.idinfo.timeperd.timeinfo
    {
        // Parse dates - simplified, would need proper parsing
        ident.extent.temporal_extent = Some(TemporalExtent {
            start: None, // Would parse begdate
            end: None,   // Would parse enddate
        });
    }

    iso.identification_info.push(ident);

    Ok(iso)
}

/// Transform ISO 19115 metadata to INSPIRE.
pub fn iso19115_to_inspire(iso: &Iso19115Metadata) -> Result<InspireMetadata> {
    let mut inspire = InspireMetadata {
        base: iso.clone(),
        ..Default::default()
    };

    // Populate resource_locator by walking distribution_info → transfer_options → online
    let mut locators: Vec<ResourceLocator> = Vec::new();
    if let Some(ref dist) = iso.distribution_info {
        for to in &dist.transfer_options {
            for res in &to.online {
                locators.push(ResourceLocator {
                    url: res.linkage.clone(),
                    description: res.description.clone(),
                    function: map_online_function(&res.function),
                });
            }
        }
    }

    // INSPIRE requires at least one resource locator
    if locators.is_empty() {
        return Err(MetadataError::TransformError(
            "ISO 19115 metadata lacks resource locator for INSPIRE".to_string(),
        ));
    }

    inspire.resource_locator = locators;
    Ok(inspire)
}

/// Transform INSPIRE metadata to ISO 19115.
pub fn inspire_to_iso19115(inspire: &InspireMetadata) -> Result<Iso19115Metadata> {
    Ok(inspire.base.clone())
}

/// Transform ISO 19115 metadata to DataCite.
pub fn iso19115_to_datacite(iso: &Iso19115Metadata) -> Result<DataCiteMetadata> {
    if iso.identification_info.is_empty() {
        return Err(MetadataError::TransformError(
            "ISO 19115 metadata lacks identification info".to_string(),
        ));
    }

    let ident = &iso.identification_info[0];

    // Extract a real DOI from citation.identifier; fail if none is present.
    let doi = extract_doi(&ident.citation.identifier).ok_or_else(|| {
        MetadataError::TransformError("ISO 19115 metadata lacks a DOI for DataCite".to_string())
    })?;

    // Extract publisher: first point_of_contact with an organization_name, then
    // individual_name as fallback, then a hard fallback when neither is present.
    let publisher: String = ident
        .point_of_contact
        .iter()
        .find_map(|p| p.organization_name.clone())
        .or_else(|| {
            ident
                .point_of_contact
                .iter()
                .find_map(|p| p.individual_name.clone())
        })
        // fallback: no organization/individual in ISO record
        .unwrap_or_else(|| "Unknown Publisher".to_string());

    // Publication year: use the first citation date that carries a year,
    // preferring Publication-typed dates, then any date available.
    let publication_year: u16 = ident
        .citation
        .date
        .iter()
        .find(|cd| matches!(cd.date_type, crate::iso19115::DateType::Publication))
        .or_else(|| ident.citation.date.first())
        .map(|cd| cd.date.year() as u16)
        // fallback: ISO record carries no publication date
        .unwrap_or_else(|| chrono::Utc::now().year() as u16);

    let mut builder = DataCiteMetadata::builder()
        .identifier(doi, IdentifierType::Doi)
        .title(ident.citation.title.clone())
        .publisher(publisher)
        .publication_year(publication_year)
        .resource_type(ResourceTypeGeneral::Dataset);

    // Add creators from responsible parties
    for party in &ident.point_of_contact {
        if let Some(ref name) = party.individual_name {
            builder = builder.creator(Creator::new(name));
        } else if let Some(ref org) = party.organization_name {
            builder = builder.creator(Creator::organization(org));
        }
    }

    builder.build()
}

/// Transform DataCite metadata to ISO 19115.
pub fn datacite_to_iso19115(datacite: &DataCiteMetadata) -> Result<Iso19115Metadata> {
    let mut iso = Iso19115Metadata::default();
    let mut ident = DataIdentification::default();

    // Transform title
    if let Some(title) = datacite.titles.first() {
        ident.citation.title = title.title.clone();
    }

    // Transform creators to responsible parties
    for creator in &datacite.creators {
        ident.point_of_contact.push(ResponsibleParty {
            individual_name: Some(creator.name.clone()),
            organization_name: None,
            position_name: None,
            contact_info: None,
            role: Role::Originator,
        });
    }

    // Transform descriptions to abstract
    if let Some(desc) = datacite.descriptions.first() {
        ident.abstract_text = desc.description.clone();
    }

    // Transform subjects to keywords
    if !datacite.subjects.is_empty() {
        let keywords: Vec<Keyword> = datacite
            .subjects
            .iter()
            .map(|s| Keyword {
                keyword: s.subject.clone(),
                thesaurus: s.subject_scheme.clone(),
            })
            .collect();
        ident.keywords.push(keywords);
    }

    // Transform geo locations to bounding box
    if let Some(geo_loc) = datacite.geo_locations.first() {
        if let Some(bbox_dc) = &geo_loc.geo_location_box {
            ident.extent.geographic_extent = Some(BoundingBox {
                west: bbox_dc.west_bound_longitude,
                east: bbox_dc.east_bound_longitude,
                south: bbox_dc.south_bound_latitude,
                north: bbox_dc.north_bound_latitude,
            });
        }
    }

    iso.identification_info.push(ident);

    Ok(iso)
}

/// Transform ISO 19115 metadata to DCAT.
pub fn iso19115_to_dcat(iso: &Iso19115Metadata) -> Result<DcatDataset> {
    if iso.identification_info.is_empty() {
        return Err(MetadataError::TransformError(
            "ISO 19115 metadata lacks identification info".to_string(),
        ));
    }

    let ident = &iso.identification_info[0];

    let mut builder = DcatDataset::builder()
        .title(ident.citation.title.clone())
        .description(ident.abstract_text.clone());

    // Add keywords
    for keyword_group in &ident.keywords {
        for keyword in keyword_group {
            builder = builder.keyword(keyword.keyword.clone());
        }
    }

    builder.build()
}

/// Transform DCAT Dataset to ISO 19115.
pub fn dcat_to_iso19115(dcat: &DcatDataset) -> Result<Iso19115Metadata> {
    let mut iso = Iso19115Metadata::default();
    let mut ident = DataIdentification::default();

    // Transform title
    if let Some(title) = dcat.title.first() {
        ident.citation.title = title.value.clone();
    }

    // Transform description
    if let Some(desc) = dcat.description.first() {
        ident.abstract_text = desc.value.clone();
    }

    // Transform keywords
    if !dcat.keyword.is_empty() {
        let keywords: Vec<Keyword> = dcat
            .keyword
            .iter()
            .map(|k| Keyword {
                keyword: k.clone(),
                thesaurus: None,
            })
            .collect();
        ident.keywords.push(keywords);
    }

    // Transform spatial coverage
    if let Some(spatial) = dcat.spatial.first() {
        if let Some(bbox_dcat) = &spatial.bbox {
            ident.extent.geographic_extent = Some(BoundingBox {
                west: bbox_dcat.west,
                east: bbox_dcat.east,
                south: bbox_dcat.south,
                north: bbox_dcat.north,
            });
        }
    }

    // Transform temporal coverage
    if let Some(temporal) = dcat.temporal.first() {
        ident.extent.temporal_extent = Some(TemporalExtent {
            start: temporal.start_date,
            end: temporal.end_date,
        });
    }

    iso.identification_info.push(ident);

    Ok(iso)
}

/// Field mapping table for transformation.
#[derive(Debug, Clone)]
pub struct FieldMapping {
    /// Source standard
    pub source_standard: String,
    /// Source field path
    pub source_field: String,
    /// Target standard
    pub target_standard: String,
    /// Target field path
    pub target_field: String,
    /// Transformation function name
    pub transform_function: Option<String>,
}

/// Get field mappings between standards.
pub fn get_field_mappings(source: &str, target: &str) -> Vec<FieldMapping> {
    let mut mappings = Vec::new();

    match (source, target) {
        ("ISO19115", "FGDC") => {
            mappings.push(FieldMapping {
                source_standard: "ISO19115".to_string(),
                source_field: "identification_info.citation.title".to_string(),
                target_standard: "FGDC".to_string(),
                target_field: "idinfo.citation.citeinfo.title".to_string(),
                transform_function: None,
            });

            mappings.push(FieldMapping {
                source_standard: "ISO19115".to_string(),
                source_field: "identification_info.abstract".to_string(),
                target_standard: "FGDC".to_string(),
                target_field: "idinfo.descript.abstract".to_string(),
                transform_function: None,
            });

            mappings.push(FieldMapping {
                source_standard: "ISO19115".to_string(),
                source_field: "identification_info.extent.geographic_extent".to_string(),
                target_standard: "FGDC".to_string(),
                target_field: "idinfo.spdom.bounding".to_string(),
                transform_function: None,
            });
        }

        ("FGDC", "ISO19115") => {
            // Reverse mappings
            for mapping in get_field_mappings("ISO19115", "FGDC") {
                mappings.push(FieldMapping {
                    source_standard: mapping.target_standard,
                    source_field: mapping.target_field,
                    target_standard: mapping.source_standard,
                    target_field: mapping.source_field,
                    transform_function: mapping.transform_function,
                });
            }
        }

        _ => {}
    }

    mappings
}

/// Metadata transformer with configurable options.
pub struct MetadataTransformer {
    /// Preserve all fields when possible
    pub preserve_all: bool,
    /// Strict mode (fail on unmappable fields)
    pub strict_mode: bool,
}

impl Default for MetadataTransformer {
    fn default() -> Self {
        Self {
            preserve_all: true,
            strict_mode: false,
        }
    }
}

impl MetadataTransformer {
    /// Create a new transformer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set preserve all option.
    pub fn with_preserve_all(mut self, preserve: bool) -> Self {
        self.preserve_all = preserve;
        self
    }

    /// Set strict mode.
    pub fn with_strict_mode(mut self, strict: bool) -> Self {
        self.strict_mode = strict;
        self
    }

    /// Transform ISO 19115 to FGDC.
    pub fn transform_iso_to_fgdc(&self, iso: &Iso19115Metadata) -> Result<FgdcMetadata> {
        iso19115_to_fgdc(iso)
    }

    /// Transform FGDC to ISO 19115.
    pub fn transform_fgdc_to_iso(&self, fgdc: &FgdcMetadata) -> Result<Iso19115Metadata> {
        fgdc_to_iso19115(fgdc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transformer_builder() {
        let transformer = MetadataTransformer::new()
            .with_preserve_all(true)
            .with_strict_mode(false);

        assert!(transformer.preserve_all);
        assert!(!transformer.strict_mode);
    }

    #[test]
    fn test_field_mappings() {
        let mappings = get_field_mappings("ISO19115", "FGDC");
        assert!(!mappings.is_empty());

        let title_mapping = mappings.iter().find(|m| m.source_field.contains("title"));
        assert!(title_mapping.is_some());
    }
}
