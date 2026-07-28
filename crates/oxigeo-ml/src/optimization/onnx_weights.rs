//! Targeted, loss-less ONNX protobuf walker for locating float32 weight
//! initializers and rewriting individual weight values *in place*.
//!
//! # Why a bespoke walker
//!
//! Real model optimization (pruning, weight editing) must operate on the actual
//! weight tensors stored inside an ONNX model, not on a byte reinterpretation of
//! the whole file. `oxionnx` can *parse* an ONNX model into a [`ModelProto`],
//! but it does not preserve every field of the original message and it cannot
//! *serialize* a model back to disk. Re-encoding from the parsed representation
//! would silently drop information (attributes, subgraphs, unknown fields) and
//! corrupt the model.
//!
//! Instead, this module walks the raw protobuf wire format just far enough to
//! find each `TensorProto` initializer and the **absolute byte offset** of its
//! float32 payload. Because pruning only *zeroes* existing values (it never
//! changes any field length), the edited bytes remain a byte-for-byte valid
//! ONNX message — every varint, tag and length prefix is preserved. The output
//! is therefore guaranteed to load in any conformant ONNX runtime.
//!
//! Only inline float32 initializers stored as `raw_data` (little-endian) or a
//! packed `float_data` field are located; non-float, externally-stored, or
//! non-packed initializers are skipped (they are never touched or corrupted).
//!
//! Relevant ONNX field numbers (from `onnx.proto`):
//!
//! - `ModelProto.graph`         = field 7 (message)
//! - `GraphProto.initializer`   = field 5 (repeated `TensorProto`)
//! - `TensorProto.dims`         = field 1 (repeated int64)
//! - `TensorProto.data_type`    = field 2 (int32)
//! - `TensorProto.float_data`   = field 4 (repeated float, packed)
//! - `TensorProto.name`         = field 8 (string)
//! - `TensorProto.raw_data`     = field 9 (bytes)
//! - `TensorProto.data_location`= field 14 (int32; 1 = external)

use crate::error::{ModelError, Result};

const WIRE_VARINT: u8 = 0;
const WIRE_I64: u8 = 1;
const WIRE_LEN: u8 = 2;
const WIRE_I32: u8 = 5;

/// `TensorProto.data_type` value for IEEE-754 32-bit float.
const DT_FLOAT: u64 = 1;

/// Field number of `ModelProto.graph`.
const F_MODEL_GRAPH: u64 = 7;
/// Field number of `GraphProto.initializer`.
const F_GRAPH_INITIALIZER: u64 = 5;

/// A float32 initializer located inside an ONNX model file.
///
/// The [`data_offset`](Self::data_offset) is the absolute byte offset of the
/// first float within the model file; consecutive values are exactly 4 bytes
/// apart (little-endian), which holds for both `raw_data` and packed
/// `float_data` encodings.
#[derive(Debug, Clone)]
pub struct FloatInitializer {
    /// Initializer (tensor) name as stored in the graph.
    pub name: String,
    /// Tensor dimensions (row-major). Empty for a scalar.
    pub dims: Vec<usize>,
    /// Decoded float32 values (in storage order).
    pub values: Vec<f32>,
    /// Absolute byte offset of `values[0]` within the model file.
    pub data_offset: usize,
}

impl FloatInitializer {
    /// Number of elements in this initializer.
    #[must_use]
    pub fn numel(&self) -> usize {
        self.values.len()
    }

    /// Returns the tensor shape, falling back to a flat `[numel]` shape when
    /// the stored dims do not multiply out to the element count (which would
    /// otherwise make the tensor invalid for shape-aware consumers).
    #[must_use]
    pub fn normalized_shape(&self) -> Vec<usize> {
        let product: usize = self.dims.iter().product();
        if !self.dims.is_empty() && product == self.values.len() {
            self.dims.clone()
        } else {
            vec![self.values.len()]
        }
    }
}

/// A decoded protobuf field with absolute value byte range.
struct Field {
    number: u64,
    wire: u8,
    /// Absolute start offset of the value payload.
    value_start: usize,
    /// Absolute end offset (exclusive) of the value payload.
    value_end: usize,
    /// Absolute offset of the next field key.
    next: usize,
}

fn malformed(msg: impl Into<String>) -> crate::error::MlError {
    ModelError::InvalidFormat {
        message: msg.into(),
    }
    .into()
}

/// Reads a base-128 varint starting at `pos`, returning `(value, next_pos)`.
fn read_varint(buf: &[u8], pos: usize) -> Result<(u64, usize)> {
    let mut result: u64 = 0;
    let mut shift: u32 = 0;
    let mut p = pos;
    loop {
        let byte = *buf
            .get(p)
            .ok_or_else(|| malformed("truncated varint in ONNX protobuf"))?;
        if shift >= 64 {
            return Err(malformed("overlong varint in ONNX protobuf"));
        }
        result |= u64::from(byte & 0x7f) << shift;
        p += 1;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
    }
    Ok((result, p))
}

/// Reads the next field between `pos` and `end`, or `None` at the end.
fn next_field(buf: &[u8], pos: usize, end: usize) -> Result<Option<Field>> {
    if pos >= end {
        return Ok(None);
    }
    let (key, after_key) = read_varint(buf, pos)?;
    let number = key >> 3;
    let wire = (key & 0x7) as u8;

    let (value_start, value_end, next) = match wire {
        WIRE_VARINT => {
            let (_v, after) = read_varint(buf, after_key)?;
            (after_key, after, after)
        }
        WIRE_I64 => {
            let e = after_key
                .checked_add(8)
                .ok_or_else(|| malformed("64-bit field overflow"))?;
            if e > end {
                return Err(malformed("64-bit field exceeds message bounds"));
            }
            (after_key, e, e)
        }
        WIRE_I32 => {
            let e = after_key
                .checked_add(4)
                .ok_or_else(|| malformed("32-bit field overflow"))?;
            if e > end {
                return Err(malformed("32-bit field exceeds message bounds"));
            }
            (after_key, e, e)
        }
        WIRE_LEN => {
            let (len, after_len) = read_varint(buf, after_key)?;
            let e = after_len
                .checked_add(len as usize)
                .ok_or_else(|| malformed("length-delimited field overflow"))?;
            if e > end {
                return Err(malformed("length-delimited field exceeds message bounds"));
            }
            (after_len, e, e)
        }
        other => return Err(malformed(format!("unsupported protobuf wire type {other}"))),
    };

    Ok(Some(Field {
        number,
        wire,
        value_start,
        value_end,
        next,
    }))
}

/// Finds the byte range of the `GraphProto` inside a `ModelProto`.
fn find_graph_span(buf: &[u8]) -> Result<(usize, usize)> {
    let mut pos = 0;
    let end = buf.len();
    while let Some(field) = next_field(buf, pos, end)? {
        pos = field.next;
        if field.number == F_MODEL_GRAPH && field.wire == WIRE_LEN {
            return Ok((field.value_start, field.value_end));
        }
    }
    Err(malformed(
        "no GraphProto found (input is not an ONNX ModelProto)",
    ))
}

/// Parses a single `TensorProto` and, if it is an inline float32 initializer,
/// returns its [`FloatInitializer`] descriptor. Non-float / external / unpacked
/// tensors return `Ok(None)` and are left untouched by callers.
fn parse_tensor(buf: &[u8], start: usize, end: usize) -> Result<Option<FloatInitializer>> {
    let mut data_type: u64 = 0;
    let mut data_location: u64 = 0;
    let mut name = String::new();
    let mut dims: Vec<usize> = Vec::new();
    let mut raw_span: Option<(usize, usize)> = None;
    let mut packed_float_span: Option<(usize, usize)> = None;
    let mut has_unpacked_float = false;

    let mut pos = start;
    while let Some(field) = next_field(buf, pos, end)? {
        pos = field.next;
        match (field.number, field.wire) {
            // dims: unpacked int64
            (1, WIRE_VARINT) => {
                let (v, _) = read_varint(buf, field.value_start)?;
                dims.push(v as usize);
            }
            // dims: packed int64
            (1, WIRE_LEN) => {
                let mut p = field.value_start;
                while p < field.value_end {
                    let (v, np) = read_varint(buf, p)?;
                    dims.push(v as usize);
                    p = np;
                }
            }
            // data_type
            (2, WIRE_VARINT) => {
                let (v, _) = read_varint(buf, field.value_start)?;
                data_type = v;
            }
            // float_data, packed
            (4, WIRE_LEN) => {
                packed_float_span = Some((field.value_start, field.value_end));
            }
            // float_data, unpacked (each element a separate fixed32 field)
            (4, WIRE_I32) => {
                has_unpacked_float = true;
            }
            // name
            (8, WIRE_LEN) => {
                name =
                    String::from_utf8_lossy(&buf[field.value_start..field.value_end]).into_owned();
            }
            // raw_data
            (9, WIRE_LEN) => {
                raw_span = Some((field.value_start, field.value_end));
            }
            // data_location
            (14, WIRE_VARINT) => {
                let (v, _) = read_varint(buf, field.value_start)?;
                data_location = v;
            }
            _ => {}
        }
    }

    if data_type != DT_FLOAT || data_location == 1 || has_unpacked_float {
        // Not an inline float32 tensor we can safely edit; skip it.
        return Ok(None);
    }

    let (data_offset, byte_len) = if let Some((s, e)) = raw_span {
        (s, e - s)
    } else if let Some((s, e)) = packed_float_span {
        (s, e - s)
    } else {
        // Float tensor with no inline data (e.g. all-zero omitted); nothing to prune.
        return Ok(None);
    };

    if !byte_len.is_multiple_of(4) {
        return Ok(None);
    }

    let mut values = Vec::with_capacity(byte_len / 4);
    let mut p = data_offset;
    let data_end = data_offset + byte_len;
    while p + 4 <= data_end {
        let bytes = [buf[p], buf[p + 1], buf[p + 2], buf[p + 3]];
        values.push(f32::from_le_bytes(bytes));
        p += 4;
    }

    Ok(Some(FloatInitializer {
        name,
        dims,
        values,
        data_offset,
    }))
}

/// Parses all inline float32 initializers from a complete ONNX model file.
///
/// # Errors
/// Returns [`ModelError::InvalidFormat`] if the bytes are not a valid ONNX
/// `ModelProto` (no graph field, or malformed protobuf framing).
pub fn parse_float_initializers(buf: &[u8]) -> Result<Vec<FloatInitializer>> {
    let (graph_start, graph_end) = find_graph_span(buf)?;

    let mut out = Vec::new();
    let mut pos = graph_start;
    while let Some(field) = next_field(buf, pos, graph_end)? {
        pos = field.next;
        if field.number == F_GRAPH_INITIALIZER && field.wire == WIRE_LEN {
            if let Some(init) = parse_tensor(buf, field.value_start, field.value_end)? {
                out.push(init);
            }
        }
    }
    Ok(out)
}

/// Zeroes the float32 values of `init` for which `keep_mask[i] == false`,
/// editing `bytes` in place. `keep_mask` must be aligned with `init.values`.
///
/// This preserves every protobuf field length, so the resulting buffer remains
/// a byte-for-byte valid ONNX model (only the numeric weight values change).
pub fn zero_values_in_place(bytes: &mut [u8], init: &FloatInitializer, keep_mask: &[bool]) {
    let zero = 0f32.to_le_bytes();
    for (i, &keep) in keep_mask.iter().enumerate() {
        if keep {
            continue;
        }
        let offset = init.data_offset + i * 4;
        if offset + 4 <= bytes.len() {
            bytes[offset..offset + 4].copy_from_slice(&zero);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
pub(crate) mod test_support {
    //! Minimal ONNX protobuf *encoder* used only for tests, so that the
    //! in-place editing round-trips against genuine ONNX wire format.

    /// Encodes a protobuf varint.
    fn put_varint(out: &mut Vec<u8>, mut value: u64) {
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if value == 0 {
                break;
            }
        }
    }

    fn put_key(out: &mut Vec<u8>, field: u64, wire: u8) {
        put_varint(out, (field << 3) | u64::from(wire));
    }

    fn put_len_delimited(out: &mut Vec<u8>, field: u64, payload: &[u8]) {
        put_key(out, field, 2);
        put_varint(out, payload.len() as u64);
        out.extend_from_slice(payload);
    }

    /// Encodes a `TensorProto` with a float32 `raw_data` payload.
    fn encode_tensor(name: &str, dims: &[i64], values: &[f32]) -> Vec<u8> {
        let mut t = Vec::new();
        // field 1: dims (unpacked int64)
        for &d in dims {
            put_key(&mut t, 1, 0);
            put_varint(&mut t, d as u64);
        }
        // field 2: data_type = 1 (FLOAT)
        put_key(&mut t, 2, 0);
        put_varint(&mut t, 1);
        // field 8: name
        put_len_delimited(&mut t, 8, name.as_bytes());
        // field 9: raw_data (little-endian f32)
        let mut raw = Vec::with_capacity(values.len() * 4);
        for &v in values {
            raw.extend_from_slice(&v.to_le_bytes());
        }
        put_len_delimited(&mut t, 9, &raw);
        t
    }

    /// Builds a complete `ModelProto` containing the given initializers.
    pub fn build_model(initializers: &[(&str, Vec<i64>, Vec<f32>)]) -> Vec<u8> {
        // GraphProto
        let mut graph = Vec::new();
        // field 2: graph name
        put_len_delimited(&mut graph, 2, b"test_graph");
        // field 5: initializer(s)
        for (name, dims, values) in initializers {
            let tensor = encode_tensor(name, dims, values);
            put_len_delimited(&mut graph, 5, &tensor);
        }

        // ModelProto
        let mut model = Vec::new();
        // field 1: ir_version
        put_key(&mut model, 1, 0);
        put_varint(&mut model, 8);
        // field 7: graph
        put_len_delimited(&mut model, 7, &graph);
        model
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::test_support::build_model;
    use super::*;

    #[test]
    fn test_parse_single_initializer() {
        let values = vec![1.0f32, -2.0, 3.0, -4.0];
        let model = build_model(&[("w", vec![2, 2], values.clone())]);

        let inits = parse_float_initializers(&model).expect("parse");
        assert_eq!(inits.len(), 1);
        assert_eq!(inits[0].name, "w");
        assert_eq!(inits[0].dims, vec![2, 2]);
        assert_eq!(inits[0].values, values);
    }

    #[test]
    fn test_parse_multiple_initializers() {
        let a = vec![0.5f32; 8];
        let b = vec![-1.5f32; 3];
        let model = build_model(&[("a", vec![2, 4], a.clone()), ("b", vec![3], b.clone())]);

        let inits = parse_float_initializers(&model).expect("parse");
        assert_eq!(inits.len(), 2);
        assert_eq!(inits[0].values, a);
        assert_eq!(inits[1].values, b);
    }

    #[test]
    fn test_zero_in_place_preserves_framing_and_values() {
        let values = vec![1.0f32, 2.0, 3.0, 4.0];
        let mut model = build_model(&[("w", vec![4], values)]);

        let inits = parse_float_initializers(&model).expect("parse");
        // Prune elements 1 and 3 (keep 0 and 2).
        let keep = vec![true, false, true, false];
        zero_values_in_place(&mut model, &inits[0], &keep);

        // Re-parse: framing must still be valid and values must reflect zeroing.
        let reparsed = parse_float_initializers(&model).expect("reparse");
        assert_eq!(reparsed.len(), 1);
        assert_eq!(reparsed[0].values, vec![1.0, 0.0, 3.0, 0.0]);
    }

    #[test]
    fn test_reject_non_onnx_bytes() {
        let junk = vec![0xAAu8; 64];
        assert!(parse_float_initializers(&junk).is_err());
    }

    #[test]
    fn test_oxionnx_can_load_edited_model() {
        // The edited bytes must still parse through the real oxionnx parser.
        let values = vec![10.0f32, 20.0, 30.0, 40.0];
        let mut model = build_model(&[("weight", vec![2, 2], values)]);
        let inits = parse_float_initializers(&model).expect("parse");
        let keep = vec![true, true, false, false];
        zero_values_in_place(&mut model, &inits[0], &keep);

        let parsed = oxionnx::proto::parse_model(&model).expect("oxionnx must parse edited model");
        let init = parsed
            .graph
            .initializers
            .iter()
            .find(|t| t.name == "weight")
            .expect("weight initializer present");
        let tensor = init.to_tensor();
        assert_eq!(tensor.data, vec![10.0, 20.0, 0.0, 0.0]);
    }
}
