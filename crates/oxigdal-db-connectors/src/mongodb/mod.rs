//! MongoDB spatial database connector.
//!
//! Provides support for reading and writing GeoJSON data to MongoDB
//! with native geospatial query support.

pub mod reader;
pub mod writer;

use crate::error::{Error, Result};
use geo_types::Geometry;
use mongodb::{
    Client, Collection, Database, IndexModel,
    bson::{Document, doc},
    options::{ClientOptions, IndexOptions},
};
use std::time::Duration;

/// MongoDB connector configuration.
#[derive(Debug, Clone)]
pub struct MongoDbConfig {
    /// Connection URI.
    pub uri: String,
    /// Database name.
    pub database: String,
    /// Connection timeout.
    pub connection_timeout: Duration,
    /// Server selection timeout.
    pub server_selection_timeout: Duration,
    /// Application name.
    pub app_name: Option<String>,
}

impl Default for MongoDbConfig {
    fn default() -> Self {
        Self {
            uri: "mongodb://localhost:27017".to_string(),
            database: "gis".to_string(),
            connection_timeout: Duration::from_secs(30),
            server_selection_timeout: Duration::from_secs(30),
            app_name: Some("oxigdal".to_string()),
        }
    }
}

/// MongoDB spatial database connector.
pub struct MongoDbConnector {
    #[allow(dead_code)]
    client: Client,
    database: Database,
    #[allow(dead_code)]
    config: MongoDbConfig,
}

impl MongoDbConnector {
    /// Create a new MongoDB connector.
    pub async fn new(config: MongoDbConfig) -> Result<Self> {
        let mut client_options = ClientOptions::parse(&config.uri)
            .await
            .map_err(|e| Error::MongoDB(e.to_string()))?;

        client_options.connect_timeout = Some(config.connection_timeout);
        client_options.server_selection_timeout = Some(config.server_selection_timeout);
        client_options.app_name = config.app_name.clone();

        let client =
            Client::with_options(client_options).map_err(|e| Error::MongoDB(e.to_string()))?;

        let database = client.database(&config.database);

        Ok(Self {
            client,
            database,
            config,
        })
    }

    /// Get a collection.
    pub fn collection(&self, name: &str) -> Collection<Document> {
        self.database.collection(name)
    }

    /// Get database reference.
    pub fn database(&self) -> &Database {
        &self.database
    }

    /// Check if the connection is healthy.
    pub async fn health_check(&self) -> Result<bool> {
        let result = self
            .database
            .run_command(doc! { "ping": 1 })
            .await
            .map_err(|e| Error::MongoDB(e.to_string()))?;

        Ok(result.get_i32("ok") == Ok(1))
    }

    /// Get database version.
    pub async fn version(&self) -> Result<String> {
        let result = self
            .database
            .run_command(doc! { "buildInfo": 1 })
            .await
            .map_err(|e| Error::MongoDB(e.to_string()))?;

        result
            .get_str("version")
            .map(|s| s.to_string())
            .map_err(|e| Error::MongoDB(e.to_string()))
    }

    /// List all collections.
    pub async fn list_collections(&self) -> Result<Vec<String>> {
        let collections = self
            .database
            .list_collection_names()
            .await
            .map_err(|e| Error::MongoDB(e.to_string()))?;

        Ok(collections)
    }

    /// Create a 2dsphere index for geospatial queries.
    pub async fn create_geo_index(
        &self,
        collection_name: &str,
        geometry_field: &str,
    ) -> Result<()> {
        let collection = self.collection(collection_name);

        let index = IndexModel::builder()
            .keys(doc! { geometry_field: "2dsphere" })
            .options(
                IndexOptions::builder()
                    .name(format!("{}_2dsphere", geometry_field))
                    .build(),
            )
            .build();

        collection
            .create_index(index)
            .await
            .map_err(|e| Error::MongoDB(e.to_string()))?;

        Ok(())
    }

    /// Drop a collection.
    pub async fn drop_collection(&self, collection_name: &str) -> Result<()> {
        let collection = self.collection(collection_name);
        collection
            .drop()
            .await
            .map_err(|e| Error::MongoDB(e.to_string()))?;

        Ok(())
    }

    /// Get collection statistics.
    pub async fn collection_stats(&self, collection_name: &str) -> Result<Document> {
        let result = self
            .database
            .run_command(doc! { "collStats": collection_name })
            .await
            .map_err(|e| Error::MongoDB(e.to_string()))?;

        Ok(result)
    }
}

/// Convert geo-types Geometry to GeoJSON document.
pub fn geometry_to_geojson(geom: &Geometry<f64>) -> Result<Document> {
    match geom {
        Geometry::Point(p) => Ok(doc! {
            "type": "Point",
            "coordinates": [p.x(), p.y()]
        }),
        Geometry::LineString(ls) => {
            let coords: Vec<_> = ls.coords().map(|c| vec![c.x, c.y]).collect();
            Ok(doc! {
                "type": "LineString",
                "coordinates": coords
            })
        }
        Geometry::Polygon(poly) => {
            let mut rings = Vec::new();

            // Exterior ring
            let exterior: Vec<_> = poly.exterior().coords().map(|c| vec![c.x, c.y]).collect();
            rings.push(exterior);

            // Interior rings
            for interior in poly.interiors() {
                let interior_coords: Vec<_> = interior.coords().map(|c| vec![c.x, c.y]).collect();
                rings.push(interior_coords);
            }

            Ok(doc! {
                "type": "Polygon",
                "coordinates": rings
            })
        }
        Geometry::MultiPoint(mp) => {
            let coords: Vec<_> = mp.iter().map(|p| vec![p.x(), p.y()]).collect();
            Ok(doc! {
                "type": "MultiPoint",
                "coordinates": coords
            })
        }
        Geometry::MultiLineString(mls) => {
            let lines: Vec<_> = mls
                .iter()
                .map(|ls| ls.coords().map(|c| vec![c.x, c.y]).collect::<Vec<_>>())
                .collect();

            Ok(doc! {
                "type": "MultiLineString",
                "coordinates": lines
            })
        }
        Geometry::MultiPolygon(mpoly) => {
            let polygons: Vec<_> = mpoly
                .iter()
                .map(|poly| {
                    let mut rings = Vec::new();
                    let exterior: Vec<_> =
                        poly.exterior().coords().map(|c| vec![c.x, c.y]).collect();
                    rings.push(exterior);

                    for interior in poly.interiors() {
                        let interior_coords: Vec<_> =
                            interior.coords().map(|c| vec![c.x, c.y]).collect();
                        rings.push(interior_coords);
                    }

                    rings
                })
                .collect();

            Ok(doc! {
                "type": "MultiPolygon",
                "coordinates": polygons
            })
        }
        _ => Err(Error::TypeConversion(format!(
            "Unsupported geometry type: {:?}",
            geom
        ))),
    }
}

/// Convert GeoJSON document to geo-types Geometry.
pub fn geojson_to_geometry(doc: &Document) -> Result<Geometry<f64>> {
    use geo_types::{Coord, LineString, Point, Polygon};

    let geom_type = doc
        .get_str("type")
        .map_err(|_| Error::GeometryParsing("Missing or invalid 'type' field".to_string()))?;

    match geom_type {
        "Point" => {
            let coords = doc
                .get_array("coordinates")
                .map_err(|_| Error::GeometryParsing("Missing coordinates".to_string()))?;

            if coords.len() != 2 {
                return Err(Error::GeometryParsing(
                    "Invalid Point coordinates".to_string(),
                ));
            }

            let x = coords[0]
                .as_f64()
                .ok_or_else(|| Error::GeometryParsing("Invalid x coordinate".to_string()))?;
            let y = coords[1]
                .as_f64()
                .ok_or_else(|| Error::GeometryParsing("Invalid y coordinate".to_string()))?;

            Ok(Geometry::Point(Point::new(x, y)))
        }
        "LineString" => {
            let coords = doc
                .get_array("coordinates")
                .map_err(|_| Error::GeometryParsing("Missing coordinates".to_string()))?;

            let line_coords: Result<Vec<Coord<f64>>> = coords
                .iter()
                .map(|c| {
                    let arr = c
                        .as_array()
                        .ok_or_else(|| Error::GeometryParsing("Invalid coordinate".to_string()))?;
                    if arr.len() != 2 {
                        return Err(Error::GeometryParsing(
                            "Invalid coordinate length".to_string(),
                        ));
                    }
                    let x = arr[0]
                        .as_f64()
                        .ok_or_else(|| Error::GeometryParsing("Invalid x".to_string()))?;
                    let y = arr[1]
                        .as_f64()
                        .ok_or_else(|| Error::GeometryParsing("Invalid y".to_string()))?;
                    Ok(Coord { x, y })
                })
                .collect();

            Ok(Geometry::LineString(LineString::from(line_coords?)))
        }
        "Polygon" => {
            let rings = doc
                .get_array("coordinates")
                .map_err(|_| Error::GeometryParsing("Missing coordinates".to_string()))?;

            if rings.is_empty() {
                return Err(Error::GeometryParsing("Polygon has no rings".to_string()));
            }

            let parse_ring = |ring: &mongodb::bson::Bson| -> Result<LineString<f64>> {
                let coords = ring
                    .as_array()
                    .ok_or_else(|| Error::GeometryParsing("Invalid ring".to_string()))?;

                let line_coords: Result<Vec<Coord<f64>>> = coords
                    .iter()
                    .map(|c| {
                        let arr = c.as_array().ok_or_else(|| {
                            Error::GeometryParsing("Invalid coordinate".to_string())
                        })?;
                        if arr.len() != 2 {
                            return Err(Error::GeometryParsing(
                                "Invalid coordinate length".to_string(),
                            ));
                        }
                        let x = arr[0]
                            .as_f64()
                            .ok_or_else(|| Error::GeometryParsing("Invalid x".to_string()))?;
                        let y = arr[1]
                            .as_f64()
                            .ok_or_else(|| Error::GeometryParsing("Invalid y".to_string()))?;
                        Ok(Coord { x, y })
                    })
                    .collect();

                Ok(LineString::from(line_coords?))
            };

            let exterior = parse_ring(&rings[0])?;
            let interiors: Result<Vec<LineString<f64>>> =
                rings.iter().skip(1).map(parse_ring).collect();

            Ok(Geometry::Polygon(Polygon::new(exterior, interiors?)))
        }
        _ => Err(Error::GeometryParsing(format!(
            "Unsupported geometry type: {}",
            geom_type
        ))),
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use geo_types::point;

    #[test]
    fn test_point_to_geojson() {
        let p = Geometry::Point(point!(x: 1.0, y: 2.0));
        let doc = geometry_to_geojson(&p).expect("Failed to convert");

        assert_eq!(doc.get_str("type").ok(), Some("Point"));
        let coords = doc.get_array("coordinates").expect("No coordinates");
        assert_eq!(coords.len(), 2);
    }

    #[test]
    fn test_geojson_to_point() {
        let doc = doc! {
            "type": "Point",
            "coordinates": [1.0, 2.0]
        };

        let geom = geojson_to_geometry(&doc).expect("Failed to parse");
        match geom {
            Geometry::Point(p) => {
                assert_eq!(p.x(), 1.0);
                assert_eq!(p.y(), 2.0);
            }
            _ => panic!("Expected Point geometry"),
        }
    }
}
