//! Complete OGC services server example
//!
//! Demonstrates how to set up and run all four OGC web services:
//! - WFS (Web Feature Service)
//! - WCS (Web Coverage Service)
//! - WPS (Web Processing Service)
//! - CSW (Catalog Service for the Web)

use axum::{Router, routing::get};
use oxigdal_services::{csw, wcs, wfs, wps};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Create WFS service
    let wfs_info = wfs::ServiceInfo {
        title: "OxiGDAL WFS Service".to_string(),
        abstract_text: Some("Web Feature Service for vector data access".to_string()),
        provider: "COOLJAPAN OU (Team Kitasan)".to_string(),
        service_url: "http://localhost:8080/wfs".to_string(),
        versions: vec!["2.0.0".to_string(), "3.0.0".to_string()],
    };
    let wfs_state = wfs::WfsState::new(wfs_info);

    // Add sample feature type
    let feature_type = wfs::FeatureTypeInfo {
        name: "sample_layer".to_string(),
        title: "Sample Layer".to_string(),
        abstract_text: Some("Sample feature layer".to_string()),
        default_crs: "EPSG:4326".to_string(),
        other_crs: vec!["EPSG:3857".to_string()],
        bbox: Some((-180.0, -90.0, 180.0, 90.0)),
        source: wfs::FeatureSource::Memory(vec![]),
    };
    wfs_state.add_feature_type(feature_type)?;

    // Create WCS service
    let wcs_info = wcs::ServiceInfo {
        title: "OxiGDAL WCS Service".to_string(),
        abstract_text: Some("Web Coverage Service for raster data access".to_string()),
        provider: "COOLJAPAN OU (Team Kitasan)".to_string(),
        service_url: "http://localhost:8080/wcs".to_string(),
        versions: vec!["2.0.1".to_string()],
    };
    let wcs_state = wcs::WcsState::new(wcs_info);

    // Add sample coverage
    let coverage = wcs::CoverageInfo {
        coverage_id: "sample_coverage".to_string(),
        title: "Sample Coverage".to_string(),
        abstract_text: Some("Sample raster coverage".to_string()),
        native_crs: "EPSG:4326".to_string(),
        bbox: (-180.0, -90.0, 180.0, 90.0),
        grid_size: (1024, 512),
        grid_origin: (-180.0, 90.0),
        grid_resolution: (0.35, -0.35),
        band_count: 3,
        band_names: vec!["Red".to_string(), "Green".to_string(), "Blue".to_string()],
        data_type: "Byte".to_string(),
        source: wcs::CoverageSource::Memory,
        formats: vec!["image/tiff".to_string(), "image/png".to_string()],
    };
    wcs_state.add_coverage(coverage)?;

    // Create WPS service
    let wps_info = wps::ServiceInfo {
        title: "OxiGDAL WPS Service".to_string(),
        abstract_text: Some("Web Processing Service for geospatial processing".to_string()),
        provider: "COOLJAPAN OU (Team Kitasan)".to_string(),
        service_url: "http://localhost:8080/wps".to_string(),
        versions: vec!["2.0.0".to_string()],
    };
    let wps_state = wps::WpsState::new(wps_info);

    // Create CSW service
    let csw_info = csw::ServiceInfo {
        title: "OxiGDAL CSW Service".to_string(),
        abstract_text: Some("Catalog Service for metadata search".to_string()),
        provider: "COOLJAPAN OU (Team Kitasan)".to_string(),
        service_url: "http://localhost:8080/csw".to_string(),
        versions: vec!["2.0.2".to_string()],
    };
    let csw_state = csw::CswState::new(csw_info);

    // Add sample metadata record
    let record = csw::MetadataRecord {
        identifier: "sample_record".to_string(),
        title: "Sample Dataset".to_string(),
        abstract_text: Some("Sample metadata record".to_string()),
        keywords: vec!["geospatial".to_string(), "sample".to_string()],
        bbox: Some((-180.0, -90.0, 180.0, 90.0)),
    };
    csw_state.add_record(record)?;

    // Build router
    let app = Router::new()
        .route("/wfs", get(wfs::handle_wfs_request).with_state(wfs_state))
        .route("/wcs", get(wcs::handle_wcs_request).with_state(wcs_state))
        .route("/wps", get(wps::handle_wps_request).with_state(wps_state))
        .route("/csw", get(csw::handle_csw_request).with_state(csw_state));

    // Start server
    let addr = "0.0.0.0:8080";
    println!("OxiGDAL Services running on http://{}", addr);
    println!(
        "  WFS: http://{}/wfs?SERVICE=WFS&REQUEST=GetCapabilities",
        addr
    );
    println!(
        "  WCS: http://{}/wcs?SERVICE=WCS&REQUEST=GetCapabilities",
        addr
    );
    println!(
        "  WPS: http://{}/wps?SERVICE=WPS&REQUEST=GetCapabilities",
        addr
    );
    println!(
        "  CSW: http://{}/csw?SERVICE=CSW&REQUEST=GetCapabilities",
        addr
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
