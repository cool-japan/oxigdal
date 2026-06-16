//! Integration tests for the trigger-based change tracking feature.
//!
//! All tests are compiled and run only when the `change-tracking` Cargo
//! feature is enabled.

#[cfg(feature = "change-tracking")]
mod tests {
    use oxigdal_gpkg::{ChangeOperation, ChangeTracker};
    use oxisql_core::ToSqlValue;

    // ── helpers ───────────────────────────────────────────────────────────────

    /// Open an in-memory tracker and create a `features` table on its
    /// connection so that trigger DDL has a target to attach to.
    fn make_tracker_with_features_table() -> Result<ChangeTracker, Box<dyn std::error::Error>> {
        let tracker = ChangeTracker::open_in_memory()?;
        tracker
            .connection()
            .execute_batch("CREATE TABLE features (fid INTEGER PRIMARY KEY, name TEXT);")?;
        Ok(tracker)
    }

    // ── test 1 ────────────────────────────────────────────────────────────────

    #[test]
    fn test_create_changes_table() -> Result<(), Box<dyn std::error::Error>> {
        let tracker = ChangeTracker::open_in_memory()?;
        tracker.create_changes_table()?;
        // Calling it a second time must also succeed (IF NOT EXISTS).
        tracker.create_changes_table()?;
        Ok(())
    }

    // ── test 2 ────────────────────────────────────────────────────────────────

    #[test]
    #[ignore = "Limbo/oxisqlite engine does not yet support CREATE TRIGGER DDL"]
    fn test_enable_tracking_creates_three_triggers() -> Result<(), Box<dyn std::error::Error>> {
        let tracker = make_tracker_with_features_table()?;
        tracker.enable_tracking("features", "fid")?;

        let rows = tracker.connection().query(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger'",
            &[],
        )?;
        let count = rows
            .first()
            .ok_or("no row returned from COUNT(*)")?
            .try_get_by_index::<i64>(0)?;

        assert_eq!(
            count, 3,
            "expected exactly three triggers after enable_tracking"
        );
        Ok(())
    }

    // ── test 3 ────────────────────────────────────────────────────────────────

    #[test]
    #[ignore = "Limbo/oxisqlite engine does not yet support CREATE TRIGGER DDL"]
    fn test_disable_tracking_drops_triggers() -> Result<(), Box<dyn std::error::Error>> {
        let tracker = make_tracker_with_features_table()?;
        tracker.enable_tracking("features", "fid")?;
        tracker.disable_tracking("features")?;

        let is_still_tracked = tracker.is_tracking("features")?;

        assert!(
            !is_still_tracked,
            "tracking should be disabled after disable_tracking"
        );
        Ok(())
    }

    // ── test 4 ────────────────────────────────────────────────────────────────

    #[test]
    #[ignore = "Limbo/oxisqlite engine does not yet support CREATE TRIGGER DDL"]
    fn test_is_tracking_returns_true_when_enabled() -> Result<(), Box<dyn std::error::Error>> {
        let tracker = make_tracker_with_features_table()?;
        tracker.enable_tracking("features", "fid")?;

        let result = tracker.is_tracking("features")?;

        assert!(
            result,
            "is_tracking should return true after enable_tracking"
        );
        Ok(())
    }

    // ── test 5 ────────────────────────────────────────────────────────────────

    #[test]
    #[ignore = "Limbo/oxisqlite engine does not yet support CREATE TRIGGER DDL"]
    fn test_is_tracking_returns_false_when_disabled() -> Result<(), Box<dyn std::error::Error>> {
        let tracker = make_tracker_with_features_table()?;
        tracker.enable_tracking("features", "fid")?;
        tracker.disable_tracking("features")?;

        let result = tracker.is_tracking("features")?;

        assert!(
            !result,
            "is_tracking should return false after disable_tracking"
        );
        Ok(())
    }

    // ── test 6 ────────────────────────────────────────────────────────────────

    #[test]
    #[ignore = "Limbo/oxisqlite engine does not yet fire AFTER INSERT/UPDATE/DELETE triggers"]
    fn test_insert_logged_with_operation_1() -> Result<(), Box<dyn std::error::Error>> {
        let tracker = make_tracker_with_features_table()?;
        tracker.enable_tracking("features", "fid")?;

        tracker.connection().execute(
            "INSERT INTO features (fid, name) VALUES ($1, $2)",
            &[&1i64 as &dyn ToSqlValue, &"alpha" as &dyn ToSqlValue],
        )?;

        let changes = tracker.get_all_changes("features")?;

        assert_eq!(changes.len(), 1, "expected one change entry after INSERT");
        assert_eq!(
            changes[0].operation,
            ChangeOperation::Insert,
            "expected operation Insert (1)"
        );
        assert_eq!(changes[0].feature_id, 1);
        assert_eq!(changes[0].table_name, "features");
        Ok(())
    }

    // ── test 7 ────────────────────────────────────────────────────────────────

    #[test]
    #[ignore = "Limbo/oxisqlite engine does not yet fire AFTER INSERT/UPDATE/DELETE triggers"]
    fn test_update_logged_with_operation_2() -> Result<(), Box<dyn std::error::Error>> {
        let tracker = make_tracker_with_features_table()?;
        tracker.enable_tracking("features", "fid")?;

        let conn = tracker.connection();
        conn.execute(
            "INSERT INTO features (fid, name) VALUES ($1, $2)",
            &[&42i64 as &dyn ToSqlValue, &"beta" as &dyn ToSqlValue],
        )?;
        conn.execute(
            "UPDATE features SET name=$1 WHERE fid=$2",
            &[
                &"beta-updated" as &dyn ToSqlValue,
                &42i64 as &dyn ToSqlValue,
            ],
        )?;

        let changes = tracker.get_all_changes("features")?;

        assert_eq!(
            changes.len(),
            2,
            "expected two change entries (insert + update)"
        );
        assert_eq!(
            changes[1].operation,
            ChangeOperation::Update,
            "last entry should be Update (2)"
        );
        assert_eq!(changes[1].feature_id, 42);
        Ok(())
    }

    // ── test 8 ────────────────────────────────────────────────────────────────

    #[test]
    #[ignore = "Limbo/oxisqlite engine does not yet fire AFTER INSERT/UPDATE/DELETE triggers"]
    fn test_delete_logged_with_operation_3() -> Result<(), Box<dyn std::error::Error>> {
        let tracker = make_tracker_with_features_table()?;
        tracker.enable_tracking("features", "fid")?;

        let conn = tracker.connection();
        conn.execute(
            "INSERT INTO features (fid, name) VALUES ($1, $2)",
            &[&7i64 as &dyn ToSqlValue, &"gamma" as &dyn ToSqlValue],
        )?;
        conn.execute(
            "DELETE FROM features WHERE fid=$1",
            &[&7i64 as &dyn ToSqlValue],
        )?;

        let changes = tracker.get_all_changes("features")?;

        assert_eq!(
            changes.len(),
            2,
            "expected two change entries (insert + delete)"
        );
        assert_eq!(
            changes[1].operation,
            ChangeOperation::Delete,
            "last entry should be Delete (3)"
        );
        assert_eq!(changes[1].feature_id, 7);
        Ok(())
    }

    // ── test 9 ────────────────────────────────────────────────────────────────

    #[test]
    #[ignore = "Limbo/oxisqlite engine does not yet fire AFTER INSERT/UPDATE/DELETE triggers"]
    fn test_get_changes_since_filters_by_id() -> Result<(), Box<dyn std::error::Error>> {
        let tracker = make_tracker_with_features_table()?;
        tracker.enable_tracking("features", "fid")?;

        let conn = tracker.connection();
        for fid in 1i64..=3 {
            conn.execute(
                "INSERT INTO features (fid, name) VALUES ($1, $2)",
                &[
                    &fid as &dyn ToSqlValue,
                    &format!("row-{fid}") as &dyn ToSqlValue,
                ],
            )?;
        }

        // After 3 inserts the ids in gpkg_changes are 1, 2, 3.
        // get_changes_since(id=1) should return ids 2 and 3.
        let changes = tracker.get_changes_since("features", 1)?;

        assert_eq!(changes.len(), 2, "expected 2 entries with id > 1");
        assert!(
            changes.iter().all(|e| e.id > 1),
            "all returned ids must be > 1"
        );
        Ok(())
    }

    // ── test 10 ───────────────────────────────────────────────────────────────

    #[test]
    #[ignore = "Limbo/oxisqlite engine does not yet fire AFTER INSERT/UPDATE/DELETE triggers"]
    fn test_clear_changes_for_table() -> Result<(), Box<dyn std::error::Error>> {
        let tracker = make_tracker_with_features_table()?;
        tracker.enable_tracking("features", "fid")?;

        let conn = tracker.connection();
        conn.execute(
            "INSERT INTO features (fid, name) VALUES ($1, $2)",
            &[&1i64 as &dyn ToSqlValue, &"a" as &dyn ToSqlValue],
        )?;
        conn.execute(
            "INSERT INTO features (fid, name) VALUES ($1, $2)",
            &[&2i64 as &dyn ToSqlValue, &"b" as &dyn ToSqlValue],
        )?;

        let removed = tracker.clear_changes("features")?;
        assert_eq!(removed, 2, "expected 2 rows removed");

        let remaining = tracker.get_all_changes("features")?;
        assert!(
            remaining.is_empty(),
            "expected no changes after clear_changes"
        );
        Ok(())
    }

    // ── test 11 ───────────────────────────────────────────────────────────────

    #[test]
    #[ignore = "Limbo/oxisqlite engine does not yet fire AFTER INSERT/UPDATE/DELETE triggers"]
    fn test_clear_all_changes() -> Result<(), Box<dyn std::error::Error>> {
        let tracker = ChangeTracker::open_in_memory()?;

        let conn = tracker.connection();
        conn.execute_batch(
            "CREATE TABLE t1 (id INTEGER PRIMARY KEY, v TEXT);
             CREATE TABLE t2 (id INTEGER PRIMARY KEY, v TEXT);",
        )?;

        tracker.enable_tracking("t1", "id")?;
        tracker.enable_tracking("t2", "id")?;

        conn.execute(
            "INSERT INTO t1 (id, v) VALUES ($1, $2)",
            &[&1i64 as &dyn ToSqlValue, &"x" as &dyn ToSqlValue],
        )?;
        conn.execute(
            "INSERT INTO t2 (id, v) VALUES ($1, $2)",
            &[&1i64 as &dyn ToSqlValue, &"y" as &dyn ToSqlValue],
        )?;

        let total_removed = tracker.clear_all_changes()?;
        assert_eq!(total_removed, 2, "expected 2 total rows removed");

        let t1_changes = tracker.get_all_changes("t1")?;
        let t2_changes = tracker.get_all_changes("t2")?;
        assert!(
            t1_changes.is_empty(),
            "t1 changes should be empty after clear_all"
        );
        assert!(
            t2_changes.is_empty(),
            "t2 changes should be empty after clear_all"
        );
        Ok(())
    }

    // ── test 12 ───────────────────────────────────────────────────────────────

    #[test]
    #[ignore = "Limbo/oxisqlite engine does not yet support CREATE TRIGGER DDL"]
    fn test_tracked_tables_lists_enabled() -> Result<(), Box<dyn std::error::Error>> {
        let tracker = ChangeTracker::open_in_memory()?;

        let conn = tracker.connection();
        conn.execute_batch(
            "CREATE TABLE rivers  (fid INTEGER PRIMARY KEY, name TEXT);
             CREATE TABLE parcels (fid INTEGER PRIMARY KEY, area REAL);",
        )?;

        tracker.enable_tracking("rivers", "fid")?;
        tracker.enable_tracking("parcels", "fid")?;

        let mut tables = tracker.tracked_tables()?;
        tables.sort(); // order is non-deterministic in sqlite_master

        assert_eq!(
            tables,
            vec!["parcels".to_owned(), "rivers".to_owned()],
            "tracked_tables should list both enabled tables"
        );
        Ok(())
    }
}
