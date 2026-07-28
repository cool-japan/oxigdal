//! OGC API - Tiles Part 1: Core — HTTP server.
//!
//! Exposes the [`crate::ogc_tiles`] `TileMatrixSet` / `TileSetMetadata` types
//! over a conformant REST surface:
//!
//! - `GET /`                                                   landing page
//! - `GET /conformance`                                        conformance classes
//! - `GET /tileMatrixSets`                                     list of tile matrix sets
//! - `GET /tileMatrixSets/{tileMatrixSetId}`                   a tile matrix set
//! - `GET /collections/{collectionId}/tiles`                   tilesets of a collection
//! - `GET /collections/{collectionId}/tiles/{tileMatrixSetId}` tileset metadata
//! - `GET /collections/{collectionId}/tiles/{tileMatrixSetId}/{tileMatrix}/{tileRow}/{tileCol}`
//!   the vector tile itself (MVT), built from a registered
//!   [`TileFeatureProvider`].
//!
//! The router is mounted by an HTTP server binary (e.g. `oxigeo-server`) via
//! [`OgcTilesState::router`].

use crate::error::ServiceError;
use crate::mvt::build_tile_from_features;
use crate::ogc_tiles::{
    ConformanceDeclaration, TileDataType, TileLink, TileMatrixSet, TileSetMetadata, tile_to_bbox,
    validate_tile_coords,
};
use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use std::collections::BTreeMap;
use std::sync::Arc;

/// MVT media type.
const MVT_MEDIA_TYPE: &str = "application/vnd.mapbox-vector-tile";

/// Supplies the features that populate a vector tile for a collection.
///
/// An implementation queries the real feature source (a WFS `FeatureSource`, a
/// dataset registry, a database, …) restricted to the tile's geographic
/// bounding box and returns the features to encode.
#[async_trait]
pub trait TileFeatureProvider: Send + Sync {
    /// Return the features of `collection` intersecting the given tile.
    ///
    /// `bbox` is `[west, south, east, north]` in WGS84 degrees.
    async fn features_for_tile(
        &self,
        collection: &str,
        z: u8,
        x: u32,
        y: u32,
        bbox: [f64; 4],
    ) -> Result<Vec<geojson::Feature>, ServiceError>;
}

/// A collection that publishes tiles.
#[derive(Clone)]
struct TiledCollection {
    id: String,
    title: String,
    /// Identifiers of the tile matrix sets this collection is available in.
    tile_matrix_set_ids: Vec<String>,
}

struct OgcTilesInner {
    base_url: String,
    /// Well-known tile matrix sets keyed by identifier.
    tile_matrix_sets: BTreeMap<String, TileMatrixSet>,
    /// Registered tiled collections keyed by identifier.
    collections: dashmap::DashMap<String, TiledCollection>,
    /// Optional provider of tile features (required for the tile data route).
    provider: Option<Arc<dyn TileFeatureProvider>>,
}

/// Shared state for the OGC API - Tiles server.
#[derive(Clone)]
pub struct OgcTilesState {
    inner: Arc<OgcTilesInner>,
}

impl OgcTilesState {
    /// Create a new server state.
    ///
    /// `base_url` is the externally-visible root URL (no trailing slash) used
    /// to build absolute hypermedia links. The well-known `WebMercatorQuad`
    /// and `WorldCRS84Quad` tile matrix sets are registered automatically.
    pub fn new(base_url: impl Into<String>) -> Self {
        let mut tile_matrix_sets = BTreeMap::new();
        let wmq = TileMatrixSet::web_mercator_quad();
        tile_matrix_sets.insert(wmq.id.clone(), wmq);
        let crs84 = TileMatrixSet::world_crs84_quad();
        tile_matrix_sets.insert(crs84.id.clone(), crs84);

        Self {
            inner: Arc::new(OgcTilesInner {
                base_url: base_url.into().trim_end_matches('/').to_string(),
                tile_matrix_sets,
                collections: dashmap::DashMap::new(),
                provider: None,
            }),
        }
    }

    /// Attach a feature provider used to build vector tiles.
    ///
    /// Without a provider the tile-data endpoint returns an honest
    /// `OperationNotSupported` error rather than a fake/empty tile.
    pub fn with_provider(self, provider: Arc<dyn TileFeatureProvider>) -> Self {
        // Rebuild `inner` with the provider set; avoids any `Arc::get_mut`
        // panic path and works regardless of how many handles exist.
        Self {
            inner: Arc::new(OgcTilesInner {
                base_url: self.inner.base_url.clone(),
                tile_matrix_sets: self.inner.tile_matrix_sets.clone(),
                collections: self.inner.collections.clone(),
                provider: Some(provider),
            }),
        }
    }

    /// Register a tiled collection available in the given tile matrix sets.
    pub fn register_collection(
        &self,
        id: impl Into<String>,
        title: impl Into<String>,
        tile_matrix_set_ids: Vec<String>,
    ) {
        let id = id.into();
        self.inner.collections.insert(
            id.clone(),
            TiledCollection {
                id,
                title: title.into(),
                tile_matrix_set_ids,
            },
        );
    }

    /// Build the axum [`Router`] for the OGC API - Tiles endpoints.
    pub fn router(self) -> Router {
        Router::new()
            .route("/", get(landing_page))
            .route("/conformance", get(conformance))
            .route("/tileMatrixSets", get(tile_matrix_sets))
            .route("/tileMatrixSets/{tileMatrixSetId}", get(tile_matrix_set))
            .route("/collections/{collectionId}/tiles", get(collection_tilesets))
            .route(
                "/collections/{collectionId}/tiles/{tileMatrixSetId}",
                get(collection_tileset),
            )
            .route(
                "/collections/{collectionId}/tiles/{tileMatrixSetId}/{tileMatrix}/{tileRow}/{tileCol}",
                get(collection_tile),
            )
            .with_state(self)
    }

    fn base(&self) -> &str {
        &self.inner.base_url
    }
}

/// Landing page (`GET /`).
async fn landing_page(State(state): State<OgcTilesState>) -> Json<serde_json::Value> {
    let base = state.base();
    Json(serde_json::json!({
        "title": "OxiGeo OGC API - Tiles",
        "description": "Vector and map tiles served via OGC API - Tiles Part 1: Core",
        "links": [
            { "href": format!("{base}/"), "rel": "self", "type": "application/json", "title": "This document" },
            { "href": format!("{base}/conformance"), "rel": "http://www.opengis.net/def/rel/ogc/1.0/conformance", "type": "application/json", "title": "Conformance declaration" },
            { "href": format!("{base}/tileMatrixSets"), "rel": "http://www.opengis.net/def/rel/ogc/1.0/tiling-schemes", "type": "application/json", "title": "Tile matrix sets" },
            { "href": format!("{base}/collections"), "rel": "data", "type": "application/json", "title": "Collections" }
        ]
    }))
}

/// Conformance declaration (`GET /conformance`).
async fn conformance() -> Json<ConformanceDeclaration> {
    Json(ConformanceDeclaration::ogc_tiles())
}

/// List of tile matrix sets (`GET /tileMatrixSets`).
async fn tile_matrix_sets(State(state): State<OgcTilesState>) -> Json<serde_json::Value> {
    let base = state.base();
    let items: Vec<serde_json::Value> = state
        .inner
        .tile_matrix_sets
        .values()
        .map(|tms| {
            serde_json::json!({
                "id": tms.id,
                "title": tms.title,
                "uri": tms.uri,
                "crs": tms.crs,
                "links": [
                    {
                        "href": format!("{base}/tileMatrixSets/{}", tms.id),
                        "rel": "self",
                        "type": "application/json",
                        "title": tms.title,
                    }
                ]
            })
        })
        .collect();
    Json(serde_json::json!({ "tileMatrixSets": items }))
}

/// A single tile matrix set (`GET /tileMatrixSets/{id}`).
async fn tile_matrix_set(
    State(state): State<OgcTilesState>,
    Path(tms_id): Path<String>,
) -> Result<Json<TileMatrixSet>, ServiceError> {
    state
        .inner
        .tile_matrix_sets
        .get(&tms_id)
        .cloned()
        .map(Json)
        .ok_or_else(|| ServiceError::NotFound(format!("TileMatrixSet: {tms_id}")))
}

/// Build the tileset metadata for a collection + tile matrix set.
fn build_tileset_metadata(
    state: &OgcTilesState,
    collection: &TiledCollection,
    tms_id: &str,
) -> TileSetMetadata {
    let base = state.base();
    let template = format!(
        "{base}/collections/{}/tiles/{tms_id}/{{tileMatrix}}/{{tileRow}}/{{tileCol}}",
        collection.id
    );
    let tms = state.inner.tile_matrix_sets.get(tms_id);
    let mut metadata = TileSetMetadata::vector_web_mercator(template);
    metadata.tile_matrix_set_id = tms_id.to_string();
    metadata.data_type = TileDataType::Vector;
    metadata.title = Some(collection.title.clone());
    if let Some(tms) = tms {
        metadata.min_tile_matrix = tms.tile_matrices.first().map(|m| m.id.clone());
        metadata.max_tile_matrix = tms.tile_matrices.last().map(|m| m.id.clone());
    }
    metadata.links.push(TileLink {
        href: format!("{base}/tileMatrixSets/{tms_id}"),
        rel: "http://www.opengis.net/def/rel/ogc/1.0/tiling-scheme".to_string(),
        media_type: Some("application/json".to_string()),
        title: Some(format!("{tms_id} tiling scheme")),
    });
    metadata
}

/// Tilesets of a collection (`GET /collections/{id}/tiles`).
async fn collection_tilesets(
    State(state): State<OgcTilesState>,
    Path(collection_id): Path<String>,
) -> Result<Json<serde_json::Value>, ServiceError> {
    let collection = state
        .inner
        .collections
        .get(&collection_id)
        .ok_or_else(|| ServiceError::NotFound(format!("Collection: {collection_id}")))?
        .clone();

    let tilesets: Vec<TileSetMetadata> = collection
        .tile_matrix_set_ids
        .iter()
        .map(|tms_id| build_tileset_metadata(&state, &collection, tms_id))
        .collect();

    Ok(Json(serde_json::json!({ "tilesets": tilesets })))
}

/// Tileset metadata for a collection + TMS (`GET /collections/{id}/tiles/{tmsId}`).
async fn collection_tileset(
    State(state): State<OgcTilesState>,
    Path((collection_id, tms_id)): Path<(String, String)>,
) -> Result<Json<TileSetMetadata>, ServiceError> {
    let collection = state
        .inner
        .collections
        .get(&collection_id)
        .ok_or_else(|| ServiceError::NotFound(format!("Collection: {collection_id}")))?
        .clone();

    if !collection
        .tile_matrix_set_ids
        .iter()
        .any(|id| id == &tms_id)
    {
        return Err(ServiceError::NotFound(format!(
            "Collection {collection_id} has no tileset for {tms_id}"
        )));
    }

    Ok(Json(build_tileset_metadata(&state, &collection, &tms_id)))
}

/// A single vector tile
/// (`GET /collections/{id}/tiles/{tmsId}/{tileMatrix}/{tileRow}/{tileCol}`).
async fn collection_tile(
    State(state): State<OgcTilesState>,
    Path((collection_id, tms_id, tile_matrix, tile_row, tile_col)): Path<(
        String,
        String,
        u8,
        u32,
        u32,
    )>,
) -> Result<Response, ServiceError> {
    let collection = state
        .inner
        .collections
        .get(&collection_id)
        .ok_or_else(|| ServiceError::NotFound(format!("Collection: {collection_id}")))?
        .clone();

    if !collection
        .tile_matrix_set_ids
        .iter()
        .any(|id| id == &tms_id)
    {
        return Err(ServiceError::NotFound(format!(
            "Collection {collection_id} is not tiled with {tms_id}"
        )));
    }

    // The XYZ tile-to-bbox math assumes the WebMercatorQuad grid.
    if tms_id != "WebMercatorQuad" {
        return Err(ServiceError::UnsupportedOperation(format!(
            "vector tiles are only served for WebMercatorQuad, not {tms_id}"
        )));
    }

    // tileRow is Y, tileCol is X in OGC API - Tiles addressing.
    let (z, x, y) = (tile_matrix, tile_col, tile_row);
    if !validate_tile_coords(z, x, y) {
        return Err(ServiceError::InvalidParameter(
            "tile".to_string(),
            format!("tile coordinates z={z} x={x} y={y} are out of range"),
        ));
    }

    let provider = state.inner.provider.as_ref().ok_or_else(|| {
        ServiceError::UnsupportedOperation(
            "no tile feature provider is configured for this server".to_string(),
        )
    })?;

    let bbox = tile_to_bbox(z, x, y);
    let features = provider
        .features_for_tile(&collection_id, z, x, y, bbox)
        .await?;

    let tile = build_tile_from_features(&collection_id, bbox, 4096, &features)?;

    // An empty tile (no features) is legitimately 204 No Content per the spec.
    if tile.is_empty() {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    Ok(([(header::CONTENT_TYPE, MVT_MEDIA_TYPE)], tile).into_response())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::Request;
    use tower::ServiceExt;

    struct StaticProvider;

    #[async_trait]
    impl TileFeatureProvider for StaticProvider {
        async fn features_for_tile(
            &self,
            _collection: &str,
            _z: u8,
            _x: u32,
            _y: u32,
            bbox: [f64; 4],
        ) -> Result<Vec<geojson::Feature>, ServiceError> {
            // Return one polygon covering the tile bbox.
            let [w, s, e, n] = bbox;
            let feature: geojson::Feature = serde_json::from_value(serde_json::json!({
                "type": "Feature",
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [[[w, s], [e, s], [e, n], [w, n], [w, s]]]
                },
                "properties": { "name": "cell" }
            }))
            .unwrap();
            Ok(vec![feature])
        }
    }

    fn state_with_collection() -> OgcTilesState {
        let state = OgcTilesState::new("http://localhost:8080/tiles")
            .with_provider(Arc::new(StaticProvider));
        state.register_collection("parcels", "Parcels", vec!["WebMercatorQuad".to_string()]);
        state
    }

    async fn get(state: OgcTilesState, uri: &str) -> Response {
        let router = state.router();
        router
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn body_string(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn landing_page_lists_links() {
        let resp = get(state_with_collection(), "/").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("conformance"));
        assert!(body.contains("tileMatrixSets"));
    }

    #[tokio::test]
    async fn conformance_lists_core_class() {
        let resp = get(state_with_collection(), "/conformance").await;
        let body = body_string(resp).await;
        assert!(body.contains("ogcapi-tiles-1/1.0/conf/core"));
    }

    #[tokio::test]
    async fn tile_matrix_sets_include_web_mercator() {
        let resp = get(state_with_collection(), "/tileMatrixSets").await;
        let body = body_string(resp).await;
        assert!(body.contains("WebMercatorQuad"));
        assert!(body.contains("WorldCRS84Quad"));
    }

    #[tokio::test]
    async fn single_tile_matrix_set_returned() {
        let resp = get(state_with_collection(), "/tileMatrixSets/WebMercatorQuad").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("tileMatrices"));
    }

    #[tokio::test]
    async fn unknown_tile_matrix_set_404() {
        let resp = get(state_with_collection(), "/tileMatrixSets/Nope").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn collection_tileset_metadata() {
        let resp = get(
            state_with_collection(),
            "/collections/parcels/tiles/WebMercatorQuad",
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("WebMercatorQuad"));
        assert!(body.contains("tileMatrixSetId"));
    }

    #[tokio::test]
    async fn vector_tile_is_built_from_provider() {
        let resp = get(
            state_with_collection(),
            "/collections/parcels/tiles/WebMercatorQuad/2/1/1",
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .and_then(|h| h.to_str().ok()),
            Some(MVT_MEDIA_TYPE)
        );
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert!(!bytes.is_empty(), "vector tile payload should be non-empty");
    }

    #[tokio::test]
    async fn tile_without_provider_is_not_implemented() {
        let state = OgcTilesState::new("http://localhost/tiles");
        state.register_collection("parcels", "Parcels", vec!["WebMercatorQuad".to_string()]);
        let resp = get(state, "/collections/parcels/tiles/WebMercatorQuad/2/1/1").await;
        // UnsupportedOperation maps to 400 with OperationNotSupported.
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn tile_out_of_range_is_rejected() {
        let resp = get(
            state_with_collection(),
            "/collections/parcels/tiles/WebMercatorQuad/0/5/5",
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn unknown_collection_404() {
        let resp = get(state_with_collection(), "/collections/ghost/tiles").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
