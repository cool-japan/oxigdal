//! KML/KMZ format tests.

use oxigdal_drivers_advanced::Result;
use oxigdal_drivers_advanced::kml::*;
use oxigdal_drivers_advanced::kmz::*;
use std::io::Cursor;
use std::str::FromStr;

#[test]
fn test_kml_document_creation() {
    let doc = KmlDocument::new();
    assert!(doc.name.is_none());
    assert_eq!(doc.placemark_count(), 0);
}

#[test]
fn test_kml_document_builder() {
    let doc = KmlDocument::new()
        .with_name("Test Document")
        .with_description("A test KML document");

    assert_eq!(doc.name, Some("Test Document".to_string()));
    assert!(doc.description.is_some());
}

#[test]
fn test_kml_placemark() {
    let coords = Coordinates::new(-122.08, 37.42);
    let placemark = Placemark::new()
        .with_name("Test Point")
        .with_geometry(KmlGeometry::Point(coords));

    assert_eq!(placemark.name, Some("Test Point".to_string()));
    assert!(placemark.geometry.is_some());
}

#[test]
fn test_coordinates() {
    let coords = Coordinates::new(-122.08, 37.42);
    assert_eq!(coords.lon, -122.08);
    assert_eq!(coords.lat, 37.42);
    assert!(coords.alt.is_none());

    let coords_3d = Coordinates::with_altitude(-122.08, 37.42, 100.0);
    assert_eq!(coords_3d.alt, Some(100.0));
}

#[test]
fn test_coordinates_to_kml_string() {
    let coords = Coordinates::new(-122.08, 37.42);
    let kml_str = coords.to_kml_string();
    assert!(kml_str.contains("-122.08"));
    assert!(kml_str.contains("37.42"));
}

#[test]
fn test_kml_styles() {
    let icon_style = IconStyle::new()
        .with_color("ff0000ff")
        .with_scale(1.5)
        .with_href("http://example.com/icon.png");

    assert_eq!(icon_style.color, Some("ff0000ff".to_string()));
    assert_eq!(icon_style.scale, 1.5);

    let line_style = LineStyle::new().with_color("ff00ff00").with_width(2.0);

    assert_eq!(line_style.width, 2.0);
}

#[test]
fn test_poly_style() {
    let poly_style = PolyStyle::new()
        .with_color("7fff0000")
        .with_fill(true)
        .with_outline(false);

    assert!(poly_style.fill);
    assert!(!poly_style.outline);
}

#[test]
fn test_style_map() {
    let style_map = StyleMap::new("#normal", "#highlight").with_id("test-map");

    assert_eq!(style_map.id, Some("test-map".to_string()));
    assert_eq!(style_map.normal, "#normal");
}

#[test]
fn test_kml_write_empty_document() -> Result<()> {
    let mut buf = Vec::new();
    let doc = KmlDocument::new();
    write_kml(&mut buf, &doc)?;

    let output = String::from_utf8(buf)?;
    assert!(output.contains("<kml"));
    assert!(output.contains("<Document>"));

    Ok(())
}

#[test]
fn test_kml_write_with_placemark() -> Result<()> {
    let mut buf = Vec::new();
    let mut doc = KmlDocument::new().with_name("Test");

    let placemark = Placemark::new()
        .with_name("Test Point")
        .with_geometry(KmlGeometry::Point(Coordinates::new(-122.08, 37.42)));

    doc.add_placemark(placemark);
    write_kml(&mut buf, &doc)?;

    let output = String::from_utf8(buf)?;
    assert!(output.contains("Test Point"));
    assert!(output.contains("-122.08"));

    Ok(())
}

#[test]
fn test_refresh_mode() {
    let rm = RefreshMode::from_str("onChange");
    assert!(rm.is_ok());
    if let Ok(mode) = rm {
        assert_eq!(mode, RefreshMode::OnChange);
    }
    let rm = RefreshMode::from_str("oninterval");
    assert!(rm.is_ok());
    if let Ok(mode) = rm {
        assert_eq!(mode, RefreshMode::OnInterval);
    }
    assert_eq!(RefreshMode::OnChange.as_str(), "onChange");
}

#[test]
fn test_network_link() {
    let link = NetworkLink {
        name: Some("Test Link".to_string()),
        visibility: true,
        refresh_mode: RefreshMode::OnChange,
        href: "http://example.com/data.kml".to_string(),
    };

    assert_eq!(link.name, Some("Test Link".to_string()));
    assert!(link.visibility);
}

#[test]
fn test_kmz_archive_creation() {
    let mut archive = KmzArchive::new();
    assert_eq!(archive.document_count(), 0);
    assert_eq!(archive.image_count(), 0);

    let doc = KmlDocument::new();
    archive.add_document("doc.kml", doc);
    assert_eq!(archive.document_count(), 1);

    archive.add_image("icon.png", vec![0, 1, 2, 3]);
    assert_eq!(archive.image_count(), 1);
}

#[test]
fn test_kmz_write_read_roundtrip() -> Result<()> {
    let doc = KmlDocument::new().with_name("Test KMZ");
    let images = vec![("test.png".to_string(), vec![0u8; 100])];

    let mut buffer = Cursor::new(Vec::new());
    write_kmz(&mut buffer, &doc, &images)?;

    buffer.set_position(0);
    let archive = read_kmz(buffer)?;

    assert!(archive.document_count() >= 1);
    assert_eq!(archive.image_count(), 1);

    Ok(())
}

#[test]
fn test_kmz_get_document() {
    let mut archive = KmzArchive::new();
    let doc = KmlDocument::new().with_name("Test");

    archive.add_document("test.kml", doc);

    let retrieved = archive.get_document("test.kml");
    assert!(retrieved.is_some());

    let not_found = archive.get_document("missing.kml");
    assert!(not_found.is_none());
}

#[test]
fn test_kmz_get_image() {
    let mut archive = KmzArchive::new();
    archive.add_image("icon.png", vec![1, 2, 3, 4]);

    let image = archive.get_image("icon.png");
    assert!(image.is_some());
    assert_eq!(image.map(|i| i.len()), Some(4));
}
