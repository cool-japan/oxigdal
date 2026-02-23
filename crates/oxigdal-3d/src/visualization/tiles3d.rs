//! 3D Tiles (Cesium) format support
//!
//! Implements the 3D Tiles 1.0 specification for web-based 3D visualization.

use crate::error::{Error, Result};
use crate::mesh::Mesh;
use crate::pointcloud::{Bounds3d, PointCloud};
use byteorder::{LittleEndian, WriteBytesExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

/// 3D Tiles asset information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    /// Version (must be "1.0")
    pub version: String,
    /// Optional tileset version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tileset_version: Option<String>,
    /// Optional generator
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generator: Option<String>,
}

impl Default for Asset {
    fn default() -> Self {
        Self {
            version: "1.0".to_string(),
            tileset_version: None,
            generator: Some("OxiGDAL 3D".to_string()),
        }
    }
}

/// Bounding volume types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BoundingVolume {
    /// Box (center_x, center_y, center_z, half_x_axis, half_y_axis, half_z_axis, ...)
    Box(Vec<f64>),
    /// Region (west, south, east, north, min_height, max_height)
    Region(Vec<f64>),
    /// Sphere (center_x, center_y, center_z, radius)
    Sphere(Vec<f64>),
}

impl BoundingVolume {
    /// Create a box bounding volume from bounds
    pub fn from_bounds(bounds: &Bounds3d) -> Self {
        let center_x = (bounds.min_x + bounds.max_x) / 2.0;
        let center_y = (bounds.min_y + bounds.max_y) / 2.0;
        let center_z = (bounds.min_z + bounds.max_z) / 2.0;

        let half_x = (bounds.max_x - bounds.min_x) / 2.0;
        let half_y = (bounds.max_y - bounds.min_y) / 2.0;
        let half_z = (bounds.max_z - bounds.min_z) / 2.0;

        BoundingVolume::Box(vec![
            center_x, center_y, center_z, half_x, 0.0, 0.0, 0.0, half_y, 0.0, 0.0, 0.0, half_z,
        ])
    }

    /// Create a sphere bounding volume
    pub fn sphere(center_x: f64, center_y: f64, center_z: f64, radius: f64) -> Self {
        BoundingVolume::Sphere(vec![center_x, center_y, center_z, radius])
    }
}

/// Refinement strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Refinement {
    /// Replace parent tile with child tiles
    Replace,
    /// Add child tiles to parent tile
    Add,
}

/// Tile content reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileContent {
    /// URI of the tile content
    pub uri: String,
    /// Optional bounding volume (tighter than tile bounding volume)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "boundingVolume")]
    pub bounding_volume: Option<BoundingVolume>,
}

/// 3D Tile
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tile {
    /// Bounding volume
    pub bounding_volume: BoundingVolume,
    /// Geometric error (in meters)
    pub geometric_error: f64,
    /// Optional refinement strategy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refine: Option<Refinement>,
    /// Optional content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<TileContent>,
    /// Optional child tiles
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<Tile>>,
    /// Optional viewer request volume
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewer_request_volume: Option<BoundingVolume>,
}

impl Tile {
    /// Create a new tile
    pub fn new(bounding_volume: BoundingVolume, geometric_error: f64) -> Self {
        Self {
            bounding_volume,
            geometric_error,
            refine: None,
            content: None,
            children: None,
            viewer_request_volume: None,
        }
    }

    /// Set content
    pub fn with_content(mut self, uri: impl Into<String>) -> Self {
        self.content = Some(TileContent {
            uri: uri.into(),
            bounding_volume: None,
        });
        self
    }

    /// Set refinement
    pub fn with_refinement(mut self, refine: Refinement) -> Self {
        self.refine = Some(refine);
        self
    }

    /// Add child tiles
    pub fn with_children(mut self, children: Vec<Tile>) -> Self {
        self.children = Some(children);
        self
    }
}

/// 3D Tileset
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tileset {
    /// Asset information
    pub asset: Asset,
    /// Root tile
    pub root: Tile,
    /// Optional geometric error (default error for tileset)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geometric_error: Option<f64>,
}

impl Tileset {
    /// Create a new tileset
    pub fn new(root: Tile) -> Self {
        Self {
            asset: Asset::default(),
            root,
            geometric_error: None,
        }
    }

    /// Write tileset to JSON file
    pub fn write<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, self)?;
        Ok(())
    }

    /// Read tileset from JSON file
    pub fn read<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)?;
        let tileset = serde_json::from_reader(file)?;
        Ok(tileset)
    }
}

/// Options for tileset creation
#[derive(Debug, Clone)]
pub struct TilesetOptions {
    /// Maximum geometric error
    pub max_error: f64,
    /// Minimum geometric error
    pub min_error: f64,
    /// Refinement strategy
    pub refinement: Refinement,
    /// Output directory
    pub output_dir: PathBuf,
}

impl Default for TilesetOptions {
    fn default() -> Self {
        Self {
            max_error: 100.0,
            min_error: 1.0,
            refinement: Refinement::Replace,
            output_dir: PathBuf::from("./tiles"),
        }
    }
}

impl TilesetOptions {
    /// Create with custom error thresholds
    pub fn with_error(mut self, max: f64, min: f64) -> Self {
        self.max_error = max;
        self.min_error = min;
        self
    }

    /// Set output directory
    pub fn with_output_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.output_dir = dir.into();
        self
    }
}

/// Create a 3D tileset from a mesh
pub fn create_3d_tileset(mesh: &Mesh, options: &TilesetOptions) -> Result<Tileset> {
    mesh.validate()?;

    // Calculate bounding box
    let (min, max) = mesh
        .bounding_box()
        .ok_or_else(|| Error::Tiles3d("Empty mesh".to_string()))?;

    let bounds = Bounds3d::new(
        min[0] as f64,
        max[0] as f64,
        min[1] as f64,
        max[1] as f64,
        min[2] as f64,
        max[2] as f64,
    );

    let bounding_volume = BoundingVolume::from_bounds(&bounds);

    let root = Tile::new(bounding_volume, options.max_error)
        .with_content("content.b3dm")
        .with_refinement(options.refinement);

    Ok(Tileset::new(root))
}

/// Write 3D tileset and content to disk
pub fn write_3d_tiles(tileset: &Tileset, mesh: &Mesh, options: &TilesetOptions) -> Result<()> {
    // Create output directory
    fs::create_dir_all(&options.output_dir)?;

    // Write tileset.json
    let tileset_path = options.output_dir.join("tileset.json");
    tileset.write(&tileset_path)?;

    // Write B3DM content
    let content_path = options.output_dir.join("content.b3dm");
    write_b3dm(mesh, &content_path)?;

    Ok(())
}

/// B3DM (Batched 3D Model) header
const B3DM_MAGIC: &[u8; 4] = b"b3dm";
const B3DM_VERSION: u32 = 1;

/// Options for B3DM generation
#[derive(Debug, Clone, Default)]
pub struct B3dmOptions {
    /// Batch table with feature attributes
    pub batch_table: Option<BatchTable>,
    /// GLB generation options
    pub glb_options: GlbOptions,
}

impl B3dmOptions {
    /// Create new B3DM options
    pub fn new() -> Self {
        Self::default()
    }

    /// Set batch table
    pub fn with_batch_table(mut self, batch_table: BatchTable) -> Self {
        self.batch_table = Some(batch_table);
        self
    }

    /// Set GLB options
    pub fn with_glb_options(mut self, options: GlbOptions) -> Self {
        self.glb_options = options;
        self
    }
}

/// Write B3DM file (simple version)
fn write_b3dm<P: AsRef<Path>>(mesh: &Mesh, path: P) -> Result<()> {
    write_b3dm_with_options(mesh, path, &B3dmOptions::default())
}

/// Write B3DM file with options and batch table support
pub fn write_b3dm_with_options<P: AsRef<Path>>(
    mesh: &Mesh,
    path: P,
    options: &B3dmOptions,
) -> Result<()> {
    mesh.validate()?;

    // Create GLB data with options
    let glb_data = create_glb_bytes_with_options(mesh, &options.glb_options)?;

    // Get batch table JSON bytes
    let batch_table_json_bytes = match &options.batch_table {
        Some(bt) => bt.to_json_bytes()?,
        None => Vec::new(),
    };
    let batch_table_json_len = batch_table_json_bytes.len();

    // Pad batch table to 8-byte alignment
    let batch_table_json_padding = if batch_table_json_len > 0 {
        (8 - (batch_table_json_len % 8)) % 8
    } else {
        0
    };

    // Batch table binary (not implemented yet, for future extension)
    let batch_table_binary_len = 0;

    // Create feature table JSON
    let batch_length = options.batch_table.as_ref().map_or(0, |bt| bt.len());
    let feature_table_json = json!({
        "BATCH_LENGTH": batch_length
    });
    let feature_table_json_bytes = serde_json::to_vec(&feature_table_json)?;
    let feature_table_json_len = feature_table_json_bytes.len();

    // Pad feature table to 8-byte alignment
    let feature_table_json_padding = (8 - (feature_table_json_len % 8)) % 8;

    // Feature table binary (empty for now)
    let feature_table_binary_len = 0;

    // Calculate total length
    let header_len = 28; // Fixed header size
    let total_len = header_len
        + feature_table_json_len
        + feature_table_json_padding
        + feature_table_binary_len
        + batch_table_json_len
        + batch_table_json_padding
        + batch_table_binary_len
        + glb_data.len();

    // Write B3DM file
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);

    // Header
    writer.write_all(B3DM_MAGIC)?;
    writer.write_u32::<LittleEndian>(B3DM_VERSION)?;
    writer.write_u32::<LittleEndian>(total_len as u32)?;
    writer
        .write_u32::<LittleEndian>((feature_table_json_len + feature_table_json_padding) as u32)?;
    writer.write_u32::<LittleEndian>(feature_table_binary_len as u32)?;
    writer.write_u32::<LittleEndian>((batch_table_json_len + batch_table_json_padding) as u32)?;
    writer.write_u32::<LittleEndian>(batch_table_binary_len as u32)?;

    // Feature table JSON with padding
    writer.write_all(&feature_table_json_bytes)?;
    for _ in 0..feature_table_json_padding {
        writer.write_u8(0x20)?; // Space for JSON padding
    }

    // Batch table JSON with padding
    if batch_table_json_len > 0 {
        writer.write_all(&batch_table_json_bytes)?;
        for _ in 0..batch_table_json_padding {
            writer.write_u8(0x20)?; // Space for JSON padding
        }
    }

    // GLB data
    writer.write_all(&glb_data)?;

    writer.flush()?;
    Ok(())
}

/// GLB magic number
const GLB_MAGIC: u32 = 0x46546C67; // "glTF"
const GLB_VERSION: u32 = 2;

/// GLB chunk types
const GLB_CHUNK_JSON: u32 = 0x4E4F534A; // "JSON"
const GLB_CHUNK_BIN: u32 = 0x004E4942; // "BIN\0"

/// BatchTable for feature attributes in 3D Tiles
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchTable {
    /// Feature attributes (property name -> values for each batch ID)
    properties: HashMap<String, Vec<serde_json::Value>>,
    /// Number of features in the batch
    batch_length: usize,
}

impl BatchTable {
    /// Create a new empty batch table
    pub fn new() -> Self {
        Self {
            properties: HashMap::new(),
            batch_length: 0,
        }
    }

    /// Create a batch table with specified length
    pub fn with_length(batch_length: usize) -> Self {
        Self {
            properties: HashMap::new(),
            batch_length,
        }
    }

    /// Add a property to the batch table
    pub fn add_property(
        &mut self,
        name: impl Into<String>,
        values: Vec<serde_json::Value>,
    ) -> Result<()> {
        let name = name.into();
        if !self.properties.is_empty() && values.len() != self.batch_length {
            return Err(Error::Tiles3d(format!(
                "Property '{}' has {} values but batch length is {}",
                name,
                values.len(),
                self.batch_length
            )));
        }
        if self.properties.is_empty() {
            self.batch_length = values.len();
        }
        self.properties.insert(name, values);
        Ok(())
    }

    /// Get the batch length
    pub fn len(&self) -> usize {
        self.batch_length
    }

    /// Check if batch table is empty
    pub fn is_empty(&self) -> bool {
        self.batch_length == 0
    }

    /// Convert to JSON bytes for B3DM
    pub fn to_json_bytes(&self) -> Result<Vec<u8>> {
        if self.properties.is_empty() {
            return Ok(Vec::new());
        }
        let json = serde_json::to_vec(&self.properties)?;
        Ok(json)
    }
}

/// Options for GLB generation
#[derive(Debug, Clone)]
pub struct GlbOptions {
    /// Include normals in the output
    pub include_normals: bool,
    /// Include texture coordinates
    pub include_texcoords: bool,
    /// Include material
    pub include_material: bool,
}

impl Default for GlbOptions {
    fn default() -> Self {
        Self {
            include_normals: true,
            include_texcoords: true,
            include_material: true,
        }
    }
}

/// Create binary buffer containing mesh data
fn create_mesh_buffer(mesh: &Mesh, options: &GlbOptions) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();

    // Write positions (vec3 float)
    for vertex in &mesh.vertices {
        buffer.write_f32::<LittleEndian>(vertex.position[0])?;
        buffer.write_f32::<LittleEndian>(vertex.position[1])?;
        buffer.write_f32::<LittleEndian>(vertex.position[2])?;
    }

    // Write normals (vec3 float)
    if options.include_normals {
        for vertex in &mesh.vertices {
            buffer.write_f32::<LittleEndian>(vertex.normal[0])?;
            buffer.write_f32::<LittleEndian>(vertex.normal[1])?;
            buffer.write_f32::<LittleEndian>(vertex.normal[2])?;
        }
    }

    // Write texture coordinates (vec2 float)
    if options.include_texcoords {
        for vertex in &mesh.vertices {
            buffer.write_f32::<LittleEndian>(vertex.tex_coords[0])?;
            buffer.write_f32::<LittleEndian>(vertex.tex_coords[1])?;
        }
    }

    // Write indices (uint32)
    for triangle in &mesh.triangles {
        buffer.write_u32::<LittleEndian>(triangle.indices[0])?;
        buffer.write_u32::<LittleEndian>(triangle.indices[1])?;
        buffer.write_u32::<LittleEndian>(triangle.indices[2])?;
    }

    Ok(buffer)
}

/// Create glTF JSON structure for the mesh
fn create_gltf_json(
    mesh: &Mesh,
    buffer_size: usize,
    options: &GlbOptions,
) -> Result<serde_json::Value> {
    let vertex_count = mesh.vertex_count();
    let triangle_count = mesh.triangle_count();

    // Calculate buffer offsets
    let positions_offset = 0;
    let positions_size = vertex_count * 12; // 3 floats * 4 bytes

    let mut current_offset = positions_size;
    let normals_offset = if options.include_normals {
        current_offset
    } else {
        0
    };
    let normals_size = if options.include_normals {
        vertex_count * 12
    } else {
        0
    };
    if options.include_normals {
        current_offset += normals_size;
    }

    let texcoords_offset = if options.include_texcoords {
        current_offset
    } else {
        0
    };
    let texcoords_size = if options.include_texcoords {
        vertex_count * 8 // 2 floats * 4 bytes
    } else {
        0
    };
    if options.include_texcoords {
        current_offset += texcoords_size;
    }

    let indices_offset = current_offset;
    let indices_size = triangle_count * 12; // 3 uint32 * 4 bytes

    // Calculate bounding box for positions
    let bbox = mesh.bounding_box().unwrap_or(([0.0; 3], [0.0; 3]));

    // Build accessor array
    let mut accessors = vec![
        // 0: POSITION
        json!({
            "bufferView": 0,
            "componentType": 5126, // FLOAT
            "count": vertex_count,
            "type": "VEC3",
            "min": bbox.0,
            "max": bbox.1,
        }),
    ];

    let mut buffer_views = vec![
        // 0: Positions
        json!({
            "buffer": 0,
            "byteOffset": positions_offset,
            "byteLength": positions_size,
            "target": 34962, // ARRAY_BUFFER
        }),
    ];

    let mut attributes = json!({
        "POSITION": 0,
    });

    let mut accessor_index = 1;
    let mut buffer_view_index = 1;

    // Add normal accessor
    if options.include_normals {
        accessors.push(json!({
            "bufferView": buffer_view_index,
            "componentType": 5126, // FLOAT
            "count": vertex_count,
            "type": "VEC3",
        }));
        buffer_views.push(json!({
            "buffer": 0,
            "byteOffset": normals_offset,
            "byteLength": normals_size,
            "target": 34962, // ARRAY_BUFFER
        }));
        attributes["NORMAL"] = json!(accessor_index);
        accessor_index += 1;
        buffer_view_index += 1;
    }

    // Add texcoord accessor
    if options.include_texcoords {
        accessors.push(json!({
            "bufferView": buffer_view_index,
            "componentType": 5126, // FLOAT
            "count": vertex_count,
            "type": "VEC2",
        }));
        buffer_views.push(json!({
            "buffer": 0,
            "byteOffset": texcoords_offset,
            "byteLength": texcoords_size,
            "target": 34962, // ARRAY_BUFFER
        }));
        attributes["TEXCOORD_0"] = json!(accessor_index);
        accessor_index += 1;
        buffer_view_index += 1;
    }

    // Add indices accessor
    let indices_accessor_index = accessor_index;
    accessors.push(json!({
        "bufferView": buffer_view_index,
        "componentType": 5125, // UNSIGNED_INT
        "count": triangle_count * 3,
        "type": "SCALAR",
    }));
    buffer_views.push(json!({
        "buffer": 0,
        "byteOffset": indices_offset,
        "byteLength": indices_size,
        "target": 34963, // ELEMENT_ARRAY_BUFFER
    }));

    // Build primitive
    let mut primitive = json!({
        "attributes": attributes,
        "indices": indices_accessor_index,
        "mode": 4, // TRIANGLES
    });

    // Build materials array if needed
    let materials = if options.include_material {
        primitive["material"] = json!(0);
        Some(vec![json!({
            "name": mesh.material.name,
            "pbrMetallicRoughness": {
                "baseColorFactor": mesh.material.base_color,
                "metallicFactor": mesh.material.metallic,
                "roughnessFactor": mesh.material.roughness,
            }
        })])
    } else {
        None
    };

    // Build glTF JSON
    let mut gltf = json!({
        "asset": {
            "version": "2.0",
            "generator": "OxiGDAL 3D Tiles",
        },
        "scene": 0,
        "scenes": [
            {
                "nodes": [0]
            }
        ],
        "nodes": [
            {
                "mesh": 0
            }
        ],
        "meshes": [
            {
                "primitives": [primitive]
            }
        ],
        "accessors": accessors,
        "bufferViews": buffer_views,
        "buffers": [
            {
                "byteLength": buffer_size,
            }
        ],
    });

    if let Some(mats) = materials {
        gltf["materials"] = json!(mats);
    }

    Ok(gltf)
}

/// Create GLB bytes from mesh with full glTF 2.0 support
/// This is a convenience wrapper around `create_glb_bytes_with_options` with default options.
#[allow(dead_code)]
fn create_glb_bytes(mesh: &Mesh) -> Result<Vec<u8>> {
    create_glb_bytes_with_options(mesh, &GlbOptions::default())
}

/// Create GLB bytes from mesh with custom options
pub fn create_glb_bytes_with_options(mesh: &Mesh, options: &GlbOptions) -> Result<Vec<u8>> {
    // Create binary buffer
    let buffer_data = create_mesh_buffer(mesh, options)?;
    let buffer_size = buffer_data.len();

    // Create glTF JSON
    let gltf_json = create_gltf_json(mesh, buffer_size, options)?;
    let json_string = serde_json::to_string(&gltf_json)?;
    let json_bytes = json_string.as_bytes();

    // Pad JSON to 4-byte alignment
    let json_padding = (4 - (json_bytes.len() % 4)) % 4;
    let json_length = json_bytes.len() + json_padding;

    // Pad binary buffer to 4-byte alignment
    let bin_padding = (4 - (buffer_data.len() % 4)) % 4;
    let bin_length = buffer_data.len() + bin_padding;

    // Calculate total file size
    let total_size = 12 + 8 + json_length + 8 + bin_length;

    // Build GLB bytes
    let mut glb = Vec::with_capacity(total_size);

    // GLB header
    glb.write_u32::<LittleEndian>(GLB_MAGIC)?;
    glb.write_u32::<LittleEndian>(GLB_VERSION)?;
    glb.write_u32::<LittleEndian>(total_size as u32)?;

    // JSON chunk header
    glb.write_u32::<LittleEndian>(json_length as u32)?;
    glb.write_u32::<LittleEndian>(GLB_CHUNK_JSON)?;

    // JSON chunk data
    glb.write_all(json_bytes)?;
    for _ in 0..json_padding {
        glb.write_u8(0x20)?; // Space character for padding
    }

    // Binary chunk header
    glb.write_u32::<LittleEndian>(bin_length as u32)?;
    glb.write_u32::<LittleEndian>(GLB_CHUNK_BIN)?;

    // Binary chunk data
    glb.write_all(&buffer_data)?;
    for _ in 0..bin_padding {
        glb.write_u8(0)?; // Null bytes for padding
    }

    Ok(glb)
}

/// Create PNTS (Point Cloud) tile
pub fn write_pnts<P: AsRef<Path>>(point_cloud: &PointCloud, path: P) -> Result<()> {
    const PNTS_MAGIC: &[u8; 4] = b"pnts";
    const PNTS_VERSION: u32 = 1;

    let point_count = point_cloud.len() as u32;

    // Create feature table JSON
    let feature_table_json = serde_json::json!({
        "POINTS_LENGTH": point_count,
        "POSITION": {
            "byteOffset": 0
        }
    });
    let feature_table_json_bytes = serde_json::to_vec(&feature_table_json)?;
    let feature_table_json_len = feature_table_json_bytes.len();

    // Pad to 8-byte alignment
    let feature_table_json_padding = (8 - (feature_table_json_len % 8)) % 8;

    // Create feature table binary (positions)
    let mut feature_table_binary = Vec::new();
    for point in &point_cloud.points {
        feature_table_binary.write_f32::<LittleEndian>(point.x as f32)?;
        feature_table_binary.write_f32::<LittleEndian>(point.y as f32)?;
        feature_table_binary.write_f32::<LittleEndian>(point.z as f32)?;
    }
    let feature_table_binary_len = feature_table_binary.len();

    // Batch table (empty)
    let batch_table_json_len = 0;
    let batch_table_binary_len = 0;

    // Calculate total length
    let header_len = 28;
    let total_len = header_len
        + feature_table_json_len
        + feature_table_json_padding
        + feature_table_binary_len
        + batch_table_json_len
        + batch_table_binary_len;

    // Write PNTS file
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);

    // Header
    writer.write_all(PNTS_MAGIC)?;
    writer.write_u32::<LittleEndian>(PNTS_VERSION)?;
    writer.write_u32::<LittleEndian>(total_len as u32)?;
    writer.write_u32::<LittleEndian>(feature_table_json_len as u32)?;
    writer.write_u32::<LittleEndian>(feature_table_binary_len as u32)?;
    writer.write_u32::<LittleEndian>(batch_table_json_len as u32)?;
    writer.write_u32::<LittleEndian>(batch_table_binary_len as u32)?;

    // Feature table JSON
    writer.write_all(&feature_table_json_bytes)?;
    for _ in 0..feature_table_json_padding {
        writer.write_u8(0x20)?;
    }

    // Feature table binary
    writer.write_all(&feature_table_binary)?;

    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{Material, Vertex};
    use std::env;

    #[test]
    fn test_bounding_volume_from_bounds() {
        let bounds = Bounds3d::new(0.0, 10.0, 0.0, 10.0, 0.0, 10.0);
        let bv = BoundingVolume::from_bounds(&bounds);

        assert!(
            matches!(&bv, BoundingVolume::Box(_)),
            "Expected Box bounding volume"
        );
        if let BoundingVolume::Box(values) = bv {
            assert_eq!(values[0], 5.0); // center_x
            assert_eq!(values[1], 5.0); // center_y
            assert_eq!(values[2], 5.0); // center_z
        }
    }

    #[test]
    fn test_tile_creation() {
        let bv = BoundingVolume::sphere(0.0, 0.0, 0.0, 100.0);
        let tile = Tile::new(bv, 10.0)
            .with_content("test.b3dm")
            .with_refinement(Refinement::Replace);

        assert_eq!(tile.geometric_error, 10.0);
        assert!(tile.content.is_some());
        assert_eq!(tile.refine, Some(Refinement::Replace));
    }

    #[test]
    fn test_tileset_creation() {
        let bv = BoundingVolume::sphere(0.0, 0.0, 0.0, 100.0);
        let root = Tile::new(bv, 10.0);
        let tileset = Tileset::new(root);

        assert_eq!(tileset.asset.version, "1.0");
    }

    #[test]
    fn test_create_3d_tileset() {
        let mut mesh = Mesh::new();
        mesh.add_vertex(Vertex::new([0.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([1.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([0.0, 1.0, 0.0]));
        mesh.add_triangle(0, 1, 2);

        let options = TilesetOptions::default();
        let tileset_result = create_3d_tileset(&mesh, &options);

        assert!(tileset_result.is_ok());
        if let Ok(tileset) = tileset_result {
            assert_eq!(tileset.asset.version, "1.0");
        }
    }

    #[test]
    fn test_tileset_json_roundtrip() {
        let bv = BoundingVolume::sphere(0.0, 0.0, 0.0, 100.0);
        let root = Tile::new(bv, 10.0).with_content("test.b3dm");
        let tileset = Tileset::new(root);

        let temp_dir = env::temp_dir();
        let path = temp_dir.join("test_tileset.json");

        // Write
        let write_result = tileset.write(&path);
        assert!(write_result.is_ok());

        // Read
        let read_result = Tileset::read(&path);
        assert!(read_result.is_ok());

        // Clean up
        let _ = fs::remove_file(&path);
    }

    // ========== glTF Export Tests ==========

    #[test]
    fn test_create_glb_bytes() {
        let mut mesh = Mesh::new();
        mesh.add_vertex(Vertex::new([0.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([1.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([0.0, 1.0, 0.0]));
        mesh.add_triangle(0, 1, 2);
        mesh.calculate_normals();

        let glb_result = create_glb_bytes(&mesh);
        assert!(glb_result.is_ok());

        if let Ok(glb_data) = glb_result {
            // Check GLB magic number
            assert_eq!(&glb_data[0..4], b"glTF");

            // Check version (should be 2)
            let version = u32::from_le_bytes([glb_data[4], glb_data[5], glb_data[6], glb_data[7]]);
            assert_eq!(version, 2);

            // Check that file length is correct
            let length = u32::from_le_bytes([glb_data[8], glb_data[9], glb_data[10], glb_data[11]]);
            assert_eq!(length as usize, glb_data.len());
        }
    }

    #[test]
    fn test_create_glb_bytes_with_options() {
        let mut mesh = Mesh::new();
        mesh.add_vertex(Vertex::with_attributes(
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0],
        ));
        mesh.add_vertex(Vertex::with_attributes(
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0],
        ));
        mesh.add_vertex(Vertex::with_attributes(
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 1.0],
        ));
        mesh.add_triangle(0, 1, 2);

        // Test with all options disabled
        let options = GlbOptions {
            include_normals: false,
            include_texcoords: false,
            include_material: false,
        };

        let glb_result = create_glb_bytes_with_options(&mesh, &options);
        assert!(glb_result.is_ok());

        if let Ok(glb_data) = glb_result {
            // Should still have valid GLB header
            assert_eq!(&glb_data[0..4], b"glTF");
        }
    }

    #[test]
    fn test_create_mesh_buffer() {
        let mut mesh = Mesh::new();
        mesh.add_vertex(Vertex::new([1.0, 2.0, 3.0]));
        mesh.add_vertex(Vertex::new([4.0, 5.0, 6.0]));
        mesh.add_vertex(Vertex::new([7.0, 8.0, 9.0]));
        mesh.add_triangle(0, 1, 2);

        let options = GlbOptions::default();
        let buffer_result = create_mesh_buffer(&mesh, &options);
        assert!(buffer_result.is_ok());

        if let Ok(buffer) = buffer_result {
            // Expected size:
            // 3 vertices * 3 floats (position) * 4 bytes = 36
            // 3 vertices * 3 floats (normal) * 4 bytes = 36
            // 3 vertices * 2 floats (texcoord) * 4 bytes = 24
            // 1 triangle * 3 indices * 4 bytes = 12
            // Total = 108 bytes
            assert_eq!(buffer.len(), 108);
        }
    }

    #[test]
    fn test_create_gltf_json() {
        let mut mesh = Mesh::new();
        mesh.add_vertex(Vertex::new([0.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([1.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([0.0, 1.0, 0.0]));
        mesh.add_triangle(0, 1, 2);
        mesh.material = Material::new("test_material").with_color(0.8, 0.2, 0.2, 1.0);

        let options = GlbOptions::default();
        let json_result = create_gltf_json(&mesh, 100, &options);
        assert!(json_result.is_ok());

        if let Ok(json) = json_result {
            // Check asset version
            assert_eq!(json["asset"]["version"], "2.0");

            // Check scene is defined
            assert_eq!(json["scene"], 0);

            // Check nodes
            assert!(json["nodes"].is_array());

            // Check meshes
            assert!(json["meshes"].is_array());

            // Check accessors
            assert!(json["accessors"].is_array());

            // Check buffer views
            assert!(json["bufferViews"].is_array());

            // Check materials
            assert!(json["materials"].is_array());
            assert_eq!(json["materials"][0]["name"], "test_material");
        }
    }

    // ========== BatchTable Tests ==========

    #[test]
    fn test_batch_table_new() {
        let batch_table = BatchTable::new();
        assert!(batch_table.is_empty());
        assert_eq!(batch_table.len(), 0);
    }

    #[test]
    fn test_batch_table_with_length() {
        let batch_table = BatchTable::with_length(5);
        // Initially empty but has target length
        assert_eq!(batch_table.len(), 5);
    }

    #[test]
    fn test_batch_table_add_property() {
        let mut batch_table = BatchTable::new();

        let values = vec![
            serde_json::json!("building_1"),
            serde_json::json!("building_2"),
            serde_json::json!("building_3"),
        ];

        let result = batch_table.add_property("name", values);
        assert!(result.is_ok());
        assert_eq!(batch_table.len(), 3);
    }

    #[test]
    fn test_batch_table_add_multiple_properties() {
        let mut batch_table = BatchTable::new();

        let names = vec![
            serde_json::json!("building_1"),
            serde_json::json!("building_2"),
        ];
        let heights = vec![serde_json::json!(100.0), serde_json::json!(150.0)];

        let _ = batch_table.add_property("name", names);
        let result = batch_table.add_property("height", heights);
        assert!(result.is_ok());
    }

    #[test]
    fn test_batch_table_mismatched_length() {
        let mut batch_table = BatchTable::new();

        let names = vec![
            serde_json::json!("building_1"),
            serde_json::json!("building_2"),
        ];
        let heights = vec![
            serde_json::json!(100.0),
            serde_json::json!(150.0),
            serde_json::json!(200.0), // Extra value
        ];

        let _ = batch_table.add_property("name", names);
        let result = batch_table.add_property("height", heights);
        assert!(result.is_err());
    }

    #[test]
    fn test_batch_table_to_json_bytes() {
        let mut batch_table = BatchTable::new();

        let values = vec![serde_json::json!(1), serde_json::json!(2)];
        let _ = batch_table.add_property("id", values);

        let json_bytes_result = batch_table.to_json_bytes();
        assert!(json_bytes_result.is_ok());

        if let Ok(json_bytes) = json_bytes_result {
            assert!(!json_bytes.is_empty());
            // Should be valid JSON
            let parsed: std::result::Result<serde_json::Value, _> =
                serde_json::from_slice(&json_bytes);
            assert!(parsed.is_ok());
        }
    }

    // ========== B3DM Tests ==========

    #[test]
    fn test_b3dm_options() {
        let mut batch_table = BatchTable::new();
        let _ = batch_table.add_property("id", vec![serde_json::json!(1)]);

        let options = B3dmOptions::new()
            .with_batch_table(batch_table)
            .with_glb_options(GlbOptions {
                include_normals: true,
                include_texcoords: false,
                include_material: true,
            });

        assert!(options.batch_table.is_some());
        assert!(options.glb_options.include_normals);
        assert!(!options.glb_options.include_texcoords);
    }

    #[test]
    fn test_write_b3dm_with_batch_table() {
        let mut mesh = Mesh::new();
        mesh.add_vertex(Vertex::new([0.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([1.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([0.0, 1.0, 0.0]));
        mesh.add_triangle(0, 1, 2);
        mesh.calculate_normals();

        let mut batch_table = BatchTable::new();
        let _ = batch_table.add_property("name", vec![serde_json::json!("test_feature")]);
        let _ = batch_table.add_property("height", vec![serde_json::json!(50.5)]);

        let options = B3dmOptions::new().with_batch_table(batch_table);

        let temp_dir = env::temp_dir();
        let path = temp_dir.join("test_with_batch_table.b3dm");

        let result = write_b3dm_with_options(&mesh, &path, &options);
        assert!(result.is_ok());

        // Verify file was created and has correct magic
        if path.exists() {
            let data = std::fs::read(&path);
            if let Ok(bytes) = data {
                assert_eq!(&bytes[0..4], b"b3dm");
                // Version should be 1
                let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
                assert_eq!(version, 1);
            }
            let _ = fs::remove_file(&path);
        }
    }

    #[test]
    fn test_write_3d_tiles_integration() {
        let mut mesh = Mesh::new();
        mesh.add_vertex(Vertex::new([0.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([10.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([5.0, 10.0, 5.0]));
        mesh.add_triangle(0, 1, 2);
        mesh.calculate_normals();

        let temp_dir = env::temp_dir().join("tiles3d_test");
        let options = TilesetOptions::default().with_output_dir(&temp_dir);

        let tileset_result = create_3d_tileset(&mesh, &options);
        assert!(tileset_result.is_ok());

        if let Ok(tileset) = tileset_result {
            let write_result = write_3d_tiles(&tileset, &mesh, &options);
            assert!(write_result.is_ok());

            // Check files exist
            let tileset_path = temp_dir.join("tileset.json");
            let content_path = temp_dir.join("content.b3dm");

            assert!(tileset_path.exists());
            assert!(content_path.exists());

            // Verify B3DM has valid glTF data
            if let Ok(b3dm_data) = std::fs::read(&content_path) {
                assert_eq!(&b3dm_data[0..4], b"b3dm");

                // Find GLB data position (after header and feature/batch tables)
                let feature_table_len = u32::from_le_bytes([
                    b3dm_data[12],
                    b3dm_data[13],
                    b3dm_data[14],
                    b3dm_data[15],
                ]) as usize;
                let feature_binary_len = u32::from_le_bytes([
                    b3dm_data[16],
                    b3dm_data[17],
                    b3dm_data[18],
                    b3dm_data[19],
                ]) as usize;
                let batch_table_len = u32::from_le_bytes([
                    b3dm_data[20],
                    b3dm_data[21],
                    b3dm_data[22],
                    b3dm_data[23],
                ]) as usize;
                let batch_binary_len = u32::from_le_bytes([
                    b3dm_data[24],
                    b3dm_data[25],
                    b3dm_data[26],
                    b3dm_data[27],
                ]) as usize;

                let glb_offset = 28
                    + feature_table_len
                    + feature_binary_len
                    + batch_table_len
                    + batch_binary_len;

                // Verify GLB magic
                assert_eq!(&b3dm_data[glb_offset..glb_offset + 4], b"glTF");
            }

            // Clean up
            let _ = fs::remove_dir_all(&temp_dir);
        }
    }

    #[test]
    fn test_glb_alignment() {
        // Test that GLB data is properly aligned
        let mut mesh = Mesh::new();
        mesh.add_vertex(Vertex::new([0.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([1.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([0.0, 1.0, 0.0]));
        mesh.add_triangle(0, 1, 2);

        let glb_result = create_glb_bytes(&mesh);
        assert!(glb_result.is_ok());

        if let Ok(glb_data) = glb_result {
            // GLB total length should match header value
            let length = u32::from_le_bytes([glb_data[8], glb_data[9], glb_data[10], glb_data[11]]);
            assert_eq!(length as usize, glb_data.len());

            // Total length should be 4-byte aligned
            assert_eq!(glb_data.len() % 4, 0);
        }
    }
}
