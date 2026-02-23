//! Backup tests.

use oxigdal_ha::backup::{
    BackupCompression, BackupType, differential::DifferentialBackup, full::FullBackup,
    incremental::IncrementalBackup,
};
use uuid::Uuid;

#[tokio::test]
async fn test_full_backup() {
    let backup_dir = std::env::temp_dir().join("oxigdal-ha-test-full-backup");
    tokio::fs::create_dir_all(&backup_dir).await.ok();

    let backup = FullBackup::new(backup_dir, BackupCompression::Zstd);

    let metadata = backup.create().await.ok();
    assert!(metadata.is_some());

    let metadata = metadata.expect("full backup creation should return metadata");
    assert_eq!(metadata.backup_type, BackupType::Full);
    assert_eq!(metadata.compression, BackupCompression::Zstd);
}

#[tokio::test]
async fn test_incremental_backup() {
    let backup_dir = std::env::temp_dir().join("oxigdal-ha-test-incr-backup");
    tokio::fs::create_dir_all(&backup_dir).await.ok();

    let backup = IncrementalBackup::new(backup_dir, BackupCompression::Lz4);

    let parent_id = Uuid::new_v4();
    let metadata = backup.create(Some(parent_id)).await.ok();
    assert!(metadata.is_some());

    let metadata = metadata.expect("incremental backup creation should return metadata");
    assert_eq!(metadata.backup_type, BackupType::Incremental);
    assert_eq!(metadata.parent_id, Some(parent_id));
}

#[tokio::test]
async fn test_differential_backup() {
    let backup_dir = std::env::temp_dir().join("oxigdal-ha-test-diff-backup");
    tokio::fs::create_dir_all(&backup_dir).await.ok();

    let backup = DifferentialBackup::new(backup_dir, BackupCompression::Gzip);

    let full_backup_id = Uuid::new_v4();
    let metadata = backup.create(full_backup_id).await.ok();
    assert!(metadata.is_some());

    let metadata = metadata.expect("differential backup creation should return metadata");
    assert_eq!(metadata.backup_type, BackupType::Differential);
    assert_eq!(metadata.parent_id, Some(full_backup_id));
}
