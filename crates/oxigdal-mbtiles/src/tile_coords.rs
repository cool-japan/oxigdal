//! Tile coordinate types and TMS ↔ XYZ conversion helpers.

/// A tile coordinate expressed as (zoom, x, y) in the XYZ scheme.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TileCoord {
    /// Zoom level (0 = world overview).
    pub z: u8,
    /// Tile column (west → east).
    pub x: u32,
    /// Tile row (north → south in XYZ).
    pub y: u32,
}

/// The image (or vector) format used for tile data.
#[derive(Debug, Clone, PartialEq)]
pub enum TileFormat {
    /// PNG raster tile.
    Png,
    /// JPEG raster tile.
    Jpeg,
    /// WebP raster tile.
    Webp,
    /// Mapbox Vector Tile (protocol buffer).
    Pbf,
    /// Any other format identified by its MIME-type string.
    Unknown(String),
}

impl TileFormat {
    /// Parse a format string (case-insensitive) as used in MBTiles metadata.
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "png" => Self::Png,
            "jpg" | "jpeg" => Self::Jpeg,
            "webp" => Self::Webp,
            "pbf" => Self::Pbf,
            other => Self::Unknown(other.to_string()),
        }
    }

    /// Return the MIME type string for this format.
    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Pbf => "application/x-protobuf",
            Self::Unknown(_) => "application/octet-stream",
        }
    }

    /// Return `true` for vector tile formats.
    pub fn is_vector(&self) -> bool {
        matches!(self, Self::Pbf)
    }

    /// Return `true` for raster tile formats.
    pub fn is_raster(&self) -> bool {
        matches!(self, Self::Png | Self::Jpeg | Self::Webp)
    }
}

/// Convert a TMS y-coordinate to XYZ (flips the y axis).
///
/// TMS counts rows from south to north; XYZ counts from north to south.
/// The conversion is symmetric: `tms_to_xyz(z, xyz_to_tms(z, y)) == y`.
pub fn tms_to_xyz(z: u8, tms_y: u32) -> u32 {
    let n = 1u32 << z; // 2^z tiles on each axis
    n.saturating_sub(1).saturating_sub(tms_y)
}

/// Convert an XYZ y-coordinate to TMS (identical formula — the mapping is its own inverse).
pub fn xyz_to_tms(z: u8, xyz_y: u32) -> u32 {
    tms_to_xyz(z, xyz_y)
}
