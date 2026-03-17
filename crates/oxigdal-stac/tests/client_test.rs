//! Integration tests for the STAC API client module.
//!
//! Covers request building, response parsing, pagination logic, and the
//! transport-agnostic `StacApiClient` helpers.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use oxigdal_stac::client::{
    ClientError, FieldsSpec, ItemCollection, PageIterator, PaginationStrategy, SearchRequest,
    SortDirection, SortField, StacApiClient, StacApiRequest, StacAsset, StacCollection, StacItem,
    StacLink,
};

// ---------------------------------------------------------------------------
// Helper constructors
// ---------------------------------------------------------------------------

fn make_item(id: &str) -> StacItem {
    StacItem {
        stac_version: "1.0.0".to_string(),
        stac_extensions: Vec::new(),
        id: id.to_string(),
        type_: "Feature".to_string(),
        geometry: None,
        bbox: Some([-10.0, -10.0, 10.0, 10.0]),
        properties: serde_json::json!({
            "datetime": "2023-06-15T12:00:00Z",
            "platform": "sentinel-2a",
            "constellation": "sentinel-2",
            "eo:cloud_cover": 7.3,
            "gsd": 10.0,
        }),
        links: Vec::new(),
        assets: HashMap::new(),
        collection: Some("sentinel-2-l2a".to_string()),
    }
}

fn make_page_with_token(count: usize, token: Option<&str>) -> ItemCollection {
    let mut links = Vec::new();
    if let Some(t) = token {
        links.push(StacLink::new(
            "next",
            format!("https://api.example.com/search?token={}", t),
        ));
    }
    ItemCollection {
        type_: "FeatureCollection".to_string(),
        features: (0..count).map(|i| make_item(&i.to_string())).collect(),
        links,
        context: None,
        number_matched: Some(100),
        number_returned: Some(count as u64),
    }
}

fn make_page_of(count: usize) -> ItemCollection {
    make_page_with_token(count, None)
}

// ---------------------------------------------------------------------------
// SearchRequest builder tests
// ---------------------------------------------------------------------------

#[test]
fn test_search_request_new_is_empty() {
    let req = SearchRequest::new();
    assert!(req.bbox.is_none());
    assert!(req.collections.is_none());
    assert!(req.limit.is_none());
    assert!(req.ids.is_none());
    assert!(req.datetime.is_none());
}

#[test]
fn test_search_request_with_bbox() {
    let bbox = [-10.0_f64, -20.0, 10.0, 20.0];
    let req = SearchRequest::new().with_bbox(bbox);
    assert_eq!(req.bbox, Some(bbox));
}

#[test]
fn test_search_request_with_collections() {
    let req = SearchRequest::new().with_collections(vec!["col-a".to_string(), "col-b".to_string()]);
    let cols = req.collections.expect("collections should be set");
    assert_eq!(cols, vec!["col-a", "col-b"]);
}

#[test]
fn test_search_request_with_datetime() {
    let req = SearchRequest::new().with_datetime("2023-01-01T00:00:00Z/2024-01-01T00:00:00Z");
    assert_eq!(
        req.datetime.as_deref(),
        Some("2023-01-01T00:00:00Z/2024-01-01T00:00:00Z")
    );
}

#[test]
fn test_search_request_with_limit() {
    let req = SearchRequest::new().with_limit(42);
    assert_eq!(req.limit, Some(42));
}

#[test]
fn test_search_request_with_ids() {
    let req = SearchRequest::new().with_ids(vec!["id1".to_string(), "id2".to_string()]);
    let ids = req.ids.expect("ids should be set");
    assert_eq!(ids, vec!["id1", "id2"]);
}

#[test]
fn test_search_request_with_token() {
    let req = SearchRequest::new().with_token("opaque-cursor-abc");
    assert_eq!(req.token.as_deref(), Some("opaque-cursor-abc"));
}

#[test]
fn test_search_request_with_page() {
    let req = SearchRequest::new().with_page(3);
    assert_eq!(req.page, Some(3));
}

#[test]
fn test_search_request_with_sort() {
    let req = SearchRequest::new()
        .with_sort(SortField::desc("properties.datetime"))
        .with_sort(SortField::asc("id"));
    let sorts = req.sort_by.as_ref().expect("sort_by should be set");
    assert_eq!(sorts.len(), 2);
    assert_eq!(sorts[0].direction, SortDirection::Descending);
    assert_eq!(sorts[1].direction, SortDirection::Ascending);
}

#[test]
fn test_search_request_with_fields() {
    let fs = FieldsSpec::new()
        .include("properties.datetime")
        .exclude("assets.thumbnail");
    let req = SearchRequest::new().with_fields(fs.clone());
    assert_eq!(req.fields.expect("fields should be set"), fs);
}

// ---------------------------------------------------------------------------
// SearchRequest JSON round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_search_request_to_json_and_from_json_roundtrip() {
    let req = SearchRequest::new()
        .with_bbox([-5.0, -5.0, 5.0, 5.0])
        .with_collections(vec!["my-col".to_string()])
        .with_datetime("2023-01-01T00:00:00Z")
        .with_limit(25)
        .with_sort(SortField::asc("datetime"));
    let json = req.to_json().expect("serialize");
    let back = SearchRequest::from_json(&json).expect("deserialize");
    assert_eq!(req, back);
}

#[test]
fn test_search_request_from_json_minimal() {
    let json = r#"{"limit": 10}"#;
    let req = SearchRequest::from_json(json).expect("parse");
    assert_eq!(req.limit, Some(10));
    assert!(req.bbox.is_none());
}

#[test]
fn test_search_request_to_json_omits_none_fields() {
    let req = SearchRequest::new().with_limit(10);
    let json = req.to_json().expect("serialize");
    assert!(!json.contains("bbox"));
    assert!(!json.contains("collections"));
}

#[test]
fn test_search_request_fields_in_json() {
    let fs = FieldsSpec::new().include("id").exclude("assets");
    let req = SearchRequest::new().with_fields(fs);
    let json = req.to_json().expect("serialize");
    assert!(json.contains("include"));
    assert!(json.contains("exclude"));
}

// ---------------------------------------------------------------------------
// StacApiRequest tests
// ---------------------------------------------------------------------------

#[test]
fn test_get_landing_method_get() {
    let r = StacApiRequest::GetLanding;
    assert_eq!(r.method(), "GET");
}

#[test]
fn test_get_landing_path_empty() {
    let r = StacApiRequest::GetLanding;
    assert_eq!(r.path(), "");
}

#[test]
fn test_get_conformance_path() {
    let r = StacApiRequest::GetConformance;
    assert_eq!(r.path(), "conformance");
}

#[test]
fn test_get_collections_path() {
    let r = StacApiRequest::GetCollections { limit: None };
    assert_eq!(r.path(), "collections");
}

#[test]
fn test_get_collection_path() {
    let r = StacApiRequest::GetCollection {
        id: "my-col".to_string(),
    };
    assert_eq!(r.path(), "collections/my-col");
}

#[test]
fn test_get_items_path() {
    let r = StacApiRequest::GetItems {
        collection_id: "col".to_string(),
        limit: None,
        offset: None,
        bbox: None,
        datetime: None,
        fields: None,
    };
    assert_eq!(r.path(), "collections/col/items");
}

#[test]
fn test_get_item_path() {
    let r = StacApiRequest::GetItem {
        collection_id: "col".to_string(),
        item_id: "item-123".to_string(),
    };
    assert_eq!(r.path(), "collections/col/items/item-123");
}

#[test]
fn test_search_method_post() {
    let r = StacApiRequest::Search(SearchRequest::new());
    assert_eq!(r.method(), "POST");
}

#[test]
fn test_search_body_json_is_some() {
    let r = StacApiRequest::Search(SearchRequest::new().with_limit(5));
    assert!(r.body_json().is_some());
    let body = r.body_json().expect("body_json should be present");
    assert!(body.contains("limit"));
}

#[test]
fn test_get_items_query_params_contain_bbox() {
    let r = StacApiRequest::GetItems {
        collection_id: "col".to_string(),
        limit: None,
        offset: None,
        bbox: Some([-10.0, -20.0, 10.0, 20.0]),
        datetime: None,
        fields: None,
    };
    let params = r.query_params();
    let bbox_val = params
        .iter()
        .find(|(k, _)| k == "bbox")
        .map(|(_, v)| v.as_str());
    assert!(bbox_val.is_some());
    assert!(bbox_val.expect("bbox param should exist").contains("-10"));
}

#[test]
fn test_to_url_landing_no_trailing_slash() {
    let r = StacApiRequest::GetLanding;
    let url = r.to_url("https://example.com/stac");
    assert_eq!(url, "https://example.com/stac");
}

#[test]
fn test_to_url_conformance() {
    let r = StacApiRequest::GetConformance;
    let url = r.to_url("https://example.com");
    assert_eq!(url, "https://example.com/conformance");
}

#[test]
fn test_to_url_trailing_slash_normalised() {
    let r = StacApiRequest::GetConformance;
    let url = r.to_url("https://example.com/");
    assert_eq!(url, "https://example.com/conformance");
}

#[test]
fn test_to_url_with_query_params() {
    let r = StacApiRequest::GetCollections { limit: Some(20) };
    let url = r.to_url("https://example.com");
    assert!(url.starts_with("https://example.com/collections"));
    assert!(url.contains("limit=20"));
}

// ---------------------------------------------------------------------------
// SortField and FieldsSpec serialization
// ---------------------------------------------------------------------------

#[test]
fn test_sort_field_ascending_serialization() {
    let sf = SortField::asc("datetime");
    let json = serde_json::to_string(&sf).expect("serialize");
    assert!(json.contains("ascending"));
}

#[test]
fn test_sort_field_descending_serialization() {
    let sf = SortField::desc("created");
    let json = serde_json::to_string(&sf).expect("serialize");
    assert!(json.contains("descending"));
}

#[test]
fn test_fields_spec_include_exclude() {
    let fs = FieldsSpec::new()
        .include("properties.datetime")
        .include("id")
        .exclude("assets");
    assert_eq!(fs.include.len(), 2);
    assert_eq!(fs.exclude.len(), 1);
}

#[test]
fn test_fields_spec_in_search_json() {
    let fs = FieldsSpec::new().include("id").exclude("assets.thumbnail");
    let req = SearchRequest::new().with_fields(fs);
    let json = req.to_json().expect("serialize");
    assert!(json.contains("\"id\""));
    assert!(json.contains("assets.thumbnail"));
}

// ---------------------------------------------------------------------------
// StacAsset
// ---------------------------------------------------------------------------

#[test]
fn test_stac_asset_multiple_roles() {
    let asset = StacAsset {
        href: "https://example.com/file.tif".to_string(),
        title: Some("Visual".to_string()),
        description: None,
        type_: Some("image/tiff".to_string()),
        roles: vec!["data".to_string(), "overview".to_string()],
    };
    assert_eq!(asset.roles.len(), 2);
    assert!(asset.roles.contains(&"data".to_string()));
}

// ---------------------------------------------------------------------------
// StacItem property accessors
// ---------------------------------------------------------------------------

#[test]
fn test_stac_item_datetime() {
    let item = make_item("test");
    assert_eq!(item.datetime(), Some("2023-06-15T12:00:00Z"));
}

#[test]
fn test_stac_item_cloud_cover() {
    let item = make_item("test");
    let cc = item.cloud_cover().expect("cloud cover");
    assert!((cc - 7.3).abs() < 1e-9);
}

#[test]
fn test_stac_item_platform() {
    let item = make_item("test");
    assert_eq!(item.platform(), Some("sentinel-2a"));
}

#[test]
fn test_stac_item_constellation() {
    let item = make_item("test");
    assert_eq!(item.constellation(), Some("sentinel-2"));
}

#[test]
fn test_stac_item_get_property_f64() {
    let item = make_item("test");
    let gsd: Option<f64> = item.get_property("gsd");
    assert_eq!(gsd, Some(10.0));
}

#[test]
fn test_stac_item_get_property_string() {
    let item = make_item("test");
    let platform: Option<String> = item.get_property("platform");
    assert_eq!(platform.as_deref(), Some("sentinel-2a"));
}

#[test]
fn test_stac_item_get_property_missing() {
    let item = make_item("test");
    let missing: Option<f64> = item.get_property("nonexistent_key");
    assert!(missing.is_none());
}

// ---------------------------------------------------------------------------
// ItemCollection helper methods
// ---------------------------------------------------------------------------

#[test]
fn test_item_collection_next_page_token_from_href() {
    let page = make_page_with_token(5, Some("cursor-abc"));
    assert_eq!(page.next_page_token(), Some("cursor-abc".to_string()));
}

#[test]
fn test_item_collection_next_page_token_none() {
    let page = make_page_of(5);
    assert!(page.next_page_token().is_none());
}

#[test]
fn test_item_collection_prev_page_token() {
    let mut page = make_page_of(5);
    page.links.push(StacLink::new(
        "prev",
        "https://api.example.com/search?token=prev-cursor",
    ));
    assert_eq!(page.prev_page_token(), Some("prev-cursor".to_string()));
}

#[test]
fn test_item_collection_is_empty() {
    let ic = ItemCollection::empty();
    assert!(ic.is_empty());
}

#[test]
fn test_item_collection_len() {
    let page = make_page_of(7);
    assert_eq!(page.len(), 7);
}

// ---------------------------------------------------------------------------
// PaginationStrategy tests
// ---------------------------------------------------------------------------

#[test]
fn test_pagination_strategy_first_page_offset_zero() {
    let s = PaginationStrategy::first_page(10);
    match &s {
        PaginationStrategy::OffsetBased { offset, limit } => {
            assert_eq!(*offset, 0);
            assert_eq!(*limit, 10);
        }
        _ => unreachable!("expected OffsetBased"),
    }
}

#[test]
fn test_pagination_strategy_next_page_increments_offset() {
    let s = PaginationStrategy::first_page(5);
    let page = make_page_of(5); // exactly limit → more pages
    let next = s.next_page(&page).expect("should have next");
    match &next {
        PaginationStrategy::OffsetBased { offset, .. } => assert_eq!(*offset, 5),
        _ => unreachable!("expected OffsetBased"),
    }
}

#[test]
fn test_pagination_strategy_apply_to_request_sets_limit() {
    let s = PaginationStrategy::OffsetBased {
        offset: 0,
        limit: 20,
    };
    let mut req = SearchRequest::new();
    s.apply_to_request(&mut req);
    assert_eq!(req.limit, Some(20));
}

#[test]
fn test_pagination_strategy_apply_token() {
    let s = PaginationStrategy::TokenBased {
        token: Some("tok".to_string()),
        limit: 15,
    };
    let mut req = SearchRequest::new();
    s.apply_to_request(&mut req);
    assert_eq!(req.token.as_deref(), Some("tok"));
    assert_eq!(req.limit, Some(15));
}

// ---------------------------------------------------------------------------
// PageIterator tests
// ---------------------------------------------------------------------------

#[test]
fn test_page_iterator_two_real_pages_then_empty() {
    let call = Arc::new(AtomicU32::new(0));
    let c = call.clone();

    let fetch = move |_req: SearchRequest| -> Result<ItemCollection, ClientError> {
        let n = c.fetch_add(1, Ordering::SeqCst);
        match n {
            0 => Ok(make_page_of(5)),         // page 1: 5 items (== limit)
            _ => Ok(ItemCollection::empty()), // page 2: empty
        }
    };

    let req = SearchRequest::new().with_limit(5);
    let strategy = PaginationStrategy::first_page(5);
    let pages: Vec<_> = PageIterator::new(req, strategy, fetch).collect();

    // Page 1 has items, page 2 is empty (returns it but stops).
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].as_ref().expect("page 0").len(), 5);
    assert!(pages[1].as_ref().expect("page 1").is_empty());
}

#[test]
fn test_page_iterator_max_pages_stops_early() {
    let fetch = |_req: SearchRequest| -> Result<ItemCollection, ClientError> {
        Ok(make_page_of(10)) // always returns 10 items
    };

    let req = SearchRequest::new().with_limit(10);
    let strategy = PaginationStrategy::first_page(10);
    let pages: Vec<_> = PageIterator::new(req, strategy, fetch)
        .max_pages(3)
        .collect();

    assert_eq!(pages.len(), 3);
}

#[test]
fn test_page_iterator_stops_on_fewer_items() {
    let fetch = |_req: SearchRequest| -> Result<ItemCollection, ClientError> {
        Ok(make_page_of(3)) // 3 < limit(10) → last page
    };

    let req = SearchRequest::new().with_limit(10);
    let strategy = PaginationStrategy::first_page(10);
    let pages: Vec<_> = PageIterator::new(req, strategy, fetch).collect();

    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].as_ref().expect("page 0").len(), 3);
}

// ---------------------------------------------------------------------------
// StacApiClient tests
// ---------------------------------------------------------------------------

#[test]
fn test_stac_api_client_search_url_format() {
    let c = StacApiClient::new("https://earth-search.aws.element84.com/v1");
    let req = SearchRequest::new();
    assert_eq!(
        c.search_url(&req),
        "https://earth-search.aws.element84.com/v1/search"
    );
}

#[test]
fn test_stac_api_client_items_url_no_params() {
    let c = StacApiClient::new("https://api.example.com");
    let url = c.items_url("sentinel-2-l2a", None, None);
    assert_eq!(
        url,
        "https://api.example.com/collections/sentinel-2-l2a/items"
    );
}

#[test]
fn test_stac_api_client_items_url_with_limit() {
    let c = StacApiClient::new("https://api.example.com");
    let url = c.items_url("col", Some(10), None);
    assert!(url.contains("limit=10"));
    assert!(!url.contains("offset"));
}

#[test]
fn test_stac_api_client_items_url_with_limit_and_offset() {
    let c = StacApiClient::new("https://api.example.com");
    let url = c.items_url("col", Some(10), Some(30));
    assert!(url.contains("limit=10"));
    assert!(url.contains("offset=30"));
}

#[test]
fn test_stac_api_client_next_page_request_from_token_link() {
    let c = StacApiClient::new("https://api.example.com");
    let mut response = ItemCollection::empty();
    response.links.push(StacLink::new(
        "next",
        "https://api.example.com/search?token=page2tok",
    ));
    let req = SearchRequest::new().with_limit(10);
    let next = c
        .next_page_request(&response, &req)
        .expect("should have next");
    assert_eq!(next.token.as_deref(), Some("page2tok"));
}

#[test]
fn test_stac_api_client_next_page_none_when_no_next_link() {
    let c = StacApiClient::new("https://api.example.com");
    let response = ItemCollection::empty();
    let req = SearchRequest::new();
    assert!(c.next_page_request(&response, &req).is_none());
}

#[test]
fn test_stac_api_client_parse_item_collection_minimal() {
    let c = StacApiClient::new("https://api.example.com");
    let json = r#"{"type":"FeatureCollection","features":[]}"#;
    let ic = c.parse_item_collection(json).expect("parse");
    assert_eq!(ic.type_, "FeatureCollection");
    assert!(ic.is_empty());
}

#[test]
fn test_stac_api_client_parse_item_collection_with_items() {
    let c = StacApiClient::new("https://api.example.com");
    let json = r#"{
        "type": "FeatureCollection",
        "features": [
            {
                "stac_version": "1.0.0",
                "type": "Feature",
                "id": "item-1",
                "geometry": null,
                "properties": {"datetime": "2023-01-01T00:00:00Z"},
                "links": [],
                "assets": {}
            }
        ]
    }"#;
    let ic = c.parse_item_collection(json).expect("parse");
    assert_eq!(ic.len(), 1);
    assert_eq!(ic.features[0].id, "item-1");
}

#[test]
fn test_stac_api_client_parse_item_minimal() {
    let c = StacApiClient::new("https://api.example.com");
    let json = r#"{
        "stac_version": "1.0.0",
        "type": "Feature",
        "id": "my-item",
        "geometry": null,
        "properties": {"datetime": "2024-06-01T00:00:00Z"},
        "links": [],
        "assets": {}
    }"#;
    let item = c.parse_item(json).expect("parse");
    assert_eq!(item.id, "my-item");
    assert_eq!(item.type_, "Feature");
    assert_eq!(item.datetime(), Some("2024-06-01T00:00:00Z"));
}

#[test]
fn test_stac_api_client_parse_collection_minimal() {
    let c = StacApiClient::new("https://api.example.com");
    let json = r#"{
        "stac_version": "1.0.0",
        "type": "Collection",
        "id": "sentinel-2-l2a",
        "description": "Sentinel-2 Level-2A",
        "license": "proprietary",
        "extent": {
            "spatial": { "bbox": [[-180, -90, 180, 90]] },
            "temporal": { "interval": [["2015-06-23T00:00:00Z", null]] }
        },
        "links": []
    }"#;
    let col: StacCollection = c.parse_collection(json).expect("parse");
    assert_eq!(col.id, "sentinel-2-l2a");
    assert_eq!(col.license, "proprietary");
}
