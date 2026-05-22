//! COPC (Cloud Optimized Point Cloud) format support
//!
//! Provides streaming access to cloud-optimized point clouds over HTTP using range requests.

use crate::error::{Error, Result};
use crate::pointcloud::{Bounds3d, LasHeader, Point, PointFormat};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[cfg(feature = "async")]
use bytes::Bytes;
#[cfg(feature = "async")]
use reqwest::Client;

/// COPC VLR (Variable Length Record) signature
#[allow(dead_code)]
const COPC_VLR_USER_ID: &str = "copc";
#[allow(dead_code)]
const COPC_VLR_RECORD_ID: u16 = 1;

/// COPC hierarchy VLR
#[allow(dead_code)]
const COPC_HIERARCHY_RECORD_ID: u16 = 1000;

/// COPC info structure (from VLR)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopcInfo {
    /// Center X coordinate
    pub center_x: f64,
    /// Center Y coordinate
    pub center_y: f64,
    /// Center Z coordinate
    pub center_z: f64,
    /// Half-size (spacing at root level)
    pub halfsize: f64,
    /// Spacing factor (typically 0.5 for octree)
    pub spacing: f64,
    /// Root hierarchy page offset
    pub root_hier_offset: u64,
    /// Root hierarchy page size
    pub root_hier_size: u64,
    /// GPS time minimum
    pub gps_time_min: f64,
    /// GPS time maximum
    pub gps_time_max: f64,
}

/// VoxelKey for octree addressing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VoxelKey {
    /// Depth level (0 = root)
    pub level: i32,
    /// X index at this level
    pub x: i32,
    /// Y index at this level
    pub y: i32,
    /// Z index at this level
    pub z: i32,
}

impl VoxelKey {
    /// Create a new voxel key
    pub fn new(level: i32, x: i32, y: i32, z: i32) -> Self {
        Self { level, x, y, z }
    }

    /// Get root voxel key
    pub fn root() -> Self {
        Self::new(0, 0, 0, 0)
    }

    /// Get child voxel keys
    pub fn children(&self) -> [VoxelKey; 8] {
        let level = self.level + 1;
        let x = self.x * 2;
        let y = self.y * 2;
        let z = self.z * 2;

        [
            VoxelKey::new(level, x, y, z),
            VoxelKey::new(level, x + 1, y, z),
            VoxelKey::new(level, x, y + 1, z),
            VoxelKey::new(level, x + 1, y + 1, z),
            VoxelKey::new(level, x, y, z + 1),
            VoxelKey::new(level, x + 1, y, z + 1),
            VoxelKey::new(level, x, y + 1, z + 1),
            VoxelKey::new(level, x + 1, y + 1, z + 1),
        ]
    }

    /// Get parent voxel key
    pub fn parent(&self) -> Option<VoxelKey> {
        if self.level == 0 {
            return None;
        }

        Some(VoxelKey::new(
            self.level - 1,
            self.x / 2,
            self.y / 2,
            self.z / 2,
        ))
    }

    /// Calculate bounds for this voxel
    pub fn bounds(&self, info: &CopcInfo) -> Bounds3d {
        let size = info.halfsize * 2.0 / (1_i32 << self.level) as f64;
        let min_x = info.center_x - info.halfsize + self.x as f64 * size;
        let min_y = info.center_y - info.halfsize + self.y as f64 * size;
        let min_z = info.center_z - info.halfsize + self.z as f64 * size;

        Bounds3d::new(
            min_x,
            min_x + size,
            min_y,
            min_y + size,
            min_z,
            min_z + size,
        )
    }
}

/// COPC hierarchy entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopcEntry {
    /// Voxel key
    pub key: VoxelKey,
    /// Byte offset in LAZ file
    pub offset: u64,
    /// Byte size
    pub byte_size: i32,
    /// Number of points
    pub point_count: i32,
}

/// COPC hierarchy (octree structure)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopcHierarchy {
    /// Map of voxel key to entry
    entries: HashMap<VoxelKey, CopcEntry>,
}

impl CopcHierarchy {
    /// Create a new empty hierarchy
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Add an entry
    pub fn add_entry(&mut self, entry: CopcEntry) {
        self.entries.insert(entry.key, entry);
    }

    /// Get an entry by voxel key
    pub fn get_entry(&self, key: &VoxelKey) -> Option<&CopcEntry> {
        self.entries.get(key)
    }

    /// Get all entries
    pub fn entries(&self) -> impl Iterator<Item = &CopcEntry> {
        self.entries.values()
    }

    /// Find entries within bounds
    pub fn find_in_bounds(&self, bounds: &Bounds3d, info: &CopcInfo) -> Vec<&CopcEntry> {
        self.entries
            .values()
            .filter(|entry| {
                let voxel_bounds = entry.key.bounds(info);
                voxel_bounds.intersects(bounds)
            })
            .collect()
    }

    /// Traverse hierarchy depth-first
    pub fn traverse_from(&self, start: &VoxelKey) -> Vec<&CopcEntry> {
        let mut result = Vec::new();
        let mut stack = vec![*start];

        while let Some(key) = stack.pop() {
            if let Some(entry) = self.get_entry(&key) {
                result.push(entry);

                // Add children to stack
                for child in key.children() {
                    if self.entries.contains_key(&child) {
                        stack.push(child);
                    }
                }
            }
        }

        result
    }
}

impl Default for CopcHierarchy {
    fn default() -> Self {
        Self::new()
    }
}

/// COPC reader for local files
pub struct CopcReader {
    file: File,
    header: LasHeader,
    info: CopcInfo,
    hierarchy: CopcHierarchy,
}

impl CopcReader {
    /// Open a COPC file
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();

        // Read LAS header first (using temporary file handle)
        let las_reader = {
            let temp_file = File::open(path)?;
            las::Reader::new(temp_file)
                .map_err(|e| Error::Copc(format!("Failed to read LAS header: {}", e)))?
        };
        let las_header = las_reader.header();

        // Open file for our own use
        let mut file = File::open(path)?;

        // Parse COPC info from VLR
        let info = Self::read_copc_info(&mut file, las_header)?;

        // Read hierarchy
        let hierarchy = Self::read_hierarchy(&mut file, &info)?;

        // Construct our header
        let version = format!(
            "{}.{}",
            las_header.version().major,
            las_header.version().minor
        );
        let point_format_u8 = las_header
            .point_format()
            .to_u8()
            .map_err(|e| Error::Copc(format!("Failed to convert point format: {}", e)))?;
        let point_format = PointFormat::try_from(point_format_u8)?;

        let bounds = Bounds3d::new(
            las_header.bounds().min.x,
            las_header.bounds().max.x,
            las_header.bounds().min.y,
            las_header.bounds().max.y,
            las_header.bounds().min.z,
            las_header.bounds().max.z,
        );

        let header = LasHeader {
            version,
            point_format,
            point_count: las_header.number_of_points(),
            bounds,
            scale: (
                las_header.transforms().x.scale,
                las_header.transforms().y.scale,
                las_header.transforms().z.scale,
            ),
            offset: (
                las_header.transforms().x.offset,
                las_header.transforms().y.offset,
                las_header.transforms().z.offset,
            ),
            system_identifier: las_header.system_identifier().to_string(),
            generating_software: las_header.generating_software().to_string(),
        };

        Ok(Self {
            file,
            header,
            info,
            hierarchy,
        })
    }

    /// Read COPC info from the LAS header's VLR list.
    ///
    /// Locates the COPC info VLR (user_id=`copc`, record_id=1) and parses its
    /// 160-byte payload per the COPC 1.0 spec (<https://copc.io/>).
    fn read_copc_info(_file: &mut File, header: &las::Header) -> Result<CopcInfo> {
        use crate::pointcloud::copc_vlr::{find_copc_info_vlr, parse_copc_info};
        let vlr = find_copc_info_vlr(header).ok_or_else(Error::missing_copc_vlr)?;
        let payload = parse_copc_info(&vlr.data)?;
        Ok(CopcInfo {
            center_x: payload.center_x,
            center_y: payload.center_y,
            center_z: payload.center_z,
            halfsize: payload.halfsize,
            spacing: payload.spacing,
            root_hier_offset: payload.root_hier_offset,
            root_hier_size: payload.root_hier_size,
            gps_time_min: payload.gps_time_min,
            gps_time_max: payload.gps_time_max,
        })
    }

    /// Walk the COPC hierarchy starting from the root page and collect all
    /// leaf entries.
    ///
    /// Negative `byte_size` entries are treated as forward references to a
    /// child hierarchy page (the absolute value is the page size in bytes).
    /// Traversal is iterative with a sanity cap of
    /// [`copc_vlr::COPC_MAX_HIERARCHY_DEPTH`] page-loads to defend against
    /// malformed files.
    fn read_hierarchy(file: &mut File, info: &CopcInfo) -> Result<CopcHierarchy> {
        use crate::pointcloud::copc_vlr::{COPC_MAX_HIERARCHY_DEPTH, parse_hierarchy_page};

        let mut hierarchy = CopcHierarchy::new();
        if info.root_hier_size == 0 {
            return Ok(hierarchy);
        }

        let mut pending: Vec<(u64, u64)> = vec![(info.root_hier_offset, info.root_hier_size)];
        let mut pages_loaded = 0usize;

        while let Some((page_off, page_size)) = pending.pop() {
            if pages_loaded >= COPC_MAX_HIERARCHY_DEPTH {
                return Err(Error::hierarchy_recursion_limit());
            }
            pages_loaded += 1;

            file.seek(SeekFrom::Start(page_off))?;
            let mut buf = vec![0u8; page_size as usize];
            file.read_exact(&mut buf)?;
            let entries = parse_hierarchy_page(&buf)?;
            for entry in entries {
                if entry.byte_size < 0 {
                    // Child hierarchy page reference.
                    let child_size = (-(entry.byte_size as i64)) as u64;
                    pending.push((entry.offset, child_size));
                } else {
                    let key = VoxelKey::new(entry.key.level, entry.key.x, entry.key.y, entry.key.z);
                    hierarchy.add_entry(CopcEntry {
                        key,
                        offset: entry.offset,
                        byte_size: entry.byte_size,
                        point_count: entry.point_count,
                    });
                }
            }
        }
        Ok(hierarchy)
    }

    /// Get header
    pub fn header(&self) -> &LasHeader {
        &self.header
    }

    /// Get COPC info
    pub fn info(&self) -> &CopcInfo {
        &self.info
    }

    /// Get hierarchy
    pub fn hierarchy(&self) -> &CopcHierarchy {
        &self.hierarchy
    }

    /// Read points from a voxel
    pub fn read_voxel(&mut self, key: &VoxelKey) -> Result<Vec<Point>> {
        let entry = self
            .hierarchy
            .get_entry(key)
            .ok_or_else(|| Error::Copc(format!("Voxel not found: {:?}", key)))?;

        if entry.point_count == 0 {
            return Ok(Vec::new());
        }

        // Seek to the data offset
        self.file.seek(SeekFrom::Start(entry.offset))?;

        // Read compressed chunk
        let mut compressed = vec![0u8; entry.byte_size as usize];
        self.file.read_exact(&mut compressed)?;

        // Decompress and parse points
        // Simplified: In real implementation, use LAZ decompression
        let points = Vec::new();

        Ok(points)
    }

    /// Query points within bounds
    pub fn query_bounds(&mut self, bounds: &Bounds3d) -> Result<Vec<Point>> {
        let keys: Vec<VoxelKey> = self
            .hierarchy
            .find_in_bounds(bounds, &self.info)
            .iter()
            .map(|entry| entry.key)
            .collect();
        let mut all_points = Vec::new();

        for key in keys {
            let points = self.read_voxel(&key)?;
            all_points.extend(points);
        }

        Ok(all_points)
    }

    /// Get points at a specific level
    pub fn read_level(&mut self, level: i32) -> Result<Vec<Point>> {
        let keys: Vec<VoxelKey> = self
            .hierarchy
            .entries()
            .filter(|e| e.key.level == level)
            .map(|e| e.key)
            .collect();

        let mut all_points = Vec::new();

        for key in keys {
            let points = self.read_voxel(&key)?;
            all_points.extend(points);
        }

        Ok(all_points)
    }
}

/// COPC reader for HTTP streaming
#[cfg(feature = "async")]
pub struct CopcHttpReader {
    url: String,
    client: Client,
    header: LasHeader,
    info: CopcInfo,
    hierarchy: CopcHierarchy,
}

#[cfg(feature = "async")]
impl CopcHttpReader {
    /// Open a COPC file via HTTP
    pub async fn open(url: impl Into<String>) -> Result<Self> {
        let url = url.into();
        let client = Client::new();

        // Fetch header using range request (first 4KB)
        let _header_bytes = Self::fetch_range(&client, &url, 0, 4096).await?;

        // Parse header and VLRs
        // Simplified: In real implementation, parse LAS header and COPC VLR

        let header = LasHeader {
            version: "1.4".to_string(),
            point_format: PointFormat::Format7,
            point_count: 0,
            bounds: Bounds3d::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
            scale: (0.01, 0.01, 0.01),
            offset: (0.0, 0.0, 0.0),
            system_identifier: String::new(),
            generating_software: String::new(),
        };

        let info = CopcInfo {
            center_x: 0.0,
            center_y: 0.0,
            center_z: 0.0,
            halfsize: 1000.0,
            spacing: 0.5,
            root_hier_offset: 0,
            root_hier_size: 0,
            gps_time_min: 0.0,
            gps_time_max: 0.0,
        };

        // Fetch root hierarchy page
        let hierarchy = CopcHierarchy::new();

        Ok(Self {
            url,
            client,
            header,
            info,
            hierarchy,
        })
    }

    /// Fetch byte range from URL
    async fn fetch_range(client: &Client, url: &str, start: u64, end: u64) -> Result<Bytes> {
        let response = client
            .get(url)
            .header("Range", format!("bytes={}-{}", start, end))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(Error::RangeRequest(format!(
                "HTTP {}: {}",
                response.status(),
                response.status().canonical_reason().unwrap_or("Unknown")
            )));
        }

        Ok(response.bytes().await?)
    }

    /// Get header
    pub fn header(&self) -> &LasHeader {
        &self.header
    }

    /// Get COPC info
    pub fn info(&self) -> &CopcInfo {
        &self.info
    }

    /// Read voxel via HTTP range request
    pub async fn read_voxel(&self, key: &VoxelKey) -> Result<Vec<Point>> {
        let entry = self
            .hierarchy
            .get_entry(key)
            .ok_or_else(|| Error::Copc(format!("Voxel not found: {:?}", key)))?;

        if entry.point_count == 0 {
            return Ok(Vec::new());
        }

        // Fetch compressed chunk
        let _compressed = Self::fetch_range(
            &self.client,
            &self.url,
            entry.offset,
            entry.offset + entry.byte_size as u64,
        )
        .await?;

        // Decompress and parse points
        // Simplified: In real implementation, use LAZ decompression
        let points = Vec::new();

        Ok(points)
    }

    /// Query points within bounds
    pub async fn query_bounds(&self, bounds: &Bounds3d) -> Result<Vec<Point>> {
        let entries = self.hierarchy.find_in_bounds(bounds, &self.info);
        let mut all_points = Vec::new();

        for entry in entries {
            let points = self.read_voxel(&entry.key).await?;
            all_points.extend(points);
        }

        Ok(all_points)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_voxel_key_root() {
        let root = VoxelKey::root();
        assert_eq!(root.level, 0);
        assert_eq!(root.x, 0);
        assert_eq!(root.y, 0);
        assert_eq!(root.z, 0);
    }

    #[test]
    fn test_voxel_key_children() {
        let root = VoxelKey::root();
        let children = root.children();

        assert_eq!(children.len(), 8);
        assert_eq!(children[0].level, 1);
        assert_eq!(children[0].x, 0);
        assert_eq!(children[7].x, 1);
        assert_eq!(children[7].y, 1);
        assert_eq!(children[7].z, 1);
    }

    #[test]
    fn test_voxel_key_parent() {
        let child = VoxelKey::new(1, 1, 1, 1);
        let parent = child.parent();

        assert!(parent.is_some());
        let parent = parent.expect("Parent should exist for non-root voxel key");
        assert_eq!(parent.level, 0);
        assert_eq!(parent.x, 0);

        let root = VoxelKey::root();
        assert!(root.parent().is_none());
    }

    #[test]
    fn test_voxel_bounds() {
        let info = CopcInfo {
            center_x: 0.0,
            center_y: 0.0,
            center_z: 0.0,
            halfsize: 100.0,
            spacing: 0.5,
            root_hier_offset: 0,
            root_hier_size: 0,
            gps_time_min: 0.0,
            gps_time_max: 0.0,
        };

        let root = VoxelKey::root();
        let bounds = root.bounds(&info);

        assert_relative_eq!(bounds.min_x, -100.0);
        assert_relative_eq!(bounds.max_x, 100.0);
    }

    #[test]
    fn test_copc_hierarchy() {
        let mut hierarchy = CopcHierarchy::new();

        let entry = CopcEntry {
            key: VoxelKey::root(),
            offset: 0,
            byte_size: 1024,
            point_count: 100,
        };

        hierarchy.add_entry(entry.clone());

        let retrieved = hierarchy.get_entry(&VoxelKey::root());
        assert!(retrieved.is_some());
        assert_eq!(
            retrieved
                .expect("Root entry should be present in hierarchy")
                .point_count,
            100
        );
    }

    #[test]
    fn test_hierarchy_traverse() {
        let mut hierarchy = CopcHierarchy::new();

        // Add root
        hierarchy.add_entry(CopcEntry {
            key: VoxelKey::root(),
            offset: 0,
            byte_size: 1024,
            point_count: 100,
        });

        // Add some children
        for child in VoxelKey::root().children() {
            hierarchy.add_entry(CopcEntry {
                key: child,
                offset: 0,
                byte_size: 512,
                point_count: 50,
            });
        }

        let entries = hierarchy.traverse_from(&VoxelKey::root());
        assert_eq!(entries.len(), 9); // root + 8 children
    }
}
