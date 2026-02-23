//! Create test FlatGeobuf files for cloud-native vector data
//!
//! FlatGeobuf is a performant binary encoding for geographic data with spatial indexing.
//!
//! Usage:
//!     cargo run --example create_test_flatgeobuf_samples
//!
//! Output:
//!     demo/cog-viewer/golden-triangle.fgb
//!     demo/cog-viewer/iron-belt.fgb

use oxigdal_core::vector::{
    Coordinate, Feature, Geometry, LineString, Point, Polygon, PropertyValue,
};
use oxigdal_flatgeobuf::header::{Column, ColumnType, CrsInfo, GeometryType};
use oxigdal_flatgeobuf::writer::FlatGeobufWriterBuilder;
use std::fs::{File, create_dir_all};
use std::io::BufWriter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating test FlatGeobuf samples with R-tree spatial indexing...\n");

    // Ensure output directory exists
    create_dir_all("demo/cog-viewer")?;

    create_golden_triangle_fgb()?;
    create_iron_belt_fgb()?;

    println!("\n✅ All FlatGeobuf samples created successfully!");
    println!("Files support HTTP Range Requests for cloud-native access!");
    println!("R-tree spatial index enables efficient spatial queries!");

    Ok(())
}

fn create_golden_triangle_fgb() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating Golden Triangle FlatGeobuf...");

    let center_lon = 100.08466749884738;
    let center_lat = 20.35223590060906;

    // Create writer with spatial index enabled
    let path = "demo/cog-viewer/golden-triangle.fgb";
    let file = File::create(path)?;
    let buf_writer = BufWriter::new(file);

    let mut writer = FlatGeobufWriterBuilder::new(GeometryType::GeometryCollection)
        .with_index() // Enable R-tree spatial index
        .with_crs(CrsInfo::from_epsg(4326))
        .with_column(Column::new("name", ColumnType::String))
        .with_column(Column::new("country", ColumnType::String))
        .with_column(Column::new("area_km2", ColumnType::Double))
        .with_column(Column::new("population", ColumnType::Long))
        .build(buf_writer)?;

    // Create polygon features for Golden Triangle region
    // Thailand region
    let thailand_coords = create_polygon_coords(
        center_lon - 2.0,
        center_lat - 1.0,
        center_lon,
        center_lat + 1.0,
    );
    let thailand_polygon = create_polygon(thailand_coords)?;
    let mut thailand_feature = Feature::new(Geometry::Polygon(thailand_polygon));
    thailand_feature.set_property(
        "name",
        PropertyValue::String("Golden Triangle - Thailand".to_string()),
    );
    thailand_feature.set_property("country", PropertyValue::String("Thailand".to_string()));
    thailand_feature.set_property("area_km2", PropertyValue::Float(15000.0));
    thailand_feature.set_property("population", PropertyValue::Integer(500000));
    writer.add_feature(&thailand_feature)?;

    // Myanmar region
    let myanmar_coords = create_polygon_coords(
        center_lon,
        center_lat - 1.0,
        center_lon + 2.0,
        center_lat + 1.0,
    );
    let myanmar_polygon = create_polygon(myanmar_coords)?;
    let mut myanmar_feature = Feature::new(Geometry::Polygon(myanmar_polygon));
    myanmar_feature.set_property(
        "name",
        PropertyValue::String("Golden Triangle - Myanmar".to_string()),
    );
    myanmar_feature.set_property("country", PropertyValue::String("Myanmar".to_string()));
    myanmar_feature.set_property("area_km2", PropertyValue::Float(12000.0));
    myanmar_feature.set_property("population", PropertyValue::Integer(300000));
    writer.add_feature(&myanmar_feature)?;

    // Laos region
    let laos_coords = create_polygon_coords(
        center_lon - 1.0,
        center_lat + 1.0,
        center_lon + 1.0,
        center_lat + 3.0,
    );
    let laos_polygon = create_polygon(laos_coords)?;
    let mut laos_feature = Feature::new(Geometry::Polygon(laos_polygon));
    laos_feature.set_property(
        "name",
        PropertyValue::String("Golden Triangle - Laos".to_string()),
    );
    laos_feature.set_property("country", PropertyValue::String("Laos".to_string()));
    laos_feature.set_property("area_km2", PropertyValue::Float(8000.0));
    laos_feature.set_property("population", PropertyValue::Integer(200000));
    writer.add_feature(&laos_feature)?;

    // Add some point features for major cities
    add_city_point(
        &mut writer,
        center_lon - 1.0,
        center_lat,
        "Chiang Rai",
        "Thailand",
        200000,
    )?;
    add_city_point(
        &mut writer,
        center_lon + 0.5,
        center_lat + 0.5,
        "Mae Sai",
        "Thailand",
        50000,
    )?;
    add_city_point(
        &mut writer,
        center_lon + 1.5,
        center_lat - 0.5,
        "Tachileik",
        "Myanmar",
        80000,
    )?;

    // Finish writing and flush
    let _writer = writer.finish()?;

    println!("  ✓ Created {}", path);
    println!("  ℹ️  Features: 6 (3 polygons, 3 points)");
    println!("  ℹ️  R-tree spatial index: Enabled");
    println!("  📍 Center: {:.6}, {:.6}", center_lat, center_lon);

    Ok(())
}

fn create_iron_belt_fgb() -> Result<(), Box<dyn std::error::Error>> {
    println!("\nCreating Basque Country FlatGeobuf...");

    let center_lon = -2.9253;
    let center_lat = 43.2630;

    let path = "demo/cog-viewer/iron-belt.fgb";
    let file = File::create(path)?;
    let buf_writer = BufWriter::new(file);

    let mut writer = FlatGeobufWriterBuilder::new(GeometryType::GeometryCollection)
        .with_index() // Enable R-tree spatial index
        .with_crs(CrsInfo::from_epsg(4326))
        .with_column(Column::new("name", ColumnType::String))
        .with_column(Column::new("country", ColumnType::String))
        .with_column(Column::new("iron_production_mt", ColumnType::Double))
        .with_column(Column::new("mining_sites", ColumnType::Int))
        .build(buf_writer)?;

    // Basque Iron Belt
    let spain_coords = create_polygon_coords(
        center_lon - 2.0,
        center_lat - 2.0,
        center_lon + 1.0,
        center_lat,
    );
    let spain_polygon = create_polygon(spain_coords)?;
    let mut spain_feature = Feature::new(Geometry::Polygon(spain_polygon));
    spain_feature.set_property(
        "name",
        PropertyValue::String("Basque Iron Belt".to_string()),
    );
    spain_feature.set_property("country", PropertyValue::String("Spain".to_string()));
    spain_feature.set_property("iron_production_mt", PropertyValue::Float(800000.0));
    spain_feature.set_property("mining_sites", PropertyValue::Integer(15));
    writer.add_feature(&spain_feature)?;

    // French Pyrenees (Ariège)
    let drc_coords = create_polygon_coords(
        center_lon - 2.0,
        center_lat,
        center_lon + 2.0,
        center_lat + 2.0,
    );
    let drc_polygon = create_polygon(drc_coords)?;
    let mut drc_feature = Feature::new(Geometry::Polygon(drc_polygon));
    drc_feature.set_property(
        "name",
        PropertyValue::String("Ariège Mining Region".to_string()),
    );
    drc_feature.set_property("country", PropertyValue::String("France".to_string()));
    drc_feature.set_property("iron_production_mt", PropertyValue::Float(1200000.0));
    drc_feature.set_property("mining_sites", PropertyValue::Integer(25));
    writer.add_feature(&drc_feature)?;

    // Add major mining cities
    add_mining_city(
        &mut writer,
        center_lon - 1.0,
        center_lat - 1.0,
        "Vitoria-Gasteiz",
        "Spain",
        500000,
        6,
    )?;
    add_mining_city(
        &mut writer,
        center_lon - 0.5,
        center_lat - 1.5,
        "Pamplona",
        "Spain",
        450000,
        4,
    )?;
    add_mining_city(
        &mut writer,
        center_lon + 0.5,
        center_lat + 1.0,
        "Bayonne",
        "France",
        2000000,
        12,
    )?;
    add_mining_city(
        &mut writer,
        center_lon - 1.5,
        center_lat + 0.5,
        "Pau",
        "France",
        500000,
        8,
    )?;

    let _writer = writer.finish()?;

    println!("  ✓ Created {}", path);
    println!("  ℹ️  Features: 6 (2 polygons, 4 points)");
    println!("  ℹ️  R-tree spatial index: Enabled");
    println!("  📍 Center: {:.6}, {:.6}", center_lat, center_lon);

    Ok(())
}

/// Creates polygon coordinates from bounding box
fn create_polygon_coords(
    min_lon: f64,
    min_lat: f64,
    max_lon: f64,
    max_lat: f64,
) -> Vec<Coordinate> {
    vec![
        Coordinate::new_2d(min_lon, min_lat),
        Coordinate::new_2d(max_lon, min_lat),
        Coordinate::new_2d(max_lon, max_lat),
        Coordinate::new_2d(min_lon, max_lat),
        Coordinate::new_2d(min_lon, min_lat), // Close the ring
    ]
}

/// Creates a polygon from coordinates
fn create_polygon(coords: Vec<Coordinate>) -> Result<Polygon, Box<dyn std::error::Error>> {
    let exterior = LineString::new(coords)?;
    Ok(Polygon::new(exterior, vec![])?)
}

/// Adds a city point feature
fn add_city_point(
    writer: &mut impl AddFeatureTrait,
    lon: f64,
    lat: f64,
    name: &str,
    country: &str,
    population: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let point = Point::new(lon, lat);
    let mut feature = Feature::new(Geometry::Point(point));
    feature.set_property("name", PropertyValue::String(name.to_string()));
    feature.set_property("country", PropertyValue::String(country.to_string()));
    feature.set_property("area_km2", PropertyValue::Float(0.0));
    feature.set_property("population", PropertyValue::Integer(population));
    writer.add_feature(&feature)?;
    Ok(())
}

/// Adds a mining city point feature
fn add_mining_city(
    writer: &mut impl AddFeatureTrait,
    lon: f64,
    lat: f64,
    name: &str,
    country: &str,
    _population: i64,
    mines: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    let point = Point::new(lon, lat);
    let mut feature = Feature::new(Geometry::Point(point));
    feature.set_property("name", PropertyValue::String(name.to_string()));
    feature.set_property("country", PropertyValue::String(country.to_string()));
    feature.set_property("iron_production_mt", PropertyValue::Float(0.0));
    feature.set_property("mining_sites", PropertyValue::Integer(i64::from(mines)));
    writer.add_feature(&feature)?;
    Ok(())
}

/// Trait to abstract over writer types
trait AddFeatureTrait {
    fn add_feature(&mut self, feature: &Feature) -> Result<(), Box<dyn std::error::Error>>;
}

impl<W: std::io::Write + std::io::Seek> AddFeatureTrait
    for oxigdal_flatgeobuf::writer::FlatGeobufWriter<W>
{
    fn add_feature(&mut self, feature: &Feature) -> Result<(), Box<dyn std::error::Error>> {
        self.add_feature(feature)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }
}
