//! MongoDB spatial data writer.

use crate::error::{Error, Result};
use crate::mongodb::{MongoDbConnector, geometry_to_geojson};
use geo_types::Geometry;
use mongodb::bson::{Document, doc, oid::ObjectId};
use mongodb::options::{InsertManyOptions, UpdateOptions};

/// MongoDB spatial data writer.
pub struct MongoDbWriter {
    connector: MongoDbConnector,
    collection_name: String,
    geometry_field: String,
    batch_size: usize,
}

impl MongoDbWriter {
    /// Create a new MongoDB writer.
    pub fn new(
        connector: MongoDbConnector,
        collection_name: String,
        geometry_field: String,
    ) -> Self {
        Self {
            connector,
            collection_name,
            geometry_field,
            batch_size: 1000,
        }
    }

    /// Set batch size for bulk inserts.
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Insert a single document.
    pub async fn insert(&self, geometry: &Geometry<f64>, properties: Document) -> Result<ObjectId> {
        let collection = self.connector.collection(&self.collection_name);

        let geom_doc = geometry_to_geojson(geometry)?;
        let mut doc = properties;
        doc.insert(&self.geometry_field, geom_doc);

        let result = collection
            .insert_one(doc)
            .await
            .map_err(|e| Error::MongoDB(e.to_string()))?;

        result
            .inserted_id
            .as_object_id()
            .ok_or_else(|| Error::MongoDB("Failed to get inserted ID".to_string()))
    }

    /// Insert multiple documents in batch.
    pub async fn insert_batch(
        &self,
        features: &[(Geometry<f64>, Document)],
    ) -> Result<Vec<ObjectId>> {
        if features.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_ids = Vec::with_capacity(features.len());

        for chunk in features.chunks(self.batch_size) {
            let mut docs = Vec::with_capacity(chunk.len());

            for (geometry, properties) in chunk {
                let geom_doc = geometry_to_geojson(geometry)?;
                let mut doc = properties.clone();
                doc.insert(&self.geometry_field, geom_doc);
                docs.push(doc);
            }

            let collection = self.connector.collection(&self.collection_name);
            let options = InsertManyOptions::builder().ordered(false).build();

            let result = collection
                .insert_many(docs)
                .with_options(options)
                .await
                .map_err(|e| Error::MongoDB(e.to_string()))?;

            for (_, id) in result.inserted_ids {
                if let Some(oid) = id.as_object_id() {
                    all_ids.push(oid);
                }
            }
        }

        Ok(all_ids)
    }

    /// Update a document by ID.
    pub async fn update(
        &self,
        id: ObjectId,
        geometry: &Geometry<f64>,
        properties: Document,
    ) -> Result<()> {
        let collection = self.connector.collection(&self.collection_name);

        let geom_doc = geometry_to_geojson(geometry)?;
        let mut update_doc = properties;
        update_doc.insert(&self.geometry_field, geom_doc);

        let filter = doc! { "_id": id };
        let update = doc! { "$set": update_doc };

        collection
            .update_one(filter, update)
            .await
            .map_err(|e| Error::MongoDB(e.to_string()))?;

        Ok(())
    }

    /// Upsert a document (insert if not exists, update if exists).
    pub async fn upsert(
        &self,
        filter: Document,
        geometry: &Geometry<f64>,
        properties: Document,
    ) -> Result<ObjectId> {
        let collection = self.connector.collection(&self.collection_name);

        let geom_doc = geometry_to_geojson(geometry)?;
        let mut update_doc = properties;
        update_doc.insert(&self.geometry_field, geom_doc);

        let update = doc! { "$set": update_doc };
        let options = UpdateOptions::builder().upsert(true).build();

        let result = collection
            .update_one(filter, update)
            .with_options(options)
            .await
            .map_err(|e| Error::MongoDB(e.to_string()))?;

        if let Some(upserted_id) = result.upserted_id {
            upserted_id
                .as_object_id()
                .ok_or_else(|| Error::MongoDB("Failed to get upserted ID".to_string()))
        } else {
            Err(Error::MongoDB("No upserted ID returned".to_string()))
        }
    }

    /// Delete a document by ID.
    pub async fn delete(&self, id: ObjectId) -> Result<u64> {
        let collection = self.connector.collection(&self.collection_name);
        let filter = doc! { "_id": id };

        let result = collection
            .delete_one(filter)
            .await
            .map_err(|e| Error::MongoDB(e.to_string()))?;

        Ok(result.deleted_count)
    }

    /// Delete documents matching a filter.
    pub async fn delete_many(&self, filter: Document) -> Result<u64> {
        let collection = self.connector.collection(&self.collection_name);

        let result = collection
            .delete_many(filter)
            .await
            .map_err(|e| Error::MongoDB(e.to_string()))?;

        Ok(result.deleted_count)
    }

    /// Drop the collection.
    pub async fn drop_collection(&self) -> Result<()> {
        self.connector.drop_collection(&self.collection_name).await
    }

    /// Create a 2dsphere index for geospatial queries.
    pub async fn create_geo_index(&self) -> Result<()> {
        self.connector
            .create_geo_index(&self.collection_name, &self.geometry_field)
            .await
    }
}
