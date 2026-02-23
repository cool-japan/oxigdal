# Working with Vector Data in OxiGDAL

## Overview

This tutorial covers reading, writing, and processing vector geospatial data including points, lines, polygons, and their attributes.

## Reading Vector Data

### Opening a Vector Dataset

```rust
use oxigdal_core::Dataset;

async fn open_vector() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("cities.geojson").await?;

    println!("Number of layers: {}", dataset.layer_count());

    for i in 0..dataset.layer_count() {
        let layer = dataset.layer(i)?;
        println!("Layer {}: {} ({} features)",
                 i,
                 layer.name(),
                 layer.feature_count()?);
    }

    Ok(())
}
```

### Reading Features

```rust
use oxigdal_core::Dataset;

async fn read_features() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("boundaries.shp").await?;
    let layer = dataset.layer(0)?;

    for feature in layer.features()? {
        let fid = feature.id();
        let geometry = feature.geometry()?;

        println!("Feature {}: {:?}", fid, geometry.geometry_type());

        // Access attributes
        if let Some(name) = feature.field_as_string("name")? {
            println!("  Name: {}", name);
        }

        if let Some(area) = feature.field_as_double("area")? {
            println!("  Area: {}", area);
        }
    }

    Ok(())
}
```

### Filtering Features

```rust
use oxigdal_core::Dataset;

async fn filter_features() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("data.geojson").await?;
    let mut layer = dataset.layer(0)?;

    // SQL filter
    layer.set_attribute_filter("population > 100000")?;

    for feature in layer.features()? {
        if let Some(name) = feature.field_as_string("name")? {
            if let Some(pop) = feature.field_as_integer("population")? {
                println!("{}: {}", name, pop);
            }
        }
    }

    Ok(())
}
```

## Geometry Types

### Working with Points

```rust
use oxigdal_core::{Dataset, Geometry};
use geo_types::Point;

async fn process_points() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("points.geojson").await?;
    let layer = dataset.layer(0)?;

    for feature in layer.features()? {
        let geometry = feature.geometry()?;

        if let Some(point) = geometry.as_point() {
            println!("Point: ({}, {})", point.x(), point.y());
        }
    }

    Ok(())
}
```

### Working with LineStrings

```rust
use oxigdal_core::Dataset;

async fn process_lines() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("roads.geojson").await?;
    let layer = dataset.layer(0)?;

    for feature in layer.features()? {
        let geometry = feature.geometry()?;

        if let Some(linestring) = geometry.as_linestring() {
            println!("Line with {} points", linestring.coords().count());

            // Calculate length
            let length = calculate_line_length(&linestring);
            println!("  Length: {} meters", length);
        }
    }

    Ok(())
}

fn calculate_line_length(linestring: &geo_types::LineString<f64>) -> f64 {
    let mut length = 0.0;
    let coords: Vec<_> = linestring.coords().collect();

    for i in 1..coords.len() {
        let dx = coords[i].x - coords[i-1].x;
        let dy = coords[i].y - coords[i-1].y;
        length += (dx*dx + dy*dy).sqrt();
    }

    length
}
```

### Working with Polygons

```rust
use oxigdal_core::Dataset;
use geo::Area;

async fn process_polygons() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("parcels.geojson").await?;
    let layer = dataset.layer(0)?;

    for feature in layer.features()? {
        let geometry = feature.geometry()?;

        if let Some(polygon) = geometry.as_polygon() {
            // Calculate area
            let area = polygon.unsigned_area();
            println!("Polygon area: {} sq meters", area);

            // Get exterior ring
            let exterior = polygon.exterior();
            println!("  Exterior ring: {} points", exterior.coords().count());

            // Get interior rings (holes)
            println!("  Interior rings: {}", polygon.interiors().len());
        }
    }

    Ok(())
}
```

## Writing Vector Data

### Creating a GeoJSON File

```rust
use oxigdal_core::{Dataset, Driver, FieldType};
use oxigdal_geojson::GeoJsonDriver;
use geo_types::{Point, Geometry};

async fn create_geojson() -> Result<(), Box<dyn std::error::Error>> {
    let driver = GeoJsonDriver::new();
    let mut dataset = driver.create("output.geojson").await?;

    // Create layer
    let mut layer = dataset.create_layer("cities", None)?;

    // Add fields
    layer.create_field("name", FieldType::String, 100)?;
    layer.create_field("population", FieldType::Integer, 0)?;
    layer.create_field("elevation", FieldType::Real, 0)?;

    // Add features
    let cities = vec![
        ("Tokyo", 13960000, 40.0),
        ("New York", 8336000, 10.0),
        ("London", 8982000, 11.0),
    ];

    for (name, pop, elev) in cities {
        let point = Point::new(0.0, 0.0);  // Replace with actual coordinates
        let geometry = Geometry::Point(point);

        let mut feature = layer.create_feature()?;
        feature.set_geometry(geometry)?;
        feature.set_field_string("name", name)?;
        feature.set_field_integer("population", pop)?;
        feature.set_field_double("elevation", elev)?;

        layer.add_feature(feature)?;
    }

    dataset.flush().await?;
    Ok(())
}
```

### Creating a Shapefile

```rust
use oxigdal_core::{Dataset, Driver};
use oxigdal_shapefile::ShapefileDriver;
use oxigdal_proj::SpatialRef;
use geo_types::{Polygon, Geometry};

async fn create_shapefile() -> Result<(), Box<dyn std::error::Error>> {
    let driver = ShapefileDriver::new();
    let mut dataset = driver.create("output.shp").await?;

    // Create layer with spatial reference
    let srs = SpatialRef::from_epsg(4326)?;
    let mut layer = dataset.create_layer("polygons", Some(&srs))?;

    // Add fields
    layer.create_field("id", FieldType::Integer, 0)?;
    layer.create_field("area", FieldType::Real, 0)?;

    // Create a polygon
    let exterior = vec![
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 10.0),
        (0.0, 10.0),
        (0.0, 0.0),
    ];
    let polygon = Polygon::new(exterior.into(), vec![]);
    let geometry = Geometry::Polygon(polygon);

    let mut feature = layer.create_feature()?;
    feature.set_geometry(geometry)?;
    feature.set_field_integer("id", 1)?;
    feature.set_field_double("area", 100.0)?;

    layer.add_feature(feature)?;

    dataset.flush().await?;
    Ok(())
}
```

## Spatial Operations

### Point in Polygon

```rust
use oxigdal_core::Dataset;
use geo::{Contains, Point};

async fn point_in_polygon() -> Result<(), Box<dyn std::error::Error>> {
    let polygons = Dataset::open("zones.geojson").await?;
    let points = Dataset::open("locations.geojson").await?;

    let poly_layer = polygons.layer(0)?;
    let point_layer = points.layer(0)?;

    for point_feature in point_layer.features()? {
        let point_geom = point_feature.geometry()?;

        if let Some(point) = point_geom.as_point() {
            // Check which polygon contains this point
            for poly_feature in poly_layer.features()? {
                let poly_geom = poly_feature.geometry()?;

                if let Some(polygon) = poly_geom.as_polygon() {
                    if polygon.contains(&point) {
                        let zone_name = poly_feature.field_as_string("name")?
                            .unwrap_or_else(|| "Unknown".to_string());
                        println!("Point in zone: {}", zone_name);
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}
```

### Buffer Operation

```rust
use geo::algorithm::Buffer;
use oxigdal_core::Dataset;

async fn buffer_geometries() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("roads.geojson").await?;
    let layer = dataset.layer(0)?;

    let buffer_distance = 100.0;  // meters

    for feature in layer.features()? {
        let geometry = feature.geometry()?;

        if let Some(linestring) = geometry.as_linestring() {
            // Create buffer around line
            let buffered = linestring.buffer(buffer_distance);

            println!("Buffered geometry: {:?}", buffered);
        }
    }

    Ok(())
}
```

### Intersection

```rust
use geo::algorithm::Intersects;
use oxigdal_core::Dataset;

async fn find_intersections() -> Result<(), Box<dyn std::error::Error>> {
    let dataset1 = Dataset::open("layer1.geojson").await?;
    let dataset2 = Dataset::open("layer2.geojson").await?;

    let layer1 = dataset1.layer(0)?;
    let layer2 = dataset2.layer(0)?;

    for feature1 in layer1.features()? {
        let geom1 = feature1.geometry()?;

        for feature2 in layer2.features()? {
            let geom2 = feature2.geometry()?;

            if let (Some(poly1), Some(poly2)) = (geom1.as_polygon(), geom2.as_polygon()) {
                if poly1.intersects(poly2) {
                    println!("Intersection found!");
                }
            }
        }
    }

    Ok(())
}
```

## Attribute Manipulation

### Reading Attributes

```rust
use oxigdal_core::Dataset;

async fn read_attributes() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("data.geojson").await?;
    let layer = dataset.layer(0)?;

    let layer_def = layer.definition()?;

    // Print field definitions
    for i in 0..layer_def.field_count() {
        let field = layer_def.field(i)?;
        println!("Field {}: {} ({:?})",
                 i,
                 field.name(),
                 field.field_type());
    }

    // Read feature attributes
    for feature in layer.features()? {
        for i in 0..layer_def.field_count() {
            let field = layer_def.field(i)?;
            let value = match field.field_type() {
                FieldType::Integer => {
                    feature.field_as_integer(field.name())?
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "NULL".to_string())
                }
                FieldType::Real => {
                    feature.field_as_double(field.name())?
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "NULL".to_string())
                }
                FieldType::String => {
                    feature.field_as_string(field.name())?
                        .unwrap_or_else(|| "NULL".to_string())
                }
                _ => "UNSUPPORTED".to_string(),
            };

            println!("  {}: {}", field.name(), value);
        }
    }

    Ok(())
}
```

### Updating Attributes

```rust
use oxigdal_core::Dataset;

async fn update_attributes() -> Result<(), Box<dyn std::error::Error>> {
    let mut dataset = Dataset::open_writable("data.geojson").await?;
    let mut layer = dataset.layer_mut(0)?;

    for mut feature in layer.features_mut()? {
        // Update existing field
        if let Some(current_value) = feature.field_as_integer("count")? {
            feature.set_field_integer("count", current_value + 1)?;
        }

        // Add new calculated field
        let geometry = feature.geometry()?;
        if let Some(polygon) = geometry.as_polygon() {
            use geo::Area;
            let area = polygon.unsigned_area();
            feature.set_field_double("area", area)?;
        }

        layer.update_feature(feature)?;
    }

    dataset.flush().await?;
    Ok(())
}
```

## Format Conversion

### GeoJSON to Shapefile

```rust
use oxigdal_core::Dataset;
use oxigdal_shapefile::ShapefileDriver;

async fn geojson_to_shapefile() -> Result<(), Box<dyn std::error::Error>> {
    // Open source
    let src = Dataset::open("input.geojson").await?;
    let src_layer = src.layer(0)?;

    // Create destination
    let driver = ShapefileDriver::new();
    let mut dst = driver.create("output.shp").await?;

    // Copy layer structure
    let srs = src_layer.spatial_ref()?;
    let mut dst_layer = dst.create_layer("converted", Some(&srs))?;

    // Copy field definitions
    let layer_def = src_layer.definition()?;
    for i in 0..layer_def.field_count() {
        let field = layer_def.field(i)?;
        dst_layer.create_field(field.name(), field.field_type(), field.width())?;
    }

    // Copy features
    for feature in src_layer.features()? {
        dst_layer.add_feature(feature)?;
    }

    dst.flush().await?;
    Ok(())
}
```

## Spatial Joins

### Join by Location

```rust
use oxigdal_core::Dataset;
use geo::Contains;

async fn spatial_join() -> Result<(), Box<dyn std::error::Error>> {
    let polygons = Dataset::open("zones.geojson").await?;
    let points = Dataset::open("locations.geojson").await?;

    let poly_layer = polygons.layer(0)?;
    let point_layer = points.layer(0)?;

    // Create output
    let mut output = Dataset::create_vector("joined.geojson").await?;
    let mut out_layer = output.create_layer("joined", None)?;

    // Create combined fields
    out_layer.create_field("point_id", FieldType::Integer, 0)?;
    out_layer.create_field("zone_name", FieldType::String, 100)?;

    // Perform join
    for point_feature in point_layer.features()? {
        let point_geom = point_feature.geometry()?;

        if let Some(point) = point_geom.as_point() {
            for poly_feature in poly_layer.features()? {
                let poly_geom = poly_feature.geometry()?;

                if let Some(polygon) = poly_geom.as_polygon() {
                    if polygon.contains(&point) {
                        let mut out_feature = out_layer.create_feature()?;
                        out_feature.set_geometry(point_geom.clone())?;

                        if let Some(id) = point_feature.field_as_integer("id")? {
                            out_feature.set_field_integer("point_id", id)?;
                        }

                        if let Some(name) = poly_feature.field_as_string("name")? {
                            out_feature.set_field_string("zone_name", &name)?;
                        }

                        out_layer.add_feature(out_feature)?;
                        break;
                    }
                }
            }
        }
    }

    output.flush().await?;
    Ok(())
}
```

## Complete Example: Vector Analysis Workflow

```rust
use oxigdal_core::{Dataset, FieldType};
use oxigdal_geojson::GeoJsonDriver;
use geo::{Area, Contains, Point};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Load boundary polygons
    let boundaries = Dataset::open("city_boundaries.geojson").await?;
    let boundary_layer = boundaries.layer(0)?;

    // 2. Load point locations
    let locations = Dataset::open("locations.geojson").await?;
    let location_layer = locations.layer(0)?;

    // 3. Create output dataset
    let driver = GeoJsonDriver::new();
    let mut output = driver.create("analysis_results.geojson").await?;
    let mut out_layer = output.create_layer("results", None)?;

    // 4. Define output fields
    out_layer.create_field("city", FieldType::String, 100)?;
    out_layer.create_field("point_count", FieldType::Integer, 0)?;
    out_layer.create_field("area_sqkm", FieldType::Real, 0)?;

    // 5. Process each boundary
    for boundary_feature in boundary_layer.features()? {
        let boundary_geom = boundary_feature.geometry()?;

        if let Some(polygon) = boundary_geom.as_polygon() {
            // Count points within boundary
            let mut point_count = 0;

            for location_feature in location_layer.features()? {
                let location_geom = location_feature.geometry()?;

                if let Some(point) = location_geom.as_point() {
                    if polygon.contains(&point) {
                        point_count += 1;
                    }
                }
            }

            // Calculate area in square kilometers
            let area_sqm = polygon.unsigned_area();
            let area_sqkm = area_sqm / 1_000_000.0;

            // Create output feature
            let mut out_feature = out_layer.create_feature()?;
            out_feature.set_geometry(boundary_geom)?;

            if let Some(city_name) = boundary_feature.field_as_string("name")? {
                out_feature.set_field_string("city", &city_name)?;
            }

            out_feature.set_field_integer("point_count", point_count)?;
            out_feature.set_field_double("area_sqkm", area_sqkm)?;

            out_layer.add_feature(out_feature)?;

            println!("Processed: {} points, {:.2} km²", point_count, area_sqkm);
        }
    }

    output.flush().await?;
    println!("Analysis complete!");

    Ok(())
}
```

## Performance Tips

1. **Spatial indexing** - Use spatial indexes for large datasets
2. **Attribute filtering** - Filter early to reduce processing
3. **Geometry simplification** - Simplify complex geometries when appropriate
4. **Batch operations** - Group similar operations together
5. **Appropriate formats** - Use GeoParquet for large datasets

## Next Steps

- Learn about [Projections](05_projections.md)
- Explore [Cloud Storage](06_cloud_storage.md)
- Study [Distributed Processing](08_distributed_processing.md)

---

Copyright (c) 2025 COOLJAPAN OU (Team Kitasan)
Licensed under Apache-2.0
