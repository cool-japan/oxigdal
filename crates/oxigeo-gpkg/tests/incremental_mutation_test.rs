//! Integration tests for [`GeoPackageEditor`] incremental INSERT / UPDATE /
//! DELETE support.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::needless_borrows_for_generic_args
)]

use std::collections::HashMap;

use oxigeo_gpkg::{
    GeoPackage, GeoPackageBuilder, GeoPackageEditor, error::GpkgError, vector::FeatureRow,
    vector::GpkgGeometry,
};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Build a minimal 2-feature GeoPackage and return the raw bytes.
fn two_point_gpkg() -> Vec<u8> {
    GeoPackageBuilder::new(4326)
        .add_feature_table("pts", "POINT", vec![(1, 1.0, 2.0), (2, 3.0, 4.0)])
        .build()
        .expect("build two-point gpkg")
}

/// Create a temp file path that is unique per test name.
fn temp_path(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("oxigeo_im_test_{tag}.gpkg"))
}

/// Construct a [`FeatureRow`] with a single POINT geometry and empty fields.
fn make_point_row(x: f64, y: f64) -> FeatureRow {
    FeatureRow {
        fid: 0, // ignored / overwritten by insert_feature
        geometry: Some(GpkgGeometry::Point { x, y }),
        fields: HashMap::new(),
    }
}

/// Count rows in a named table by opening the bytes with [`GeoPackage`].
fn count_rows(bytes: &[u8], table: &str) -> usize {
    let gpkg = GeoPackage::from_bytes(bytes.to_vec()).expect("reopen gpkg");
    gpkg.scan_table_by_name(table)
        .expect("scan")
        .map(|rows| rows.len())
        .unwrap_or(0)
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Opening the editor reads existing features
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_open_editor_reads_existing_features() {
    let path = temp_path("open_reads");
    std::fs::write(&path, &two_point_gpkg()).unwrap();

    let ed = GeoPackageEditor::open(&path, "pts").unwrap();

    // No mutations buffered yet.
    assert_eq!(ed.pending_inserts(), 0);
    assert_eq!(ed.pending_updates(), 0);
    assert_eq!(ed.pending_deletes(), 0);
    // Snapshot has two features.
    assert_eq!(ed.snapshot_feature_count(), 2);

    let _ = std::fs::remove_file(&path);
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Insert assigns the next FID
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_insert_feature_assigns_next_fid() {
    let path = temp_path("insert_fid");
    std::fs::write(&path, &two_point_gpkg()).unwrap();

    let mut ed = GeoPackageEditor::open(&path, "pts").unwrap();
    let fid = ed.insert_feature(make_point_row(5.0, 6.0));

    // max existing fid = 2, so next = 3
    assert_eq!(fid, 3);

    let _ = std::fs::remove_file(&path);
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Two successive inserts get consecutive FIDs
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_insert_feature_increments_next_fid() {
    let path = temp_path("insert_two");
    std::fs::write(&path, &two_point_gpkg()).unwrap();

    let mut ed = GeoPackageEditor::open(&path, "pts").unwrap();
    let fid_a = ed.insert_feature(make_point_row(5.0, 6.0));
    let fid_b = ed.insert_feature(make_point_row(7.0, 8.0));

    assert_eq!(fid_b - fid_a, 1);

    let _ = std::fs::remove_file(&path);
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Update buffers the change (pending_updates counter)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_update_feature_buffers_change() {
    let path = temp_path("update_buffers");
    std::fs::write(&path, &two_point_gpkg()).unwrap();

    let mut ed = GeoPackageEditor::open(&path, "pts").unwrap();
    ed.update_feature(1, make_point_row(99.0, 99.0)).unwrap();

    assert_eq!(ed.pending_updates(), 1);

    let _ = std::fs::remove_file(&path);
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Update a non-existent FID returns FeatureNotFound
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_update_feature_nonexistent_fid_errors() {
    let path = temp_path("update_notfound");
    std::fs::write(&path, &two_point_gpkg()).unwrap();

    let mut ed = GeoPackageEditor::open(&path, "pts").unwrap();
    let result = ed.update_feature(999, make_point_row(0.0, 0.0));

    assert!(
        matches!(result, Err(GpkgError::FeatureNotFound(999))),
        "expected FeatureNotFound(999), got {result:?}"
    );

    let _ = std::fs::remove_file(&path);
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. Delete a non-existent FID returns FeatureNotFound
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_delete_feature_nonexistent_fid_errors() {
    let path = temp_path("delete_notfound");
    std::fs::write(&path, &two_point_gpkg()).unwrap();

    let mut ed = GeoPackageEditor::open(&path, "pts").unwrap();
    let result = ed.delete_feature(999);

    assert!(
        matches!(result, Err(GpkgError::FeatureNotFound(999))),
        "expected FeatureNotFound(999), got {result:?}"
    );

    let _ = std::fs::remove_file(&path);
}

// ─────────────────────────────────────────────────────────────────────────────
// 7. All three mutation counters reflect buffered operations
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_pending_counters_reflect_buffered_mutations() {
    let path = temp_path("counters");
    std::fs::write(&path, &two_point_gpkg()).unwrap();

    let mut ed = GeoPackageEditor::open(&path, "pts").unwrap();
    ed.insert_feature(make_point_row(5.0, 6.0));
    ed.update_feature(1, make_point_row(10.0, 20.0)).unwrap();
    ed.delete_feature(2).unwrap();

    assert_eq!(ed.pending_inserts(), 1);
    assert_eq!(ed.pending_updates(), 1);
    assert_eq!(ed.pending_deletes(), 1);

    let _ = std::fs::remove_file(&path);
}

// ─────────────────────────────────────────────────────────────────────────────
// 8. Commit writes inserts; re-opening sees the new row
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_commit_to_path_writes_inserts() {
    let src = temp_path("insert_src");
    let dst = temp_path("insert_dst");
    std::fs::write(&src, &two_point_gpkg()).unwrap();

    let mut ed = GeoPackageEditor::open(&src, "pts").unwrap();
    ed.insert_feature(make_point_row(5.0, 6.0));
    let stats = ed.commit_to_path(&dst).unwrap();

    assert_eq!(stats.inserts_applied, 1);
    assert_eq!(stats.features_before, 2);
    assert_eq!(stats.features_after, 3);

    let dst_bytes = std::fs::read(&dst).unwrap();
    assert_eq!(count_rows(&dst_bytes, "pts"), 3);

    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&dst);
}

// ─────────────────────────────────────────────────────────────────────────────
// 9. Commit writes updated geometry; re-opening sees it replaced
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_commit_to_path_writes_updates_replaces_geometry() {
    let src = temp_path("update_src");
    let dst = temp_path("update_dst");
    std::fs::write(&src, &two_point_gpkg()).unwrap();

    let mut ed = GeoPackageEditor::open(&src, "pts").unwrap();
    ed.update_feature(1, make_point_row(77.0, 88.0)).unwrap();
    let stats = ed.commit_to_path(&dst).unwrap();

    assert_eq!(stats.updates_applied, 1);
    assert_eq!(stats.features_after, 2); // same count, geometry replaced

    // Re-open and verify the updated geometry
    let dst_bytes = std::fs::read(&dst).unwrap();
    let gpkg = GeoPackage::from_bytes(dst_bytes).unwrap();
    let rows = gpkg.scan_table_by_name("pts").unwrap().unwrap();
    assert_eq!(rows.len(), 2);

    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&dst);
}

// ─────────────────────────────────────────────────────────────────────────────
// 10. Commit omits deleted features
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_commit_to_path_omits_deleted_features() {
    let src = temp_path("delete_src");
    let dst = temp_path("delete_dst");
    std::fs::write(&src, &two_point_gpkg()).unwrap();

    let mut ed = GeoPackageEditor::open(&src, "pts").unwrap();
    ed.delete_feature(1).unwrap();
    let stats = ed.commit_to_path(&dst).unwrap();

    assert_eq!(stats.deletes_applied, 1);
    assert_eq!(stats.features_before, 2);
    assert_eq!(stats.features_after, 1);

    let dst_bytes = std::fs::read(&dst).unwrap();
    assert_eq!(count_rows(&dst_bytes, "pts"), 1);

    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&dst);
}

// ─────────────────────────────────────────────────────────────────────────────
// 11. Combined ops: 2 original + 1 insert − 1 delete = 2 after
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_commit_to_path_combined_ops_produces_correct_count() {
    let src = temp_path("combined_src");
    let dst = temp_path("combined_dst");
    std::fs::write(&src, &two_point_gpkg()).unwrap();

    let mut ed = GeoPackageEditor::open(&src, "pts").unwrap();
    ed.insert_feature(make_point_row(9.0, 9.0));
    ed.delete_feature(2).unwrap();
    let stats = ed.commit_to_path(&dst).unwrap();

    assert_eq!(stats.inserts_applied, 1);
    assert_eq!(stats.deletes_applied, 1);
    assert_eq!(stats.features_before, 2);
    assert_eq!(stats.features_after, 2); // 2 + 1 - 1 = 2

    let dst_bytes = std::fs::read(&dst).unwrap();
    assert_eq!(count_rows(&dst_bytes, "pts"), 2);

    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&dst);
}

// ─────────────────────────────────────────────────────────────────────────────
// 12. Rollback drops pending mutations; source file is untouched
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_rollback_drops_pending_without_writing() {
    let src = temp_path("rollback");
    let original_bytes = two_point_gpkg();
    std::fs::write(&src, &original_bytes).unwrap();

    let mut ed = GeoPackageEditor::open(&src, "pts").unwrap();
    ed.insert_feature(make_point_row(5.0, 6.0));
    ed.delete_feature(1).unwrap();

    // Verify mutations are buffered before rollback.
    assert_eq!(ed.pending_inserts(), 1);
    assert_eq!(ed.pending_deletes(), 1);

    ed.rollback();

    // All pending mutations cleared.
    assert_eq!(ed.pending_inserts(), 0);
    assert_eq!(ed.pending_deletes(), 0);

    // Source file is unchanged (we never wrote anything).
    let after_bytes = std::fs::read(&src).unwrap();
    assert_eq!(
        after_bytes, original_bytes,
        "source file must be untouched after rollback"
    );

    let _ = std::fs::remove_file(&src);
}
