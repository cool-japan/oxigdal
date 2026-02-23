//! Rasterize command - Convert vector geometries to raster
//!
//! Converts vector features (points, lines, polygons) into raster format.
//! Supports attribute burning, fixed value burning, and various burn modes.
//!
//! Examples:
//! ```bash
//! # Burn vector to raster with fixed value
//! oxigdal rasterize input.geojson output.tif -ts 1024 1024 -burn 255
//!
//! # Burn using attribute value
//! oxigdal rasterize input.geojson output.tif -ts 1024 1024 -a population
//!
//! # All-touched mode
//! oxigdal rasterize input.shp output.tif -ts 512 512 -burn 1 --all-touched
//! ```

use crate::OutputFormat;
use crate::util::progress;
use anyhow::{Context, Result};
use clap::Args;
use console::style;
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{GeoTransform, RasterDataType};
use oxigdal_core::vector::geometry::{
    Coordinate, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon,
};
use oxigdal_geojson::GeoJsonReader;
use serde::Serialize;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

/// Convert vector geometries to raster
#[derive(Args, Debug)]
pub struct RasterizeArgs {
    /// Input vector file
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output raster file
    #[arg(value_name = "OUTPUT")]
    output: PathBuf,

    /// Output size in pixels (width height)
    #[arg(long = "ts", num_args = 2, value_names = ["WIDTH", "HEIGHT"])]
    target_size: Option<Vec<usize>>,

    /// Output resolution (xres yres)
    #[arg(long = "tr", num_args = 2, value_names = ["XRES", "YRES"])]
    target_resolution: Option<Vec<f64>>,

    /// Output extent (minx miny maxx maxy)
    #[arg(long = "te", num_args = 4, value_names = ["MINX", "MINY", "MAXX", "MAXY"])]
    target_extent: Option<Vec<f64>>,

    /// Fixed value to burn
    #[arg(long)]
    burn: Option<f64>,

    /// Attribute field to use for burn values
    #[arg(short, long)]
    attribute: Option<String>,

    /// Initialization value for raster
    #[arg(long, default_value = "0.0")]
    init: f64,

    /// NoData value
    #[arg(long)]
    no_data: Option<f64>,

    /// Enable all-touched mode (burn all pixels touched by geometry)
    #[arg(long)]
    all_touched: bool,

    /// Add burn value to existing raster values
    #[arg(long)]
    add: bool,

    /// Invert rasterization (burn pixels outside geometries)
    #[arg(long)]
    invert: bool,

    /// Output data type
    #[arg(long, default_value = "uint8")]
    data_type: DataTypeArg,

    /// Overwrite existing output file
    #[arg(long)]
    overwrite: bool,
}

#[derive(Debug, Clone, Copy)]
enum DataTypeArg {
    UInt8,
    UInt16,
    UInt32,
    Int16,
    Int32,
    Float32,
    Float64,
}

impl std::str::FromStr for DataTypeArg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "uint8" | "byte" => Ok(DataTypeArg::UInt8),
            "uint16" => Ok(DataTypeArg::UInt16),
            "uint32" => Ok(DataTypeArg::UInt32),
            "int16" => Ok(DataTypeArg::Int16),
            "int32" => Ok(DataTypeArg::Int32),
            "float32" => Ok(DataTypeArg::Float32),
            "float64" => Ok(DataTypeArg::Float64),
            _ => Err(format!("Invalid data type: {}", s)),
        }
    }
}

impl From<DataTypeArg> for RasterDataType {
    fn from(arg: DataTypeArg) -> Self {
        match arg {
            DataTypeArg::UInt8 => RasterDataType::UInt8,
            DataTypeArg::UInt16 => RasterDataType::UInt16,
            DataTypeArg::UInt32 => RasterDataType::UInt32,
            DataTypeArg::Int16 => RasterDataType::Int16,
            DataTypeArg::Int32 => RasterDataType::Int32,
            DataTypeArg::Float32 => RasterDataType::Float32,
            DataTypeArg::Float64 => RasterDataType::Float64,
        }
    }
}

#[derive(Serialize)]
struct RasterizeResult {
    input_file: String,
    output_file: String,
    width: usize,
    height: usize,
    features_burned: usize,
    processing_time_ms: u128,
}

pub fn execute(args: RasterizeArgs, format: OutputFormat) -> Result<()> {
    let start = std::time::Instant::now();

    // Validate inputs
    if !args.input.exists() {
        anyhow::bail!("Input file not found: {}", args.input.display());
    }

    if args.output.exists() && !args.overwrite {
        anyhow::bail!(
            "Output file already exists: {}. Use --overwrite to replace.",
            args.output.display()
        );
    }

    if args.burn.is_none() && args.attribute.is_none() {
        anyhow::bail!("Must specify either --burn or --attribute");
    }

    if args.target_size.is_none() && args.target_resolution.is_none() {
        anyhow::bail!("Must specify either --ts (target size) or --tr (target resolution)");
    }

    // Read vector data
    let pb = progress::create_spinner("Reading vector data");
    let file = File::open(&args.input)
        .with_context(|| format!("Failed to open file: {}", args.input.display()))?;
    let buf_reader = BufReader::new(file);
    let mut reader = GeoJsonReader::new(buf_reader);

    let feature_collection = reader
        .read_feature_collection()
        .context("Failed to read vector data")?;

    let feature_count = feature_collection.features.len();
    pb.finish_with_message(format!("Read {} features", feature_count));

    // Determine output extent
    let extent = if let Some(ref te) = args.target_extent {
        if te.len() != 4 {
            anyhow::bail!("Target extent must have 4 values: minx miny maxx maxy");
        }
        (te[0], te[1], te[2], te[3])
    } else {
        // Calculate from feature collection bounds
        let bbox = feature_collection
            .bbox
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Vector file has no bounds and --te not specified"))?;

        if bbox.len() < 4 {
            anyhow::bail!("Invalid bounding box in vector file");
        }

        (bbox[0], bbox[1], bbox[2], bbox[3])
    };

    // Determine output dimensions
    let (width, height) = if let Some(ref ts) = args.target_size {
        if ts.len() != 2 {
            anyhow::bail!("Target size must have 2 values: width height");
        }
        (ts[0], ts[1])
    } else if let Some(ref tr) = args.target_resolution {
        if tr.len() != 2 {
            anyhow::bail!("Target resolution must have 2 values: xres yres");
        }

        let width = ((extent.2 - extent.0) / tr[0]).ceil() as usize;
        let height = ((extent.3 - extent.1) / tr[1]).ceil() as usize;
        (width, height)
    } else {
        unreachable!();
    };

    // Create geotransform
    let pixel_width = (extent.2 - extent.0) / width as f64;
    let pixel_height = -(extent.3 - extent.1) / height as f64; // Negative for north-up

    let geo_transform = GeoTransform {
        origin_x: extent.0,
        pixel_width,
        row_rotation: 0.0,
        origin_y: extent.3,
        col_rotation: 0.0,
        pixel_height,
    };

    // Initialize raster
    let mut raster_data: Vec<f64> = vec![args.init; width * height];

    // Rasterize features
    let pb = progress::create_progress_bar(feature_count as u64, "Rasterizing features");

    let mut features_burned = 0;

    for feature in &feature_collection.features {
        if let Some(ref geom) = feature.geometry {
            // Determine burn value
            let burn_value = if let Some(burn) = args.burn {
                burn
            } else if let Some(ref attr_name) = args.attribute {
                // Get attribute value from properties map
                if let Some(ref props) = feature.properties {
                    if let Some(attr_value) = props.get(attr_name) {
                        // Try to parse as f64
                        match attr_value {
                            serde_json::Value::Number(n) => n.as_f64().ok_or_else(|| {
                                anyhow::anyhow!("Could not convert attribute to number")
                            })?,
                            _ => {
                                anyhow::bail!("Attribute '{}' is not a number", attr_name);
                            }
                        }
                    } else {
                        pb.inc(1);
                        continue; // Skip features without the attribute
                    }
                } else {
                    pb.inc(1);
                    continue; // Skip features with no properties
                }
            } else {
                unreachable!();
            };

            // Convert and rasterize geometry
            let core_geom = convert_geojson_geometry(geom)?;
            let mut params = RasterizeParams {
                raster_data: &mut raster_data,
                width,
                height,
                geo_transform: &geo_transform,
                burn_value,
                all_touched: args.all_touched,
                add: args.add,
            };
            rasterize_geometry(&core_geom, &mut params)?;

            features_burned += 1;
        }

        pb.inc(1);
    }

    pb.finish_with_message(format!("Rasterized {} features", features_burned));

    // Apply invert if requested
    if args.invert {
        let pb = progress::create_spinner("Inverting raster");
        for pixel in &mut raster_data {
            if *pixel == args.init {
                *pixel = args
                    .burn
                    .ok_or_else(|| anyhow::anyhow!("--invert requires --burn to be specified"))?;
            } else {
                *pixel = args.init;
            }
        }
        pb.finish_and_clear();
    }

    // Convert f64 raster data to bytes for the target data type
    let data_type: RasterDataType = args.data_type.into();
    let pixel_count = width * height;
    let mut byte_data: Vec<u8> = Vec::with_capacity(pixel_count * data_type.size_bytes());

    for &value in &raster_data {
        match data_type {
            RasterDataType::UInt8 => byte_data.push(value as u8),
            RasterDataType::Int8 => byte_data.push((value as i8) as u8),
            RasterDataType::UInt16 => {
                byte_data.extend_from_slice(&(value as u16).to_ne_bytes());
            }
            RasterDataType::Int16 => {
                byte_data.extend_from_slice(&(value as i16).to_ne_bytes());
            }
            RasterDataType::UInt32 => {
                byte_data.extend_from_slice(&(value as u32).to_ne_bytes());
            }
            RasterDataType::Int32 => {
                byte_data.extend_from_slice(&(value as i32).to_ne_bytes());
            }
            RasterDataType::Float32 => {
                byte_data.extend_from_slice(&(value as f32).to_ne_bytes());
            }
            RasterDataType::Float64 => {
                byte_data.extend_from_slice(&value.to_ne_bytes());
            }
            _ => {
                anyhow::bail!("Unsupported data type for rasterization: {:?}", data_type);
            }
        }
    }

    // Create RasterBuffer
    let nodata_val = if let Some(nd) = args.no_data {
        match data_type {
            RasterDataType::UInt8
            | RasterDataType::UInt16
            | RasterDataType::UInt32
            | RasterDataType::Int8
            | RasterDataType::Int16
            | RasterDataType::Int32 => oxigdal_core::types::NoDataValue::Integer(nd as i64),
            _ => oxigdal_core::types::NoDataValue::Float(nd),
        }
    } else {
        oxigdal_core::types::NoDataValue::None
    };

    let raster_buffer = RasterBuffer::new(
        byte_data,
        width as u64,
        height as u64,
        data_type,
        nodata_val,
    )
    .context("Failed to create raster buffer")?;

    // Extract CRS from vector file
    let epsg_code = feature_collection
        .crs
        .as_ref()
        .and_then(|crs| crs.epsg_code());

    // Write output
    let pb = progress::create_spinner("Writing output");
    crate::util::raster::write_single_band(
        &args.output,
        &raster_buffer,
        Some(geo_transform),
        epsg_code,
        args.no_data,
    )
    .context("Failed to write output raster")?;
    pb.finish_with_message("Output written successfully");

    // Output results
    let result = RasterizeResult {
        input_file: args.input.display().to_string(),
        output_file: args.output.display().to_string(),
        width,
        height,
        features_burned,
        processing_time_ms: start.elapsed().as_millis(),
    };

    match format {
        OutputFormat::Json => {
            let json =
                serde_json::to_string_pretty(&result).context("Failed to serialize to JSON")?;
            println!("{}", json);
        }
        OutputFormat::Text => {
            println!("{}", style("Rasterization complete").green().bold());
            println!("  Input:    {}", result.input_file);
            println!("  Output:   {}", result.output_file);
            println!("  Size:     {} x {}", result.width, result.height);
            println!("  Features: {}", result.features_burned);
            println!("  Time:     {} ms", result.processing_time_ms);
        }
    }

    Ok(())
}

/// Convert GeoJSON geometry to OxiGDAL core geometry
fn convert_geojson_geometry(geom: &oxigdal_geojson::Geometry) -> Result<Geometry> {
    match geom {
        oxigdal_geojson::Geometry::Point(point) => {
            let coord = convert_position(&point.coordinates)?;
            Ok(Geometry::Point(Point::from_coord(coord)))
        }
        oxigdal_geojson::Geometry::LineString(line) => {
            let coords: Result<Vec<_>> = line
                .coordinates
                .iter()
                .map(|pos| convert_position(pos.as_slice()))
                .collect();
            let line_string = LineString::new(coords?)
                .map_err(|e| anyhow::anyhow!("Failed to create LineString: {}", e))?;
            Ok(Geometry::LineString(line_string))
        }
        oxigdal_geojson::Geometry::Polygon(poly) => {
            if poly.coordinates.is_empty() {
                anyhow::bail!("Polygon has no coordinates");
            }

            let exterior_coords: Result<Vec<_>> = poly.coordinates[0]
                .iter()
                .map(|pos| convert_position(pos.as_slice()))
                .collect();
            let exterior = LineString::new(exterior_coords?)
                .map_err(|e| anyhow::anyhow!("Failed to create exterior ring: {}", e))?;

            let mut interiors = Vec::new();
            for interior_ring in &poly.coordinates[1..] {
                let interior_coords: Result<Vec<_>> = interior_ring
                    .iter()
                    .map(|pos| convert_position(pos.as_slice()))
                    .collect();
                let interior = LineString::new(interior_coords?)
                    .map_err(|e| anyhow::anyhow!("Failed to create interior ring: {}", e))?;
                interiors.push(interior);
            }

            let polygon = Polygon::new(exterior, interiors)
                .map_err(|e| anyhow::anyhow!("Failed to create Polygon: {}", e))?;
            Ok(Geometry::Polygon(polygon))
        }
        oxigdal_geojson::Geometry::MultiPoint(mp) => {
            let points: Result<Vec<_>> = mp
                .coordinates
                .iter()
                .map(|pos| {
                    let coord = convert_position(pos.as_slice())?;
                    Ok(Point::from_coord(coord))
                })
                .collect();
            Ok(Geometry::MultiPoint(MultiPoint::new(points?)))
        }
        oxigdal_geojson::Geometry::MultiLineString(mls) => {
            let line_strings: Result<Vec<_>> = mls
                .coordinates
                .iter()
                .map(|line_coords| {
                    let coords: Result<Vec<_>> = line_coords
                        .iter()
                        .map(|pos| convert_position(pos.as_slice()))
                        .collect();
                    LineString::new(coords?)
                        .map_err(|e| anyhow::anyhow!("Failed to create LineString: {}", e))
                })
                .collect();
            Ok(Geometry::MultiLineString(MultiLineString::new(
                line_strings?,
            )))
        }
        oxigdal_geojson::Geometry::MultiPolygon(mp) => {
            let polygons: Result<Vec<_>> = mp
                .coordinates
                .iter()
                .map(|poly_coords| {
                    if poly_coords.is_empty() {
                        anyhow::bail!("Polygon in MultiPolygon has no coordinates");
                    }

                    let exterior_coords: Result<Vec<_>> = poly_coords[0]
                        .iter()
                        .map(|pos| convert_position(pos.as_slice()))
                        .collect();
                    let exterior = LineString::new(exterior_coords?)
                        .map_err(|e| anyhow::anyhow!("Failed to create exterior ring: {}", e))?;

                    let mut interiors = Vec::new();
                    for interior_ring in &poly_coords[1..] {
                        let interior_coords: Result<Vec<_>> = interior_ring
                            .iter()
                            .map(|pos| convert_position(pos.as_slice()))
                            .collect();
                        let interior = LineString::new(interior_coords?).map_err(|e| {
                            anyhow::anyhow!("Failed to create interior ring: {}", e)
                        })?;
                        interiors.push(interior);
                    }

                    Polygon::new(exterior, interiors)
                        .map_err(|e| anyhow::anyhow!("Failed to create Polygon: {}", e))
                })
                .collect();
            Ok(Geometry::MultiPolygon(MultiPolygon::new(polygons?)))
        }
        oxigdal_geojson::Geometry::GeometryCollection(gc) => {
            let geometries: Result<Vec<_>> =
                gc.geometries.iter().map(convert_geojson_geometry).collect();
            Ok(Geometry::GeometryCollection(
                oxigdal_core::vector::geometry::GeometryCollection::new(geometries?),
            ))
        }
    }
}

/// Convert GeoJSON position to OxiGDAL coordinate
fn convert_position(pos: &[f64]) -> Result<Coordinate> {
    if pos.len() < 2 {
        anyhow::bail!("Position must have at least 2 coordinates");
    }

    let coord = if pos.len() >= 3 {
        Coordinate::new_3d(pos[0], pos[1], pos[2])
    } else {
        Coordinate::new_2d(pos[0], pos[1])
    };

    Ok(coord)
}

/// Parameters for rasterization operations
struct RasterizeParams<'a> {
    raster_data: &'a mut [f64],
    width: usize,
    height: usize,
    geo_transform: &'a GeoTransform,
    burn_value: f64,
    all_touched: bool,
    add: bool,
}

/// Rasterize a single geometry into the raster data
fn rasterize_geometry(geometry: &Geometry, params: &mut RasterizeParams) -> Result<()> {
    match geometry {
        Geometry::Point(point) => {
            let (px, py) = geo_to_pixel(
                point.coord.x,
                point.coord.y,
                params.geo_transform,
                params.width,
                params.height,
            )?;
            if px < params.width && py < params.height {
                let idx = py * params.width + px;
                if params.add {
                    params.raster_data[idx] += params.burn_value;
                } else {
                    params.raster_data[idx] = params.burn_value;
                }
            }
        }
        Geometry::LineString(line) => {
            rasterize_line_string(
                &line.coords,
                params.raster_data,
                params.width,
                params.height,
                params.geo_transform,
                params.burn_value,
                params.add,
            )?;
        }
        Geometry::Polygon(poly) => {
            let mut poly_params = PolygonRasterParams {
                raster_data: params.raster_data,
                width: params.width,
                height: params.height,
                geo_transform: params.geo_transform,
                burn_value: params.burn_value,
                _all_touched: params.all_touched,
                add: params.add,
            };
            rasterize_polygon(&poly.exterior.coords, &poly.interiors, &mut poly_params)?;
        }
        Geometry::MultiPoint(mp) => {
            for point in &mp.points {
                let (px, py) = geo_to_pixel(
                    point.coord.x,
                    point.coord.y,
                    params.geo_transform,
                    params.width,
                    params.height,
                )?;
                if px < params.width && py < params.height {
                    let idx = py * params.width + px;
                    if params.add {
                        params.raster_data[idx] += params.burn_value;
                    } else {
                        params.raster_data[idx] = params.burn_value;
                    }
                }
            }
        }
        Geometry::MultiLineString(mls) => {
            for line in &mls.line_strings {
                rasterize_line_string(
                    &line.coords,
                    params.raster_data,
                    params.width,
                    params.height,
                    params.geo_transform,
                    params.burn_value,
                    params.add,
                )?;
            }
        }
        Geometry::MultiPolygon(mp) => {
            for poly in &mp.polygons {
                let mut poly_params = PolygonRasterParams {
                    raster_data: params.raster_data,
                    width: params.width,
                    height: params.height,
                    geo_transform: params.geo_transform,
                    burn_value: params.burn_value,
                    _all_touched: params.all_touched,
                    add: params.add,
                };
                rasterize_polygon(&poly.exterior.coords, &poly.interiors, &mut poly_params)?;
            }
        }
        Geometry::GeometryCollection(gc) => {
            for geom in &gc.geometries {
                rasterize_geometry(geom, params)?;
            }
        }
    }

    Ok(())
}

/// Convert geographic coordinates to pixel coordinates
fn geo_to_pixel(
    x: f64,
    y: f64,
    gt: &GeoTransform,
    width: usize,
    height: usize,
) -> Result<(usize, usize)> {
    let px = ((x - gt.origin_x) / gt.pixel_width) as isize;
    let py = ((y - gt.origin_y) / gt.pixel_height) as isize;

    if px >= 0 && py >= 0 && (px as usize) < width && (py as usize) < height {
        Ok((px as usize, py as usize))
    } else {
        Ok((usize::MAX, usize::MAX)) // Out of bounds
    }
}

/// Rasterize a line string using Bresenham's algorithm
fn rasterize_line_string(
    coords: &[oxigdal_core::vector::geometry::Coordinate],
    raster_data: &mut [f64],
    width: usize,
    height: usize,
    geo_transform: &GeoTransform,
    burn_value: f64,
    add: bool,
) -> Result<()> {
    for window in coords.windows(2) {
        let (x0, y0) = geo_to_pixel(window[0].x, window[0].y, geo_transform, width, height)?;
        let (x1, y1) = geo_to_pixel(window[1].x, window[1].y, geo_transform, width, height)?;

        if x0 == usize::MAX || x1 == usize::MAX {
            continue; // Skip out of bounds
        }

        // Bresenham's line algorithm
        let mut params = LineRasterParams {
            raster_data,
            width,
            height,
            burn_value,
            add,
        };
        bresenham_line(x0, y0, x1, y1, &mut params);
    }

    Ok(())
}

/// Parameters for line rasterization
struct LineRasterParams<'a> {
    raster_data: &'a mut [f64],
    width: usize,
    height: usize,
    burn_value: f64,
    add: bool,
}

/// Bresenham's line drawing algorithm
fn bresenham_line(x0: usize, y0: usize, x1: usize, y1: usize, params: &mut LineRasterParams) {
    let dx = (x1 as isize - x0 as isize).abs();
    let dy = -(y1 as isize - y0 as isize).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    let mut x = x0 as isize;
    let mut y = y0 as isize;

    loop {
        if x >= 0 && y >= 0 && (x as usize) < params.width && (y as usize) < params.height {
            let idx = (y as usize) * params.width + (x as usize);
            if params.add {
                params.raster_data[idx] += params.burn_value;
            } else {
                params.raster_data[idx] = params.burn_value;
            }
        }

        if x == x1 as isize && y == y1 as isize {
            break;
        }

        let e2 = 2 * err;

        if e2 >= dy {
            err += dy;
            x += sx;
        }

        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Parameters for polygon rasterization
struct PolygonRasterParams<'a> {
    raster_data: &'a mut [f64],
    width: usize,
    height: usize,
    geo_transform: &'a GeoTransform,
    burn_value: f64,
    _all_touched: bool,
    add: bool,
}

/// Edge structure for scanline algorithm
#[derive(Debug, Clone)]
struct Edge {
    /// Y coordinate of the lower vertex (in pixel space)
    y_min: f64,
    /// Y coordinate of the upper vertex (in pixel space)
    y_max: f64,
    /// X coordinate at y_min
    x_at_y_min: f64,
    /// Inverse slope (dx/dy)
    inv_slope: f64,
}

impl Edge {
    /// Creates a new edge from two coordinates in pixel space
    fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Option<Self> {
        // Skip horizontal edges
        if (y1 - y0).abs() < f64::EPSILON {
            return None;
        }

        let (y_min, y_max, x_at_y_min) = if y0 < y1 { (y0, y1, x0) } else { (y1, y0, x1) };

        let inv_slope = (x1 - x0) / (y1 - y0);

        Some(Self {
            y_min,
            y_max,
            x_at_y_min,
            inv_slope,
        })
    }

    /// Get x coordinate at a given y
    fn x_at(&self, y: f64) -> f64 {
        self.x_at_y_min + (y - self.y_min) * self.inv_slope
    }
}

/// Collect edges from a ring of coordinates
fn collect_edges_from_ring(
    coords: &[oxigdal_core::vector::geometry::Coordinate],
    geo_transform: &GeoTransform,
    _width: usize,
    height: usize,
) -> Vec<Edge> {
    let mut edges = Vec::new();

    if coords.len() < 3 {
        return edges;
    }

    for i in 0..coords.len() {
        let j = (i + 1) % coords.len();

        // Convert to pixel coordinates
        let (px0, py0) = geo_to_pixel_f64(coords[i].x, coords[i].y, geo_transform);
        let (px1, py1) = geo_to_pixel_f64(coords[j].x, coords[j].y, geo_transform);

        // Clip edges to raster bounds (with some margin)
        let y_min = py0.min(py1);
        let y_max = py0.max(py1);

        // Skip if completely outside vertical bounds
        if y_max < 0.0 || y_min >= height as f64 {
            continue;
        }

        if let Some(edge) = Edge::new(px0, py0, px1, py1) {
            edges.push(edge);
        }
    }

    edges
}

/// Convert geographic coordinates to pixel coordinates (floating point)
fn geo_to_pixel_f64(x: f64, y: f64, gt: &GeoTransform) -> (f64, f64) {
    let px = (x - gt.origin_x) / gt.pixel_width;
    let py = (y - gt.origin_y) / gt.pixel_height;
    (px, py)
}

/// Rasterize a polygon using scanline algorithm with even-odd fill rule
///
/// This implementation uses the scanline algorithm with an Active Edge Table (AET)
/// to efficiently rasterize polygons. The even-odd fill rule is used to correctly
/// handle holes in polygons.
fn rasterize_polygon(
    exterior: &[oxigdal_core::vector::geometry::Coordinate],
    interiors: &[LineString],
    params: &mut PolygonRasterParams,
) -> Result<()> {
    // Collect all edges from exterior and interior rings
    let mut all_edges: Vec<Edge> = Vec::new();

    // Add edges from exterior ring
    all_edges.extend(collect_edges_from_ring(
        exterior,
        params.geo_transform,
        params.width,
        params.height,
    ));

    // Add edges from interior rings (holes)
    // The even-odd rule will automatically handle the holes
    for interior in interiors {
        all_edges.extend(collect_edges_from_ring(
            &interior.coords,
            params.geo_transform,
            params.width,
            params.height,
        ));
    }

    if all_edges.is_empty() {
        return Ok(());
    }

    // Find the scanline range
    let y_min = all_edges
        .iter()
        .map(|e| e.y_min.floor() as isize)
        .min()
        .unwrap_or(0);
    let y_max = all_edges
        .iter()
        .map(|e| e.y_max.ceil() as isize)
        .max()
        .unwrap_or(0);

    let scanline_start = y_min.max(0) as usize;
    let scanline_end = (y_max as usize).min(params.height);

    // Process each scanline
    for scanline in scanline_start..scanline_end {
        let y = scanline as f64 + 0.5; // Sample at pixel center

        // Find all edges that intersect this scanline
        let mut intersections: Vec<f64> = all_edges
            .iter()
            .filter(|edge| edge.y_min <= y && edge.y_max > y)
            .map(|edge| edge.x_at(y))
            .collect();

        // Sort intersections by x coordinate
        intersections.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // Apply even-odd fill rule: fill between pairs of intersections
        for chunk in intersections.chunks_exact(2) {
            let x_start = chunk[0].floor() as isize;
            let x_end = chunk[1].ceil() as isize;

            // Fill pixels between this pair of intersections
            let pixel_start = x_start.max(0) as usize;
            let pixel_end = (x_end as usize).min(params.width);

            for x in pixel_start..pixel_end {
                let idx = scanline * params.width + x;
                if idx < params.raster_data.len() {
                    if params.add {
                        params.raster_data[idx] += params.burn_value;
                    } else {
                        params.raster_data[idx] = params.burn_value;
                    }
                }
            }
        }

        // Handle all-touched mode: also burn edge pixels
        if params._all_touched {
            for edge in &all_edges {
                if edge.y_min <= y && edge.y_max > y {
                    let x = edge.x_at(y);
                    let x_floor = x.floor() as isize;
                    let x_ceil = x.ceil() as isize;

                    for px in [x_floor, x_ceil] {
                        if px >= 0 && (px as usize) < params.width {
                            let idx = scanline * params.width + px as usize;
                            if idx < params.raster_data.len() {
                                if params.add {
                                    params.raster_data[idx] += params.burn_value;
                                } else {
                                    params.raster_data[idx] = params.burn_value;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_type_parsing() {
        use std::str::FromStr;

        assert!(matches!(
            DataTypeArg::from_str("uint8"),
            Ok(DataTypeArg::UInt8)
        ));
        assert!(matches!(
            DataTypeArg::from_str("byte"),
            Ok(DataTypeArg::UInt8)
        ));
        assert!(matches!(
            DataTypeArg::from_str("float32"),
            Ok(DataTypeArg::Float32)
        ));
        assert!(DataTypeArg::from_str("invalid").is_err());
    }

    #[test]
    fn test_edge_creation() {
        // Vertical edge
        let edge = Edge::new(5.0, 0.0, 5.0, 10.0);
        assert!(edge.is_some());
        let e = edge.expect("should create edge");
        assert_eq!(e.y_min, 0.0);
        assert_eq!(e.y_max, 10.0);
        assert_eq!(e.x_at_y_min, 5.0);
        assert_eq!(e.inv_slope, 0.0);

        // Diagonal edge
        let edge2 = Edge::new(0.0, 0.0, 10.0, 10.0);
        assert!(edge2.is_some());
        let e2 = edge2.expect("should create edge");
        assert_eq!(e2.x_at(5.0), 5.0);

        // Horizontal edge should be None
        let horizontal = Edge::new(0.0, 5.0, 10.0, 5.0);
        assert!(horizontal.is_none());
    }

    #[test]
    fn test_edge_x_interpolation() {
        // 45-degree line from (0,0) to (10,10)
        let edge = Edge::new(0.0, 0.0, 10.0, 10.0).expect("valid edge");

        assert_eq!(edge.x_at(0.0), 0.0);
        assert_eq!(edge.x_at(5.0), 5.0);
        assert_eq!(edge.x_at(10.0), 10.0);

        // Steeper slope: from (0,0) to (5,10)
        let steep = Edge::new(0.0, 0.0, 5.0, 10.0).expect("valid edge");
        assert_eq!(steep.x_at(0.0), 0.0);
        assert_eq!(steep.x_at(10.0), 5.0);
        assert_eq!(steep.x_at(5.0), 2.5);
    }

    #[test]
    fn test_geo_to_pixel_f64() {
        let gt = GeoTransform {
            origin_x: 0.0,
            origin_y: 100.0,
            pixel_width: 1.0,
            pixel_height: -1.0,
            row_rotation: 0.0,
            col_rotation: 0.0,
        };

        let (px, py) = geo_to_pixel_f64(50.0, 50.0, &gt);
        assert_eq!(px, 50.0);
        assert_eq!(py, 50.0);

        let (px2, py2) = geo_to_pixel_f64(0.0, 100.0, &gt);
        assert_eq!(px2, 0.0);
        assert_eq!(py2, 0.0);
    }

    #[test]
    fn test_simple_polygon_rasterization() {
        // Create a simple square polygon: (10,10) to (90,90) in a 100x100 raster
        let exterior = vec![
            Coordinate::new_2d(10.0, 90.0),
            Coordinate::new_2d(90.0, 90.0),
            Coordinate::new_2d(90.0, 10.0),
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(10.0, 90.0), // Close the ring
        ];

        let mut raster_data = vec![0.0; 100 * 100];
        let geo_transform = GeoTransform {
            origin_x: 0.0,
            origin_y: 100.0,
            pixel_width: 1.0,
            pixel_height: -1.0,
            row_rotation: 0.0,
            col_rotation: 0.0,
        };

        let mut params = PolygonRasterParams {
            raster_data: &mut raster_data,
            width: 100,
            height: 100,
            geo_transform: &geo_transform,
            burn_value: 1.0,
            _all_touched: false,
            add: false,
        };

        let result = rasterize_polygon(&exterior, &[], &mut params);
        assert!(result.is_ok());

        // Check that pixels inside the polygon are burned
        let center_idx = 50 * 100 + 50;
        assert_eq!(raster_data[center_idx], 1.0);

        // Check that pixels outside the polygon are not burned
        let outside_idx = 5 * 100 + 5;
        assert_eq!(raster_data[outside_idx], 0.0);
    }

    #[test]
    fn test_polygon_with_hole_rasterization() {
        // Create a square polygon with a hole
        // Outer square: (10,10) to (90,90)
        let exterior = vec![
            Coordinate::new_2d(10.0, 90.0),
            Coordinate::new_2d(90.0, 90.0),
            Coordinate::new_2d(90.0, 10.0),
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(10.0, 90.0),
        ];

        // Inner hole: (40,40) to (60,60)
        let hole_coords = vec![
            Coordinate::new_2d(40.0, 60.0),
            Coordinate::new_2d(60.0, 60.0),
            Coordinate::new_2d(60.0, 40.0),
            Coordinate::new_2d(40.0, 40.0),
            Coordinate::new_2d(40.0, 60.0),
        ];
        let hole = LineString::new(hole_coords).expect("valid hole ring");

        let mut raster_data = vec![0.0; 100 * 100];
        let geo_transform = GeoTransform {
            origin_x: 0.0,
            origin_y: 100.0,
            pixel_width: 1.0,
            pixel_height: -1.0,
            row_rotation: 0.0,
            col_rotation: 0.0,
        };

        let mut params = PolygonRasterParams {
            raster_data: &mut raster_data,
            width: 100,
            height: 100,
            geo_transform: &geo_transform,
            burn_value: 1.0,
            _all_touched: false,
            add: false,
        };

        let result = rasterize_polygon(&exterior, &[hole], &mut params);
        assert!(result.is_ok());

        // Check that pixels inside the polygon (but outside the hole) are burned
        let inside_idx = 20 * 100 + 20;
        assert_eq!(raster_data[inside_idx], 1.0);

        // Check that pixels inside the hole are NOT burned (even-odd rule)
        let hole_center_idx = 50 * 100 + 50;
        assert_eq!(raster_data[hole_center_idx], 0.0);

        // Check that pixels outside the polygon are not burned
        let outside_idx = 5 * 100 + 5;
        assert_eq!(raster_data[outside_idx], 0.0);
    }

    #[test]
    fn test_polygon_add_mode() {
        // Test that add mode accumulates values
        let exterior = vec![
            Coordinate::new_2d(10.0, 90.0),
            Coordinate::new_2d(90.0, 90.0),
            Coordinate::new_2d(90.0, 10.0),
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(10.0, 90.0),
        ];

        let mut raster_data = vec![5.0; 100 * 100]; // Pre-fill with 5.0
        let geo_transform = GeoTransform {
            origin_x: 0.0,
            origin_y: 100.0,
            pixel_width: 1.0,
            pixel_height: -1.0,
            row_rotation: 0.0,
            col_rotation: 0.0,
        };

        let mut params = PolygonRasterParams {
            raster_data: &mut raster_data,
            width: 100,
            height: 100,
            geo_transform: &geo_transform,
            burn_value: 3.0,
            _all_touched: false,
            add: true, // Add mode
        };

        let result = rasterize_polygon(&exterior, &[], &mut params);
        assert!(result.is_ok());

        // Check that pixels inside have accumulated value (5 + 3 = 8)
        let center_idx = 50 * 100 + 50;
        assert_eq!(raster_data[center_idx], 8.0);

        // Check that pixels outside still have original value
        let outside_idx = 5 * 100 + 5;
        assert_eq!(raster_data[outside_idx], 5.0);
    }

    #[test]
    fn test_collect_edges_from_ring() {
        let coords = vec![
            Coordinate::new_2d(0.0, 100.0),
            Coordinate::new_2d(100.0, 100.0),
            Coordinate::new_2d(100.0, 0.0),
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(0.0, 100.0),
        ];

        let geo_transform = GeoTransform {
            origin_x: 0.0,
            origin_y: 100.0,
            pixel_width: 1.0,
            pixel_height: -1.0,
            row_rotation: 0.0,
            col_rotation: 0.0,
        };

        let edges = collect_edges_from_ring(&coords, &geo_transform, 100, 100);

        // Should have 4 edges (square has 4 sides, but 2 are horizontal and skipped)
        // Actually, with pixel_height = -1.0, we get vertical edges on left and right
        assert!(!edges.is_empty());
    }

    #[test]
    fn test_triangle_polygon_rasterization() {
        // Create a triangle to test non-rectangular shapes
        let exterior = vec![
            Coordinate::new_2d(50.0, 90.0), // Top vertex
            Coordinate::new_2d(90.0, 10.0), // Bottom right
            Coordinate::new_2d(10.0, 10.0), // Bottom left
            Coordinate::new_2d(50.0, 90.0), // Close the ring
        ];

        let mut raster_data = vec![0.0; 100 * 100];
        let geo_transform = GeoTransform {
            origin_x: 0.0,
            origin_y: 100.0,
            pixel_width: 1.0,
            pixel_height: -1.0,
            row_rotation: 0.0,
            col_rotation: 0.0,
        };

        let mut params = PolygonRasterParams {
            raster_data: &mut raster_data,
            width: 100,
            height: 100,
            geo_transform: &geo_transform,
            burn_value: 1.0,
            _all_touched: false,
            add: false,
        };

        let result = rasterize_polygon(&exterior, &[], &mut params);
        assert!(result.is_ok());

        // Check that centroid area is burned
        let center_idx = 50 * 100 + 50;
        assert_eq!(raster_data[center_idx], 1.0);

        // Check that top-left corner (outside triangle) is not burned
        let outside_idx = 15 * 100 + 15;
        assert_eq!(raster_data[outside_idx], 0.0);
    }

    #[test]
    fn test_empty_polygon() {
        // Test that empty polygon doesn't cause issues
        let exterior: Vec<Coordinate> = vec![];

        let mut raster_data = vec![0.0; 100 * 100];
        let geo_transform = GeoTransform {
            origin_x: 0.0,
            origin_y: 100.0,
            pixel_width: 1.0,
            pixel_height: -1.0,
            row_rotation: 0.0,
            col_rotation: 0.0,
        };

        let mut params = PolygonRasterParams {
            raster_data: &mut raster_data,
            width: 100,
            height: 100,
            geo_transform: &geo_transform,
            burn_value: 1.0,
            _all_touched: false,
            add: false,
        };

        let result = rasterize_polygon(&exterior, &[], &mut params);
        assert!(result.is_ok());

        // All pixels should remain 0
        assert!(raster_data.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn test_polygon_outside_raster_bounds() {
        // Create a polygon completely outside the raster bounds
        let exterior = vec![
            Coordinate::new_2d(200.0, 300.0),
            Coordinate::new_2d(300.0, 300.0),
            Coordinate::new_2d(300.0, 200.0),
            Coordinate::new_2d(200.0, 200.0),
            Coordinate::new_2d(200.0, 300.0),
        ];

        let mut raster_data = vec![0.0; 100 * 100];
        let geo_transform = GeoTransform {
            origin_x: 0.0,
            origin_y: 100.0,
            pixel_width: 1.0,
            pixel_height: -1.0,
            row_rotation: 0.0,
            col_rotation: 0.0,
        };

        let mut params = PolygonRasterParams {
            raster_data: &mut raster_data,
            width: 100,
            height: 100,
            geo_transform: &geo_transform,
            burn_value: 1.0,
            _all_touched: false,
            add: false,
        };

        let result = rasterize_polygon(&exterior, &[], &mut params);
        assert!(result.is_ok());

        // All pixels should remain 0
        assert!(raster_data.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn test_all_touched_mode() {
        // Create a small polygon that tests all-touched mode
        let exterior = vec![
            Coordinate::new_2d(45.5, 55.5),
            Coordinate::new_2d(55.5, 55.5),
            Coordinate::new_2d(55.5, 45.5),
            Coordinate::new_2d(45.5, 45.5),
            Coordinate::new_2d(45.5, 55.5),
        ];

        let mut raster_data = vec![0.0; 100 * 100];
        let geo_transform = GeoTransform {
            origin_x: 0.0,
            origin_y: 100.0,
            pixel_width: 1.0,
            pixel_height: -1.0,
            row_rotation: 0.0,
            col_rotation: 0.0,
        };

        let mut params = PolygonRasterParams {
            raster_data: &mut raster_data,
            width: 100,
            height: 100,
            geo_transform: &geo_transform,
            burn_value: 1.0,
            _all_touched: true, // All-touched mode
            add: false,
        };

        let result = rasterize_polygon(&exterior, &[], &mut params);
        assert!(result.is_ok());

        // Check that center is burned
        let center_idx = 50 * 100 + 50;
        assert_eq!(raster_data[center_idx], 1.0);

        // In all-touched mode, edge pixels should also be burned
        let edge_idx = 45 * 100 + 45;
        assert_eq!(raster_data[edge_idx], 1.0);
    }
}
