//! Generates `demo/geoparquet/sample/jpn-sample.parquet` — the offline sample
//! dataset for the GeoParquet Live demo (`demo/geoparquet/`).
//!
//! The file mirrors the layout of the real VIDA
//! `google-microsoft-osm-open-buildings` GeoParquet so every code path the
//! demo exercises against the 5.9 GB remote file also works offline:
//!
//! * SNAPPY-compressed column chunks,
//! * WKB geometry column named `geometry`,
//! * a covering bounding box stored in a struct column literally named
//!   `bbox` with children `xmin`, `xmax`, `ymin`, `ymax` (VIDA field order),
//! * a GeoParquet 1.1 `covering.bbox` object in the `geo` metadata,
//! * `area_in_meters` and `confidence` attribute columns,
//! * three row groups in disjoint spatial zones (Shinjuku, Tachikawa,
//!   Yokohama) so bounding-box row-group pruning is observable.
//!
//! The output is fully deterministic (seeded xorshift PRNG), so re-running
//! the generator reproduces the same file byte-for-byte.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p oxigeo-geoparquet --example generate_sample --features snappy
//! ```
//!
//! An alternative output path may be passed as the first CLI argument.

/// Entry point used when the required Cargo features are missing: fails with
/// an instructive message instead of producing a broken file.
#[cfg(not(all(feature = "std", feature = "snappy")))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    Err(
        "generate_sample requires the `snappy` feature (and default `std`): \
         cargo run -p oxigeo-geoparquet --example generate_sample --features snappy"
            .into(),
    )
}

#[cfg(all(feature = "std", feature = "snappy"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    generator::run()
}

#[cfg(all(feature = "std", feature = "snappy"))]
mod generator {
    use arrow_array::{ArrayRef, BinaryArray, Float64Array, RecordBatch, StructArray};
    use arrow_schema::{DataType, Field, Fields, Schema};
    use oxigeo_geoparquet::GeoParquetReader;
    use oxigeo_geoparquet::geometry::{Coordinate, Geometry, LineString, Polygon, WkbWriter};
    use oxigeo_geoparquet::metadata::{Covering, Crs, GeoParquetMetadata, GeometryColumnMetadata};
    use oxigeo_geoparquet::plan::plan_pushdown;
    use oxigeo_geoparquet::predicate::{AttributeFilter, CmpOp, ScalarValue};
    use parquet::arrow::ArrowWriter;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use parquet::basic::Compression;
    use parquet::file::properties::WriterProperties;
    use parquet::schema::types::ColumnPath;
    use std::error::Error;
    use std::fs::File;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    /// Repo-root-relative default output location (never an absolute path).
    const DEFAULT_OUTPUT: &str = "demo/geoparquet/sample/jpn-sample.parquet";

    /// Name of the WKB geometry column (matches VIDA).
    const GEOM_COL: &str = "geometry";

    /// Coordinate grid resolution: all building corners snap to integer
    /// multiples of 2⁻¹⁴ degrees (≈ 5.5 m E-W / 6.8 m N-S around Tokyo).
    /// Grid-aligned values are exact binary fractions, so the low mantissa
    /// bytes of every stored `f64` are zero — which SNAPPY compresses well.
    const GRID_PER_DEG: f64 = 16384.0;

    /// Metres per degree of longitude at ≈ 35.6° N.
    const M_PER_DEG_LON: f64 = 90_409.0;

    /// Metres per degree of latitude at ≈ 35.6° N.
    const M_PER_DEG_LAT: f64 = 110_946.0;

    /// Buildings per zone; one zone = one row group.
    const BUILDINGS_PER_ZONE: usize = 2_000;

    /// Footprint edge lengths are 1..=MAX_FOOTPRINT_UNITS grid cells,
    /// i.e. roughly 5–44 m E-W and 7–54 m N-S.
    const MAX_FOOTPRINT_UNITS: u64 = 8;

    /// The Shinjuku preset bounding box used by the demo (and the E2E
    /// plan): it must select exactly the first row group.
    const SHINJUKU_BBOX: (f64, f64, f64, f64) = (139.69, 35.67, 139.72, 35.70);

    /// One synthetic spatial zone → one Parquet row group.
    struct Zone {
        /// Human-readable label used in progress output.
        name: &'static str,
        /// Zone extent in degrees (buildings stay strictly inside).
        lon_min: f64,
        lat_min: f64,
        lon_max: f64,
        lat_max: f64,
        /// PRNG seed making the zone deterministic.
        seed: u64,
    }

    /// Three disjoint Tokyo-area zones. Zone order = row-group order, so the
    /// Shinjuku preset box prunes to row group 0 only.
    const ZONES: [Zone; 3] = [
        Zone {
            name: "Shinjuku (central Tokyo)",
            lon_min: 139.69,
            lat_min: 35.67,
            lon_max: 139.72,
            lat_max: 35.70,
            seed: 0x5EED_0000_0000_0001,
        },
        Zone {
            name: "Tachikawa (west Tokyo)",
            lon_min: 139.40,
            lat_min: 35.69,
            lon_max: 139.43,
            lat_max: 35.72,
            seed: 0x5EED_0000_0000_0002,
        },
        Zone {
            name: "Yokohama (Kannai)",
            lon_min: 139.61,
            lat_min: 35.43,
            lon_max: 139.64,
            lat_max: 35.46,
            seed: 0x5EED_0000_0000_0003,
        },
    ];

    /// An axis-aligned building footprint on the coordinate grid.
    ///
    /// Corner coordinates are stored as integer grid indices; degrees are
    /// recovered as `index / GRID_PER_DEG`, which is exact in `f64`.
    struct Building {
        x_k: i64,
        y_k: i64,
        w_u: i64,
        h_u: i64,
        confidence: f64,
    }

    impl Building {
        fn lon_min(&self) -> f64 {
            self.x_k as f64 / GRID_PER_DEG
        }
        fn lon_max(&self) -> f64 {
            (self.x_k + self.w_u) as f64 / GRID_PER_DEG
        }
        fn lat_min(&self) -> f64 {
            self.y_k as f64 / GRID_PER_DEG
        }
        fn lat_max(&self) -> f64 {
            (self.y_k + self.h_u) as f64 / GRID_PER_DEG
        }
        /// Footprint area in square metres (width × height, both exact
        /// multiples of the grid pitch → a small set of distinct values,
        /// which keeps the dictionary-encoded column tiny).
        fn area_m2(&self) -> f64 {
            let w_m = self.w_u as f64 / GRID_PER_DEG * M_PER_DEG_LON;
            let h_m = self.h_u as f64 / GRID_PER_DEG * M_PER_DEG_LAT;
            w_m * h_m
        }
    }

    /// Minimal deterministic PRNG (xorshift64*): no external dependencies,
    /// stable output across platforms and releases.
    struct XorShift64Star {
        state: u64,
    }

    impl XorShift64Star {
        fn new(seed: u64) -> Self {
            // Any non-zero state is valid.
            Self { state: seed.max(1) }
        }

        fn next_u64(&mut self) -> u64 {
            let mut x = self.state;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.state = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        /// Uniform-ish value in `0..n` (modulo bias is irrelevant for
        /// synthetic demo data).
        fn next_below(&mut self, n: u64) -> u64 {
            self.next_u64() % n.max(1)
        }
    }

    /// Generates one zone's buildings, strictly inside the zone rectangle,
    /// sorted by (south→north, west→east) for spatial locality — real
    /// building datasets exhibit the same ordering, and it improves both
    /// dictionary-index RLE and SNAPPY match rates.
    fn generate_zone(zone: &Zone) -> Vec<Building> {
        let mut rng = XorShift64Star::new(zone.seed);
        let k0_x = (zone.lon_min * GRID_PER_DEG).ceil() as i64;
        let k1_x = (zone.lon_max * GRID_PER_DEG).floor() as i64;
        let k0_y = (zone.lat_min * GRID_PER_DEG).ceil() as i64;
        let k1_y = (zone.lat_max * GRID_PER_DEG).floor() as i64;

        let mut buildings: Vec<Building> = (0..BUILDINGS_PER_ZONE)
            .map(|_| {
                let w_u = 1 + rng.next_below(MAX_FOOTPRINT_UNITS) as i64;
                let h_u = 1 + rng.next_below(MAX_FOOTPRINT_UNITS) as i64;
                let x_span = (k1_x - k0_x - w_u).max(0) as u64;
                let y_span = (k1_y - k0_y - h_u).max(0) as u64;
                let x_k = k0_x + rng.next_below(x_span + 1) as i64;
                let y_k = k0_y + rng.next_below(y_span + 1) as i64;
                // Two-decimal ML-style confidence in 0.65..=0.98.
                let confidence = 0.65 + rng.next_below(34) as f64 / 100.0;
                Building {
                    x_k,
                    y_k,
                    w_u,
                    h_u,
                    confidence,
                }
            })
            .collect();
        buildings.sort_by_key(|b| (b.y_k, b.x_k));
        buildings
    }

    /// Encodes a building footprint as a closed, counter-clockwise WKB
    /// polygon ring (5 points, little-endian).
    fn polygon_wkb(b: &Building) -> Result<Vec<u8>, Box<dyn Error>> {
        let (x0, y0, x1, y1) = (b.lon_min(), b.lat_min(), b.lon_max(), b.lat_max());
        let ring = LineString::new(vec![
            Coordinate::new_2d(x0, y0),
            Coordinate::new_2d(x1, y0),
            Coordinate::new_2d(x1, y1),
            Coordinate::new_2d(x0, y1),
            Coordinate::new_2d(x0, y0),
        ]);
        let geom = Geometry::Polygon(Polygon::new_simple(ring));
        let mut writer = WkbWriter::new(true);
        Ok(writer.write_geometry(&geom)?)
    }

    /// Ordered struct children matching the VIDA layout: xmin, xmax, ymin, ymax.
    fn bbox_fields() -> Fields {
        Fields::from(vec![
            Field::new("xmin", DataType::Float64, false),
            Field::new("xmax", DataType::Float64, false),
            Field::new("ymin", DataType::Float64, false),
            Field::new("ymax", DataType::Float64, false),
        ])
    }

    /// GeoParquet `geo` metadata mirroring VIDA: WKB polygons, WGS 84, and a
    /// `covering.bbox` object pointing at the plain `bbox` struct column.
    fn geo_metadata(extent: [f64; 4]) -> GeoParquetMetadata {
        let col_meta = GeometryColumnMetadata::new_wkb()
            .with_crs(Crs::wgs84())
            .with_geometry_types(vec!["Polygon".to_string()])
            .with_bbox(extent.to_vec())
            .with_covering(Covering::bbox_struct("bbox"));
        let mut geo = GeoParquetMetadata::new(GEOM_COL);
        geo.add_column(GEOM_COL, col_meta);
        geo
    }

    /// Arrow schema: geometry + bbox struct + area_in_meters + confidence,
    /// with the serialized `geo` JSON attached as schema metadata.
    fn build_schema(geo: &GeoParquetMetadata) -> Result<Arc<Schema>, Box<dyn Error>> {
        let base = Schema::new(vec![
            Field::new(GEOM_COL, DataType::Binary, true),
            Field::new("bbox", DataType::Struct(bbox_fields()), false),
            Field::new("area_in_meters", DataType::Float64, false),
            Field::new("confidence", DataType::Float64, false),
        ]);
        let mut schema_meta = base.metadata().clone();
        schema_meta.insert("geo".to_string(), geo.to_json()?);
        Ok(Arc::new(base.with_metadata(schema_meta)))
    }

    /// Converts one zone's buildings into a `RecordBatch` (= one row group).
    fn build_batch(
        schema: Arc<Schema>,
        buildings: &[Building],
    ) -> Result<RecordBatch, Box<dyn Error>> {
        let mut wkb_blobs: Vec<Vec<u8>> = Vec::with_capacity(buildings.len());
        for b in buildings {
            wkb_blobs.push(polygon_wkb(b)?);
        }
        let wkb_refs: Vec<Option<&[u8]>> = wkb_blobs.iter().map(|v| Some(v.as_slice())).collect();
        let geom_array: ArrayRef = Arc::new(BinaryArray::from(wkb_refs));

        let xmin: ArrayRef = Arc::new(Float64Array::from_iter_values(
            buildings.iter().map(Building::lon_min),
        ));
        let xmax: ArrayRef = Arc::new(Float64Array::from_iter_values(
            buildings.iter().map(Building::lon_max),
        ));
        let ymin: ArrayRef = Arc::new(Float64Array::from_iter_values(
            buildings.iter().map(Building::lat_min),
        ));
        let ymax: ArrayRef = Arc::new(Float64Array::from_iter_values(
            buildings.iter().map(Building::lat_max),
        ));
        let bbox = StructArray::new(bbox_fields(), vec![xmin, xmax, ymin, ymax], None);

        let areas: ArrayRef = Arc::new(Float64Array::from_iter_values(
            buildings.iter().map(Building::area_m2),
        ));
        let confidence: ArrayRef = Arc::new(Float64Array::from_iter_values(
            buildings.iter().map(|b| b.confidence),
        ));

        Ok(RecordBatch::try_new(
            schema,
            vec![geom_array, Arc::new(bbox), areas, confidence],
        )?)
    }

    /// Dataset extent [lon_min, lat_min, lon_max, lat_max] over all zones.
    fn dataset_extent(zones: &[Vec<Building>]) -> [f64; 4] {
        let mut extent = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for b in zones.iter().flatten() {
            extent[0] = extent[0].min(b.lon_min());
            extent[1] = extent[1].min(b.lat_min());
            extent[2] = extent[2].max(b.lon_max());
            extent[3] = extent[3].max(b.lat_max());
        }
        extent
    }

    /// Fails with `msg` when `cond` is false (panic-free assertion).
    fn ensure(cond: bool, msg: String) -> Result<(), Box<dyn Error>> {
        if cond { Ok(()) } else { Err(msg.into()) }
    }

    fn total_rows(batches: &[RecordBatch]) -> usize {
        batches.iter().map(RecordBatch::num_rows).sum()
    }

    /// Writes the sample file: one flushed row group per zone, SNAPPY
    /// everywhere, dictionary encoding disabled only for the all-distinct
    /// WKB geometry column.
    fn write_sample(out_path: &Path) -> Result<(), Box<dyn Error>> {
        let zones: Vec<Vec<Building>> = ZONES.iter().map(generate_zone).collect();
        let geo = geo_metadata(dataset_extent(&zones));
        let schema = build_schema(&geo)?;

        let props = WriterProperties::builder()
            .set_compression(Compression::SNAPPY)
            .set_column_dictionary_enabled(ColumnPath::from(GEOM_COL), false)
            .build();
        let file = File::create(out_path)?;
        let mut writer = ArrowWriter::try_new(file, schema.clone(), Some(props))?;
        for (zone, buildings) in ZONES.iter().zip(&zones) {
            let batch = build_batch(schema.clone(), buildings)?;
            writer.write(&batch)?;
            writer.flush()?; // one row group per zone
            println!(
                "  wrote row group: {} ({} buildings)",
                zone.name,
                buildings.len()
            );
        }
        writer.close()?;
        Ok(())
    }

    /// Re-opens the generated file and checks every property the demo (and
    /// the pushdown planner) relies on.
    fn verify(out_path: &Path) -> Result<(), Box<dyn Error>> {
        let expected_rows = (BUILDINGS_PER_ZONE * ZONES.len()) as i64;

        // 1. Structure: 3 row groups, 6,000 rows, GeoParquet metadata intact.
        let reader = GeoParquetReader::open(out_path)?;
        ensure(
            reader.num_row_groups() == ZONES.len(),
            format!(
                "expected {} row groups, got {}",
                ZONES.len(),
                reader.num_row_groups()
            ),
        )?;
        ensure(
            reader.num_rows() == expected_rows,
            format!("expected {expected_rows} rows, got {}", reader.num_rows()),
        )?;
        ensure(
            reader.geometry_column_name() == GEOM_COL,
            format!(
                "unexpected geometry column {:?}",
                reader.geometry_column_name()
            ),
        )?;

        // 2. Metadata-only planner: the Shinjuku box must prune to RG 0
        //    via the covering bbox columns.
        let parquet_meta = ParquetRecordBatchReaderBuilder::try_new(File::open(out_path)?)?
            .metadata()
            .clone();
        let geo = reader.metadata().clone();
        let plan = plan_pushdown(
            &parquet_meta,
            &geo,
            GEOM_COL,
            Some(SHINJUKU_BBOX),
            &[],
            &[GEOM_COL.to_string()],
        )?;
        ensure(
            plan.bbox_cols.is_some(),
            "covering bbox columns not detected from geo metadata".to_string(),
        )?;
        ensure(
            plan.row_groups == vec![0] && plan.total_row_groups == ZONES.len(),
            format!(
                "Shinjuku box should survive only RG 0, got {:?}/{}",
                plan.row_groups, plan.total_row_groups
            ),
        )?;
        println!(
            "  plan: Shinjuku box -> row groups {:?} of {}, estimated fetch {} bytes",
            plan.row_groups, plan.total_row_groups, plan.estimated_bytes
        );

        // 3. Full Shinjuku box: every zone-A building lies inside it.
        let zone_a = GeoParquetReader::open(out_path)?
            .with_bbox_filter(SHINJUKU_BBOX)
            .read_pushdown()?;
        let zone_a_rows = total_rows(&zone_a);
        ensure(
            zone_a_rows == BUILDINGS_PER_ZONE,
            format!("Shinjuku box should match {BUILDINGS_PER_ZONE} rows, got {zone_a_rows}"),
        )?;

        // 4. A quarter-size sub-box returns a strict, plausible subset.
        let sub_box = (139.70, 35.68, 139.71, 35.69);
        let sub = GeoParquetReader::open(out_path)?
            .with_bbox_filter(sub_box)
            .read_pushdown()?;
        let sub_rows = total_rows(&sub);
        ensure(
            sub_rows > 0 && sub_rows < BUILDINGS_PER_ZONE,
            format!("sub-box count {sub_rows} out of plausible range"),
        )?;

        // 5. Attribute pushdown, as used by the demo filter box.
        let area_filter = AttributeFilter::Cmp {
            col: "area_in_meters".to_string(),
            op: CmpOp::Gt,
            value: ScalarValue::Float64(1000.0),
        };
        let large = GeoParquetReader::open(out_path)?
            .with_bbox_filter(SHINJUKU_BBOX)
            .with_attribute_filter(area_filter.clone())
            .read_pushdown()?;
        let large_rows = total_rows(&large);
        ensure(
            large_rows > 100 && large_rows < 1_200,
            format!("area>1000 in Shinjuku: {large_rows} rows out of plausible range"),
        )?;

        let large_all = GeoParquetReader::open(out_path)?
            .with_attribute_filter(area_filter)
            .read_pushdown()?;
        let large_all_rows = total_rows(&large_all);
        ensure(
            large_all_rows > 900 && large_all_rows < 2_600,
            format!("area>1000 overall: {large_all_rows} rows out of plausible range"),
        )?;

        println!(
            "  verify: full box {zone_a_rows} rows, sub-box {sub_rows}, \
             area>1000 m² {large_rows} (Shinjuku) / {large_all_rows} (all zones)"
        );
        Ok(())
    }

    /// Generates, verifies, and reports the sample file.
    pub(crate) fn run() -> Result<(), Box<dyn Error>> {
        let out_path: PathBuf = std::env::args()
            .nth(1)
            .map_or_else(|| PathBuf::from(DEFAULT_OUTPUT), PathBuf::from);
        if let Some(parent) = out_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }

        println!("generating {} ...", out_path.display());
        write_sample(&out_path)?;

        let size = std::fs::metadata(&out_path)?.len();
        println!("  file size: {size} bytes (~{} KB)", size.div_ceil(1024));
        ensure(
            (100_000..=350_000).contains(&size),
            format!("file size {size} bytes outside the ~200 KB target band"),
        )?;

        verify(&out_path)?;
        println!("OK: {} is ready for the offline demo", out_path.display());
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

        use super::*;

        #[test]
        fn test_prng_is_deterministic_and_bounded() {
            let mut a = XorShift64Star::new(42);
            let mut b = XorShift64Star::new(42);
            for _ in 0..1_000 {
                let va = a.next_below(97);
                let vb = b.next_below(97);
                assert_eq!(va, vb);
                assert!(va < 97);
            }
        }

        #[test]
        fn test_buildings_snap_to_grid_and_stay_inside_zone() {
            for zone in &ZONES {
                let buildings = generate_zone(zone);
                assert_eq!(buildings.len(), BUILDINGS_PER_ZONE);
                for b in &buildings {
                    // Exact grid alignment (round-trips through f64).
                    assert_eq!((b.lon_min() * GRID_PER_DEG).fract(), 0.0);
                    assert_eq!((b.lat_max() * GRID_PER_DEG).fract(), 0.0);
                    // Strictly inside the zone rectangle.
                    assert!(b.lon_min() >= zone.lon_min && b.lon_max() <= zone.lon_max);
                    assert!(b.lat_min() >= zone.lat_min && b.lat_max() <= zone.lat_max);
                    assert!(b.area_m2() > 0.0);
                    assert!((0.65..=0.99).contains(&b.confidence));
                }
                // Sorted south→north for spatial locality.
                for w in buildings.windows(2) {
                    assert!(w[0].y_k <= w[1].y_k);
                }
            }
        }

        #[test]
        fn test_polygon_wkb_is_closed_ring() {
            let b = Building {
                x_k: 2_288_800,
                y_k: 584_500,
                w_u: 3,
                h_u: 2,
                confidence: 0.9,
            };
            let wkb = polygon_wkb(&b).expect("wkb");
            // little-endian byte order marker + polygon type (3).
            assert_eq!(wkb[0], 1);
            assert_eq!(&wkb[1..5], &3u32.to_le_bytes());
            // 1 ring, 5 points, 5 × 2 × 8 coordinate bytes.
            assert_eq!(wkb.len(), 1 + 4 + 4 + 4 + 80);
        }
    }
}
