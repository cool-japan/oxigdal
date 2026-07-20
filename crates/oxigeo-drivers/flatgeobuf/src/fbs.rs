//! Minimal, bounds-checked `FlatBuffers` table reader for the `FlatGeobuf`
//! schema.
//!
//! `flatc` is not available in this environment, so instead of generated code
//! this module hand-writes the table accessors directly against the raw
//! `FlatBuffers` wire format, following the official `FlatGeobuf` schema
//! (`header.fbs` / `feature.fbs`). Writing is performed with
//! [`flatbuffers::FlatBufferBuilder`]; reading uses the manual vtable-offset
//! walk implemented here.
//!
//! Every read is bounds-checked and returns a typed [`FlatGeobufError`] instead
//! of panicking on malformed or truncated input, satisfying the no-panic policy
//! for production code.

use crate::error::{FlatGeobufError, Result};

/// Convenience alias for a finished `FlatBuffers` table offset produced by the
/// builder helpers in this crate.
pub type Offset = flatbuffers::WIPOffset<flatbuffers::TableFinishedWIPOffset>;

// ── Header table vtable slot offsets (`4 + 2 * field_id`) ────────────────────
/// Vtable slot for `Header.name`.
pub const HEADER_VT_NAME: u16 = 4;
/// Vtable slot for `Header.envelope`.
pub const HEADER_VT_ENVELOPE: u16 = 6;
/// Vtable slot for `Header.geometry_type`.
pub const HEADER_VT_GEOMETRY_TYPE: u16 = 8;
/// Vtable slot for `Header.has_z`.
pub const HEADER_VT_HAS_Z: u16 = 10;
/// Vtable slot for `Header.has_m`.
pub const HEADER_VT_HAS_M: u16 = 12;
/// Vtable slot for `Header.has_t`.
pub const HEADER_VT_HAS_T: u16 = 14;
/// Vtable slot for `Header.has_tm`.
pub const HEADER_VT_HAS_TM: u16 = 16;
/// Vtable slot for `Header.columns`.
pub const HEADER_VT_COLUMNS: u16 = 18;
/// Vtable slot for `Header.features_count`.
pub const HEADER_VT_FEATURES_COUNT: u16 = 20;
/// Vtable slot for `Header.index_node_size`.
pub const HEADER_VT_INDEX_NODE_SIZE: u16 = 22;
/// Vtable slot for `Header.crs`.
pub const HEADER_VT_CRS: u16 = 24;
/// Vtable slot for `Header.title`.
pub const HEADER_VT_TITLE: u16 = 26;
/// Vtable slot for `Header.description`.
pub const HEADER_VT_DESCRIPTION: u16 = 28;
/// Vtable slot for `Header.metadata`.
pub const HEADER_VT_METADATA: u16 = 30;

// ── Column table vtable slot offsets ─────────────────────────────────────────
/// Vtable slot for `Column.name`.
pub const COLUMN_VT_NAME: u16 = 4;
/// Vtable slot for `Column.type`.
pub const COLUMN_VT_TYPE: u16 = 6;
/// Vtable slot for `Column.title`.
pub const COLUMN_VT_TITLE: u16 = 8;
/// Vtable slot for `Column.description`.
pub const COLUMN_VT_DESCRIPTION: u16 = 10;
/// Vtable slot for `Column.width`.
pub const COLUMN_VT_WIDTH: u16 = 12;
/// Vtable slot for `Column.precision`.
pub const COLUMN_VT_PRECISION: u16 = 14;
/// Vtable slot for `Column.scale`.
pub const COLUMN_VT_SCALE: u16 = 16;
/// Vtable slot for `Column.nullable`.
pub const COLUMN_VT_NULLABLE: u16 = 18;
/// Vtable slot for `Column.unique`.
pub const COLUMN_VT_UNIQUE: u16 = 20;
/// Vtable slot for `Column.primary_key`.
pub const COLUMN_VT_PRIMARY_KEY: u16 = 22;
/// Vtable slot for `Column.metadata`.
pub const COLUMN_VT_METADATA: u16 = 24;

// ── Crs table vtable slot offsets ────────────────────────────────────────────
/// Vtable slot for `Crs.org`.
pub const CRS_VT_ORG: u16 = 4;
/// Vtable slot for `Crs.code`.
pub const CRS_VT_CODE: u16 = 6;
/// Vtable slot for `Crs.name`.
pub const CRS_VT_NAME: u16 = 8;
/// Vtable slot for `Crs.description`.
pub const CRS_VT_DESCRIPTION: u16 = 10;
/// Vtable slot for `Crs.wkt`.
pub const CRS_VT_WKT: u16 = 12;
/// Vtable slot for `Crs.code_string`.
pub const CRS_VT_CODE_STRING: u16 = 14;

// ── Geometry table vtable slot offsets ───────────────────────────────────────
/// Vtable slot for `Geometry.ends`.
pub const GEOM_VT_ENDS: u16 = 4;
/// Vtable slot for `Geometry.xy`.
pub const GEOM_VT_XY: u16 = 6;
/// Vtable slot for `Geometry.z`.
pub const GEOM_VT_Z: u16 = 8;
/// Vtable slot for `Geometry.m`.
pub const GEOM_VT_M: u16 = 10;
/// Vtable slot for `Geometry.type`.
pub const GEOM_VT_TYPE: u16 = 16;
/// Vtable slot for `Geometry.parts`.
pub const GEOM_VT_PARTS: u16 = 18;

// ── Feature table vtable slot offsets ────────────────────────────────────────
/// Vtable slot for `Feature.geometry`.
pub const FEATURE_VT_GEOMETRY: u16 = 4;
/// Vtable slot for `Feature.properties`.
pub const FEATURE_VT_PROPERTIES: u16 = 6;
/// Vtable slot for `Feature.columns`.
pub const FEATURE_VT_COLUMNS: u16 = 8;

#[inline]
fn oob() -> FlatGeobufError {
    FlatGeobufError::FlatBuffers("unexpected end of FlatBuffers data".to_string())
}

/// A bounds-checked view over the bytes of a `FlatBuffers` message.
#[derive(Clone, Copy)]
struct FbBuffer<'a> {
    data: &'a [u8],
}

impl<'a> FbBuffer<'a> {
    #[inline]
    const fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    #[inline]
    fn len(&self) -> usize {
        self.data.len()
    }

    #[inline]
    fn slice(&self, loc: usize, n: usize) -> Result<&'a [u8]> {
        let end = loc.checked_add(n).ok_or_else(oob)?;
        self.data.get(loc..end).ok_or_else(oob)
    }

    #[inline]
    fn u8(&self, loc: usize) -> Result<u8> {
        self.data.get(loc).copied().ok_or_else(oob)
    }

    #[inline]
    fn i8(&self, loc: usize) -> Result<i8> {
        Ok(self.u8(loc)? as i8)
    }

    #[inline]
    fn u16(&self, loc: usize) -> Result<u16> {
        let b = self.slice(loc, 2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    #[inline]
    fn i16(&self, loc: usize) -> Result<i16> {
        Ok(self.u16(loc)? as i16)
    }

    #[inline]
    fn u32(&self, loc: usize) -> Result<u32> {
        let b = self.slice(loc, 4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    #[inline]
    fn i32(&self, loc: usize) -> Result<i32> {
        Ok(self.u32(loc)? as i32)
    }

    #[inline]
    fn u64(&self, loc: usize) -> Result<u64> {
        let b = self.slice(loc, 8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    #[inline]
    fn i64(&self, loc: usize) -> Result<i64> {
        Ok(self.u64(loc)? as i64)
    }

    #[inline]
    fn f32(&self, loc: usize) -> Result<f32> {
        Ok(f32::from_bits(self.u32(loc)?))
    }

    #[inline]
    fn f64(&self, loc: usize) -> Result<f64> {
        Ok(f64::from_bits(self.u64(loc)?))
    }
}

/// A bounds-checked handle to a single `FlatBuffers` table.
#[derive(Clone, Copy)]
pub struct FbTable<'a> {
    buf: FbBuffer<'a>,
    loc: usize,
}

impl<'a> FbTable<'a> {
    /// Parses the root table of a (non size-prefixed) `FlatBuffers` buffer.
    ///
    /// The first `uoffset` at position 0 of `data` points to the root table.
    pub fn root(data: &'a [u8]) -> Result<Self> {
        let buf = FbBuffer::new(data);
        let root = buf.u32(0)? as usize;
        Self::at(buf, root)
    }

    #[inline]
    fn at(buf: FbBuffer<'a>, loc: usize) -> Result<Self> {
        // The table must at least contain its soffset_t (i32).
        if loc.checked_add(4).ok_or_else(oob)? > buf.len() {
            return Err(oob());
        }
        Ok(Self { buf, loc })
    }

    /// Returns the absolute buffer offset of a field's inline data, or `None`
    /// when the field is absent (not listed in the vtable).
    fn field(&self, vt_slot: u16) -> Result<Option<usize>> {
        let soffset = self.buf.i32(self.loc)?;
        // vtable_pos = table_loc - soffset (soffset may be negative).
        let vt_pos = i64::try_from(self.loc)
            .ok()
            .and_then(|l| l.checked_sub(i64::from(soffset)))
            .ok_or_else(oob)?;
        if vt_pos < 0 {
            return Err(oob());
        }
        let vt_pos = usize::try_from(vt_pos).map_err(|_| oob())?;
        let vt_size = self.buf.u16(vt_pos)? as usize;
        let slot = vt_slot as usize;
        // The 2-byte voffset for this slot must fit within the vtable.
        if slot.checked_add(2).ok_or_else(oob)? > vt_size {
            return Ok(None);
        }
        let field_off = self.buf.u16(vt_pos + slot)? as usize;
        if field_off == 0 {
            Ok(None)
        } else {
            Ok(Some(self.loc.checked_add(field_off).ok_or_else(oob)?))
        }
    }

    /// Reads a `u8` scalar field, returning `default` when absent.
    pub fn get_u8(&self, slot: u16, default: u8) -> Result<u8> {
        match self.field(slot)? {
            Some(o) => self.buf.u8(o),
            None => Ok(default),
        }
    }

    /// Reads an `i8` scalar field, returning `default` when absent.
    pub fn get_i8(&self, slot: u16, default: i8) -> Result<i8> {
        match self.field(slot)? {
            Some(o) => self.buf.i8(o),
            None => Ok(default),
        }
    }

    /// Reads a `bool` scalar field, returning `default` when absent.
    pub fn get_bool(&self, slot: u16, default: bool) -> Result<bool> {
        Ok(self.get_u8(slot, u8::from(default))? != 0)
    }

    /// Reads a `u16` scalar field, returning `default` when absent.
    pub fn get_u16(&self, slot: u16, default: u16) -> Result<u16> {
        match self.field(slot)? {
            Some(o) => self.buf.u16(o),
            None => Ok(default),
        }
    }

    /// Reads an `i16` scalar field, returning `default` when absent.
    pub fn get_i16(&self, slot: u16, default: i16) -> Result<i16> {
        match self.field(slot)? {
            Some(o) => self.buf.i16(o),
            None => Ok(default),
        }
    }

    /// Reads a `u32` scalar field, returning `default` when absent.
    pub fn get_u32(&self, slot: u16, default: u32) -> Result<u32> {
        match self.field(slot)? {
            Some(o) => self.buf.u32(o),
            None => Ok(default),
        }
    }

    /// Reads an `i32` scalar field, returning `default` when absent.
    pub fn get_i32(&self, slot: u16, default: i32) -> Result<i32> {
        match self.field(slot)? {
            Some(o) => self.buf.i32(o),
            None => Ok(default),
        }
    }

    /// Reads a `u64` scalar field, returning `default` when absent.
    pub fn get_u64(&self, slot: u16, default: u64) -> Result<u64> {
        match self.field(slot)? {
            Some(o) => self.buf.u64(o),
            None => Ok(default),
        }
    }

    /// Reads an `i64` scalar field, returning `default` when absent.
    pub fn get_i64(&self, slot: u16, default: i64) -> Result<i64> {
        match self.field(slot)? {
            Some(o) => self.buf.i64(o),
            None => Ok(default),
        }
    }

    /// Reads an `f32` scalar field, returning `default` when absent.
    pub fn get_f32(&self, slot: u16, default: f32) -> Result<f32> {
        match self.field(slot)? {
            Some(o) => self.buf.f32(o),
            None => Ok(default),
        }
    }

    /// Reads an `f64` scalar field, returning `default` when absent.
    pub fn get_f64(&self, slot: u16, default: f64) -> Result<f64> {
        match self.field(slot)? {
            Some(o) => self.buf.f64(o),
            None => Ok(default),
        }
    }

    /// Reads a UTF-8 string field, returning `None` when absent.
    pub fn get_string(&self, slot: u16) -> Result<Option<String>> {
        match self.field(slot)? {
            None => Ok(None),
            Some(o) => {
                let rel = self.buf.u32(o)? as usize;
                let sp = o.checked_add(rel).ok_or_else(oob)?;
                let len = self.buf.u32(sp)? as usize;
                let start = sp.checked_add(4).ok_or_else(oob)?;
                let bytes = self.buf.slice(start, len)?;
                Ok(Some(String::from_utf8(bytes.to_vec())?))
            }
        }
    }

    /// Resolves a vector field to `(element_start_offset, element_count)`,
    /// validating that `count * stride` bytes fit within the buffer.
    fn vector(&self, slot: u16, stride: usize) -> Result<Option<(usize, usize)>> {
        match self.field(slot)? {
            None => Ok(None),
            Some(o) => {
                let rel = self.buf.u32(o)? as usize;
                let vp = o.checked_add(rel).ok_or_else(oob)?;
                let count = self.buf.u32(vp)? as usize;
                let start = vp.checked_add(4).ok_or_else(oob)?;
                // Guard against absurd counts from malformed input before
                // allocating: the element bytes must lie inside the buffer.
                let span = count.checked_mul(stride).ok_or_else(oob)?;
                let end = start.checked_add(span).ok_or_else(oob)?;
                if end > self.buf.len() {
                    return Err(oob());
                }
                Ok(Some((start, count)))
            }
        }
    }

    /// Reads a `[double]` vector field, returning `None` when absent.
    pub fn get_f64_vector(&self, slot: u16) -> Result<Option<Vec<f64>>> {
        match self.vector(slot, 8)? {
            None => Ok(None),
            Some((start, count)) => {
                let mut v = Vec::with_capacity(count);
                for i in 0..count {
                    v.push(self.buf.f64(start + i * 8)?);
                }
                Ok(Some(v))
            }
        }
    }

    /// Reads a `[uint]` vector field, returning `None` when absent.
    pub fn get_u32_vector(&self, slot: u16) -> Result<Option<Vec<u32>>> {
        match self.vector(slot, 4)? {
            None => Ok(None),
            Some((start, count)) => {
                let mut v = Vec::with_capacity(count);
                for i in 0..count {
                    v.push(self.buf.u32(start + i * 4)?);
                }
                Ok(Some(v))
            }
        }
    }

    /// Reads a `[ubyte]` vector field, returning `None` when absent.
    pub fn get_u8_vector(&self, slot: u16) -> Result<Option<&'a [u8]>> {
        match self.vector(slot, 1)? {
            None => Ok(None),
            Some((start, count)) => Ok(Some(self.buf.slice(start, count)?)),
        }
    }

    /// Reads a nested table field, returning `None` when absent.
    pub fn get_table(&self, slot: u16) -> Result<Option<FbTable<'a>>> {
        match self.field(slot)? {
            None => Ok(None),
            Some(o) => {
                let rel = self.buf.u32(o)? as usize;
                let tp = o.checked_add(rel).ok_or_else(oob)?;
                Ok(Some(Self::at(self.buf, tp)?))
            }
        }
    }

    /// Reads a vector-of-tables field into a `Vec<FbTable>` (empty when absent).
    pub fn get_table_vector(&self, slot: u16) -> Result<Vec<FbTable<'a>>> {
        match self.vector(slot, 4)? {
            None => Ok(Vec::new()),
            Some((start, count)) => {
                let mut v = Vec::with_capacity(count);
                for i in 0..count {
                    let elem = start + i * 4;
                    let rel = self.buf.u32(elem)? as usize;
                    let tp = elem.checked_add(rel).ok_or_else(oob)?;
                    v.push(Self::at(self.buf, tp)?);
                }
                Ok(v)
            }
        }
    }
}
