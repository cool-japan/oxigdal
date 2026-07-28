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

/// Read up to `buf.len()` bytes from `reader`, returning the number actually
/// read.
///
/// Unlike [`Read::read_exact`], a short read (EOF reached before the buffer is
/// full) is not an error — used when the requested window may extend past the
/// end of the file (e.g. reading a fixed-size preamble from a small file).
fn read_up_to<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Error::Io(e)),
        }
    }
    Ok(filled)
}

/// Convert an [`oxigeo_copc::Point3D`] into the crate-native [`Point`].
///
/// COPC formats 6-10 carry RGB (7/8) and NIR (8/10); the current oxigeo-copc
/// point model surfaces RGB but not NIR, so `nir` is left `None`.
fn copc_point_to_point(p: &oxigeo_copc::Point3D) -> Point {
    use crate::pointcloud::{Classification, ColorRgb};

    let color = match (p.red, p.green, p.blue) {
        (Some(red), Some(green), Some(blue)) => Some(ColorRgb { red, green, blue }),
        _ => None,
    };

    Point {
        x: p.x,
        y: p.y,
        z: p.z,
        intensity: p.intensity,
        return_number: p.return_number,
        number_of_returns: p.number_of_returns,
        classification: Classification::from(p.classification),
        scan_angle: p.scan_angle_rank.round() as i16,
        user_data: p.user_data,
        point_source_id: p.point_source_id,
        gps_time: p.gps_time,
        color,
        nir: None,
    }
}

/// Decode a single COPC point-data chunk into [`Point`]s.
///
/// When [`ChunkDecodeParams::is_laz`] is set the chunk is routed through the
/// pure-Rust LAZ decompressor ([`oxigeo_copc::decompress_chunk`]) before the
/// LAS records are deserialized; otherwise the chunk bytes are treated as raw
/// LAS records. Point formats the LAZ decoder does not yet support surface as a
/// typed [`Error::LazCompression`] rather than silently returning no points.
fn decode_copc_chunk(
    chunk: &[u8],
    point_count: usize,
    decode: &ChunkDecodeParams,
) -> Result<Vec<Point>> {
    if point_count == 0 {
        return Ok(Vec::new());
    }

    let raw_records: std::borrow::Cow<'_, [u8]> = if decode.is_laz {
        let decompressed = oxigeo_copc::decompress_chunk(
            chunk,
            point_count,
            decode.record_length,
            decode.format_id,
        )
        .map_err(|e| Error::LazCompression(e.to_string()))?;
        std::borrow::Cow::Owned(decompressed)
    } else {
        std::borrow::Cow::Borrowed(chunk)
    };

    let points3d = oxigeo_copc::deserialize_points(
        &raw_records,
        point_count,
        decode.record_length,
        decode.format_id,
        decode.scale,
        decode.offset,
    )
    .map_err(|e| Error::Copc(format!("point deserialization failed: {e}")))?;

    Ok(points3d.iter().map(copc_point_to_point).collect())
}

/// Render an [`oxigeo_copc::LasVersion`] as a `"major.minor"` string.
#[cfg(feature = "async")]
fn las_version_string(v: oxigeo_copc::LasVersion) -> String {
    use oxigeo_copc::LasVersion;
    match v {
        LasVersion::V10 => "1.0",
        LasVersion::V11 => "1.1",
        LasVersion::V12 => "1.2",
        LasVersion::V13 => "1.3",
        LasVersion::V14 => "1.4",
    }
    .to_string()
}

/// Decode a null-padded fixed-width ASCII field into a trimmed [`String`].
#[cfg(feature = "async")]
fn null_padded_to_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end])
        .trim_end()
        .to_string()
}

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

/// Decode parameters required to turn a raw LAZ/LAS point-data chunk into
/// [`Point`]s. Parsed once from the LAS public header + LASzip VLR so that
/// per-voxel reads do not re-parse the file preamble.
#[derive(Debug, Clone)]
struct ChunkDecodeParams {
    /// LAS point data record format ID (0-10).
    format_id: u8,
    /// LAS point record length in bytes.
    record_length: usize,
    /// (x, y, z) scale factors from the LAS header.
    scale: [f64; 3],
    /// (x, y, z) coordinate offsets from the LAS header.
    offset: [f64; 3],
    /// `true` when the point data is LAZ compressed (a LASzip VLR is present)
    /// and each chunk must be routed through the LAZ decompressor before
    /// deserialization; `false` when chunks hold raw LAS records.
    is_laz: bool,
}

/// COPC reader for local files
pub struct CopcReader {
    file: File,
    header: LasHeader,
    info: CopcInfo,
    hierarchy: CopcHierarchy,
    decode: ChunkDecodeParams,
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

        // Parse the LAS public header + VLR chain directly from the raw bytes
        // via oxigeo-copc so the decode parameters (format id, record length,
        // scale/offset and the LASzip-VLR presence flag) exactly match the
        // decoder's own expectations.
        let decode = Self::parse_decode_params(&mut file)?;

        Ok(Self {
            file,
            header,
            info,
            hierarchy,
            decode,
        })
    }

    /// Parse the [`ChunkDecodeParams`] from the file preamble.
    ///
    /// Reads the LAS public header to locate the VLR region, parses the VLR
    /// chain, and records whether a LASzip VLR (`user_id = "laszip encoded"`,
    /// `record_id = 22204`) is present. The file cursor is left at an
    /// unspecified position; callers must `seek` before subsequent reads.
    fn parse_decode_params(file: &mut File) -> Result<ChunkDecodeParams> {
        // The LAS public header is at most a few hundred bytes; read enough to
        // parse it and learn where point data begins.
        file.seek(SeekFrom::Start(0))?;
        let mut preamble = vec![0u8; 375];
        let read = read_up_to(file, &mut preamble)?;
        preamble.truncate(read);
        let copc_header = oxigeo_copc::LasHeader::parse(&preamble)
            .map_err(|e| Error::Copc(format!("failed to parse LAS header: {e}")))?;

        // Read the full VLR region (header .. offset_to_point_data) so the VLR
        // chain parser sees every record.
        let vlr_region_len = copc_header.offset_to_point_data as usize;
        let mut vlr_region = vec![0u8; vlr_region_len.max(preamble.len())];
        file.seek(SeekFrom::Start(0))?;
        let region_read = read_up_to(file, &mut vlr_region)?;
        vlr_region.truncate(region_read);

        let is_laz = match oxigeo_copc::vlr_chain::parse_vlrs(&vlr_region, &copc_header) {
            Ok(vlrs) => oxigeo_copc::detect_laszip_vlr(&vlrs).is_some(),
            // A malformed VLR chain should not sink an otherwise-readable
            // header; treat point data as uncompressed and let the chunk reader
            // surface any real error.
            Err(_) => false,
        };

        Ok(ChunkDecodeParams {
            format_id: copc_header.point_data_format_id,
            record_length: copc_header.point_data_record_length as usize,
            scale: [
                copc_header.scale_x,
                copc_header.scale_y,
                copc_header.scale_z,
            ],
            offset: [
                copc_header.offset_x,
                copc_header.offset_y,
                copc_header.offset_z,
            ],
            is_laz,
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

        if entry.byte_size < 0 {
            return Err(Error::Copc(format!(
                "voxel {:?} points at a child hierarchy page, not a data chunk",
                key
            )));
        }
        let point_count = entry.point_count as usize;
        let byte_size = entry.byte_size as usize;
        let offset = entry.offset;
        let decode = self.decode.clone();

        // Seek to the data offset
        self.file.seek(SeekFrom::Start(offset))?;

        // Read compressed chunk
        let mut chunk = vec![0u8; byte_size];
        self.file.read_exact(&mut chunk)?;

        // Decompress (if LAZ) and deserialize into points.
        decode_copc_chunk(&chunk, point_count, &decode)
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
    decode: ChunkDecodeParams,
}

#[cfg(feature = "async")]
impl CopcHttpReader {
    /// Open a COPC file via HTTP.
    ///
    /// Fetches the LAS public header and VLR chain via HTTP range requests,
    /// extracts the COPC info VLR and (if present) the LASzip VLR, then walks
    /// the octree hierarchy by fetching the root hierarchy page (and any child
    /// pages it references). No point data is fetched until [`read_voxel`] or
    /// [`query_bounds`] is called.
    ///
    /// [`read_voxel`]: Self::read_voxel
    /// [`query_bounds`]: Self::query_bounds
    pub async fn open(url: impl Into<String>) -> Result<Self> {
        let url = url.into();
        let client = Client::new();

        // Fetch enough of the preamble to parse the LAS public header.
        let head = Self::fetch_range_len(&client, &url, 0, 375).await?;
        let copc_header = oxigeo_copc::LasHeader::parse(&head)
            .map_err(|e| Error::Copc(format!("failed to parse LAS header: {e}")))?;

        // Fetch the full VLR region so the chain parser sees every record.
        let vlr_region_len = copc_header.offset_to_point_data as u64;
        let vlr_region = if vlr_region_len as usize <= head.len() {
            head.clone()
        } else {
            Self::fetch_range_len(&client, &url, 0, vlr_region_len).await?
        };

        let vlrs = oxigeo_copc::vlr_chain::parse_vlrs(&vlr_region, &copc_header)
            .map_err(|e| Error::Copc(format!("failed to parse VLR chain: {e}")))?;
        let copc_info = oxigeo_copc::vlr_chain::find_copc_info(&vlrs)
            .map_err(|e| Error::Copc(format!("missing COPC info VLR: {e}")))?;
        let is_laz = oxigeo_copc::detect_laszip_vlr(&vlrs).is_some();

        let point_format = PointFormat::try_from(copc_header.point_data_format_id)?;
        let (bmin, bmax) = (
            [copc_header.min_x, copc_header.min_y, copc_header.min_z],
            [copc_header.max_x, copc_header.max_y, copc_header.max_z],
        );
        let header = LasHeader {
            version: las_version_string(copc_header.version),
            point_format,
            point_count: copc_header.number_of_point_records,
            bounds: Bounds3d::new(bmin[0], bmax[0], bmin[1], bmax[1], bmin[2], bmax[2]),
            scale: (
                copc_header.scale_x,
                copc_header.scale_y,
                copc_header.scale_z,
            ),
            offset: (
                copc_header.offset_x,
                copc_header.offset_y,
                copc_header.offset_z,
            ),
            system_identifier: null_padded_to_string(&copc_header.system_id),
            generating_software: null_padded_to_string(&copc_header.generating_software),
        };

        let info = CopcInfo {
            center_x: copc_info.center_x,
            center_y: copc_info.center_y,
            center_z: copc_info.center_z,
            halfsize: copc_info.halfsize,
            spacing: copc_info.spacing,
            root_hier_offset: copc_info.root_hier_offset,
            root_hier_size: copc_info.root_hier_size,
            gps_time_min: copc_info.gpstime_minimum,
            gps_time_max: copc_info.gpstime_maximum,
        };

        let decode = ChunkDecodeParams {
            format_id: copc_header.point_data_format_id,
            record_length: copc_header.point_data_record_length as usize,
            scale: [
                copc_header.scale_x,
                copc_header.scale_y,
                copc_header.scale_z,
            ],
            offset: [
                copc_header.offset_x,
                copc_header.offset_y,
                copc_header.offset_z,
            ],
            is_laz,
        };

        // Walk the hierarchy over HTTP, fetching each page as needed.
        let hierarchy = Self::fetch_hierarchy(&client, &url, &info).await?;

        Ok(Self {
            url,
            client,
            header,
            info,
            hierarchy,
            decode,
        })
    }

    /// Fetch and parse the COPC octree hierarchy over HTTP.
    ///
    /// Starts from the root page referenced by `info` and follows child-page
    /// references (entries with negative `byte_size`), capped at
    /// [`copc_vlr::COPC_MAX_HIERARCHY_DEPTH`] page fetches.
    async fn fetch_hierarchy(client: &Client, url: &str, info: &CopcInfo) -> Result<CopcHierarchy> {
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

            let buf = Self::fetch_range_len(client, url, page_off, page_size).await?;
            let entries = parse_hierarchy_page(&buf)?;
            for entry in entries {
                if entry.byte_size < 0 {
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

    /// Fetch exactly `len` bytes starting at `start` via an HTTP range request.
    ///
    /// HTTP byte ranges are inclusive, so `len` bytes span `start..=start+len-1`.
    /// The response is truncated to `len` bytes in case the server returns more.
    async fn fetch_range_len(client: &Client, url: &str, start: u64, len: u64) -> Result<Bytes> {
        if len == 0 {
            return Ok(Bytes::new());
        }
        let end = start + len - 1;
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

        let mut bytes = response.bytes().await?;
        if bytes.len() as u64 > len {
            bytes = bytes.slice(0..len as usize);
        }
        Ok(bytes)
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
        if entry.byte_size < 0 {
            return Err(Error::Copc(format!(
                "voxel {:?} points at a child hierarchy page, not a data chunk",
                key
            )));
        }
        let point_count = entry.point_count as usize;
        let byte_size = entry.byte_size as u64;
        let offset = entry.offset;

        // Fetch the compressed chunk (exactly `byte_size` bytes).
        let chunk = Self::fetch_range_len(&self.client, &self.url, offset, byte_size).await?;

        // Decompress (if LAZ) and deserialize into points.
        decode_copc_chunk(&chunk, point_count, &self.decode)
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

#[cfg(test)]
mod decode_tests {
    //! Tests for the LAZ/LAS chunk decode path that `read_voxel` delegates to.
    //!
    //! These exercise the real wiring into `oxigeo-copc`: an uncompressed raw
    //! LAS chunk and a genuinely LAZ-compressed chunk (produced by the
    //! `oxigeo-copc` encoder) are both decoded through the exact same code path
    //! used in production, replacing the former silent-stub behaviour.

    use super::*;

    /// Build a tightly-packed LAS Point Format 0 record (20 bytes).
    fn make_pf0(raw_x: i32, raw_y: i32, raw_z: i32, intensity: u16, classification: u8) -> Vec<u8> {
        let mut rec = vec![0u8; 20];
        rec[0..4].copy_from_slice(&raw_x.to_le_bytes());
        rec[4..8].copy_from_slice(&raw_y.to_le_bytes());
        rec[8..12].copy_from_slice(&raw_z.to_le_bytes());
        rec[12..14].copy_from_slice(&intensity.to_le_bytes());
        rec[14] = 1 | (1 << 3); // return_number=1, number_of_returns=1
        rec[15] = classification;
        rec
    }

    fn decode_params(is_laz: bool, format_id: u8, record_length: usize) -> ChunkDecodeParams {
        ChunkDecodeParams {
            format_id,
            record_length,
            scale: [0.001, 0.001, 0.001],
            offset: [0.0, 0.0, 0.0],
            is_laz,
        }
    }

    #[test]
    fn decode_uncompressed_chunk_yields_points() {
        let mut chunk = Vec::new();
        chunk.extend_from_slice(&make_pf0(123_456, 789_012, 345_678, 42, 2));
        chunk.extend_from_slice(&make_pf0(200_000, 100_000, 50_000, 7, 5));

        let params = decode_params(false, 0, 20);
        let points = decode_copc_chunk(&chunk, 2, &params).expect("decode uncompressed chunk");

        assert_eq!(points.len(), 2);
        assert!((points[0].x - 123.456).abs() < 1e-6);
        assert!((points[0].y - 789.012).abs() < 1e-6);
        assert!((points[0].z - 345.678).abs() < 1e-6);
        assert_eq!(points[0].intensity, 42);
        assert!((points[1].x - 200.0).abs() < 1e-6);
    }

    #[test]
    fn decode_laz_compressed_chunk_round_trips() {
        // Assemble three raw PF0 records, LAZ-compress them with the
        // oxigeo-copc encoder, then decode through the production path.
        let records: Vec<Vec<u8>> = vec![
            make_pf0(100_000, 100_000, 10_000, 11, 2),
            make_pf0(250_000, 260_000, 20_000, 22, 5),
            make_pf0(-50_000, 400_000, -3_000, 33, 6),
        ];
        let mut raw = Vec::new();
        for r in &records {
            raw.extend_from_slice(r);
        }

        let compressed = oxigeo_copc::laz::format_v1::compress_format_0(&raw, records.len());

        let params = decode_params(true, 0, 20);
        let points =
            decode_copc_chunk(&compressed, records.len(), &params).expect("decode LAZ chunk");

        assert_eq!(points.len(), 3);
        assert!((points[0].x - 100.0).abs() < 1e-6);
        assert!((points[1].x - 250.0).abs() < 1e-6);
        assert!((points[1].y - 260.0).abs() < 1e-6);
        assert!((points[2].x - (-50.0)).abs() < 1e-6);
        assert_eq!(points[0].intensity, 11);
        assert_eq!(points[2].intensity, 33);
    }

    #[test]
    fn decode_empty_chunk_is_empty() {
        let params = decode_params(true, 0, 20);
        let points = decode_copc_chunk(&[], 0, &params).expect("zero-point chunk");
        assert!(points.is_empty());
    }

    #[test]
    fn decode_unsupported_laz_format_errors() {
        // Format 6 is a valid COPC format but the pure-Rust LAZ decoder does
        // not yet support it: the decode must surface a typed error rather than
        // silently returning no points.
        let params = decode_params(true, 6, 30);
        let err =
            decode_copc_chunk(&[0u8; 64], 1, &params).expect_err("format 6 LAZ decode must error");
        assert!(matches!(err, Error::LazCompression(_)));
    }

    #[test]
    fn convert_preserves_rgb_and_classification() {
        let p = oxigeo_copc::Point3D {
            x: 1.0,
            y: 2.0,
            z: 3.0,
            intensity: 5,
            return_number: 1,
            number_of_returns: 1,
            classification: 6,
            scan_angle_rank: 0.0,
            user_data: 0,
            point_source_id: 0,
            gps_time: None,
            red: Some(100),
            green: Some(200),
            blue: Some(300),
            nir: None,
            waveform: None,
        };
        let point = copc_point_to_point(&p);
        assert_eq!(point.intensity, 5);
        let color = point.color.expect("rgb preserved");
        assert_eq!((color.red, color.green, color.blue), (100, 200, 300));
    }

    #[test]
    fn read_up_to_stops_at_eof() {
        let data = vec![1u8, 2, 3];
        let mut cursor = std::io::Cursor::new(data);
        let mut buf = vec![0u8; 8];
        let n = read_up_to(&mut cursor, &mut buf).expect("read");
        assert_eq!(n, 3);
        assert_eq!(&buf[..3], &[1, 2, 3]);
    }
}
