//! # Pure-Rust ONNX Export
//!
//! A minimal, dependency-free ONNX protobuf **writer** for the model
//! architectures produced by this crate (UNet, ResNet).
//!
//! ## Why hand-written?
//!
//! The COOLJAPAN ONNX stack (`oxionnx-proto`) is decode-only: it can parse an
//! `.onnx` file but exposes no general-purpose encoder. Rather than pull in a
//! protobuf code generator (`prost` and friends), this module implements the
//! small slice of the proto3 wire format that ONNX needs:
//!
//! - **Varint** (wire type 0): `int32`, `int64`, `bool`, `enum`
//! - **32-bit fixed** (wire type 5): `float`
//! - **Length-delimited** (wire type 2): strings, bytes, nested messages
//!
//! From those primitives it assembles the ONNX message hierarchy
//! `ModelProto → GraphProto → {NodeProto, TensorProto, ValueInfoProto}` with
//! `ir_version = 8` and opset 13, emitting standard operators
//! (`Conv`, `Relu`, `MaxPool`, `ConvTranspose`, `Concat`, `Sigmoid`, `Add`,
//! `GlobalAveragePool`, `Flatten`, `Gemm`).
//!
//! Weight tensors are written as `TensorProto.raw_data` (little-endian `f32`).
//! Trainable values can be supplied through a [`WeightMap`]; when omitted, the
//! initializers are zero-filled but carry the correct shapes, so the exported
//! graph still round-trips through an ONNX loader with the full architecture.
//!
//! The tensor names follow the same convention as the autograd backend
//! (`initial_conv_w`, `block{N}_conv1_w`, `enc{L}_conv1_w`, `bottleneck_w`,
//! `fc_w`, …), so weights trained there can be slotted straight in.
//!
//! ## Weight serialization
//!
//! [`serialize_named_weights`] / [`deserialize_named_weights`] provide a small,
//! self-describing, little-endian `f32` checkpoint format (magic + version +
//! per-tensor name/length/data) for round-tripping raw parameter values without
//! ONNX overhead.

use crate::error::{Error, Result};
use crate::models::resnet::ResNetConfig;
use crate::models::unet::UNetConfig;
use std::collections::BTreeMap;
use std::path::Path;

/// Named trainable weights keyed by tensor name, values are flat little-endian
/// `f32` data laid out in row-major (C) order matching the initializer shape.
pub type WeightMap = BTreeMap<String, Vec<f32>>;

/// ONNX IR version emitted by this writer (opset-13 era).
const IR_VERSION: u64 = 8;
/// Default ONNX operator-set version.
const OPSET_VERSION: u64 = 13;
/// ONNX `TensorProto.DataType::FLOAT`.
const DT_FLOAT: u64 = 1;

// ONNX `AttributeProto.AttributeType` discriminants.
const AT_FLOAT: u64 = 1;
const AT_INT: u64 = 2;
const AT_STRING: u64 = 3;
const AT_INTS: u64 = 7;

// ─────────────────────────────────────────────────────────────────
// proto3 wire-format primitives
// ─────────────────────────────────────────────────────────────────

/// Append a base-128 varint (proto wire type 0).
fn write_varint(buf: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            buf.push(byte);
            break;
        }
        buf.push(byte | 0x80);
    }
}

/// Append a field tag = `(field_number << 3) | wire_type`.
fn write_tag(buf: &mut Vec<u8>, field: u32, wire_type: u8) {
    write_varint(buf, ((field << 3) | wire_type as u32) as u64);
}

/// Append a varint field (wire type 0).
fn write_varint_field(buf: &mut Vec<u8>, field: u32, value: u64) {
    write_tag(buf, field, 0);
    write_varint(buf, value);
}

/// Append a 32-bit fixed field (wire type 5) carrying an `f32`.
fn write_fixed32_field(buf: &mut Vec<u8>, field: u32, value: f32) {
    write_tag(buf, field, 5);
    buf.extend_from_slice(&value.to_le_bytes());
}

/// Append a length-delimited field (wire type 2) with an explicit byte payload.
fn write_len_field(buf: &mut Vec<u8>, field: u32, data: &[u8]) {
    write_tag(buf, field, 2);
    write_varint(buf, data.len() as u64);
    buf.extend_from_slice(data);
}

/// Append a length-delimited field carrying a UTF-8 string.
fn write_string_field(buf: &mut Vec<u8>, field: u32, value: &str) {
    write_len_field(buf, field, value.as_bytes());
}

// ─────────────────────────────────────────────────────────────────
// In-memory ONNX graph representation
// ─────────────────────────────────────────────────────────────────

/// A single tensor dimension in a `ValueInfoProto` shape.
#[derive(Debug, Clone)]
pub enum Dim {
    /// Concrete, statically known size.
    Static(i64),
    /// Symbolic / dynamic dimension (e.g. `"batch"`), carrying its parameter name.
    Dynamic(String),
}

/// A typed ONNX node attribute value.
#[derive(Debug, Clone)]
pub enum AttrValue {
    /// Single `int64` (`AttributeType::INT`).
    Int(i64),
    /// Repeated `int64` (`AttributeType::INTS`).
    Ints(Vec<i64>),
    /// Single `float` (`AttributeType::FLOAT`).
    Float(f32),
    /// Single UTF-8 string (`AttributeType::STRING`).
    Str(String),
}

/// An ONNX computation node (`NodeProto`).
#[derive(Debug, Clone)]
pub struct Node {
    /// Operator type, e.g. `"Conv"`.
    pub op_type: String,
    /// Unique node name.
    pub name: String,
    /// Input tensor names (initializers, graph inputs, or upstream outputs).
    pub inputs: Vec<String>,
    /// Output tensor names.
    pub outputs: Vec<String>,
    /// Node attributes as `(name, value)` pairs.
    pub attributes: Vec<(String, AttrValue)>,
}

/// A constant weight tensor stored as a graph initializer (`TensorProto`).
#[derive(Debug, Clone)]
pub struct Initializer {
    /// Tensor name.
    pub name: String,
    /// Tensor shape.
    pub dims: Vec<i64>,
    /// Flat `f32` data in row-major order (`dims.product()` elements).
    pub data: Vec<f32>,
}

/// A graph input/output declaration (`ValueInfoProto`), always `f32`-typed.
#[derive(Debug, Clone)]
pub struct ValueInfo {
    /// Tensor name.
    pub name: String,
    /// Per-dimension shape.
    pub dims: Vec<Dim>,
}

/// A complete ONNX graph ready to be serialized into a `ModelProto`.
#[derive(Debug, Clone, Default)]
pub struct OnnxGraph {
    /// Graph name.
    pub name: String,
    /// Ordered computation nodes.
    pub nodes: Vec<Node>,
    /// Constant weight tensors.
    pub initializers: Vec<Initializer>,
    /// Graph inputs.
    pub inputs: Vec<ValueInfo>,
    /// Graph outputs.
    pub outputs: Vec<ValueInfo>,
}

impl OnnxGraph {
    /// Serialize this graph into a full ONNX `ModelProto` byte buffer.
    pub fn to_model_bytes(&self) -> Vec<u8> {
        let graph_bytes = encode_graph(self);

        let mut buf = Vec::new();
        // ModelProto.ir_version = 1
        write_varint_field(&mut buf, 1, IR_VERSION);
        // ModelProto.producer_name = 2
        write_string_field(&mut buf, 2, "oxigeo-ml-foundation");
        // ModelProto.producer_version = 3
        write_string_field(&mut buf, 3, crate::VERSION);
        // ModelProto.graph = 7
        write_len_field(&mut buf, 7, &graph_bytes);
        // ModelProto.opset_import = 8 (OperatorSetIdProto: version at field 2, default domain)
        let mut opset = Vec::new();
        write_varint_field(&mut opset, 2, OPSET_VERSION);
        write_len_field(&mut buf, 8, &opset);
        buf
    }
}

// ─────────────────────────────────────────────────────────────────
// message encoders
// ─────────────────────────────────────────────────────────────────

fn encode_initializer(init: &Initializer) -> Vec<u8> {
    let mut buf = Vec::new();
    // TensorProto.dims = 1 (repeated int64, one field per dim)
    for &d in &init.dims {
        write_varint_field(&mut buf, 1, d as u64);
    }
    // TensorProto.data_type = 2
    write_varint_field(&mut buf, 2, DT_FLOAT);
    // TensorProto.name = 8
    write_string_field(&mut buf, 8, &init.name);
    // TensorProto.raw_data = 9 (little-endian f32)
    let mut raw = Vec::with_capacity(init.data.len() * 4);
    for &x in &init.data {
        raw.extend_from_slice(&x.to_le_bytes());
    }
    write_len_field(&mut buf, 9, &raw);
    buf
}

fn encode_value_info(vi: &ValueInfo) -> Vec<u8> {
    // TensorShapeProto: repeated Dimension at field 1.
    let mut shape_buf = Vec::new();
    for dim in &vi.dims {
        let mut dim_buf = Vec::new();
        match dim {
            // Dimension.dim_value = 1
            Dim::Static(v) => write_varint_field(&mut dim_buf, 1, *v as u64),
            // Dimension.dim_param = 2
            Dim::Dynamic(name) => write_string_field(&mut dim_buf, 2, name),
        }
        write_len_field(&mut shape_buf, 1, &dim_buf);
    }

    // TypeProto.Tensor: elem_type (1) + shape (2).
    let mut tensor_buf = Vec::new();
    write_varint_field(&mut tensor_buf, 1, DT_FLOAT);
    write_len_field(&mut tensor_buf, 2, &shape_buf);

    // TypeProto: tensor_type = 1.
    let mut type_buf = Vec::new();
    write_len_field(&mut type_buf, 1, &tensor_buf);

    // ValueInfoProto: name (1) + type (2).
    let mut buf = Vec::new();
    write_string_field(&mut buf, 1, &vi.name);
    write_len_field(&mut buf, 2, &type_buf);
    buf
}

fn encode_attribute(name: &str, value: &AttrValue) -> Vec<u8> {
    let mut buf = Vec::new();
    // AttributeProto.name = 1
    write_string_field(&mut buf, 1, name);
    match value {
        AttrValue::Float(f) => {
            // AttributeProto.f = 2
            write_fixed32_field(&mut buf, 2, *f);
            write_varint_field(&mut buf, 20, AT_FLOAT);
        }
        AttrValue::Int(i) => {
            // AttributeProto.i = 3
            write_varint_field(&mut buf, 3, *i as u64);
            write_varint_field(&mut buf, 20, AT_INT);
        }
        AttrValue::Str(s) => {
            // AttributeProto.s = 4
            write_string_field(&mut buf, 4, s);
            write_varint_field(&mut buf, 20, AT_STRING);
        }
        AttrValue::Ints(values) => {
            // AttributeProto.ints = 8 (repeated varint)
            for &v in values {
                write_varint_field(&mut buf, 8, v as u64);
            }
            write_varint_field(&mut buf, 20, AT_INTS);
        }
    }
    buf
}

fn encode_node(node: &Node) -> Vec<u8> {
    let mut buf = Vec::new();
    // NodeProto.input = 1 (repeated string)
    for input in &node.inputs {
        write_string_field(&mut buf, 1, input);
    }
    // NodeProto.output = 2 (repeated string)
    for output in &node.outputs {
        write_string_field(&mut buf, 2, output);
    }
    // NodeProto.name = 3
    write_string_field(&mut buf, 3, &node.name);
    // NodeProto.op_type = 4
    write_string_field(&mut buf, 4, &node.op_type);
    // NodeProto.attribute = 5 (repeated)
    for (attr_name, attr_value) in &node.attributes {
        let attr_bytes = encode_attribute(attr_name, attr_value);
        write_len_field(&mut buf, 5, &attr_bytes);
    }
    buf
}

fn encode_graph(graph: &OnnxGraph) -> Vec<u8> {
    let mut buf = Vec::new();
    // GraphProto.node = 1 (repeated)
    for node in &graph.nodes {
        let node_bytes = encode_node(node);
        write_len_field(&mut buf, 1, &node_bytes);
    }
    // GraphProto.name = 2
    write_string_field(&mut buf, 2, &graph.name);
    // GraphProto.initializer = 5 (repeated)
    for init in &graph.initializers {
        let init_bytes = encode_initializer(init);
        write_len_field(&mut buf, 5, &init_bytes);
    }
    // GraphProto.input = 11 (repeated ValueInfoProto)
    for vi in &graph.inputs {
        let vi_bytes = encode_value_info(vi);
        write_len_field(&mut buf, 11, &vi_bytes);
    }
    // GraphProto.output = 12 (repeated ValueInfoProto)
    for vi in &graph.outputs {
        let vi_bytes = encode_value_info(vi);
        write_len_field(&mut buf, 12, &vi_bytes);
    }
    buf
}

// ─────────────────────────────────────────────────────────────────
// graph builder
// ─────────────────────────────────────────────────────────────────

/// Accumulates nodes and initializers while assigning unique names.
struct GraphBuilder {
    nodes: Vec<Node>,
    inits: Vec<Initializer>,
    counter: usize,
}

impl GraphBuilder {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            inits: Vec::new(),
            counter: 0,
        }
    }

    /// Register an initializer with the given shape, sourcing values from
    /// `weights` when present (validating the element count) or zero-filling.
    fn add_init(&mut self, weights: Option<&WeightMap>, name: &str, dims: &[usize]) -> Result<()> {
        let count: usize = dims.iter().product();
        let data = match weights.and_then(|w| w.get(name)) {
            Some(values) => {
                if values.len() != count {
                    return Err(Error::Backend(format!(
                        "ONNX export: weight '{name}' has {} values but architecture expects {count}",
                        values.len()
                    )));
                }
                values.clone()
            }
            None => vec![0.0f32; count],
        };
        self.inits.push(Initializer {
            name: name.to_string(),
            dims: dims.iter().map(|&d| d as i64).collect(),
            data,
        });
        Ok(())
    }

    /// Emit a node with an auto-generated output tensor name and return it.
    fn push(
        &mut self,
        op: &str,
        inputs: Vec<String>,
        attributes: Vec<(String, AttrValue)>,
    ) -> String {
        let id = self.counter;
        self.counter += 1;
        let output = format!("t{id}");
        self.nodes.push(Node {
            op_type: op.to_string(),
            name: format!("{op}_{id}"),
            inputs,
            outputs: vec![output.clone()],
            attributes,
        });
        output
    }

    /// Emit a node writing to an explicit (named) output tensor.
    fn push_to(
        &mut self,
        op: &str,
        inputs: Vec<String>,
        attributes: Vec<(String, AttrValue)>,
        output: &str,
    ) {
        let id = self.counter;
        self.counter += 1;
        self.nodes.push(Node {
            op_type: op.to_string(),
            name: format!("{op}_{id}"),
            inputs,
            outputs: vec![output.to_string()],
            attributes,
        });
    }

    fn conv_attrs(kernel: i64, stride: i64, pad: i64) -> Vec<(String, AttrValue)> {
        vec![
            (
                "kernel_shape".to_string(),
                AttrValue::Ints(vec![kernel, kernel]),
            ),
            ("strides".to_string(), AttrValue::Ints(vec![stride, stride])),
            (
                "pads".to_string(),
                AttrValue::Ints(vec![pad, pad, pad, pad]),
            ),
            ("dilations".to_string(), AttrValue::Ints(vec![1, 1])),
            ("group".to_string(), AttrValue::Int(1)),
        ]
    }

    fn conv(
        &mut self,
        input: &str,
        weight: &str,
        bias: &str,
        kernel: i64,
        stride: i64,
        pad: i64,
    ) -> String {
        self.push(
            "Conv",
            vec![input.to_string(), weight.to_string(), bias.to_string()],
            Self::conv_attrs(kernel, stride, pad),
        )
    }

    fn conv_transpose(&mut self, input: &str, weight: &str, kernel: i64, stride: i64) -> String {
        self.push(
            "ConvTranspose",
            vec![input.to_string(), weight.to_string()],
            vec![
                (
                    "kernel_shape".to_string(),
                    AttrValue::Ints(vec![kernel, kernel]),
                ),
                ("strides".to_string(), AttrValue::Ints(vec![stride, stride])),
                ("pads".to_string(), AttrValue::Ints(vec![0, 0, 0, 0])),
                ("group".to_string(), AttrValue::Int(1)),
            ],
        )
    }

    fn relu(&mut self, input: &str) -> String {
        self.push("Relu", vec![input.to_string()], Vec::new())
    }

    fn maxpool(&mut self, input: &str, kernel: i64, stride: i64, pad: i64) -> String {
        self.push(
            "MaxPool",
            vec![input.to_string()],
            vec![
                (
                    "kernel_shape".to_string(),
                    AttrValue::Ints(vec![kernel, kernel]),
                ),
                ("strides".to_string(), AttrValue::Ints(vec![stride, stride])),
                (
                    "pads".to_string(),
                    AttrValue::Ints(vec![pad, pad, pad, pad]),
                ),
            ],
        )
    }

    fn concat(&mut self, inputs: &[String], axis: i64) -> String {
        self.push(
            "Concat",
            inputs.to_vec(),
            vec![("axis".to_string(), AttrValue::Int(axis))],
        )
    }

    fn add(&mut self, a: &str, b: &str) -> String {
        self.push("Add", vec![a.to_string(), b.to_string()], Vec::new())
    }

    fn global_avg_pool(&mut self, input: &str) -> String {
        self.push("GlobalAveragePool", vec![input.to_string()], Vec::new())
    }

    fn flatten(&mut self, input: &str, axis: i64) -> String {
        self.push(
            "Flatten",
            vec![input.to_string()],
            vec![("axis".to_string(), AttrValue::Int(axis))],
        )
    }
}

// ─────────────────────────────────────────────────────────────────
// UNet
// ─────────────────────────────────────────────────────────────────

/// Build an ONNX graph for the given [`UNetConfig`].
///
/// The encoder/decoder follow the standard UNet topology: each encoder level is
/// a `Conv → Relu → Conv → Relu → MaxPool` block, the bottleneck is a single
/// `Conv → Relu`, and each decoder level up-samples with a `ConvTranspose`,
/// concatenates the matching skip connection, then applies `Conv → Relu → Conv
/// → Relu`. A `1×1 Conv` followed by `Sigmoid` produces the segmentation map.
///
/// Weight tensors are named `enc{L}_conv1_w/_b`, `enc{L}_conv2_w/_b`,
/// `bottleneck_w/_b`, `dec{L}_up_w`, `dec{L}_conv1_w/_b`, `dec{L}_conv2_w/_b`
/// and `final_w/_b`. Supply trained values through `weights` or leave `None` to
/// zero-fill.
pub fn build_unet_graph(config: &UNetConfig, weights: Option<&WeightMap>) -> Result<OnnxGraph> {
    if config.depth == 0 {
        return Err(Error::Backend(
            "ONNX export: UNet depth must be >= 1".to_string(),
        ));
    }

    let base = config.base_filters;
    let depth = config.depth;
    let mut b = GraphBuilder::new();

    let mut cur = "input".to_string();
    // (skip tensor name, channel count) per encoder level.
    let mut skips: Vec<(String, usize)> = Vec::with_capacity(depth);

    // Encoder.
    for level in 0..depth {
        let in_ch = if level == 0 {
            config.in_channels
        } else {
            base * (1 << (level - 1))
        };
        let out_ch = base * (1 << level);

        let w1 = format!("enc{level}_conv1_w");
        let b1 = format!("enc{level}_conv1_b");
        b.add_init(weights, &w1, &[out_ch, in_ch, 3, 3])?;
        b.add_init(weights, &b1, &[out_ch])?;
        let y = b.conv(&cur, &w1, &b1, 3, 1, 1);
        cur = b.relu(&y);

        let w2 = format!("enc{level}_conv2_w");
        let b2 = format!("enc{level}_conv2_b");
        b.add_init(weights, &w2, &[out_ch, out_ch, 3, 3])?;
        b.add_init(weights, &b2, &[out_ch])?;
        let y2 = b.conv(&cur, &w2, &b2, 3, 1, 1);
        cur = b.relu(&y2);

        skips.push((cur.clone(), out_ch));
        cur = b.maxpool(&cur, 2, 2, 0);
    }

    // Bottleneck.
    let bn_in = base * (1 << (depth - 1));
    let bn_out = base * (1 << depth);
    b.add_init(weights, "bottleneck_w", &[bn_out, bn_in, 3, 3])?;
    b.add_init(weights, "bottleneck_b", &[bn_out])?;
    let yb = b.conv(&cur, "bottleneck_w", "bottleneck_b", 3, 1, 1);
    cur = b.relu(&yb);
    let mut cur_ch = bn_out;

    // Decoder.
    for level in (0..depth).rev() {
        let out_ch = base * (1 << level);

        // ConvTranspose upsample: weight [in_channels, out_channels, 2, 2].
        let up_w = format!("dec{level}_up_w");
        b.add_init(weights, &up_w, &[cur_ch, out_ch, 2, 2])?;
        let up = b.conv_transpose(&cur, &up_w, 2, 2);

        // Concatenate with the encoder skip connection along the channel axis.
        let (skip_name, skip_ch) = skips[level].clone();
        let cat = b.concat(&[up, skip_name], 1);
        let concat_ch = out_ch + skip_ch;

        let w1 = format!("dec{level}_conv1_w");
        let b1 = format!("dec{level}_conv1_b");
        b.add_init(weights, &w1, &[out_ch, concat_ch, 3, 3])?;
        b.add_init(weights, &b1, &[out_ch])?;
        let y1 = b.conv(&cat, &w1, &b1, 3, 1, 1);
        let r1 = b.relu(&y1);

        let w2 = format!("dec{level}_conv2_w");
        let b2 = format!("dec{level}_conv2_b");
        b.add_init(weights, &w2, &[out_ch, out_ch, 3, 3])?;
        b.add_init(weights, &b2, &[out_ch])?;
        let y2 = b.conv(&r1, &w2, &b2, 3, 1, 1);
        cur = b.relu(&y2);
        cur_ch = out_ch;
    }

    // Final 1x1 conv + sigmoid.
    b.add_init(weights, "final_w", &[config.num_classes, base, 1, 1])?;
    b.add_init(weights, "final_b", &[config.num_classes])?;
    let yf = b.conv(&cur, "final_w", "final_b", 1, 1, 0);
    b.push_to("Sigmoid", vec![yf], Vec::new(), "output");

    let inputs = vec![ValueInfo {
        name: "input".to_string(),
        dims: vec![
            Dim::Dynamic("batch".to_string()),
            Dim::Static(config.in_channels as i64),
            Dim::Dynamic("height".to_string()),
            Dim::Dynamic("width".to_string()),
        ],
    }];
    let outputs = vec![ValueInfo {
        name: "output".to_string(),
        dims: vec![
            Dim::Dynamic("batch".to_string()),
            Dim::Static(config.num_classes as i64),
            Dim::Dynamic("height".to_string()),
            Dim::Dynamic("width".to_string()),
        ],
    }];

    Ok(OnnxGraph {
        name: "unet".to_string(),
        nodes: b.nodes,
        initializers: b.inits,
        inputs,
        outputs,
    })
}

// ─────────────────────────────────────────────────────────────────
// ResNet
// ─────────────────────────────────────────────────────────────────

/// Build an ONNX graph for the given [`ResNetConfig`].
///
/// The stem is `Conv(7×7, stride 2) → Relu → MaxPool(3×3, stride 2)`. Each
/// residual block is `Conv → Relu → Conv (+ optional 1×1 shortcut Conv) → Add →
/// Relu`, with a stride-2 first block per stage for down-sampling. The head is
/// `GlobalAveragePool → Flatten → Gemm`.
///
/// Weight tensors are named `initial_conv_w/_b`, `block{N}_conv1_w/_b`,
/// `block{N}_conv2_w/_b`, `block{N}_shortcut_w/_b` and `fc_w/_b`.
pub fn build_resnet_graph(config: &ResNetConfig, weights: Option<&WeightMap>) -> Result<OnnxGraph> {
    let base = config.base_filters;
    if base == 0 {
        return Err(Error::Backend(
            "ONNX export: ResNet base_filters must be >= 1".to_string(),
        ));
    }
    let stages = config.variant.blocks_per_stage();
    let mut b = GraphBuilder::new();

    // Stem.
    b.add_init(weights, "initial_conv_w", &[base, config.in_channels, 7, 7])?;
    b.add_init(weights, "initial_conv_b", &[base])?;
    let stem = b.conv("input", "initial_conv_w", "initial_conv_b", 7, 2, 3);
    let stem_relu = b.relu(&stem);
    let mut cur = b.maxpool(&stem_relu, 3, 2, 1);

    // Residual stages.
    let mut in_ch = base;
    let mut block_idx = 0usize;
    for (stage_idx, &num_blocks) in stages.iter().enumerate() {
        let out_ch = base * (1 << stage_idx);
        let stage_stride = if stage_idx == 0 { 1 } else { 2 };

        for block in 0..num_blocks {
            let block_stride = if block == 0 { stage_stride } else { 1 };
            let block_in = if block == 0 { in_ch } else { out_ch };

            let w1 = format!("block{block_idx}_conv1_w");
            let b1 = format!("block{block_idx}_conv1_b");
            b.add_init(weights, &w1, &[out_ch, block_in, 3, 3])?;
            b.add_init(weights, &b1, &[out_ch])?;
            let c1 = b.conv(&cur, &w1, &b1, 3, block_stride, 1);
            let r1 = b.relu(&c1);

            let w2 = format!("block{block_idx}_conv2_w");
            let b2 = format!("block{block_idx}_conv2_b");
            b.add_init(weights, &w2, &[out_ch, out_ch, 3, 3])?;
            b.add_init(weights, &b2, &[out_ch])?;
            let c2 = b.conv(&r1, &w2, &b2, 3, 1, 1);

            // Projection shortcut when channels or spatial resolution change.
            let shortcut = if block_in != out_ch || block_stride != 1 {
                let ws = format!("block{block_idx}_shortcut_w");
                let bs = format!("block{block_idx}_shortcut_b");
                b.add_init(weights, &ws, &[out_ch, block_in, 1, 1])?;
                b.add_init(weights, &bs, &[out_ch])?;
                b.conv(&cur, &ws, &bs, 1, block_stride, 0)
            } else {
                cur.clone()
            };

            let summed = b.add(&c2, &shortcut);
            cur = b.relu(&summed);
            block_idx += 1;
        }

        in_ch = out_ch;
    }

    // Head: global average pool → flatten → Gemm.
    let pooled = b.global_avg_pool(&cur);
    let flat = b.flatten(&pooled, 1);
    let fc_in = base * (1 << (stages.len() - 1));
    b.add_init(weights, "fc_w", &[config.num_classes, fc_in])?;
    b.add_init(weights, "fc_b", &[config.num_classes])?;
    b.push_to(
        "Gemm",
        vec![flat, "fc_w".to_string(), "fc_b".to_string()],
        vec![
            ("alpha".to_string(), AttrValue::Float(1.0)),
            ("beta".to_string(), AttrValue::Float(1.0)),
            ("transA".to_string(), AttrValue::Int(0)),
            ("transB".to_string(), AttrValue::Int(1)),
        ],
        "output",
    );

    let inputs = vec![ValueInfo {
        name: "input".to_string(),
        dims: vec![
            Dim::Dynamic("batch".to_string()),
            Dim::Static(config.in_channels as i64),
            Dim::Dynamic("height".to_string()),
            Dim::Dynamic("width".to_string()),
        ],
    }];
    let outputs = vec![ValueInfo {
        name: "output".to_string(),
        dims: vec![
            Dim::Dynamic("batch".to_string()),
            Dim::Static(config.num_classes as i64),
        ],
    }];

    Ok(OnnxGraph {
        name: "resnet".to_string(),
        nodes: b.nodes,
        initializers: b.inits,
        inputs,
        outputs,
    })
}

// ─────────────────────────────────────────────────────────────────
// public export entry points
// ─────────────────────────────────────────────────────────────────

/// Serialize a UNet architecture to ONNX `ModelProto` bytes.
pub fn export_unet_bytes(config: &UNetConfig, weights: Option<&WeightMap>) -> Result<Vec<u8>> {
    Ok(build_unet_graph(config, weights)?.to_model_bytes())
}

/// Serialize a ResNet architecture to ONNX `ModelProto` bytes.
pub fn export_resnet_bytes(config: &ResNetConfig, weights: Option<&WeightMap>) -> Result<Vec<u8>> {
    Ok(build_resnet_graph(config, weights)?.to_model_bytes())
}

/// Write a UNet architecture to an `.onnx` file at `path`.
pub fn export_unet_to_path(
    config: &UNetConfig,
    weights: Option<&WeightMap>,
    path: &Path,
) -> Result<()> {
    let bytes = export_unet_bytes(config, weights)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

/// Write a ResNet architecture to an `.onnx` file at `path`.
pub fn export_resnet_to_path(
    config: &ResNetConfig,
    weights: Option<&WeightMap>,
    path: &Path,
) -> Result<()> {
    let bytes = export_resnet_bytes(config, weights)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────
// weight checkpoint serialization (raw little-endian f32)
// ─────────────────────────────────────────────────────────────────

/// Magic prefix identifying an OxiGeo weight checkpoint (`OXW1`).
const WEIGHT_MAGIC: &[u8; 4] = b"OXW1";
/// Weight checkpoint format version.
const WEIGHT_FORMAT_VERSION: u32 = 1;

/// Serialize a [`WeightMap`] into a self-describing little-endian byte buffer.
///
/// Layout: `magic(4) | version(u32) | count(u32)` then, per entry (ordered by
/// name), `name_len(u32) | name(utf8) | data_len(u32) | data(f32 × data_len)`.
pub fn serialize_named_weights(weights: &WeightMap) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(WEIGHT_MAGIC);
    buf.extend_from_slice(&WEIGHT_FORMAT_VERSION.to_le_bytes());
    buf.extend_from_slice(&(weights.len() as u32).to_le_bytes());
    for (name, data) in weights {
        let name_bytes = name.as_bytes();
        buf.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(name_bytes);
        buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
        for &x in data {
            buf.extend_from_slice(&x.to_le_bytes());
        }
    }
    buf
}

/// Read a `u32` (little-endian), advancing `pos`.
fn read_u32(bytes: &[u8], pos: &mut usize) -> Result<u32> {
    let end = *pos + 4;
    if end > bytes.len() {
        return Err(Error::Serialization(
            "weight checkpoint: unexpected end of buffer reading u32".to_string(),
        ));
    }
    let value = u32::from_le_bytes([
        bytes[*pos],
        bytes[*pos + 1],
        bytes[*pos + 2],
        bytes[*pos + 3],
    ]);
    *pos = end;
    Ok(value)
}

/// Borrow `len` bytes, advancing `pos`.
fn read_slice<'a>(bytes: &'a [u8], pos: &mut usize, len: usize) -> Result<&'a [u8]> {
    let end = pos
        .checked_add(len)
        .ok_or_else(|| Error::Serialization("weight checkpoint: length overflow".to_string()))?;
    if end > bytes.len() {
        return Err(Error::Serialization(
            "weight checkpoint: unexpected end of buffer".to_string(),
        ));
    }
    let slice = &bytes[*pos..end];
    *pos = end;
    Ok(slice)
}

/// Deserialize a [`WeightMap`] previously produced by [`serialize_named_weights`].
pub fn deserialize_named_weights(bytes: &[u8]) -> Result<WeightMap> {
    let mut pos = 0usize;
    let magic = read_slice(bytes, &mut pos, 4)?;
    if magic != WEIGHT_MAGIC {
        return Err(Error::Serialization(
            "weight checkpoint: bad magic (not an OxiGeo weight file)".to_string(),
        ));
    }
    let version = read_u32(bytes, &mut pos)?;
    if version != WEIGHT_FORMAT_VERSION {
        return Err(Error::Serialization(format!(
            "weight checkpoint: unsupported version {version}"
        )));
    }
    let count = read_u32(bytes, &mut pos)? as usize;
    let mut map = WeightMap::new();
    for _ in 0..count {
        let name_len = read_u32(bytes, &mut pos)? as usize;
        let name_bytes = read_slice(bytes, &mut pos, name_len)?;
        let name = String::from_utf8(name_bytes.to_vec()).map_err(|e| {
            Error::Serialization(format!("weight checkpoint: invalid utf-8 name: {e}"))
        })?;
        let data_len = read_u32(bytes, &mut pos)? as usize;
        let raw = read_slice(bytes, &mut pos, data_len * 4)?;
        let data = raw
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        map.insert(name, data);
    }
    Ok(map)
}

/// Write a [`WeightMap`] checkpoint to `path`.
pub fn save_weights_to_path(weights: &WeightMap, path: &Path) -> Result<()> {
    std::fs::write(path, serialize_named_weights(weights))?;
    Ok(())
}

/// Read a [`WeightMap`] checkpoint from `path`.
pub fn load_weights_from_path(path: &Path) -> Result<WeightMap> {
    let bytes = std::fs::read(path)?;
    deserialize_named_weights(&bytes)
}

// ─────────────────────────────────────────────────────────────────
// tests
// ─────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::models::resnet::ResNetVariant;

    fn count_op(model_nodes: &[oxionnx::proto::NodeProto], op: &str) -> usize {
        model_nodes.iter().filter(|n| n.op_type == op).count()
    }

    #[test]
    fn test_varint_roundtrip() {
        for &value in &[0u64, 1, 127, 128, 300, 16_384, u32::MAX as u64] {
            let mut buf = Vec::new();
            write_varint(&mut buf, value);
            let (decoded, pos) =
                oxionnx::proto::read_varint(&buf, 0).expect("varint should decode");
            assert_eq!(decoded, value);
            assert_eq!(pos, buf.len());
        }
    }

    #[test]
    fn test_unet_onnx_roundtrip() {
        let config = UNetConfig {
            in_channels: 3,
            num_classes: 2,
            base_filters: 4,
            depth: 2,
            ..UNetConfig::default()
        };

        let bytes = export_unet_bytes(&config, None).expect("unet export");
        let model = oxionnx::proto::parse_model(&bytes).expect("model should parse");

        // Model-level metadata.
        assert_eq!(model.ir_version, 8);
        assert_eq!(model.opset_version, 13);

        // Node count: encoder 5·depth + bottleneck 2 + decoder 6·depth + final 2.
        let expected_nodes = 5 * config.depth + 2 + 6 * config.depth + 2;
        assert_eq!(model.graph.nodes.len(), expected_nodes);
        assert_eq!(model.graph.nodes.len(), 26);

        // Standard UNet operators are all present.
        assert!(count_op(&model.graph.nodes, "Conv") > 0);
        assert!(count_op(&model.graph.nodes, "Relu") > 0);
        assert_eq!(count_op(&model.graph.nodes, "MaxPool"), config.depth);
        assert_eq!(count_op(&model.graph.nodes, "ConvTranspose"), config.depth);
        assert_eq!(count_op(&model.graph.nodes, "Concat"), config.depth);
        assert_eq!(count_op(&model.graph.nodes, "Sigmoid"), 1);

        // Named initializers with the expected shapes.
        let find = |name: &str| {
            model
                .graph
                .initializers
                .iter()
                .find(|t| t.name == name)
                .unwrap_or_else(|| panic!("missing initializer {name}"))
        };
        assert_eq!(find("enc0_conv1_w").dims, vec![4, 3, 3, 3]);
        assert_eq!(find("enc1_conv1_w").dims, vec![8, 4, 3, 3]);
        assert_eq!(find("bottleneck_w").dims, vec![16, 8, 3, 3]);
        assert_eq!(find("dec1_up_w").dims, vec![16, 8, 2, 2]);
        assert_eq!(find("dec1_conv1_w").dims, vec![8, 16, 3, 3]);
        assert_eq!(find("final_w").dims, vec![2, 4, 1, 1]);

        // Graph I/O names and shapes.
        assert_eq!(model.graph.inputs, vec!["input".to_string()]);
        assert_eq!(model.graph.outputs, vec!["output".to_string()]);
        let input_info = &model.graph.input_value_infos[0];
        // [batch(dynamic), channels=3, H(dynamic), W(dynamic)]
        assert_eq!(input_info.shape.len(), 4);
        assert_eq!(input_info.shape[0], None);
        assert_eq!(input_info.shape[1], Some(3));
        let output_info = &model.graph.output_value_infos[0];
        assert_eq!(output_info.shape[1], Some(2));
    }

    #[test]
    fn test_resnet_onnx_roundtrip() {
        let config = ResNetConfig {
            variant: ResNetVariant::ResNet18,
            in_channels: 3,
            num_classes: 5,
            base_filters: 2,
            ..ResNetConfig::default()
        };

        let bytes = export_resnet_bytes(&config, None).expect("resnet export");
        let model = oxionnx::proto::parse_model(&bytes).expect("model should parse");

        assert_eq!(model.ir_version, 8);
        assert_eq!(model.opset_version, 13);

        // Stem 3 + blocks (8 blocks: 5×5-node + 3×6-node with shortcut) + head 3.
        assert_eq!(model.graph.nodes.len(), 49);

        // Classification-head operators required by the task are present.
        assert_eq!(count_op(&model.graph.nodes, "GlobalAveragePool"), 1);
        assert_eq!(count_op(&model.graph.nodes, "Gemm"), 1);
        assert_eq!(count_op(&model.graph.nodes, "Flatten"), 1);
        assert_eq!(count_op(&model.graph.nodes, "Add"), 8);
        assert!(count_op(&model.graph.nodes, "Conv") > 0);
        assert_eq!(count_op(&model.graph.nodes, "MaxPool"), 1);

        // Three projection shortcuts (first block of stages 1, 2, 3).
        let shortcut_count = model
            .graph
            .initializers
            .iter()
            .filter(|t| t.name.ends_with("_shortcut_w"))
            .count();
        assert_eq!(shortcut_count, 3);

        let fc = model
            .graph
            .initializers
            .iter()
            .find(|t| t.name == "fc_w")
            .expect("fc_w initializer");
        // fc_in = base_filters * 2^(stages-1) = 2 * 8 = 16, num_classes = 5.
        assert_eq!(fc.dims, vec![5, 16]);

        let output_info = &model.graph.output_value_infos[0];
        assert_eq!(output_info.shape[1], Some(5));
    }

    #[test]
    fn test_export_with_supplied_weights() {
        let config = UNetConfig {
            in_channels: 1,
            num_classes: 1,
            base_filters: 2,
            depth: 1,
            ..UNetConfig::default()
        };

        // enc0_conv1_w has shape [2, 1, 3, 3] = 18 elements.
        let mut weights = WeightMap::new();
        weights.insert(
            "enc0_conv1_w".to_string(),
            (0..18).map(|i| i as f32).collect(),
        );

        let bytes = export_unet_bytes(&config, Some(&weights)).expect("export with weights");
        let model = oxionnx::proto::parse_model(&bytes).expect("parse");

        let tensor = model
            .graph
            .initializers
            .iter()
            .find(|t| t.name == "enc0_conv1_w")
            .expect("enc0_conv1_w");
        let restored = tensor.to_tensor();
        assert_eq!(restored.data.len(), 18);
        assert_eq!(restored.data[0], 0.0);
        assert_eq!(restored.data[17], 17.0);
    }

    #[test]
    fn test_supplied_weights_wrong_length_errors() {
        let config = UNetConfig {
            in_channels: 1,
            num_classes: 1,
            base_filters: 2,
            depth: 1,
            ..UNetConfig::default()
        };
        let mut weights = WeightMap::new();
        weights.insert("enc0_conv1_w".to_string(), vec![1.0, 2.0, 3.0]); // wrong length
        let result = export_unet_bytes(&config, Some(&weights));
        assert!(result.is_err());
    }

    #[test]
    fn test_weight_checkpoint_roundtrip() {
        let mut weights = WeightMap::new();
        weights.insert("conv1_w".to_string(), vec![1.5, -2.25, 3.75, 0.0]);
        weights.insert("conv1_b".to_string(), vec![0.1, 0.2]);
        weights.insert("empty".to_string(), Vec::new());

        let bytes = serialize_named_weights(&weights);
        let restored = deserialize_named_weights(&bytes).expect("deserialize");

        assert_eq!(restored.len(), 3);
        assert_eq!(restored.get("conv1_w"), Some(&vec![1.5, -2.25, 3.75, 0.0]));
        assert_eq!(restored.get("conv1_b"), Some(&vec![0.1, 0.2]));
        assert_eq!(restored.get("empty"), Some(&Vec::new()));
    }

    #[test]
    fn test_weight_checkpoint_file_roundtrip() {
        let mut weights = WeightMap::new();
        weights.insert("w".to_string(), vec![10.0, 20.0, 30.0]);

        let path =
            std::env::temp_dir().join(format!("oxigeo_ml_weights_test_{}.oxw", std::process::id()));
        save_weights_to_path(&weights, &path).expect("save");
        let restored = load_weights_from_path(&path).expect("load");
        let _ = std::fs::remove_file(&path);

        assert_eq!(restored.get("w"), Some(&vec![10.0, 20.0, 30.0]));
    }

    #[test]
    fn test_weight_checkpoint_bad_magic() {
        let result = deserialize_named_weights(b"NOPE\x01\x00\x00\x00");
        assert!(result.is_err());
    }

    #[test]
    fn test_weight_checkpoint_truncated() {
        // Valid header claiming 1 entry, but no entry bytes follow.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(WEIGHT_MAGIC);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        let result = deserialize_named_weights(&bytes);
        assert!(result.is_err());
    }

    /// Regression guard for the decoder-topology consistency bug: the exported
    /// UNet decoder up-samples with a *learned* `ConvTranspose`, and every such
    /// node MUST have a matching trainable `dec{L}_up_w` initializer with the
    /// correct `[in_ch, out_ch, 2, 2]` shape — i.e. no orphan ConvTranspose that
    /// no supplied weight can ever feed. This locks the single canonical
    /// topology after the divergent (parameter-free `upsample2d`) autograd
    /// backend was removed, so a trained checkpoint threaded through
    /// [`build_unet_graph`] reproduces exactly the exported forward graph.
    #[test]
    fn test_unet_decoder_convtranspose_has_matching_trainable_weight() {
        let config = UNetConfig {
            in_channels: 3,
            num_classes: 2,
            base_filters: 4,
            depth: 3,
            ..UNetConfig::default()
        };

        let graph = build_unet_graph(&config, None).expect("build unet graph");

        // Count ConvTranspose nodes directly on the in-memory graph.
        let ct_nodes = graph
            .nodes
            .iter()
            .filter(|n| n.op_type == "ConvTranspose")
            .count();
        assert_eq!(ct_nodes, config.depth);

        // Every decoder level must expose a `dec{L}_up_w` initializer whose shape
        // matches the ConvTranspose weight contract [cur_ch, out_ch, 2, 2].
        let base = config.base_filters;
        for level in 0..config.depth {
            let up_name = format!("dec{level}_up_w");
            let init = graph
                .initializers
                .iter()
                .find(|t| t.name == up_name)
                .unwrap_or_else(|| panic!("missing decoder upsample weight {up_name}"));

            let out_ch = base * (1 << level);
            let cur_ch = if level == config.depth - 1 {
                base * (1 << config.depth) // bottleneck output feeds the top decoder
            } else {
                base * (1 << (level + 1)) // previous decoder level output
            };
            assert_eq!(
                init.dims,
                vec![cur_ch as i64, out_ch as i64, 2, 2],
                "decoder upsample weight {up_name} has wrong shape"
            );
        }

        // There must be no ConvTranspose node whose weight input lacks an
        // initializer (an "orphan" op no checkpoint can feed).
        let init_names: std::collections::BTreeSet<&str> =
            graph.initializers.iter().map(|t| t.name.as_str()).collect();
        for node in graph.nodes.iter().filter(|n| n.op_type == "ConvTranspose") {
            let weight_name = node
                .inputs
                .get(1)
                .expect("ConvTranspose must have a weight input");
            assert!(
                init_names.contains(weight_name.as_str()),
                "ConvTranspose weight {weight_name} has no initializer"
            );
        }
    }

    /// The learned decoder upsample weights are genuinely trainable: values
    /// supplied through a [`WeightMap`] for `dec{L}_up_w` are threaded into the
    /// exported initializer (not silently discarded / zero-filled). This is the
    /// live counterpart to the removed backend `export_onnx` that always passed
    /// `weights: None`.
    #[test]
    fn test_unet_decoder_upsample_weights_are_threaded() {
        let config = UNetConfig {
            in_channels: 1,
            num_classes: 1,
            base_filters: 2,
            depth: 1,
            ..UNetConfig::default()
        };

        // depth==1: dec0_up_w shape = [bottleneck_out=base*2, out=base*1, 2, 2]
        // = [4, 2, 2, 2] = 32 elements.
        let mut weights = WeightMap::new();
        weights.insert(
            "dec0_up_w".to_string(),
            (0..32).map(|i| i as f32 * 0.5).collect(),
        );

        let bytes = export_unet_bytes(&config, Some(&weights)).expect("export with up weights");
        let model = oxionnx::proto::parse_model(&bytes).expect("parse");

        let tensor = model
            .graph
            .initializers
            .iter()
            .find(|t| t.name == "dec0_up_w")
            .expect("dec0_up_w initializer");
        let restored = tensor.to_tensor();
        assert_eq!(restored.data.len(), 32);
        assert_eq!(restored.data[0], 0.0);
        assert_eq!(restored.data[1], 0.5);
        assert_eq!(restored.data[31], 15.5);
    }
}
