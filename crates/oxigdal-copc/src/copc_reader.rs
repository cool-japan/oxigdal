//! High-level COPC file reader that ties together header parsing, VLR chain
//! walking, hierarchy traversal and point deserialization.
//!
//! [`CopcReader`] provides a single entry-point for loading a COPC file from
//! an in-memory byte slice and running spatial queries against its octree
//! index.

use crate::copc_vlr::CopcInfo;
use crate::error::CopcError;
use crate::hierarchy::{HierarchyEntry, query_hierarchy_with_page_pointers};
use crate::las_header::LasHeader;
use crate::point::{BoundingBox3D, Point3D};
use crate::point_format;
use crate::vlr_chain;

/// A parsed COPC file ready for spatial queries.
///
/// # Example
/// ```ignore
/// use oxigdal_copc::copc_reader::CopcReader;
/// use oxigdal_copc::point::BoundingBox3D;
///
/// let data = std::fs::read("my_file.copc.laz").expect("read file");
/// let reader = CopcReader::from_bytes(&data).expect("parse COPC");
/// let bbox = BoundingBox3D::new(0.0, 0.0, 0.0, 100.0, 100.0, 50.0).expect("valid bbox");
/// let points = reader.query_points_in_bbox(&bbox).expect("query");
/// ```
pub struct CopcReader {
    /// Parsed LAS public header.
    header: LasHeader,
    /// COPC info VLR body.
    copc_info: CopcInfo,
    /// All VLRs from the file.
    vlrs: Vec<crate::copc_vlr::Vlr>,
    /// Extended VLRs (LAS 1.4), parsed once from the header's EVLR locator.
    extended_vlrs: Vec<crate::extended_vlr::ExtendedVlr>,
    /// The raw file bytes, kept for hierarchy + point data access.
    data: Vec<u8>,
    /// Parsed LASzip VLR information, present iff the point data is LAZ
    /// compressed.  When `None`, chunk bytes are treated as raw LAS records;
    /// when `Some`, the chunk is routed through [`crate::laz::decompress_chunk`].
    pub(crate) laz_info: Option<crate::laz::LazVlrInfo>,
}

impl CopcReader {
    /// Parse a COPC file from an in-memory byte slice.
    ///
    /// Parses the LAS header, walks the VLR chain to extract the COPC info
    /// and hierarchy VLR, and stores the raw data for later queries.
    ///
    /// # Errors
    /// Returns [`CopcError`] when the data is not a valid COPC file (bad
    /// magic, missing VLRs, truncated data, etc.).
    pub fn from_bytes(data: &[u8]) -> Result<Self, CopcError> {
        let header = LasHeader::parse(data)?;

        let vlrs = vlr_chain::parse_vlrs(data, &header)?;

        let copc_info = vlr_chain::find_copc_info(&vlrs)?;

        // Verify the hierarchy VLR exists (we don't need its payload yet).
        let _hierarchy_vlr = vlr_chain::find_copc_hierarchy_vlr(&vlrs)?;

        // Detect and parse the optional LASzip VLR.  If present, chunks will
        // be routed through the LAZ decompressor before deserialization.
        let laz_info = if let Some(vlr) = crate::laz::detect_laszip_vlr(&vlrs) {
            Some(crate::laz::parse_laszip_vlr_data(&vlr.data)?)
        } else {
            None
        };

        // Parse Extended VLRs (LAS 1.4) from the header's EVLR locator. EVLRs
        // are advisory metadata, so a malformed/absent EVLR region degrades to
        // an empty list rather than failing an otherwise-valid COPC read.
        let extended_vlrs = crate::extended_vlr::parse_evlr_chain(
            data,
            header.start_of_first_evlr(),
            header.number_of_evlrs(),
        )
        .unwrap_or_default();

        Ok(Self {
            header,
            copc_info,
            vlrs,
            extended_vlrs,
            data: data.to_vec(),
            laz_info,
        })
    }

    /// Return a reference to the parsed LAS header.
    pub fn header(&self) -> &LasHeader {
        &self.header
    }

    /// Return a reference to the parsed COPC info.
    pub fn copc_info(&self) -> &CopcInfo {
        &self.copc_info
    }

    /// Return a reference to the VLR list.
    pub fn vlrs(&self) -> &[crate::copc_vlr::Vlr] {
        &self.vlrs
    }

    /// Return a reference to the Extended VLR (EVLR) list.
    ///
    /// EVLRs are parsed once at construction from the LAS 1.4 header's EVLR
    /// locator (`start_of_first_evlr` / `number_of_evlrs`). The slice is empty
    /// for files without EVLRs or for pre-1.4 versions.
    pub fn extended_vlrs(&self) -> &[crate::extended_vlr::ExtendedVlr] {
        &self.extended_vlrs
    }

    /// Extract structural coordinate-reference-system metadata.
    ///
    /// Scans both the classic VLRs and the Extended VLRs for the GeoTIFF tags
    /// (record IDs 34735/34736/34737) and the OGC WKT record (2112), returning
    /// the parsed structures. No semantic CRS interpretation is performed.
    pub fn crs(&self) -> crate::crs_vlr::CrsInfo {
        crate::crs_vlr::extract_crs_info(&self.vlrs, &self.extended_vlrs)
    }

    /// Return the full octree bounding box as a [`BoundingBox3D`].
    pub fn octree_bounds(&self) -> BoundingBox3D {
        let (min, max) = self.copc_info.bounds();
        BoundingBox3D {
            min_x: min[0],
            min_y: min[1],
            min_z: min[2],
            max_x: max[0],
            max_y: max[1],
            max_z: max[2],
        }
    }

    /// Query all points whose positions fall within the given 3D bounding box.
    ///
    /// Traverses the COPC hierarchy to find octree nodes that intersect
    /// `bbox`, reads the corresponding point data chunks and deserializes
    /// them, then filters to only those points geometrically inside `bbox`.
    ///
    /// # Errors
    /// Returns [`CopcError`] on hierarchy traversal or point deserialization
    /// errors.
    pub fn query_points_in_bbox(&self, bbox: &BoundingBox3D) -> Result<Vec<Point3D>, CopcError> {
        let entries = query_hierarchy_with_page_pointers(
            &self.data,
            &self.copc_info,
            self.copc_info.root_hier_offset,
            self.copc_info.root_hier_size,
            bbox,
        )?;

        let scale = [
            self.header.scale_x,
            self.header.scale_y,
            self.header.scale_z,
        ];
        let offset = [
            self.header.offset_x,
            self.header.offset_y,
            self.header.offset_z,
        ];
        let format_id = self.header.point_data_format_id;
        let record_length = self.header.point_data_record_length as usize;

        let mut result: Vec<Point3D> = Vec::new();

        for entry in &entries {
            let chunk_offset = entry.offset as usize;
            let chunk_size = entry.byte_count as usize;
            let point_count = entry.point_count as usize;

            if chunk_offset + chunk_size > self.data.len() {
                return Err(CopcError::InvalidFormat(format!(
                    "Point data chunk at offset {chunk_offset} + size {chunk_size} \
                     exceeds file length {}",
                    self.data.len()
                )));
            }

            let chunk_data = &self.data[chunk_offset..chunk_offset + chunk_size];

            // Route through the LAZ decompressor when the file carries a
            // LASzip VLR, otherwise feed the raw bytes straight to the
            // point-record deserializer.
            let decompressed: Vec<u8>;
            let bytes_for_deserialize: &[u8] = if self.laz_info.is_some() {
                if format_id <= 1 {
                    decompressed = crate::laz::decompress_chunk(
                        chunk_data,
                        point_count,
                        record_length,
                        format_id,
                    )?;
                    &decompressed
                } else {
                    return Err(CopcError::UnsupportedLazFormat { format_id });
                }
            } else {
                chunk_data
            };

            let points = point_format::deserialize_points(
                bytes_for_deserialize,
                point_count,
                record_length,
                format_id,
                scale,
                offset,
            )?;

            // Filter to only points inside the bbox
            for pt in points {
                if bbox.contains(&pt) {
                    result.push(pt);
                }
            }
        }

        Ok(result)
    }

    /// Query all points within the entire file (no spatial filter).
    ///
    /// Walks the hierarchy and reads all point data chunks.
    ///
    /// # Errors
    /// Returns [`CopcError`] on hierarchy or point deserialization errors.
    pub fn all_points(&self) -> Result<Vec<Point3D>, CopcError> {
        let full_bbox = self.octree_bounds();
        // Use a slightly expanded bbox to ensure all points are captured
        let expanded = full_bbox.expand_by(1.0);
        self.query_points_in_bbox(&expanded)
    }

    /// Return the total number of hierarchy entries (data chunks) that
    /// intersect the given bounding box, without reading point data.
    ///
    /// Useful for estimating query cost before fetching actual points.
    pub fn count_intersecting_chunks(
        &self,
        bbox: &BoundingBox3D,
    ) -> Result<Vec<HierarchyEntry>, CopcError> {
        query_hierarchy_with_page_pointers(
            &self.data,
            &self.copc_info,
            self.copc_info.root_hier_offset,
            self.copc_info.root_hier_size,
            bbox,
        )
    }

    /// Return all hierarchy entries at a given depth level.
    ///
    /// This traverses the full hierarchy and filters by depth.
    pub fn entries_at_depth(&self, depth: i32) -> Result<Vec<HierarchyEntry>, CopcError> {
        let full_bbox = self.octree_bounds().expand_by(1.0);
        let all_entries = query_hierarchy_with_page_pointers(
            &self.data,
            &self.copc_info,
            self.copc_info.root_hier_offset,
            self.copc_info.root_hier_size,
            &full_bbox,
        )?;
        Ok(all_entries
            .into_iter()
            .filter(|e| e.key.depth == depth)
            .collect())
    }
}

impl std::fmt::Debug for CopcReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CopcReader")
            .field("version", &self.header.version)
            .field("point_format", &self.header.point_data_format_id)
            .field("record_length", &self.header.point_data_record_length)
            .field("num_vlrs", &self.vlrs.len())
            .field("octree_center_x", &self.copc_info.center_x)
            .field("octree_center_y", &self.copc_info.center_y)
            .field("octree_center_z", &self.copc_info.center_z)
            .field("octree_halfsize", &self.copc_info.halfsize)
            .field("file_size", &self.data.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a complete synthetic COPC file with the given point format.
    ///
    /// Layout:
    ///   [0..227]    LAS header
    ///   [227..441]  VLR #0: COPC info (54 hdr + 160 payload)
    ///   [441..509]  VLR #1: COPC hierarchy (54 hdr + 14 payload = placeholder)
    ///   [509..]     Point data
    ///   [....]      Hierarchy page
    fn build_synthetic_copc(format_id: u8, record_length: u16, points: &[Vec<u8>]) -> Vec<u8> {
        // We'll build the file in stages, then patch offsets at the end.

        // 1. LAS header (227 bytes minimum for LAS 1.4)
        let header_size: u16 = 227;
        let mut file = vec![0u8; header_size as usize];
        file[0..4].copy_from_slice(b"LASF");
        file[24] = 1; // major
        file[25] = 4; // minor
        file[94..96].copy_from_slice(&header_size.to_le_bytes());
        file[100..104].copy_from_slice(&2u32.to_le_bytes()); // 2 VLRs
        file[104] = format_id;
        file[105..107].copy_from_slice(&record_length.to_le_bytes());
        // Legacy point count
        file[107..111].copy_from_slice(&(points.len() as u32).to_le_bytes());
        // Scale
        let scale = 0.001f64.to_le_bytes();
        file[131..139].copy_from_slice(&scale);
        file[139..147].copy_from_slice(&scale);
        file[147..155].copy_from_slice(&scale);
        // Offset = 0
        // Bounds: compute from actual point values would be complex, use generous values
        file[179..187].copy_from_slice(&1000.0f64.to_le_bytes()); // max_x
        file[187..195].copy_from_slice(&(-1000.0f64).to_le_bytes()); // min_x
        file[195..203].copy_from_slice(&1000.0f64.to_le_bytes()); // max_y
        file[203..211].copy_from_slice(&(-1000.0f64).to_le_bytes()); // min_y
        file[211..219].copy_from_slice(&1000.0f64.to_le_bytes()); // max_z
        file[219..227].copy_from_slice(&(-1000.0f64).to_le_bytes()); // min_z

        // 2. VLR #0: COPC info (user_id="copc", record_id=1, 160-byte body)
        let copc_info_body = {
            let mut body = vec![0u8; 160];
            body[0..8].copy_from_slice(&500.0f64.to_le_bytes()); // center_x
            body[8..16].copy_from_slice(&500.0f64.to_le_bytes()); // center_y
            body[16..24].copy_from_slice(&500.0f64.to_le_bytes()); // center_z
            body[24..32].copy_from_slice(&500.0f64.to_le_bytes()); // halfsize
            body[32..40].copy_from_slice(&1.0f64.to_le_bytes()); // spacing
            // root_hier_offset and root_hier_size will be patched later
            body
        };
        append_vlr_to_file(&mut file, "copc", 1, &copc_info_body);

        // 3. VLR #1: COPC hierarchy (user_id="copc", record_id=1000, empty for now)
        // The actual hierarchy data is placed after point data.
        append_vlr_to_file(&mut file, "copc", 1000, b"");

        // 4. Patch offset_to_point_data
        let point_data_offset = file.len() as u32;
        file[96..100].copy_from_slice(&point_data_offset.to_le_bytes());

        // 5. Write point data
        let point_data_start = file.len();
        for p in points {
            file.extend_from_slice(p);
        }
        let point_data_end = file.len();
        let point_data_size = point_data_end - point_data_start;

        // 6. Write hierarchy page: single root entry pointing to all points
        let hier_offset = file.len();
        let hier_entry = crate::hierarchy::HierarchyEntry {
            key: crate::hierarchy::VoxelKey {
                depth: 0,
                x: 0,
                y: 0,
                z: 0,
            },
            offset: point_data_start as u64,
            byte_count: point_data_size as i32,
            point_count: points.len() as i32,
        };
        file.extend_from_slice(&hier_entry.to_bytes());
        let hier_size = 32u64; // one entry

        // 7. Patch CopcInfo: root_hier_offset at body offset 40, root_hier_size at 48
        // CopcInfo VLR body starts at header_size + 54 (VLR header for first VLR)
        let copc_body_offset = header_size as usize + 54;
        file[copc_body_offset + 40..copc_body_offset + 48]
            .copy_from_slice(&(hier_offset as u64).to_le_bytes());
        file[copc_body_offset + 48..copc_body_offset + 56]
            .copy_from_slice(&hier_size.to_le_bytes());

        file
    }

    fn append_vlr_to_file(file: &mut Vec<u8>, user_id: &str, record_id: u16, payload: &[u8]) {
        // reserved (2 bytes)
        file.extend_from_slice(&[0u8; 2]);
        // user_id (16 bytes)
        let uid_bytes = user_id.as_bytes();
        let mut uid_buf = [0u8; 16];
        let len = uid_bytes.len().min(16);
        uid_buf[..len].copy_from_slice(&uid_bytes[..len]);
        file.extend_from_slice(&uid_buf);
        // record_id
        file.extend_from_slice(&record_id.to_le_bytes());
        // record_length_after_header
        file.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        // description (32 bytes)
        file.extend_from_slice(&[0u8; 32]);
        // payload
        file.extend_from_slice(payload);
    }

    /// Build a format-6 point record (30 bytes).
    fn make_format6_point(raw_x: i32, raw_y: i32, raw_z: i32) -> Vec<u8> {
        let mut rec = vec![0u8; 30];
        rec[0..4].copy_from_slice(&raw_x.to_le_bytes());
        rec[4..8].copy_from_slice(&raw_y.to_le_bytes());
        rec[8..12].copy_from_slice(&raw_z.to_le_bytes());
        rec[12..14].copy_from_slice(&100u16.to_le_bytes()); // intensity
        rec[14] = 1 | (1 << 4); // return_number=1, number_of_returns=1
        rec[16] = 2; // classification = Ground
        rec[22..30].copy_from_slice(&1.0f64.to_le_bytes()); // GPS time
        rec
    }

    /// Build a format-0 point record (20 bytes).
    fn make_format0_point(raw_x: i32, raw_y: i32, raw_z: i32) -> Vec<u8> {
        let mut rec = vec![0u8; 20];
        rec[0..4].copy_from_slice(&raw_x.to_le_bytes());
        rec[4..8].copy_from_slice(&raw_y.to_le_bytes());
        rec[8..12].copy_from_slice(&raw_z.to_le_bytes());
        rec[12..14].copy_from_slice(&50u16.to_le_bytes()); // intensity
        rec[14] = 1 | (1 << 3); // return=1, num_returns=1
        rec[15] = 2; // classification = Ground
        rec
    }

    // ---- CopcReader construction tests ----

    #[test]
    fn test_copc_reader_from_bytes_format6() {
        let points = vec![
            make_format6_point(100_000, 200_000, 50_000), // (100, 200, 50)
            make_format6_point(300_000, 400_000, 100_000), // (300, 400, 100)
        ];
        let file_data = build_synthetic_copc(6, 30, &points);
        let reader = CopcReader::from_bytes(&file_data).expect("should parse synthetic COPC");

        assert_eq!(reader.header().point_data_format_id, 6);
        assert_eq!(reader.vlrs().len(), 2);
        assert!((reader.copc_info().center_x - 500.0).abs() < f64::EPSILON);
        assert!((reader.copc_info().halfsize - 500.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_copc_reader_from_bytes_format0() {
        let points = vec![
            make_format0_point(50_000, 50_000, 10_000), // (50, 50, 10)
        ];
        let file_data = build_synthetic_copc(0, 20, &points);
        let reader = CopcReader::from_bytes(&file_data).expect("should parse format-0 COPC");
        assert_eq!(reader.header().point_data_format_id, 0);
    }

    #[test]
    fn test_copc_reader_octree_bounds() {
        let points = vec![make_format6_point(0, 0, 0)];
        let file_data = build_synthetic_copc(6, 30, &points);
        let reader = CopcReader::from_bytes(&file_data).expect("parse");
        let bounds = reader.octree_bounds();
        // center=500, halfsize=500 -> min=0, max=1000
        assert!((bounds.min_x - 0.0).abs() < 1e-9);
        assert!((bounds.max_x - 1000.0).abs() < 1e-9);
    }

    // ---- Spatial query tests ----

    #[test]
    fn test_query_points_all_inside_bbox() {
        let points = vec![
            make_format6_point(100_000, 200_000, 50_000),
            make_format6_point(300_000, 400_000, 100_000),
        ];
        let file_data = build_synthetic_copc(6, 30, &points);
        let reader = CopcReader::from_bytes(&file_data).expect("parse");

        // Query that covers all points
        let bbox = BoundingBox3D::new(0.0, 0.0, 0.0, 500.0, 500.0, 500.0).expect("valid bbox");
        let result = reader.query_points_in_bbox(&bbox).expect("query");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_query_points_partial_bbox() {
        let points = vec![
            make_format6_point(100_000, 100_000, 10_000), // (100, 100, 10)
            make_format6_point(800_000, 800_000, 10_000), // (800, 800, 10)
        ];
        let file_data = build_synthetic_copc(6, 30, &points);
        let reader = CopcReader::from_bytes(&file_data).expect("parse");

        // Query only covers the first point
        let bbox = BoundingBox3D::new(50.0, 50.0, 0.0, 200.0, 200.0, 50.0).expect("valid bbox");
        let result = reader.query_points_in_bbox(&bbox).expect("query");
        assert_eq!(result.len(), 1);
        assert!((result[0].x - 100.0).abs() < 1e-6);
    }

    #[test]
    fn test_query_points_none_in_bbox() {
        let points = vec![
            make_format6_point(100_000, 100_000, 10_000), // (100, 100, 10)
        ];
        let file_data = build_synthetic_copc(6, 30, &points);
        let reader = CopcReader::from_bytes(&file_data).expect("parse");

        // Query bbox that doesn't contain any points
        let bbox =
            BoundingBox3D::new(500.0, 500.0, 500.0, 600.0, 600.0, 600.0).expect("valid bbox");
        let result = reader.query_points_in_bbox(&bbox).expect("query");
        assert!(result.is_empty());
    }

    #[test]
    fn test_query_points_coordinates_correct() {
        let points = vec![make_format6_point(123_456, 789_012, 345_678)];
        let file_data = build_synthetic_copc(6, 30, &points);
        let reader = CopcReader::from_bytes(&file_data).expect("parse");

        let bbox = BoundingBox3D::new(0.0, 0.0, 0.0, 1000.0, 1000.0, 1000.0).expect("valid bbox");
        let result = reader.query_points_in_bbox(&bbox).expect("query");
        assert_eq!(result.len(), 1);
        // 123456 * 0.001 = 123.456
        assert!((result[0].x - 123.456).abs() < 1e-6);
        assert!((result[0].y - 789.012).abs() < 1e-6);
        assert!((result[0].z - 345.678).abs() < 1e-6);
    }

    #[test]
    fn test_all_points() {
        let points = vec![
            make_format6_point(100_000, 100_000, 100_000),
            make_format6_point(200_000, 200_000, 200_000),
            make_format6_point(300_000, 300_000, 300_000),
        ];
        let file_data = build_synthetic_copc(6, 30, &points);
        let reader = CopcReader::from_bytes(&file_data).expect("parse");

        let all = reader.all_points().expect("all_points");
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_count_intersecting_chunks() {
        let points = vec![make_format6_point(500_000, 500_000, 500_000)];
        let file_data = build_synthetic_copc(6, 30, &points);
        let reader = CopcReader::from_bytes(&file_data).expect("parse");

        let bbox = BoundingBox3D::new(0.0, 0.0, 0.0, 1000.0, 1000.0, 1000.0).expect("valid bbox");
        let entries = reader.count_intersecting_chunks(&bbox).expect("count");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].point_count, 1);
    }

    #[test]
    fn test_entries_at_depth() {
        let points = vec![make_format6_point(500_000, 500_000, 500_000)];
        let file_data = build_synthetic_copc(6, 30, &points);
        let reader = CopcReader::from_bytes(&file_data).expect("parse");

        let depth0 = reader.entries_at_depth(0).expect("depth 0");
        assert_eq!(depth0.len(), 1);

        let depth1 = reader.entries_at_depth(1).expect("depth 1");
        assert!(depth1.is_empty());
    }

    // ---- Error handling tests ----

    #[test]
    fn test_copc_reader_bad_magic() {
        let mut file_data = build_synthetic_copc(6, 30, &[]);
        file_data[0] = b'X'; // corrupt magic
        assert!(CopcReader::from_bytes(&file_data).is_err());
    }

    #[test]
    fn test_copc_reader_missing_copc_vlr() {
        // Build a file with only a non-COPC VLR
        let mut file = vec![0u8; 227];
        file[0..4].copy_from_slice(b"LASF");
        file[24] = 1;
        file[25] = 4;
        file[94..96].copy_from_slice(&227u16.to_le_bytes());
        file[96..100].copy_from_slice(&227u32.to_le_bytes());
        file[100..104].copy_from_slice(&1u32.to_le_bytes()); // 1 VLR
        file[104] = 6;
        file[105..107].copy_from_slice(&30u16.to_le_bytes());
        let scale = 0.001f64.to_le_bytes();
        file[131..139].copy_from_slice(&scale);
        file[139..147].copy_from_slice(&scale);
        file[147..155].copy_from_slice(&scale);
        append_vlr_to_file(&mut file, "laszip", 22204, b"data");
        assert!(CopcReader::from_bytes(&file).is_err());
    }

    #[test]
    fn test_copc_reader_debug_format() {
        let points = vec![make_format6_point(0, 0, 0)];
        let file_data = build_synthetic_copc(6, 30, &points);
        let reader = CopcReader::from_bytes(&file_data).expect("parse");
        let debug_str = format!("{reader:?}");
        assert!(debug_str.contains("CopcReader"));
        assert!(debug_str.contains("point_format"));
    }

    #[test]
    fn test_copc_reader_format0_query() {
        let points = vec![
            make_format0_point(250_000, 250_000, 50_000), // (250, 250, 50)
            make_format0_point(750_000, 750_000, 50_000), // (750, 750, 50)
        ];
        let file_data = build_synthetic_copc(0, 20, &points);
        let reader = CopcReader::from_bytes(&file_data).expect("parse");

        let bbox = BoundingBox3D::new(200.0, 200.0, 0.0, 300.0, 300.0, 100.0).expect("valid bbox");
        let result = reader.query_points_in_bbox(&bbox).expect("query");
        assert_eq!(result.len(), 1);
        assert!((result[0].x - 250.0).abs() < 1e-6);
    }

    #[test]
    fn test_copc_reader_multiple_points_ordering() {
        let points = vec![
            make_format6_point(100_000, 100_000, 10_000),
            make_format6_point(200_000, 200_000, 20_000),
            make_format6_point(300_000, 300_000, 30_000),
            make_format6_point(400_000, 400_000, 40_000),
            make_format6_point(500_000, 500_000, 50_000),
        ];
        let file_data = build_synthetic_copc(6, 30, &points);
        let reader = CopcReader::from_bytes(&file_data).expect("parse");

        let bbox = BoundingBox3D::new(0.0, 0.0, 0.0, 600.0, 600.0, 100.0).expect("valid bbox");
        let result = reader.query_points_in_bbox(&bbox).expect("query");
        assert_eq!(result.len(), 5);
        // Points should appear in order
        assert!((result[0].x - 100.0).abs() < 1e-6);
        assert!((result[4].x - 500.0).abs() < 1e-6);
    }

    #[test]
    fn test_copc_reader_point_attributes_preserved() {
        let mut rec = make_format6_point(100_000, 200_000, 50_000);
        rec[12..14].copy_from_slice(&42u16.to_le_bytes()); // intensity = 42
        rec[14] = 2 | (3 << 4); // return=2, num_returns=3
        rec[16] = 5; // classification = High Vegetation

        let points = vec![rec];
        let file_data = build_synthetic_copc(6, 30, &points);
        let reader = CopcReader::from_bytes(&file_data).expect("parse");

        let bbox = BoundingBox3D::new(0.0, 0.0, 0.0, 200.0, 300.0, 100.0).expect("valid bbox");
        let result = reader.query_points_in_bbox(&bbox).expect("query");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].intensity, 42);
        assert_eq!(result[0].return_number, 2);
        assert_eq!(result[0].number_of_returns, 3);
        assert_eq!(result[0].classification, 5);
    }
}
