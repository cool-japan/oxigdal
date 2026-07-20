//! NTv2 binary grid-shift parser and bilinear interpolator.
//!
//! Implements the NTv2 (National Transformation version 2) format as specified
//! in NRCan IOGP Geomatics Guidance Note 7-2 §4.4.4.  NTv2 `.gsb` files are
//! used for high-accuracy geographic datum shifts such as:
//!
//! - NAD27 ↔ NAD83 (Canada / CONUS)
//! - AGD66 ↔ GDA94 (Australia)
//! - OSGB36 ↔ ETRS89 (Great Britain)
//!
//! # File Format Overview
//!
//! An NTv2 file consists of:
//! 1. **Overview header** — 11 OREC records of 16 bytes each (176 bytes total).
//! 2. **Sub-grid blocks** — one or more sub-grids, each comprising:
//!    - An 11-SREC sub-grid header (176 bytes)
//!    - `GS_COUNT` data records of 16 bytes each
//!      (4 × f32: lat_shift, lon_shift, lat_accuracy, lon_accuracy)
//!
//! # Endianness
//!
//! The byte order is determined by reading bytes 8–11 of the file.  If they
//! equal `[0x0B, 0x00, 0x00, 0x00]` the file is little-endian; if they equal
//! `[0x00, 0x00, 0x00, 0x0B]` the file is big-endian.
//!
//! # Nested Sub-Grids
//!
//! Sub-grids may be nested: a child sub-grid's `PARENT` field names its parent
//! sub-grid.  When transforming a point, the most-specific (deepest) sub-grid
//! whose bounds contain the point is preferred over its ancestors.
//!
//! # Interpolation
//!
//! Shifts are obtained by bilinear interpolation over the four surrounding
//! grid nodes; the computed shift (in arc-seconds) is added to the input
//! geographic coordinates (in degrees).

use byteorder::{BigEndian, ByteOrder, LittleEndian};
use std::io::{Cursor, Read};

use crate::error::{Error, Result};

// ─────────────────────────────────────────────────────────────────────────────
// Public data structures
// ─────────────────────────────────────────────────────────────────────────────

/// Top-level NTv2 grid container parsed from a `.gsb` binary file.
///
/// This is the primary entry point: parse with [`NtV2Grid::from_bytes`] and
/// apply with [`NtV2Grid::transform`].
#[derive(Debug, Clone)]
pub struct NtV2Grid {
    /// Overview (file-level) header metadata.
    pub overview: NtV2Header,
    /// Ordered list of sub-grids.  Indices into this `Vec` are used by the
    /// `children` field of each [`NtV2SubGrid`].
    pub sub_grids: Vec<NtV2SubGrid>,
}

/// File-level overview header parsed from the 11 OREC records.
#[derive(Debug, Clone)]
pub struct NtV2Header {
    /// Number of sub-grid blocks in the file.
    pub num_file: u32,
    /// Grid-shift type string (e.g. `"SECONDS"` for arc-second shifts).
    pub gs_type: String,
    /// File format version string (e.g. `"NTv2.0"`).
    pub version: String,
    /// Source coordinate system name.
    pub system_f: String,
    /// Target coordinate system name.
    pub system_t: String,
    /// Semi-major axis of the source ellipsoid (metres).
    pub major_f: f64,
    /// Semi-minor axis of the source ellipsoid (metres).
    pub minor_f: f64,
    /// Semi-major axis of the target ellipsoid (metres).
    pub major_t: f64,
    /// Semi-minor axis of the target ellipsoid (metres).
    pub minor_t: f64,
}

/// A single sub-grid block, containing both header metadata and shift records.
#[derive(Debug, Clone)]
pub struct NtV2SubGrid {
    /// Sub-grid name (8-character ASCII, trimmed).
    pub name: String,
    /// Name of the parent sub-grid, or `"NONE"` for a root grid.
    pub parent: String,
    /// Southern boundary in arc-seconds.
    pub south_lat: f64,
    /// Northern boundary in arc-seconds.
    pub north_lat: f64,
    /// Eastern boundary in arc-seconds.
    pub east_lon: f64,
    /// Western boundary in arc-seconds.
    pub west_lon: f64,
    /// Grid spacing in latitude (arc-seconds).
    pub lat_inc: f64,
    /// Grid spacing in longitude (arc-seconds).
    pub lon_inc: f64,
    /// Total number of shift records (`GS_COUNT`).
    pub gs_count: u32,
    /// Shift and accuracy records, ordered south-to-north, west-to-east.
    pub records: Vec<NtV2Record>,
    /// Indices into [`NtV2Grid::sub_grids`] of sub-grids whose `parent`
    /// field names this sub-grid (populated after parsing all sub-grids).
    pub children: Vec<usize>,
}

impl NtV2SubGrid {
    /// Number of columns (longitude nodes) in the sub-grid.
    ///
    /// Computed as `floor((west_lon - east_lon) / lon_inc) + 1`.
    /// Note: in NTv2 the longitude field `W_LON` is the *western* (less
    /// negative / larger) longitude and `E_LON` is the *eastern* (more
    /// negative / smaller) longitude for files covering the western
    /// hemisphere.  The convention used in the file is that `W_LON > E_LON`
    /// for western-hemisphere grids.
    #[inline]
    pub fn num_cols(&self) -> usize {
        ((self.west_lon - self.east_lon) / self.lon_inc).round() as usize + 1
    }

    /// Number of rows (latitude nodes) in the sub-grid.
    #[inline]
    pub fn num_rows(&self) -> usize {
        ((self.north_lat - self.south_lat) / self.lat_inc).round() as usize + 1
    }

    /// Returns `true` if the point `(lon_sec, lat_sec)` (arc-seconds) lies
    /// within this sub-grid's bounding box (inclusive).
    #[inline]
    pub fn contains(&self, lon_sec: f64, lat_sec: f64) -> bool {
        lat_sec >= self.south_lat
            && lat_sec <= self.north_lat
            && lon_sec >= self.east_lon
            && lon_sec <= self.west_lon
    }

    /// Fetch the shift record at grid indices `(row, col)`.
    ///
    /// Records are stored row-major, south-to-north, west-to-east.
    pub fn record_at(&self, row: usize, col: usize) -> Option<&NtV2Record> {
        let idx = row * self.num_cols() + col;
        self.records.get(idx)
    }
}

/// A single NTv2 shift record: 4 × f32 = 16 bytes.
#[derive(Debug, Clone, Copy)]
pub struct NtV2Record {
    /// Latitude shift in arc-seconds (add to source latitude).
    pub lat_shift: f32,
    /// Longitude shift in arc-seconds (add to source longitude).
    pub lon_shift: f32,
    /// Accuracy indicator for the latitude shift (arc-seconds, 1σ).
    pub lat_accuracy: f32,
    /// Accuracy indicator for the longitude shift (arc-seconds, 1σ).
    pub lon_accuracy: f32,
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal parsing constants
// ─────────────────────────────────────────────────────────────────────────────

/// Size of a single OREC / SREC header record, or data record, in bytes.
pub const RECORD_BYTES: usize = 16;

/// Number of OREC records in the overview header.
const NUM_OREC_RECORDS: usize = 11;

/// Number of SREC records in each sub-grid header.
const NUM_SREC_RECORDS: usize = 11;

/// Magic LE bytes at offset 8 indicating little-endian byte order.
const LE_MAGIC: [u8; 4] = [0x0B, 0x00, 0x00, 0x00];

/// Magic BE bytes at offset 8 indicating big-endian byte order.
const BE_MAGIC: [u8; 4] = [0x00, 0x00, 0x00, 0x0B];

// ─────────────────────────────────────────────────────────────────────────────
// Endianness sentinel
// ─────────────────────────────────────────────────────────────────────────────

/// File byte-order sentinel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endian {
    Little,
    Big,
}

// ─────────────────────────────────────────────────────────────────────────────
// Raw record reader helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Extract the 8-byte key string from a raw 16-byte OREC/SREC record.
///
/// The key occupies the first 8 bytes and is zero-/space-padded ASCII.
#[inline]
fn record_key(rec: &[u8; 16]) -> &str {
    // Trim trailing spaces and nulls for robustness.
    let raw = &rec[0..8];
    let end = raw
        .iter()
        .rposition(|&b| b != 0 && b != b' ')
        .map(|i| i + 1)
        .unwrap_or(0);
    core::str::from_utf8(&raw[..end]).unwrap_or("")
}

/// Read the 4-byte integer value from record bytes 8–11 (key 4-byte pad = bytes 8-11 actual integer).
///
/// NTv2 layout per record (16 bytes total):
/// `[key:8][int_or_float_value:4][padding:4]`
#[inline]
fn record_u32(rec: &[u8; 16], endian: Endian) -> u32 {
    let b = &rec[8..12];
    match endian {
        Endian::Little => LittleEndian::read_u32(b),
        Endian::Big => BigEndian::read_u32(b),
    }
}

/// Read the 8-byte real8 double value from record bytes 8–15.
///
/// Per NTv2/NRCan Guidance Note 7-2 §4.4.4, the "real8" header fields
/// (MAJOR_F, MINOR_F, MAJOR_T, MINOR_T, S_LAT, N_LAT, E_LON, W_LON,
/// LAT_INC, LON_INC) occupy the *entire* 8-byte value field of the
/// record — unlike the 4-byte int/float fields (e.g. NUM_OREC,
/// GS_COUNT) which use only bytes 8-11 with bytes 12-15 reserved.
#[inline]
fn record_f64(rec: &[u8; 16], endian: Endian) -> f64 {
    let b = &rec[8..16];
    match endian {
        Endian::Little => LittleEndian::read_f64(b),
        Endian::Big => BigEndian::read_f64(b),
    }
}

/// Read the 8-byte string value from record bytes 8–15 (used for string records
/// where the value occupies bytes 8-15 instead of 8-11).
fn record_str8(rec: &[u8; 16]) -> String {
    let raw = &rec[8..16];
    let end = raw
        .iter()
        .rposition(|&b| b != 0 && b != b' ')
        .map(|i| i + 1)
        .unwrap_or(0);
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

/// Sniff the file's byte order from bytes 8–11.
fn sniff_endian(data: &[u8]) -> Result<Endian> {
    if data.len() < 12 {
        return Err(Error::Ntv2ParseError(
            "file too small to determine byte order (need ≥12 bytes)".into(),
        ));
    }
    let probe: [u8; 4] = [data[8], data[9], data[10], data[11]];
    if probe == LE_MAGIC {
        Ok(Endian::Little)
    } else if probe == BE_MAGIC {
        Ok(Endian::Big)
    } else {
        Err(Error::Ntv2ParseError(format!(
            "unrecognised byte-order signature at offset 8: {:02X?}",
            probe
        )))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Record block reader
// ─────────────────────────────────────────────────────────────────────────────

/// Advance the cursor by one 16-byte record and return it.
fn read_record(cursor: &mut Cursor<&[u8]>) -> Result<[u8; 16]> {
    let mut rec = [0u8; 16];
    cursor.read_exact(&mut rec).map_err(|e| {
        Error::Ntv2ParseError(format!("unexpected end of file while reading record: {e}"))
    })?;
    Ok(rec)
}

// ─────────────────────────────────────────────────────────────────────────────
// Overview header parser
// ─────────────────────────────────────────────────────────────────────────────

/// Parse the 11-record (176-byte) overview header.
fn parse_overview_header(cursor: &mut Cursor<&[u8]>, endian: Endian) -> Result<NtV2Header> {
    // We must read exactly NUM_OREC_RECORDS records.
    let mut num_file = 0u32;
    let mut gs_type = String::new();
    let mut version = String::new();
    let mut system_f = String::new();
    let mut system_t = String::new();
    let mut major_f = 0.0f64;
    let mut minor_f = 0.0f64;
    let mut major_t = 0.0f64;
    let mut minor_t = 0.0f64;

    for i in 0..NUM_OREC_RECORDS {
        let rec = read_record(cursor)?;
        let key = record_key(&rec);
        match key {
            "NUM_OREC" => {
                // Already know this is 11 from spec — validate
                let val = record_u32(&rec, endian);
                if val != NUM_OREC_RECORDS as u32 {
                    return Err(Error::Ntv2ParseError(format!(
                        "NUM_OREC mismatch: expected 11, got {val}"
                    )));
                }
            }
            "NUM_SREC" => {
                let val = record_u32(&rec, endian);
                if val != NUM_SREC_RECORDS as u32 {
                    return Err(Error::Ntv2ParseError(format!(
                        "NUM_SREC mismatch: expected 11, got {val}"
                    )));
                }
            }
            "NUM_FILE" => {
                num_file = record_u32(&rec, endian);
            }
            "GS_TYPE" => {
                gs_type = record_str8(&rec);
            }
            "VERSION" => {
                version = record_str8(&rec);
            }
            "SYSTEM_F" => {
                system_f = record_str8(&rec);
            }
            "SYSTEM_T" => {
                system_t = record_str8(&rec);
            }
            "MAJOR_F" => {
                // real8: full 8-byte IEEE double occupying bytes 8-15.
                major_f = record_f64(&rec, endian);
            }
            "MINOR_F" => {
                minor_f = record_f64(&rec, endian);
            }
            "MAJOR_T" => {
                major_t = record_f64(&rec, endian);
            }
            "MINOR_T" => {
                minor_t = record_f64(&rec, endian);
            }
            other => {
                // Unknown record — tolerate for forward compatibility
                let _ = (other, i);
            }
        }
    }

    if num_file == 0 {
        return Err(Error::Ntv2ParseError(
            "NUM_FILE is zero — no sub-grids present".into(),
        ));
    }

    Ok(NtV2Header {
        num_file,
        gs_type,
        version,
        system_f,
        system_t,
        major_f,
        minor_f,
        major_t,
        minor_t,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Sub-grid header parser
// ─────────────────────────────────────────────────────────────────────────────

/// Parse one sub-grid header block (11 SREC records = 176 bytes).
///
/// Does **not** read the data records — the caller must advance the cursor
/// by `gs_count * 16` bytes afterwards.
fn parse_sub_grid_header(cursor: &mut Cursor<&[u8]>, endian: Endian) -> Result<NtV2SubGrid> {
    let mut sub_name = String::new();
    let mut parent = String::new();
    let mut south_lat = 0.0f64;
    let mut north_lat = 0.0f64;
    let mut east_lon = 0.0f64;
    let mut west_lon = 0.0f64;
    let mut lat_inc = 0.0f64;
    let mut lon_inc = 0.0f64;
    let mut gs_count = 0u32;

    for _ in 0..NUM_SREC_RECORDS {
        let rec = read_record(cursor)?;
        let key = record_key(&rec);
        match key {
            "SUB_NAME" => {
                sub_name = record_str8(&rec);
            }
            "PARENT" => {
                parent = record_str8(&rec);
            }
            "CREATED" | "UPDATED" => {
                // Informational only — ignore values
            }
            "S_LAT" => {
                south_lat = record_f64(&rec, endian);
            }
            "N_LAT" => {
                north_lat = record_f64(&rec, endian);
            }
            "E_LON" => {
                east_lon = record_f64(&rec, endian);
            }
            "W_LON" => {
                west_lon = record_f64(&rec, endian);
            }
            "LAT_INC" => {
                lat_inc = record_f64(&rec, endian);
            }
            "LON_INC" => {
                lon_inc = record_f64(&rec, endian);
            }
            "GS_COUNT" => {
                gs_count = record_u32(&rec, endian);
            }
            _ => {
                // Tolerate unknown records
            }
        }
    }

    // Basic sanity checks
    if lat_inc <= 0.0 {
        return Err(Error::Ntv2ParseError(format!(
            "sub-grid '{sub_name}': LAT_INC ({lat_inc}) must be positive"
        )));
    }
    if lon_inc <= 0.0 {
        return Err(Error::Ntv2ParseError(format!(
            "sub-grid '{sub_name}': LON_INC ({lon_inc}) must be positive"
        )));
    }
    if north_lat <= south_lat {
        return Err(Error::Ntv2ParseError(format!(
            "sub-grid '{sub_name}': N_LAT ({north_lat}) must be > S_LAT ({south_lat})"
        )));
    }
    if west_lon <= east_lon {
        return Err(Error::Ntv2ParseError(format!(
            "sub-grid '{sub_name}': W_LON ({west_lon}) must be > E_LON ({east_lon})"
        )));
    }
    if gs_count == 0 {
        return Err(Error::Ntv2ParseError(format!(
            "sub-grid '{sub_name}': GS_COUNT is zero"
        )));
    }

    Ok(NtV2SubGrid {
        name: sub_name,
        parent,
        south_lat,
        north_lat,
        east_lon,
        west_lon,
        lat_inc,
        lon_inc,
        gs_count,
        records: Vec::new(), // filled by caller
        children: Vec::new(),
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Data record parser
// ─────────────────────────────────────────────────────────────────────────────

/// Read `count` shift records (each 16 bytes = 4 × f32) from the cursor.
fn parse_data_records(
    cursor: &mut Cursor<&[u8]>,
    count: u32,
    endian: Endian,
) -> Result<Vec<NtV2Record>> {
    let count = count as usize;
    let mut records = Vec::with_capacity(count);

    for _ in 0..count {
        let rec = read_record(cursor)?;
        // Each data record = 4 × f32 (16 bytes).
        let (lat_shift, lon_shift, lat_accuracy, lon_accuracy) = match endian {
            Endian::Little => {
                let ls = LittleEndian::read_f32(&rec[0..4]);
                let lo = LittleEndian::read_f32(&rec[4..8]);
                let la = LittleEndian::read_f32(&rec[8..12]);
                let loa = LittleEndian::read_f32(&rec[12..16]);
                (ls, lo, la, loa)
            }
            Endian::Big => {
                let ls = BigEndian::read_f32(&rec[0..4]);
                let lo = BigEndian::read_f32(&rec[4..8]);
                let la = BigEndian::read_f32(&rec[8..12]);
                let loa = BigEndian::read_f32(&rec[12..16]);
                (ls, lo, la, loa)
            }
        };
        records.push(NtV2Record {
            lat_shift,
            lon_shift,
            lat_accuracy,
            lon_accuracy,
        });
    }

    Ok(records)
}

// ─────────────────────────────────────────────────────────────────────────────
// Child-index builder
// ─────────────────────────────────────────────────────────────────────────────

/// After all sub-grids have been parsed, populate each sub-grid's `children`
/// vector with the indices of sub-grids that name it as their parent.
fn build_child_index(sub_grids: &mut [NtV2SubGrid]) {
    // Two-pass: first collect (child_idx, parent_name) pairs, then assign.
    let parent_names: Vec<String> = sub_grids.iter().map(|sg| sg.parent.clone()).collect();
    let grid_names: Vec<String> = sub_grids.iter().map(|sg| sg.name.clone()).collect();

    for (child_idx, parent_name) in parent_names.iter().enumerate() {
        if parent_name.eq_ignore_ascii_case("NONE") {
            continue;
        }
        if let Some(parent_idx) = grid_names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(parent_name))
        {
            sub_grids[parent_idx].children.push(child_idx);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// NtV2Grid implementation
// ─────────────────────────────────────────────────────────────────────────────

impl NtV2Grid {
    /// Parse a complete NTv2 `.gsb` file from a byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Ntv2ParseError`] if the data is malformed or truncated.
    pub fn from_bytes(data: &[u8]) -> Result<NtV2Grid> {
        let endian = sniff_endian(data)?;
        let mut cursor = Cursor::new(data);

        // Parse overview header
        let overview = parse_overview_header(&mut cursor, endian)?;

        // Parse each sub-grid
        let mut sub_grids: Vec<NtV2SubGrid> = Vec::with_capacity(overview.num_file as usize);

        for grid_idx in 0..overview.num_file as usize {
            let mut sub_grid = parse_sub_grid_header(&mut cursor, endian)?;

            // Validate expected record count against grid dimensions
            let expected_cols =
                ((sub_grid.west_lon - sub_grid.east_lon) / sub_grid.lon_inc).round() as u32 + 1;
            let expected_rows =
                ((sub_grid.north_lat - sub_grid.south_lat) / sub_grid.lat_inc).round() as u32 + 1;
            let expected_count = expected_rows * expected_cols;

            if sub_grid.gs_count != expected_count {
                return Err(Error::Ntv2ParseError(format!(
                    "sub-grid {} '{}': GS_COUNT={} does not match grid dimensions {}×{}={}",
                    grid_idx,
                    sub_grid.name,
                    sub_grid.gs_count,
                    expected_rows,
                    expected_cols,
                    expected_count
                )));
            }

            let records = parse_data_records(&mut cursor, sub_grid.gs_count, endian)?;
            sub_grid.records = records;
            sub_grids.push(sub_grid);
        }

        build_child_index(&mut sub_grids);

        Ok(NtV2Grid {
            overview,
            sub_grids,
        })
    }

    /// Apply the NTv2 grid shift to a geographic point.
    ///
    /// The algorithm:
    /// 1. Converts degrees to arc-seconds.
    /// 2. Selects the most specific sub-grid covering the point (deepest
    ///    child preferred over its parent, per the NTv2 specification).
    /// 3. Performs bilinear interpolation over the four surrounding nodes.
    /// 4. Returns `(lon_deg + Δlon/3600, lat_deg + Δlat/3600)`.
    ///
    /// # Parameters
    /// * `lon_deg` — source longitude in decimal degrees
    /// * `lat_deg` — source latitude in decimal degrees
    ///
    /// # Errors
    ///
    /// Returns [`Error::Ntv2OutOfGrid`] when no sub-grid covers the point.
    pub fn transform(&self, lon_deg: f64, lat_deg: f64) -> Result<(f64, f64)> {
        let lon_sec = lon_deg * 3600.0;
        let lat_sec = lat_deg * 3600.0;

        // Find the index of the deepest sub-grid containing the point.
        let sg_idx = self
            .find_best_sub_grid(lon_sec, lat_sec)
            .ok_or(Error::Ntv2OutOfGrid {
                lon: lon_deg,
                lat: lat_deg,
            })?;

        let sg = &self.sub_grids[sg_idx];
        let (lat_shift_sec, lon_shift_sec) = bilinear_interpolate(sg, lon_sec, lat_sec)?;

        Ok((
            lon_deg + f64::from(lon_shift_sec) / 3600.0,
            lat_deg + f64::from(lat_shift_sec) / 3600.0,
        ))
    }

    /// Recursively find the index of the most-specific sub-grid (deepest
    /// child) that contains `(lon_sec, lat_sec)`.
    ///
    /// The search starts from the root grids (`parent == "NONE"`) and
    /// descends into children.  If a child sub-grid contains the point we
    /// recurse into that child's children; otherwise we return the current
    /// grid's index.
    fn find_best_sub_grid(&self, lon_sec: f64, lat_sec: f64) -> Option<usize> {
        // Collect root (parentless) grids.
        let roots: Vec<usize> = self
            .sub_grids
            .iter()
            .enumerate()
            .filter(|(_, sg)| sg.parent.eq_ignore_ascii_case("NONE"))
            .map(|(i, _)| i)
            .collect();

        for root_idx in roots {
            if self.sub_grids[root_idx].contains(lon_sec, lat_sec) {
                return Some(self.descend_to_best(root_idx, lon_sec, lat_sec));
            }
        }
        None
    }

    /// Given that `current_idx` contains the point, recursively check
    /// children for a more specific match.
    fn descend_to_best(&self, current_idx: usize, lon_sec: f64, lat_sec: f64) -> usize {
        let children = self.sub_grids[current_idx].children.clone();
        for child_idx in children {
            if self.sub_grids[child_idx].contains(lon_sec, lat_sec) {
                return self.descend_to_best(child_idx, lon_sec, lat_sec);
            }
        }
        // No children contain the point — this is the best match.
        current_idx
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Bilinear interpolation
// ─────────────────────────────────────────────────────────────────────────────

/// Perform bilinear interpolation over the four surrounding grid nodes.
///
/// Returns `(lat_shift_arcsec, lon_shift_arcsec)` as `f32` values before
/// the caller converts to degrees.
///
/// # Grid layout
///
/// Records are ordered south→north, west→east.  Index into the flat array:
///
/// ```text
/// record_index = row * num_cols + col
/// ```
///
/// where `row = 0` is the southernmost row and `col = 0` is the westernmost
/// column.
fn bilinear_interpolate(sg: &NtV2SubGrid, lon_sec: f64, lat_sec: f64) -> Result<(f32, f32)> {
    // Normalised continuous grid coordinates (south-north, west-east).
    let i_cont = (lat_sec - sg.south_lat) / sg.lat_inc;
    let j_cont = (lon_sec - sg.east_lon) / sg.lon_inc;

    // Integer base indices (lower-left corner of the interpolation cell).
    let i0 = i_cont.floor() as usize;
    let j0 = j_cont.floor() as usize;

    let num_rows = sg.num_rows();
    let num_cols = sg.num_cols();

    if i0 + 1 >= num_rows || j0 + 1 >= num_cols {
        // Point is exactly on or beyond the upper/right edge: clamp or error.
        // If i_cont / j_cont is exactly at the maximum index, clamp.
        let i_clamped = i0.min(num_rows.saturating_sub(1));
        let j_clamped = j0.min(num_cols.saturating_sub(1));
        let rec = sg.record_at(i_clamped, j_clamped).ok_or_else(|| {
            Error::Ntv2ParseError(format!(
                "record index ({i_clamped}, {j_clamped}) out of range in sub-grid '{}'",
                sg.name
            ))
        })?;
        return Ok((rec.lat_shift, rec.lon_shift));
    }

    // Fractional parts (bilinear weights).
    let t = i_cont - i0 as f64; // fractional latitude
    let s = j_cont - j0 as f64; // fractional longitude

    // Retrieve the four surrounding records.
    let r00 = sg
        .record_at(i0, j0)
        .ok_or_else(|| Error::Ntv2ParseError(format!("missing record at ({i0}, {j0})")))?;
    let r10 = sg
        .record_at(i0 + 1, j0)
        .ok_or_else(|| Error::Ntv2ParseError(format!("missing record at ({}, {j0})", i0 + 1)))?;
    let r01 = sg
        .record_at(i0, j0 + 1)
        .ok_or_else(|| Error::Ntv2ParseError(format!("missing record at ({i0}, {})", j0 + 1)))?;
    let r11 = sg.record_at(i0 + 1, j0 + 1).ok_or_else(|| {
        Error::Ntv2ParseError(format!("missing record at ({}, {})", i0 + 1, j0 + 1))
    })?;

    // Bilinear formula: (1-t)(1-s)·r00 + t(1-s)·r10 + (1-t)s·r01 + ts·r11
    let w00 = (1.0 - t) * (1.0 - s);
    let w10 = t * (1.0 - s);
    let w01 = (1.0 - t) * s;
    let w11 = t * s;

    let lat_shift = (w00 * f64::from(r00.lat_shift)
        + w10 * f64::from(r10.lat_shift)
        + w01 * f64::from(r01.lat_shift)
        + w11 * f64::from(r11.lat_shift)) as f32;

    let lon_shift = (w00 * f64::from(r00.lon_shift)
        + w10 * f64::from(r10.lon_shift)
        + w01 * f64::from(r01.lon_shift)
        + w11 * f64::from(r11.lon_shift)) as f32;

    Ok((lat_shift, lon_shift))
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use byteorder::WriteBytesExt;

    // ─────────────────────────────────────────────────────────────────────────
    // Synthetic .gsb builder
    // ─────────────────────────────────────────────────────────────────────────

    /// Build a minimal valid NTv2 `.gsb` byte sequence for a 3×3 grid.
    ///
    /// Grid layout:
    /// - Latitude: from 60°00'00" to 60°02'00"
    ///   (south=216000", north=216120", inc=60" — 3 rows)
    /// - Longitude: from 10°00'00" to 10°02'00"
    ///   (east=36000", west=36120", inc=60" — 3 cols)
    /// - GS_COUNT:  9
    ///
    /// Each shift record has lat_shift = `row + col + 1` arcsec and
    /// lon_shift = `-(row + col + 1)` arcsec (as f32), accuracy = 0.5.
    fn build_synthetic_gsb(little_endian: bool) -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();

        // Helper closures that write correctly-endian integers/floats.
        macro_rules! write_u32 {
            ($v:expr) => {
                if little_endian {
                    buf.write_u32::<LittleEndian>($v).expect("write_u32 LE");
                } else {
                    buf.write_u32::<BigEndian>($v).expect("write_u32 BE");
                }
            };
        }
        macro_rules! write_f32 {
            ($v:expr) => {
                if little_endian {
                    buf.write_f32::<LittleEndian>($v).expect("write_f32 LE");
                } else {
                    buf.write_f32::<BigEndian>($v).expect("write_f32 BE");
                }
            };
        }
        // real8 header fields occupy the full 8-byte value slot (bytes 8-15),
        // unlike the 4-byte int/float fields which use bytes 8-11 + 4 pad bytes.
        macro_rules! write_f64 {
            ($v:expr) => {
                if little_endian {
                    buf.write_f64::<LittleEndian>($v).expect("write_f64 LE");
                } else {
                    buf.write_f64::<BigEndian>($v).expect("write_f64 BE");
                }
            };
        }

        /// Write an 8-byte padded ASCII key into `buf`.
        fn push_key(buf: &mut Vec<u8>, key: &str) {
            let mut bytes = [b' '; 8];
            let src = key.as_bytes();
            let len = src.len().min(8);
            bytes[..len].copy_from_slice(&src[..len]);
            buf.extend_from_slice(&bytes);
        }

        /// Write a 4-byte pad (zeros).
        fn push_pad4(buf: &mut Vec<u8>) {
            buf.extend_from_slice(&[0u8; 4]);
        }

        // ── Overview header (11 × 16-byte OREC records) ─────────────────────

        // Record 0: NUM_OREC = 11
        push_key(&mut buf, "NUM_OREC");
        if little_endian {
            buf.write_u32::<LittleEndian>(11).expect("orec0");
        } else {
            buf.write_u32::<BigEndian>(11).expect("orec0");
        }
        push_pad4(&mut buf);

        // Record 1: NUM_SREC = 11
        push_key(&mut buf, "NUM_SREC");
        if little_endian {
            buf.write_u32::<LittleEndian>(11).expect("srec0");
        } else {
            buf.write_u32::<BigEndian>(11).expect("srec0");
        }
        push_pad4(&mut buf);

        // Record 2: NUM_FILE = 1
        push_key(&mut buf, "NUM_FILE");
        if little_endian {
            buf.write_u32::<LittleEndian>(1).expect("nf");
        } else {
            buf.write_u32::<BigEndian>(1).expect("nf");
        }
        push_pad4(&mut buf);

        // Record 3: GS_TYPE = "SECONDS "
        push_key(&mut buf, "GS_TYPE");
        buf.extend_from_slice(b"SECONDS "); // 8 bytes value

        // Record 4: VERSION = "NTv2.0  "
        push_key(&mut buf, "VERSION");
        buf.extend_from_slice(b"NTv2.0  ");

        // Record 5: SYSTEM_F = "NAD27   "
        push_key(&mut buf, "SYSTEM_F");
        buf.extend_from_slice(b"NAD27   ");

        // Record 6: SYSTEM_T = "NAD83   "
        push_key(&mut buf, "SYSTEM_T");
        buf.extend_from_slice(b"NAD83   ");

        // Record 7: MAJOR_F = Clarke 1866 a (6378206.4 m)
        push_key(&mut buf, "MAJOR_F");
        write_f64!(6_378_206.4_f64);

        // Record 8: MINOR_F = Clarke 1866 b (6356583.8 m)
        push_key(&mut buf, "MINOR_F");
        write_f64!(6_356_583.8_f64);

        // Record 9: MAJOR_T = GRS80 a (6378137.0 m)
        push_key(&mut buf, "MAJOR_T");
        write_f64!(6_378_137.0_f64);

        // Record 10: MINOR_T = GRS80 b (6356752.31414 m)
        push_key(&mut buf, "MINOR_T");
        write_f64!(6_356_752.314_14_f64);

        // ── Sub-grid header (11 × 16-byte SREC records) ─────────────────────

        // S_LAT = 216000" = 60° × 3600
        let s_lat = 216_000.0_f64;
        // N_LAT = 216120" = 60°02'
        let n_lat = 216_120.0_f64;
        // E_LON = 36000" = 10°
        let e_lon = 36_000.0_f64;
        // W_LON = 36120" = 10°02'
        let w_lon = 36_120.0_f64;
        // LAT_INC = LON_INC = 60"
        let lat_inc = 60.0_f64;
        let lon_inc = 60.0_f64;
        // GS_COUNT = 3×3 = 9
        let gs_count = 9u32;

        // SREC 0: SUB_NAME
        push_key(&mut buf, "SUB_NAME");
        buf.extend_from_slice(b"GRID_1  ");

        // SREC 1: PARENT = "NONE"
        push_key(&mut buf, "PARENT");
        buf.extend_from_slice(b"NONE    ");

        // SREC 2: CREATED
        push_key(&mut buf, "CREATED");
        buf.extend_from_slice(b"20240101");

        // SREC 3: UPDATED
        push_key(&mut buf, "UPDATED");
        buf.extend_from_slice(b"20240101");

        // SREC 4: S_LAT
        push_key(&mut buf, "S_LAT");
        write_f64!(s_lat);

        // SREC 5: N_LAT
        push_key(&mut buf, "N_LAT");
        write_f64!(n_lat);

        // SREC 6: E_LON
        push_key(&mut buf, "E_LON");
        write_f64!(e_lon);

        // SREC 7: W_LON
        push_key(&mut buf, "W_LON");
        write_f64!(w_lon);

        // SREC 8: LAT_INC
        push_key(&mut buf, "LAT_INC");
        write_f64!(lat_inc);

        // SREC 9: LON_INC
        push_key(&mut buf, "LON_INC");
        write_f64!(lon_inc);

        // SREC 10: GS_COUNT
        push_key(&mut buf, "GS_COUNT");
        write_u32!(gs_count);
        push_pad4(&mut buf);

        // ── Data records (9 × 16 bytes) ──────────────────────────────────────
        // Order: south→north (row 0..2), west→east (col 0..2)
        for row in 0..3usize {
            for col in 0..3usize {
                let shift_val = (row + col + 1) as f32;
                write_f32!(shift_val); // lat_shift (arcsec)
                write_f32!(-shift_val); // lon_shift (arcsec)
                write_f32!(0.5_f32); // lat_accuracy
                write_f32!(0.5_f32); // lon_accuracy
            }
        }

        buf
    }

    /// Build a two-sub-grid NTv2 file for testing nested-grid preference.
    ///
    /// Sub-grid 0 (root, 3×3):
    /// - S_LAT=216000", N_LAT=216120", E_LON=36000", W_LON=36120"
    /// - all shifts = 1.0"
    ///
    /// Sub-grid 1 (child of sub-grid 0, 3×3):
    /// - S_LAT=216000", N_LAT=216120", E_LON=36000", W_LON=36120"
    ///   (same extent but named child — in reality, a child would cover a
    ///   smaller area; for the purpose of this test we name it as child and
    ///   give it different shift values)
    /// - all shifts = 5.0"
    fn build_nested_gsb() -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();

        fn push_key(buf: &mut Vec<u8>, key: &str) {
            let mut bytes = [b' '; 8];
            let src = key.as_bytes();
            let len = src.len().min(8);
            bytes[..len].copy_from_slice(&src[..len]);
            buf.extend_from_slice(&bytes);
        }
        fn push_pad4(buf: &mut Vec<u8>) {
            buf.extend_from_slice(&[0u8; 4]);
        }

        // Overview header — NUM_FILE = 2
        let records_orec: &[(&str, &[u8])] = &[
            ("NUM_OREC", &[11u8, 0, 0, 0, 0, 0, 0, 0]),
            ("NUM_SREC", &[11u8, 0, 0, 0, 0, 0, 0, 0]),
            ("NUM_FILE", &[2u8, 0, 0, 0, 0, 0, 0, 0]),
            ("GS_TYPE", b"SECONDS "),
            ("VERSION", b"NTv2.0  "),
            ("SYSTEM_F", b"NAD27   "),
            ("SYSTEM_T", b"NAD83   "),
            // real8 zero (8 zero bytes) is bit-identical to f64 0.0, so these
            // placeholder ellipsoid fields need no change for the real8 fix.
            ("MAJOR_F", &[0u8; 8]),
            ("MINOR_F", &[0u8; 8]),
            ("MAJOR_T", &[0u8; 8]),
            ("MINOR_T", &[0u8; 8]),
        ];

        // Write overview header with LE u32 for integer fields
        for (key, val) in records_orec.iter() {
            push_key(&mut buf, key);
            buf.extend_from_slice(val);
        }

        // Sub-grid 0 header (PARENT = "NONE")
        let write_sub_header = |buf: &mut Vec<u8>, name: &[u8; 8], parent: &[u8; 8]| {
            push_key(buf, "SUB_NAME");
            buf.extend_from_slice(name);
            push_key(buf, "PARENT");
            buf.extend_from_slice(parent);
            push_key(buf, "CREATED");
            buf.extend_from_slice(b"20240101");
            push_key(buf, "UPDATED");
            buf.extend_from_slice(b"20240101");
            // S_LAT (real8: full 8-byte value slot, no separate pad)
            push_key(buf, "S_LAT");
            buf.write_f64::<LittleEndian>(216_000.0).expect("f64");
            // N_LAT
            push_key(buf, "N_LAT");
            buf.write_f64::<LittleEndian>(216_120.0).expect("f64");
            // E_LON
            push_key(buf, "E_LON");
            buf.write_f64::<LittleEndian>(36_000.0).expect("f64");
            // W_LON
            push_key(buf, "W_LON");
            buf.write_f64::<LittleEndian>(36_120.0).expect("f64");
            // LAT_INC
            push_key(buf, "LAT_INC");
            buf.write_f64::<LittleEndian>(60.0).expect("f64");
            // LON_INC
            push_key(buf, "LON_INC");
            buf.write_f64::<LittleEndian>(60.0).expect("f64");
            // GS_COUNT
            push_key(buf, "GS_COUNT");
            buf.write_u32::<LittleEndian>(9).expect("u32");
            push_pad4(buf);
        };

        write_sub_header(&mut buf, b"GRID_0  ", b"NONE    ");

        // Data records for sub-grid 0 — all shifts = 1.0"
        for _ in 0..9 {
            buf.write_f32::<LittleEndian>(1.0).expect("f32"); // lat_shift
            buf.write_f32::<LittleEndian>(1.0).expect("f32"); // lon_shift
            buf.write_f32::<LittleEndian>(0.5).expect("f32"); // lat_accuracy
            buf.write_f32::<LittleEndian>(0.5).expect("f32"); // lon_accuracy
        }

        write_sub_header(&mut buf, b"GRID_1  ", b"GRID_0  ");

        // Data records for sub-grid 1 (child) — all shifts = 5.0"
        for _ in 0..9 {
            buf.write_f32::<LittleEndian>(5.0).expect("f32"); // lat_shift
            buf.write_f32::<LittleEndian>(5.0).expect("f32"); // lon_shift
            buf.write_f32::<LittleEndian>(0.1).expect("f32"); // lat_accuracy
            buf.write_f32::<LittleEndian>(0.1).expect("f32"); // lon_accuracy
        }

        buf
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test: LE endianness detection and header parsing
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_ntv2_parse_header_endianness_detection_le() {
        let data = build_synthetic_gsb(true);
        let grid = NtV2Grid::from_bytes(&data).expect("parse LE gsb");
        assert_eq!(grid.overview.num_file, 1, "LE: num_file should be 1");
        assert_eq!(grid.sub_grids.len(), 1, "LE: should have 1 sub-grid");
        assert_eq!(grid.sub_grids[0].gs_count, 9, "LE: 3×3 = 9 records");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test: BE endianness detection and header parsing
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_ntv2_parse_header_endianness_detection_be() {
        let data = build_synthetic_gsb(false);
        let grid = NtV2Grid::from_bytes(&data).expect("parse BE gsb");
        assert_eq!(grid.overview.num_file, 1, "BE: num_file should be 1");
        assert_eq!(grid.sub_grids.len(), 1, "BE: should have 1 sub-grid");
        assert_eq!(grid.sub_grids[0].gs_count, 9, "BE: 3×3 = 9 records");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test: bilinear interpolation at an exact node returns that node's shift
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_ntv2_bilinear_interpolation_at_node_returns_shift_exactly() {
        let data = build_synthetic_gsb(true);
        let grid = NtV2Grid::from_bytes(&data).expect("parse");

        // The grid covers:
        //   lat: 216000" to 216120" (60°00' to 60°02'), inc=60"
        //   lon: 36000" to 36120" (10°00' to 10°02'), inc=60"
        //
        // Node (row=0, col=0) shift = (row+col+1) = 1.0"
        // lat_deg = 216000 / 3600 = 60.0, lon_deg = 36000 / 3600 = 10.0
        let (lon_out, lat_out) = grid.transform(10.0, 60.0).expect("transform node 0,0");

        // Expected: lat shifted by +1.0", lon shifted by -1.0"
        let expected_lat = 60.0 + 1.0 / 3600.0;
        let expected_lon = 10.0 + (-1.0) / 3600.0;

        assert!(
            (lat_out - expected_lat).abs() < 1e-5,
            "lat_out={lat_out} expected={expected_lat}"
        );
        assert!(
            (lon_out - expected_lon).abs() < 1e-5,
            "lon_out={lon_out} expected={expected_lon}"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test: bilinear interpolation at cell centre averages four neighbours
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_ntv2_bilinear_interpolation_at_center_averages_four_neighbours() {
        // Build a grid where all 9 nodes have the same lat_shift = 2.0" and
        // lon_shift = -2.0" (so any interpolation returns the same value).
        let mut data = build_synthetic_gsb(true);

        // Find the start of the data section and overwrite with uniform shifts.
        // Overview: 176 bytes, Sub-grid header: 176 bytes, Data: 9 × 16 bytes.
        let data_offset = 176 + 176;
        for i in 0..9usize {
            let off = data_offset + i * 16;
            LittleEndian::write_f32(&mut data[off..off + 4], 2.0); // lat_shift
            LittleEndian::write_f32(&mut data[off + 4..off + 8], -2.0); // lon_shift
            LittleEndian::write_f32(&mut data[off + 8..off + 12], 0.5); // lat_acc
            LittleEndian::write_f32(&mut data[off + 12..off + 16], 0.5); // lon_acc
        }

        let grid = NtV2Grid::from_bytes(&data).expect("parse uniform grid");

        // Centre of cell (0,0)→(1,1):
        // lat = 216000" + 30" = 216030" → 216030/3600 = 60.008333...°
        // lon = 36000" + 30"  = 36030"  → 36030/3600  = 10.008333...°
        let centre_lat = 216_030.0 / 3600.0;
        let centre_lon = 36_030.0 / 3600.0;

        let (lon_out, lat_out) = grid.transform(centre_lon, centre_lat).expect("centre");

        let expected_lat = centre_lat + 2.0 / 3600.0;
        let expected_lon = centre_lon + (-2.0) / 3600.0;

        assert!(
            (lat_out - expected_lat).abs() < 1e-5,
            "centre lat: got {lat_out}, expected {expected_lat}"
        );
        assert!(
            (lon_out - expected_lon).abs() < 1e-5,
            "centre lon: got {lon_out}, expected {expected_lon}"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test: point outside grid extent errors with Ntv2OutOfGrid
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_ntv2_outside_grid_extent_errors() {
        let data = build_synthetic_gsb(true);
        let grid = NtV2Grid::from_bytes(&data).expect("parse");

        // Point outside: lat=0°, lon=0° (grid covers 60°–60°02', 10°–10°02')
        let result = grid.transform(0.0, 0.0);
        assert!(result.is_err(), "should error for point outside grid");

        let err = result.expect_err("expected error");
        assert!(
            matches!(err, Error::Ntv2OutOfGrid { .. }),
            "expected Ntv2OutOfGrid, got: {err:?}"
        );
        if let Error::Ntv2OutOfGrid { lon, lat } = err {
            assert!((lon - 0.0).abs() < 1e-10, "lon={lon}");
            assert!((lat - 0.0).abs() < 1e-10, "lat={lat}");
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test: nested grid — child is preferred over parent
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_ntv2_nested_grid_child_preferred_when_parent_set() {
        let data = build_nested_gsb();
        let grid = NtV2Grid::from_bytes(&data).expect("parse nested gsb");

        assert_eq!(grid.sub_grids.len(), 2, "two sub-grids");

        // Check that GRID_0 has GRID_1 as a child
        let root = &grid.sub_grids[0];
        assert_eq!(root.name, "GRID_0", "root name");
        assert_eq!(
            root.children.len(),
            1,
            "root should have 1 child, got {:?}",
            root.children
        );

        let child = &grid.sub_grids[1];
        assert_eq!(child.name, "GRID_1", "child name");
        assert_eq!(child.parent, "GRID_0", "child parent name");

        // A point inside the child's extent (which equals the parent's extent)
        // should use the child's shift (5.0") not the parent's (1.0")
        let lat_deg = 216_000.0_f64 / 3600.0; // south edge = 60°
        let lon_deg = 36_000.0_f64 / 3600.0; // west edge = 10°

        let (lon_out, lat_out) = grid.transform(lon_deg, lat_deg).expect("nested transform");

        // Child shift = 5.0" = 5/3600 degrees
        let expected_lat = lat_deg + 5.0 / 3600.0;
        let expected_lon = lon_deg + 5.0 / 3600.0;

        assert!(
            (lat_out - expected_lat).abs() < 1e-5,
            "nested lat: got {lat_out}, expected {expected_lat}"
        );
        assert!(
            (lon_out - expected_lon).abs() < 1e-5,
            "nested lon: got {lon_out}, expected {expected_lon}"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Additional sanity checks
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_ntv2_sub_grid_geometry() {
        let data = build_synthetic_gsb(true);
        let grid = NtV2Grid::from_bytes(&data).expect("parse");
        let sg = &grid.sub_grids[0];

        assert_eq!(sg.num_rows(), 3, "3 latitude nodes");
        assert_eq!(sg.num_cols(), 3, "3 longitude nodes");
        assert!(sg.contains(36_060.0, 216_060.0), "centre point in grid");
        assert!(!sg.contains(0.0, 0.0), "far point outside grid");
    }

    #[test]
    fn test_ntv2_overview_fields() {
        let data = build_synthetic_gsb(true);
        let grid = NtV2Grid::from_bytes(&data).expect("parse");
        let hdr = &grid.overview;
        assert_eq!(hdr.num_file, 1);
        assert_eq!(hdr.gs_type, "SECONDS");
        assert_eq!(hdr.version, "NTv2.0");
        assert_eq!(hdr.system_f, "NAD27");
        assert_eq!(hdr.system_t, "NAD83");
        assert!(hdr.major_f > 6_000_000.0, "major_f plausible");
        assert!(hdr.major_t > 6_000_000.0, "major_t plausible");
    }

    #[test]
    fn test_ntv2_top_right_node_exact() {
        let data = build_synthetic_gsb(true);
        let grid = NtV2Grid::from_bytes(&data).expect("parse");

        // Node (row=2, col=2): shift = 2+2+1 = 5.0"
        let lat_deg = 216_120.0 / 3600.0; // N_LAT = 60°02'
        let lon_deg = 36_120.0 / 3600.0; // W_LON = 10°02'

        let (lon_out, lat_out) = grid.transform(lon_deg, lat_deg).expect("top-right node");

        let expected_lat = lat_deg + 5.0 / 3600.0;
        let expected_lon = lon_deg + (-5.0) / 3600.0;

        assert!(
            (lat_out - expected_lat).abs() < 1e-4,
            "top-right lat: {lat_out} vs {expected_lat}"
        );
        assert!(
            (lon_out - expected_lon).abs() < 1e-4,
            "top-right lon: {lon_out} vs {expected_lon}"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Regression: record_f64 must read the full 8-byte real8 value field,
    // not a 4-byte float truncated to bytes 8-11.
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_record_f64_reads_full_8_byte_real8_le() {
        // Byte-for-byte real8 record per NRCan Guidance Note 7-2 §4.4.4:
        // [key:8][real8 double:8]. Use a value whose f32-vs-f64 encodings
        // differ measurably so a truncated 4-byte read would fail the
        // exact-match assertion below.
        let value = 6_378_137.0_f64; // GRS80 semi-major axis (exact in f64)
        let mut rec = [0u8; 16];
        rec[0..8].copy_from_slice(b"MAJOR_T ");
        LittleEndian::write_f64(&mut rec[8..16], value);

        let got = record_f64(&rec, Endian::Little);
        assert_eq!(got, value, "record_f64 must recover the exact real8 double");
    }

    #[test]
    fn test_record_f64_reads_full_8_byte_real8_be() {
        let value = 6_356_752.314_140_356_f64; // GRS80 semi-minor axis
        let mut rec = [0u8; 16];
        rec[0..8].copy_from_slice(b"MINOR_T ");
        BigEndian::write_f64(&mut rec[8..16], value);

        let got = record_f64(&rec, Endian::Big);
        assert_eq!(got, value, "record_f64 must recover the exact real8 double");
    }

    #[test]
    fn test_record_f64_distinguishes_from_truncated_f32_read() {
        // A value that round-trips losslessly through f64 but loses
        // precision through f32 — proves the reader is not silently
        // discarding bytes 12-15.
        let value = 216_030.123_456_789_f64;
        let mut rec = [0u8; 16];
        rec[0..8].copy_from_slice(b"S_LAT   ");
        LittleEndian::write_f64(&mut rec[8..16], value);

        let got = record_f64(&rec, Endian::Little);
        assert_eq!(got, value);
        // Sanity: the buggy (f32-truncating) implementation would have
        // produced a different value here because f32 cannot represent
        // this many significant digits.
        assert_ne!(got, f64::from(value as f32));
    }

    #[test]
    fn test_ntv2_overview_header_ellipsoid_values_exact() {
        // Confirms the overview header parser recovers the exact real8
        // ellipsoid axis values written by build_synthetic_gsb, not values
        // corrupted by a 4-byte-float truncation.
        let data = build_synthetic_gsb(true);
        let grid = NtV2Grid::from_bytes(&data).expect("parse");
        let hdr = &grid.overview;

        assert!(
            (hdr.major_f - 6_378_206.4).abs() < 1e-6,
            "major_f={} expected 6378206.4",
            hdr.major_f
        );
        assert!(
            (hdr.minor_f - 6_356_583.8).abs() < 1e-6,
            "minor_f={} expected 6356583.8",
            hdr.minor_f
        );
        assert!(
            (hdr.major_t - 6_378_137.0).abs() < 1e-6,
            "major_t={} expected 6378137.0",
            hdr.major_t
        );
    }
}
