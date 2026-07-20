//! WFS Transaction operations
//!
//! Implements WFS-T (Transactional WFS) for feature insert, update, and delete operations.

use crate::error::{ServiceError, ServiceResult};
use crate::wfs::database::{CqlFilter, DatabaseSource};
use crate::wfs::features::{create_legacy_database_source, feature_matches_filter};
use crate::wfs::{FeatureSource, WfsState};
use axum::{
    http::header,
    response::{IntoResponse, Response},
};
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use serde::Deserialize;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
///
/// WFS feature locking (`LockFeature`/`GetFeatureWithLock`) is not
/// implemented by this server: there is no per-feature lock table and no
/// mechanism to validate that a caller actually holds the lock it claims.
/// Rather than silently discarding a client-supplied `lockId` (which would
/// let a `Transaction` mutate/delete features regardless of a lock another
/// session believes it holds), reject the transaction outright. This matches
/// the WFS-T spec's expectation that a server unable to honor a lock returns
/// an exception rather than proceeding as if the lock were valid.
///
/// `release_action` is accepted (for XML/request compatibility) but has no
/// effect beyond this check, since locking itself is unsupported.
async fn execute_transaction(
    state: &WfsState,
    transaction: Transaction,
) -> ServiceResult<TransactionResponse> {
    if transaction.lock_id.is_some() {
        return Err(ServiceError::UnsupportedOperation(
            "WFS feature locking (lockId) is not supported by this server".to_string(),
        ));
    }

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

/// Dispatch target for a non-memory feature source.
///
/// Memory sources are handled inline (they mutate the live `DashMap` entry),
/// so this enum only covers the cases that require dropping the map guard
/// before performing asynchronous I/O.
enum SourceDispatch {
    /// File-backed source at the given path.
    File(PathBuf),
    /// Database-backed source.
    Database(DatabaseSource),
}

/// Returns whether a feature satisfies a transaction filter.
///
/// A missing filter matches every feature (WFS-T applies the action to all
/// features of the type); a present filter is evaluated with the shared simple
/// CQL matcher.
fn filter_matches(feature: &geojson::Feature, filter: Option<&str>) -> bool {
    match filter {
        None => true,
        Some(expr) => feature_matches_filter(feature, expr),
    }
}

/// Insert features
async fn insert_features(
    state: &WfsState,
    type_name: &str,
    features: Vec<geojson::Feature>,
) -> ServiceResult<Vec<String>> {
    // Generate feature IDs up-front so the response is deterministic regardless
    // of the backing store.
    let mut fids = Vec::with_capacity(features.len());
    for _ in 0..features.len() {
        fids.push(format!("{}.{}", type_name, uuid::Uuid::new_v4()));
    }

    let dispatch = {
        let mut entry = state
            .feature_types
            .get_mut(type_name)
            .ok_or_else(|| ServiceError::NotFound(format!("Feature type: {}", type_name)))?;

        match &mut entry.source {
            FeatureSource::Memory(feats) => {
                for (feature, fid) in features.into_iter().zip(fids.iter()) {
                    let mut feature = feature;
                    if feature.id.is_none() {
                        feature.id = Some(geojson::feature::Id::String(fid.clone()));
                    }
                    feats.push(feature);
                }
                return Ok(fids);
            }
            FeatureSource::File(path) => SourceDispatch::File(path.clone()),
            FeatureSource::Database(conn) => {
                SourceDispatch::Database(create_legacy_database_source(conn, type_name))
            }
            FeatureSource::DatabaseSource(db) => SourceDispatch::Database(db.clone()),
        }
    };

    match dispatch {
        SourceDispatch::File(path) => file_insert(&path, features, &fids).await,
        SourceDispatch::Database(db) => database_insert(&db, features, &fids).await,
    }
}

/// Update features
async fn update_features(
    state: &WfsState,
    type_name: &str,
    filter: Option<&str>,
    properties: serde_json::Map<String, serde_json::Value>,
) -> ServiceResult<usize> {
    let dispatch = {
        let mut entry = state
            .feature_types
            .get_mut(type_name)
            .ok_or_else(|| ServiceError::NotFound(format!("Feature type: {}", type_name)))?;

        match &mut entry.source {
            FeatureSource::Memory(feats) => {
                let mut count = 0;
                for feature in feats.iter_mut() {
                    if filter_matches(feature, filter) {
                        let props = feature.properties.get_or_insert_with(serde_json::Map::new);
                        for (key, value) in &properties {
                            props.insert(key.clone(), value.clone());
                        }
                        count += 1;
                    }
                }
                return Ok(count);
            }
            FeatureSource::File(path) => SourceDispatch::File(path.clone()),
            FeatureSource::Database(conn) => {
                SourceDispatch::Database(create_legacy_database_source(conn, type_name))
            }
            FeatureSource::DatabaseSource(db) => SourceDispatch::Database(db.clone()),
        }
    };

    match dispatch {
        SourceDispatch::File(path) => file_update(&path, filter, &properties).await,
        SourceDispatch::Database(db) => database_update(&db, filter, &properties).await,
    }
}

/// Delete features
async fn delete_features(
    state: &WfsState,
    type_name: &str,
    filter: Option<&str>,
) -> ServiceResult<usize> {
    let dispatch = {
        let mut entry = state
            .feature_types
            .get_mut(type_name)
            .ok_or_else(|| ServiceError::NotFound(format!("Feature type: {}", type_name)))?;

        match &mut entry.source {
            FeatureSource::Memory(feats) => {
                let before = feats.len();
                feats.retain(|feature| !filter_matches(feature, filter));
                return Ok(before - feats.len());
            }
            FeatureSource::File(path) => SourceDispatch::File(path.clone()),
            FeatureSource::Database(conn) => {
                SourceDispatch::Database(create_legacy_database_source(conn, type_name))
            }
            FeatureSource::DatabaseSource(db) => SourceDispatch::Database(db.clone()),
        }
    };

    match dispatch {
        SourceDispatch::File(path) => file_delete(&path, filter).await,
        SourceDispatch::Database(db) => database_delete(&db, filter).await,
    }
}

/// Replace features
async fn replace_features(
    state: &WfsState,
    type_name: &str,
    filter: &str,
    feature: geojson::Feature,
) -> ServiceResult<usize> {
    let dispatch = {
        let mut entry = state
            .feature_types
            .get_mut(type_name)
            .ok_or_else(|| ServiceError::NotFound(format!("Feature type: {}", type_name)))?;

        match &mut entry.source {
            FeatureSource::Memory(feats) => {
                let mut count = 0;
                for existing in feats.iter_mut() {
                    if feature_matches_filter(existing, filter) {
                        let mut replacement = feature.clone();
                        // Preserve the resource identity across a replace.
                        if replacement.id.is_none() {
                            replacement.id = existing.id.clone();
                        }
                        *existing = replacement;
                        count += 1;
                    }
                }
                return Ok(count);
            }
            FeatureSource::File(path) => SourceDispatch::File(path.clone()),
            FeatureSource::Database(conn) => {
                SourceDispatch::Database(create_legacy_database_source(conn, type_name))
            }
            FeatureSource::DatabaseSource(db) => SourceDispatch::Database(db.clone()),
        }
    };

    match dispatch {
        SourceDispatch::File(path) => file_replace(&path, filter, &feature).await,
        SourceDispatch::Database(db) => database_replace(&db, filter, &feature).await,
    }
}

// ---------------------------------------------------------------------------
// File-backed transactions
// ---------------------------------------------------------------------------

/// Per-path write locks that serialize concurrent transactions against the same
/// GeoJSON file, preventing torn read-modify-write cycles.
static FILE_LOCKS: std::sync::OnceLock<dashmap::DashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>> =
    std::sync::OnceLock::new();

/// Returns the write mutex associated with `path`, creating it on first use.
fn file_lock_for(path: &Path) -> Arc<tokio::sync::Mutex<()>> {
    let locks = FILE_LOCKS.get_or_init(dashmap::DashMap::new);
    locks
        .entry(path.to_path_buf())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

/// Serialize a feature collection back to a GeoJSON file.
fn write_features_to_file(path: &Path, features: Vec<geojson::Feature>) -> ServiceResult<()> {
    let collection = geojson::FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    };
    let json = serde_json::to_string_pretty(&collection)
        .map_err(|e| ServiceError::Serialization(e.to_string()))?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Insert features into a file-backed source.
async fn file_insert(
    path: &Path,
    features: Vec<geojson::Feature>,
    fids: &[String],
) -> ServiceResult<Vec<String>> {
    let lock = file_lock_for(path);
    let _guard = lock.lock().await;

    let mut existing = crate::wfs::features::load_features_from_file(path)?;
    for (feature, fid) in features.into_iter().zip(fids.iter()) {
        let mut feature = feature;
        if feature.id.is_none() {
            feature.id = Some(geojson::feature::Id::String(fid.clone()));
        }
        existing.push(feature);
    }
    write_features_to_file(path, existing)?;
    Ok(fids.to_vec())
}

/// Update matching features in a file-backed source.
async fn file_update(
    path: &Path,
    filter: Option<&str>,
    properties: &serde_json::Map<String, serde_json::Value>,
) -> ServiceResult<usize> {
    let lock = file_lock_for(path);
    let _guard = lock.lock().await;

    let mut features = crate::wfs::features::load_features_from_file(path)?;
    let mut count = 0;
    for feature in features.iter_mut() {
        if filter_matches(feature, filter) {
            let props = feature.properties.get_or_insert_with(serde_json::Map::new);
            for (key, value) in properties {
                props.insert(key.clone(), value.clone());
            }
            count += 1;
        }
    }
    write_features_to_file(path, features)?;
    Ok(count)
}

/// Delete matching features from a file-backed source.
async fn file_delete(path: &Path, filter: Option<&str>) -> ServiceResult<usize> {
    let lock = file_lock_for(path);
    let _guard = lock.lock().await;

    let mut features = crate::wfs::features::load_features_from_file(path)?;
    let before = features.len();
    features.retain(|feature| !filter_matches(feature, filter));
    let removed = before - features.len();
    write_features_to_file(path, features)?;
    Ok(removed)
}

/// Replace matching features in a file-backed source.
async fn file_replace(
    path: &Path,
    filter: &str,
    feature: &geojson::Feature,
) -> ServiceResult<usize> {
    let lock = file_lock_for(path);
    let _guard = lock.lock().await;

    let mut features = crate::wfs::features::load_features_from_file(path)?;
    let mut count = 0;
    for existing in features.iter_mut() {
        if feature_matches_filter(existing, filter) {
            let mut replacement = feature.clone();
            if replacement.id.is_none() {
                replacement.id = existing.id.clone();
            }
            *existing = replacement;
            count += 1;
        }
    }
    write_features_to_file(path, features)?;
    Ok(count)
}

// ---------------------------------------------------------------------------
// Database-backed transactions (PostGIS)
// ---------------------------------------------------------------------------

/// Build the parameterized `INSERT` statement for a single feature.
///
/// Columns are `[id, geometry, <property keys…>]`; the geometry is bound through
/// `ST_GeomFromGeoJSON($n)` (or `NULL` when absent). Placeholders are numbered
/// so they align with the value list produced by [`database_insert`]: `$1` is
/// the id, `$2` the geometry (if present), and the remaining `$n` follow the
/// feature's property iteration order. Pure and testable without a live DB.
#[cfg_attr(not(feature = "postgis"), allow(dead_code))]
fn build_insert_sql(db: &DatabaseSource, feature: &geojson::Feature) -> String {
    let geom_col = db.geometry_column.replace('"', "\"\"");
    let id_col = db.id_column.as_deref().unwrap_or("id").replace('"', "\"\"");

    let mut columns: Vec<String> = vec![format!("\"{id_col}\""), format!("\"{geom_col}\"")];
    let mut placeholders: Vec<String> = Vec::new();
    let mut idx = 0;

    // Feature id -> $1.
    idx += 1;
    placeholders.push(format!("${idx}"));

    // Geometry -> ST_GeomFromGeoJSON($2) or NULL.
    if feature.geometry.is_some() {
        idx += 1;
        placeholders.push(format!("ST_GeomFromGeoJSON(${idx})"));
    } else {
        placeholders.push("NULL".to_string());
    }

    // Attribute columns.
    if let Some(props) = &feature.properties {
        for key in props.keys() {
            columns.push(format!("\"{}\"", key.replace('"', "\"\"")));
            idx += 1;
            placeholders.push(format!("${idx}"));
        }
    }

    format!(
        "INSERT INTO {} ({}) VALUES ({})",
        db.qualified_table_name(),
        columns.join(", "),
        placeholders.join(", ")
    )
}

/// Build the parameterized `UPDATE` statement for an attribute update.
///
/// Returns `None` when there is nothing to set. Placeholders `$1..$n` follow the
/// order of `keys`; the WHERE clause (if any) is translated from CQL.
#[cfg_attr(not(feature = "postgis"), allow(dead_code))]
fn build_update_sql(
    db: &DatabaseSource,
    keys: &[&str],
    filter: Option<&str>,
) -> ServiceResult<Option<String>> {
    if keys.is_empty() {
        return Ok(None);
    }
    let set: Vec<String> = keys
        .iter()
        .enumerate()
        .map(|(i, k)| format!("\"{}\" = ${}", k.replace('"', "\"\""), i + 1))
        .collect();
    let where_clause = match filter {
        Some(f) => format!(" WHERE {}", CqlFilter::new(f).to_sql(&db.database_type)?),
        None => String::new(),
    };
    Ok(Some(format!(
        "UPDATE {} SET {}{}",
        db.qualified_table_name(),
        set.join(", "),
        where_clause
    )))
}

/// Build the `DELETE` statement for a database source. A filter is required.
fn build_delete_sql(db: &DatabaseSource, filter: Option<&str>) -> ServiceResult<String> {
    let where_sql = match filter {
        Some(f) => CqlFilter::new(f).to_sql(&db.database_type)?,
        None => {
            return Err(ServiceError::Transaction(
                "Delete operation requires a filter for database sources".to_string(),
            ));
        }
    };
    Ok(format!(
        "DELETE FROM {} WHERE {}",
        db.qualified_table_name(),
        where_sql
    ))
}

/// Build the parameterized `UPDATE` statement for a full-feature replace.
///
/// When `has_geometry` is true the geometry is set first through
/// `ST_GeomFromGeoJSON($1)`; property placeholders follow in `keys` order.
/// Returns `None` when nothing would be set.
#[cfg_attr(not(feature = "postgis"), allow(dead_code))]
fn build_replace_sql(
    db: &DatabaseSource,
    has_geometry: bool,
    keys: &[&str],
    filter: &str,
) -> ServiceResult<Option<String>> {
    let mut set: Vec<String> = Vec::new();
    let mut idx = 0;
    if has_geometry {
        idx += 1;
        set.push(format!(
            "\"{}\" = ST_GeomFromGeoJSON(${})",
            db.geometry_column.replace('"', "\"\""),
            idx
        ));
    }
    for key in keys {
        idx += 1;
        set.push(format!("\"{}\" = ${}", key.replace('"', "\"\""), idx));
    }
    if set.is_empty() {
        return Ok(None);
    }
    let where_sql = CqlFilter::new(filter).to_sql(&db.database_type)?;
    Ok(Some(format!(
        "UPDATE {} SET {} WHERE {}",
        db.qualified_table_name(),
        set.join(", "),
        where_sql
    )))
}

/// Convert a JSON property value into a boxed PostgreSQL parameter.
#[cfg(feature = "postgis")]
fn json_to_sql_param(
    value: &serde_json::Value,
) -> Box<dyn tokio_postgres::types::ToSql + Sync + Send> {
    match value {
        serde_json::Value::Null => Box::new(Option::<String>::None),
        serde_json::Value::Bool(b) => Box::new(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Box::new(i)
            } else if let Some(f) = n.as_f64() {
                Box::new(f)
            } else {
                Box::new(n.to_string())
            }
        }
        serde_json::Value::String(s) => Box::new(s.clone()),
        other => Box::new(other.to_string()),
    }
}

/// Serialize a geometry to its GeoJSON string representation.
#[cfg(feature = "postgis")]
fn geometry_to_geojson_string(geometry: &geojson::Geometry) -> ServiceResult<String> {
    serde_json::to_string(geometry).map_err(|e| ServiceError::Serialization(e.to_string()))
}

/// Acquire a pooled PostGIS client for the given source.
#[cfg(feature = "postgis")]
async fn database_client(db: &DatabaseSource) -> ServiceResult<deadpool_postgres::Object> {
    let pool = oxigeo_postgis::ConnectionPool::from_connection_string(&db.connection_string)
        .map_err(|e| ServiceError::Transaction(format!("PostGIS connection setup failed: {e}")))?;
    pool.get()
        .await
        .map_err(|e| ServiceError::Transaction(format!("PostGIS pool error: {e}")))
}

/// Error returned when a database transaction is requested but the `postgis`
/// feature was not compiled in.
#[cfg(not(feature = "postgis"))]
fn postgis_unavailable(operation: &str) -> ServiceError {
    ServiceError::Transaction(format!(
        "Database transaction '{operation}' is unavailable: oxigeo-services was built \
         without the 'postgis' feature. Rebuild with `--features postgis` to enable \
         PostGIS-backed WFS-T operations."
    ))
}

/// Insert features into a database-backed source.
#[cfg_attr(
    not(feature = "postgis"),
    allow(unused_variables, clippy::unused_async)
)]
async fn database_insert(
    db: &DatabaseSource,
    features: Vec<geojson::Feature>,
    fids: &[String],
) -> ServiceResult<Vec<String>> {
    #[cfg(feature = "postgis")]
    {
        use tokio_postgres::types::ToSql;

        let client = database_client(db).await?;

        for (feature, fid) in features.iter().zip(fids.iter()) {
            let sql = build_insert_sql(db, feature);
            let mut boxed: Vec<Box<dyn ToSql + Sync + Send>> = Vec::new();
            boxed.push(Box::new(fid.clone()));
            if let Some(geometry) = &feature.geometry {
                boxed.push(Box::new(geometry_to_geojson_string(geometry)?));
            }
            if let Some(props) = &feature.properties {
                for value in props.values() {
                    boxed.push(json_to_sql_param(value));
                }
            }

            let params: Vec<&(dyn ToSql + Sync)> = boxed
                .iter()
                .map(|b| -> &(dyn ToSql + Sync) { b.as_ref() })
                .collect();
            client
                .execute(&sql, &params)
                .await
                .map_err(|e| ServiceError::Transaction(format!("Insert failed: {e}")))?;
        }

        Ok(fids.to_vec())
    }

    #[cfg(not(feature = "postgis"))]
    {
        Err(postgis_unavailable("Insert"))
    }
}

/// Update matching features in a database-backed source.
#[cfg_attr(
    not(feature = "postgis"),
    allow(unused_variables, clippy::unused_async)
)]
async fn database_update(
    db: &DatabaseSource,
    filter: Option<&str>,
    properties: &serde_json::Map<String, serde_json::Value>,
) -> ServiceResult<usize> {
    #[cfg(feature = "postgis")]
    {
        use tokio_postgres::types::ToSql;

        let keys: Vec<&str> = properties.keys().map(String::as_str).collect();
        let sql = match build_update_sql(db, &keys, filter)? {
            Some(sql) => sql,
            None => return Ok(0),
        };

        let boxed: Vec<Box<dyn ToSql + Sync + Send>> =
            properties.values().map(json_to_sql_param).collect();
        let params: Vec<&(dyn ToSql + Sync)> = boxed
            .iter()
            .map(|b| -> &(dyn ToSql + Sync) { b.as_ref() })
            .collect();

        let client = database_client(db).await?;
        let affected = client
            .execute(&sql, &params)
            .await
            .map_err(|e| ServiceError::Transaction(format!("Update failed: {e}")))?;
        Ok(affected as usize)
    }

    #[cfg(not(feature = "postgis"))]
    {
        Err(postgis_unavailable("Update"))
    }
}

/// Delete matching features from a database-backed source.
#[cfg_attr(
    not(feature = "postgis"),
    allow(unused_variables, clippy::unused_async)
)]
async fn database_delete(db: &DatabaseSource, filter: Option<&str>) -> ServiceResult<usize> {
    #[cfg(feature = "postgis")]
    {
        let sql = build_delete_sql(db, filter)?;
        let client = database_client(db).await?;
        let affected = client
            .execute(&sql, &[])
            .await
            .map_err(|e| ServiceError::Transaction(format!("Delete failed: {e}")))?;
        Ok(affected as usize)
    }

    #[cfg(not(feature = "postgis"))]
    {
        // Preserve the "filter required" contract even without a live backend.
        let _ = build_delete_sql(db, filter)?;
        Err(postgis_unavailable("Delete"))
    }
}

/// Replace matching features in a database-backed source.
#[cfg_attr(
    not(feature = "postgis"),
    allow(unused_variables, clippy::unused_async)
)]
async fn database_replace(
    db: &DatabaseSource,
    filter: &str,
    feature: &geojson::Feature,
) -> ServiceResult<usize> {
    #[cfg(feature = "postgis")]
    {
        use tokio_postgres::types::ToSql;

        let keys: Vec<&str> = feature
            .properties
            .as_ref()
            .map(|p| p.keys().map(String::as_str).collect())
            .unwrap_or_default();
        let has_geometry = feature.geometry.is_some();
        let sql = match build_replace_sql(db, has_geometry, &keys, filter)? {
            Some(sql) => sql,
            None => return Ok(0),
        };

        let mut boxed: Vec<Box<dyn ToSql + Sync + Send>> = Vec::new();
        if let Some(geometry) = &feature.geometry {
            boxed.push(Box::new(geometry_to_geojson_string(geometry)?));
        }
        if let Some(props) = &feature.properties {
            for value in props.values() {
                boxed.push(json_to_sql_param(value));
            }
        }
        let params: Vec<&(dyn ToSql + Sync)> = boxed
            .iter()
            .map(|b| -> &(dyn ToSql + Sync) { b.as_ref() })
            .collect();

        let client = database_client(db).await?;
        let affected = client
            .execute(&sql, &params)
            .await
            .map_err(|e| ServiceError::Transaction(format!("Replace failed: {e}")))?;
        Ok(affected as usize)
    }

    #[cfg(not(feature = "postgis"))]
    {
        Err(postgis_unavailable("Replace"))
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

    use crate::wfs::{FeatureTypeInfo, ServiceInfo};

    fn service_info() -> ServiceInfo {
        ServiceInfo {
            title: "Test WFS".to_string(),
            abstract_text: None,
            provider: "COOLJAPAN OU".to_string(),
            service_url: "http://localhost/wfs".to_string(),
            versions: vec!["2.0.0".to_string()],
        }
    }

    fn sample_feature(name: &str) -> geojson::Feature {
        geojson::Feature {
            bbox: None,
            geometry: Some(geojson::Geometry::new_point([1.0, 2.0])),
            id: None,
            properties: Some(
                [("name".to_string(), serde_json::json!(name))]
                    .into_iter()
                    .collect(),
            ),
            foreign_members: None,
        }
    }

    fn memory_state(name: &str, feats: Vec<geojson::Feature>) -> WfsState {
        let state = WfsState::new(service_info());
        let ft = FeatureTypeInfo {
            name: name.to_string(),
            title: name.to_string(),
            abstract_text: None,
            default_crs: "EPSG:4326".to_string(),
            other_crs: vec![],
            bbox: None,
            source: FeatureSource::Memory(feats),
        };
        state.add_feature_type(ft).expect("add feature type");
        state
    }

    fn file_state(name: &str, path: &Path) -> WfsState {
        let state = WfsState::new(service_info());
        let ft = FeatureTypeInfo {
            name: name.to_string(),
            title: name.to_string(),
            abstract_text: None,
            default_crs: "EPSG:4326".to_string(),
            other_crs: vec![],
            bbox: None,
            source: FeatureSource::File(path.to_path_buf()),
        };
        state.add_feature_type(ft).expect("add feature type");
        state
    }

    fn memory_features(state: &WfsState, name: &str) -> Vec<geojson::Feature> {
        let ft = state.get_feature_type(name).expect("feature type");
        if let FeatureSource::Memory(v) = ft.source {
            v
        } else {
            Vec::new()
        }
    }

    fn feature_name(feature: &geojson::Feature) -> Option<String> {
        feature
            .properties
            .as_ref()
            .and_then(|p| p.get("name"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }

    fn unique_geojson_path() -> PathBuf {
        std::env::temp_dir().join(format!("oxigeo_wfs_tx_{}.geojson", uuid::Uuid::new_v4()))
    }

    fn write_empty_collection(path: &Path) {
        std::fs::write(path, r#"{"type":"FeatureCollection","features":[]}"#)
            .expect("write empty collection");
    }

    // ---- Memory transaction paths ----

    #[tokio::test]
    async fn test_memory_insert_appends_and_assigns_ids() {
        let state = memory_state("layer", Vec::new());
        let feats = vec![sample_feature("a"), sample_feature("b")];
        let fids = insert_features(&state, "layer", feats)
            .await
            .expect("insert");
        assert_eq!(fids.len(), 2);

        let stored = memory_features(&state, "layer");
        assert_eq!(stored.len(), 2);
        for feature in &stored {
            assert!(
                matches!(feature.id, Some(geojson::feature::Id::String(_))),
                "insert should assign a string id"
            );
        }
    }

    #[tokio::test]
    async fn test_memory_update_matching_filter() {
        let state = memory_state("layer", vec![sample_feature("a"), sample_feature("b")]);
        let mut props = serde_json::Map::new();
        props.insert("status".to_string(), serde_json::json!("active"));

        let count = update_features(&state, "layer", Some("name = 'a'"), props)
            .await
            .expect("update");
        assert_eq!(count, 1);

        let stored = memory_features(&state, "layer");
        let updated = stored
            .iter()
            .find(|f| feature_name(f).as_deref() == Some("a"))
            .expect("feature a");
        assert_eq!(
            updated
                .properties
                .as_ref()
                .and_then(|p| p.get("status"))
                .and_then(|v| v.as_str()),
            Some("active")
        );
        let untouched = stored
            .iter()
            .find(|f| feature_name(f).as_deref() == Some("b"))
            .expect("feature b");
        assert!(
            untouched
                .properties
                .as_ref()
                .and_then(|p| p.get("status"))
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_memory_delete_matching_filter() {
        let state = memory_state("layer", vec![sample_feature("a"), sample_feature("b")]);
        let count = delete_features(&state, "layer", Some("name = 'a'"))
            .await
            .expect("delete");
        assert_eq!(count, 1);

        let stored = memory_features(&state, "layer");
        assert_eq!(stored.len(), 1);
        assert_eq!(feature_name(&stored[0]).as_deref(), Some("b"));
    }

    #[tokio::test]
    async fn test_memory_replace_matching_filter() {
        let state = memory_state("layer", vec![sample_feature("a"), sample_feature("b")]);
        let replacement = sample_feature("a-new");
        let count = replace_features(&state, "layer", "name = 'a'", replacement)
            .await
            .expect("replace");
        assert_eq!(count, 1);

        let stored = memory_features(&state, "layer");
        assert!(
            stored
                .iter()
                .any(|f| feature_name(f).as_deref() == Some("a-new"))
        );
        assert!(
            stored
                .iter()
                .any(|f| feature_name(f).as_deref() == Some("b"))
        );
    }

    // ---- File transaction paths ----

    #[tokio::test]
    async fn test_file_insert_update_delete_roundtrip() {
        let path = unique_geojson_path();
        write_empty_collection(&path);
        let state = file_state("layer", &path);

        // Insert two features.
        let fids = insert_features(
            &state,
            "layer",
            vec![sample_feature("a"), sample_feature("b")],
        )
        .await
        .expect("insert");
        assert_eq!(fids.len(), 2);
        let loaded = crate::wfs::features::load_features_from_file(&path).expect("load");
        assert_eq!(loaded.len(), 2);

        // Update feature "a".
        let mut props = serde_json::Map::new();
        props.insert("status".to_string(), serde_json::json!("done"));
        let updated = update_features(&state, "layer", Some("name = 'a'"), props)
            .await
            .expect("update");
        assert_eq!(updated, 1);
        let loaded = crate::wfs::features::load_features_from_file(&path).expect("load");
        let a = loaded
            .iter()
            .find(|f| feature_name(f).as_deref() == Some("a"))
            .expect("feature a");
        assert_eq!(
            a.properties
                .as_ref()
                .and_then(|p| p.get("status"))
                .and_then(|v| v.as_str()),
            Some("done")
        );

        // Delete feature "b".
        let deleted = delete_features(&state, "layer", Some("name = 'b'"))
            .await
            .expect("delete");
        assert_eq!(deleted, 1);
        let loaded = crate::wfs::features::load_features_from_file(&path).expect("load");
        assert_eq!(loaded.len(), 1);
        assert_eq!(feature_name(&loaded[0]).as_deref(), Some("a"));

        std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn test_file_replace_roundtrip() {
        let path = unique_geojson_path();
        write_empty_collection(&path);
        let state = file_state("layer", &path);

        insert_features(&state, "layer", vec![sample_feature("a")])
            .await
            .expect("insert");
        let count = replace_features(&state, "layer", "name = 'a'", sample_feature("a-new"))
            .await
            .expect("replace");
        assert_eq!(count, 1);

        let loaded = crate::wfs::features::load_features_from_file(&path).expect("load");
        assert_eq!(loaded.len(), 1);
        assert_eq!(feature_name(&loaded[0]).as_deref(), Some("a-new"));

        std::fs::remove_file(&path).ok();
    }

    // ---- Locking is rejected, not silently ignored ----

    #[tokio::test]
    async fn test_transaction_with_lock_id_is_rejected() {
        let state = memory_state("layer", vec![sample_feature("a")]);
        let transaction = Transaction {
            actions: vec![TransactionAction::Delete {
                type_name: "layer".to_string(),
                filter: None,
            }],
            release_action: default_release_action(),
            lock_id: Some("some-lock-token".to_string()),
        };

        let result = execute_transaction(&state, transaction).await;
        assert!(
            matches!(result, Err(ServiceError::UnsupportedOperation(_))),
            "expected UnsupportedOperation, got {result:?}"
        );

        // The delete must not have been applied: locking is rejected before
        // any action is dispatched.
        let stored = memory_features(&state, "layer");
        assert_eq!(stored.len(), 1, "transaction with lockId must be a no-op");
    }

    #[tokio::test]
    async fn test_transaction_without_lock_id_still_executes() {
        let state = memory_state("layer", vec![sample_feature("a")]);
        let transaction = Transaction {
            actions: vec![TransactionAction::Delete {
                type_name: "layer".to_string(),
                filter: None,
            }],
            release_action: default_release_action(),
            lock_id: None,
        };

        let response = execute_transaction(&state, transaction)
            .await
            .expect("transaction without lockId should succeed");
        assert_eq!(response.total_deleted, 1);
    }

    // ---- SQL-string assertions for the PostGIS paths ----

    #[test]
    fn test_build_insert_sql() {
        let db = DatabaseSource::new("host=localhost dbname=gis", "roads")
            .with_geometry_column("geom")
            .with_id_column("gid");
        let sql = build_insert_sql(&db, &sample_feature("x"));
        assert!(sql.starts_with("INSERT INTO \"roads\" ("));
        assert!(sql.contains("\"gid\""));
        assert!(sql.contains("\"geom\""));
        assert!(sql.contains("\"name\""));
        assert!(sql.contains("ST_GeomFromGeoJSON($2)"));
        assert!(sql.contains("VALUES ($1, ST_GeomFromGeoJSON($2), $3)"));
    }

    #[test]
    fn test_build_insert_sql_without_geometry() {
        let db = DatabaseSource::new("host=localhost dbname=gis", "roads");
        let mut feature = sample_feature("x");
        feature.geometry = None;
        let sql = build_insert_sql(&db, &feature);
        assert!(sql.contains("VALUES ($1, NULL, $2)"));
    }

    #[test]
    fn test_build_update_sql() {
        let db = DatabaseSource::new("host=localhost dbname=gis", "roads");
        let sql = build_update_sql(&db, &["name", "status"], Some("id = 5"))
            .expect("ok")
            .expect("some");
        assert!(sql.starts_with("UPDATE \"roads\" SET "));
        assert!(sql.contains("\"name\" = $1"));
        assert!(sql.contains("\"status\" = $2"));
        assert!(sql.contains(" WHERE "));
        assert!(sql.contains("\"id\" = 5"));
        assert!(build_update_sql(&db, &[], None).expect("ok").is_none());
    }

    #[test]
    fn test_build_delete_sql_requires_filter() {
        let db = DatabaseSource::new("host=localhost dbname=gis", "roads");
        let sql = build_delete_sql(&db, Some("id = 5")).expect("ok");
        assert!(sql.starts_with("DELETE FROM \"roads\" WHERE "));
        assert!(sql.contains("\"id\" = 5"));
        assert!(build_delete_sql(&db, None).is_err());
    }

    #[test]
    fn test_build_replace_sql() {
        let db =
            DatabaseSource::new("host=localhost dbname=gis", "roads").with_geometry_column("geom");
        let sql = build_replace_sql(&db, true, &["name"], "id = 5")
            .expect("ok")
            .expect("some");
        assert!(sql.contains("\"geom\" = ST_GeomFromGeoJSON($1)"));
        assert!(sql.contains("\"name\" = $2"));
        assert!(sql.contains(" WHERE "));
        assert!(
            build_replace_sql(&db, false, &[], "id = 5")
                .expect("ok")
                .is_none()
        );
    }
}
