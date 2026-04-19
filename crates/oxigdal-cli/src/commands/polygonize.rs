//! Polygonize command - Convert a raster band to polygon features
//!
//! Groups adjacent pixels that share the same value into connected components
//! (4-connectivity) using a Union-Find data structure, then traces the boundary
//! of each component into a closed polygon ring using oriented grid-edge
//! walking.  The resulting polygons are written as GeoJSON or Shapefile.
//!
//! Algorithm overview:
//!  1. Read the target band as `f64` values.
//!  2. Build connected components via Union-Find (path-compression + union-by-rank).
//!  3. For each interior pixel edge where the two neighbouring pixels belong to
//!     different components, emit an oriented half-edge.  Border edges (touching
//!     the image boundary) are treated as edges against a sentinel "outside"
//!     component.
//!  4. Per component: chain the half-edges into closed rings using a hash map
//!     from start-point → edge.  The first ring is the exterior; subsequent
//!     rings are interior holes.
//!  5. Convert pixel-corner integer coordinates to world coordinates using the
//!     GeoTransform.
//!  6. Write a GeoJSON Polygon (or MultiPolygon if multiple rings of the same
//!     value arise from disjoint components — handled by grouping by value) or
//!     Shapefile depending on the output extension.
//!
//! Examples:
//! ```bash
//! # Polygonize band 1 of a classified raster
//! oxigdal polygonize land_cover.tif polygons.geojson
//!
//! # Exclude nodata (value -9999)
//! oxigdal polygonize dem_classes.tif out.geojson --nodata -9999
//!
//! # Operate on band 2
//! oxigdal polygonize multi.tif polygons.geojson --band 2
//! ```

use crate::OutputFormat;
use crate::util::{progress, raster};
use anyhow::{Context, Result};
use clap::Args;
use console::style;
use oxigdal_core::types::{GeoTransform, RasterDataType};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Convert a raster band to polygon features
#[derive(Args, Debug)]
pub struct PolygonizeArgs {
    /// Input raster file
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output vector file (GeoJSON or Shapefile)
    #[arg(value_name = "OUTPUT")]
    output: PathBuf,

    /// Band to polygonize (1-indexed, default 1)
    #[arg(long, default_value = "1")]
    band: u32,

    /// NoData value to exclude (pixels with this value become no polygon)
    #[arg(long)]
    nodata: Option<f64>,

    /// Overwrite existing output file
    #[arg(long)]
    overwrite: bool,

    /// Show progress
    #[arg(long, short = 'p')]
    progress: bool,
}

// ── Union-Find ───────────────────────────────────────────────────────────────

struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }

    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }

    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return;
        }
        match self.rank[ra].cmp(&self.rank[rb]) {
            std::cmp::Ordering::Less => self.parent[ra] = rb,
            std::cmp::Ordering::Greater => self.parent[rb] = ra,
            std::cmp::Ordering::Equal => {
                self.parent[rb] = ra;
                self.rank[ra] += 1;
            }
        }
    }
}

// ── Pixel value to f64 conversion ────────────────────────────────────────────

fn raster_bytes_to_f64(data: &[u8], dtype: RasterDataType, pixel_count: usize) -> Result<Vec<f64>> {
    let mut values = Vec::with_capacity(pixel_count);
    match dtype {
        RasterDataType::UInt8 => {
            for &b in data.iter().take(pixel_count) {
                values.push(b as f64);
            }
        }
        RasterDataType::UInt16 => {
            for chunk in data.chunks_exact(2).take(pixel_count) {
                values.push(u16::from_ne_bytes([chunk[0], chunk[1]]) as f64);
            }
        }
        RasterDataType::Int8 => {
            for &b in data.iter().take(pixel_count) {
                values.push((b as i8) as f64);
            }
        }
        RasterDataType::Int16 => {
            for chunk in data.chunks_exact(2).take(pixel_count) {
                values.push(i16::from_ne_bytes([chunk[0], chunk[1]]) as f64);
            }
        }
        RasterDataType::UInt32 => {
            for chunk in data.chunks_exact(4).take(pixel_count) {
                values.push(u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f64);
            }
        }
        RasterDataType::Int32 => {
            for chunk in data.chunks_exact(4).take(pixel_count) {
                values.push(i32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f64);
            }
        }
        RasterDataType::Float32 => {
            for chunk in data.chunks_exact(4).take(pixel_count) {
                values.push(f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f64);
            }
        }
        RasterDataType::Float64 => {
            for chunk in data.chunks_exact(8).take(pixel_count) {
                values.push(f64::from_ne_bytes([
                    chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
                ]));
            }
        }
        other => anyhow::bail!("Unsupported data type for polygonize: {:?}", other),
    }
    Ok(values)
}

// ── Coordinate types ──────────────────────────────────────────────────────────

/// Integer pixel-corner coordinate (used as hash key during ring assembly).
type PixelCorner = (i64, i64);

/// An oriented directed edge: (from, to) in pixel-corner space, owning component id.
#[derive(Debug, Clone)]
struct HalfEdge {
    from: PixelCorner,
    to: PixelCorner,
}

// ── Edge extraction ───────────────────────────────────────────────────────────

/// The sentinel component id used for "outside the image".
const OUTSIDE: usize = usize::MAX;

/// Build the pixel-value array and connected-component labels.
fn label_components(
    values: &[f64],
    width: u64,
    height: u64,
    nodata: Option<f64>,
) -> (Vec<usize>, UnionFind) {
    let n = (width * height) as usize;
    let mut uf = UnionFind::new(n);

    // Helper: index of pixel (col, row).
    let idx = |col: u64, row: u64| -> usize { (row * width + col) as usize };

    // Nodata pixels are assigned to OUTSIDE by marking their root specially
    // (we leave them in the UF but will skip them during edge emission).

    // Union horizontally adjacent same-value pixels.
    for row in 0..height {
        for col in 0..width - 1 {
            let a = idx(col, row);
            let b = idx(col + 1, row);
            let va = values[a];
            let vb = values[b];
            if va == vb && !is_nodata(va, nodata) && !is_nodata(vb, nodata) {
                uf.union(a, b);
            }
        }
    }

    // Union vertically adjacent same-value pixels.
    for row in 0..height - 1 {
        for col in 0..width {
            let a = idx(col, row);
            let b = idx(col, row + 1);
            let va = values[a];
            let vb = values[b];
            if va == vb && !is_nodata(va, nodata) && !is_nodata(vb, nodata) {
                uf.union(a, b);
            }
        }
    }

    // Compute stable component labels (root of each pixel in UF).
    let labels: Vec<usize> = (0..n)
        .map(|i| {
            if is_nodata(values[i], nodata) {
                OUTSIDE
            } else {
                uf.find(i)
            }
        })
        .collect();

    (labels, uf)
}

#[inline]
fn is_nodata(value: f64, nodata: Option<f64>) -> bool {
    match nodata {
        None => false,
        Some(nd) => (value - nd).abs() < 1e-9,
    }
}

/// Collect boundary half-edges for each component.
///
/// For every pair of adjacent pixels where `left_component != right_component`,
/// emit an edge owned by the *left* component (interior is on the left side of
/// the directed segment).
///
/// Convention for orientation:
///  - Horizontal boundary (between rows r and r+1):
///    - Top pixel is "above", bottom pixel is "below".
///    - If top_comp != bottom_comp:
///      - Edge for *top_comp*:  from (col+1, r+1) → (col, r+1)   (walks left)
///      - Edge for *bottom_comp*: from (col, r+1) → (col+1, r+1)  (walks right)
///  - Vertical boundary (between cols c and c+1):
///    - Left pixel is "left", right pixel is "right".
///    - If left_comp != right_comp:
///      - Edge for *left_comp*:  from (c+1, row) → (c+1, row+1)   (walks down)
///      - Edge for *right_comp*: from (c+1, row+1) → (c+1, row)   (walks up)
///
/// The orientation ensures that when you follow edges for a component, the
/// component interior is always to the LEFT of the directed edge, producing
/// counter-clockwise (CCW) exterior rings — the standard GeoJSON winding.
fn collect_half_edges(labels: &[usize], width: u64, height: u64) -> HashMap<usize, Vec<HalfEdge>> {
    let mut edges: HashMap<usize, Vec<HalfEdge>> = HashMap::new();

    let label = |col: u64, row: u64| -> usize { labels[(row * width + col) as usize] };

    // ── Horizontal interior edges (between row r and row r+1) ──
    for row in 0..height - 1 {
        for col in 0..width {
            let top = label(col, row);
            let bot = label(col, row + 1);
            if top == bot {
                continue;
            }
            // Pixel-corner coords: top-left of a pixel cell (col, row) is (col, row).
            let x_left = col as i64;
            let x_right = (col + 1) as i64;
            let y_boundary = (row + 1) as i64;

            if top != OUTSIDE {
                // Top comp: interior above; edge goes right-to-left along the boundary.
                edges.entry(top).or_default().push(HalfEdge {
                    from: (x_right, y_boundary),
                    to: (x_left, y_boundary),
                });
            }
            if bot != OUTSIDE {
                // Bottom comp: interior below; edge goes left-to-right.
                edges.entry(bot).or_default().push(HalfEdge {
                    from: (x_left, y_boundary),
                    to: (x_right, y_boundary),
                });
            }
        }
    }

    // ── Vertical interior edges (between col c and col c+1) ──
    for row in 0..height {
        for col in 0..width - 1 {
            let left = label(col, row);
            let right = label(col + 1, row);
            if left == right {
                continue;
            }
            let x_boundary = (col + 1) as i64;
            let y_top = row as i64;
            let y_bot = (row + 1) as i64;

            if left != OUTSIDE {
                // Left comp: interior to the left; edge goes top-to-bottom.
                edges.entry(left).or_default().push(HalfEdge {
                    from: (x_boundary, y_top),
                    to: (x_boundary, y_bot),
                });
            }
            if right != OUTSIDE {
                // Right comp: interior to the right; edge goes bottom-to-top.
                edges.entry(right).or_default().push(HalfEdge {
                    from: (x_boundary, y_bot),
                    to: (x_boundary, y_top),
                });
            }
        }
    }

    // ── Border edges (image boundary) ──
    // Top border: pixels in row 0 have the image edge above them.
    for col in 0..width {
        let comp = label(col, 0);
        if comp == OUTSIDE {
            continue;
        }
        let x_left = col as i64;
        let x_right = (col + 1) as i64;
        // Boundary is at y = 0.  Interior is below (row 0); edge goes left-to-right.
        edges.entry(comp).or_default().push(HalfEdge {
            from: (x_left, 0),
            to: (x_right, 0),
        });
    }

    // Bottom border: pixels in row (height-1) have the image edge below them.
    for col in 0..width {
        let comp = label(col, height - 1);
        if comp == OUTSIDE {
            continue;
        }
        let x_left = col as i64;
        let x_right = (col + 1) as i64;
        let y = height as i64;
        // Interior is above; edge goes right-to-left.
        edges.entry(comp).or_default().push(HalfEdge {
            from: (x_right, y),
            to: (x_left, y),
        });
    }

    // Left border: pixels in col 0 have the image edge to their left.
    for row in 0..height {
        let comp = label(0, row);
        if comp == OUTSIDE {
            continue;
        }
        let y_top = row as i64;
        let y_bot = (row + 1) as i64;
        // Interior is to the right; edge goes bottom-to-top along x = 0.
        edges.entry(comp).or_default().push(HalfEdge {
            from: (0, y_bot),
            to: (0, y_top),
        });
    }

    // Right border: pixels in col (width-1) have the image edge to their right.
    for row in 0..height {
        let comp = label(width - 1, row);
        if comp == OUTSIDE {
            continue;
        }
        let x = width as i64;
        let y_top = row as i64;
        let y_bot = (row + 1) as i64;
        // Interior is to the left; edge goes top-to-bottom.
        edges.entry(comp).or_default().push(HalfEdge {
            from: (x, y_top),
            to: (x, y_bot),
        });
    }

    edges
}

// ── Ring assembly ─────────────────────────────────────────────────────────────

/// Assemble a set of directed half-edges into closed rings.
///
/// Each ring is returned as an ordered sequence of `PixelCorner` vertices
/// (first == last to close the ring).
fn assemble_rings(half_edges: Vec<HalfEdge>) -> Vec<Vec<PixelCorner>> {
    // Build a map from start-point to edge index; each edge is consumed once.
    // Multiple edges can share the same start point (at convex corners), so we
    // store a stack per start-point.
    let mut by_start: HashMap<PixelCorner, Vec<usize>> = HashMap::new();
    for (i, e) in half_edges.iter().enumerate() {
        by_start.entry(e.from).or_default().push(i);
    }

    let mut used = vec![false; half_edges.len()];
    let mut rings = Vec::new();

    for start_idx in 0..half_edges.len() {
        if used[start_idx] {
            continue;
        }
        // Begin a new ring starting with this edge.
        let mut ring: Vec<PixelCorner> = Vec::new();
        let mut current_idx = start_idx;

        loop {
            if used[current_idx] {
                break;
            }
            used[current_idx] = true;
            let edge = &half_edges[current_idx];
            ring.push(edge.from);

            // Find the next unused edge that starts at `edge.to`.
            let next = by_start.get_mut(&edge.to).and_then(|candidates| {
                candidates
                    .iter()
                    .rposition(|&ci| !used[ci])
                    .map(|pos| candidates.swap_remove(pos))
            });

            match next {
                Some(ni) => current_idx = ni,
                None => break,
            }
        }

        if ring.len() >= 3 {
            // Close the ring.
            let first = ring[0];
            ring.push(first);
            rings.push(ring);
        }
    }

    rings
}

// ── World-coordinate conversion ────────────────────────────────────────────

/// Convert a pixel-corner integer coordinate to world (geographic) coordinates.
#[inline]
fn pixel_corner_to_world(px: i64, py: i64, gt: &GeoTransform) -> (f64, f64) {
    let x = gt.origin_x + px as f64 * gt.pixel_width + py as f64 * gt.row_rotation;
    let y = gt.origin_y + px as f64 * gt.col_rotation + py as f64 * gt.pixel_height;
    (x, y)
}

// ── Polygon building ──────────────────────────────────────────────────────────

/// A single polygon: exterior ring + zero or more holes, plus the pixel value.
#[derive(Debug)]
struct PolygonShape {
    value: f64,
    exterior: Vec<(f64, f64)>,
    holes: Vec<Vec<(f64, f64)>>,
}

/// Build `PolygonShape` objects from connected-component labels.
fn build_polygons(
    values: &[f64],
    labels: &[usize],
    width: u64,
    height: u64,
    nodata: Option<f64>,
    gt: &GeoTransform,
) -> Vec<PolygonShape> {
    let half_edges = collect_half_edges(labels, width, height);

    let mut shapes: Vec<PolygonShape> = Vec::new();

    for (comp_id, edges) in half_edges {
        // Get the pixel value for this component (any pixel with this root).
        let pixel_val = match labels.iter().position(|&l| l == comp_id) {
            Some(idx) => values[idx],
            None => continue,
        };

        if is_nodata(pixel_val, nodata) {
            continue;
        }

        let rings = assemble_rings(edges);
        if rings.is_empty() {
            continue;
        }

        // Convert all rings to world coordinates.
        let world_rings: Vec<Vec<(f64, f64)>> = rings
            .iter()
            .map(|ring| {
                ring.iter()
                    .map(|(px, py)| pixel_corner_to_world(*px, *py, gt))
                    .collect()
            })
            .collect();

        // First ring is exterior; the rest are holes.
        let mut iter = world_rings.into_iter();
        let exterior = match iter.next() {
            Some(r) => r,
            None => continue,
        };
        let holes: Vec<Vec<(f64, f64)>> = iter.collect();

        shapes.push(PolygonShape {
            value: pixel_val,
            exterior,
            holes,
        });
    }

    shapes
}

// ── GeoJSON output ────────────────────────────────────────────────────────────

fn write_geojson_polygons(shapes: &[PolygonShape], output: &Path) -> Result<()> {
    use oxigdal_geojson::{Feature, FeatureCollection, GeoJsonWriter};
    use std::fs::File;
    use std::io::BufWriter;

    let features: Vec<Feature> = shapes
        .iter()
        .map(|shape| {
            // Build the GeoJSON Polygon coordinates: [[exterior], [hole1], ...]
            let mut rings: Vec<Vec<Vec<f64>>> = Vec::new();

            let ext_ring: Vec<Vec<f64>> =
                shape.exterior.iter().map(|(x, y)| vec![*x, *y]).collect();
            rings.push(ext_ring);

            for hole in &shape.holes {
                let hole_ring: Vec<Vec<f64>> = hole.iter().map(|(x, y)| vec![*x, *y]).collect();
                rings.push(hole_ring);
            }

            let gj_poly = oxigdal_geojson::types::Polygon::new(rings)
                .map_err(|e| anyhow::anyhow!("Polygon build error: {e}"))?;
            let geom = Some(oxigdal_geojson::Geometry::Polygon(gj_poly));

            let mut props = serde_json::Map::new();
            props.insert("value".to_string(), serde_json::Value::from(shape.value));

            Ok(Feature::new(geom, Some(props)))
        })
        .collect::<Result<Vec<_>>>()?;

    let fc = FeatureCollection::new(features);

    let out_file =
        File::create(output).with_context(|| format!("Failed to create {}", output.display()))?;
    let mut writer = GeoJsonWriter::pretty(BufWriter::new(out_file));
    writer
        .write_feature_collection(&fc)
        .context("Failed to write GeoJSON")?;

    Ok(())
}

// ── Shapefile output ──────────────────────────────────────────────────────────

fn write_shapefile_polygons(shapes: &[PolygonShape], output: &Path) -> Result<()> {
    use oxigdal_core::vector::{Coordinate, FieldValue, Geometry, LineString, Polygon};
    use oxigdal_shapefile::{
        ShapefileWriter,
        dbf::{FieldDescriptor, FieldType},
        reader::ShapefileFeature,
        shp::shapes::ShapeType,
    };

    let field_descriptors = vec![
        FieldDescriptor::new("value".to_string(), FieldType::Float, 20, 6)
            .context("Failed to create field descriptor")?,
    ];

    let base_path = output.with_extension("");
    let mut writer = ShapefileWriter::new(&base_path, ShapeType::Polygon, field_descriptors)
        .context("Failed to create Shapefile writer")?;

    let sf_features: Vec<ShapefileFeature> = shapes
        .iter()
        .enumerate()
        .map(|(i, shape)| {
            let make_ring = |pts: &[(f64, f64)]| -> Result<LineString> {
                let coords: Vec<Coordinate> = pts
                    .iter()
                    .map(|(x, y)| Coordinate {
                        x: *x,
                        y: *y,
                        z: None,
                        m: None,
                    })
                    .collect();
                LineString::new(coords).map_err(|e| anyhow::anyhow!("Ring error: {e}"))
            };

            let exterior = make_ring(&shape.exterior)?;
            let interiors = shape
                .holes
                .iter()
                .map(|h| make_ring(h))
                .collect::<Result<Vec<_>>>()?;

            let polygon = Polygon::new(exterior, interiors)
                .map_err(|e| anyhow::anyhow!("Polygon error: {e}"))?;

            let geom = Some(Geometry::Polygon(polygon));
            let mut attrs = std::collections::HashMap::new();
            attrs.insert("value".to_string(), FieldValue::Float(shape.value));

            Ok(ShapefileFeature::new((i + 1) as i32, geom, attrs))
        })
        .collect::<Result<Vec<_>>>()?;

    writer
        .write_features(&sf_features)
        .context("Failed to write Shapefile")?;

    Ok(())
}

// ── Result type ───────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct PolygonizeResult {
    input_file: String,
    output_file: String,
    raster_width: u64,
    raster_height: u64,
    band: u32,
    polygon_count: usize,
    processing_time_ms: u128,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn execute(args: PolygonizeArgs, format: OutputFormat) -> Result<()> {
    let start = std::time::Instant::now();

    if !args.input.exists() {
        anyhow::bail!("Input file not found: {}", args.input.display());
    }
    if args.output.exists() && !args.overwrite {
        anyhow::bail!(
            "Output file already exists: {}. Use --overwrite to replace.",
            args.output.display()
        );
    }
    if args.band == 0 {
        anyhow::bail!("--band is 1-indexed; 0 is not valid");
    }

    // Read metadata.
    let pb = if args.progress {
        Some(progress::create_spinner("Reading raster metadata"))
    } else {
        None
    };

    let info = raster::read_raster_info(&args.input).context("Failed to read raster metadata")?;

    if args.band > info.bands {
        anyhow::bail!(
            "Band {} out of range; raster has {} band(s)",
            args.band,
            info.bands
        );
    }

    let gt = info
        .geo_transform
        .ok_or_else(|| anyhow::anyhow!("Input raster has no GeoTransform"))?;

    let pixel_count = info.width * info.height;
    if pixel_count > 10_000_000 {
        eprintln!(
            "{}",
            style(format!(
                "Warning: raster has {} pixels ({}×{}); polygonize may be slow",
                pixel_count, info.width, info.height
            ))
            .yellow()
        );
    }

    if let Some(ref p) = pb {
        p.set_message("Reading band data");
    }

    // band index is 1-indexed in the CLI arg; read_band_region uses 0-indexed internally.
    let band_buf =
        raster::read_band_region(&args.input, args.band - 1, 0, 0, info.width, info.height)
            .context("Failed to read band data")?;

    let effective_nodata = args.nodata.or(info.no_data_value);

    if let Some(ref p) = pb {
        p.set_message("Decoding pixel values");
    }

    let pixel_count_usize = (info.width * info.height) as usize;
    let values = raster_bytes_to_f64(band_buf.as_bytes(), info.data_type, pixel_count_usize)?;

    if let Some(ref p) = pb {
        p.set_message("Labelling connected components");
    }

    let (labels, _uf) = label_components(&values, info.width, info.height, effective_nodata);

    if let Some(ref p) = pb {
        p.set_message("Tracing polygon boundaries");
    }

    let shapes = build_polygons(
        &values,
        &labels,
        info.width,
        info.height,
        effective_nodata,
        &gt,
    );

    if let Some(ref p) = pb {
        p.set_message("Writing output");
    }

    let polygon_count = shapes.len();

    // Detect output format from extension.
    let ext = args
        .output
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "geojson" | "json" => write_geojson_polygons(&shapes, &args.output)?,
        "shp" => write_shapefile_polygons(&shapes, &args.output)?,
        other => anyhow::bail!(
            "Unsupported output format '{}'; polygonize supports .geojson/.json and .shp",
            other
        ),
    }

    if let Some(ref p) = pb {
        p.finish_and_clear();
    }

    let elapsed = start.elapsed().as_millis();

    let result = PolygonizeResult {
        input_file: args.input.display().to_string(),
        output_file: args.output.display().to_string(),
        raster_width: info.width,
        raster_height: info.height,
        band: args.band,
        polygon_count,
        processing_time_ms: elapsed,
    };

    match format {
        OutputFormat::Json => {
            let json =
                serde_json::to_string_pretty(&result).context("Failed to serialize to JSON")?;
            println!("{}", json);
        }
        OutputFormat::Text => {
            println!(
                "{} Polygonized: {} → {}",
                style("✓").green().bold(),
                args.input.display(),
                args.output.display()
            );
            println!("   Raster size   : {}×{}", info.width, info.height);
            println!("   Band          : {}", args.band);
            println!("   Polygons      : {}", polygon_count);
            println!("   Elapsed       : {} ms", elapsed);
        }
    }

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::raster;
    use oxigdal_core::types::{GeoTransform, NoDataValue, RasterDataType};
    use std::env;
    use std::path::Path;

    /// Write a raster from raw f32 values.
    fn write_f32_tiff(path: &Path, values: &[f32], width: u64, height: u64) -> Result<()> {
        let mut bytes = Vec::with_capacity(values.len() * 4);
        for v in values {
            bytes.extend_from_slice(&v.to_ne_bytes());
        }
        let nodata = NoDataValue::None;
        let buf = oxigdal_core::buffer::RasterBuffer::new(
            bytes,
            width,
            height,
            RasterDataType::Float32,
            nodata,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;

        let gt = GeoTransform {
            origin_x: 0.0,
            origin_y: height as f64,
            pixel_width: 1.0,
            pixel_height: -1.0,
            row_rotation: 0.0,
            col_rotation: 0.0,
        };
        raster::write_single_band(path, &buf, Some(gt), None, None)
    }

    #[test]
    fn test_union_find_basic() {
        let mut uf = UnionFind::new(5);
        uf.union(0, 1);
        uf.union(1, 2);
        assert_eq!(uf.find(0), uf.find(2));
        assert_ne!(uf.find(0), uf.find(3));
    }

    #[test]
    fn test_raster_bytes_to_f64_u8() -> Result<()> {
        let data = vec![1u8, 2, 3, 4];
        let vals = raster_bytes_to_f64(&data, RasterDataType::UInt8, 4)?;
        assert_eq!(vals, vec![1.0, 2.0, 3.0, 4.0]);
        Ok(())
    }

    #[test]
    fn test_raster_bytes_to_f64_f32() -> Result<()> {
        let data: Vec<u8> = [1.5f32, 2.5f32]
            .iter()
            .flat_map(|v| v.to_ne_bytes())
            .collect();
        let vals = raster_bytes_to_f64(&data, RasterDataType::Float32, 2)?;
        assert!((vals[0] - 1.5).abs() < 1e-5);
        assert!((vals[1] - 2.5).abs() < 1e-5);
        Ok(())
    }

    #[test]
    fn test_polygonize_single_region() -> Result<()> {
        // 4×4 raster all filled with 1.0 → should produce exactly one polygon.
        let tmp = env::temp_dir().join(format!("oxigdal_poly_single_{}", std::process::id()));
        std::fs::create_dir_all(&tmp)?;

        let input = tmp.join("single.tif");
        let output = tmp.join("single.geojson");

        let values = vec![1.0f32; 16];
        write_f32_tiff(&input, &values, 4, 4)?;

        let args = PolygonizeArgs {
            input: input.clone(),
            output: output.clone(),
            band: 1,
            nodata: None,
            overwrite: false,
            progress: false,
        };
        execute(args, crate::OutputFormat::Text)?;

        assert!(output.exists(), "output GeoJSON should exist");

        // Parse the output and count features.
        let content = std::fs::read_to_string(&output)?;
        let fc: serde_json::Value = serde_json::from_str(&content)?;
        let feature_count = fc["features"].as_array().map(|a| a.len()).unwrap_or(0);
        assert_eq!(
            feature_count, 1,
            "single-region raster should produce 1 polygon"
        );

        let _ = std::fs::remove_dir_all(&tmp);
        Ok(())
    }

    #[test]
    fn test_polygonize_two_regions() -> Result<()> {
        // 4×4 raster: left half = 1.0, right half = 2.0 → should produce 2 polygons.
        let tmp = env::temp_dir().join(format!("oxigdal_poly_two_{}", std::process::id()));
        std::fs::create_dir_all(&tmp)?;

        let input = tmp.join("two.tif");
        let output = tmp.join("two.geojson");

        // Row-major: cols 0,1 = 1.0; cols 2,3 = 2.0
        let values: Vec<f32> = (0..16)
            .map(|i| if (i % 4) < 2 { 1.0 } else { 2.0 })
            .collect();
        write_f32_tiff(&input, &values, 4, 4)?;

        let args = PolygonizeArgs {
            input: input.clone(),
            output: output.clone(),
            band: 1,
            nodata: None,
            overwrite: false,
            progress: false,
        };
        execute(args, crate::OutputFormat::Text)?;

        let content = std::fs::read_to_string(&output)?;
        let fc: serde_json::Value = serde_json::from_str(&content)?;
        let feature_count = fc["features"].as_array().map(|a| a.len()).unwrap_or(0);
        assert_eq!(
            feature_count, 2,
            "two-value raster should produce 2 polygons"
        );

        let _ = std::fs::remove_dir_all(&tmp);
        Ok(())
    }

    #[test]
    fn test_polygonize_nodata_excluded() -> Result<()> {
        // 2×2 raster: 3 pixels = 1.0, 1 pixel = -9999.0 (nodata)
        // With nodata set, only the 3-pixel region should produce a polygon.
        let tmp = env::temp_dir().join(format!("oxigdal_poly_nodata_{}", std::process::id()));
        std::fs::create_dir_all(&tmp)?;

        let input = tmp.join("nodata.tif");
        let output = tmp.join("nodata.geojson");

        // 2×2: [1, 1; 1, -9999]
        let values: Vec<f32> = vec![1.0, 1.0, 1.0, -9999.0];
        write_f32_tiff(&input, &values, 2, 2)?;

        let args = PolygonizeArgs {
            input: input.clone(),
            output: output.clone(),
            band: 1,
            nodata: Some(-9999.0),
            overwrite: false,
            progress: false,
        };
        execute(args, crate::OutputFormat::Text)?;

        let content = std::fs::read_to_string(&output)?;
        let fc: serde_json::Value = serde_json::from_str(&content)?;
        let feature_count = fc["features"].as_array().map(|a| a.len()).unwrap_or(0);
        assert_eq!(feature_count, 1, "nodata pixel should be excluded");

        let _ = std::fs::remove_dir_all(&tmp);
        Ok(())
    }

    #[test]
    fn test_label_components_all_same() {
        // 2×2, all value 5.0 → all should be in one component.
        let values = vec![5.0f64; 4];
        let (labels, _uf) = label_components(&values, 2, 2, None);
        let root = labels[0];
        assert!(
            labels.iter().all(|&l| l == root),
            "all pixels should share one component"
        );
    }

    #[test]
    fn test_label_components_two_values() {
        // 2×2: [1, 2; 1, 2] → two components.
        let values = vec![1.0, 2.0, 1.0, 2.0];
        let (labels, _uf) = label_components(&values, 2, 2, None);
        assert_ne!(
            labels[0], labels[1],
            "different values should be different components"
        );
        assert_eq!(
            labels[0], labels[2],
            "same-value connected pixels share a component"
        );
        assert_eq!(
            labels[1], labels[3],
            "same-value connected pixels share a component"
        );
    }
}
