//! Create test GeoJSON files for various geographic regions
//!
//! This example demonstrates creating GeoJSON files with different feature types
//! for the COG viewer demo regions.
//!
//! Usage:
//!     cargo run --example create_test_geojson_samples
//!
//! Output:
//!     demo/cog-viewer/golden-triangle.geojson
//!     demo/cog-viewer/iron-belt.geojson

use serde_json::json;
use std::fs::File;
use std::io::Write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating test GeoJSON samples...\n");

    // Golden Triangle (Thailand/Myanmar/Laos)
    create_golden_triangle()?;

    // Basque Country (Spain/France)
    create_iron_belt()?;

    println!("\n✅ All GeoJSON samples created successfully!");
    println!("Files ready for use in COG viewer at http://localhost:8080/");

    Ok(())
}

fn create_golden_triangle() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating Golden Triangle GeoJSON...");

    let center_lon = 100.08466749884738;
    let center_lat = 20.35223590060906;

    // Create features
    let mut features = Vec::new();

    // Center point marker
    features.push(json!({
        "type": "Feature",
        "properties": {
            "name": "Golden Triangle Center",
            "description": "Thailand/Myanmar/Laos border region",
            "marker-color": "#FFD700",
            "marker-symbol": "star"
        },
        "geometry": {
            "type": "Point",
            "coordinates": [center_lon, center_lat]
        }
    }));

    // Triangle representing the three countries
    let offset = 0.05;
    features.push(json!({
        "type": "Feature",
        "properties": {
            "name": "Golden Triangle Area",
            "description": "Symbolic triangle",
            "fill": "#FFD700",
            "fill-opacity": 0.3,
            "stroke": "#B8860B",
            "stroke-width": 2
        },
        "geometry": {
            "type": "Polygon",
            "coordinates": [[
                [center_lon, center_lat + offset],
                [center_lon - offset * 0.866, center_lat - offset * 0.5],
                [center_lon + offset * 0.866, center_lat - offset * 0.5],
                [center_lon, center_lat + offset]
            ]]
        }
    }));

    // Points of interest
    let pois = [
        (100.05, 20.38, "Thailand Border", "🇹🇭"),
        (100.12, 20.35, "Myanmar Border", "🇲🇲"),
        (100.08, 20.32, "Laos Border", "🇱🇦"),
    ];

    for (lon, lat, name, flag) in pois.iter() {
        features.push(json!({
            "type": "Feature",
            "properties": {
                "name": name,
                "flag": flag,
                "marker-color": "#4169E1",
                "marker-size": "small"
            },
            "geometry": {
                "type": "Point",
                "coordinates": [lon, lat]
            }
        }));
    }

    // Create GeoJSON
    let geojson = json!({
        "type": "FeatureCollection",
        "name": "Golden Triangle",
        "features": features
    });

    // Write to file
    let path = "demo/cog-viewer/golden-triangle.geojson";
    let mut file = File::create(path)?;
    file.write_all(serde_json::to_string_pretty(&geojson)?.as_bytes())?;

    println!("  ✓ Created: {}", path);
    Ok(())
}

fn create_iron_belt() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating Basque Country GeoJSON...");

    let center_lon = -2.9253;
    let center_lat = 43.2630;

    let mut features = Vec::new();

    // Center point (Bilbao)
    features.push(json!({
        "type": "Feature",
        "properties": {
            "name": "Bilbao Mining Area",
            "description": "Central Basque mining region",
            "marker-color": "#CD7F32",
            "marker-symbol": "industrial"
        },
        "geometry": {
            "type": "Point",
            "coordinates": [center_lon, center_lat]
        }
    }));

    // Mining zones (circles approximated as polygons)
    let zones = [
        (-2.96, 43.25, "Orconera Mine", 0.02),
        (-2.91, 43.22, "Malaespera Mine", 0.025),
        (-2.88, 43.20, "Triano Mine", 0.02),
    ];

    for (lon, lat, name, radius) in zones.iter() {
        let mut coords = Vec::new();
        for i in 0..=32 {
            let angle = (i as f64) * std::f64::consts::PI * 2.0 / 32.0;
            coords.push([lon + radius * angle.cos(), lat + radius * angle.sin()]);
        }

        features.push(json!({
            "type": "Feature",
            "properties": {
                "name": name,
                "type": "Mining Zone",
                "fill": "#8B4513",
                "fill-opacity": 0.4,
                "stroke": "#654321",
                "stroke-width": 2
            },
            "geometry": {
                "type": "Polygon",
                "coordinates": [coords]
            }
        }));
    }

    // Transportation routes (roads connecting mines)
    features.push(json!({
        "type": "Feature",
        "properties": {
            "name": "Mining Road Network",
            "type": "Infrastructure",
            "stroke": "#696969",
            "stroke-width": 3
        },
        "geometry": {
            "type": "LineString",
            "coordinates": [
                [-2.96, 43.25],
                [-2.93, 43.24],
                [-2.91, 43.22],
                [-2.88, 43.20]
            ]
        }
    }));

    // Processing facilities
    let facilities = [
        (-2.94, 43.26, "Smelter A"),
        (-2.89, 43.21, "Processing Plant B"),
    ];

    for (lon, lat, name) in facilities.iter() {
        features.push(json!({
            "type": "Feature",
            "properties": {
                "name": name,
                "type": "Processing Facility",
                "marker-color": "#FF4500",
                "marker-symbol": "building"
            },
            "geometry": {
                "type": "Point",
                "coordinates": [lon, lat]
            }
        }));
    }

    let geojson = json!({
        "type": "FeatureCollection",
        "name": "Basque Country Mining Region",
        "features": features
    });

    let path = "demo/cog-viewer/iron-belt.geojson";
    let mut file = File::create(path)?;
    file.write_all(serde_json::to_string_pretty(&geojson)?.as_bytes())?;

    println!("  ✓ Created: {}", path);
    Ok(())
}
