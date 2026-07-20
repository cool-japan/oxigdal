//! GraphQL mutation resolvers.

/// Mutation helper functions.
pub mod helpers {
    use crate::error::Result;

    /// Validates dataset name.
    pub fn validate_dataset_name(name: &str) -> Result<()> {
        if name.is_empty() {
            return Err(crate::error::GatewayError::InvalidRequest(
                "Dataset name cannot be empty".to_string(),
            ));
        }
        Ok(())
    }

    /// Validates format.
    pub fn validate_format(format: &str) -> Result<()> {
        let valid_formats = ["GeoTIFF", "GeoJSON", "Shapefile", "NetCDF"];
        if !valid_formats.contains(&format) {
            return Err(crate::error::GatewayError::InvalidRequest(format!(
                "Invalid format: {}",
                format
            )));
        }
        Ok(())
    }
}
