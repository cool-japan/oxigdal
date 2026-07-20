//! MongoDB spatial data reader.

use crate::error::{Error, Result};
use crate::mongodb::{MongoDbConnector, geojson_to_geometry};
use futures::stream::TryStreamExt;
use geo_types::Geometry;
use mongodb::bson::{Document, doc};
use mongodb::options::FindOptions;

/// Feature read from MongoDB.
#[derive(Debug, Clone)]
pub struct MongoDbFeature {
    /// Document ID.
    pub id: mongodb::bson::oid::ObjectId,
    /// Geometry.
    pub geometry: Geometry<f64>,
    /// Properties (full document excluding geometry and _id).
    pub properties: Document,
}

/// MongoDB spatial data reader.
pub struct MongoDbReader {
    connector: MongoDbConnector,
    collection_name: String,
    geometry_field: String,
}

impl MongoDbReader {
    /// Create a new MongoDB reader.
    pub fn new(
        connector: MongoDbConnector,
        collection_name: String,
        geometry_field: String,
    ) -> Self {
        Self {
            connector,
            collection_name,
            geometry_field,
        }
    }

    /// Read all documents from the collection.
    pub async fn read_all(&self) -> Result<Vec<MongoDbFeature>> {
        self.read_with_filter(doc! {}).await
    }

    /// Read documents with a filter.
    pub async fn read_with_filter(&self, filter: Document) -> Result<Vec<MongoDbFeature>> {
        let collection = self.connector.collection(&self.collection_name);

        let mut cursor = collection
            .find(filter)
            .await
            .map_err(|e| Error::MongoDB(e.to_string()))?;

        let mut features = Vec::new();

        while let Some(doc) = cursor
            .try_next()
            .await
            .map_err(|e| Error::MongoDB(e.to_string()))?
        {
            if let Ok(feature) = self.doc_to_feature(doc) {
                features.push(feature);
            }
        }

        Ok(features)
    }

    /// Read documents within a bounding box using $geoWithin.
    pub async fn read_bbox(
        &self,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
    ) -> Result<Vec<MongoDbFeature>> {
        let filter = doc! {
            &self.geometry_field: {
                "$geoWithin": {
                    "$box": [
                        [min_x, min_y],
                        [max_x, max_y]
                    ]
                }
            }
        };

        self.read_with_filter(filter).await
    }

    /// Read documents near a point using $near.
    pub async fn read_near(
        &self,
        x: f64,
        y: f64,
        max_distance: f64,
    ) -> Result<Vec<MongoDbFeature>> {
        let filter = doc! {
            &self.geometry_field: {
                "$near": {
                    "$geometry": {
                        "type": "Point",
                        "coordinates": [x, y]
                    },
                    "$maxDistance": max_distance
                }
            }
        };

        self.read_with_filter(filter).await
    }

    /// Read documents that intersect with a geometry using $geoIntersects.
    pub async fn read_intersects(&self, geometry_doc: Document) -> Result<Vec<MongoDbFeature>> {
        let filter = doc! {
            &self.geometry_field: {
                "$geoIntersects": {
                    "$geometry": geometry_doc
                }
            }
        };

        self.read_with_filter(filter).await
    }

    /// Count documents in the collection.
    pub async fn count(&self) -> Result<u64> {
        let collection = self.connector.collection(&self.collection_name);
        collection
            .count_documents(doc! {})
            .await
            .map_err(|e| Error::MongoDB(e.to_string()))
    }

    /// Count documents matching a filter.
    pub async fn count_with_filter(&self, filter: Document) -> Result<u64> {
        let collection = self.connector.collection(&self.collection_name);
        collection
            .count_documents(filter)
            .await
            .map_err(|e| Error::MongoDB(e.to_string()))
    }

    /// Aggregate with $geoNear stage.
    pub async fn geo_near_aggregate(
        &self,
        x: f64,
        y: f64,
        max_distance: f64,
        distance_field: &str,
        limit: i64,
    ) -> Result<Vec<Document>> {
        let collection = self.connector.collection(&self.collection_name);

        let pipeline = vec![
            doc! {
                "$geoNear": {
                    "near": {
                        "type": "Point",
                        "coordinates": [x, y]
                    },
                    "distanceField": distance_field,
                    "maxDistance": max_distance,
                    "spherical": true
                }
            },
            doc! {
                "$limit": limit
            },
        ];

        let mut cursor = collection
            .aggregate(pipeline)
            .await
            .map_err(|e| Error::MongoDB(e.to_string()))?;

        let mut results = Vec::new();

        while let Some(doc) = cursor
            .try_next()
            .await
            .map_err(|e| Error::MongoDB(e.to_string()))?
        {
            results.push(doc);
        }

        Ok(results)
    }

    /// Read with pagination.
    pub async fn read_paginated(&self, skip: u64, limit: i64) -> Result<Vec<MongoDbFeature>> {
        let collection = self.connector.collection(&self.collection_name);

        let options = FindOptions::builder().skip(skip).limit(limit).build();

        let mut cursor = collection
            .find(doc! {})
            .with_options(options)
            .await
            .map_err(|e| Error::MongoDB(e.to_string()))?;

        let mut features = Vec::new();

        while let Some(doc) = cursor
            .try_next()
            .await
            .map_err(|e| Error::MongoDB(e.to_string()))?
        {
            if let Ok(feature) = self.doc_to_feature(doc) {
                features.push(feature);
            }
        }

        Ok(features)
    }

    /// Convert document to feature.
    fn doc_to_feature(&self, mut doc: Document) -> Result<MongoDbFeature> {
        let id = doc
            .get_object_id("_id")
            .map_err(|_| Error::MongoDB("Missing _id field".to_string()))?;

        let geom_doc = doc.get_document(&self.geometry_field).map_err(|_| {
            Error::MongoDB(format!("Missing geometry field: {}", self.geometry_field))
        })?;

        let geometry = geojson_to_geometry(geom_doc)?;

        doc.remove("_id");
        doc.remove(&self.geometry_field);

        Ok(MongoDbFeature {
            id,
            geometry,
            properties: doc,
        })
    }
}
