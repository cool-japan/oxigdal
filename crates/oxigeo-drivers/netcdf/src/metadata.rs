//! NetCDF file metadata structures.
//!
//! This module provides structures for representing NetCDF file metadata,
//! including global attributes, dimensions, and variables.

use serde::{Deserialize, Serialize};

use crate::attribute::{Attribute, AttributeValue, Attributes};
use crate::dimension::Dimensions;
use crate::error::{NetCdfError, Result};
use crate::variable::Variables;

/// NetCDF file format version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NetCdfVersion {
    /// NetCDF-3 Classic format
    #[default]
    Classic,
    /// NetCDF-3 64-bit offset format
    Offset64Bit,
    /// NetCDF-4 (HDF5-based)
    NetCdf4,
    /// NetCDF-4 Classic model
    NetCdf4Classic,
}

impl NetCdfVersion {
    /// Check if this is a NetCDF-4 variant.
    #[must_use]
    pub const fn is_netcdf4(&self) -> bool {
        matches!(self, Self::NetCdf4 | Self::NetCdf4Classic)
    }

    /// Check if this is a NetCDF-3 variant.
    #[must_use]
    pub const fn is_netcdf3(&self) -> bool {
        matches!(self, Self::Classic | Self::Offset64Bit)
    }

    /// Get the version number.
    #[must_use]
    pub const fn version_number(&self) -> u8 {
        match self {
            Self::Classic | Self::Offset64Bit => 3,
            Self::NetCdf4 | Self::NetCdf4Classic => 4,
        }
    }

    /// Get the format name.
    #[must_use]
    pub const fn format_name(&self) -> &'static str {
        match self {
            Self::Classic => "NetCDF-3 Classic",
            Self::Offset64Bit => "NetCDF-3 64-bit Offset",
            Self::NetCdf4 => "NetCDF-4",
            Self::NetCdf4Classic => "NetCDF-4 Classic",
        }
    }
}

/// CF (Climate and Forecast) conventions metadata.
///
/// CF conventions provide standardized metadata for climate and forecast data.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CfMetadata {
    /// CF conventions version (e.g., "CF-1.8")
    pub conventions: Option<String>,
    /// Title of the dataset
    pub title: Option<String>,
    /// Institution where data was produced
    pub institution: Option<String>,
    /// Source of the data (e.g., model name)
    pub source: Option<String>,
    /// History of processing
    pub history: Option<String>,
    /// Additional references
    pub references: Option<String>,
    /// Comments
    pub comment: Option<String>,
}

impl CfMetadata {
    /// Create new CF metadata.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            conventions: None,
            title: None,
            institution: None,
            source: None,
            history: None,
            references: None,
            comment: None,
        }
    }

    /// Create from global attributes.
    #[must_use]
    pub fn from_attributes(attrs: &Attributes) -> Self {
        let mut cf = Self::new();

        if let Some(value) = attrs.get_value("Conventions")
            && let Ok(s) = value.as_text()
        {
            cf.conventions = Some(s.to_string());
        }

        if let Some(value) = attrs.get_value("title")
            && let Ok(s) = value.as_text()
        {
            cf.title = Some(s.to_string());
        }

        if let Some(value) = attrs.get_value("institution")
            && let Ok(s) = value.as_text()
        {
            cf.institution = Some(s.to_string());
        }

        if let Some(value) = attrs.get_value("source")
            && let Ok(s) = value.as_text()
        {
            cf.source = Some(s.to_string());
        }

        if let Some(value) = attrs.get_value("history")
            && let Ok(s) = value.as_text()
        {
            cf.history = Some(s.to_string());
        }

        if let Some(value) = attrs.get_value("references")
            && let Ok(s) = value.as_text()
        {
            cf.references = Some(s.to_string());
        }

        if let Some(value) = attrs.get_value("comment")
            && let Ok(s) = value.as_text()
        {
            cf.comment = Some(s.to_string());
        }

        cf
    }

    /// Convert to attributes.
    ///
    /// Every attribute name here is a hardcoded non-empty literal, so
    /// `Attribute::new` (which only errors on an empty name) cannot actually
    /// fail today — but this never `.expect()`s or `.unwrap()`s on that fact:
    /// each attribute is added only `if let Ok(..)`, matching the workspace's
    /// no-unwrap/no-expect production-code policy so a future refactor that
    /// parameterizes the name can never reintroduce a panic path here.
    #[must_use]
    pub fn to_attributes(&self) -> Attributes {
        let mut attrs = Attributes::new();

        if let Some(ref conventions) = self.conventions {
            add_text_attribute(&mut attrs, "Conventions", conventions);
        }
        if let Some(ref title) = self.title {
            add_text_attribute(&mut attrs, "title", title);
        }
        if let Some(ref institution) = self.institution {
            add_text_attribute(&mut attrs, "institution", institution);
        }
        if let Some(ref source) = self.source {
            add_text_attribute(&mut attrs, "source", source);
        }
        if let Some(ref history) = self.history {
            add_text_attribute(&mut attrs, "history", history);
        }
        if let Some(ref references) = self.references {
            add_text_attribute(&mut attrs, "references", references);
        }
        if let Some(ref comment) = self.comment {
            add_text_attribute(&mut attrs, "comment", comment);
        }

        attrs
    }

    /// Check if CF conventions are specified.
    #[must_use]
    pub fn has_conventions(&self) -> bool {
        self.conventions.is_some()
    }

    /// Check if this is a CF-compliant dataset.
    #[must_use]
    pub fn is_cf_compliant(&self) -> bool {
        self.conventions
            .as_ref()
            .is_some_and(|c| c.starts_with("CF-"))
    }
}

/// Add a text attribute named `name` with value `value` to `attrs`, silently
/// skipping it if construction fails (only possible for an empty `name`,
/// which never happens for the hardcoded literals [`CfMetadata::to_attributes`]
/// passes) — never `.expect()`/`.unwrap()`s, per the no-unwrap production-code
/// policy.
fn add_text_attribute(attrs: &mut Attributes, name: &str, value: &str) {
    if let Ok(a) = Attribute::new(name, AttributeValue::text(value)) {
        let _ = attrs.add(a);
    }
}

/// NetCDF file metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetCdfMetadata {
    /// File format version
    version: NetCdfVersion,
    /// Global attributes
    global_attributes: Attributes,
    /// Dimensions
    dimensions: Dimensions,
    /// Variables
    variables: Variables,
    /// CF conventions metadata (optional)
    cf_metadata: Option<CfMetadata>,
}

impl NetCdfMetadata {
    /// Create new metadata.
    ///
    /// # Arguments
    ///
    /// * `version` - NetCDF format version
    pub fn new(version: NetCdfVersion) -> Self {
        Self {
            version,
            global_attributes: Attributes::new(),
            dimensions: Dimensions::new(),
            variables: Variables::new(),
            cf_metadata: None,
        }
    }

    /// Create NetCDF-3 Classic metadata.
    #[must_use]
    pub fn new_classic() -> Self {
        Self::new(NetCdfVersion::Classic)
    }

    /// Create NetCDF-4 metadata.
    #[must_use]
    pub fn new_netcdf4() -> Self {
        Self::new(NetCdfVersion::NetCdf4)
    }

    /// Get the format version.
    #[must_use]
    pub const fn version(&self) -> NetCdfVersion {
        self.version
    }

    /// Get global attributes.
    #[must_use]
    pub const fn global_attributes(&self) -> &Attributes {
        &self.global_attributes
    }

    /// Get mutable access to global attributes.
    pub fn global_attributes_mut(&mut self) -> &mut Attributes {
        &mut self.global_attributes
    }

    /// Get dimensions.
    #[must_use]
    pub const fn dimensions(&self) -> &Dimensions {
        &self.dimensions
    }

    /// Get mutable access to dimensions.
    pub fn dimensions_mut(&mut self) -> &mut Dimensions {
        &mut self.dimensions
    }

    /// Get variables.
    #[must_use]
    pub const fn variables(&self) -> &Variables {
        &self.variables
    }

    /// Get mutable access to variables.
    pub fn variables_mut(&mut self) -> &mut Variables {
        &mut self.variables
    }

    /// Get CF metadata.
    #[must_use]
    pub const fn cf_metadata(&self) -> Option<&CfMetadata> {
        self.cf_metadata.as_ref()
    }

    /// Set CF metadata.
    pub fn set_cf_metadata(&mut self, cf: CfMetadata) {
        self.cf_metadata = Some(cf);
    }

    /// Parse CF metadata from global attributes.
    pub fn parse_cf_metadata(&mut self) {
        let cf = CfMetadata::from_attributes(&self.global_attributes);
        if cf.has_conventions() {
            self.cf_metadata = Some(cf);
        }
    }

    /// Validate the metadata.
    ///
    /// # Errors
    ///
    /// Returns error if metadata is invalid.
    pub fn validate(&self) -> Result<()> {
        // Check that all variable dimensions exist
        for var in self.variables.iter() {
            for dim_name in var.dimension_names() {
                if !self.dimensions.contains(dim_name) {
                    return Err(NetCdfError::DimensionNotFound {
                        name: dim_name.clone(),
                    });
                }
            }

            // Check NetCDF-3 compatibility
            if self.version.is_netcdf3() && !var.is_netcdf3_compatible() {
                return Err(NetCdfError::VariableError(format!(
                    "Variable '{}' uses data type '{}' which is not compatible with NetCDF-3",
                    var.name(),
                    var.data_type().name()
                )));
            }
        }

        // Check unlimited dimension (only one allowed in NetCDF-3)
        if self.version.is_netcdf3() {
            let unlimited_count = self.dimensions.iter().filter(|d| d.is_unlimited()).count();
            if unlimited_count > 1 {
                return Err(NetCdfError::UnlimitedDimensionError(
                    "NetCDF-3 can only have one unlimited dimension".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Get a summary of the metadata.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "NetCDF {} file with {} dimensions, {} variables, {} global attributes",
            self.version.format_name(),
            self.dimensions.len(),
            self.variables.len(),
            self.global_attributes.len()
        )
    }
}

impl Default for NetCdfMetadata {
    fn default() -> Self {
        Self::new_classic()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dimension::Dimension;
    use crate::variable::{DataType, Variable};

    #[test]
    fn test_netcdf_version() {
        assert!(NetCdfVersion::Classic.is_netcdf3());
        assert!(!NetCdfVersion::Classic.is_netcdf4());
        assert_eq!(NetCdfVersion::Classic.version_number(), 3);

        assert!(NetCdfVersion::NetCdf4.is_netcdf4());
        assert!(!NetCdfVersion::NetCdf4.is_netcdf3());
        assert_eq!(NetCdfVersion::NetCdf4.version_number(), 4);
    }

    #[test]
    fn test_cf_metadata() {
        let mut cf = CfMetadata::new();
        cf.conventions = Some("CF-1.8".to_string());
        cf.title = Some("Test Dataset".to_string());

        assert!(cf.has_conventions());
        assert!(cf.is_cf_compliant());

        let attrs = cf.to_attributes();
        assert_eq!(attrs.len(), 2);
        assert!(attrs.contains("Conventions"));
        assert!(attrs.contains("title"));
    }

    /// `to_attributes()` must round-trip every optional CF field (not just a
    /// couple) into its named attribute, without panicking — regression test
    /// for the panic-free rewrite of the `.expect()`-based implementation.
    #[test]
    fn test_cf_metadata_to_attributes_all_fields() {
        let mut cf = CfMetadata::new();
        cf.conventions = Some("CF-1.8".to_string());
        cf.title = Some("Test Dataset".to_string());
        cf.institution = Some("COOLJAPAN".to_string());
        cf.source = Some("simulation".to_string());
        cf.history = Some("created today".to_string());
        cf.references = Some("doi:10.0/example".to_string());
        cf.comment = Some("a comment".to_string());

        let attrs = cf.to_attributes();
        assert_eq!(attrs.len(), 7);
        assert_eq!(
            attrs
                .get("Conventions")
                .and_then(|a| a.value().as_text().ok()),
            Some("CF-1.8")
        );
        assert_eq!(
            attrs.get("title").and_then(|a| a.value().as_text().ok()),
            Some("Test Dataset")
        );
        assert_eq!(
            attrs
                .get("institution")
                .and_then(|a| a.value().as_text().ok()),
            Some("COOLJAPAN")
        );
        assert_eq!(
            attrs.get("source").and_then(|a| a.value().as_text().ok()),
            Some("simulation")
        );
        assert_eq!(
            attrs.get("history").and_then(|a| a.value().as_text().ok()),
            Some("created today")
        );
        assert_eq!(
            attrs
                .get("references")
                .and_then(|a| a.value().as_text().ok()),
            Some("doi:10.0/example")
        );
        assert_eq!(
            attrs.get("comment").and_then(|a| a.value().as_text().ok()),
            Some("a comment")
        );
    }

    /// With no optional fields set, `to_attributes()` must produce an empty
    /// (not panicking, not fabricated) attribute set.
    #[test]
    fn test_cf_metadata_to_attributes_empty_when_unset() {
        let cf = CfMetadata::new();
        let attrs = cf.to_attributes();
        assert_eq!(attrs.len(), 0);
    }

    #[test]
    fn test_cf_from_attributes() {
        let mut attrs = Attributes::new();
        attrs
            .add(
                Attribute::new("Conventions", AttributeValue::text("CF-1.8"))
                    .expect("Failed to create Conventions attribute"),
            )
            .expect("Failed to add Conventions attribute");
        attrs
            .add(
                Attribute::new("title", AttributeValue::text("Test"))
                    .expect("Failed to create title attribute"),
            )
            .expect("Failed to add title attribute");

        let cf = CfMetadata::from_attributes(&attrs);
        assert_eq!(cf.conventions.as_deref(), Some("CF-1.8"));
        assert_eq!(cf.title.as_deref(), Some("Test"));
    }

    #[test]
    fn test_metadata_creation() {
        let mut metadata = NetCdfMetadata::new_classic();
        assert_eq!(metadata.version(), NetCdfVersion::Classic);

        metadata
            .dimensions_mut()
            .add(Dimension::new("time", 10).expect("Failed to create time dimension"))
            .expect("Failed to add time dimension");
        metadata
            .variables_mut()
            .add(
                Variable::new_coordinate("time", DataType::F64)
                    .expect("Failed to create time variable"),
            )
            .expect("Failed to add time variable");

        assert_eq!(metadata.dimensions().len(), 1);
        assert_eq!(metadata.variables().len(), 1);
    }

    #[test]
    fn test_metadata_validation() {
        let mut metadata = NetCdfMetadata::new_classic();
        metadata
            .dimensions_mut()
            .add(Dimension::new("time", 10).expect("Failed to create time dimension"))
            .expect("Failed to add time dimension");
        metadata
            .variables_mut()
            .add(
                Variable::new_coordinate("time", DataType::F64)
                    .expect("Failed to create time variable"),
            )
            .expect("Failed to add time variable");

        assert!(metadata.validate().is_ok());
    }

    #[test]
    fn test_metadata_validation_missing_dimension() {
        let mut metadata = NetCdfMetadata::new_classic();
        metadata
            .variables_mut()
            .add(
                Variable::new("temp", DataType::F32, vec!["time".to_string()])
                    .expect("Failed to create temp variable"),
            )
            .expect("Failed to add temp variable");

        let result = metadata.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_netcdf3_type_validation() {
        let mut metadata = NetCdfMetadata::new_classic();
        metadata
            .dimensions_mut()
            .add(Dimension::new("x", 10).expect("Failed to create x dimension"))
            .expect("Failed to add x dimension");
        metadata
            .variables_mut()
            .add(
                Variable::new("data", DataType::U16, vec!["x".to_string()])
                    .expect("Failed to create data variable"),
            )
            .expect("Failed to add data variable");

        let result = metadata.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_unlimited_dimension_validation() {
        let mut metadata = NetCdfMetadata::new_classic();
        metadata
            .dimensions_mut()
            .add(Dimension::new_unlimited("time", 10).expect("Failed to create time dimension"))
            .expect("Failed to add time dimension");
        metadata
            .dimensions_mut()
            .add(Dimension::new_unlimited("level", 5).expect("Failed to create level dimension"))
            .expect("Failed to add level dimension");

        let result = metadata.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_summary() {
        let mut metadata = NetCdfMetadata::new_classic();
        metadata
            .dimensions_mut()
            .add(Dimension::new("time", 10).expect("Failed to create time dimension"))
            .expect("Failed to add time dimension");
        metadata
            .variables_mut()
            .add(
                Variable::new_coordinate("time", DataType::F64)
                    .expect("Failed to create time variable"),
            )
            .expect("Failed to add time variable");

        let summary = metadata.summary();
        assert!(summary.contains("NetCDF-3"));
        assert!(summary.contains("1 dimensions"));
        assert!(summary.contains("1 variables"));
    }
}
