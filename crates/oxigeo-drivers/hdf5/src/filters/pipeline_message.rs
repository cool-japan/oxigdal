//! HDF5 Object Header "Filter Pipeline" message (type `0x000B`) parser.
//!
//! The Filter Pipeline message stored in a dataset's object header records the
//! ordered list of filters that were applied to each chunk on write, together
//! with each filter's *client data* (`cd_values`). Decoding a chunk therefore
//! requires this message: it supplies the per-filter parameters the codecs in
//! [`crate::filters`] are driven by (element count, precision, scale factor,
//! datatype descriptor, ...).
//!
//! Two on-disk layouts exist and are both supported here.
//!
//! ## Version 1
//!
//! An 8-byte header followed by 8-byte-aligned per-filter records:
//!
//! ```text
//! version                    u8   (== 1)
//! number of filters          u8
//! reserved                   u16
//! reserved                   u32
//! per filter:
//!   filter identification    u16
//!   name length              u16   (length incl. NUL, padded to a multiple of 8)
//!   flags                    u16
//!   number of client values  u16
//!   name                     name-length bytes (present only when length > 0)
//!   client data              (number of values) x u32
//!   client data padding      4 bytes when the number of values is odd
//! ```
//!
//! ## Version 2
//!
//! A 2-byte header followed by tightly packed (no 8-byte alignment) records:
//!
//! ```text
//! version                    u8   (== 2)
//! number of filters          u8
//! per filter:
//!   filter identification    u16
//!   name length              u16   (ONLY when filter id >= 256)
//!   flags                    u16
//!   number of client values  u16
//!   name                     name-length bytes (no padding)
//!   client data              (number of values) x u32 (no padding)
//! ```
//!
//! Truncated or otherwise malformed bytes always produce a typed
//! [`Hdf5Error::FilterPipeline`] error — the parser never panics and never
//! fabricates filter parameters.

use super::{Filter, FilterId, FilterPipeline};
use crate::error::{Hdf5Error, Result};

/// `H5Z_FLAG_OPTIONAL` bit in the per-filter flags field.
const FLAG_OPTIONAL: u16 = 0x0001;

/// Filter identification values at or above this threshold are user-defined and
/// carry an explicit name-length field in the version-2 layout.
const CUSTOM_FILTER_ID_MIN: u16 = 256;

/// Minimal little-endian byte cursor with bounds-checked, panic-free reads.
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    /// Borrow the next `n` bytes, advancing the cursor. Fails (never panics) when
    /// fewer than `n` bytes remain or the offset arithmetic would overflow.
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| truncated("length overflow", self.pos))?;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or_else(|| truncated("unexpected end of message", self.pos))?;
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }

    fn u32(&mut self) -> Result<u32> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n).map(|_| ())
    }
}

/// Build a typed truncation error for the filter-pipeline parser.
fn truncated(what: &str, offset: usize) -> Hdf5Error {
    Hdf5Error::FilterPipeline(format!(
        "malformed Filter Pipeline message: {what} at byte offset {offset}"
    ))
}

/// Parse a raw Object Header Filter Pipeline message (message type `0x000B`) into
/// a [`FilterPipeline`].
///
/// The pipeline preserves the on-disk (write / array) filter order, so
/// [`FilterPipeline::apply_reverse`] decodes the filters back-to-front exactly as
/// libhdf5 does. Each filter carries the client-data (`cd_values`) array verbatim
/// so the ScaleOffset / N-Bit / SZIP codecs receive their real parameters.
///
/// Supports both version 1 and version 2 of the message layout. An unsupported
/// version, a truncated body, or any inconsistent length yields a typed
/// [`Hdf5Error::FilterPipeline`] error rather than a panic or garbage pipeline.
pub fn parse_filter_pipeline_message(bytes: &[u8]) -> Result<FilterPipeline> {
    let mut cursor = Cursor::new(bytes);
    let version = cursor.u8()?;
    match version {
        1 => parse_v1(&mut cursor),
        2 => parse_v2(&mut cursor),
        other => Err(Hdf5Error::FilterPipeline(format!(
            "unsupported Filter Pipeline message version {other} (expected 1 or 2)"
        ))),
    }
}

/// Parse the version-1 (8-byte-aligned) layout.
fn parse_v1(cursor: &mut Cursor) -> Result<FilterPipeline> {
    let nfilters = cursor.u8()?;
    cursor.skip(2)?; // reserved (2 bytes)
    cursor.skip(4)?; // reserved (4 bytes)

    let mut pipeline = FilterPipeline::new();
    for _ in 0..nfilters {
        let filter_id = cursor.u16()?;
        let name_length = cursor.u16()? as usize;
        let flags = cursor.u16()?;
        let num_client_values = cursor.u16()? as usize;

        // In version 1 the name-length field already includes the NUL terminator
        // and any padding to a multiple of 8, so the name field is exactly
        // `name_length` bytes.
        let name = read_name(cursor, name_length)?;
        let cd_values = read_client_data(cursor, num_client_values)?;

        // Version 1 pads the client-data section to a multiple of 8 bytes: since
        // each value is 4 bytes, an odd count leaves 4 trailing padding bytes.
        if !num_client_values.is_multiple_of(2) {
            cursor.skip(4)?;
        }

        pipeline.add_filter(build_filter(filter_id, name, flags, cd_values));
    }
    Ok(pipeline)
}

/// Parse the version-2 (tightly packed) layout.
fn parse_v2(cursor: &mut Cursor) -> Result<FilterPipeline> {
    let nfilters = cursor.u8()?;

    let mut pipeline = FilterPipeline::new();
    for _ in 0..nfilters {
        let filter_id = cursor.u16()?;

        // The name-length field is only present for user-defined filters
        // (identification value >= 256). Predefined filters omit it entirely.
        let name_length = if filter_id >= CUSTOM_FILTER_ID_MIN {
            cursor.u16()? as usize
        } else {
            0
        };
        let flags = cursor.u16()?;
        let num_client_values = cursor.u16()? as usize;

        // Version 2 stores the name with no padding and no implicit NUL, exactly
        // `name_length` bytes; the client data is likewise unpadded.
        let name = read_name(cursor, name_length)?;
        let cd_values = read_client_data(cursor, num_client_values)?;

        pipeline.add_filter(build_filter(filter_id, name, flags, cd_values));
    }
    Ok(pipeline)
}

/// Read `name_length` bytes and decode them as the filter name, trimming any
/// trailing NUL padding. A zero length yields an empty name.
fn read_name(cursor: &mut Cursor, name_length: usize) -> Result<String> {
    if name_length == 0 {
        return Ok(String::new());
    }
    let raw = cursor.take(name_length)?;
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    Ok(String::from_utf8_lossy(&raw[..end]).into_owned())
}

/// Read `count` little-endian `u32` client-data values.
fn read_client_data(cursor: &mut Cursor, count: usize) -> Result<Vec<u32>> {
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(cursor.u32()?);
    }
    Ok(values)
}

/// Assemble a [`Filter`] from the parsed record fields, filling in a sensible
/// default name for predefined filters that carry none.
fn build_filter(filter_id: u16, name: String, flags: u16, cd_values: Vec<u32>) -> Filter {
    let id = FilterId::from_id(filter_id);
    let name = if name.is_empty() {
        default_name(id)
    } else {
        name
    };
    let optional = flags & FLAG_OPTIONAL != 0;
    Filter::new(id, name, cd_values, optional)
}

/// Default filter name for a known [`FilterId`] when the message omits one.
fn default_name(id: FilterId) -> String {
    match id {
        FilterId::Deflate => "deflate".to_string(),
        FilterId::Shuffle => "shuffle".to_string(),
        FilterId::Fletcher32 => "fletcher32".to_string(),
        FilterId::Szip => "szip".to_string(),
        FilterId::NBit => "nbit".to_string(),
        FilterId::ScaleOffset => "scaleoffset".to_string(),
        FilterId::Custom(value) => format!("filter_{value}"),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Little-endian byte builder for hand-constructing message bytes.
    #[derive(Default)]
    struct Builder {
        bytes: Vec<u8>,
    }

    impl Builder {
        fn new() -> Self {
            Self::default()
        }
        fn u8(mut self, v: u8) -> Self {
            self.bytes.push(v);
            self
        }
        fn u16(mut self, v: u16) -> Self {
            self.bytes.extend_from_slice(&v.to_le_bytes());
            self
        }
        fn u32(mut self, v: u32) -> Self {
            self.bytes.extend_from_slice(&v.to_le_bytes());
            self
        }
        fn raw(mut self, v: &[u8]) -> Self {
            self.bytes.extend_from_slice(v);
            self
        }
        fn build(self) -> Vec<u8> {
            self.bytes
        }
    }

    #[test]
    fn parse_v1_two_filters_names_and_padding() {
        // Filter A: ScaleOffset (id 6), no name, optional flag, 2 client values
        //           (even -> no trailing padding).
        // Filter B: N-Bit (id 5), name "nbit" padded to 8 bytes, 3 client values
        //           (odd -> 4 bytes of trailing padding).
        let msg = Builder::new()
            .u8(1) // version
            .u8(2) // number of filters
            .u16(0) // reserved
            .u32(0) // reserved
            // Filter A
            .u16(6) // ScaleOffset
            .u16(0) // name length
            .u16(FLAG_OPTIONAL) // flags
            .u16(2) // number of client values
            .u32(2)
            .u32(3)
            // Filter B
            .u16(5) // N-Bit
            .u16(8) // name length (padded to 8)
            .u16(0) // flags
            .u16(3) // number of client values
            .raw(b"nbit\0\0\0\0")
            .u32(8)
            .u32(0)
            .u32(8)
            .u32(0) // client-data padding (odd count)
            .build();

        let pipeline = parse_filter_pipeline_message(&msg).expect("v1 parse");
        assert_eq!(pipeline.len(), 2);

        let a = &pipeline.filters()[0];
        assert_eq!(a.id(), FilterId::ScaleOffset);
        assert_eq!(a.params(), &[2, 3]);
        assert!(a.is_optional());
        assert_eq!(a.name(), "scaleoffset"); // default name filled in

        let b = &pipeline.filters()[1];
        assert_eq!(b.id(), FilterId::NBit);
        assert_eq!(b.params(), &[8, 0, 8]);
        assert!(!b.is_optional());
        assert_eq!(b.name(), "nbit");
    }

    #[test]
    fn parse_v2_predefined_and_custom_filters() {
        // Filter A: Deflate (id 1, predefined) -> NO name-length field.
        // Filter B: custom id 32004 (>= 256) -> HAS name-length field, name
        //           "blosc" with no padding.
        let msg = Builder::new()
            .u8(2) // version
            .u8(2) // number of filters
            // Filter A (predefined, no name-length field)
            .u16(1) // Deflate
            .u16(0) // flags
            .u16(1) // number of client values
            .u32(6)
            // Filter B (custom, name-length present)
            .u16(32004) // custom filter id
            .u16(5) // name length
            .u16(FLAG_OPTIONAL) // flags
            .u16(2) // number of client values
            .raw(b"blosc")
            .u32(2)
            .u32(2)
            .build();

        let pipeline = parse_filter_pipeline_message(&msg).expect("v2 parse");
        assert_eq!(pipeline.len(), 2);

        let a = &pipeline.filters()[0];
        assert_eq!(a.id(), FilterId::Deflate);
        assert_eq!(a.params(), &[6]);
        assert!(!a.is_optional());

        let b = &pipeline.filters()[1];
        assert_eq!(b.id(), FilterId::Custom(32004));
        assert_eq!(b.params(), &[2, 2]);
        assert!(b.is_optional());
        assert_eq!(b.name(), "blosc");
    }

    #[test]
    fn parse_v2_scaleoffset_real_cd_values() {
        // The exact 20-word cd_values libhdf5 writes for an int32 ScaleOffset
        // dataset (scale_type=INT, nelmts=8, class=0, size=4, sign=1, fill def).
        let mut cd = vec![0u32; 20];
        cd[0] = 2; // SO_INT
        cd[2] = 8; // nelmts
        cd[4] = 4; // size
        cd[5] = 1; // signed
        cd[7] = 1; // fill defined

        let mut b = Builder::new()
            .u8(2)
            .u8(1)
            .u16(6) // ScaleOffset
            .u16(0) // flags
            .u16(cd.len() as u16);
        for &w in &cd {
            b = b.u32(w);
        }
        let pipeline = parse_filter_pipeline_message(&b.build()).expect("parse");
        assert_eq!(pipeline.len(), 1);
        assert_eq!(pipeline.filters()[0].id(), FilterId::ScaleOffset);
        assert_eq!(pipeline.filters()[0].params(), cd.as_slice());
    }

    #[test]
    fn from_message_bytes_matches_free_function() {
        let msg = Builder::new()
            .u8(2)
            .u8(1)
            .u16(1) // Deflate
            .u16(0)
            .u16(1)
            .u32(9)
            .build();
        let via_method = FilterPipeline::from_message_bytes(&msg).expect("method");
        assert_eq!(via_method.len(), 1);
        assert_eq!(via_method.filters()[0].params(), &[9]);
    }

    #[test]
    fn unsupported_version_is_typed_error() {
        let msg = Builder::new().u8(7).u8(0).build();
        let res = parse_filter_pipeline_message(&msg);
        assert!(matches!(res, Err(Hdf5Error::FilterPipeline(_))));
    }

    #[test]
    fn truncated_message_is_typed_error_not_panic() {
        // Claims one filter but the body is cut short.
        let msg = Builder::new()
            .u8(2)
            .u8(1)
            .u16(6)
            .u16(0)
            .u16(20) // claims 20 client values, none follow
            .build();
        let res = parse_filter_pipeline_message(&msg);
        assert!(matches!(res, Err(Hdf5Error::FilterPipeline(_))));
    }

    #[test]
    fn empty_pipeline_zero_filters() {
        let msg = Builder::new().u8(2).u8(0).build();
        let pipeline = parse_filter_pipeline_message(&msg).expect("empty");
        assert!(pipeline.is_empty());
    }
}
