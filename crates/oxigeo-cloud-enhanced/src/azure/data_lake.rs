//! Azure Data Lake Storage Gen2 integration.
//!
//! Backed by the `azure_storage_datalake` SDK (a real, already-enabled
//! workspace dependency), authenticated with this crate's
//! `azure_core::credentials::TokenCredential` (see [`super::AzureConfig`]).
//!
//! ## A note on the token bridging
//!
//! `azure_storage_datalake` 0.21 (and the `azure_storage`/`azure_core` 0.21
//! it is built on) predates this workspace's directly-declared
//! `azure_core = "1.0"` / `azure_identity = "1.0"`. Both major versions of
//! `azure_core` are present simultaneously in the dependency graph, each
//! with its own, mutually-incompatible `TokenCredential` trait --
//! `azure_core::auth::TokenCredential` (0.21, used internally by the
//! storage SDK) vs. `azure_core::credentials::TokenCredential` (1.x, the
//! one this crate's [`super::AzureConfig`] and every other module use).
//! Since our `Cargo.toml` cannot name the 0.21 `azure_core` directly (it is
//! only pulled in transitively, and is shadowed by our own `azure_core`
//! dependency resolving to 1.x), we cannot implement `azure_storage`'s
//! `TokenCredential` trait as an adapter over our 1.x credential.
//!
//! Instead, [`DataLakeClient`] fetches a bearer token from the 1.x
//! credential itself (`TokenCredential::get_token`) and hands the raw token
//! string to `StorageCredentials::bearer_token`, which -- being generic
//! over `impl Into<Secret>` -- never requires naming the 0.21 crate's types
//! in our own source. A fresh token is minted per SDK client construction
//! (i.e. per call) rather than cached, trading a small amount of latency
//! for the certainty that requests never carry a token past the point where
//! caching logic might have missed a refresh window.

use crate::error::{CloudEnhancedError, Result};
use azure_core::credentials::TokenCredential;
use azure_storage::prelude::StorageCredentials;
use azure_storage_datalake::clients::{DataLakeClient as SdkDataLakeClient, FileClient};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// OAuth2 scope requested for Azure Storage / Data Lake data-plane calls.
const DATA_LAKE_SCOPE: &str = "https://storage.azure.com/.default";

/// Azure Data Lake Storage Gen2 client.
#[derive(Debug, Clone)]
pub struct DataLakeClient {
    account_name: String,
    credential: Arc<dyn TokenCredential>,
}

impl DataLakeClient {
    /// Returns the account name.
    pub fn account_name(&self) -> &str {
        &self.account_name
    }

    /// Returns a reference to the credential.
    pub fn credential(&self) -> &dyn TokenCredential {
        &*self.credential
    }

    /// Builds a fresh SDK `DataLakeClient`, authenticated with a bearer
    /// token minted just-in-time from `self.credential`. See the
    /// module-level doc comment for why a token is fetched per call rather
    /// than a `TokenCredential` adapter being wired in directly.
    async fn sdk_client(&self) -> Result<SdkDataLakeClient> {
        let token = self
            .credential
            .get_token(&[DATA_LAKE_SCOPE], None)
            .await
            .map_err(|e| {
                CloudEnhancedError::authentication(format!(
                    "Failed to acquire Data Lake access token: {e}"
                ))
            })?;
        let storage_credentials =
            StorageCredentials::bearer_token(token.token.secret().to_string());
        Ok(SdkDataLakeClient::new(
            self.account_name.clone(),
            storage_credentials,
        ))
    }

    async fn file_client(&self, filesystem: &str, path: &str) -> Result<FileClient> {
        Ok(self
            .sdk_client()
            .await?
            .file_system_client(filesystem.to_string())
            .into_file_client(path.to_string()))
    }
}

impl DataLakeClient {
    /// Creates a new Data Lake client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created.
    pub fn new(config: &super::AzureConfig) -> Result<Self> {
        // Extract account name from subscription or configuration
        // In a real implementation, this would come from environment or config
        let account_name =
            std::env::var("AZURE_STORAGE_ACCOUNT").unwrap_or_else(|_| "default".to_string());
        let credential = config.credential.clone();

        Ok(Self {
            account_name,
            credential,
        })
    }

    /// Creates a filesystem (container) in Data Lake.
    ///
    /// # Errors
    ///
    /// Returns an error if the filesystem cannot be created.
    pub async fn create_filesystem(&self, filesystem_name: &str) -> Result<()> {
        tracing::info!("Creating filesystem: {}", filesystem_name);

        self.sdk_client()
            .await?
            .file_system_client(filesystem_name.to_string())
            .create()
            .into_future()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Failed to create filesystem '{filesystem_name}': {e}"
                ))
            })?;
        Ok(())
    }

    /// Deletes a filesystem from Data Lake.
    ///
    /// # Errors
    ///
    /// Returns an error if the filesystem cannot be deleted.
    pub async fn delete_filesystem(&self, filesystem_name: &str) -> Result<()> {
        tracing::info!("Deleting filesystem: {}", filesystem_name);

        self.sdk_client()
            .await?
            .file_system_client(filesystem_name.to_string())
            .delete()
            .into_future()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Failed to delete filesystem '{filesystem_name}': {e}"
                ))
            })?;
        Ok(())
    }

    /// Lists filesystems in the account.
    ///
    /// # Errors
    ///
    /// Returns an error if the filesystems cannot be listed.
    pub async fn list_filesystems(&self) -> Result<Vec<String>> {
        tracing::info!("Listing filesystems");

        let mut stream = self.sdk_client().await?.list_file_systems().into_stream();
        let mut names = Vec::new();
        while let Some(page) = stream.next().await {
            let page = page.map_err(|e| {
                CloudEnhancedError::azure_service(format!("Failed to list filesystems: {e}"))
            })?;
            names.extend(page.file_systems.into_iter().map(|fs| fs.name));
        }
        Ok(names)
    }

    /// Creates a directory in a filesystem.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created.
    pub async fn create_directory(&self, filesystem: &str, path: &str) -> Result<()> {
        tracing::info!("Creating directory: {}/{}", filesystem, path);

        self.sdk_client()
            .await?
            .file_system_client(filesystem.to_string())
            .get_directory_client(path.to_string())
            .create()
            .into_future()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Failed to create directory '{filesystem}/{path}': {e}"
                ))
            })?;
        Ok(())
    }

    /// Deletes a directory from a filesystem.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be deleted.
    pub async fn delete_directory(&self, filesystem: &str, path: &str) -> Result<()> {
        tracing::info!("Deleting directory: {}/{}", filesystem, path);

        let mut stream = self
            .sdk_client()
            .await?
            .file_system_client(filesystem.to_string())
            .get_directory_client(path.to_string())
            .delete(true)
            .into_stream();
        while let Some(page) = stream.next().await {
            page.map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Failed to delete directory '{filesystem}/{path}': {e}"
                ))
            })?;
        }
        Ok(())
    }

    /// Uploads a file to Data Lake.
    ///
    /// # Errors
    ///
    /// Returns an error if the upload fails.
    pub async fn upload_file(&self, filesystem: &str, path: &str, data: &[u8]) -> Result<()> {
        tracing::info!(
            "Uploading file: {}/{} ({} bytes)",
            filesystem,
            path,
            data.len()
        );

        let file_client = self.file_client(filesystem, path).await?;

        file_client.create().into_future().await.map_err(|e| {
            CloudEnhancedError::azure_service(format!(
                "Failed to create file '{filesystem}/{path}': {e}"
            ))
        })?;

        if !data.is_empty() {
            file_client
                .append(0, data.to_vec())
                .into_future()
                .await
                .map_err(|e| {
                    CloudEnhancedError::azure_service(format!(
                        "Failed to append data to file '{filesystem}/{path}': {e}"
                    ))
                })?;
        }

        file_client
            .flush(data.len() as i64)
            .into_future()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Failed to flush file '{filesystem}/{path}': {e}"
                ))
            })?;

        Ok(())
    }

    /// Downloads a file from Data Lake.
    ///
    /// # Errors
    ///
    /// Returns an error if the download fails.
    pub async fn download_file(&self, filesystem: &str, path: &str) -> Result<Vec<u8>> {
        tracing::info!("Downloading file: {}/{}", filesystem, path);

        let response = self
            .file_client(filesystem, path)
            .await?
            .read()
            .into_future()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Failed to download file '{filesystem}/{path}': {e}"
                ))
            })?;

        Ok(response.data.to_vec())
    }

    /// Lists paths in a directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the paths cannot be listed.
    pub async fn list_paths(
        &self,
        filesystem: &str,
        directory: Option<&str>,
        recursive: bool,
    ) -> Result<Vec<PathItem>> {
        tracing::info!(
            "Listing paths in {}/{:?} (recursive: {})",
            filesystem,
            directory,
            recursive
        );

        let fs_client = self
            .sdk_client()
            .await?
            .file_system_client(filesystem.to_string());
        let mut builder = fs_client.list_paths().recursive(recursive);
        if let Some(dir) = directory {
            builder = builder.directory(dir.to_string());
        }

        let mut stream = builder.into_stream();
        let mut items = Vec::new();
        while let Some(page) = stream.next().await {
            let page = page.map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Failed to list paths in '{filesystem}': {e}"
                ))
            })?;
            for path in page.paths {
                items.push(PathItem {
                    name: path.name,
                    is_directory: path.is_directory,
                    size: u64::try_from(path.content_length).unwrap_or(0),
                    last_modified: offset_date_time_to_chrono(path.last_modified),
                    etag: Some(path.etag.to_string()),
                });
            }
        }
        Ok(items)
    }

    /// Gets file properties.
    ///
    /// # Errors
    ///
    /// Returns an error if the properties cannot be retrieved.
    pub async fn get_file_properties(
        &self,
        filesystem: &str,
        path: &str,
    ) -> Result<FileProperties> {
        tracing::info!("Getting file properties: {}/{}", filesystem, path);

        let response = self
            .file_client(filesystem, path)
            .await?
            .get_properties()
            .into_future()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Failed to get properties for '{filesystem}/{path}': {e}"
                ))
            })?;

        Ok(FileProperties {
            name: path.to_string(),
            size: response
                .content_length
                .and_then(|n| u64::try_from(n).ok())
                .unwrap_or(0),
            last_modified: offset_date_time_to_chrono(response.last_modified),
            etag: Some(response.etag),
            content_type: response.content_type,
        })
    }

    /// Sets file metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if the metadata cannot be set.
    pub async fn set_file_metadata(
        &self,
        filesystem: &str,
        path: &str,
        metadata: std::collections::HashMap<String, String>,
    ) -> Result<()> {
        tracing::info!(
            "Setting file metadata: {}/{} ({} items)",
            filesystem,
            path,
            metadata.len()
        );

        let mut properties = azure_storage_datalake::Properties::new();
        for (key, value) in metadata {
            properties.insert(key, value);
        }

        self.file_client(filesystem, path)
            .await?
            .set_properties(properties)
            .into_future()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Failed to set metadata for '{filesystem}/{path}': {e}"
                ))
            })?;
        Ok(())
    }

    /// Renames or moves a file.
    ///
    /// # Errors
    ///
    /// Returns an error if the rename fails.
    pub async fn rename_file(
        &self,
        filesystem: &str,
        source_path: &str,
        destination_path: &str,
    ) -> Result<()> {
        tracing::info!(
            "Renaming file: {}/{} -> {}",
            filesystem,
            source_path,
            destination_path
        );

        self.file_client(filesystem, source_path)
            .await?
            .rename(destination_path.to_string())
            .into_future()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Failed to rename '{filesystem}/{source_path}' to '{destination_path}': {e}"
                ))
            })?;
        Ok(())
    }

    /// Sets access control list (ACL) for a path.
    ///
    /// # Errors
    ///
    /// Returns an error if the ACL cannot be set.
    pub async fn set_acl(&self, filesystem: &str, path: &str, acl: Vec<AclEntry>) -> Result<()> {
        tracing::info!(
            "Setting ACL for: {}/{} ({} entries)",
            filesystem,
            path,
            acl.len()
        );

        let acl_string = format_acl(&acl);

        self.file_client(filesystem, path)
            .await?
            .set_access_control_list(acl_string)
            .into_future()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Failed to set ACL for '{filesystem}/{path}': {e}"
                ))
            })?;
        Ok(())
    }

    /// Gets access control list (ACL) for a path.
    ///
    /// # Errors
    ///
    /// Returns an error if the ACL cannot be retrieved.
    pub async fn get_acl(&self, filesystem: &str, path: &str) -> Result<Vec<AclEntry>> {
        tracing::info!("Getting ACL for: {}/{}", filesystem, path);

        let response = self
            .file_client(filesystem, path)
            .await?
            .get_access_control_list()
            .into_future()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Failed to get ACL for '{filesystem}/{path}': {e}"
                ))
            })?;

        match response.acl {
            Some(acl_string) => parse_acl(&acl_string),
            None => Ok(vec![]),
        }
    }

    /// Appends data to a file.
    ///
    /// # Errors
    ///
    /// Returns an error if the append fails.
    pub async fn append_file(
        &self,
        filesystem: &str,
        path: &str,
        data: &[u8],
        position: u64,
    ) -> Result<()> {
        tracing::info!(
            "Appending to file: {}/{} at position {} ({} bytes)",
            filesystem,
            path,
            position,
            data.len()
        );

        self.file_client(filesystem, path)
            .await?
            .append(i64::try_from(position).unwrap_or(i64::MAX), data.to_vec())
            .into_future()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Failed to append to '{filesystem}/{path}': {e}"
                ))
            })?;
        Ok(())
    }

    /// Flushes data to a file.
    ///
    /// # Errors
    ///
    /// Returns an error if the flush fails.
    pub async fn flush_file(&self, filesystem: &str, path: &str, position: u64) -> Result<()> {
        tracing::info!(
            "Flushing file: {}/{} at position {}",
            filesystem,
            path,
            position
        );

        self.file_client(filesystem, path)
            .await?
            .flush(i64::try_from(position).unwrap_or(i64::MAX))
            .into_future()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Failed to flush '{filesystem}/{path}': {e}"
                ))
            })?;
        Ok(())
    }
}

/// Converts a `time::OffsetDateTime` (used by `azure_storage_datalake`) into
/// a `chrono::DateTime<Utc>`, falling back to the current time if the
/// timestamp is somehow out of chrono's representable range.
fn offset_date_time_to_chrono(dt: time::OffsetDateTime) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(dt.unix_timestamp(), dt.nanosecond())
        .unwrap_or_else(chrono::Utc::now)
}

/// Formats a list of [`AclEntry`] values as an ADLS Gen2 ACL string, e.g.
/// `"user::rwx,group::r-x,other::---"`.
fn format_acl(entries: &[AclEntry]) -> String {
    entries
        .iter()
        .map(|entry| {
            let scope_prefix = match entry.scope {
                AclScope::Default => "default:",
                AclScope::Access => "",
            };
            let principal = entry.principal_id.as_deref().unwrap_or("");
            format!(
                "{scope_prefix}{}:{principal}:{}",
                entry.acl_type, entry.permissions
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Parses an ADLS Gen2 ACL string (as returned by the `getAccessControl`
/// operation) into a list of [`AclEntry`] values.
fn parse_acl(acl_string: &str) -> Result<Vec<AclEntry>> {
    acl_string
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|entry| {
            let (scope, rest) = match entry.strip_prefix("default:") {
                Some(rest) => (AclScope::Default, rest),
                None => (AclScope::Access, entry),
            };

            let mut parts = rest.splitn(3, ':');
            let acl_type_str = parts.next().unwrap_or("");
            let principal_id = parts.next().unwrap_or("");
            let permissions = parts.next().unwrap_or("");

            let acl_type = match acl_type_str {
                "user" => AclType::User,
                "group" => AclType::Group,
                "other" => AclType::Other,
                "mask" => AclType::Mask,
                other => {
                    return Err(CloudEnhancedError::azure_service(format!(
                        "Unrecognized ACL entry type '{other}' in ACL string '{acl_string}'"
                    )));
                }
            };

            Ok(AclEntry {
                scope,
                acl_type,
                principal_id: if principal_id.is_empty() {
                    None
                } else {
                    Some(principal_id.to_string())
                },
                permissions: permissions.to_string(),
            })
        })
        .collect()
}

/// Path item in Data Lake.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathItem {
    /// Path name
    pub name: String,
    /// Is directory
    pub is_directory: bool,
    /// Size in bytes
    pub size: u64,
    /// Last modified time
    pub last_modified: chrono::DateTime<chrono::Utc>,
    /// ETag
    pub etag: Option<String>,
}

/// File properties.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileProperties {
    /// File name
    pub name: String,
    /// Size in bytes
    pub size: u64,
    /// Last modified time
    pub last_modified: chrono::DateTime<chrono::Utc>,
    /// ETag
    pub etag: Option<String>,
    /// Content type
    pub content_type: Option<String>,
}

/// ACL entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AclEntry {
    /// ACL scope (access or default)
    pub scope: AclScope,
    /// ACL type (user, group, other)
    pub acl_type: AclType,
    /// Principal ID (for user/group)
    pub principal_id: Option<String>,
    /// Permissions (read, write, execute)
    pub permissions: String,
}

/// ACL scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AclScope {
    /// Access ACL
    Access,
    /// Default ACL
    Default,
}

/// ACL type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AclType {
    /// User
    User,
    /// Group
    Group,
    /// Other
    Other,
    /// Mask
    Mask,
}

impl std::fmt::Display for AclScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Access => write!(f, "access"),
            Self::Default => write!(f, "default"),
        }
    }
}

impl std::fmt::Display for AclType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::Group => write!(f, "group"),
            Self::Other => write!(f, "other"),
            Self::Mask => write!(f, "mask"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_item() {
        let item = PathItem {
            name: "test.txt".to_string(),
            is_directory: false,
            size: 1024,
            last_modified: chrono::Utc::now(),
            etag: Some("abc123".to_string()),
        };

        assert_eq!(item.name, "test.txt");
        assert!(!item.is_directory);
        assert_eq!(item.size, 1024);
    }

    #[test]
    fn test_acl_entry() {
        let acl = AclEntry {
            scope: AclScope::Access,
            acl_type: AclType::User,
            principal_id: Some("user123".to_string()),
            permissions: "rwx".to_string(),
        };

        assert_eq!(acl.scope, AclScope::Access);
        assert_eq!(acl.acl_type, AclType::User);
        assert_eq!(acl.permissions, "rwx");
    }

    #[test]
    fn test_format_acl_round_trips_through_parse_acl() {
        let entries = vec![
            AclEntry {
                scope: AclScope::Access,
                acl_type: AclType::User,
                principal_id: None,
                permissions: "rwx".to_string(),
            },
            AclEntry {
                scope: AclScope::Access,
                acl_type: AclType::Group,
                principal_id: None,
                permissions: "r-x".to_string(),
            },
            AclEntry {
                scope: AclScope::Access,
                acl_type: AclType::Other,
                principal_id: None,
                permissions: "---".to_string(),
            },
            AclEntry {
                scope: AclScope::Default,
                acl_type: AclType::User,
                principal_id: Some("11111111-1111-1111-1111-111111111111".to_string()),
                permissions: "rwx".to_string(),
            },
        ];

        let formatted = format_acl(&entries);
        assert_eq!(
            formatted,
            "user::rwx,group::r-x,other::---,default:user:11111111-1111-1111-1111-111111111111:rwx"
        );

        let parsed = parse_acl(&formatted).expect("parse formatted ACL");
        assert_eq!(parsed.len(), entries.len());
        assert_eq!(parsed[0].acl_type, AclType::User);
        assert_eq!(parsed[0].scope, AclScope::Access);
        assert_eq!(parsed[0].permissions, "rwx");
        assert_eq!(parsed[3].scope, AclScope::Default);
        assert_eq!(
            parsed[3].principal_id.as_deref(),
            Some("11111111-1111-1111-1111-111111111111")
        );
    }

    #[test]
    fn test_parse_acl_rejects_unknown_type() {
        let result = parse_acl("bogus::rwx");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_acl_empty_string_yields_empty_vec() {
        let parsed = parse_acl("").expect("parse empty ACL");
        assert!(parsed.is_empty());
    }
}
