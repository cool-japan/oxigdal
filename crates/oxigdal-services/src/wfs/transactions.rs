//! WFS Transaction operations
//!
//! Implements WFS-T (Transactional WFS) for feature insert, update, and delete operations.

use crate::error::{ServiceError, ServiceResult};
use crate::wfs::{FeatureSource, WfsState};
use axum::{
    http::header,
    response::{IntoResponse, Response},
};
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use serde::Deserialize;
use std::io::Cursor;

/// Transaction action types
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum TransactionAction {
    /// Insert new features
    Insert {
        /// Feature type name
        type_name: String,
        /// Features to insert
        features: Box<Vec<geojson::Feature>>,
    },
    /// Update existing features
    Update {
        /// Feature type name
        type_name: String,
        /// Filter to select features
        filter: Option<String>,
        /// Properties to update
        properties: Box<serde_json::Map<String, serde_json::Value>>,
    },
    /// Delete features
    Delete {
        /// Feature type name
        type_name: String,
        /// Filter to select features
        filter: Option<String>,
    },
    /// Replace features
    Replace {
        /// Feature type name
        type_name: String,
        /// Filter to select features
        filter: String,
        /// Replacement feature
        feature: Box<geojson::Feature>,
    },
}

/// Transaction request
#[derive(Debug, Deserialize)]
pub struct Transaction {
    /// Transaction actions
    pub actions: Vec<TransactionAction>,
    /// Release action (ALL or SOME)
    #[serde(default = "default_release_action")]
    pub release_action: String,
    /// Lock ID for locked features
    pub lock_id: Option<String>,
}

fn default_release_action() -> String {
    "ALL".to_string()
}

/// Transaction response
#[derive(Debug)]
pub struct TransactionResponse {
    /// Number of features inserted
    pub total_inserted: usize,
    /// Number of features updated
    pub total_updated: usize,
    /// Number of features deleted
    pub total_deleted: usize,
    /// Number of features replaced
    pub total_replaced: usize,
    /// Inserted feature IDs
    pub inserted_fids: Vec<String>,
}

/// Handle transaction request
pub async fn handle_transaction(
    state: &WfsState,
    _version: &str,
    params: &serde_json::Value,
) -> Result<Response, ServiceError> {
    if !state.transactions_enabled {
        return Err(ServiceError::UnsupportedOperation(
            "Transactions not enabled".to_string(),
        ));
    }

    // Parse transaction from POST body (typically XML)
    // For simplicity, we'll accept JSON as well
    let transaction: Transaction = serde_json::from_value(params.clone())
        .map_err(|e| ServiceError::InvalidParameter("Transaction".to_string(), e.to_string()))?;

    // Execute transaction
    let response = execute_transaction(state, transaction).await?;

    // Generate response
    generate_transaction_response(&response)
}

/// Execute transaction actions
async fn execute_transaction(
    state: &WfsState,
    transaction: Transaction,
) -> ServiceResult<TransactionResponse> {
    let mut total_inserted = 0;
    let mut total_updated = 0;
    let mut total_deleted = 0;
    let mut total_replaced = 0;
    let mut inserted_fids = Vec::new();

    for action in transaction.actions {
        match action {
            TransactionAction::Insert {
                type_name,
                features,
            } => {
                let result = insert_features(state, &type_name, *features).await?;
                total_inserted += result.len();
                inserted_fids.extend(result);
            }
            TransactionAction::Update {
                type_name,
                filter,
                properties,
            } => {
                let count =
                    update_features(state, &type_name, filter.as_deref(), *properties).await?;
                total_updated += count;
            }
            TransactionAction::Delete { type_name, filter } => {
                let count = delete_features(state, &type_name, filter.as_deref()).await?;
                total_deleted += count;
            }
            TransactionAction::Replace {
                type_name,
                filter,
                feature,
            } => {
                let count = replace_features(state, &type_name, &filter, *feature).await?;
                total_replaced += count;
            }
        }
    }

    Ok(TransactionResponse {
        total_inserted,
        total_updated,
        total_deleted,
        total_replaced,
        inserted_fids,
    })
}

/// Insert features
async fn insert_features(
    state: &WfsState,
    type_name: &str,
    features: Vec<geojson::Feature>,
) -> ServiceResult<Vec<String>> {
    let ft = state
        .get_feature_type(type_name)
        .ok_or_else(|| ServiceError::NotFound(format!("Feature type: {}", type_name)))?;

    // Generate feature IDs
    let mut fids = Vec::new();
    for _ in 0..features.len() {
        let fid = format!("{}.{}", type_name, uuid::Uuid::new_v4());
        fids.push(fid);
    }

    // For now, only support in-memory features
    match &ft.source {
        FeatureSource::Memory(_) => {
            // In a real implementation, we would store the features
            // For now, just return success
            Ok(fids)
        }
        FeatureSource::File(_) => Err(ServiceError::Transaction(
            "File-based transactions not yet implemented".to_string(),
        )),
        FeatureSource::Database(_) => Err(ServiceError::Transaction(
            "Database transactions not yet implemented".to_string(),
        )),
        FeatureSource::DatabaseSource(db) => {
            // Generate INSERT SQL for the database source
            let table = db.qualified_table_name();
            let geom_col = &db.geometry_column;
            let id_col = db.id_column.as_deref().unwrap_or("id");

            // Build column list from first feature's properties
            let columns: Vec<String> = if let Some(first) = features.first() {
                let mut cols = vec![id_col.to_string(), geom_col.clone()];
                if let Some(props) = &first.properties {
                    cols.extend(props.keys().cloned());
                }
                cols
            } else {
                return Ok(fids); // No features to insert
            };

            // Generate INSERT statements for each feature
            let _insert_sql = format!(
                "INSERT INTO {} ({}) VALUES ({})",
                table,
                columns
                    .iter()
                    .map(|c| format!("\"{}\"", c))
                    .collect::<Vec<_>>()
                    .join(", "),
                columns.iter().map(|_| "?").collect::<Vec<_>>().join(", ")
            );

            // Database execution requires an actual connection
            // Return error until oxigdal-postgis or oxigdal-db-connectors is integrated
            Err(ServiceError::Transaction(format!(
                "Database insert requires connection. Use oxigdal-postgis for PostGIS. Table: {}, Features: {}",
                table,
                features.len()
            )))
        }
    }
}

/// Update features
async fn update_features(
    state: &WfsState,
    type_name: &str,
    filter: Option<&str>,
    properties: serde_json::Map<String, serde_json::Value>,
) -> ServiceResult<usize> {
    let ft = state
        .get_feature_type(type_name)
        .ok_or_else(|| ServiceError::NotFound(format!("Feature type: {}", type_name)))?;

    match &ft.source {
        FeatureSource::Memory(_) => {
            // In a real implementation, we would update the features
            Ok(0)
        }
        FeatureSource::File(_) => Err(ServiceError::Transaction(
            "File-based transactions not yet implemented".to_string(),
        )),
        FeatureSource::Database(_) => Err(ServiceError::Transaction(
            "Database transactions not yet implemented".to_string(),
        )),
        FeatureSource::DatabaseSource(db) => {
            // Generate UPDATE SQL for the database source
            let table = db.qualified_table_name();

            // Build SET clause from properties
            let set_clauses: Vec<String> = properties
                .keys()
                .map(|k| format!("\"{}\" = ?", k))
                .collect();

            if set_clauses.is_empty() {
                return Ok(0); // No properties to update
            }

            // Build WHERE clause from filter
            let where_clause = filter.map(|f| format!(" WHERE {}", f)).unwrap_or_default();

            let _update_sql = format!(
                "UPDATE {} SET {}{}",
                table,
                set_clauses.join(", "),
                where_clause
            );

            // Database execution requires an actual connection
            Err(ServiceError::Transaction(format!(
                "Database update requires connection. Use oxigdal-postgis for PostGIS. Table: {}, Properties: {}",
                table,
                properties.len()
            )))
        }
    }
}

/// Delete features
async fn delete_features(
    state: &WfsState,
    type_name: &str,
    filter: Option<&str>,
) -> ServiceResult<usize> {
    let ft = state
        .get_feature_type(type_name)
        .ok_or_else(|| ServiceError::NotFound(format!("Feature type: {}", type_name)))?;

    match &ft.source {
        FeatureSource::Memory(_) => {
            // In a real implementation, we would delete the features
            Ok(0)
        }
        FeatureSource::File(_) => Err(ServiceError::Transaction(
            "File-based transactions not yet implemented".to_string(),
        )),
        FeatureSource::Database(_) => Err(ServiceError::Transaction(
            "Database transactions not yet implemented".to_string(),
        )),
        FeatureSource::DatabaseSource(db) => {
            // Generate DELETE SQL for the database source
            let table = db.qualified_table_name();

            // Build WHERE clause from filter (required for safe deletes)
            let where_clause = match filter {
                Some(f) => format!(" WHERE {}", f),
                None => {
                    // Disallow delete without filter for safety
                    return Err(ServiceError::Transaction(
                        "Delete operation requires a filter for database sources".to_string(),
                    ));
                }
            };

            let _delete_sql = format!("DELETE FROM {}{}", table, where_clause);

            // Database execution requires an actual connection
            Err(ServiceError::Transaction(format!(
                "Database delete requires connection. Use oxigdal-postgis for PostGIS. Table: {}",
                table
            )))
        }
    }
}

/// Replace features
async fn replace_features(
    state: &WfsState,
    type_name: &str,
    filter: &str,
    feature: geojson::Feature,
) -> ServiceResult<usize> {
    let ft = state
        .get_feature_type(type_name)
        .ok_or_else(|| ServiceError::NotFound(format!("Feature type: {}", type_name)))?;

    match &ft.source {
        FeatureSource::Memory(_) => {
            // In a real implementation, we would replace the features
            Ok(0)
        }
        FeatureSource::File(_) => Err(ServiceError::Transaction(
            "File-based transactions not yet implemented".to_string(),
        )),
        FeatureSource::Database(_) => Err(ServiceError::Transaction(
            "Database transactions not yet implemented".to_string(),
        )),
        FeatureSource::DatabaseSource(db) => {
            // Replace is implemented as UPDATE with all columns from the feature
            let table = db.qualified_table_name();
            let geom_col = &db.geometry_column;

            // Build SET clause from feature properties
            let mut set_clauses = Vec::new();

            // Add geometry column update
            if feature.geometry.is_some() {
                set_clauses.push(format!("\"{}\" = ?", geom_col));
            }

            // Add property updates
            if let Some(props) = &feature.properties {
                for key in props.keys() {
                    set_clauses.push(format!("\"{}\" = ?", key));
                }
            }

            if set_clauses.is_empty() {
                return Ok(0); // Nothing to replace
            }

            let _replace_sql = format!(
                "UPDATE {} SET {} WHERE {}",
                table,
                set_clauses.join(", "),
                filter
            );

            // Database execution requires an actual connection
            Err(ServiceError::Transaction(format!(
                "Database replace requires connection. Use oxigdal-postgis for PostGIS. Table: {}",
                table
            )))
        }
    }
}

/// Generate transaction response XML
fn generate_transaction_response(response: &TransactionResponse) -> Result<Response, ServiceError> {
    use quick_xml::{
        Writer,
        events::{BytesDecl, BytesEnd, BytesStart, Event},
    };
    use std::io::Cursor;

    let mut writer = Writer::new(Cursor::new(Vec::new()));

    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    let mut root = BytesStart::new("wfs:TransactionResponse");
    root.push_attribute(("version", "2.0.0"));
    root.push_attribute(("xmlns:wfs", "http://www.opengis.net/wfs/2.0"));
    root.push_attribute(("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance"));

    writer
        .write_event(Event::Start(root))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    // TransactionSummary
    writer
        .write_event(Event::Start(BytesStart::new("wfs:TransactionSummary")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    write_text_element(
        &mut writer,
        "wfs:totalInserted",
        &response.total_inserted.to_string(),
    )?;
    write_text_element(
        &mut writer,
        "wfs:totalUpdated",
        &response.total_updated.to_string(),
    )?;
    write_text_element(
        &mut writer,
        "wfs:totalDeleted",
        &response.total_deleted.to_string(),
    )?;

    writer
        .write_event(Event::End(BytesEnd::new("wfs:TransactionSummary")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    // InsertResults
    if !response.inserted_fids.is_empty() {
        writer
            .write_event(Event::Start(BytesStart::new("wfs:InsertResults")))
            .map_err(|e| ServiceError::Xml(e.to_string()))?;

        for fid in &response.inserted_fids {
            writer
                .write_event(Event::Start(BytesStart::new("wfs:Feature")))
                .map_err(|e| ServiceError::Xml(e.to_string()))?;

            write_text_element(&mut writer, "wfs:FeatureId", fid)?;

            writer
                .write_event(Event::End(BytesEnd::new("wfs:Feature")))
                .map_err(|e| ServiceError::Xml(e.to_string()))?;
        }

        writer
            .write_event(Event::End(BytesEnd::new("wfs:InsertResults")))
            .map_err(|e| ServiceError::Xml(e.to_string()))?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("wfs:TransactionResponse")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    let xml = String::from_utf8(writer.into_inner().into_inner())
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(([(header::CONTENT_TYPE, "application/xml")], xml).into_response())
}

/// Helper to write simple text element
fn write_text_element(
    writer: &mut quick_xml::Writer<Cursor<Vec<u8>>>,
    tag: &str,
    text: &str,
) -> ServiceResult<()> {
    writer
        .write_event(Event::Start(BytesStart::new(tag)))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    writer
        .write_event(Event::Text(BytesText::new(text)))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    writer
        .write_event(Event::End(BytesEnd::new(tag)))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_response_generation() -> Result<(), Box<dyn std::error::Error>> {
        let response = TransactionResponse {
            total_inserted: 5,
            total_updated: 3,
            total_deleted: 2,
            total_replaced: 1,
            inserted_fids: vec!["layer.123".to_string(), "layer.456".to_string()],
        };

        let result = generate_transaction_response(&response)?;

        let (parts, _) = result.into_parts();
        assert_eq!(
            parts
                .headers
                .get(header::CONTENT_TYPE)
                .and_then(|h| h.to_str().ok()),
            Some("application/xml")
        );
        Ok(())
    }
}
