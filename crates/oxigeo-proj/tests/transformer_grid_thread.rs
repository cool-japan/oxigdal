//! Integration tests for cross-thread `Transformer` sharing (the cache fix)
//! and the compound-CRS vertical-datum warning diagnostic. Public-API only.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use oxigeo_proj::{Coordinate, Coordinate3D, Crs, Transformer};

#[test]
fn transformer_is_send_sync_and_shares_across_threads() {
    // The cross-thread cache fix stores the built oxiproj engine on the struct,
    // so an Arc<Transformer> can be shared and reused from other threads.
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Transformer>();

    let shared = Arc::new(Transformer::from_epsg(4326, 3857).expect("transformer"));
    let mut handles = Vec::new();
    for _ in 0..4 {
        let t = Arc::clone(&shared);
        handles.push(std::thread::spawn(move || {
            t.transform(&Coordinate::from_lon_lat(0.0, 51.5))
                .expect("transform on worker thread")
                .x
        }));
    }
    let reference = shared
        .transform(&Coordinate::from_lon_lat(0.0, 51.5))
        .expect("transform on main thread")
        .x;
    for h in handles {
        let x = h.join().expect("thread join");
        assert!((x - reference).abs() < 1e-6, "cross-thread result differs");
    }
}

#[test]
fn compound_orthometric_to_ellipsoidal_records_warning_without_geoid() {
    // Orthometric (EGM96 geoid) → Ellipsoidal (WGS84 ellipsoidal height)
    // requires an undulation correction. Without a geoid attached, z must pass
    // through unchanged BUT a VerticalDatumWarning must be recorded (previously
    // a wholly-silent fall-through).
    let horiz = Crs::wgs84();
    let ortho = Crs::from_wkt(r#"VERTCRS["EGM96 height",VDATUM["EGM96 geoid"],UNIT["metre",1]]"#)
        .expect("ortho parse");
    let ellip =
        Crs::from_wkt(r#"VERTCRS["WGS84 ellipsoidal height",VDATUM["ellipsoid"],UNIT["metre",1]]"#)
            .expect("ellip parse");
    let src = Crs::compound(horiz.clone(), ortho).expect("compound src");
    let dst = Crs::compound(horiz, ellip).expect("compound dst");

    let transformer = Transformer::new(src, dst).expect("transformer");
    assert!(transformer.last_vertical_warning().is_none());

    let input = Coordinate3D::new(5.0, 45.0, 100.0);
    let output = transformer
        .transform_3d(&input)
        .expect("no-geoid passthrough");
    assert!((output.z - input.z).abs() < 1e-9, "z must pass through");

    let warn = transformer
        .last_vertical_warning()
        .expect("a vertical-datum warning must be recorded");
    assert!((warn.lon - 5.0).abs() < 1e-9 && (warn.lat - 45.0).abs() < 1e-9);
    assert!(warn.source_vertical.to_lowercase().contains("egm96"));
    assert!(warn.target_vertical.to_lowercase().contains("ellipsoidal"));

    transformer.clear_vertical_warning();
    assert!(transformer.last_vertical_warning().is_none());
}
