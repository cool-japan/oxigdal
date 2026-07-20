//! GeoPackage raster tile support.

use super::{connection::GpkgConnection, metadata::Extent};
use crate::error::{Error, Result};
use oxisql_core::ToSqlValue;

/// Tile matrix set for raster data.
pub struct TileMatrixSet {
    table_name: String,
    #[allow(dead_code)]
    srs_id: i32,
    extent: Extent,
    matrices: Vec<TileMatrix>,
}

impl TileMatrixSet {
    /// Open existing tile matrix set.
    pub fn open(conn: &GpkgConnection, table_name: &str) -> Result<Self> {
        let tname = table_name.to_string();
        let rows = conn.connection().query(
            "SELECT srs_id, min_x, min_y, max_x, max_y FROM gpkg_tile_matrix_set WHERE table_name = $1",
            &[&tname as &dyn ToSqlValue],
        )?;
        let row = rows.into_iter().next().ok_or_else(|| {
            Error::geopackage(format!("no tile matrix set for table '{table_name}'"))
        })?;
        let srs_id: i32 = row
            .try_get_by_index(0)
            .map_err(|e| Error::geopackage(format!("column 0: {e}")))?;
        let min_x: f64 = row
            .try_get_by_index(1)
            .map_err(|e| Error::geopackage(format!("column 1: {e}")))?;
        let min_y: f64 = row
            .try_get_by_index(2)
            .map_err(|e| Error::geopackage(format!("column 2: {e}")))?;
        let max_x: f64 = row
            .try_get_by_index(3)
            .map_err(|e| Error::geopackage(format!("column 3: {e}")))?;
        let max_y: f64 = row
            .try_get_by_index(4)
            .map_err(|e| Error::geopackage(format!("column 4: {e}")))?;

        let extent = Extent::new(min_x, min_y, max_x, max_y);
        let matrices = Self::load_matrices(conn, table_name)?;

        Ok(Self {
            table_name: table_name.to_string(),
            srs_id,
            extent,
            matrices,
        })
    }

    /// Create new tile matrix set.
    pub fn create(
        conn: &GpkgConnection,
        table_name: &str,
        srs_id: i32,
        extent: Extent,
    ) -> Result<Self> {
        // Create tile table
        let create_sql = format!(
            "CREATE TABLE {} (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                zoom_level INTEGER NOT NULL,
                tile_column INTEGER NOT NULL,
                tile_row INTEGER NOT NULL,
                tile_data BLOB NOT NULL,
                UNIQUE (zoom_level, tile_column, tile_row)
            )",
            table_name
        );
        conn.execute_batch(&create_sql)?;

        // Register in gpkg_contents
        let tname = table_name.to_string();
        let tiles_str = "tiles".to_string();
        let srs_id_i64: i64 = i64::from(srs_id);
        conn.execute(
            "INSERT INTO gpkg_contents (table_name, data_type, identifier, srs_id, min_x, min_y, max_x, max_y) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            &[
                &tname as &dyn ToSqlValue,
                &tiles_str,
                &tname,
                &srs_id_i64,
                &extent.min_x,
                &extent.min_y,
                &extent.max_x,
                &extent.max_y,
            ],
        )?;

        // Register in gpkg_tile_matrix_set
        conn.execute(
            "INSERT INTO gpkg_tile_matrix_set (table_name, srs_id, min_x, min_y, max_x, max_y) VALUES ($1, $2, $3, $4, $5, $6)",
            &[
                &tname as &dyn ToSqlValue,
                &srs_id_i64,
                &extent.min_x,
                &extent.min_y,
                &extent.max_x,
                &extent.max_y,
            ],
        )?;

        Ok(Self {
            table_name: table_name.to_string(),
            srs_id,
            extent,
            matrices: Vec::new(),
        })
    }

    /// Load tile matrices.
    fn load_matrices(conn: &GpkgConnection, table_name: &str) -> Result<Vec<TileMatrix>> {
        let tname = table_name.to_string();
        let rows = conn.connection().query(
            "SELECT zoom_level, matrix_width, matrix_height, tile_width, tile_height, pixel_x_size, pixel_y_size
             FROM gpkg_tile_matrix WHERE table_name = $1 ORDER BY zoom_level",
            &[&tname as &dyn ToSqlValue],
        )?;

        rows.into_iter()
            .map(|row| {
                let zoom_level: i32 = row
                    .try_get_by_index(0)
                    .map_err(|e| Error::geopackage(format!("zoom_level: {e}")))?;
                let matrix_width: i32 = row
                    .try_get_by_index(1)
                    .map_err(|e| Error::geopackage(format!("matrix_width: {e}")))?;
                let matrix_height: i32 = row
                    .try_get_by_index(2)
                    .map_err(|e| Error::geopackage(format!("matrix_height: {e}")))?;
                let tile_width: i32 = row
                    .try_get_by_index(3)
                    .map_err(|e| Error::geopackage(format!("tile_width: {e}")))?;
                let tile_height: i32 = row
                    .try_get_by_index(4)
                    .map_err(|e| Error::geopackage(format!("tile_height: {e}")))?;
                let pixel_x_size: f64 = row
                    .try_get_by_index(5)
                    .map_err(|e| Error::geopackage(format!("pixel_x_size: {e}")))?;
                let pixel_y_size: f64 = row
                    .try_get_by_index(6)
                    .map_err(|e| Error::geopackage(format!("pixel_y_size: {e}")))?;
                Ok(TileMatrix {
                    zoom_level,
                    matrix_width,
                    matrix_height,
                    tile_width,
                    tile_height,
                    pixel_x_size,
                    pixel_y_size,
                })
            })
            .collect()
    }

    /// Add tile matrix for a zoom level.
    pub fn add_matrix(&mut self, conn: &GpkgConnection, matrix: TileMatrix) -> Result<()> {
        let tname = self.table_name.clone();
        let zoom_level_i64: i64 = i64::from(matrix.zoom_level);
        let matrix_width_i64: i64 = i64::from(matrix.matrix_width);
        let matrix_height_i64: i64 = i64::from(matrix.matrix_height);
        let tile_width_i64: i64 = i64::from(matrix.tile_width);
        let tile_height_i64: i64 = i64::from(matrix.tile_height);
        conn.execute(
            "INSERT INTO gpkg_tile_matrix (table_name, zoom_level, matrix_width, matrix_height, tile_width, tile_height, pixel_x_size, pixel_y_size)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            &[
                &tname as &dyn ToSqlValue,
                &zoom_level_i64,
                &matrix_width_i64,
                &matrix_height_i64,
                &tile_width_i64,
                &tile_height_i64,
                &matrix.pixel_x_size,
                &matrix.pixel_y_size,
            ],
        )?;

        self.matrices.push(matrix);
        Ok(())
    }

    /// Get table name.
    pub fn name(&self) -> &str {
        &self.table_name
    }

    /// Get extent.
    pub fn extent(&self) -> &Extent {
        &self.extent
    }

    /// Get tile matrices.
    pub fn matrices(&self) -> &[TileMatrix] {
        &self.matrices
    }

    /// Get tile count for zoom level.
    pub fn tile_count(&self, conn: &GpkgConnection, zoom_level: i32) -> Result<i64> {
        let zoom_level_i64: i64 = i64::from(zoom_level);
        let rows = conn.connection().query(
            &format!(
                "SELECT COUNT(*) FROM {} WHERE zoom_level = $1",
                self.table_name
            ),
            &[&zoom_level_i64 as &dyn ToSqlValue],
        )?;
        let row = rows
            .into_iter()
            .next()
            .ok_or_else(|| Error::geopackage("tile_count: no row returned"))?;
        let count: i64 = row
            .try_get_by_index(0)
            .map_err(|e| Error::geopackage(format!("column 0: {e}")))?;
        Ok(count)
    }
}

/// Tile matrix for a specific zoom level.
#[derive(Debug, Clone)]
pub struct TileMatrix {
    /// Zoom level
    pub zoom_level: i32,
    /// Matrix width (number of tiles)
    pub matrix_width: i32,
    /// Matrix height (number of tiles)
    pub matrix_height: i32,
    /// Tile width in pixels
    pub tile_width: i32,
    /// Tile height in pixels
    pub tile_height: i32,
    /// Pixel size in X direction
    pub pixel_x_size: f64,
    /// Pixel size in Y direction
    pub pixel_y_size: f64,
}

impl TileMatrix {
    /// Create new tile matrix.
    pub fn new(
        zoom_level: i32,
        matrix_width: i32,
        matrix_height: i32,
        tile_width: i32,
        tile_height: i32,
        pixel_x_size: f64,
        pixel_y_size: f64,
    ) -> Self {
        Self {
            zoom_level,
            matrix_width,
            matrix_height,
            tile_width,
            tile_height,
            pixel_x_size,
            pixel_y_size,
        }
    }

    /// Calculate total pixel dimensions.
    pub fn pixel_dimensions(&self) -> (i32, i32) {
        (
            self.matrix_width * self.tile_width,
            self.matrix_height * self.tile_height,
        )
    }

    /// Check if tile coordinates are valid.
    pub fn is_valid_tile(&self, column: i32, row: i32) -> bool {
        column >= 0 && column < self.matrix_width && row >= 0 && row < self.matrix_height
    }
}

/// Individual tile data.
#[derive(Debug, Clone)]
pub struct Tile {
    /// Zoom level
    pub zoom_level: i32,
    /// Tile column
    pub tile_column: i32,
    /// Tile row
    pub tile_row: i32,
    /// Tile data (image bytes)
    pub data: Vec<u8>,
}

impl Tile {
    /// Create new tile.
    pub fn new(zoom_level: i32, tile_column: i32, tile_row: i32, data: Vec<u8>) -> Self {
        Self {
            zoom_level,
            tile_column,
            tile_row,
            data,
        }
    }

    /// Get data size.
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;
    use crate::gpkg::schema;
    use tempfile::NamedTempFile;

    #[test]
    fn test_tile_matrix() {
        let matrix = TileMatrix::new(0, 4, 4, 256, 256, 10.0, 10.0);
        assert_eq!(matrix.zoom_level, 0);
        assert_eq!(matrix.pixel_dimensions(), (1024, 1024));
        assert!(matrix.is_valid_tile(0, 0));
        assert!(matrix.is_valid_tile(3, 3));
        assert!(!matrix.is_valid_tile(4, 4));
    }

    #[test]
    fn test_tile_matrix_set_creation() -> Result<()> {
        let temp_file = NamedTempFile::new().map_err(Error::from)?;
        let conn = GpkgConnection::create(temp_file.path())?;
        schema::initialize_schema(&conn)?;

        let extent = Extent::new(-180.0, -90.0, 180.0, 90.0);
        let tms = TileMatrixSet::create(&conn, "test_tiles", 4326, extent)?;

        assert_eq!(tms.name(), "test_tiles");
        assert_eq!(tms.extent().min_x, -180.0);

        Ok(())
    }

    #[test]
    fn test_tile_creation() {
        let data = vec![0u8; 1024];
        let tile = Tile::new(0, 5, 10, data);
        assert_eq!(tile.zoom_level, 0);
        assert_eq!(tile.tile_column, 5);
        assert_eq!(tile.size(), 1024);
    }
}
