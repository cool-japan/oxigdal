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

        if let Some(value) = attrs.get_value("Conventions") {
            if let Ok(s) = value.as_text() {
                cf.conventions = Some(s.to_string());
            }
        }

        if let Some(value) = attrs.get_value("title") {
            if let Ok(s) = value.as_text() {
                cf.title = Some(s.to_string());
            }
        }

        if let Some(value) = attrs.get_value("institution") {
            if let Ok(s) = value.as_text() {
                cf.institution = Some(s.to_string());
            }
        }

        if let Some(value) = attrs.get_value("source") {
            if let Ok(s) = value.as_text() {
                cf.source = Some(s.to_string());
            }
        }

        if let Some(value) = attrs.get_value("history") {
            if let Ok(s) = value.as_text() {
                cf.history = Some(s.to_string());
            }
        }

        if let Some(value) = attrs.get_value("references") {
            if let Ok(s) = value.as_text() {
                cf.references = Some(s.to_string());
            }
        }

        if let Some(value) = attrs.get_value("comment") {
            if let Ok(s) = value.as_text() {
                cf.comment = Some(s.to_string());
            }
        }

        cf
    }

    /// Convert to attributes.
    pub fn to_attributes(&self) -> Attributes {
        let mut attrs = Attributes::new();

        if let Some(ref conventions) = self.conventions {
            let _ = attrs.add(
                Attribute::new("Conventions", AttributeValue::text(conventions.clone()))
                    .expect("Valid attribute"),
            );
        }

        if let Some(ref title) = self.title {
            let _ = attrs.add(
                Attribute::new("title", AttributeValue::text(title.clone()))
                    .expect("Valid attribute"),
            );
        }

        if let Some(ref institution) = self.institution {
            let _ = attrs.add(
                Attribute::new("institution", AttributeValue::text(institution.clone()))
                    .expect("Valid attribute"),
            );
        }

        if let Some(ref source) = self.source {
            let _ = attrs.add(
                Attribute::new("source", AttributeValue::text(source.clone()))
                    .expect("Valid attribute"),
            );
        }

        if let Some(ref history) = self.history {
            let _ = attrs.add(
                Attribute::new("history", AttributeValue::text(history.clone()))
                    .expect("Valid attribute"),
            );
        }

        if let Some(ref references) = self.references {
            let _ = attrs.add(
                Attribute::new("references", AttributeValue::text(references.clone()))
                    .expect("Valid attribute"),
            );
        }

        if let Some(ref comment) = self.comment {
            let _ = attrs.add(
                Attribute::new("comment", AttributeValue::text(comment.clone()))
                    .expect("Valid attribute"),
            );
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
