//! Grid registry and grid-shift step evaluators for [`crate::pipeline`].
//!
//! PROJ pipeline strings reference datum-shift grids by *name*
//! (`+proj=hgridshift +grids=BETA2007.gsb`). Because the pipeline evaluator
//! performs no file I/O of its own, a [`GridRegistry`] lets callers pre-load
//! grid bytes (horizontal NTv2 `.gsb` via [`NtV2Grid`], vertical geoid grids
//! via [`GeoidGrid`]) and bind them to the names used in the pipeline string.

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::geoid::GeoidGrid;
use crate::grid_shift::ntv2::NtV2Grid;
use crate::pipeline::ShiftDirection;
use crate::transform::{Coordinate, Coordinate3D};

/// A registry of loaded grid-shift files that a [`crate::pipeline::Pipeline`]
/// consults when it executes `hgridshift` / `vgridshift` steps.
///
/// This crate deliberately performs no file I/O inside the pipeline evaluator,
/// so the caller loads the grid bytes (via [`NtV2Grid::from_bytes`] for
/// horizontal grids, or a [`GeoidGrid`] for vertical grids) and registers them
/// here under the exact name used in the pipeline string.
///
/// Grids are stored behind [`Arc`] so a registry — and therefore a pipeline —
/// is cheap to clone and share.
#[derive(Debug, Clone, Default)]
pub struct GridRegistry {
    /// Horizontal NTv2 grids keyed by their pipeline `+grids=` name.
    hgrids: HashMap<String, Arc<NtV2Grid>>,
    /// Vertical (geoid-separation) grids keyed by their pipeline `+grids=` name.
    vgrids: HashMap<String, Arc<GeoidGrid>>,
}

impl GridRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a horizontal NTv2 grid under `name`.
    pub fn insert_hgrid(&mut self, name: impl Into<String>, grid: Arc<NtV2Grid>) {
        self.hgrids.insert(name.into(), grid);
    }

    /// Registers a vertical (geoid) grid under `name`.
    pub fn insert_vgrid(&mut self, name: impl Into<String>, grid: Arc<GeoidGrid>) {
        self.vgrids.insert(name.into(), grid);
    }

    /// Returns `true` if no grids are registered.
    pub fn is_empty(&self) -> bool {
        self.hgrids.is_empty() && self.vgrids.is_empty()
    }

    /// Looks up a horizontal grid, tolerating PROJ's optional-grid `@` prefix
    /// and comma-separated grid lists (the first resolvable name wins).
    fn lookup_hgrid(&self, grid_id: &str) -> Option<&Arc<NtV2Grid>> {
        Self::candidate_names(grid_id).find_map(|name| self.hgrids.get(name))
    }

    /// Looks up a vertical grid (same name-resolution rules as
    /// [`lookup_hgrid`](Self::lookup_hgrid)).
    fn lookup_vgrid(&self, grid_id: &str) -> Option<&Arc<GeoidGrid>> {
        Self::candidate_names(grid_id).find_map(|name| self.vgrids.get(name))
    }

    /// Yields candidate lookup keys for a PROJ `+grids=` value: each
    /// comma-separated entry, with any leading `@` (optional-grid marker)
    /// stripped.
    fn candidate_names(grid_id: &str) -> impl Iterator<Item = &str> {
        grid_id
            .split(',')
            .map(|s| s.trim().trim_start_matches('@'))
            .filter(|s| !s.is_empty())
    }
}

/// Resolve the effective direction of a grid-shift step from its declared
/// [`ShiftDirection`] and the runtime `inverse` flag (they compose by XOR).
fn effective_grid_forward(direction: &ShiftDirection, inverse: bool) -> bool {
    let declared_forward = matches!(direction, ShiftDirection::Forward);
    declared_forward ^ inverse
}

/// Apply an `hgridshift` step: look up the NTv2 grid by name in `grids` and
/// transform the geographic `(lon, lat)` coordinate (degrees). Errors with a
/// clear message if the grid was never registered.
pub(crate) fn apply_hgridshift(
    coord: &Coordinate,
    grid_id: &str,
    direction: &ShiftDirection,
    inverse: bool,
    grids: &GridRegistry,
) -> Result<Coordinate> {
    let grid = grids.lookup_hgrid(grid_id).ok_or_else(|| {
        Error::PipelineParseError(format!(
            "hgridshift: grid '{grid_id}' not loaded — register it with Pipeline::with_hgrid"
        ))
    })?;
    let (lon, lat) = if effective_grid_forward(direction, inverse) {
        grid.transform(coord.x, coord.y)?
    } else {
        grid.inverse_transform(coord.x, coord.y)?
    };
    Ok(Coordinate::new(lon, lat))
}

/// Apply a `vgridshift` step to a **2-D** coordinate.
///
/// A 2-D coordinate carries no height, so there is nothing to shift and the
/// horizontal position passes through. The grid is still required to be
/// registered — a reference to an unloaded grid is an error (consistent with
/// `hgridshift`), never a silent no-op.
pub(crate) fn apply_vgridshift_2d(
    coord: &Coordinate,
    grid_id: &str,
    grids: &GridRegistry,
) -> Result<Coordinate> {
    if grids.lookup_vgrid(grid_id).is_none() {
        return Err(Error::PipelineParseError(format!(
            "vgridshift: grid '{grid_id}' not loaded — register it with Pipeline::with_vgrid"
        )));
    }
    Ok(*coord)
}

/// Apply a `vgridshift` step to a 3-D coordinate: look up the vertical (geoid)
/// grid by name and shift the height `z`.
///
/// Forward (`SYSTEM_F → SYSTEM_T`) is defined as ellipsoidal → orthometric
/// (`h_out = h_in − N`); the inverse adds `N` back. This matches PROJ's
/// `vgridshift` sign for a geoid-separation grid.
pub(crate) fn apply_vgridshift(
    coord: &Coordinate3D,
    grid_id: &str,
    direction: &ShiftDirection,
    inverse: bool,
    grids: &GridRegistry,
) -> Result<Coordinate3D> {
    let grid = grids.lookup_vgrid(grid_id).ok_or_else(|| {
        Error::PipelineParseError(format!(
            "vgridshift: grid '{grid_id}' not loaded — register it with Pipeline::with_vgrid"
        ))
    })?;
    // `coord.x` is longitude, `coord.y` is latitude (degrees).
    let n = grid.geoid_height_m(coord.y, coord.x);
    let z = if effective_grid_forward(direction, inverse) {
        coord.z - n
    } else {
        coord.z + n
    };
    Ok(Coordinate3D::new(coord.x, coord.y, z))
}
