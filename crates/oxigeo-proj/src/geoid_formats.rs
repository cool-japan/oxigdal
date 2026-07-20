//! Real-world EGM geoid **distribution-format** parsers.
//!
//! [`crate::geoid::load_egm_grid`] is a generic NGA-style little-endian `f32`
//! reader that requires the caller to supply the full grid geometry
//! (`lat_min`, `lon_min`, steps, and dimensions) out-of-band.  Real EGM
//! distributions instead ship that metadata *inside* the file.  This module
//! adds parsers for the two most common shipped containers and builds a
//! [`GeoidGrid`] consistent with the conventions documented on that type:
//!
//! * **EGM96 ASCII** — the public `WW15MGH.GRD` 15′×15′ grid.  The first
//!   non-empty line is a header of six numbers
//!   `south_lat north_lat west_lon east_lon dlat dlon`; the remaining
//!   whitespace-separated tokens are undulation values (metres) in row-major
//!   order.  See [`parse_egm96_ascii_str`].
//!
//! * **EGM2008 2.5′ binary** — a self-consistent little-endian container that
//!   *this crate controls*: the first 8 bytes are two little-endian `u32`
//!   values (`n_lat`, `n_lon`), followed by `n_lat * n_lon` little-endian
//!   `f32` undulation records in row-major order.  The canonical 2.5′ grid has
//!   `n_lat = 4321`, `n_lon = 8640`.
//!
//!   > **Note.** The *actual* NGA distribution file
//!   > (`Und_min2.5x2.5_egm2008_isw=82_WGS84_TideFree`) is a raw Fortran
//!   > record stream framed by 4-byte record-length markers and carries no
//!   > inline dimension header.  Re-deriving those dimensions reliably is
//!   > out of scope here, so this module defines the explicit 8-byte
//!   > dimension header above as a stable, self-describing container we can
//!   > round-trip without ambiguity.  See [`parse_egm2008_binary_25`].
//!
//! All [`GeoidGrid`]s produced here follow the crate-wide convention used by
//! [`crate::geoid::synthetic_grid`]: rows run **south-to-north** (`lat_step >
//! 0`, row 0 at `lat_min`), columns run **west-to-east** (column 0 at
//! `lon_min`), heights are stored row-major in metres, and the longitude
//! origin is `-180.0°` so that [`GeoidGrid::geoid_height_m`]'s modulo-360°
//! wrap behaves identically for every grid in the crate.
//!
//! Every parser validates exhaustively and returns a descriptive
//! [`Error::GeoidFileFormat`] on any malformed input — these functions never
//! panic on bad data.
//!
//! # Examples
//!
//! ```
//! use oxigeo_proj::geoid_formats::parse_egm96_ascii_str;
//!
//! // Tiny 3×3 grid: header is south north west east dlat dlon.
//! let text = "\
//! -1.0 1.0 -1.0 1.0 1.0 1.0
//! 0 1 2
//! 3 4 5
//! 6 7 8
//! ";
//! let grid = parse_egm96_ascii_str(text).expect("valid grid");
//! assert_eq!(grid.n_lat, 3);
//! assert_eq!(grid.n_lon, 3);
//! ```

use std::path::Path;

use crate::error::Error;
use crate::geoid::{GeoidGrid, GeoidModel};

// ---------------------------------------------------------------------------
// Shared constants and conventions
// ---------------------------------------------------------------------------

/// Latitude origin (degrees) assigned to every grid produced by this module.
///
/// Matches [`crate::geoid::synthetic_grid`]; row 0 corresponds to the South
/// Pole, with `lat_step > 0` running northward.
const GRID_LAT_MIN_DEG: f64 = -90.0;

/// Longitude origin (degrees) assigned to every grid produced by this module.
///
/// Matches [`crate::geoid::synthetic_grid`]; [`GeoidGrid::geoid_height_m`]
/// wraps longitude modulo 360° relative to this origin, so a `0..360`-indexed
/// distribution and a `-180..180`-indexed one resolve identically for global
/// coverage.
const GRID_LON_MIN_DEG: f64 = -180.0;

/// Upper sanity bound on either grid dimension (rejects absurd headers that
/// would otherwise demand multi-terabyte allocations).
const MAX_GRID_DIM: u32 = 100_000;

/// Number of bytes in the EGM2008 binary dimension header (two `u32`).
const EGM2008_HEADER_LEN: usize = 8;

// ===========================================================================
// EGM96 ASCII (`WW15MGH.GRD`)
// ===========================================================================

/// Parsed geometry from the leading header line of an EGM96 ASCII grid.
///
/// The `WW15MGH.GRD` header line carries six numbers in the order
/// `south_lat north_lat west_lon east_lon dlat dlon`.  Node counts are derived
/// inclusively from the bounds and steps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Egm96AsciiHeader {
    /// Southernmost latitude (degrees).
    pub lat_min: f64,
    /// Northernmost latitude (degrees).
    pub lat_max: f64,
    /// Western longitude (degrees).
    pub lon_min: f64,
    /// Eastern longitude (degrees).
    pub lon_max: f64,
    /// Latitude step between adjacent rows (degrees, positive).
    pub lat_step: f64,
    /// Longitude step between adjacent columns (degrees, positive).
    pub lon_step: f64,
    /// Number of latitude rows (`(lat_max-lat_min)/lat_step` rounded + 1).
    pub n_lat: usize,
    /// Number of longitude columns (`(lon_max-lon_min)/lon_step` rounded + 1).
    pub n_lon: usize,
}

/// Parses the canonical EGM96 ASCII header line.
///
/// The line must contain (at least) six whitespace-separated numbers in the
/// order `south_lat north_lat west_lon east_lon dlat dlon`.  Leading/trailing
/// whitespace and runs of multiple spaces between tokens are tolerated; only
/// the first six tokens are consumed (a real `WW15MGH.GRD` header line has
/// exactly six).
///
/// Node counts are derived inclusively:
/// `n_lat = round((lat_max - lat_min) / lat_step) + 1`, and `n_lon`
/// analogously.
///
/// # Errors
///
/// Returns [`Error::GeoidFileFormat`] when the line has fewer than six tokens,
/// when any token is not a finite number, when either step is non-positive,
/// when the bounds are inverted (`max < min`), or when the derived node count
/// would be zero or exceed `MAX_GRID_DIM`.
pub fn parse_egm96_ascii_header(s: &str) -> Result<Egm96AsciiHeader, Error> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(Error::GeoidFileFormat(
            "EGM96 ASCII header line is empty".to_string(),
        ));
    }

    let mut values: [f64; 6] = [0.0; 6];
    let mut tokens = trimmed.split_whitespace();
    for (slot, label) in values.iter_mut().zip([
        "south_lat",
        "north_lat",
        "west_lon",
        "east_lon",
        "dlat",
        "dlon",
    ]) {
        let tok = tokens.next().ok_or_else(|| {
            Error::GeoidFileFormat(format!(
                "EGM96 ASCII header line is truncated: expected 6 numbers \
                 (south_lat north_lat west_lon east_lon dlat dlon), missing '{label}' in {trimmed:?}"
            ))
        })?;
        let parsed: f64 = tok.parse().map_err(|_| {
            Error::GeoidFileFormat(format!(
                "EGM96 ASCII header field '{label}' is not a valid number: {tok:?}"
            ))
        })?;
        if !parsed.is_finite() {
            return Err(Error::GeoidFileFormat(format!(
                "EGM96 ASCII header field '{label}' is not finite: {tok:?}"
            )));
        }
        *slot = parsed;
    }

    let [lat_min, lat_max, lon_min, lon_max, lat_step, lon_step] = values;

    if lat_step <= 0.0 {
        return Err(Error::GeoidFileFormat(format!(
            "EGM96 ASCII header latitude step must be positive, got {lat_step}"
        )));
    }
    if lon_step <= 0.0 {
        return Err(Error::GeoidFileFormat(format!(
            "EGM96 ASCII header longitude step must be positive, got {lon_step}"
        )));
    }
    if lat_max < lat_min {
        return Err(Error::GeoidFileFormat(format!(
            "EGM96 ASCII header latitude bounds inverted: north {lat_max} < south {lat_min}"
        )));
    }
    if lon_max < lon_min {
        return Err(Error::GeoidFileFormat(format!(
            "EGM96 ASCII header longitude bounds inverted: east {lon_max} < west {lon_min}"
        )));
    }

    let n_lat = derive_node_count(lat_min, lat_max, lat_step, "latitude")?;
    let n_lon = derive_node_count(lon_min, lon_max, lon_step, "longitude")?;

    Ok(Egm96AsciiHeader {
        lat_min,
        lat_max,
        lon_min,
        lon_max,
        lat_step,
        lon_step,
        n_lat,
        n_lon,
    })
}

/// Derives an inclusive node count `round((max - min) / step) + 1`, validating
/// the result against [`MAX_GRID_DIM`].
fn derive_node_count(min: f64, max: f64, step: f64, axis: &str) -> Result<usize, Error> {
    let span_steps = ((max - min) / step).round();
    if !span_steps.is_finite() || span_steps < 0.0 {
        return Err(Error::GeoidFileFormat(format!(
            "EGM96 ASCII header {axis} span is invalid (min={min}, max={max}, step={step})"
        )));
    }
    let count = span_steps as u64 + 1;
    if count == 0 || count > MAX_GRID_DIM as u64 {
        return Err(Error::GeoidFileFormat(format!(
            "EGM96 ASCII header {axis} node count {count} is out of range (1..={MAX_GRID_DIM})"
        )));
    }
    Ok(count as usize)
}

/// Parses a complete EGM96 ASCII grid from an in-memory string.
///
/// The first non-empty line is treated as the header (see
/// [`parse_egm96_ascii_header`]); every remaining whitespace-separated token
/// (across all subsequent lines) is parsed as an `f64` undulation value in
/// metres, row-major (row 0 = `lat_min`, column 0 = `lon_min`).  The total
/// number of body tokens must equal exactly `n_lat * n_lon`.
///
/// The returned [`GeoidGrid`] uses [`GeoidModel::Egm96`], a `-180.0°`
/// longitude origin, and `lat_min = -90.0°`-style south-to-north row ordering
/// inherited from the header bounds (i.e. `lat_min` from the header is used
/// verbatim as the row-0 latitude).
///
/// # Errors
///
/// Returns [`Error::GeoidFileFormat`] when the input is empty, when the header
/// is malformed, when a body token is not a finite number, or when the body
/// token count does not match the header-declared grid size.
pub fn parse_egm96_ascii_str(s: &str) -> Result<GeoidGrid, Error> {
    let mut lines = s.lines();

    // First non-empty (after trimming) line is the header.
    let header_line = loop {
        match lines.next() {
            Some(line) if !line.trim().is_empty() => break line,
            Some(_) => continue,
            None => {
                return Err(Error::GeoidFileFormat(
                    "EGM96 ASCII input contains no header line".to_string(),
                ));
            }
        }
    };

    let header = parse_egm96_ascii_header(header_line)?;
    let expected = header.n_lat.checked_mul(header.n_lon).ok_or_else(|| {
        Error::GeoidFileFormat(format!(
            "EGM96 ASCII grid size overflow: {} × {}",
            header.n_lat, header.n_lon
        ))
    })?;

    let mut heights_m: Vec<f32> = Vec::with_capacity(expected);
    for line in lines {
        for tok in line.split_whitespace() {
            let value: f64 = tok.parse().map_err(|_| {
                Error::GeoidFileFormat(format!(
                    "EGM96 ASCII body contains a non-numeric undulation value: {tok:?}"
                ))
            })?;
            if !value.is_finite() {
                return Err(Error::GeoidFileFormat(format!(
                    "EGM96 ASCII body undulation value is not finite: {tok:?}"
                )));
            }
            // Guard against runaway allocation before pushing past the
            // declared size; report the mismatch with the running count.
            if heights_m.len() == expected {
                return Err(Error::GeoidFileFormat(format!(
                    "EGM96 ASCII body has more values than the header declares \
                     ({} expected for {} × {})",
                    expected, header.n_lat, header.n_lon
                )));
            }
            heights_m.push(value as f32);
        }
    }

    if heights_m.len() != expected {
        return Err(Error::GeoidFileFormat(format!(
            "EGM96 ASCII body has {} undulation values but the header declares \
             {} ({} rows × {} cols)",
            heights_m.len(),
            expected,
            header.n_lat,
            header.n_lon
        )));
    }

    Ok(GeoidGrid {
        model: GeoidModel::Egm96,
        lat_step_deg: header.lat_step,
        lon_step_deg: header.lon_step,
        lat_min_deg: header.lat_min,
        lon_min_deg: header.lon_min,
        n_lat: header.n_lat,
        n_lon: header.n_lon,
        heights_m,
    })
}

/// Reads and parses an EGM96 ASCII grid file (e.g. `WW15MGH.GRD`).
///
/// # Errors
///
/// Returns [`Error::GeoidFileFormat`] when the file cannot be read or when its
/// contents fail to parse (see [`parse_egm96_ascii_str`]).
pub fn parse_egm96_ascii_file(path: &Path) -> Result<GeoidGrid, Error> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        Error::GeoidFileFormat(format!(
            "failed to read EGM96 ASCII grid file {}: {e}",
            path.display()
        ))
    })?;
    parse_egm96_ascii_str(&text)
}

// ===========================================================================
// EGM2008 2.5′ binary (self-consistent container)
// ===========================================================================

/// Dimension header for the EGM2008 2.5′ binary container.
///
/// Stored as the first 8 bytes of the file: `n_lat` then `n_lon`, each a
/// little-endian `u32`.  For the canonical 2.5′ grid these are `4321` and
/// `8640` respectively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Egm2008BinaryHeader {
    /// Number of latitude rows.
    pub n_lat: u32,
    /// Number of longitude columns.
    pub n_lon: u32,
}

/// Parses the 8-byte EGM2008 binary dimension header.
///
/// Requires at least `EGM2008_HEADER_LEN` bytes; reads `n_lat` and `n_lon`
/// as consecutive little-endian `u32` values and validates that both are
/// non-zero and `<= ` `MAX_GRID_DIM`.
///
/// # Errors
///
/// Returns [`Error::GeoidFileFormat`] when fewer than 8 bytes are supplied or
/// when either dimension is zero or implausibly large.
pub fn parse_egm2008_binary_25_header(bytes: &[u8]) -> Result<Egm2008BinaryHeader, Error> {
    if bytes.len() < EGM2008_HEADER_LEN {
        return Err(Error::GeoidFileFormat(format!(
            "EGM2008 binary header requires at least {EGM2008_HEADER_LEN} bytes, got {}",
            bytes.len()
        )));
    }
    // Indexing 0..8 is sound: the length check above guarantees the bytes.
    let n_lat = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let n_lon = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);

    if n_lat == 0 || n_lon == 0 {
        return Err(Error::GeoidFileFormat(format!(
            "EGM2008 binary header dimensions must be non-zero (n_lat={n_lat}, n_lon={n_lon})"
        )));
    }
    if n_lat > MAX_GRID_DIM || n_lon > MAX_GRID_DIM {
        return Err(Error::GeoidFileFormat(format!(
            "EGM2008 binary header dimensions out of range \
             (n_lat={n_lat}, n_lon={n_lon}, max={MAX_GRID_DIM})"
        )));
    }

    Ok(Egm2008BinaryHeader { n_lat, n_lon })
}

/// Parses a complete EGM2008 2.5′ binary grid from an in-memory byte slice.
///
/// Layout: an 8-byte [`Egm2008BinaryHeader`] followed by `n_lat * n_lon`
/// little-endian `f32` undulation records (metres) in row-major order (row 0 =
/// southernmost, column 0 = `-180.0°`).
///
/// The grid geometry is derived from the dimensions for a global 2.5′-style
/// grid: `lat_min = -90.0°`, `lon_min = -180.0°`,
/// `lat_step = 180.0 / (n_lat - 1)`, `lon_step = 360.0 / n_lon`.  When
/// `n_lat == 1` the latitude step is reported as `0.0` (a single-row grid has
/// no inter-row spacing).
///
/// # Errors
///
/// Returns [`Error::GeoidFileFormat`] when the header is malformed (see
/// [`parse_egm2008_binary_25_header`]) or when the byte length does not equal
/// exactly `8 + n_lat * n_lon * 4`.
pub fn parse_egm2008_binary_25(bytes: &[u8]) -> Result<GeoidGrid, Error> {
    let header = parse_egm2008_binary_25_header(bytes)?;
    let n_lat = header.n_lat as usize;
    let n_lon = header.n_lon as usize;

    // n_lat, n_lon <= MAX_GRID_DIM (100_000), so the products below cannot
    // overflow a 64-bit usize, but use checked arithmetic for defence in depth.
    let cells = n_lat
        .checked_mul(n_lon)
        .ok_or_else(|| Error::GeoidFileFormat("EGM2008 binary grid size overflow".to_string()))?;
    let body_len = cells
        .checked_mul(4)
        .ok_or_else(|| Error::GeoidFileFormat("EGM2008 binary body size overflow".to_string()))?;
    let expected_len = body_len
        .checked_add(EGM2008_HEADER_LEN)
        .ok_or_else(|| Error::GeoidFileFormat("EGM2008 binary total size overflow".to_string()))?;

    if bytes.len() != expected_len {
        return Err(Error::GeoidFileFormat(format!(
            "EGM2008 binary grid has {} bytes but header dimensions \
             ({n_lat} × {n_lon}) require exactly {expected_len} \
             ({EGM2008_HEADER_LEN}-byte header + {cells} × 4-byte f32 body)",
            bytes.len()
        )));
    }

    let body = &bytes[EGM2008_HEADER_LEN..];
    let mut heights_m: Vec<f32> = Vec::with_capacity(cells);
    for chunk in body.chunks_exact(4) {
        // chunks_exact yields slices of length 4; explicit array avoids any
        // panic path from try_into.
        heights_m.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }

    let lat_step_deg = if n_lat <= 1 {
        0.0
    } else {
        180.0 / (n_lat as f64 - 1.0)
    };
    let lon_step_deg = 360.0 / n_lon as f64;

    Ok(GeoidGrid {
        model: GeoidModel::Egm2008,
        lat_step_deg,
        lon_step_deg,
        lat_min_deg: GRID_LAT_MIN_DEG,
        lon_min_deg: GRID_LON_MIN_DEG,
        n_lat,
        n_lon,
        heights_m,
    })
}

/// Reads and parses an EGM2008 2.5′ binary grid file.
///
/// # Errors
///
/// Returns [`Error::GeoidFileFormat`] when the file cannot be read or when its
/// contents fail to parse (see [`parse_egm2008_binary_25`]).
pub fn parse_egm2008_binary_25_file(path: &Path) -> Result<GeoidGrid, Error> {
    let bytes = std::fs::read(path).map_err(|e| {
        Error::GeoidFileFormat(format!(
            "failed to read EGM2008 binary grid file {}: {e}",
            path.display()
        ))
    })?;
    parse_egm2008_binary_25(&bytes)
}

// ===========================================================================
// Format auto-detection
// ===========================================================================

/// Auto-detects the geoid grid format of `path` and parses it.
///
/// Sniffing reads up to the first 16 bytes:
///
/// * If the first non-whitespace byte is a printable ASCII digit, `+`, `-`, or
///   `.`, the file is treated as **EGM96 ASCII** and dispatched to
///   [`parse_egm96_ascii_file`].
/// * Otherwise an **EGM2008 binary** dimension header is attempted on the full
///   file via [`parse_egm2008_binary_25_file`].
///
/// The detector is deliberately *conservative*: a file whose leading bytes are
/// neither numeric-ASCII nor a valid binary header (e.g. a leading alphabetic
/// byte, or an empty file) yields a descriptive
/// [`Error::GeoidFileFormat`] rather than a silently mis-parsed grid.
///
/// # Errors
///
/// Returns [`Error::GeoidFileFormat`] when the file cannot be read, when its
/// leading bytes match neither format, or when the chosen parser fails.
pub fn load_geoid_auto(path: &Path) -> Result<GeoidGrid, Error> {
    let bytes = std::fs::read(path).map_err(|e| {
        Error::GeoidFileFormat(format!(
            "failed to read geoid grid file {}: {e}",
            path.display()
        ))
    })?;

    // Find the first non-whitespace byte within the sniff window.
    let sniff = &bytes[..bytes.len().min(16)];
    let leading = sniff.iter().copied().find(|b| !b.is_ascii_whitespace());

    match leading {
        Some(b) if b.is_ascii_digit() || b == b'+' || b == b'-' || b == b'.' => {
            // Looks like ASCII numeric text → EGM96 ASCII.
            parse_egm96_ascii_file(path)
        }
        Some(_) => {
            // Non-numeric leading byte: only accept it as binary if it parses
            // as a valid EGM2008 dimension header, otherwise reject clearly.
            parse_egm2008_binary_25_header(&bytes).map_err(|e| {
                Error::GeoidFileFormat(format!(
                    "ambiguous/unknown geoid format for {}: leading byte 0x{:02x} is \
                     not numeric-ASCII and the file is not a valid EGM2008 binary grid ({e})",
                    path.display(),
                    leading.unwrap_or(0)
                ))
            })?;
            parse_egm2008_binary_25(&bytes)
        }
        None => Err(Error::GeoidFileFormat(format!(
            "ambiguous/unknown geoid format for {}: file is empty or all-whitespace",
            path.display()
        ))),
    }
}

// ===========================================================================
// Unit tests (in-memory only; file I/O is covered by the integration suite)
// ===========================================================================

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_header_derives_inclusive_node_counts() {
        let h = parse_egm96_ascii_header("-90 90 -180 180 0.25 0.25").expect("valid header");
        // 180 / 0.25 + 1 = 721 rows; 360 / 0.25 + 1 = 1441 cols.
        assert_eq!(h.n_lat, 721);
        assert_eq!(h.n_lon, 1441);
        assert!((h.lat_step - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn test_header_rejects_non_numeric_token() {
        let err = parse_egm96_ascii_header("-90 NORTH -180 180 0.25 0.25")
            .expect_err("non-numeric token must fail");
        assert!(matches!(err, Error::GeoidFileFormat(_)));
    }

    #[test]
    fn test_binary_header_round_trips_canonical_dims() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&4321u32.to_le_bytes());
        bytes.extend_from_slice(&8640u32.to_le_bytes());
        let h = parse_egm2008_binary_25_header(&bytes).expect("valid header");
        assert_eq!(h.n_lat, 4321);
        assert_eq!(h.n_lon, 8640);
    }

    #[test]
    fn test_binary_rejects_short_header() {
        let err = parse_egm2008_binary_25_header(&[0u8; 4]).expect_err("4 bytes is too short");
        assert!(matches!(err, Error::GeoidFileFormat(_)));
    }

    #[test]
    fn test_egm2008_step_derivation() {
        // 3 rows over 180° → 90° step; 3 cols over 360° → 120° step.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 3 * 3 * 4]);
        let g = parse_egm2008_binary_25(&bytes).expect("valid grid");
        assert!((g.lat_step_deg - 90.0).abs() < 1e-12);
        assert!((g.lon_step_deg - 120.0).abs() < 1e-12);
        assert!((g.lat_min_deg - GRID_LAT_MIN_DEG).abs() < f64::EPSILON);
        assert!((g.lon_min_deg - GRID_LON_MIN_DEG).abs() < f64::EPSILON);
    }
}
