//! GeoPackage tile matrix structures.
//!
//! Corresponds to the `gpkg_tile_matrix` system table defined in OGC GeoPackage
//! Encoding Standard v1.3.1, Clause 2.2.6.

/// A row from the `gpkg_tile_matrix` table.
///
/// Each row describes the grid layout and pixel resolution for one zoom level
/// of a tile pyramid content table.
#[derive(Debug, Clone, PartialEq)]
pub struct TileMatrix {
    /// Name of the user-data tiles table this matrix belongs to.
    pub table_name: String,
    /// Zoom level (0 = most zoomed-out).
    pub zoom_level: u32,
    /// Number of tile columns at this zoom level.
    pub matrix_width: u32,
    /// Number of tile rows at this zoom level.
    pub matrix_height: u32,
    /// Width of each tile in pixels.
    pub tile_width: u32,
    /// Height of each tile in pixels.
    pub tile_height: u32,
    /// Ground sample distance in the X direction (CRS units per pixel).
    pub pixel_x_size: f64,
    /// Ground sample distance in the Y direction (CRS units per pixel).
    pub pixel_y_size: f64,
}

impl TileMatrix {
    /// Return the total number of tiles in this zoom level's matrix.
    ///
    /// # Example
    /// ```
    /// use oxigdal_gpkg::tile_matrix::TileMatrix;
    /// let m = TileMatrix {
    ///     table_name: "imagery".into(),
    ///     zoom_level: 0, matrix_width: 2, matrix_height: 1,
    ///     tile_width: 256, tile_height: 256,
    ///     pixel_x_size: 0.017578125, pixel_y_size: 0.017578125,
    /// };
    /// assert_eq!(m.tile_count(), 2);
    /// ```
    pub fn tile_count(&self) -> u64 {
        self.matrix_width as u64 * self.matrix_height as u64
    }

    /// Return the mean pixel resolution, averaged over X and Y.
    ///
    /// This is useful as a single scalar when comparing zoom levels.
    ///
    /// # Example
    /// ```
    /// use oxigdal_gpkg::tile_matrix::TileMatrix;
    /// let m = TileMatrix {
    ///     table_name: "t".into(),
    ///     zoom_level: 1, matrix_width: 4, matrix_height: 2,
    ///     tile_width: 256, tile_height: 256,
    ///     pixel_x_size: 0.1, pixel_y_size: 0.2,
    /// };
    /// assert!((m.pixel_resolution() - 0.15).abs() < 1e-15);
    /// ```
    pub fn pixel_resolution(&self) -> f64 {
        (self.pixel_x_size + self.pixel_y_size) / 2.0
    }
}
