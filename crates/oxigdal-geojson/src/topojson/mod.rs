//! TopoJSON 3.0 encoder for the `oxigdal-geojson-stream` crate.
//!
//! Converts a [`FeatureCollection`](crate::parser::FeatureCollection) to a
//! quantised, arc-deduplicated Topology JSON string following the
//! [TopoJSON specification](https://github.com/topojson/topojson-specification).
//!
//! ## Pipeline
//!
//! 1. Compute the bounding box of all features to build a quantisation transform.
//! 2. Walk all polygon rings, quantising coordinates to integer grid positions.
//! 3. Normalise rings (remove duplicate closing vertex).
//! 4. Detect *junctions* — vertices where two or more arcs meet with different
//!    neighbours, and ring start vertices.
//! 5. Cut rings into arcs at junctions; deduplicate using a canonical
//!    (lexicographically smallest) arc key.
//! 6. Delta-encode arcs for compact wire representation.
//! 7. Serialise as a `{"type":"Topology",...}` JSON object.
//!
//! ## Arc reversal
//!
//! Reverse arc index `i` is encoded as `!(i as i32)` per TopoJSON spec §2.1.4
//! — bitwise NOT, not negation.

mod arcs;
mod quantize;
mod writer;

pub use writer::{TopoOptions, feature_collection_to_topojson};
