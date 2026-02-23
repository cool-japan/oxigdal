//! CF Conventions Time and Bounds Support
//!
//! This module handles time bounds and related functionality.

use serde::{Deserialize, Serialize};

use crate::dimension::Dimensions;
use crate::error::{NetCdfError, Result};
use crate::variable::Variable;

// ============================================================================
// Bounds Variables
// ============================================================================

/// Bounds variable information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundsVariable {
    /// Name of the bounds variable
    pub name: String,
    /// Name of the coordinate variable this bounds
    pub coordinate_variable: String,
    /// Number of vertices (usually 2 for 1D bounds)
    pub num_vertices: usize,
}

impl BoundsVariable {
    /// Create a new bounds variable.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        coordinate_variable: impl Into<String>,
        num_vertices: usize,
    ) -> Self {
        Self {
            name: name.into(),
            coordinate_variable: coordinate_variable.into(),
            num_vertices,
        }
    }

    /// Validate the bounds variable against the coordinate variable.
    pub fn validate(
        &self,
        coord_var: &Variable,
        bounds_var: &Variable,
        dimensions: &Dimensions,
    ) -> Result<()> {
        // Bounds should have same dimensions as coordinate plus a vertex dimension
        let coord_dims = coord_var.dimension_names();
        let bounds_dims = bounds_var.dimension_names();

        if bounds_dims.len() != coord_dims.len() + 1 {
            return Err(NetCdfError::CfConventionsError(format!(
                "Bounds variable '{}' should have {} dimensions (coordinate dimensions + 1), found {}",
                self.name,
                coord_dims.len() + 1,
                bounds_dims.len()
            )));
        }

        // Check that coordinate dimensions match
        for (i, dim) in coord_dims.iter().enumerate() {
            if i < bounds_dims.len() && bounds_dims[i] != *dim {
                return Err(NetCdfError::CfConventionsError(format!(
                    "Bounds variable '{}' dimension mismatch at position {}: expected '{}', found '{}'",
                    self.name, i, dim, bounds_dims[i]
                )));
            }
        }

        // Check vertex dimension size
        if let Some(last_dim) = bounds_dims.last() {
            if let Some(dim) = dimensions.get(last_dim) {
                if dim.len() != self.num_vertices {
                    return Err(NetCdfError::CfConventionsError(format!(
                        "Bounds variable '{}' vertex dimension should have {} vertices, found {}",
                        self.name,
                        self.num_vertices,
                        dim.len()
                    )));
                }
            }
        }

        Ok(())
    }
}
