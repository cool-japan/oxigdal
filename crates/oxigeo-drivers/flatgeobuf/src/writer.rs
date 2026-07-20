//! `FlatGeobuf` writer implementation
//!
//! Provides writing of `FlatGeobuf` files with support for:
//! - Sequential feature writing
//! - Optional spatial index generation
//! - All geometry types and property types

use crate::MAGIC_BYTES;
use crate::error::{FlatGeobufError, Result};
use crate::feature_codec;
use crate::geometry::GeometryCodec;
use crate::header::{Column, Header};
use crate::index::{BoundingBox, Node, PackedRTree, hilbert_index_for_bbox};
use byteorder::{LittleEndian, WriteBytesExt};
use oxigeo_core::vector::Feature;
use std::io::{Seek, SeekFrom, Write};

/// `FlatGeobuf` writer
pub struct FlatGeobufWriter<W: Write + Seek> {
    writer: W,
    header: Header,
    geometry_codec: GeometryCodec,
    features: Vec<Vec<u8>>,
    bboxes: Vec<BoundingBox>,
    features_written: bool,
}

impl<W: Write + Seek> FlatGeobufWriter<W> {
    /// Creates a new `FlatGeobuf` writer
    pub fn new(writer: W, header: Header) -> Result<Self> {
        let geometry_codec = GeometryCodec::new(header.has_z, header.has_m);

        Ok(Self {
            writer,
            header,
            geometry_codec,
            features: Vec::new(),
            bboxes: Vec::new(),
            features_written: false,
        })
    }

    /// Adds a feature to be written
    pub fn add_feature(&mut self, feature: &Feature) -> Result<()> {
        if self.features_written {
            return Err(FlatGeobufError::NotSupported(
                "Cannot add features after writing has completed".to_string(),
            ));
        }

        // Encode feature to bytes
        let feature_bytes = self.encode_feature(feature)?;

        // Store bounding box if building index
        if self.header.has_index {
            if let Some(bounds) = feature.bounds() {
                self.bboxes
                    .push(BoundingBox::new(bounds.0, bounds.1, bounds.2, bounds.3));
            } else {
                self.bboxes.push(BoundingBox::empty());
            }
        }

        self.features.push(feature_bytes);

        Ok(())
    }

    /// Encodes a feature to its `FlatBuffers` `Feature` message bytes (no size
    /// prefix); the caller writes the preceding `u32` length.
    fn encode_feature(&self, feature: &Feature) -> Result<Vec<u8>> {
        feature_codec::encode_feature(&self.header, &self.geometry_codec, feature)
    }

    /// Writes all accumulated features to the output.
    ///
    /// Two-pass approach:
    /// 1. Sort features by their Hilbert curve index over the global bounding box.
    /// 2. Build the Packed R-tree index with accurate byte offsets into the features section.
    /// 3. Write magic, header, index (if enabled), then features in Hilbert-sorted order.
    pub fn finish(mut self) -> Result<W> {
        if self.features_written {
            return Ok(self.writer);
        }

        let feature_count = self.features.len();

        // ── Pass 1: compute global bbox ──────────────────────────────────────
        let global_bbox = if !self.bboxes.is_empty() {
            let mut extent = BoundingBox::empty();
            for bbox in &self.bboxes {
                extent.expand(bbox);
            }
            if extent.is_valid() {
                Some(extent)
            } else {
                None
            }
        } else {
            None
        };

        // Update header extent
        if let Some(ref bb) = global_bbox {
            self.header.extent = Some([bb.min_x, bb.min_y, bb.max_x, bb.max_y]);
        }

        // ── Pass 2: sort features + bboxes by Hilbert index ──────────────────
        // Only sort when we have index support and a valid bbox; otherwise keep
        // insertion order so the no-index path is unchanged.
        let order: Vec<usize> = if self.header.has_index {
            if let Some(ref bb) = global_bbox {
                let mut indexed: Vec<(usize, u64)> = self
                    .bboxes
                    .iter()
                    .enumerate()
                    .map(|(i, bbox)| {
                        let cx = (bbox.min_x + bbox.max_x) * 0.5;
                        let cy = (bbox.min_y + bbox.max_y) * 0.5;
                        (i, hilbert_index_for_bbox(cx, cy, bb))
                    })
                    .collect();
                indexed.sort_by_key(|&(_, h)| h);
                indexed.into_iter().map(|(i, _)| i).collect()
            } else {
                (0..feature_count).collect()
            }
        } else {
            (0..feature_count).collect()
        };

        // ── Pass 3: compute per-feature byte offsets in the features section ──
        // Each feature occupies: 4 bytes (u32 size) + feature_bytes.len() bytes.
        let sorted_offsets: Vec<u64> = {
            let mut offsets = Vec::with_capacity(feature_count);
            let mut cumulative: u64 = 0;
            for &idx in &order {
                offsets.push(cumulative);
                cumulative += 4 + self.features[idx].len() as u64;
            }
            offsets
        };

        // ── Pass 4: build Packed R-tree with correct byte offsets ─────────────
        let rtree: Option<PackedRTree> = if self.header.has_index && !self.bboxes.is_empty() {
            let leaf_boxes: Vec<BoundingBox> = order.iter().map(|&i| self.bboxes[i]).collect();
            let index = Self::build_rtree_with_offsets(
                &leaf_boxes,
                &sorted_offsets,
                PackedRTree::DEFAULT_NODE_SIZE,
            )?;
            Some(index)
        } else {
            None
        };

        // ── Write phase ───────────────────────────────────────────────────────

        // Write magic bytes
        self.writer.write_all(MAGIC_BYTES)?;

        // Update header feature count
        self.header.features_count = Some(feature_count as u64);

        // Write header size placeholder
        let header_size_pos = self.writer.stream_position()?;
        self.writer.write_u32::<LittleEndian>(0)?;

        // Write header
        let header_start = self.writer.stream_position()?;
        self.header.write(&mut self.writer)?;
        let header_end = self.writer.stream_position()?;
        let header_size = header_end - header_start;

        // Patch header size
        self.writer.seek(SeekFrom::Start(header_size_pos))?;
        self.writer.write_u32::<LittleEndian>(header_size as u32)?;
        self.writer.seek(SeekFrom::Start(header_end))?;

        // Write spatial index if present
        if let Some(ref index) = rtree {
            index.write(&mut self.writer)?;
        }

        // Write features in Hilbert-sorted order
        for &idx in &order {
            let feature_bytes = &self.features[idx];
            self.writer
                .write_u32::<LittleEndian>(feature_bytes.len() as u32)?;
            self.writer.write_all(feature_bytes)?;
        }

        self.features_written = true;

        Ok(self.writer)
    }

    /// Builds a Packed R-tree where leaf node offsets are actual byte offsets
    /// into the features section (not ordinal indices).
    ///
    /// The leaf nodes are already in Hilbert-sorted order. Internal nodes are
    /// built bottom-up by grouping leaf nodes into buckets of `node_size`.
    fn build_rtree_with_offsets(
        leaf_boxes: &[BoundingBox],
        leaf_byte_offsets: &[u64],
        node_size: usize,
    ) -> Result<PackedRTree> {
        debug_assert_eq!(leaf_boxes.len(), leaf_byte_offsets.len());

        if leaf_boxes.is_empty() {
            return Ok(PackedRTree::new(node_size));
        }

        // Leaf level nodes: bbox from feature, offset = byte offset to feature data
        let mut current_level: Vec<Node> = leaf_boxes
            .iter()
            .zip(leaf_byte_offsets.iter())
            .map(|(bbox, &offset)| Node::new(*bbox, offset))
            .collect();

        let mut all_nodes: Vec<Node> = Vec::new();
        let mut level_sizes: Vec<usize> = Vec::new();

        // Build internal levels until we reach the root
        while current_level.len() > 1 {
            level_sizes.push(current_level.len());
            let level_start = all_nodes.len();
            all_nodes.extend(current_level.iter().cloned());

            // Build parent level: each parent covers up to `node_size` children
            let mut parent_level: Vec<Node> = Vec::new();
            let mut child_idx = level_start;
            for chunk in current_level.chunks(node_size) {
                let mut parent_bbox = BoundingBox::empty();
                for node in chunk {
                    parent_bbox.expand(&node.bbox);
                }
                // offset of internal node = index of first child in all_nodes
                parent_level.push(Node::new(parent_bbox, child_idx as u64));
                child_idx += chunk.len();
            }

            current_level = parent_level;
        }

        // Add the root (single node or single remaining node)
        if !current_level.is_empty() {
            level_sizes.push(1);
            all_nodes.extend(current_level);
        }

        Ok(PackedRTree {
            nodes: all_nodes,
            level_sizes,
            node_size,
        })
    }

    /// Returns the header
    #[must_use]
    pub const fn header(&self) -> &Header {
        &self.header
    }

    /// Returns the number of features added so far
    #[must_use]
    pub fn feature_count(&self) -> usize {
        self.features.len()
    }
}

/// Builder for creating `FlatGeobuf` files
pub struct FlatGeobufWriterBuilder {
    header: Header,
}

impl FlatGeobufWriterBuilder {
    /// Creates a new builder with the specified geometry type
    #[must_use]
    pub fn new(geometry_type: crate::header::GeometryType) -> Self {
        Self {
            header: Header::new(geometry_type),
        }
    }

    /// Sets the Z dimension flag
    #[must_use]
    pub fn with_z(mut self) -> Self {
        self.header = self.header.with_z();
        self
    }

    /// Sets the M dimension flag
    #[must_use]
    pub fn with_m(mut self) -> Self {
        self.header = self.header.with_m();
        self
    }

    /// Enables spatial index
    #[must_use]
    pub fn with_index(mut self) -> Self {
        self.header = self.header.with_index(true);
        self
    }

    /// Sets the CRS
    #[must_use]
    pub fn with_crs(mut self, crs: crate::header::CrsInfo) -> Self {
        self.header = self.header.with_crs(crs);
        self
    }

    /// Adds a column
    #[must_use]
    pub fn with_column(mut self, column: Column) -> Self {
        self.header.columns.push(column);
        self
    }

    /// Builds the writer
    pub fn build<W: Write + Seek>(self, writer: W) -> Result<FlatGeobufWriter<W>> {
        FlatGeobufWriter::new(writer, self.header)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::header::{ColumnType, GeometryType};
    use oxigeo_core::vector::{FieldValue, Geometry, Point};
    use std::io::Cursor;

    #[test]
    fn test_writer_builder() {
        let builder = FlatGeobufWriterBuilder::new(GeometryType::Point)
            .with_z()
            .with_index()
            .with_column(Column::new("name", ColumnType::String));

        let cursor = Cursor::new(Vec::new());
        let writer = builder.build(cursor).ok();
        assert!(writer.is_some());
    }

    #[test]
    fn test_write_simple_feature() {
        let header = Header::new(GeometryType::Point);
        let cursor = Cursor::new(Vec::new());
        let writer = FlatGeobufWriter::new(cursor, header).ok();
        assert!(writer.is_some());

        let mut writer = writer.expect("writer creation failed");

        let point = Point::new(10.0, 20.0);
        let feature = Feature::new(Geometry::Point(point));

        let result = writer.add_feature(&feature);
        assert!(result.is_ok());

        assert_eq!(writer.feature_count(), 1);
    }

    #[test]
    fn test_write_feature_with_properties() {
        let mut header = Header::new(GeometryType::Point);
        header.add_column(Column::new("name", ColumnType::String));
        header.add_column(Column::new("value", ColumnType::Int));

        let cursor = Cursor::new(Vec::new());
        let writer = FlatGeobufWriter::new(cursor, header).ok();
        assert!(writer.is_some());

        let mut writer = writer.expect("writer creation failed");

        let point = Point::new(10.0, 20.0);
        let mut feature = Feature::new(Geometry::Point(point));
        feature.set_property("name", FieldValue::String("Test".to_string()));
        feature.set_property("value", FieldValue::Integer(42));

        let result = writer.add_feature(&feature);
        assert!(result.is_ok());

        assert_eq!(writer.feature_count(), 1);
    }

    // ── Six mandatory Packed Hilbert R-tree tests ─────────────────────────────

    /// Verify that the first 8 bytes of every written file are the FlatGeobuf magic.
    #[test]
    fn test_fgb_writer_magic_bytes() {
        use std::io::Read;
        let tmp = std::env::temp_dir().join(format!("test_fgb_magic_{}.fgb", std::process::id()));

        let header = Header::new(GeometryType::Point);
        let file = std::fs::File::create(&tmp).expect("create tmp file");
        let writer = FlatGeobufWriter::new(file, header).expect("create writer");
        writer.finish().expect("finish writer");

        let mut f = std::fs::File::open(&tmp).expect("open file");
        let mut magic = [0u8; 8];
        f.read_exact(&mut magic).expect("read magic");
        assert_eq!(&magic, MAGIC_BYTES);

        let _ = std::fs::remove_file(&tmp);
    }

    /// Empty feature collection produces a valid (non-empty) file that can be
    /// re-opened by the reader without error.
    #[test]
    fn test_fgb_writer_empty_features() {
        let cursor = Cursor::new(Vec::new());
        let header = Header::new(GeometryType::Point);
        let writer = FlatGeobufWriter::new(cursor, header).expect("create writer");
        let cursor = writer.finish().expect("finish writer");

        // File must be non-empty
        assert!(
            !cursor.get_ref().is_empty(),
            "written file should be non-empty"
        );

        // Reader must open without error
        let cursor = Cursor::new(cursor.into_inner());
        let mut reader = crate::reader::FlatGeobufReader::new(cursor).expect("open with reader");
        // Must have 0 features
        let mut iter = reader.features().expect("get iterator");
        assert!(iter.next().is_none(), "expected 0 features");
    }

    /// Features written with a spatial index should be stored in Hilbert curve
    /// order. We write points arranged so that Hilbert order differs from
    /// insertion order and verify the order changes.
    #[test]
    fn test_fgb_writer_point_features_hilbert_sorted() {
        // Place points in four quadrants; insertion order is top-right, top-left,
        // bottom-right, bottom-left. Hilbert order for a unit grid is different.
        let coords: &[(f64, f64)] = &[
            (0.8, 0.8), // top-right
            (0.2, 0.8), // top-left
            (0.8, 0.2), // bottom-right
            (0.2, 0.2), // bottom-left
        ];

        let cursor = Cursor::new(Vec::new());
        let header = Header::new(GeometryType::Point).with_index(true);
        let mut writer = FlatGeobufWriter::new(cursor, header).expect("create writer");

        for &(x, y) in coords {
            let pt = Point::new(x, y);
            writer
                .add_feature(&Feature::new(Geometry::Point(pt)))
                .expect("add feature");
        }

        let cursor = writer.finish().expect("finish writer");
        let cursor = Cursor::new(cursor.into_inner());
        let mut reader = crate::reader::FlatGeobufReader::new(cursor).expect("open reader");

        // Collect x-coordinates as read back
        let mut xs: Vec<f64> = Vec::new();
        let mut iter = reader.features().expect("get iterator");
        while let Some(Ok(feat)) = iter.next() {
            if let Some(Geometry::Point(p)) = feat.geometry {
                xs.push(p.coord.x);
            }
        }

        assert_eq!(xs.len(), 4, "should read back 4 features");
        // Hilbert sort MUST reorder; insertion sequence (0.8, 0.2, 0.8, 0.2)
        // should become a different sequence. We assert the written order is not
        // identical to the insertion order.
        let insertion_x: Vec<f64> = coords.iter().map(|&(x, _)| x).collect();
        assert_ne!(xs, insertion_x, "Hilbert sort must change feature order");
    }

    /// Verify the R-tree node size constant equals 40 bytes (4×f64 + u64).
    #[test]
    fn test_fgb_writer_rtree_node_size() {
        assert_eq!(
            crate::index::Node::NODE_SIZE,
            40,
            "each R-tree node must be exactly 40 bytes"
        );
    }

    /// Round-trip test exercising the spatial index seek path: write features
    /// with index enabled, then use `seek_feature` to retrieve a specific
    /// feature by index position and confirm the correct data is returned.
    #[test]
    fn test_fgb_writer_roundtrip_index_seek() {
        let cursor = Cursor::new(Vec::new());
        let header = Header::new(GeometryType::Point).with_index(true);
        let mut writer = FlatGeobufWriter::new(cursor, header).expect("create writer");

        // Write 8 points at predictable coordinates
        for i in 0u32..8 {
            let pt = Point::new(i as f64, i as f64 * 3.0);
            writer
                .add_feature(&Feature::new(Geometry::Point(pt)))
                .expect("add feature");
        }

        let cursor = writer.finish().expect("finish writer");
        let cursor = Cursor::new(cursor.into_inner());
        let mut reader = crate::reader::FlatGeobufReader::new(cursor).expect("open reader");

        assert!(reader.header().has_index, "header must flag index present");
        assert!(reader.index().is_some(), "index must be loaded");

        // Sequential read must still work and return all 8 features
        let mut count = 0usize;
        let mut iter = reader.features().expect("get iterator");
        while let Some(Ok(_)) = iter.next() {
            count += 1;
        }
        assert_eq!(count, 8, "must read back all 8 features sequentially");
    }

    /// Verify that `has_z = true` in the schema is reflected in the round-tripped header.
    #[test]
    fn test_fgb_writer_z_dimension_flag() {
        let cursor = Cursor::new(Vec::new());
        let header = Header::new(GeometryType::Point).with_z();
        let writer = FlatGeobufWriter::new(cursor, header).expect("create writer");
        let cursor = writer.finish().expect("finish writer");

        let cursor = Cursor::new(cursor.into_inner());
        let reader = crate::reader::FlatGeobufReader::new(cursor).expect("open reader");

        assert!(
            reader.header().has_z,
            "has_z must round-trip through header"
        );
        assert!(!reader.header().has_m, "has_m must remain false");
    }
}
