//! Mapbox Vector Tile (MVT) generation
//!
//! Implements MVT spec v2.1 (<https://github.com/mapbox/vector-tile-spec>)
//! using a hand-rolled Protocol Buffers encoder — no external `protobuf` crate
//! required, keeping the dependency tree lean.
//!
//! # Wire encoding
//!
//! Protocol Buffers uses a tagged binary format with **wire types**:
//! - 0 (Varint): variable-length integer
//! - 1 (64-bit): fixed 64-bit value
//! - 2 (Length-delimited): length prefix followed by bytes
//! - 5 (32-bit): fixed 32-bit value
//!
//! # MVT structure
//!
//! ```text
//! Tile
//!   └── Layer  (field 3, repeated)
//!         ├── version   (field 15)
//!         ├── name      (field 1)
//!         ├── Feature   (field 2, repeated)
//!         │     ├── id            (field 1, optional)
//!         │     ├── tags          (field 2, packed varint)
//!         │     ├── type          (field 3, varint)
//!         │     └── geometry      (field 4, packed sint32 zigzag)
//!         ├── keys      (field 3, repeated string)
//!         ├── values    (field 4, repeated Value)
//!         └── extent    (field 5)
//! ```
//!
//! # Geometry commands
//!
//! Geometry is encoded as a sequence of drawing commands (MoveTo, LineTo, ClosePath).
//! Coordinates are delta-encoded relative to the previous cursor position, then
//! zigzag-encoded as varints to keep small signed values compact.

use crate::error::ServiceError;

// ─────────────────────────────────────────────────────────────────────────────
// Protocol Buffer primitives
// ─────────────────────────────────────────────────────────────────────────────

/// Encode an unsigned integer as a LEB-128 (Protocol Buffers varint).
pub fn encode_varint(mut value: u64) -> Vec<u8> {
    let mut buf = Vec::with_capacity(10);
    loop {
        if value < 0x80 {
            buf.push(value as u8);
            return buf;
        }
        buf.push((value as u8 & 0x7F) | 0x80);
        value >>= 7;
    }
}

/// Zigzag-encode a signed integer to an unsigned integer.
///
/// Maps small negative values to small positive integers:
/// 0 → 0, -1 → 1, 1 → 2, -2 → 3, 2 → 4, …
pub fn encode_zigzag(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

/// Decode a zigzag-encoded value back to a signed integer.
pub fn decode_zigzag(value: u64) -> i64 {
    ((value >> 1) as i64) ^ (-((value & 1) as i64))
}

/// Encode a field tag: `(field_number << 3) | wire_type`.
fn encode_tag(field: u32, wire_type: u8) -> Vec<u8> {
    encode_varint(((field << 3) | wire_type as u32) as u64)
}

/// Encode a varint field (wire type 0).
pub(crate) fn encode_varint_field(field: u32, value: u64) -> Vec<u8> {
    let mut buf = encode_tag(field, 0);
    buf.extend_from_slice(&encode_varint(value));
    buf
}

/// Encode a length-delimited field (wire type 2).
pub(crate) fn encode_len_delimited(field: u32, data: &[u8]) -> Vec<u8> {
    let mut buf = encode_tag(field, 2);
    buf.extend_from_slice(&encode_varint(data.len() as u64));
    buf.extend_from_slice(data);
    buf
}

/// Encode a string field (wire type 2, UTF-8 bytes).
pub(crate) fn encode_string_field(field: u32, s: &str) -> Vec<u8> {
    encode_len_delimited(field, s.as_bytes())
}

// ─────────────────────────────────────────────────────────────────────────────
// MVT geometry types
// ─────────────────────────────────────────────────────────────────────────────

/// Geometry type as defined in the MVT spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MvtGeometryType {
    /// Unspecified / unknown geometry
    Unknown = 0,
    /// One or more points
    Point = 1,
    /// One or more line strings
    LineString = 2,
    /// One or more polygons
    Polygon = 3,
}

impl MvtGeometryType {
    fn as_u64(&self) -> u64 {
        match self {
            Self::Unknown => 0,
            Self::Point => 1,
            Self::LineString => 2,
            Self::Polygon => 3,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MVT property values
// ─────────────────────────────────────────────────────────────────────────────

/// A typed property value stored in an MVT layer value table.
///
/// Each variant maps to a distinct field number in the `Value` proto message.
#[derive(Debug, Clone)]
pub enum MvtValue {
    /// UTF-8 string (field 1)
    String(String),
    /// 32-bit floating point (field 2, wire type 5)
    Float(f32),
    /// 64-bit floating point (field 3, wire type 1)
    Double(f64),
    /// Signed integer, stored as int64 (field 4)
    Int(i64),
    /// Unsigned integer (field 5)
    UInt(u64),
    /// Signed integer, stored as sint64 (zigzag, field 6)
    Sint(i64),
    /// Boolean (field 7)
    Bool(bool),
}

impl MvtValue {
    /// Encode this value as a protobuf `Value` message body.
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::String(s) => encode_string_field(1, s),
            Self::Float(v) => {
                // field 2, wire type 5 (32-bit fixed)
                let mut buf = encode_tag(2, 5);
                buf.extend_from_slice(&v.to_le_bytes());
                buf
            }
            Self::Double(v) => {
                // field 3, wire type 1 (64-bit fixed)
                let mut buf = encode_tag(3, 1);
                buf.extend_from_slice(&v.to_le_bytes());
                buf
            }
            Self::Int(v) => encode_varint_field(4, *v as u64),
            Self::UInt(v) => encode_varint_field(5, *v),
            Self::Sint(v) => encode_varint_field(6, encode_zigzag(*v)),
            Self::Bool(v) => encode_varint_field(7, u64::from(*v)),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MVT feature
// ─────────────────────────────────────────────────────────────────────────────

/// A single geographic feature in an MVT layer.
///
/// `tags` is a flat array of `[key_idx, value_idx, key_idx, value_idx, …]` pairs
/// into the parent layer's key and value tables.
#[derive(Debug, Clone)]
pub struct MvtFeature {
    /// Optional numeric feature ID
    pub id: Option<u64>,
    /// Geometry type
    pub geometry_type: MvtGeometryType,
    /// Encoded geometry drawing commands (already command-encoded, delta, zigzag-ready integers)
    pub geometry: Vec<i32>,
    /// Flat array of (key_index, value_index) pairs
    pub tags: Vec<u32>,
}

impl MvtFeature {
    /// Encode this feature as a protobuf `Feature` message body.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // id (field 1, optional)
        if let Some(id) = self.id {
            buf.extend_from_slice(&encode_varint_field(1, id));
        }

        // tags (field 2, packed varint)
        if !self.tags.is_empty() {
            let mut packed = Vec::with_capacity(self.tags.len() * 2);
            for &tag in &self.tags {
                packed.extend_from_slice(&encode_varint(tag as u64));
            }
            buf.extend_from_slice(&encode_len_delimited(2, &packed));
        }

        // type (field 3, varint)
        buf.extend_from_slice(&encode_varint_field(3, self.geometry_type.as_u64()));

        // geometry (field 4, packed sint32 via zigzag)
        if !self.geometry.is_empty() {
            let mut packed = Vec::with_capacity(self.geometry.len() * 2);
            for &cmd in &self.geometry {
                packed.extend_from_slice(&encode_varint(encode_zigzag(cmd as i64)));
            }
            buf.extend_from_slice(&encode_len_delimited(4, &packed));
        }

        buf
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MVT layer
// ─────────────────────────────────────────────────────────────────────────────

/// A named collection of features sharing a coordinate space.
///
/// All coordinates in the layer are in tile pixel space [0, `extent`).
pub struct MvtLayer {
    /// Layer name (e.g. "roads", "buildings")
    pub name: String,
    /// MVT version (must be 2)
    pub version: u32,
    /// Tile coordinate extent (typically 4096)
    pub extent: u32,
    /// De-duplicated list of property key strings
    pub keys: Vec<String>,
    /// List of property values (not de-duplicated by default)
    pub values: Vec<MvtValue>,
    /// Features in this layer
    pub features: Vec<MvtFeature>,
}

impl MvtLayer {
    /// Create a new layer with the given name, version=2, extent=4096.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: 2,
            extent: 4096,
            keys: Vec::new(),
            values: Vec::new(),
            features: Vec::new(),
        }
    }

    /// Create a new layer with a custom extent.
    pub fn with_extent(name: impl Into<String>, extent: u32) -> Self {
        Self {
            extent,
            ..Self::new(name)
        }
    }

    /// Get or insert a key, returning its index.
    ///
    /// Keys are de-duplicated: the same string always maps to the same index.
    pub fn key_index(&mut self, key: &str) -> u32 {
        if let Some(i) = self.keys.iter().position(|k| k == key) {
            return i as u32;
        }
        self.keys.push(key.to_string());
        (self.keys.len() - 1) as u32
    }

    /// Append a value and return its index.
    ///
    /// Values are not de-duplicated by default for simplicity and performance.
    pub fn value_index(&mut self, value: MvtValue) -> u32 {
        self.values.push(value);
        (self.values.len() - 1) as u32
    }

    /// Add a feature to this layer.
    pub fn add_feature(&mut self, feature: MvtFeature) {
        self.features.push(feature);
    }

    /// Encode this layer as a protobuf `Layer` message body.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // version (field 15)
        buf.extend_from_slice(&encode_varint_field(15, self.version as u64));

        // name (field 1)
        buf.extend_from_slice(&encode_string_field(1, &self.name));

        // features (field 2, repeated)
        for feature in &self.features {
            buf.extend_from_slice(&encode_len_delimited(2, &feature.encode()));
        }

        // keys (field 3, repeated string)
        for key in &self.keys {
            buf.extend_from_slice(&encode_string_field(3, key));
        }

        // values (field 4, repeated Value message)
        for value in &self.values {
            buf.extend_from_slice(&encode_len_delimited(4, &value.encode()));
        }

        // extent (field 5)
        buf.extend_from_slice(&encode_varint_field(5, self.extent as u64));

        buf
    }

    /// Return the number of features in this layer.
    pub fn feature_count(&self) -> usize {
        self.features.len()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Geometry command helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Produce a **MoveTo** command for one point (absolute delta from cursor).
///
/// Command encoding: `(1 << 3) | 1 = 9` (id=1, count=1).
///
/// # Arguments
/// * `dx` / `dy` – delta from the current cursor position
pub fn move_to(dx: i32, dy: i32) -> Vec<i32> {
    // CommandInteger: (1 << 3) | 1 = 9
    vec![9, dx, dy]
}

/// Produce a **LineTo** command for one or more points.
///
/// Command encoding: `(count << 3) | 2`.
///
/// # Arguments
/// * `coords` – slice of `(dx, dy)` deltas
pub fn line_to(coords: &[(i32, i32)]) -> Vec<i32> {
    let count = coords.len() as i32;
    // CommandInteger: (count << 3) | 2
    let mut cmds = Vec::with_capacity(1 + coords.len() * 2);
    cmds.push((count << 3) | 2);
    for &(dx, dy) in coords {
        cmds.push(dx);
        cmds.push(dy);
    }
    cmds
}

/// Produce a **ClosePath** command.
///
/// Command encoding: `(1 << 3) | 7 = 15`.
pub fn close_path() -> Vec<i32> {
    vec![15]
}

/// Encode a point geometry (single MoveTo).
pub fn point_geometry(dx: i32, dy: i32) -> Vec<i32> {
    move_to(dx, dy)
}

/// Encode a linestring geometry (MoveTo first point, then LineTo for the rest).
///
/// All coordinates are deltas from the previous position.
/// The caller is responsible for computing deltas.
pub fn linestring_geometry(coords: &[(i32, i32)]) -> Vec<i32> {
    if coords.is_empty() {
        return Vec::new();
    }
    let mut cmds = move_to(coords[0].0, coords[0].1);
    if coords.len() > 1 {
        cmds.extend_from_slice(&line_to(&coords[1..]));
    }
    cmds
}

/// Encode a polygon ring geometry (MoveTo, LineTo, ClosePath).
///
/// All coordinates are deltas from the previous position.
pub fn polygon_ring_geometry(coords: &[(i32, i32)]) -> Vec<i32> {
    if coords.is_empty() {
        return Vec::new();
    }
    let mut cmds = move_to(coords[0].0, coords[0].1);
    if coords.len() > 1 {
        cmds.extend_from_slice(&line_to(&coords[1..]));
    }
    cmds.extend_from_slice(&close_path());
    cmds
}

/// Twice the signed area of a ring in tile pixel space (surveyor's formula).
///
/// In MVT tile coordinates the origin is top-left and `y` increases downward,
/// so a **clockwise** ring (as drawn on screen) yields a **positive** value.
/// Per the MVT 2.1 spec exterior rings must be clockwise (positive) and
/// interior rings counter-clockwise (negative).
fn signed_area_2x(ring: &[(i32, i32)]) -> i64 {
    let n = ring.len();
    if n < 3 {
        return 0;
    }
    let mut sum: i64 = 0;
    for i in 0..n {
        let (x1, y1) = ring[i];
        let (x2, y2) = ring[(i + 1) % n];
        sum += (x1 as i64) * (y2 as i64) - (x2 as i64) * (y1 as i64);
    }
    sum
}

/// Encode a multi-ring polygon geometry (exterior ring plus optional holes,
/// or several exterior/hole groups for a MultiPolygon).
///
/// `rings` is a list of `(pixel_ring, is_exterior)` pairs in tile pixel space.
/// Each ring:
/// - has its closing/duplicate final vertex removed (ClosePath re-closes it),
/// - is re-oriented to satisfy MVT winding (exterior clockwise, holes
///   counter-clockwise),
/// - is emitted as `MoveTo, LineTo…, ClosePath`.
///
/// Crucially the delta cursor is carried **continuously** across rings: after a
/// `ClosePath` the cursor remains at the last drawn vertex, so the next ring's
/// `MoveTo` is delta-encoded relative to it (as the MVT spec requires).
/// Degenerate rings (fewer than 3 distinct vertices) are skipped.
pub fn polygon_rings_geometry(rings: &[(Vec<(i32, i32)>, bool)]) -> Vec<i32> {
    let mut cmds = Vec::new();
    let mut cursor = (0i32, 0i32);

    for (ring, is_exterior) in rings {
        // Drop an explicit closing vertex if present.
        let mut pts: Vec<(i32, i32)> = ring.clone();
        if pts.len() >= 2 && pts[0] == pts[pts.len() - 1] {
            pts.pop();
        }
        if pts.len() < 3 {
            continue;
        }

        // Enforce winding: exterior clockwise (area > 0), hole CCW (area < 0).
        let area = signed_area_2x(&pts);
        let needs_reverse = if *is_exterior { area < 0 } else { area > 0 };
        if needs_reverse {
            pts.reverse();
        }

        let (fx, fy) = pts[0];
        cmds.extend_from_slice(&move_to(fx - cursor.0, fy - cursor.1));
        cursor = (fx, fy);

        let mut deltas = Vec::with_capacity(pts.len() - 1);
        for &(x, y) in &pts[1..] {
            deltas.push((x - cursor.0, y - cursor.1));
            cursor = (x, y);
        }
        cmds.extend_from_slice(&line_to(&deltas));
        cmds.extend_from_slice(&close_path());
        // ClosePath leaves the cursor at the last LineTo vertex.
    }

    cmds
}

// ─────────────────────────────────────────────────────────────────────────────
// MVT tile
// ─────────────────────────────────────────────────────────────────────────────

/// A complete MVT tile containing zero or more named layers.
///
/// Encode with [`MvtTile::encode`] to obtain the binary payload for HTTP
/// responses, PMTiles storage, or MBTiles embedding.
pub struct MvtTile {
    /// Layers in this tile (each layer = field 3 in Tile proto)
    pub layers: Vec<MvtLayer>,
}

impl MvtTile {
    /// Create an empty tile.
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    /// Add a layer to this tile.
    pub fn add_layer(&mut self, layer: MvtLayer) {
        self.layers.push(layer);
    }

    /// Encode this tile to the binary MVT protobuf format.
    ///
    /// The result can be served directly as `application/vnd.mapbox-vector-tile`.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        for layer in &self.layers {
            // Each layer is wrapped in field 3 (length-delimited)
            buf.extend_from_slice(&encode_len_delimited(3, &layer.encode()));
        }
        buf
    }

    /// Return the total number of features across all layers.
    pub fn total_feature_count(&self) -> usize {
        self.layers.iter().map(|l| l.feature_count()).sum()
    }
}

impl Default for MvtTile {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Coordinate projection helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Scale a WGS84 longitude/latitude coordinate to tile pixel space [0, extent).
///
/// `tile_bbox` is `[west, south, east, north]` in degrees.
/// Returns `(pixel_x, pixel_y)` clamped to `[0, extent − 1]`.
pub fn scale_to_tile(lon: f64, lat: f64, tile_bbox: [f64; 4], extent: u32) -> (i32, i32) {
    let [west, south, east, north] = tile_bbox;
    let x_raw = (lon - west) / (east - west) * extent as f64;
    let y_raw = (north - lat) / (north - south) * extent as f64;
    let x = x_raw as i32;
    let y = y_raw as i32;
    let max = extent as i32 - 1;
    (x.clamp(0, max), y.clamp(0, max))
}

/// Compute delta coordinates for a sequence of absolute tile-space coordinates.
///
/// MVT geometry commands use delta encoding relative to the previous position.
/// The returned `Vec` has the same length as the input.
pub fn delta_encode(coords: &[(i32, i32)]) -> Vec<(i32, i32)> {
    let mut deltas = Vec::with_capacity(coords.len());
    let mut cursor = (0i32, 0i32);
    for &(x, y) in coords {
        deltas.push((x - cursor.0, y - cursor.1));
        cursor = (x, y);
    }
    deltas
}

// ─────────────────────────────────────────────────────────────────────────────
// High-level builder helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Builder for constructing an [`MvtLayer`] from GeoJSON-like feature data.
///
/// Handles coordinate scaling, delta encoding, and property table management.
pub struct MvtLayerBuilder {
    layer: MvtLayer,
    tile_bbox: [f64; 4],
}

impl MvtLayerBuilder {
    /// Create a new builder for the given layer name, tile bbox, and extent.
    pub fn new(name: impl Into<String>, tile_bbox: [f64; 4], extent: u32) -> Self {
        Self {
            layer: MvtLayer::with_extent(name, extent),
            tile_bbox,
        }
    }

    /// Add a point feature with a set of string properties.
    ///
    /// # Errors
    /// Returns [`ServiceError::InvalidParameter`] if `properties` keys/values are invalid.
    pub fn add_point(
        &mut self,
        lon: f64,
        lat: f64,
        id: Option<u64>,
        properties: &[(&str, MvtValue)],
    ) -> Result<(), ServiceError> {
        let (px, py) = scale_to_tile(lon, lat, self.tile_bbox, self.layer.extent);
        let geom = point_geometry(px, py);

        let mut tags = Vec::with_capacity(properties.len() * 2);
        for (key, value) in properties {
            let ki = self.layer.key_index(key);
            let vi = self.layer.value_index(value.clone());
            tags.push(ki);
            tags.push(vi);
        }

        self.layer.add_feature(MvtFeature {
            id,
            geometry_type: MvtGeometryType::Point,
            geometry: geom,
            tags,
        });
        Ok(())
    }

    /// Add a linestring feature from a sequence of WGS84 coordinates.
    pub fn add_linestring(
        &mut self,
        coords: &[(f64, f64)],
        id: Option<u64>,
        properties: &[(&str, MvtValue)],
    ) -> Result<(), ServiceError> {
        if coords.is_empty() {
            return Err(ServiceError::InvalidParameter(
                "coords".into(),
                "linestring must have at least one coordinate".into(),
            ));
        }

        let pixel_coords: Vec<(i32, i32)> = coords
            .iter()
            .map(|&(lon, lat)| scale_to_tile(lon, lat, self.tile_bbox, self.layer.extent))
            .collect();
        let deltas = delta_encode(&pixel_coords);
        let geom = linestring_geometry(&deltas);

        let mut tags = Vec::with_capacity(properties.len() * 2);
        for (key, value) in properties {
            let ki = self.layer.key_index(key);
            let vi = self.layer.value_index(value.clone());
            tags.push(ki);
            tags.push(vi);
        }

        self.layer.add_feature(MvtFeature {
            id,
            geometry_type: MvtGeometryType::LineString,
            geometry: geom,
            tags,
        });
        Ok(())
    }

    /// Add a simple polygon feature from a single WGS84 exterior ring.
    ///
    /// For polygons with holes use [`MvtLayerBuilder::add_polygon_with_holes`];
    /// for MultiPolygons use [`MvtLayerBuilder::add_multipolygon`].
    pub fn add_polygon(
        &mut self,
        ring: &[(f64, f64)],
        id: Option<u64>,
        properties: &[(&str, MvtValue)],
    ) -> Result<(), ServiceError> {
        self.add_polygon_with_holes(ring, &[], id, properties)
    }

    /// Add a polygon feature with an exterior ring and zero or more hole rings.
    ///
    /// Rings are given in WGS84. Winding order is normalized automatically to
    /// satisfy the MVT specification (exterior clockwise, holes
    /// counter-clockwise in tile space), so callers need not pre-orient them.
    pub fn add_polygon_with_holes(
        &mut self,
        exterior: &[(f64, f64)],
        holes: &[&[(f64, f64)]],
        id: Option<u64>,
        properties: &[(&str, MvtValue)],
    ) -> Result<(), ServiceError> {
        if exterior.len() < 3 {
            return Err(ServiceError::InvalidParameter(
                "exterior".into(),
                "polygon exterior ring must have at least 3 coordinates".into(),
            ));
        }

        let mut rings: Vec<(Vec<(i32, i32)>, bool)> = Vec::with_capacity(1 + holes.len());
        rings.push((self.ring_to_pixels(exterior), true));
        for hole in holes {
            if hole.len() < 3 {
                return Err(ServiceError::InvalidParameter(
                    "hole".into(),
                    "polygon hole ring must have at least 3 coordinates".into(),
                ));
            }
            rings.push((self.ring_to_pixels(hole), false));
        }

        let geom = polygon_rings_geometry(&rings);
        self.push_polygon_feature(id, properties, geom);
        Ok(())
    }

    /// Add a MultiPolygon feature.
    ///
    /// Each polygon is `[exterior, hole0, hole1, …]` in WGS84 (the first ring is
    /// the exterior, the remaining rings are holes). All rings are emitted into
    /// a single MVT feature with continuous cursor/delta encoding and
    /// normalized winding, so a conformant decoder reconstructs each polygon.
    pub fn add_multipolygon(
        &mut self,
        polygons: &[Vec<Vec<(f64, f64)>>],
        id: Option<u64>,
        properties: &[(&str, MvtValue)],
    ) -> Result<(), ServiceError> {
        if polygons.is_empty() {
            return Err(ServiceError::InvalidParameter(
                "polygons".into(),
                "multipolygon must contain at least one polygon".into(),
            ));
        }

        let mut rings: Vec<(Vec<(i32, i32)>, bool)> = Vec::new();
        for polygon in polygons {
            let mut ring_iter = polygon.iter();
            let exterior = ring_iter.next().ok_or_else(|| {
                ServiceError::InvalidParameter(
                    "polygon".into(),
                    "multipolygon member must have an exterior ring".into(),
                )
            })?;
            if exterior.len() < 3 {
                return Err(ServiceError::InvalidParameter(
                    "exterior".into(),
                    "polygon exterior ring must have at least 3 coordinates".into(),
                ));
            }
            rings.push((self.ring_to_pixels(exterior), true));
            for hole in ring_iter {
                if hole.len() < 3 {
                    return Err(ServiceError::InvalidParameter(
                        "hole".into(),
                        "polygon hole ring must have at least 3 coordinates".into(),
                    ));
                }
                rings.push((self.ring_to_pixels(hole), false));
            }
        }

        let geom = polygon_rings_geometry(&rings);
        self.push_polygon_feature(id, properties, geom);
        Ok(())
    }

    /// Add a feature from a GeoJSON geometry, dispatching on its type.
    ///
    /// Supports Point, MultiPoint, LineString, MultiLineString, Polygon and
    /// MultiPolygon. Unsupported geometry types (e.g. GeometryCollection)
    /// return an error rather than being silently dropped.
    pub fn add_geojson_geometry(
        &mut self,
        geometry: &geojson::Geometry,
        id: Option<u64>,
        properties: &[(&str, MvtValue)],
    ) -> Result<(), ServiceError> {
        use geojson::GeometryValue;

        let to_coords = |ring: &[geojson::Position]| -> Vec<(f64, f64)> {
            ring.iter()
                .filter_map(|p| {
                    let s = p.as_slice();
                    if s.len() >= 2 {
                        Some((s[0], s[1]))
                    } else {
                        None
                    }
                })
                .collect()
        };

        match &geometry.value {
            GeometryValue::Point { coordinates } => {
                let s = coordinates.as_slice();
                if s.len() < 2 {
                    return Err(ServiceError::InvalidParameter(
                        "coordinates".into(),
                        "point requires at least 2 ordinates".into(),
                    ));
                }
                self.add_point(s[0], s[1], id, properties)
            }
            GeometryValue::MultiPoint { coordinates } => {
                for coord in coordinates {
                    let s = coord.as_slice();
                    if s.len() >= 2 {
                        self.add_point(s[0], s[1], id, properties)?;
                    }
                }
                Ok(())
            }
            GeometryValue::LineString { coordinates } => {
                let coords = to_coords(coordinates);
                self.add_linestring(&coords, id, properties)
            }
            GeometryValue::MultiLineString { coordinates } => {
                for line in coordinates {
                    let coords = to_coords(line);
                    self.add_linestring(&coords, id, properties)?;
                }
                Ok(())
            }
            GeometryValue::Polygon { coordinates } => {
                let rings: Vec<Vec<(f64, f64)>> =
                    coordinates.iter().map(|r| to_coords(r)).collect();
                let exterior = rings.first().ok_or_else(|| {
                    ServiceError::InvalidParameter(
                        "coordinates".into(),
                        "polygon requires an exterior ring".into(),
                    )
                })?;
                let holes: Vec<&[(f64, f64)]> = rings[1..].iter().map(|r| r.as_slice()).collect();
                self.add_polygon_with_holes(exterior, &holes, id, properties)
            }
            GeometryValue::MultiPolygon { coordinates } => {
                let polygons: Vec<Vec<Vec<(f64, f64)>>> = coordinates
                    .iter()
                    .map(|poly| poly.iter().map(|r| to_coords(r)).collect())
                    .collect();
                self.add_multipolygon(&polygons, id, properties)
            }
            GeometryValue::GeometryCollection { .. } => Err(ServiceError::UnsupportedFormat(
                "GeometryCollection cannot be encoded as a single MVT feature".into(),
            )),
        }
    }

    /// Scale a WGS84 ring to tile pixel coordinates.
    fn ring_to_pixels(&self, ring: &[(f64, f64)]) -> Vec<(i32, i32)> {
        ring.iter()
            .map(|&(lon, lat)| scale_to_tile(lon, lat, self.tile_bbox, self.layer.extent))
            .collect()
    }

    /// Append a polygon feature with the given pre-encoded geometry commands.
    fn push_polygon_feature(
        &mut self,
        id: Option<u64>,
        properties: &[(&str, MvtValue)],
        geometry: Vec<i32>,
    ) {
        let mut tags = Vec::with_capacity(properties.len() * 2);
        for (key, value) in properties {
            let ki = self.layer.key_index(key);
            let vi = self.layer.value_index(value.clone());
            tags.push(ki);
            tags.push(vi);
        }
        self.layer.add_feature(MvtFeature {
            id,
            geometry_type: MvtGeometryType::Polygon,
            geometry,
            tags,
        });
    }

    /// Add a whole GeoJSON feature, mapping its JSON properties to MVT tags.
    ///
    /// Property values are converted as: string → String, integer → Int,
    /// float → Double, bool → Bool. Null and nested (array/object) properties
    /// are skipped (MVT tag values must be scalars). The feature's numeric
    /// `id` (if any and parseable) is preserved.
    pub fn add_geojson_feature(&mut self, feature: &geojson::Feature) -> Result<(), ServiceError> {
        let geometry = feature.geometry.as_ref().ok_or_else(|| {
            ServiceError::InvalidParameter(
                "feature".into(),
                "feature has no geometry to encode".into(),
            )
        })?;

        // Convert scalar properties to owned (key, MvtValue) pairs.
        let mut owned: Vec<(String, MvtValue)> = Vec::new();
        if let Some(props) = &feature.properties {
            for (key, value) in props {
                let mvt_value = match value {
                    serde_json::Value::String(s) => Some(MvtValue::String(s.clone())),
                    serde_json::Value::Bool(b) => Some(MvtValue::Bool(*b)),
                    serde_json::Value::Number(n) => {
                        if let Some(i) = n.as_i64() {
                            Some(MvtValue::Int(i))
                        } else {
                            n.as_f64().map(MvtValue::Double)
                        }
                    }
                    _ => None,
                };
                if let Some(v) = mvt_value {
                    owned.push((key.clone(), v));
                }
            }
        }
        let properties: Vec<(&str, MvtValue)> =
            owned.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();

        let id = match &feature.id {
            Some(geojson::feature::Id::Number(n)) => n.as_u64(),
            Some(geojson::feature::Id::String(s)) => s.parse::<u64>().ok(),
            None => None,
        };

        self.add_geojson_geometry(geometry, id, &properties)
    }

    /// Consume the builder and return the completed layer.
    pub fn build(self) -> MvtLayer {
        self.layer
    }
}

/// Build a complete single-layer MVT tile from GeoJSON features.
///
/// This is the dataset-backed entry point that a vector-tile HTTP handler
/// invokes: it takes the features that fall within `tile_bbox` (in WGS84
/// `[west, south, east, north]` degrees), encodes each into the named layer,
/// and returns the binary `application/vnd.mapbox-vector-tile` payload.
///
/// Features whose geometry cannot be represented as a single MVT feature (e.g.
/// GeometryCollection) are skipped rather than aborting the whole tile.
pub fn build_tile_from_features(
    layer_name: &str,
    tile_bbox: [f64; 4],
    extent: u32,
    features: &[geojson::Feature],
) -> Result<Vec<u8>, ServiceError> {
    let mut builder = MvtLayerBuilder::new(layer_name, tile_bbox, extent);
    for feature in features {
        // Skip features with no/unsupported geometry so one bad feature does
        // not poison the entire tile.
        match builder.add_geojson_feature(feature) {
            Ok(()) => {}
            Err(ServiceError::UnsupportedFormat(_)) | Err(ServiceError::InvalidParameter(_, _)) => {
                continue;
            }
            Err(other) => return Err(other),
        }
    }
    let mut tile = MvtTile::new();
    tile.add_layer(builder.build());
    Ok(tile.encode())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── varint encoding ──────────────────────────────────────────────────────

    #[test]
    fn test_encode_varint_zero() {
        assert_eq!(encode_varint(0), vec![0x00]);
    }

    #[test]
    fn test_encode_varint_one() {
        assert_eq!(encode_varint(1), vec![0x01]);
    }

    #[test]
    fn test_encode_varint_127() {
        assert_eq!(encode_varint(127), vec![0x7F]);
    }

    #[test]
    fn test_encode_varint_128() {
        // 128 = 0x80 requires two bytes: [0x80, 0x01]
        assert_eq!(encode_varint(128), vec![0x80, 0x01]);
    }

    #[test]
    fn test_encode_varint_300() {
        // 300 = 256 + 44 = 0x12C
        // varint: 0xAC 0x02
        assert_eq!(encode_varint(300), vec![0xAC, 0x02]);
    }

    #[test]
    fn test_encode_varint_large() {
        // 2^14 = 16384 → 3 bytes
        let enc = encode_varint(16384);
        assert_eq!(enc.len(), 3);
    }

    #[test]
    fn test_encode_varint_max_u32() {
        let enc = encode_varint(u32::MAX as u64);
        assert!(!enc.is_empty());
        assert!(enc.len() <= 5);
    }

    // ── zigzag encoding ──────────────────────────────────────────────────────

    #[test]
    fn test_encode_zigzag_zero() {
        assert_eq!(encode_zigzag(0), 0);
    }

    #[test]
    fn test_encode_zigzag_minus_one() {
        assert_eq!(encode_zigzag(-1), 1);
    }

    #[test]
    fn test_encode_zigzag_plus_one() {
        assert_eq!(encode_zigzag(1), 2);
    }

    #[test]
    fn test_encode_zigzag_minus_two() {
        assert_eq!(encode_zigzag(-2), 3);
    }

    #[test]
    fn test_encode_zigzag_plus_two() {
        assert_eq!(encode_zigzag(2), 4);
    }

    #[test]
    fn test_decode_zigzag_roundtrip() {
        for v in [-100i64, -1, 0, 1, 100, 4095, -4096] {
            assert_eq!(
                decode_zigzag(encode_zigzag(v)),
                v,
                "roundtrip failed for {}",
                v
            );
        }
    }

    // ── geometry command encoding ────────────────────────────────────────────

    #[test]
    fn test_move_to_encoding() {
        let cmds = move_to(10, 20);
        // [CommandInteger=9, dx=10, dy=20]
        assert_eq!(cmds, vec![9, 10, 20]);
    }

    #[test]
    fn test_move_to_origin() {
        let cmds = move_to(0, 0);
        assert_eq!(cmds, vec![9, 0, 0]);
    }

    #[test]
    fn test_close_path_encoding() {
        let cmds = close_path();
        // CommandInteger: (1 << 3) | 7 = 15
        assert_eq!(cmds, vec![15]);
    }

    #[test]
    fn test_line_to_two_points() {
        let cmds = line_to(&[(5, 3), (10, 8)]);
        // CommandInteger: (2 << 3) | 2 = 18
        assert_eq!(cmds[0], 18, "command integer for LineTo count=2");
        assert_eq!(cmds[1], 5);
        assert_eq!(cmds[2], 3);
        assert_eq!(cmds[3], 10);
        assert_eq!(cmds[4], 8);
        assert_eq!(cmds.len(), 5);
    }

    #[test]
    fn test_line_to_one_point() {
        let cmds = line_to(&[(7, 7)]);
        // CommandInteger: (1 << 3) | 2 = 10
        assert_eq!(cmds[0], 10);
        assert_eq!(cmds.len(), 3);
    }

    #[test]
    fn test_polygon_ring_geometry_has_close_path() {
        let ring = [(0, 0), (100, 0), (100, 100), (0, 100)];
        let cmds = polygon_ring_geometry(&ring);
        // Last element should be the ClosePath command (15)
        assert_eq!(*cmds.last().expect("non-empty"), 15);
    }

    #[test]
    fn test_linestring_geometry_starts_with_move_to() {
        let coords = [(5, 10), (15, 20), (25, 30)];
        let cmds = linestring_geometry(&coords);
        // First element is MoveTo CommandInteger (9 for count=1)
        assert_eq!(cmds[0], 9, "should start with MoveTo");
        assert_eq!(cmds[1], 5);
        assert_eq!(cmds[2], 10);
    }

    // ── MvtLayer construction ────────────────────────────────────────────────

    #[test]
    fn test_mvt_layer_new_defaults() {
        let layer = MvtLayer::new("roads");
        assert_eq!(layer.name, "roads");
        assert_eq!(layer.version, 2);
        assert_eq!(layer.extent, 4096);
        assert!(layer.keys.is_empty());
        assert!(layer.values.is_empty());
        assert!(layer.features.is_empty());
    }

    #[test]
    fn test_mvt_layer_key_index_insert() {
        let mut layer = MvtLayer::new("test");
        let i0 = layer.key_index("name");
        let i1 = layer.key_index("type");
        assert_eq!(i0, 0);
        assert_eq!(i1, 1);
        assert_eq!(layer.keys.len(), 2);
    }

    #[test]
    fn test_mvt_layer_key_index_dedup() {
        let mut layer = MvtLayer::new("test");
        let i0 = layer.key_index("name");
        let i1 = layer.key_index("name");
        assert_eq!(i0, i1, "duplicate key should return same index");
        assert_eq!(layer.keys.len(), 1);
    }

    #[test]
    fn test_mvt_layer_value_index_append() {
        let mut layer = MvtLayer::new("test");
        let i0 = layer.value_index(MvtValue::String("road".into()));
        let i1 = layer.value_index(MvtValue::Int(42));
        assert_eq!(i0, 0);
        assert_eq!(i1, 1);
    }

    #[test]
    fn test_mvt_layer_encode_non_empty() {
        let mut layer = MvtLayer::new("buildings");
        layer.add_feature(MvtFeature {
            id: Some(1),
            geometry_type: MvtGeometryType::Point,
            geometry: move_to(100, 200),
            tags: vec![],
        });
        let encoded = layer.encode();
        assert!(!encoded.is_empty(), "encoded layer should not be empty");
    }

    // ── MvtFeature encoding ──────────────────────────────────────────────────

    #[test]
    fn test_mvt_feature_encode_has_geometry_type() {
        let f = MvtFeature {
            id: None,
            geometry_type: MvtGeometryType::Point,
            geometry: move_to(0, 0),
            tags: vec![],
        };
        let encoded = f.encode();
        // Geometry type field (field 3) must appear
        // field tag for field 3 wire 0 = (3<<3)|0 = 24 = 0x18
        assert!(
            encoded.windows(2).any(|w| w[0] == 0x18),
            "geometry type field (0x18) not found in encoded feature"
        );
    }

    #[test]
    fn test_mvt_feature_encode_with_id() {
        let f = MvtFeature {
            id: Some(42),
            geometry_type: MvtGeometryType::LineString,
            geometry: vec![],
            tags: vec![],
        };
        let encoded = f.encode();
        // field 1 varint tag = (1<<3)|0 = 8 = 0x08
        assert!(
            encoded.first() == Some(&0x08),
            "id field tag (0x08) should be first byte"
        );
    }

    #[test]
    fn test_mvt_feature_encode_with_tags() {
        let f = MvtFeature {
            id: None,
            geometry_type: MvtGeometryType::Polygon,
            geometry: vec![],
            tags: vec![0, 0, 1, 1],
        };
        let encoded = f.encode();
        assert!(!encoded.is_empty());
        // tag field (field 2) tag = (2<<3)|2 = 18 = 0x12
        assert!(
            encoded.contains(&0x12),
            "tags field (0x12) should be present"
        );
    }

    // ── MvtTile ──────────────────────────────────────────────────────────────

    #[test]
    fn test_mvt_tile_empty_encode() {
        let tile = MvtTile::new();
        let encoded = tile.encode();
        assert!(
            encoded.is_empty(),
            "empty tile should encode to empty bytes"
        );
    }

    #[test]
    fn test_mvt_tile_wraps_layer_in_field3() {
        let mut tile = MvtTile::new();
        let layer = MvtLayer::new("test");
        tile.add_layer(layer);
        let encoded = tile.encode();
        assert!(!encoded.is_empty());
        // Field 3, wire type 2: tag = (3<<3)|2 = 26 = 0x1A
        assert_eq!(
            encoded[0], 0x1A,
            "tile should start with layer field tag 0x1A"
        );
    }

    #[test]
    fn test_mvt_tile_multiple_layers() {
        let mut tile = MvtTile::new();
        tile.add_layer(MvtLayer::new("roads"));
        tile.add_layer(MvtLayer::new("buildings"));
        assert_eq!(tile.layers.len(), 2);
        let encoded = tile.encode();
        // Both layers should produce field-3 entries
        let count = encoded.windows(1).filter(|w| w[0] == 0x1A).count();
        assert_eq!(count, 2, "should have 2 layer field tags");
    }

    #[test]
    fn test_mvt_tile_total_feature_count() {
        let mut tile = MvtTile::new();
        let mut layer1 = MvtLayer::new("a");
        layer1.add_feature(MvtFeature {
            id: None,
            geometry_type: MvtGeometryType::Point,
            geometry: move_to(0, 0),
            tags: vec![],
        });
        let mut layer2 = MvtLayer::new("b");
        layer2.add_feature(MvtFeature {
            id: None,
            geometry_type: MvtGeometryType::Point,
            geometry: move_to(10, 10),
            tags: vec![],
        });
        layer2.add_feature(MvtFeature {
            id: None,
            geometry_type: MvtGeometryType::Point,
            geometry: move_to(20, 20),
            tags: vec![],
        });
        tile.add_layer(layer1);
        tile.add_layer(layer2);
        assert_eq!(tile.total_feature_count(), 3);
    }

    // ── scale_to_tile ────────────────────────────────────────────────────────

    #[test]
    fn test_scale_to_tile_origin() {
        let bbox = [-10.0f64, -10.0, 10.0, 10.0];
        let (x, y) = scale_to_tile(-10.0, 10.0, bbox, 4096);
        assert_eq!(x, 0);
        assert_eq!(y, 0);
    }

    #[test]
    fn test_scale_to_tile_max_corner() {
        let bbox = [0.0f64, 0.0, 1.0, 1.0];
        // Far corner: (1,0) in lon/lat → (extent-1, 0) in pixels
        let (x, y) = scale_to_tile(1.0, 1.0, bbox, 4096);
        assert_eq!(x, 4095);
        assert_eq!(y, 0);
    }

    #[test]
    fn test_scale_to_tile_clamps_negative() {
        let bbox = [0.0f64, 0.0, 1.0, 1.0];
        let (x, y) = scale_to_tile(-1.0, 2.0, bbox, 4096);
        assert_eq!(x, 0);
        assert_eq!(y, 0);
    }

    #[test]
    fn test_scale_to_tile_clamps_overflow() {
        let bbox = [0.0f64, 0.0, 1.0, 1.0];
        let (x, y) = scale_to_tile(2.0, -1.0, bbox, 4096);
        assert_eq!(x, 4095);
        assert_eq!(y, 4095);
    }

    #[test]
    fn test_scale_to_tile_center() {
        let bbox = [-180.0f64, -85.0, 180.0, 85.0];
        let (x, y) = scale_to_tile(0.0, 0.0, bbox, 4096);
        assert_eq!(x, 2048);
        // y ≈ 2048 (equator is near the middle for this symmetric bbox)
        assert!((y - 2048).abs() <= 2, "y={}", y);
    }

    // ── delta_encode ─────────────────────────────────────────────────────────

    #[test]
    fn test_delta_encode_basic() {
        let coords = [(10, 20), (15, 25), (5, 30)];
        let deltas = delta_encode(&coords);
        assert_eq!(deltas[0], (10, 20));
        assert_eq!(deltas[1], (5, 5));
        assert_eq!(deltas[2], (-10, 5));
    }

    #[test]
    fn test_delta_encode_empty() {
        let deltas = delta_encode(&[]);
        assert!(deltas.is_empty());
    }

    // ── MvtLayerBuilder ──────────────────────────────────────────────────────

    #[test]
    fn test_layer_builder_add_point() {
        let bbox = [-180.0f64, -85.0, 180.0, 85.0];
        let mut builder = MvtLayerBuilder::new("poi", bbox, 4096);
        builder
            .add_point(
                0.0,
                0.0,
                Some(1),
                &[("name", MvtValue::String("origin".into()))],
            )
            .expect("add_point should succeed");
        let layer = builder.build();
        assert_eq!(layer.feature_count(), 1);
        assert_eq!(layer.keys.len(), 1);
        assert_eq!(layer.keys[0], "name");
    }

    #[test]
    fn test_layer_builder_add_linestring() {
        let bbox = [-180.0f64, -85.0, 180.0, 85.0];
        let mut builder = MvtLayerBuilder::new("roads", bbox, 4096);
        let coords = [(-10.0f64, 20.0), (0.0, 30.0), (10.0, 40.0)];
        builder
            .add_linestring(
                &coords,
                None,
                &[("highway", MvtValue::String("motorway".into()))],
            )
            .expect("add_linestring should succeed");
        let layer = builder.build();
        assert_eq!(layer.feature_count(), 1);
        let f = &layer.features[0];
        assert_eq!(f.geometry_type, MvtGeometryType::LineString);
    }

    #[test]
    fn test_layer_builder_add_linestring_empty_error() {
        let bbox = [-180.0f64, -85.0, 180.0, 85.0];
        let mut builder = MvtLayerBuilder::new("roads", bbox, 4096);
        let result = builder.add_linestring(&[], None, &[]);
        assert!(result.is_err(), "empty linestring should return error");
    }

    #[test]
    fn test_layer_builder_add_polygon() {
        let bbox = [-1.0f64, -1.0, 1.0, 1.0];
        let mut builder = MvtLayerBuilder::new("buildings", bbox, 4096);
        let ring = [(-0.5f64, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)];
        builder
            .add_polygon(&ring, Some(99), &[("height", MvtValue::Int(10))])
            .expect("add_polygon should succeed");
        let layer = builder.build();
        let f = &layer.features[0];
        assert_eq!(f.geometry_type, MvtGeometryType::Polygon);
        assert_eq!(f.id, Some(99));
        // Polygon geometry ends with ClosePath (15 in zigzag = 30 in varint)
        assert_eq!(*f.geometry.last().expect("non-empty"), 15);
    }

    // ── Full round-trip ──────────────────────────────────────────────────────

    #[test]
    fn test_full_roundtrip_tile_encode_non_empty() {
        let bbox = [-180.0f64, -90.0, 180.0, 90.0];
        let mut builder = MvtLayerBuilder::new("countries", bbox, 4096);
        builder
            .add_point(
                139.6917,
                35.6895,
                Some(1),
                &[
                    ("name", MvtValue::String("Tokyo".into())),
                    ("pop", MvtValue::Int(13_960_000)),
                ],
            )
            .expect("add Tokyo point");
        builder
            .add_point(
                -0.1276,
                51.5074,
                Some(2),
                &[
                    ("name", MvtValue::String("London".into())),
                    ("pop", MvtValue::Int(8_982_000)),
                ],
            )
            .expect("add London point");

        let layer = builder.build();
        assert_eq!(layer.feature_count(), 2);

        let mut tile = MvtTile::new();
        tile.add_layer(layer);

        let encoded = tile.encode();
        assert!(!encoded.is_empty(), "encoded tile must not be empty");
        // Must start with layer field tag (0x1A)
        assert_eq!(encoded[0], 0x1A);
    }

    #[test]
    fn test_mvt_value_string_encode() {
        let v = MvtValue::String("hello".into());
        let enc = v.encode();
        // field 1, wire 2: tag = 0x0A, length=5, then "hello"
        assert_eq!(enc[0], 0x0A);
        assert_eq!(enc[1], 5);
    }

    #[test]
    fn test_mvt_value_bool_true_encode() {
        let v = MvtValue::Bool(true);
        let enc = v.encode();
        assert!(!enc.is_empty());
    }

    #[test]
    fn test_mvt_value_double_encode_length() {
        let v = MvtValue::Double(std::f64::consts::PI);
        let enc = v.encode();
        // tag(1 byte) + 8 bytes = 9 bytes minimum
        assert!(enc.len() >= 9);
    }

    // ── multi-ring polygon geometry ──────────────────────────────────────────

    /// Count the MoveTo commands in a geometry command stream.
    fn count_move_to(cmds: &[i32]) -> usize {
        let mut count = 0;
        let mut i = 0;
        while i < cmds.len() {
            let cmd = cmds[i];
            let id = cmd & 0x7;
            let cnt = (cmd >> 3) as usize;
            match id {
                1 => {
                    // MoveTo: cnt pairs
                    count += 1;
                    i += 1 + cnt * 2;
                }
                2 => {
                    // LineTo: cnt pairs
                    i += 1 + cnt * 2;
                }
                7 => {
                    // ClosePath: no args
                    i += 1;
                }
                _ => break,
            }
        }
        count
    }

    #[test]
    fn polygon_with_hole_emits_two_move_to() {
        // Exterior 10x10 square, interior 2..4 square (a hole).
        let exterior = vec![(0, 0), (10, 0), (10, 10), (0, 10), (0, 0)];
        let hole = vec![(2, 2), (2, 4), (4, 4), (4, 2), (2, 2)];
        let rings = vec![(exterior, true), (hole, false)];
        let geom = polygon_rings_geometry(&rings);
        // Two rings → two MoveTo commands, single feature.
        assert_eq!(count_move_to(&geom), 2);
    }

    #[test]
    fn polygon_rings_normalize_winding() {
        // Exterior given counter-clockwise (area < 0); encoder must reverse it
        // so the emitted ring is clockwise (positive area).
        let ccw_exterior = vec![(0, 0), (0, 10), (10, 10), (10, 0), (0, 0)];
        assert!(signed_area_2x(&ccw_exterior[..ccw_exterior.len() - 1]) < 0);
        let rings = vec![(ccw_exterior, true)];
        let geom = polygon_rings_geometry(&rings);
        assert_eq!(count_move_to(&geom), 1);
        assert!(!geom.is_empty());
    }

    #[test]
    fn multipolygon_builder_encodes_multiple_exteriors() {
        let mut builder = MvtLayerBuilder::new("layer", [0.0, 0.0, 10.0, 10.0], 4096);
        let poly_a = vec![vec![
            (0.0, 0.0),
            (4.0, 0.0),
            (4.0, 4.0),
            (0.0, 4.0),
            (0.0, 0.0),
        ]];
        let poly_b = vec![vec![
            (6.0, 6.0),
            (9.0, 6.0),
            (9.0, 9.0),
            (6.0, 9.0),
            (6.0, 6.0),
        ]];
        let polygons = vec![poly_a, poly_b];
        builder
            .add_multipolygon(&polygons, Some(1), &[])
            .expect("add multipolygon");
        let layer = builder.build();
        assert_eq!(layer.feature_count(), 1);
        assert_eq!(layer.features[0].geometry_type, MvtGeometryType::Polygon);
        // Two exterior rings → two MoveTo commands in the single feature.
        assert_eq!(count_move_to(&layer.features[0].geometry), 2);
    }

    #[test]
    fn polygon_with_holes_builder_roundtrips_through_tile() {
        let mut builder = MvtLayerBuilder::new("holes", [0.0, 0.0, 10.0, 10.0], 4096);
        let exterior = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let hole = [(3.0, 3.0), (6.0, 3.0), (6.0, 6.0), (3.0, 6.0)];
        let holes: Vec<&[(f64, f64)]> = vec![&hole];
        builder
            .add_polygon_with_holes(&exterior, &holes, Some(7), &[("k", MvtValue::Int(1))])
            .expect("add polygon with holes");
        let layer = builder.build();
        let mut tile = MvtTile::new();
        tile.add_layer(layer);
        let encoded = tile.encode();
        assert!(!encoded.is_empty());
        assert_eq!(tile.total_feature_count(), 1);
    }

    #[test]
    fn build_tile_from_geojson_features_encodes_polygon() {
        let feature: geojson::Feature = serde_json::from_value(serde_json::json!({
            "type": "Feature",
            "geometry": {
                "type": "Polygon",
                "coordinates": [[[0,0],[10,0],[10,10],[0,10],[0,0]]]
            },
            "properties": {"name": "square", "pop": 42}
        }))
        .expect("parse feature");
        let tile = build_tile_from_features("parcels", [0.0, 0.0, 10.0, 10.0], 4096, &[feature])
            .expect("build tile");
        assert!(!tile.is_empty());
    }

    #[test]
    fn add_geojson_geometry_rejects_geometry_collection() {
        let mut builder = MvtLayerBuilder::new("layer", [0.0, 0.0, 10.0, 10.0], 4096);
        let geom = geojson::Geometry::new(geojson::GeometryValue::GeometryCollection {
            geometries: Vec::new(),
        });
        assert!(builder.add_geojson_geometry(&geom, None, &[]).is_err());
    }
}
