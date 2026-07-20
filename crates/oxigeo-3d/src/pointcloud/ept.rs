//! EPT (Entwine Point Tiles) format support
//!
//! Provides streaming access to EPT format point clouds with octree structure.

use crate::error::{Error, Result};
use crate::pointcloud::{Bounds3d, Classification, ColorRgb, Point};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(feature = "async")]
use reqwest::Client;

/// EPT metadata structure (from ept.json)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EptMetadata {
    /// Bounds
    pub bounds: [f64; 6],
    /// Conforming bounds
    #[serde(rename = "boundsConforming")]
    pub bounds_conforming: [f64; 6],
    /// Data type (laszip, binary, zstandard)
    #[serde(rename = "dataType")]
    pub data_type: String,
    /// Hierarchical structure type
    #[serde(rename = "hierarchyType")]
    pub hierarchy_type: String,
    /// Number of points
    pub points: u64,
    /// Spatial reference system
    pub srs: Option<EptSrs>,
    /// Span (octree cell size at root)
    pub span: u64,
    /// Version
    pub version: String,
    /// Schema (point attributes)
    pub schema: Vec<EptSchemaField>,
}

/// EPT SRS (Spatial Reference System)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EptSrs {
    /// Authority (e.g., "EPSG")
    pub authority: Option<String>,
    /// Horizontal reference
    pub horizontal: Option<String>,
    /// Vertical reference
    pub vertical: Option<String>,
    /// WKT (Well-Known Text)
    pub wkt: Option<String>,
}

/// EPT schema field
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EptSchemaField {
    /// Field name
    pub name: String,
    /// Data type (signed, unsigned, float, double)
    #[serde(rename = "type")]
    pub data_type: String,
    /// Size in bytes
    pub size: u32,
    /// Scale factor (optional)
    pub scale: Option<f64>,
    /// Offset value (optional)
    pub offset: Option<f64>,
}

/// EPT octree key
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OctreeKey {
    /// Depth level (0 = root)
    pub d: u32,
    /// X index
    pub x: u32,
    /// Y index
    pub y: u32,
    /// Z index
    pub z: u32,
}

impl OctreeKey {
    /// Create a new octree key
    pub fn new(d: u32, x: u32, y: u32, z: u32) -> Self {
        Self { d, x, y, z }
    }

    /// Get root key
    pub fn root() -> Self {
        Self::new(0, 0, 0, 0)
    }

    /// Convert to string representation (e.g., "0-0-0-0")
    pub fn to_key_string(&self) -> String {
        format!("{}-{}-{}-{}", self.d, self.x, self.y, self.z)
    }

    /// Parse from string representation
    pub fn from_string(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 4 {
            return Err(Error::Ept(format!("Invalid octree key: {}", s)));
        }

        let d = parts[0]
            .parse::<u32>()
            .map_err(|_| Error::Ept(format!("Invalid depth: {}", parts[0])))?;
        let x = parts[1]
            .parse::<u32>()
            .map_err(|_| Error::Ept(format!("Invalid x: {}", parts[1])))?;
        let y = parts[2]
            .parse::<u32>()
            .map_err(|_| Error::Ept(format!("Invalid y: {}", parts[2])))?;
        let z = parts[3]
            .parse::<u32>()
            .map_err(|_| Error::Ept(format!("Invalid z: {}", parts[3])))?;

        Ok(Self::new(d, x, y, z))
    }

    /// Get child keys
    pub fn children(&self) -> [OctreeKey; 8] {
        let d = self.d + 1;
        let x = self.x * 2;
        let y = self.y * 2;
        let z = self.z * 2;

        [
            OctreeKey::new(d, x, y, z),
            OctreeKey::new(d, x + 1, y, z),
            OctreeKey::new(d, x, y + 1, z),
            OctreeKey::new(d, x + 1, y + 1, z),
            OctreeKey::new(d, x, y, z + 1),
            OctreeKey::new(d, x + 1, y, z + 1),
            OctreeKey::new(d, x, y + 1, z + 1),
            OctreeKey::new(d, x + 1, y + 1, z + 1),
        ]
    }

    /// Calculate bounds for this key
    pub fn bounds(&self, metadata: &EptMetadata) -> Bounds3d {
        let [min_x, min_y, min_z, max_x, max_y, max_z] = metadata.bounds;
        let width = max_x - min_x;
        let height = max_y - min_y;
        let depth = max_z - min_z;

        let cells = 1u32 << self.d; // 2^d
        let cell_width = width / cells as f64;
        let cell_height = height / cells as f64;
        let cell_depth = depth / cells as f64;

        let x0 = min_x + self.x as f64 * cell_width;
        let y0 = min_y + self.y as f64 * cell_height;
        let z0 = min_z + self.z as f64 * cell_depth;

        Bounds3d::new(
            x0,
            x0 + cell_width,
            y0,
            y0 + cell_height,
            z0,
            z0 + cell_depth,
        )
    }
}

/// EPT hierarchy information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EptHierarchyPage {
    /// Map of octree key string to point count
    #[serde(flatten)]
    pub counts: std::collections::HashMap<String, i64>,
}

/// EPT octree structure
#[derive(Debug, Clone)]
pub struct EptOctree {
    metadata: EptMetadata,
    hierarchy: std::collections::HashMap<OctreeKey, i64>,
}

impl EptOctree {
    /// Create a new octree from metadata
    pub fn new(metadata: EptMetadata) -> Self {
        Self {
            metadata,
            hierarchy: std::collections::HashMap::new(),
        }
    }

    /// Load hierarchy page
    pub fn load_hierarchy_page(&mut self, page: EptHierarchyPage) -> Result<()> {
        for (key_str, count) in page.counts {
            let key = OctreeKey::from_string(&key_str)?;
            self.hierarchy.insert(key, count);
        }
        Ok(())
    }

    /// Get point count for a key
    pub fn point_count(&self, key: &OctreeKey) -> Option<i64> {
        self.hierarchy.get(key).copied()
    }

    /// Find keys within bounds
    pub fn find_in_bounds(&self, bounds: &Bounds3d) -> Vec<OctreeKey> {
        self.hierarchy
            .keys()
            .filter(|key| {
                let key_bounds = key.bounds(&self.metadata);
                key_bounds.intersects(bounds)
            })
            .copied()
            .collect()
    }

    /// Get metadata
    pub fn metadata(&self) -> &EptMetadata {
        &self.metadata
    }
}

// ---------------------------------------------------------------------------
// Tile decoding
// ---------------------------------------------------------------------------

/// Read a little-endian signed integer of `size` (1, 2, 4 or 8) bytes,
/// sign-extended to `i64`. Returns `0` if `bytes` is shorter than `size`.
fn read_signed_le(bytes: &[u8], size: usize) -> i64 {
    let mut buf = [0u8; 8];
    let n = size.min(8).min(bytes.len());
    buf[..n].copy_from_slice(&bytes[..n]);
    let raw = i64::from_le_bytes(buf);
    // Sign-extend from `size` bytes when the high bit of the top byte is set.
    if n > 0 && n < 8 && (bytes[n - 1] & 0x80) != 0 {
        let shift = (8 - n) * 8;
        (raw << shift) >> shift
    } else {
        raw
    }
}

/// Read a little-endian unsigned integer of `size` (1, 2, 4 or 8) bytes into a
/// `u64`. Returns `0` if `bytes` is shorter than `size`.
fn read_unsigned_le(bytes: &[u8], size: usize) -> u64 {
    let mut buf = [0u8; 8];
    let n = size.min(8).min(bytes.len());
    buf[..n].copy_from_slice(&bytes[..n]);
    u64::from_le_bytes(buf)
}

/// Read a little-endian floating value of `size` (4 or 8) bytes as `f64`.
fn read_float_le(bytes: &[u8], size: usize) -> f64 {
    match size {
        4 if bytes.len() >= 4 => {
            let mut b = [0u8; 4];
            b.copy_from_slice(&bytes[..4]);
            f32::from_le_bytes(b) as f64
        }
        8 if bytes.len() >= 8 => {
            let mut b = [0u8; 8];
            b.copy_from_slice(&bytes[..8]);
            f64::from_le_bytes(b)
        }
        _ => 0.0,
    }
}

/// Extract a schema field's value as an `f64`, honouring its declared numeric
/// kind. Coordinate fields apply the per-field `scale`/`offset` afterwards.
fn field_as_f64(field: &EptSchemaField, bytes: &[u8]) -> f64 {
    let raw = match field.data_type.as_str() {
        "signed" => read_signed_le(bytes, field.size as usize) as f64,
        "unsigned" => read_unsigned_le(bytes, field.size as usize) as f64,
        "float" | "floating" | "double" => read_float_le(bytes, field.size as usize),
        _ => read_signed_le(bytes, field.size as usize) as f64,
    };
    raw * field.scale.unwrap_or(1.0) + field.offset.unwrap_or(0.0)
}

/// Extract a schema field's value as an unsigned integer (for attribute
/// dimensions such as intensity or classification).
fn field_as_u64(field: &EptSchemaField, bytes: &[u8]) -> u64 {
    match field.data_type.as_str() {
        "signed" => read_signed_le(bytes, field.size as usize).max(0) as u64,
        "float" | "floating" | "double" => read_float_le(bytes, field.size as usize) as u64,
        _ => read_unsigned_le(bytes, field.size as usize),
    }
}

/// Decode raw, fixed-stride binary point records (EPT `binary` / decompressed
/// `zstandard` data type) into [`Point`]s using the dataset schema.
///
/// Each record is `sum(field.size)` bytes; fields are laid out in schema order.
/// Recognised dimension names (`X`/`Y`/`Z`, `Intensity`, `ReturnNumber`,
/// `NumberOfReturns`, `Classification`, `ScanAngleRank`/`ScanAngle`,
/// `UserData`, `PointSourceId`, `GpsTime`, `Red`/`Green`/`Blue`, `NIR`/`Nir`)
/// are mapped onto the corresponding [`Point`] attributes; unrecognised
/// dimensions are skipped.
///
/// # Errors
/// Returns [`Error::Ept`] when the schema has no positional dimensions, has a
/// zero record length, or the buffer is not a whole multiple of the record
/// length.
fn parse_binary_points(data: &[u8], schema: &[EptSchemaField]) -> Result<Vec<Point>> {
    // Compute per-field byte offsets within a record and the total stride.
    let mut offsets = Vec::with_capacity(schema.len());
    let mut record_length = 0usize;
    for field in schema {
        offsets.push(record_length);
        record_length += field.size as usize;
    }

    if record_length == 0 {
        return Err(Error::Ept(
            "EPT schema defines a zero-length point record".to_string(),
        ));
    }
    let has_xyz = schema
        .iter()
        .any(|f| matches!(f.name.as_str(), "X" | "Y" | "Z"));
    if !has_xyz {
        return Err(Error::Ept(
            "EPT schema is missing X/Y/Z positional dimensions".to_string(),
        ));
    }
    if !data.len().is_multiple_of(record_length) {
        return Err(Error::Ept(format!(
            "EPT binary tile length {} is not a multiple of record length {record_length}",
            data.len()
        )));
    }

    let count = data.len() / record_length;
    let mut points = Vec::with_capacity(count);

    for i in 0..count {
        let record_start = i * record_length;
        let record = &data[record_start..record_start + record_length];

        let mut point = Point {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            intensity: 0,
            return_number: 0,
            number_of_returns: 0,
            classification: Classification::NeverClassified,
            scan_angle: 0,
            user_data: 0,
            point_source_id: 0,
            gps_time: None,
            color: None,
            nir: None,
        };
        let mut red: Option<u16> = None;
        let mut green: Option<u16> = None;
        let mut blue: Option<u16> = None;

        for (field, &off) in schema.iter().zip(offsets.iter()) {
            let end = (off + field.size as usize).min(record.len());
            let field_bytes = &record[off..end];
            match field.name.as_str() {
                "X" => point.x = field_as_f64(field, field_bytes),
                "Y" => point.y = field_as_f64(field, field_bytes),
                "Z" => point.z = field_as_f64(field, field_bytes),
                "Intensity" => point.intensity = field_as_u64(field, field_bytes) as u16,
                "ReturnNumber" => point.return_number = field_as_u64(field, field_bytes) as u8,
                "NumberOfReturns" => {
                    point.number_of_returns = field_as_u64(field, field_bytes) as u8
                }
                "Classification" => {
                    point.classification =
                        Classification::from(field_as_u64(field, field_bytes) as u8)
                }
                "ScanAngleRank" | "ScanAngle" => {
                    point.scan_angle = read_signed_le(field_bytes, field.size as usize) as i16
                }
                "UserData" => point.user_data = field_as_u64(field, field_bytes) as u8,
                "PointSourceId" => point.point_source_id = field_as_u64(field, field_bytes) as u16,
                "GpsTime" => point.gps_time = Some(field_as_f64(field, field_bytes)),
                "Red" => red = Some(field_as_u64(field, field_bytes) as u16),
                "Green" => green = Some(field_as_u64(field, field_bytes) as u16),
                "Blue" => blue = Some(field_as_u64(field, field_bytes) as u16),
                "NIR" | "Nir" => point.nir = Some(field_as_u64(field, field_bytes) as u16),
                _ => {}
            }
        }

        if let (Some(r), Some(g), Some(b)) = (red, green, blue) {
            point.color = Some(ColorRgb {
                red: r,
                green: g,
                blue: b,
            });
        }
        points.push(point);
    }

    Ok(points)
}

/// Decode a single EPT tile's raw bytes into [`Point`]s according to the
/// dataset's `dataType`.
///
/// * `binary` — fixed-stride records parsed directly via the schema.
/// * `zstandard` — Zstandard-compressed `binary` records (decompressed with the
///   pure-Rust [`oxiarc_zstd`] decoder, then parsed via the schema).
/// * `laszip` — standalone LAZ tiles. Decoding these requires a full LAZ file
///   reader that is not yet wired in; rather than silently returning no points,
///   this surfaces an explicit [`Error::Unsupported`].
///
/// # Errors
/// Returns [`Error::Ept`], [`Error::Decompression`] or [`Error::Unsupported`]
/// depending on the failure.
fn decode_ept_tile(data: &[u8], metadata: &EptMetadata) -> Result<Vec<Point>> {
    match metadata.data_type.as_str() {
        "binary" => parse_binary_points(data, &metadata.schema),
        "zstandard" => {
            let raw = oxiarc_zstd::decompress(data)
                .map_err(|e| Error::Decompression(format!("EPT zstandard tile: {e}")))?;
            parse_binary_points(&raw, &metadata.schema)
        }
        "laszip" => Err(Error::Unsupported(
            "EPT 'laszip' tiles require a standalone LAZ file reader that is not yet wired \
             into oxigeo-3d; re-export the dataset with dataType 'binary' or 'zstandard', \
             or read the .laz tiles with a dedicated LAZ reader"
                .to_string(),
        )),
        other => Err(Error::Ept(format!("unknown EPT dataType '{other}'"))),
    }
}

/// EPT reader for local files
pub struct EptReader {
    root_path: PathBuf,
    metadata: EptMetadata,
    octree: EptOctree,
}

impl EptReader {
    /// Open an EPT dataset from directory
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let root_path = path.as_ref().to_path_buf();

        // Read ept.json
        let metadata_path = root_path.join("ept.json");
        let metadata_str = fs::read_to_string(&metadata_path)
            .map_err(|e| Error::Ept(format!("Failed to read ept.json: {}", e)))?;
        let metadata: EptMetadata = serde_json::from_str(&metadata_str)?;

        // Create octree
        let mut octree = EptOctree::new(metadata.clone());

        // Load root hierarchy
        let hierarchy_path = root_path.join("ept-hierarchy").join("0-0-0-0.json");
        if hierarchy_path.exists() {
            let hierarchy_str = fs::read_to_string(&hierarchy_path)
                .map_err(|e| Error::Ept(format!("Failed to read hierarchy: {}", e)))?;
            let page: EptHierarchyPage = serde_json::from_str(&hierarchy_str)?;
            octree.load_hierarchy_page(page)?;
        }

        Ok(Self {
            root_path,
            metadata,
            octree,
        })
    }

    /// Get metadata
    pub fn metadata(&self) -> &EptMetadata {
        &self.metadata
    }

    /// Get octree
    pub fn octree(&self) -> &EptOctree {
        &self.octree
    }

    /// Read points from a tile
    pub fn read_tile(&self, key: &OctreeKey) -> Result<Vec<Point>> {
        let tile_path = self.tile_path(key);

        if !tile_path.exists() {
            return Ok(Vec::new());
        }

        // Read the tile and decode it according to the dataset `dataType`
        // (binary / zstandard / laszip).
        let data = fs::read(&tile_path)?;
        decode_ept_tile(&data, &self.metadata)
    }

    /// Get tile file path
    fn tile_path(&self, key: &OctreeKey) -> PathBuf {
        let filename = match self.metadata.data_type.as_str() {
            "laszip" => format!("{}.laz", key.to_key_string()),
            "binary" => format!("{}.bin", key.to_key_string()),
            "zstandard" => format!("{}.zst", key.to_key_string()),
            _ => format!("{}.bin", key.to_key_string()),
        };

        self.root_path.join("ept-data").join(filename)
    }

    /// Query points within bounds
    pub fn query_bounds(&self, bounds: &Bounds3d) -> Result<Vec<Point>> {
        let keys = self.octree.find_in_bounds(bounds);
        let mut all_points = Vec::new();

        for key in keys {
            let points = self.read_tile(&key)?;
            all_points.extend(points);
        }

        Ok(all_points)
    }

    /// Load additional hierarchy pages
    pub fn load_hierarchy_for_key(&mut self, key: &OctreeKey) -> Result<()> {
        let hierarchy_path = self
            .root_path
            .join("ept-hierarchy")
            .join(format!("{}.json", key.to_key_string()));

        if !hierarchy_path.exists() {
            return Ok(());
        }

        let hierarchy_str = fs::read_to_string(&hierarchy_path)?;
        let page: EptHierarchyPage = serde_json::from_str(&hierarchy_str)?;
        self.octree.load_hierarchy_page(page)?;

        Ok(())
    }
}

/// EPT HTTP reader
#[cfg(feature = "async")]
pub struct EptHttpReader {
    base_url: String,
    client: Client,
    metadata: EptMetadata,
    #[allow(dead_code)]
    octree: EptOctree,
}

#[cfg(feature = "async")]
impl EptHttpReader {
    /// Open an EPT dataset via HTTP
    pub async fn open(base_url: impl Into<String>) -> Result<Self> {
        let base_url = base_url.into();
        let client = Client::new();

        // Fetch ept.json
        let metadata_url = format!("{}/ept.json", base_url);
        let response = client.get(&metadata_url).send().await?;
        let metadata: EptMetadata = response.json().await?;

        // Create octree
        let mut octree = EptOctree::new(metadata.clone());

        // Load root hierarchy
        let hierarchy_url = format!("{}/ept-hierarchy/0-0-0-0.json", base_url);
        if let Ok(response) = client.get(&hierarchy_url).send().await
            && response.status().is_success()
        {
            let page: EptHierarchyPage = response.json().await?;
            octree.load_hierarchy_page(page)?;
        }

        Ok(Self {
            base_url,
            client,
            metadata,
            octree,
        })
    }

    /// Get metadata
    pub fn metadata(&self) -> &EptMetadata {
        &self.metadata
    }

    /// Read tile via HTTP
    pub async fn read_tile(&self, key: &OctreeKey) -> Result<Vec<Point>> {
        let extension = match self.metadata.data_type.as_str() {
            "laszip" => "laz",
            "binary" => "bin",
            "zstandard" => "zst",
            _ => "bin",
        };

        let tile_url = format!(
            "{}/ept-data/{}.{}",
            self.base_url,
            key.to_key_string(),
            extension
        );

        let response = self.client.get(&tile_url).send().await?;
        if !response.status().is_success() {
            return Ok(Vec::new());
        }

        let data = response.bytes().await?;
        decode_ept_tile(&data, &self.metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_octree_key_root() {
        let root = OctreeKey::root();
        assert_eq!(root.d, 0);
        assert_eq!(root.x, 0);
        assert_eq!(root.y, 0);
        assert_eq!(root.z, 0);
    }

    #[test]
    fn test_octree_key_string() {
        let key = OctreeKey::new(1, 2, 3, 4);
        let s = key.to_key_string();
        assert_eq!(s, "1-2-3-4");

        let parsed =
            OctreeKey::from_string(&s).expect("Valid octree key string should parse successfully");
        assert_eq!(parsed, key);
    }

    #[test]
    fn test_octree_key_children() {
        let root = OctreeKey::root();
        let children = root.children();

        assert_eq!(children.len(), 8);
        assert_eq!(children[0], OctreeKey::new(1, 0, 0, 0));
        assert_eq!(children[7], OctreeKey::new(1, 1, 1, 1));
    }

    #[test]
    fn test_octree_key_bounds() {
        let metadata = EptMetadata {
            bounds: [0.0, 0.0, 0.0, 100.0, 100.0, 100.0],
            bounds_conforming: [0.0, 0.0, 0.0, 100.0, 100.0, 100.0],
            data_type: "laszip".to_string(),
            hierarchy_type: "json".to_string(),
            points: 1000,
            srs: None,
            span: 128,
            version: "1.0.0".to_string(),
            schema: vec![],
        };

        let root = OctreeKey::root();
        let bounds = root.bounds(&metadata);

        assert_eq!(bounds.min_x, 0.0);
        assert_eq!(bounds.max_x, 100.0);
    }

    #[test]
    fn test_ept_octree() {
        let metadata = EptMetadata {
            bounds: [0.0, 0.0, 0.0, 100.0, 100.0, 100.0],
            bounds_conforming: [0.0, 0.0, 0.0, 100.0, 100.0, 100.0],
            data_type: "laszip".to_string(),
            hierarchy_type: "json".to_string(),
            points: 1000,
            srs: None,
            span: 128,
            version: "1.0.0".to_string(),
            schema: vec![],
        };

        let mut octree = EptOctree::new(metadata);

        let mut page = EptHierarchyPage {
            counts: std::collections::HashMap::new(),
        };
        page.counts.insert("0-0-0-0".to_string(), 100);

        octree
            .load_hierarchy_page(page)
            .expect("Loading valid hierarchy page should succeed");

        let count = octree.point_count(&OctreeKey::root());
        assert_eq!(count, Some(100));
    }
}

#[cfg(test)]
mod decode_tests {
    //! Tests for the schema-driven EPT tile decoder that `read_tile` delegates
    //! to, replacing the former silent-stub behaviour.

    use super::*;

    fn field(name: &str, data_type: &str, size: u32, scale: Option<f64>) -> EptSchemaField {
        EptSchemaField {
            name: name.to_string(),
            data_type: data_type.to_string(),
            size,
            scale,
            offset: None,
        }
    }

    /// Schema: X/Y/Z (signed int32, scale 0.01), Intensity (u16), Classification (u8).
    fn sample_schema() -> Vec<EptSchemaField> {
        vec![
            field("X", "signed", 4, Some(0.01)),
            field("Y", "signed", 4, Some(0.01)),
            field("Z", "signed", 4, Some(0.01)),
            field("Intensity", "unsigned", 2, None),
            field("Classification", "unsigned", 1, None),
        ]
    }

    fn make_record(x: i32, y: i32, z: i32, intensity: u16, classification: u8) -> Vec<u8> {
        let mut rec = Vec::new();
        rec.extend_from_slice(&x.to_le_bytes());
        rec.extend_from_slice(&y.to_le_bytes());
        rec.extend_from_slice(&z.to_le_bytes());
        rec.extend_from_slice(&intensity.to_le_bytes());
        rec.push(classification);
        rec
    }

    fn sample_metadata(data_type: &str, schema: Vec<EptSchemaField>) -> EptMetadata {
        EptMetadata {
            bounds: [0.0, 0.0, 0.0, 100.0, 100.0, 100.0],
            bounds_conforming: [0.0, 0.0, 0.0, 100.0, 100.0, 100.0],
            data_type: data_type.to_string(),
            hierarchy_type: "json".to_string(),
            points: 0,
            srs: None,
            span: 128,
            version: "1.0.0".to_string(),
            schema,
        }
    }

    #[test]
    fn parse_binary_two_points() {
        let mut data = Vec::new();
        data.extend_from_slice(&make_record(12_345, 6_789, -1_000, 42, 2));
        data.extend_from_slice(&make_record(50_000, 40_000, 30_000, 7, 5));

        let points = parse_binary_points(&data, &sample_schema()).expect("parse binary");
        assert_eq!(points.len(), 2);
        assert!((points[0].x - 123.45).abs() < 1e-6);
        assert!((points[0].y - 67.89).abs() < 1e-6);
        assert!((points[0].z - (-10.0)).abs() < 1e-6);
        assert_eq!(points[0].intensity, 42);
        assert_eq!(u8::from(points[0].classification), 2);
        assert!((points[1].x - 500.0).abs() < 1e-6);
        assert_eq!(points[1].intensity, 7);
    }

    #[test]
    fn parse_binary_with_rgb() {
        let mut schema = sample_schema();
        schema.push(field("Red", "unsigned", 2, None));
        schema.push(field("Green", "unsigned", 2, None));
        schema.push(field("Blue", "unsigned", 2, None));

        let mut rec = make_record(0, 0, 0, 1, 1);
        rec.extend_from_slice(&111u16.to_le_bytes());
        rec.extend_from_slice(&222u16.to_le_bytes());
        rec.extend_from_slice(&333u16.to_le_bytes());

        let points = parse_binary_points(&rec, &schema).expect("parse rgb");
        assert_eq!(points.len(), 1);
        let color = points[0].color.expect("rgb present");
        assert_eq!((color.red, color.green, color.blue), (111, 222, 333));
    }

    #[test]
    fn parse_binary_rejects_ragged_buffer() {
        // 16 bytes is not a multiple of the 15-byte record stride.
        let data = vec![0u8; 16];
        let err =
            parse_binary_points(&data, &sample_schema()).expect_err("ragged buffer must error");
        assert!(matches!(err, Error::Ept(_)));
    }

    #[test]
    fn parse_binary_rejects_schema_without_position() {
        let schema = vec![field("Intensity", "unsigned", 2, None)];
        let data = vec![0u8; 4];
        let err = parse_binary_points(&data, &schema).expect_err("no xyz must error");
        assert!(matches!(err, Error::Ept(_)));
    }

    #[test]
    fn decode_binary_tile_via_dispatch() {
        let data = make_record(10_000, 20_000, 30_000, 5, 2);
        let metadata = sample_metadata("binary", sample_schema());
        let points = decode_ept_tile(&data, &metadata).expect("decode binary tile");
        assert_eq!(points.len(), 1);
        assert!((points[0].x - 100.0).abs() < 1e-6);
    }

    #[test]
    fn decode_laszip_tile_is_explicit_error() {
        let metadata = sample_metadata("laszip", sample_schema());
        let err = decode_ept_tile(&[0u8; 32], &metadata)
            .expect_err("laszip decode must surface an explicit error");
        assert!(matches!(err, Error::Unsupported(_)));
    }

    #[test]
    fn decode_unknown_datatype_errors() {
        let metadata = sample_metadata("mystery", sample_schema());
        let err = decode_ept_tile(&[0u8; 15], &metadata).expect_err("unknown dataType errors");
        assert!(matches!(err, Error::Ept(_)));
    }

    #[test]
    fn read_signed_le_sign_extends() {
        // -1000 as int32 little-endian.
        let bytes = (-1000i32).to_le_bytes();
        assert_eq!(read_signed_le(&bytes, 4), -1000);
        // Single-byte negative.
        assert_eq!(read_signed_le(&[0xFF], 1), -1);
    }
}
