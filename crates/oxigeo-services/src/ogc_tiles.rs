//! OGC API - Tiles implementation
//!
//! Implements the OGC Two Dimensional Tile Matrix Set and Tile Set Metadata standard
//! (OGC 17-083r4 / OGC API - Tiles - Part 1: Core).
//!
//! See: <https://docs.ogc.org/is/17-083r4/17-083r4.html>
//!
//! # Standards
//!
//! - OGC Two Dimensional Tile Matrix Set (17-083r2)
//! - OGC API - Tiles - Part 1: Core (OGC 20-057)
//!
//! # Conformance Classes
//!
//! - Core: <http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/core>
//! - TileMatrixSet: <http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/tilematrixset>
//! - GeoDataTiles: <http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/geodata-tilesets>

use serde::{Deserialize, Serialize};

/// OGC TileMatrixSet defines a coordinate reference system, a set of tile matrices
/// at different scales, and the origin and dimensions of each tile matrix.
///
/// Well-known TileMatrixSets are registered at the OGC NA definition server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TileMatrixSet {
    /// Identifier of the TileMatrixSet
    pub id: String,
    /// Human-readable title
    pub title: String,
    /// URI of the TileMatrixSet definition (well-known sets have OGC URIs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    /// Coordinate Reference System as URI
    pub crs: String,
    /// Ordered list of tile matrices (one per zoom level)
    pub tile_matrices: Vec<TileMatrix>,
}

/// A single zoom level (scale) within a TileMatrixSet.
///
/// Describes the tile grid at a specific scale: origin, tile dimensions,
/// number of tiles in each direction, and scale denominator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TileMatrix {
    /// Identifier (typically the zoom level as a string, e.g. "0", "1", ...)
    pub id: String,
    /// Scale denominator of the tile matrix (at 0.28mm/pixel)
    pub scale_denominator: f64,
    /// Cell size in CRS units per pixel
    pub cell_size: f64,
    /// Corner used as origin for tile addressing
    pub corner_of_origin: CornerOfOrigin,
    /// Coordinates of the origin corner [easting/longitude, northing/latitude]
    pub point_of_origin: [f64; 2],
    /// Width of each tile in pixels
    pub tile_width: u32,
    /// Height of each tile in pixels
    pub tile_height: u32,
    /// Number of tiles in the X direction
    pub matrix_width: u32,
    /// Number of tiles in the Y direction
    pub matrix_height: u32,
}

/// Specifies which corner of the tile matrix is the origin for tile addressing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CornerOfOrigin {
    /// Origin is the top-left corner (Y increases downward)
    TopLeft,
    /// Origin is the bottom-left corner (Y increases upward)
    BottomLeft,
}

/// Metadata describing a specific TileSet — a collection of tiles covering
/// a geographic area at multiple zoom levels using a defined TileMatrixSet.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TileSetMetadata {
    /// Identifier of the TileMatrixSet used
    pub tile_matrix_set_id: String,
    /// Data type served by this TileSet
    pub data_type: TileDataType,
    /// Links to tile resources and documentation
    pub links: Vec<TileLink>,
    /// Human-readable title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Description of this TileSet
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Attribution/copyright notice
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
    /// Geographic bounding box of available tiles
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extent: Option<GeographicBoundingBox>,
    /// Minimum zoom level available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_tile_matrix: Option<String>,
    /// Maximum zoom level available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tile_matrix: Option<String>,
}

/// The type of data served in a TileSet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TileDataType {
    /// Rendered map image (PNG, JPEG, WebP)
    Map,
    /// Vector features (MVT/Mapbox Vector Tile)
    Vector,
    /// Coverage / raster data (GeoTIFF, netCDF)
    Coverage,
}

/// A hypermedia link in OGC API style.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileLink {
    /// URL of the linked resource
    pub href: String,
    /// Link relation type (e.g. "self", "item", "tiles")
    pub rel: String,
    /// Media type of the linked resource
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    /// Human-readable title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// WGS84 geographic bounding box (longitude/latitude).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeographicBoundingBox {
    /// Lower-left corner [longitude, latitude]
    pub lower_left: [f64; 2],
    /// Upper-right corner [longitude, latitude]
    pub upper_right: [f64; 2],
}

/// OGC API conformance declaration for OGC API - Tiles.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConformanceDeclaration {
    /// List of conformance class URIs this implementation satisfies
    pub conforms_to: Vec<String>,
}

impl ConformanceDeclaration {
    /// Standard conformance classes for OGC API - Tiles Part 1
    pub fn ogc_tiles() -> Self {
        Self {
            conforms_to: vec![
                "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/core".into(),
                "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/tilematrixset".into(),
                "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/geodata-tilesets".into(),
                "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/collections-selection".into(),
                "http://www.opengis.net/spec/ogcapi-common-1/1.0/conf/core".into(),
                "http://www.opengis.net/spec/ogcapi-common-1/1.0/conf/json".into(),
            ],
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TileMatrixSet constructors for well-known grids
// ─────────────────────────────────────────────────────────────────────────────

impl TileMatrixSet {
    /// Standard **WebMercatorQuad** (EPSG:3857) tile grid — zoom levels 0–24.
    ///
    /// Used by Google Maps, OpenStreetMap, Bing Maps, etc.
    /// Origin is the top-left corner of the world at (-20037508.34, 20037508.34).
    pub fn web_mercator_quad() -> Self {
        // Reference: OGC 17-083r2 Annex D, Table D.1
        // Scale at zoom 0: 559082264.0287178 (at 0.00028 m/pixel = 96 dpi)
        // Cell size at zoom 0: 156543.033928041 m/pixel
        const ORIGIN_X: f64 = -20_037_508.342_789_244;
        const ORIGIN_Y: f64 = 20_037_508.342_789_244;
        const SCALE_0: f64 = 559_082_264.028_717_8;
        const CELL_0: f64 = 156_543.033_928_041;

        let matrices = (0u8..=24)
            .map(|z| {
                let n = 1u32 << z;
                let factor = n as f64;
                TileMatrix {
                    id: z.to_string(),
                    scale_denominator: SCALE_0 / factor,
                    cell_size: CELL_0 / factor,
                    corner_of_origin: CornerOfOrigin::TopLeft,
                    point_of_origin: [ORIGIN_X, ORIGIN_Y],
                    tile_width: 256,
                    tile_height: 256,
                    matrix_width: n,
                    matrix_height: n,
                }
            })
            .collect();

        Self {
            id: "WebMercatorQuad".into(),
            title: "Google Maps Compatible for the World".into(),
            uri: Some("http://www.opengis.net/def/tilematrixset/OGC/1.0/WebMercatorQuad".into()),
            crs: "http://www.opengis.net/def/crs/EPSG/0/3857".into(),
            tile_matrices: matrices,
        }
    }

    /// Standard **WorldCRS84Quad** (EPSG:4326 / OGC:CRS84) tile grid — zoom levels 0–17.
    ///
    /// Two tiles at zoom 0 cover the whole world (2:1 aspect ratio).
    /// Used in OGC services and aerospace applications.
    pub fn world_crs84_quad() -> Self {
        // Reference: OGC 17-083r2 Annex D, Table D.2
        const SCALE_0: f64 = 279_541_132.014_358_76;
        const PIXEL_SIZE_DEG: f64 = 0.000_000_277_777_8; // 1 degree / (256 * 14)

        let matrices = (0u8..=17)
            .map(|z| {
                // At zoom 0: 2 columns × 1 row; each step doubles both
                let n_y = 1u32 << z;
                let n_x = 2u32 << z;
                let factor = n_y as f64;
                TileMatrix {
                    id: z.to_string(),
                    scale_denominator: SCALE_0 / factor,
                    cell_size: PIXEL_SIZE_DEG / factor,
                    corner_of_origin: CornerOfOrigin::TopLeft,
                    point_of_origin: [-180.0, 90.0],
                    tile_width: 256,
                    tile_height: 256,
                    matrix_width: n_x,
                    matrix_height: n_y,
                }
            })
            .collect();

        Self {
            id: "WorldCRS84Quad".into(),
            title: "CRS84 for the World".into(),
            uri: Some("http://www.opengis.net/def/tilematrixset/OGC/1.0/WorldCRS84Quad".into()),
            crs: "http://www.opengis.net/def/crs/OGC/1.3/CRS84".into(),
            tile_matrices: matrices,
        }
    }

    /// Look up a tile matrix by zoom level.
    pub fn tile_matrix(&self, zoom: u8) -> Option<&TileMatrix> {
        self.tile_matrices.iter().find(|m| m.id == zoom.to_string())
    }

    /// Return the maximum available zoom level.
    pub fn max_zoom(&self) -> u8 {
        self.tile_matrices
            .iter()
            .filter_map(|m| m.id.parse::<u8>().ok())
            .max()
            .unwrap_or(0)
    }

    /// Return the minimum available zoom level.
    pub fn min_zoom(&self) -> u8 {
        self.tile_matrices
            .iter()
            .filter_map(|m| m.id.parse::<u8>().ok())
            .min()
            .unwrap_or(0)
    }

    /// Total number of zoom levels defined in this TileMatrixSet.
    pub fn zoom_level_count(&self) -> usize {
        self.tile_matrices.len()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tile coordinate utilities
// ─────────────────────────────────────────────────────────────────────────────

/// Convert tile coordinates (z, x, y) to a geographic bounding box.
///
/// Returns `[west, south, east, north]` in degrees (WGS84 / EPSG:4326).
/// Uses the Web Mercator (Slippy Map) tile convention.
///
/// # Arguments
///
/// * `z` – zoom level (0–30)
/// * `x` – tile column (0 .. 2^z − 1)
/// * `y` – tile row, top-left origin (0 .. 2^z − 1)
pub fn tile_to_bbox(z: u8, x: u32, y: u32) -> [f64; 4] {
    let n = 1u32 << z;
    let nf = n as f64;

    let west = (x as f64 / nf) * 360.0 - 180.0;
    let east = ((x + 1) as f64 / nf) * 360.0 - 180.0;

    // Mercator latitude from tile row
    let to_lat = |row: u32| -> f64 {
        let sinh_arg = (1.0 - 2.0 * row as f64 / nf) * std::f64::consts::PI;
        sinh_arg.sinh().atan().to_degrees()
    };

    let north = to_lat(y);
    let south = to_lat(y + 1);

    [west, south, east, north]
}

/// Convert WGS84 longitude/latitude to tile coordinates at a given zoom level.
///
/// Uses the standard Slippy Map / Web Mercator tile numbering (origin top-left).
///
/// # Arguments
///
/// * `lon` – longitude in degrees (−180 to 180)
/// * `lat` – latitude in degrees (−85.051 to 85.051)
/// * `zoom` – target zoom level
///
/// Returns `(x, y)` clamped to the valid tile range.
pub fn lonlat_to_tile(lon: f64, lat: f64, zoom: u8) -> (u32, u32) {
    let n = 1u32 << zoom;
    let nf = n as f64;

    let x_raw = (lon + 180.0) / 360.0 * nf;
    let lat_rad = lat.to_radians();
    let y_raw =
        (1.0 - (lat_rad.tan() + (1.0 / lat_rad.cos())).ln() / std::f64::consts::PI) / 2.0 * nf;

    let x = (x_raw as u32).min(n.saturating_sub(1));
    let y = (y_raw as u32).min(n.saturating_sub(1));
    (x, y)
}

/// Convert tile coordinates to the pixel bounds within a tile grid.
///
/// Returns `(pixel_x_min, pixel_y_min, pixel_x_max, pixel_y_max)` in screen pixels
/// for a full tile grid rendered at the given zoom level (each tile is 256×256 px).
pub fn tile_to_pixel_bounds(_z: u8, x: u32, y: u32) -> (u64, u64, u64, u64) {
    let tile_size: u64 = 256;
    let x0 = x as u64 * tile_size;
    let y0 = y as u64 * tile_size;
    (x0, y0, x0 + tile_size, y0 + tile_size)
}

/// Validate that tile coordinates are within range for a given zoom level.
///
/// Returns `true` if x and y are both within [0, 2^z − 1].
pub fn validate_tile_coords(z: u8, x: u32, y: u32) -> bool {
    if z > 30 {
        return false;
    }
    let max = (1u32 << z).saturating_sub(1);
    x <= max && y <= max
}

/// Return the list of child tiles (next zoom level) for a given tile.
///
/// Each tile has exactly 4 children: (2x, 2y), (2x+1, 2y), (2x, 2y+1), (2x+1, 2y+1).
pub fn tile_children(z: u8, x: u32, y: u32) -> Option<[(u8, u32, u32); 4]> {
    let next_z = z.checked_add(1)?;
    Some([
        (next_z, 2 * x, 2 * y),
        (next_z, 2 * x + 1, 2 * y),
        (next_z, 2 * x, 2 * y + 1),
        (next_z, 2 * x + 1, 2 * y + 1),
    ])
}

/// Return the parent tile (previous zoom level) for a given tile.
///
/// Returns `None` if `z == 0`.
pub fn tile_parent(z: u8, x: u32, y: u32) -> Option<(u8, u32, u32)> {
    let parent_z = z.checked_sub(1)?;
    Some((parent_z, x / 2, y / 2))
}

/// Enumerate all tiles at a given zoom level that intersect a bounding box.
///
/// `bbox` is `[west, south, east, north]` in degrees.
/// Returns an iterator of `(x, y)` tile coordinates.
pub fn tiles_in_bbox(bbox: [f64; 4], zoom: u8) -> impl Iterator<Item = (u32, u32)> {
    let [west, south, east, north] = bbox;
    let (x_min, y_max) = lonlat_to_tile(west, south, zoom);
    let (x_max, y_min) = lonlat_to_tile(east, north, zoom);

    let n = (1u32 << zoom).saturating_sub(1);
    let x_min = x_min.min(n);
    let x_max = x_max.min(n);
    let y_min = y_min.min(n);
    let y_max = y_max.min(n);

    (y_min..=y_max).flat_map(move |y| (x_min..=x_max).map(move |x| (x, y)))
}

// ─────────────────────────────────────────────────────────────────────────────
// TileSetMetadata builders
// ─────────────────────────────────────────────────────────────────────────────

impl TileSetMetadata {
    /// Create a minimal TileSetMetadata for vector tiles using WebMercatorQuad.
    pub fn vector_web_mercator(tile_url_template: impl Into<String>) -> Self {
        Self {
            tile_matrix_set_id: "WebMercatorQuad".into(),
            data_type: TileDataType::Vector,
            links: vec![TileLink {
                href: tile_url_template.into(),
                rel: "item".into(),
                media_type: Some("application/vnd.mapbox-vector-tile".into()),
                title: Some("Vector tiles".into()),
            }],
            title: None,
            description: None,
            attribution: None,
            extent: None,
            min_tile_matrix: Some("0".into()),
            max_tile_matrix: Some("24".into()),
        }
    }

    /// Create a minimal TileSetMetadata for map tiles using WebMercatorQuad.
    pub fn map_web_mercator(tile_url_template: impl Into<String>) -> Self {
        Self {
            tile_matrix_set_id: "WebMercatorQuad".into(),
            data_type: TileDataType::Map,
            links: vec![TileLink {
                href: tile_url_template.into(),
                rel: "item".into(),
                media_type: Some("image/png".into()),
                title: Some("Map tiles".into()),
            }],
            title: None,
            description: None,
            attribution: None,
            extent: None,
            min_tile_matrix: Some("0".into()),
            max_tile_matrix: Some("24".into()),
        }
    }

    /// Add a geographic extent to this TileSetMetadata.
    pub fn with_extent(mut self, west: f64, south: f64, east: f64, north: f64) -> Self {
        self.extent = Some(GeographicBoundingBox {
            lower_left: [west, south],
            upper_right: [east, north],
        });
        self
    }

    /// Set the min/max tile matrix zoom levels.
    pub fn with_zoom_range(mut self, min_zoom: u8, max_zoom: u8) -> Self {
        self.min_tile_matrix = Some(min_zoom.to_string());
        self.max_tile_matrix = Some(max_zoom.to_string());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── TileMatrixSet construction ──────────────────────────────────────────

    #[test]
    fn test_web_mercator_quad_zoom_count() {
        let tms = TileMatrixSet::web_mercator_quad();
        assert_eq!(
            tms.zoom_level_count(),
            25,
            "WebMercatorQuad should have zoom 0–24 (25 levels)"
        );
    }

    #[test]
    fn test_web_mercator_quad_max_zoom() {
        let tms = TileMatrixSet::web_mercator_quad();
        assert_eq!(tms.max_zoom(), 24);
    }

    #[test]
    fn test_web_mercator_quad_min_zoom() {
        let tms = TileMatrixSet::web_mercator_quad();
        assert_eq!(tms.min_zoom(), 0);
    }

    #[test]
    fn test_web_mercator_quad_zoom0_matrix_size() {
        let tms = TileMatrixSet::web_mercator_quad();
        let m = tms.tile_matrix(0).expect("zoom 0 must exist");
        assert_eq!(m.matrix_width, 1);
        assert_eq!(m.matrix_height, 1);
    }

    #[test]
    fn test_web_mercator_quad_zoom1_matrix_size() {
        let tms = TileMatrixSet::web_mercator_quad();
        let m = tms.tile_matrix(1).expect("zoom 1 must exist");
        assert_eq!(m.matrix_width, 2);
        assert_eq!(m.matrix_height, 2);
    }

    #[test]
    fn test_web_mercator_quad_zoom24_matrix_size() {
        let tms = TileMatrixSet::web_mercator_quad();
        let m = tms.tile_matrix(24).expect("zoom 24 must exist");
        assert_eq!(m.matrix_width, 1 << 24);
        assert_eq!(m.matrix_height, 1 << 24);
    }

    #[test]
    fn test_web_mercator_quad_tile_matrix_none_for_25() {
        let tms = TileMatrixSet::web_mercator_quad();
        assert!(tms.tile_matrix(25).is_none());
    }

    #[test]
    fn test_world_crs84_quad_zoom_count() {
        let tms = TileMatrixSet::world_crs84_quad();
        assert_eq!(
            tms.zoom_level_count(),
            18,
            "WorldCRS84Quad should have zoom 0–17 (18 levels)"
        );
    }

    #[test]
    fn test_world_crs84_quad_max_zoom() {
        let tms = TileMatrixSet::world_crs84_quad();
        assert_eq!(tms.max_zoom(), 17);
    }

    #[test]
    fn test_world_crs84_quad_zoom0_aspect_ratio() {
        // WorldCRS84Quad zoom 0: 2 columns × 1 row (2:1 world aspect ratio)
        let tms = TileMatrixSet::world_crs84_quad();
        let m = tms.tile_matrix(0).expect("zoom 0 must exist");
        assert_eq!(m.matrix_width, 2);
        assert_eq!(m.matrix_height, 1);
    }

    #[test]
    fn test_world_crs84_quad_zoom1_size() {
        let tms = TileMatrixSet::world_crs84_quad();
        let m = tms.tile_matrix(1).expect("zoom 1 must exist");
        assert_eq!(m.matrix_width, 4);
        assert_eq!(m.matrix_height, 2);
    }

    #[test]
    fn test_world_crs84_quad_tile_matrix_none_for_18() {
        let tms = TileMatrixSet::world_crs84_quad();
        assert!(tms.tile_matrix(18).is_none());
    }

    #[test]
    fn test_tile_matrix_corner_of_origin() {
        let tms = TileMatrixSet::web_mercator_quad();
        let m = tms.tile_matrix(0).expect("zoom 0 must exist");
        assert_eq!(m.corner_of_origin, CornerOfOrigin::TopLeft);
    }

    #[test]
    fn test_tile_matrix_tile_size() {
        let tms = TileMatrixSet::web_mercator_quad();
        let m = tms.tile_matrix(10).expect("zoom 10 must exist");
        assert_eq!(m.tile_width, 256);
        assert_eq!(m.tile_height, 256);
    }

    #[test]
    fn test_tile_matrix_scale_decreases_with_zoom() {
        let tms = TileMatrixSet::web_mercator_quad();
        let m0 = tms.tile_matrix(0).expect("zoom 0");
        let m1 = tms.tile_matrix(1).expect("zoom 1");
        assert!(
            m0.scale_denominator > m1.scale_denominator,
            "Scale denominator should decrease as zoom increases"
        );
    }

    // ── tile_to_bbox ────────────────────────────────────────────────────────

    #[test]
    fn test_tile_to_bbox_zoom0_full_world() {
        let [west, south, east, north] = tile_to_bbox(0, 0, 0);
        assert!((west - (-180.0)).abs() < 1e-6, "west={}", west);
        assert!((east - 180.0).abs() < 1e-6, "east={}", east);
        // Mercator clips at ~±85.051°
        assert!(south < -85.0, "south={}", south);
        assert!(north > 85.0, "north={}", north);
    }

    #[test]
    fn test_tile_to_bbox_zoom1_nw_quadrant() {
        let [west, south, east, north] = tile_to_bbox(1, 0, 0);
        assert!((west - (-180.0)).abs() < 1e-6);
        assert!((east - 0.0).abs() < 1e-6);
        assert!(north > 0.0);
        assert!(south > 0.0 || south.abs() < 1e-6);
    }

    #[test]
    fn test_tile_to_bbox_zoom1_se_quadrant() {
        let [west, south, east, north] = tile_to_bbox(1, 1, 1);
        assert!((west - 0.0).abs() < 1e-6);
        assert!((east - 180.0).abs() < 1e-6);
        assert!(south < 0.0);
        assert!(north.abs() < 1e-6 || north > -1.0);
    }

    #[test]
    fn test_tile_to_bbox_ordering() {
        // west < east, south < north for any valid tile
        for z in 0u8..=5 {
            let n = 1u32 << z;
            for x in 0..n {
                for y in 0..n {
                    let [west, south, east, north] = tile_to_bbox(z, x, y);
                    assert!(west < east, "z={} x={} y={}: west >= east", z, x, y);
                    assert!(south < north, "z={} x={} y={}: south >= north", z, x, y);
                }
            }
        }
    }

    // ── lonlat_to_tile ──────────────────────────────────────────────────────

    #[test]
    fn test_lonlat_to_tile_zoom0_any_point() {
        // At zoom 0 there is only one tile (0,0)
        assert_eq!(lonlat_to_tile(0.0, 0.0, 0), (0, 0));
        assert_eq!(lonlat_to_tile(-90.0, 45.0, 0), (0, 0));
        assert_eq!(lonlat_to_tile(90.0, -45.0, 0), (0, 0));
    }

    #[test]
    fn test_lonlat_to_tile_top_left_zoom1() {
        // Top-left tile at zoom 1
        let (x, y) = lonlat_to_tile(-179.999, 84.999, 1);
        assert_eq!((x, y), (0, 0), "top-left should be (0,0) got ({},{})", x, y);
    }

    #[test]
    fn test_lonlat_to_tile_bottom_right_zoom1() {
        let (x, y) = lonlat_to_tile(179.999, -84.999, 1);
        assert_eq!(
            (x, y),
            (1, 1),
            "bottom-right at zoom 1 should be (1,1) got ({},{})",
            x,
            y
        );
    }

    #[test]
    fn test_lonlat_to_tile_prime_meridian_equator_zoom8() {
        let (x, y) = lonlat_to_tile(0.0, 0.0, 8);
        // (0,0) is at the intersection: x should be 128, y near 128
        assert_eq!(x, 128);
        assert_eq!(y, 128);
    }

    #[test]
    fn test_lonlat_to_tile_roundtrip_consistency() {
        // tile_to_bbox and lonlat_to_tile should be consistent
        for z in 0u8..=6 {
            let n = 1u32 << z;
            for x in 0..n {
                for y in 0..n {
                    let [west, _south, _east, north] = tile_to_bbox(z, x, y);
                    // Center of the tile's northern edge should map back to the same tile
                    let center_lon = (west + _east) / 2.0;
                    let (tx, ty) = lonlat_to_tile(center_lon, north - 0.0001, z);
                    assert_eq!(
                        (tx, ty),
                        (x, y),
                        "z={} x={} y={}: center mapped to ({},{})",
                        z,
                        x,
                        y,
                        tx,
                        ty
                    );
                }
            }
        }
    }

    // ── validate_tile_coords ────────────────────────────────────────────────

    #[test]
    fn test_validate_tile_coords_valid() {
        assert!(validate_tile_coords(0, 0, 0));
        assert!(validate_tile_coords(10, 0, 0));
        assert!(validate_tile_coords(10, 1023, 1023));
    }

    #[test]
    fn test_validate_tile_coords_out_of_range() {
        assert!(!validate_tile_coords(0, 1, 0));
        assert!(!validate_tile_coords(0, 0, 1));
        assert!(!validate_tile_coords(10, 1024, 0));
    }

    // ── tile_children / tile_parent ─────────────────────────────────────────

    #[test]
    fn test_tile_children_count() {
        let children = tile_children(5, 10, 7).expect("should have children");
        assert_eq!(children.len(), 4);
    }

    #[test]
    fn test_tile_children_zoom_incremented() {
        let children = tile_children(3, 2, 2).expect("should have children");
        for (cz, _, _) in &children {
            assert_eq!(*cz, 4);
        }
    }

    #[test]
    fn test_tile_parent_basic() {
        let (pz, px, py) = tile_parent(5, 10, 7).expect("should have parent");
        assert_eq!(pz, 4);
        assert_eq!(px, 5);
        assert_eq!(py, 3);
    }

    #[test]
    fn test_tile_parent_none_at_zoom0() {
        assert!(tile_parent(0, 0, 0).is_none());
    }

    // ── tiles_in_bbox ───────────────────────────────────────────────────────

    #[test]
    fn test_tiles_in_bbox_zoom0_world() {
        let tiles: Vec<_> = tiles_in_bbox([-180.0, -85.0, 180.0, 85.0], 0).collect();
        assert_eq!(tiles.len(), 1, "zoom 0 whole world = 1 tile");
    }

    #[test]
    fn test_tiles_in_bbox_zoom1_world() {
        let tiles: Vec<_> = tiles_in_bbox([-180.0, -85.0, 180.0, 85.0], 1).collect();
        assert_eq!(tiles.len(), 4, "zoom 1 whole world = 4 tiles");
    }

    // ── TileSetMetadata ──────────────────────────────────────────────────────

    #[test]
    fn test_tileset_metadata_vector_web_mercator() {
        let meta = TileSetMetadata::vector_web_mercator("https://tiles/{z}/{x}/{y}.mvt");
        assert_eq!(meta.tile_matrix_set_id, "WebMercatorQuad");
        assert_eq!(meta.data_type, TileDataType::Vector);
        assert!(!meta.links.is_empty());
    }

    #[test]
    fn test_tileset_metadata_map_web_mercator() {
        let meta = TileSetMetadata::map_web_mercator("https://tiles/{z}/{x}/{y}.png");
        assert_eq!(meta.data_type, TileDataType::Map);
        assert!(meta.links[0].href.contains("{z}"));
    }

    #[test]
    fn test_tileset_metadata_with_extent() {
        let meta = TileSetMetadata::vector_web_mercator("https://tiles/{z}/{x}/{y}.mvt")
            .with_extent(-10.0, 35.0, 40.0, 70.0);
        let ext = meta.extent.expect("extent should be set");
        assert_eq!(ext.lower_left, [-10.0, 35.0]);
        assert_eq!(ext.upper_right, [40.0, 70.0]);
    }

    #[test]
    fn test_tileset_metadata_serialization_roundtrip() {
        let meta =
            TileSetMetadata::vector_web_mercator("https://example.com/tiles/{z}/{x}/{y}.mvt")
                .with_extent(-180.0, -90.0, 180.0, 90.0)
                .with_zoom_range(0, 14);

        let json = serde_json::to_string(&meta).expect("serialization should succeed");
        let decoded: TileSetMetadata =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(decoded.tile_matrix_set_id, "WebMercatorQuad");
        assert_eq!(decoded.min_tile_matrix.as_deref(), Some("0"));
        assert_eq!(decoded.max_tile_matrix.as_deref(), Some("14"));
    }

    #[test]
    fn test_tile_link_serialization() {
        let link = TileLink {
            href: "https://example.com/tiles/0/0/0.mvt".into(),
            rel: "item".into(),
            media_type: Some("application/vnd.mapbox-vector-tile".into()),
            title: Some("Vector tile".into()),
        };
        let json = serde_json::to_string(&link).expect("serialization should succeed");
        assert!(json.contains("application/vnd.mapbox-vector-tile"));
        assert!(json.contains("item"));
    }

    #[test]
    fn test_tile_data_type_variants() {
        assert_ne!(TileDataType::Map, TileDataType::Vector);
        assert_ne!(TileDataType::Vector, TileDataType::Coverage);
        assert_ne!(TileDataType::Map, TileDataType::Coverage);
    }

    #[test]
    fn test_corner_of_origin_variants() {
        assert_ne!(CornerOfOrigin::TopLeft, CornerOfOrigin::BottomLeft);
    }

    #[test]
    fn test_geographic_bounding_box() {
        let bbox = GeographicBoundingBox {
            lower_left: [-10.0, 35.0],
            upper_right: [40.0, 70.0],
        };
        assert_eq!(bbox.lower_left[0], -10.0);
        assert_eq!(bbox.upper_right[1], 70.0);
    }

    #[test]
    fn test_conformance_declaration_ogc_tiles() {
        let conf = ConformanceDeclaration::ogc_tiles();
        assert!(!conf.conforms_to.is_empty());
        let has_core = conf
            .conforms_to
            .iter()
            .any(|c| c.contains("ogcapi-tiles-1") && c.contains("conf/core"));
        assert!(has_core, "should include OGC Tiles core conformance class");
    }

    #[test]
    fn test_conformance_declaration_serialization() {
        let conf = ConformanceDeclaration::ogc_tiles();
        let json = serde_json::to_string(&conf).expect("serialization should succeed");
        let decoded: ConformanceDeclaration =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(decoded.conforms_to.len(), conf.conforms_to.len());
    }

    #[test]
    fn test_tile_pixel_bounds() {
        let (x0, y0, x1, y1) = tile_to_pixel_bounds(0, 0, 0);
        assert_eq!(x0, 0);
        assert_eq!(y0, 0);
        assert_eq!(x1, 256);
        assert_eq!(y1, 256);

        let (x0, y0, x1, y1) = tile_to_pixel_bounds(1, 1, 1);
        assert_eq!(x0, 256);
        assert_eq!(y0, 256);
        assert_eq!(x1, 512);
        assert_eq!(y1, 512);
    }

    #[test]
    fn test_tile_matrix_set_id_and_crs() {
        let tms = TileMatrixSet::web_mercator_quad();
        assert_eq!(tms.id, "WebMercatorQuad");
        assert!(tms.crs.contains("3857"));

        let tms2 = TileMatrixSet::world_crs84_quad();
        assert_eq!(tms2.id, "WorldCRS84Quad");
        assert!(tms2.crs.contains("CRS84") || tms2.crs.contains("4326"));
    }
}
