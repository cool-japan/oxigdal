# Vector Processing Recipes

Common recipes for vector data processing with OxiGDAL.

## Reading Vectors

### Read All Features

```rust
use oxigdal_core::Dataset;

async fn read_all_features(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(path).await?;
    let layer = dataset.layer(0)?;

    for feature in layer.features()? {
        let geometry = feature.geometry()?;
        println!("Feature {}: {:?}", feature.id(), geometry.geometry_type());

        if let Some(name) = feature.field_as_string("name")? {
            println!("  Name: {}", name);
        }
    }

    Ok(())
}
```

### Filter by Attribute

```rust
async fn filter_by_attribute(path: &str, attribute: &str, value: &str) -> Result<Vec<Feature>, Box<dyn std::error::Error>> {
    let dataset = Dataset::open(path).await?;
    let mut layer = dataset.layer(0)?;

    layer.set_attribute_filter(&format!("{} = '{}'", attribute, value))?;

    let features: Vec<_> = layer.features()?.collect();
    println!("Found {} features matching filter", features.len());

    Ok(features)
}
```

## Creating Vectors

### Create Point Layer

```rust
use oxigdal_geojson::GeoJsonDriver;
use geo_types::{Point, Geometry};

async fn create_points(points: &[(f64, f64, String)]) -> Result<(), Box<dyn std::error::Error>> {
    let driver = GeoJsonDriver::new();
    let mut dataset = driver.create("points.geojson").await?;

    let mut layer = dataset.create_layer("points", None)?;
    layer.create_field("name", FieldType::String, 100)?;

    for (x, y, name) in points {
        let point = Point::new(*x, *y);
        let geometry = Geometry::Point(point);

        let mut feature = layer.create_feature()?;
        feature.set_geometry(geometry)?;
        feature.set_field_string("name", name)?;

        layer.add_feature(feature)?;
    }

    dataset.flush().await?;
    Ok(())
}
```

### Create Polygon Layer

```rust
use geo_types::{Polygon, Geometry};

async fn create_polygons(polygons: &[Vec<(f64, f64)>]) -> Result<(), Box<dyn std::error::Error>> {
    let driver = GeoJsonDriver::new();
    let mut dataset = driver.create("polygons.geojson").await?;

    let mut layer = dataset.create_layer("polygons", None)?;
    layer.create_field("id", FieldType::Integer, 0)?;
    layer.create_field("area", FieldType::Real, 0)?;

    for (id, coords) in polygons.iter().enumerate() {
        let mut ring = coords.clone();
        ring.push(coords[0]);  // Close the ring

        let polygon = Polygon::new(ring.into(), vec![]);
        let geometry = Geometry::Polygon(polygon);

        use geo::Area;
        let area = polygon.unsigned_area();

        let mut feature = layer.create_feature()?;
        feature.set_geometry(geometry)?;
        feature.set_field_integer("id", id as i64)?;
        feature.set_field_double("area", area)?;

        layer.add_feature(feature)?;
    }

    dataset.flush().await?;
    Ok(())
}
```

## Spatial Operations

### Point in Polygon

```rust
use geo::Contains;

async fn find_containing_polygon(point_path: &str, polygon_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let points = Dataset::open(point_path).await?;
    let polygons = Dataset::open(polygon_path).await?;

    let point_layer = points.layer(0)?;
    let poly_layer = polygons.layer(0)?;

    for point_feature in point_layer.features()? {
        let point_geom = point_feature.geometry()?;

        if let Some(point) = point_geom.as_point() {
            for poly_feature in poly_layer.features()? {
                let poly_geom = poly_feature.geometry()?;

                if let Some(polygon) = poly_geom.as_polygon() {
                    if polygon.contains(&point) {
                        if let Some(name) = poly_feature.field_as_string("name")? {
                            println!("Point contained in: {}", name);
                        }
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}
```

### Buffer

```rust
use geo::algorithm::Buffer;

async fn buffer_features(input: &str, output: &str, distance: f64) -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(input).await?;
    let layer = dataset.layer(0)?;

    let driver = GeoJsonDriver::new();
    let mut out_dataset = driver.create(output).await?;
    let mut out_layer = out_dataset.create_layer("buffered", None)?;

    for feature in layer.features()? {
        let geometry = feature.geometry()?;

        if let Some(point) = geometry.as_point() {
            let buffered = point.buffer(distance);

            let mut out_feature = out_layer.create_feature()?;
            out_feature.set_geometry(Geometry::Polygon(buffered))?;
            out_layer.add_feature(out_feature)?;
        }
    }

    out_dataset.flush().await?;
    Ok(())
}
```

### Intersection

```rust
use geo::algorithm::Intersects;

async fn find_intersections(layer1_path: &str, layer2_path: &str) -> Result<Vec<(usize, usize)>, Box<dyn std::error::Error>> {
    let dataset1 = Dataset::open(layer1_path).await?;
    let dataset2 = Dataset::open(layer2_path).await?;

    let layer1 = dataset1.layer(0)?;
    let layer2 = dataset2.layer(0)?;

    let mut intersections = Vec::new();

    for (i, feature1) in layer1.features()?.enumerate() {
        let geom1 = feature1.geometry()?;

        for (j, feature2) in layer2.features()?.enumerate() {
            let geom2 = feature2.geometry()?;

            if let (Some(poly1), Some(poly2)) = (geom1.as_polygon(), geom2.as_polygon()) {
                if poly1.intersects(poly2) {
                    intersections.push((i, j));
                }
            }
        }
    }

    println!("Found {} intersections", intersections.len());
    Ok(intersections)
}
```

## Attribute Operations

### Calculate Field

```rust
async fn calculate_area(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut dataset = Dataset::open_writable(path).await?;
    let mut layer = dataset.layer_mut(0)?;

    // Ensure area field exists
    if !layer.has_field("area")? {
        layer.create_field("area", FieldType::Real, 0)?;
    }

    for mut feature in layer.features_mut()? {
        let geometry = feature.geometry()?;

        if let Some(polygon) = geometry.as_polygon() {
            use geo::Area;
            let area = polygon.unsigned_area();
            feature.set_field_double("area", area)?;
            layer.update_feature(feature)?;
        }
    }

    dataset.flush().await?;
    Ok(())
}
```

### Join Attributes

```rust
async fn join_attributes(target_path: &str, join_path: &str, key_field: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut target = Dataset::open_writable(target_path).await?;
    let join_dataset = Dataset::open(join_path).await?;

    let mut target_layer = target.layer_mut(0)?;
    let join_layer = join_dataset.layer(0)?;

    // Build lookup table
    let mut lookup = std::collections::HashMap::new();
    for feature in join_layer.features()? {
        if let Some(key) = feature.field_as_string(key_field)? {
            lookup.insert(key, feature);
        }
    }

    // Join attributes
    for mut feature in target_layer.features_mut()? {
        if let Some(key) = feature.field_as_string(key_field)? {
            if let Some(join_feature) = lookup.get(&key) {
                // Copy fields from join feature
                for field_name in join_feature.field_names()? {
                    if field_name != key_field {
                        if let Some(value) = join_feature.field_as_string(&field_name)? {
                            feature.set_field_string(&field_name, &value)?;
                        }
                    }
                }

                target_layer.update_feature(feature)?;
            }
        }
    }

    target.flush().await?;
    Ok(())
}
```

## Format Conversion

### GeoJSON to Shapefile

```rust
use oxigdal_shapefile::ShapefileDriver;

async fn geojson_to_shapefile(input: &str, output: &str) -> Result<(), Box<dyn std::error::Error>> {
    let src = Dataset::open(input).await?;
    let src_layer = src.layer(0)?;

    let driver = ShapefileDriver::new();
    let mut dst = driver.create(output).await?;

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

### Shapefile to GeoParquet

```rust
use oxigdal_geoparquet::GeoParquetDriver;

async fn shapefile_to_geoparquet(input: &str, output: &str) -> Result<(), Box<dyn std::error::Error>> {
    let src = Dataset::open(input).await?;
    let src_layer = src.layer(0)?;

    let driver = GeoParquetDriver::new();
    let mut dst = driver.create(output).await?;

    let srs = src_layer.spatial_ref()?;
    let mut dst_layer = dst.create_layer("data", Some(&srs))?;

    // Copy schema and data
    let layer_def = src_layer.definition()?;
    for i in 0..layer_def.field_count() {
        let field = layer_def.field(i)?;
        dst_layer.create_field(field.name(), field.field_type(), field.width())?;
    }

    for feature in src_layer.features()? {
        dst_layer.add_feature(feature)?;
    }

    dst.flush().await?;
    Ok(())
}
```

## Simplification

### Douglas-Peucker Simplification

```rust
use geo::algorithm::Simplify;

async fn simplify_geometries(input: &str, output: &str, tolerance: f64) -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(input).await?;
    let layer = dataset.layer(0)?;

    let driver = GeoJsonDriver::new();
    let mut out_dataset = driver.create(output).await?;
    let mut out_layer = out_dataset.create_layer("simplified", layer.spatial_ref()?.into())?;

    for feature in layer.features()? {
        let geometry = feature.geometry()?;

        let simplified_geom = match geometry {
            Geometry::LineString(ls) => {
                Geometry::LineString(ls.simplify(&tolerance))
            }
            Geometry::Polygon(poly) => {
                Geometry::Polygon(poly.simplify(&tolerance))
            }
            other => other,
        };

        let mut out_feature = out_layer.create_feature()?;
        out_feature.set_geometry(simplified_geom)?;
        out_layer.add_feature(out_feature)?;
    }

    out_dataset.flush().await?;
    Ok(())
}
```

## Merging

### Merge Multiple Files

```rust
async fn merge_vector_files(inputs: &[&str], output: &str) -> Result<(), Box<dyn std::error::Error>> {
    let driver = GeoJsonDriver::new();
    let mut out_dataset = driver.create(output).await?;

    let first = Dataset::open(inputs[0]).await?;
    let first_layer = first.layer(0)?;

    let mut out_layer = out_dataset.create_layer("merged", first_layer.spatial_ref()?.into())?;

    // Copy schema from first file
    let layer_def = first_layer.definition()?;
    for i in 0..layer_def.field_count() {
        let field = layer_def.field(i)?;
        out_layer.create_field(field.name(), field.field_type(), field.width())?;
    }

    // Merge all features
    for input in inputs {
        let dataset = Dataset::open(input).await?;
        let layer = dataset.layer(0)?;

        for feature in layer.features()? {
            out_layer.add_feature(feature)?;
        }
    }

    out_dataset.flush().await?;
    Ok(())
}
```

---

Copyright (c) 2025 COOLJAPAN OU (Team Kitasan)
Licensed under Apache-2.0
