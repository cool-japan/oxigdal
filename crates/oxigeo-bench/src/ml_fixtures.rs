//! Minimal, self-contained ONNX model fixtures for the `ml` benchmark suite.
//!
//! `oxigeo-bench` intentionally does not ship a binary `.onnx` file. Instead
//! this module hand-encodes a tiny valid ONNX `ModelProto` (proto3 wire
//! format) at run time, writes it to a temporary file, and hands it to
//! [`oxigeo_ml::OnnxModel::from_file`]. This means the `ml_inference`
//! benchmarks exercise the *real* `oxigeo-ml` -> `oxionnx` parse/build/run
//! pipeline end-to-end rather than a hand-rolled arithmetic stand-in.
//!
//! The fixture graph is deliberately the simplest possible non-trivial
//! computation graph: `output = Relu(input)` over a fixed
//! `[1, 1, height, width]` float32 tensor. It carries no weights, so it can
//! be encoded in a couple hundred bytes without a protobuf code generator.

use std::path::PathBuf;

/// Encodes a protobuf varint (base-128, little-endian group order).
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

/// Encodes a protobuf field tag (`field_number << 3 | wire_type`).
fn put_key(out: &mut Vec<u8>, field: u64, wire: u8) {
    put_varint(out, (field << 3) | u64::from(wire));
}

/// Encodes a length-delimited (wire type 2) field: a nested message, string, or bytes.
fn put_len_delimited(out: &mut Vec<u8>, field: u64, payload: &[u8]) {
    put_key(out, field, 2);
    put_varint(out, payload.len() as u64);
    out.extend_from_slice(payload);
}

/// Encodes a UTF-8 string field.
fn put_string(out: &mut Vec<u8>, field: u64, s: &str) {
    put_len_delimited(out, field, s.as_bytes());
}

/// Encodes a single `TensorShapeProto.Dimension` message carrying a static `dim_value`.
fn encode_dim(n: i64) -> Vec<u8> {
    let mut d = Vec::new();
    put_key(&mut d, 1, 0);
    put_varint(&mut d, n as u64);
    d
}

/// Encodes a `ValueInfoProto` describing a fixed-shape float32 tensor.
fn encode_value_info(name: &str, dims: &[i64]) -> Vec<u8> {
    // TensorShapeProto: repeated field 1 = Dimension.
    let mut shape = Vec::new();
    for &d in dims {
        put_len_delimited(&mut shape, 1, &encode_dim(d));
    }
    // Tensor (nested inside TypeProto): field 1 = elem_type (1 == FLOAT), field 2 = shape.
    let mut tensor_type = Vec::new();
    put_key(&mut tensor_type, 1, 0);
    put_varint(&mut tensor_type, 1); // onnx.TensorProto.DataType.FLOAT
    put_len_delimited(&mut tensor_type, 2, &shape);
    // TypeProto: field 1 = tensor_type.
    let mut type_proto = Vec::new();
    put_len_delimited(&mut type_proto, 1, &tensor_type);
    // ValueInfoProto: field 1 = name, field 2 = type.
    let mut vi = Vec::new();
    put_string(&mut vi, 1, name);
    put_len_delimited(&mut vi, 2, &type_proto);
    vi
}

/// Encodes a `NodeProto` for a single-input, single-output, attribute-free op.
fn encode_node(op_type: &str, name: &str, input: &str, output: &str) -> Vec<u8> {
    let mut n = Vec::new();
    put_string(&mut n, 1, input); // inputs[0]
    put_string(&mut n, 2, output); // outputs[0]
    put_string(&mut n, 3, name);
    put_string(&mut n, 4, op_type);
    n
}

/// Builds a complete, minimal one-node ONNX `ModelProto` performing
/// `output = Relu(input)` over a fixed `[1, 1, height, width]` float32 tensor.
///
/// The returned bytes are a real (if tiny) ONNX file: they decode through
/// `oxionnx`'s protobuf parser exactly like a model exported from PyTorch or
/// TensorFlow would.
#[must_use]
pub fn build_relu_model_bytes(height: i64, width: i64) -> Vec<u8> {
    let node = encode_node("Relu", "relu_0", "input", "output");
    let input_vi = encode_value_info("input", &[1, 1, height, width]);
    let output_vi = encode_value_info("output", &[1, 1, height, width]);

    // GraphProto: field 1 = node, field 2 = name, field 11 = input, field 12 = output.
    let mut graph = Vec::new();
    put_len_delimited(&mut graph, 1, &node);
    put_string(&mut graph, 2, "oxigeo_bench_relu_graph");
    put_len_delimited(&mut graph, 11, &input_vi);
    put_len_delimited(&mut graph, 12, &output_vi);

    // OperatorSetIdProto { domain: "" (default), version: 13 }.
    let mut opset = Vec::new();
    put_key(&mut opset, 2, 0);
    put_varint(&mut opset, 13);

    // ModelProto: field 1 = ir_version, field 7 = graph, field 8 = opset_import.
    let mut model = Vec::new();
    put_key(&mut model, 1, 0);
    put_varint(&mut model, 8); // a recent-enough IR version
    put_len_delimited(&mut model, 7, &graph);
    put_len_delimited(&mut model, 8, &opset);
    model
}

/// Writes `bytes` to a uniquely-named file under [`std::env::temp_dir`] and
/// returns its path. `tag` is folded into the filename purely for
/// debuggability if a run is interrupted before cleanup.
///
/// # Errors
/// Returns the underlying [`std::io::Error`] if the temporary file cannot be
/// written (e.g. a full or read-only temp directory). Callers (benchmark
/// setup code) should surface this loudly rather than silently continuing
/// with a missing fixture.
pub fn write_temp_model(bytes: &[u8], tag: &str) -> std::io::Result<PathBuf> {
    let mut path = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    path.push(format!(
        "oxigeo-bench-ml-{tag}-{}-{nanos}.onnx",
        std::process::id()
    ));
    std::fs::write(&path, bytes)?;
    Ok(path)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn relu_model_bytes_round_trip_through_real_oxigeo_ml() {
        use oxigeo_core::buffer::RasterBuffer;
        use oxigeo_core::types::RasterDataType;
        use oxigeo_ml::OnnxModel;

        let bytes = build_relu_model_bytes(4, 4);
        let path = write_temp_model(&bytes, "unit-test").expect("write temp fixture");

        let mut model =
            OnnxModel::from_file(&path).expect("real oxionnx parser must load the fixture");

        let mut buffer = RasterBuffer::zeros(4, 4, RasterDataType::Float32);
        // Mix of negative and positive values so Relu's clamping is observable.
        for y in 0..4u64 {
            for x in 0..4u64 {
                let v = (x as f64) - 2.0 - (y as f64) * 0.25;
                buffer.set_pixel(x, y, v).expect("set pixel");
            }
        }

        let output = model.infer(&buffer).expect("real forward pass");
        for y in 0..4u64 {
            for x in 0..4u64 {
                let input_v = buffer.get_pixel(x, y).expect("get input pixel");
                let output_v = output.get_pixel(x, y).expect("get output pixel");
                assert!(
                    (output_v - input_v.max(0.0)).abs() < 1e-6,
                    "Relu(x) must clamp negatives to 0 and pass positives through unchanged"
                );
            }
        }

        let _ = std::fs::remove_file(&path);
    }
}
