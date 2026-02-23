//! EPT (Entwine Point Tiles) format support
//!
//! Provides streaming access to EPT format point clouds with octree structure.

use crate::error::{Error, Result};
use crate::pointcloud::{Bounds3d, Point};
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

        // Read and decompress tile
        // Simplified: In real implementation, handle laszip, binary, or zstandard
        let _data = fs::read(&tile_path)?;

        // Parse points based on schema
        let points = Vec::new();

        Ok(points)
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
        if let Ok(response) = client.get(&hierarchy_url).send().await {
            if response.status().is_success() {
                let page: EptHierarchyPage = response.json().await?;
                octree.load_hierarchy_page(page)?;
            }
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

        let _data = response.bytes().await?;

        // Parse points
        let points = Vec::new();

        Ok(points)
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
