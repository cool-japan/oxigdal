//! Azure Managed Identity integration.
//!
//! The identity/federated-credential *management* surface below
//! (`create_user_assigned_identity`, `list_user_assigned_identities`, ...)
//! talks to the real Azure Resource Manager (ARM) control plane
//! (`management.azure.com`) over `reqwest`, authenticated with this crate's
//! `azure_core::credentials::TokenCredential` (see [`super::AzureConfig`]).
//! `get_token` is a separate concern: it mints *data-plane* tokens for a
//! downstream resource via the VM's attached managed identity
//! (`ManagedIdentityCredential`), independent of whichever credential is
//! used to manage ARM resources.

use crate::error::{CloudEnhancedError, Result};
use azure_core::credentials::TokenCredential;
use azure_identity::ManagedIdentityCredential;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Default base URL of the Azure Resource Manager control plane.
const DEFAULT_ARM_BASE_URL: &str = "https://management.azure.com";

/// API version for the `Microsoft.ManagedIdentity` resource provider
/// (user-assigned identities and federated identity credentials).
const MSI_API_VERSION: &str = "2023-01-31";

/// API version used for generic-resource `PATCH` calls when assigning /
/// removing a user-assigned identity from an arbitrary resource.
const GENERIC_RESOURCE_API_VERSION: &str = "2021-04-01";

/// Converts a resource identifier (e.g. `https://storage.azure.com`) into an
/// Entra ID OAuth2 scope (e.g. `https://storage.azure.com/.default`).
///
/// If `resource` is already scope-formatted (ends with `/.default`), it is
/// returned unchanged.
fn resource_to_scope(resource: &str) -> String {
    if resource.ends_with("/.default") {
        return resource.to_string();
    }
    let trimmed = resource.trim_end_matches('/');
    format!("{trimmed}/.default")
}

/// Managed Identity client.
#[derive(Debug, Clone)]
pub struct ManagedIdentityClient {
    subscription_id: String,
    /// Base URL of the Azure Resource Manager control plane (overridable
    /// for tests).
    arm_base_url: String,
    /// Credential used to authenticate ARM management-plane calls (distinct
    /// from the `ManagedIdentityCredential` used by [`Self::get_token`]).
    credential: Arc<dyn TokenCredential>,
    http_client: reqwest::Client,
}

impl ManagedIdentityClient {
    /// Creates a new Managed Identity client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created.
    pub fn new(config: &super::AzureConfig) -> Result<Self> {
        Self::with_arm_base_url(config, DEFAULT_ARM_BASE_URL)
    }

    /// Creates a new Managed Identity client pointed at a custom ARM base
    /// URL.
    ///
    /// This is primarily intended for tests, which spin up a local mock ARM
    /// server rather than talking to the real `management.azure.com`
    /// endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn with_arm_base_url(
        config: &super::AzureConfig,
        arm_base_url: impl Into<String>,
    ) -> Result<Self> {
        let http_client = reqwest::Client::builder().build().map_err(|e| {
            CloudEnhancedError::configuration(format!("Failed to build HTTP client: {e}"))
        })?;

        Ok(Self {
            subscription_id: config.subscription_id().to_string(),
            arm_base_url: arm_base_url.into(),
            credential: config.credential.clone(),
            http_client,
        })
    }

    /// Obtains a bearer token for authenticating ARM management-plane
    /// calls.
    async fn arm_bearer_token(&self) -> Result<String> {
        let token = self
            .credential
            .get_token(&["https://management.azure.com/.default"], None)
            .await
            .map_err(|e| {
                CloudEnhancedError::authentication(format!(
                    "Failed to acquire Azure Resource Manager token: {e}"
                ))
            })?;
        Ok(token.token.secret().to_string())
    }

    /// Builds the ARM resource path for a user-assigned identity.
    fn identity_path(&self, resource_group: &str, identity_name: &str) -> String {
        format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{}",
            self.subscription_id, resource_group, identity_name
        )
    }

    /// Gets an access token for a specific resource using managed identity.
    ///
    /// # Errors
    ///
    /// Returns an error if the token cannot be retrieved.
    pub async fn get_token(&self, resource: &str) -> Result<AccessToken> {
        tracing::info!("Getting access token for resource: {}", resource);

        // `ManagedIdentityCredential` drives the IMDS (or App Service/Service
        // Fabric) managed identity flow depending on the runtime environment.
        let credential = ManagedIdentityCredential::new(None).map_err(|e| {
            CloudEnhancedError::authentication(format!("Failed to create credential: {}", e))
        })?;

        let scope = resource_to_scope(resource);
        let token = credential
            .get_token(&[scope.as_str()], None)
            .await
            .map_err(|e| {
                CloudEnhancedError::authentication(format!(
                    "Failed to acquire managed identity token for resource '{resource}': {e}"
                ))
            })?;

        let expires_on = chrono::DateTime::from_timestamp(token.expires_on.unix_timestamp(), 0)
            .ok_or_else(|| {
                CloudEnhancedError::authentication(
                    "Managed identity token has an out-of-range expiry timestamp".to_string(),
                )
            })?;

        Ok(AccessToken {
            token: token.token.secret().to_string(),
            expires_on,
        })
    }

    /// Creates a user-assigned managed identity.
    ///
    /// # Errors
    ///
    /// Returns an error if the identity cannot be created.
    pub async fn create_user_assigned_identity(
        &self,
        resource_group: &str,
        identity_name: &str,
        location: &str,
    ) -> Result<String> {
        tracing::info!(
            "Creating user-assigned identity: {} in resource group: {} (location: {})",
            identity_name,
            resource_group,
            location
        );

        let token = self.arm_bearer_token().await?;
        let url = format!(
            "{}{}?api-version={MSI_API_VERSION}",
            self.arm_base_url,
            self.identity_path(resource_group, identity_name)
        );

        let response = self
            .http_client
            .put(&url)
            .bearer_auth(&token)
            .json(&serde_json::json!({ "location": location }))
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "ARM userAssignedIdentities PUT request failed: {e}"
                ))
            })?;

        let resource: ArmIdentityResource =
            parse_arm_response(response, "create user-assigned identity").await?;
        Ok(resource.id)
    }

    /// Deletes a user-assigned managed identity.
    ///
    /// # Errors
    ///
    /// Returns an error if the identity cannot be deleted.
    pub async fn delete_user_assigned_identity(
        &self,
        resource_group: &str,
        identity_name: &str,
    ) -> Result<()> {
        tracing::info!(
            "Deleting user-assigned identity: {} from resource group: {}",
            identity_name,
            resource_group
        );

        let token = self.arm_bearer_token().await?;
        let url = format!(
            "{}{}?api-version={MSI_API_VERSION}",
            self.arm_base_url,
            self.identity_path(resource_group, identity_name)
        );

        let response = self
            .http_client
            .delete(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "ARM userAssignedIdentities DELETE request failed: {e}"
                ))
            })?;

        ensure_arm_success(response, "delete user-assigned identity").await?;
        Ok(())
    }

    /// Lists user-assigned managed identities in a resource group.
    ///
    /// # Errors
    ///
    /// Returns an error if the identities cannot be listed.
    pub async fn list_user_assigned_identities(
        &self,
        resource_group: &str,
    ) -> Result<Vec<IdentityInfo>> {
        tracing::info!(
            "Listing user-assigned identities in resource group: {}",
            resource_group
        );

        let token = self.arm_bearer_token().await?;
        let url = format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ManagedIdentity/userAssignedIdentities?api-version={MSI_API_VERSION}",
            self.arm_base_url, self.subscription_id, resource_group
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "ARM userAssignedIdentities LIST request failed: {e}"
                ))
            })?;

        let body: ArmListResponse<ArmIdentityResource> =
            parse_arm_response(response, "list user-assigned identities").await?;
        Ok(body.value.into_iter().map(Into::into).collect())
    }

    /// Gets details of a user-assigned managed identity.
    ///
    /// # Errors
    ///
    /// Returns an error if the identity cannot be retrieved.
    pub async fn get_user_assigned_identity(
        &self,
        resource_group: &str,
        identity_name: &str,
    ) -> Result<IdentityInfo> {
        tracing::info!(
            "Getting user-assigned identity: {} from resource group: {}",
            identity_name,
            resource_group
        );

        let token = self.arm_bearer_token().await?;
        let url = format!(
            "{}{}?api-version={MSI_API_VERSION}",
            self.arm_base_url,
            self.identity_path(resource_group, identity_name)
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "ARM userAssignedIdentities GET request failed: {e}"
                ))
            })?;

        let resource: ArmIdentityResource =
            parse_arm_response(response, "get user-assigned identity").await?;
        Ok(resource.into())
    }

    /// Assigns a managed identity to a resource, via a generic-resource ARM
    /// `PATCH` that adds `identity_id` to the target resource's
    /// `identity.userAssignedIdentities` map.
    ///
    /// # Errors
    ///
    /// Returns an error if the assignment fails.
    pub async fn assign_identity_to_resource(
        &self,
        resource_id: &str,
        identity_id: &str,
    ) -> Result<()> {
        tracing::info!(
            "Assigning identity {} to resource: {}",
            identity_id,
            resource_id
        );

        let token = self.arm_bearer_token().await?;
        let url = format!(
            "{}{resource_id}?api-version={GENERIC_RESOURCE_API_VERSION}",
            self.arm_base_url
        );

        let response = self
            .http_client
            .patch(&url)
            .bearer_auth(&token)
            .json(&serde_json::json!({
                "identity": {
                    "type": "UserAssigned",
                    "userAssignedIdentities": { (identity_id): {} }
                }
            }))
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "ARM generic-resource PATCH (assign identity) request failed: {e}"
                ))
            })?;

        ensure_arm_success(response, "assign identity to resource").await?;
        Ok(())
    }

    /// Removes a managed identity from a resource, via a generic-resource
    /// ARM `PATCH` with a `null` entry for `identity_id`, which ARM
    /// interprets as "remove this user-assigned identity".
    ///
    /// # Errors
    ///
    /// Returns an error if the removal fails.
    pub async fn remove_identity_from_resource(
        &self,
        resource_id: &str,
        identity_id: &str,
    ) -> Result<()> {
        tracing::info!(
            "Removing identity {} from resource: {}",
            identity_id,
            resource_id
        );

        let token = self.arm_bearer_token().await?;
        let url = format!(
            "{}{resource_id}?api-version={GENERIC_RESOURCE_API_VERSION}",
            self.arm_base_url
        );

        let response = self
            .http_client
            .patch(&url)
            .bearer_auth(&token)
            .json(&serde_json::json!({
                "identity": {
                    "type": "UserAssigned",
                    "userAssignedIdentities": { (identity_id): serde_json::Value::Null }
                }
            }))
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "ARM generic-resource PATCH (remove identity) request failed: {e}"
                ))
            })?;

        ensure_arm_success(response, "remove identity from resource").await?;
        Ok(())
    }

    /// Creates a federated identity credential for OIDC.
    ///
    /// # Errors
    ///
    /// Returns an error if the credential cannot be created.
    pub async fn create_federated_credential(
        &self,
        resource_group: &str,
        identity_name: &str,
        credential_name: &str,
        issuer: &str,
        subject: &str,
        audiences: Vec<String>,
    ) -> Result<()> {
        tracing::info!(
            "Creating federated credential: {} for identity: {} (issuer: {}, subject: {})",
            credential_name,
            identity_name,
            issuer,
            subject
        );

        let token = self.arm_bearer_token().await?;
        let url = format!(
            "{}{}/federatedIdentityCredentials/{credential_name}?api-version={MSI_API_VERSION}",
            self.arm_base_url,
            self.identity_path(resource_group, identity_name)
        );

        let body = FederatedCredentialCreateRequest {
            properties: FederatedCredentialProperties {
                issuer: issuer.to_string(),
                subject: subject.to_string(),
                audiences,
            },
        };

        let response = self
            .http_client
            .put(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "ARM federatedIdentityCredentials PUT request failed: {e}"
                ))
            })?;

        ensure_arm_success(response, "create federated credential").await?;
        Ok(())
    }

    /// Deletes a federated identity credential.
    ///
    /// # Errors
    ///
    /// Returns an error if the credential cannot be deleted.
    pub async fn delete_federated_credential(
        &self,
        resource_group: &str,
        identity_name: &str,
        credential_name: &str,
    ) -> Result<()> {
        tracing::info!(
            "Deleting federated credential: {} from identity: {}",
            credential_name,
            identity_name
        );

        let token = self.arm_bearer_token().await?;
        let url = format!(
            "{}{}/federatedIdentityCredentials/{credential_name}?api-version={MSI_API_VERSION}",
            self.arm_base_url,
            self.identity_path(resource_group, identity_name)
        );

        let response = self
            .http_client
            .delete(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "ARM federatedIdentityCredentials DELETE request failed: {e}"
                ))
            })?;

        ensure_arm_success(response, "delete federated credential").await?;
        Ok(())
    }

    /// Lists federated credentials for an identity.
    ///
    /// # Errors
    ///
    /// Returns an error if the credentials cannot be listed.
    pub async fn list_federated_credentials(
        &self,
        resource_group: &str,
        identity_name: &str,
    ) -> Result<Vec<FederatedCredentialInfo>> {
        tracing::info!(
            "Listing federated credentials for identity: {}",
            identity_name
        );

        let token = self.arm_bearer_token().await?;
        let url = format!(
            "{}{}/federatedIdentityCredentials?api-version={MSI_API_VERSION}",
            self.arm_base_url,
            self.identity_path(resource_group, identity_name)
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "ARM federatedIdentityCredentials LIST request failed: {e}"
                ))
            })?;

        let body: ArmListResponse<ArmFederatedCredentialResource> =
            parse_arm_response(response, "list federated credentials").await?;
        Ok(body.value.into_iter().map(Into::into).collect())
    }
}

/// Verifies that `response` carries a success status, mapping non-success
/// statuses to a descriptive [`CloudEnhancedError::azure_service`].
async fn ensure_arm_success(
    response: reqwest::Response,
    action: &str,
) -> Result<reqwest::Response> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "<unreadable response body>".to_string());
    Err(CloudEnhancedError::azure_service(format!(
        "Azure Resource Manager returned status {status} while trying to {action}: {body}"
    )))
}

/// Verifies `response` is a success and deserializes its JSON body as `T`.
async fn parse_arm_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
    action: &str,
) -> Result<T> {
    let response = ensure_arm_success(response, action).await?;
    response.json::<T>().await.map_err(|e| {
        CloudEnhancedError::azure_service(format!(
            "Failed to parse Azure Resource Manager response while trying to {action}: {e}"
        ))
    })
}

// ---------------------------------------------------------------------
// Wire (JSON) types for the Azure Resource Manager REST API.
// ---------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ArmListResponse<T> {
    #[serde(default)]
    value: Vec<T>,
}

#[derive(Debug, Deserialize, Default)]
struct ArmIdentityResource {
    id: String,
    name: String,
    #[serde(default)]
    location: String,
    #[serde(default)]
    properties: ArmIdentityProperties,
}

#[derive(Debug, Deserialize, Default)]
struct ArmIdentityProperties {
    #[serde(rename = "principalId", default)]
    principal_id: String,
    #[serde(rename = "clientId", default)]
    client_id: String,
}

impl From<ArmIdentityResource> for IdentityInfo {
    fn from(resource: ArmIdentityResource) -> Self {
        Self {
            name: resource.name,
            resource_id: resource.id,
            principal_id: resource.properties.principal_id,
            client_id: resource.properties.client_id,
            location: resource.location,
        }
    }
}

#[derive(Debug, Serialize)]
struct FederatedCredentialCreateRequest {
    properties: FederatedCredentialProperties,
}

#[derive(Debug, Serialize, Deserialize)]
struct FederatedCredentialProperties {
    issuer: String,
    subject: String,
    #[serde(default)]
    audiences: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ArmFederatedCredentialResource {
    name: String,
    #[serde(default)]
    properties: FederatedCredentialPropertiesOpt,
}

#[derive(Debug, Deserialize, Default)]
struct FederatedCredentialPropertiesOpt {
    #[serde(default)]
    issuer: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    audiences: Vec<String>,
}

impl From<ArmFederatedCredentialResource> for FederatedCredentialInfo {
    fn from(resource: ArmFederatedCredentialResource) -> Self {
        Self {
            name: resource.name,
            issuer: resource.properties.issuer,
            subject: resource.properties.subject,
            _audiences: resource.properties.audiences,
        }
    }
}

/// Access token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessToken {
    /// Token string
    pub token: String,
    /// Expiration time
    pub expires_on: chrono::DateTime<chrono::Utc>,
}

/// Identity information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityInfo {
    /// Identity name
    pub name: String,
    /// Resource ID
    pub resource_id: String,
    /// Principal ID
    pub principal_id: String,
    /// Client ID
    pub client_id: String,
    /// Location
    pub location: String,
}

/// Federated credential information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederatedCredentialInfo {
    /// Credential name
    pub name: String,
    /// Issuer URL
    pub issuer: String,
    /// Subject
    pub subject: String,
    /// Audiences
    pub _audiences: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Spawns a minimal HTTP/1.1 mock server on an ephemeral local port that
    /// replies to every accepted connection with `body`.
    async fn spawn_mock_server(status_line: &str, content_type: &str, body: String) -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("mock server local addr");

        let response = format!(
            "{status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );

        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let response = response.clone();
                tokio::spawn(async move {
                    let mut buf = [0_u8; 4096];
                    let _ = socket.read(&mut buf).await;
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.flush().await;
                });
            }
        });

        format!("http://{addr}")
    }

    /// A `TokenCredential` that always returns a fixed fake token, used to
    /// exercise ARM calls in tests without touching the real Azure AD IMDS
    /// endpoint (which `DeveloperToolsCredential`/`ManagedIdentityCredential`
    /// would otherwise try to reach).
    #[derive(Debug)]
    struct FakeCredential;

    #[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
    #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
    impl TokenCredential for FakeCredential {
        async fn get_token(
            &self,
            _scopes: &[&str],
            _options: Option<azure_core::credentials::TokenRequestOptions<'_>>,
        ) -> azure_core::Result<azure_core::credentials::AccessToken> {
            Ok(azure_core::credentials::AccessToken::new(
                azure_core::credentials::Secret::new("fake-arm-token".to_string()),
                time::OffsetDateTime::now_utc() + time::Duration::hours(1),
            ))
        }
    }

    fn test_config() -> super::super::AzureConfig {
        super::super::AzureConfig {
            subscription_id: "11111111-1111-1111-1111-111111111111".to_string(),
            resource_group: Some("test-rg".to_string()),
            credential: Arc::new(FakeCredential),
        }
    }

    #[tokio::test]
    async fn test_list_user_assigned_identities_parses_response() {
        let body = r#"{"value":[{"id":"/subscriptions/11111111-1111-1111-1111-111111111111/resourceGroups/test-rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/my-identity","name":"my-identity","location":"eastus","properties":{"principalId":"22222222-2222-2222-2222-222222222222","clientId":"33333333-3333-3333-3333-333333333333"}}]}"#;
        let arm_base =
            spawn_mock_server("HTTP/1.1 200 OK", "application/json", body.to_string()).await;

        let config = test_config();
        let client = ManagedIdentityClient::with_arm_base_url(&config, arm_base).expect("client");

        let identities = client
            .list_user_assigned_identities("test-rg")
            .await
            .expect("identities");

        assert_eq!(identities.len(), 1);
        assert_eq!(identities[0].name, "my-identity");
        assert_eq!(
            identities[0].principal_id,
            "22222222-2222-2222-2222-222222222222"
        );
        assert_eq!(identities[0].location, "eastus");
    }

    #[tokio::test]
    async fn test_list_user_assigned_identities_error_status_is_not_swallowed() {
        let arm_base = spawn_mock_server(
            "HTTP/1.1 403 Forbidden",
            "application/json",
            r#"{"error":{"code":"AuthorizationFailed"}}"#.to_string(),
        )
        .await;

        let config = test_config();
        let client = ManagedIdentityClient::with_arm_base_url(&config, arm_base).expect("client");

        let result = client.list_user_assigned_identities("test-rg").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_user_assigned_identity_returns_real_resource_id() {
        let body = r#"{"id":"/subscriptions/11111111-1111-1111-1111-111111111111/resourceGroups/test-rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/my-identity","name":"my-identity","location":"eastus","properties":{"principalId":"22222222-2222-2222-2222-222222222222","clientId":"33333333-3333-3333-3333-333333333333"}}"#;
        let arm_base =
            spawn_mock_server("HTTP/1.1 200 OK", "application/json", body.to_string()).await;

        let config = test_config();
        let client = ManagedIdentityClient::with_arm_base_url(&config, arm_base).expect("client");

        let resource_id = client
            .create_user_assigned_identity("test-rg", "my-identity", "eastus")
            .await
            .expect("resource id");

        assert!(resource_id.ends_with("/userAssignedIdentities/my-identity"));
    }

    #[tokio::test]
    async fn test_list_federated_credentials_parses_response() {
        let body = r#"{"value":[{"name":"gh-actions","properties":{"issuer":"https://token.actions.githubusercontent.com","subject":"repo:org/repo:ref:refs/heads/main","audiences":["api://AzureADTokenExchange"]}}]}"#;
        let arm_base =
            spawn_mock_server("HTTP/1.1 200 OK", "application/json", body.to_string()).await;

        let config = test_config();
        let client = ManagedIdentityClient::with_arm_base_url(&config, arm_base).expect("client");

        let creds = client
            .list_federated_credentials("test-rg", "my-identity")
            .await
            .expect("federated credentials");

        assert_eq!(creds.len(), 1);
        assert_eq!(creds[0].name, "gh-actions");
        assert_eq!(
            creds[0].issuer,
            "https://token.actions.githubusercontent.com"
        );
        assert_eq!(creds[0]._audiences, vec!["api://AzureADTokenExchange"]);
    }

    #[tokio::test]
    async fn test_assign_identity_to_resource_succeeds() {
        let arm_base = spawn_mock_server(
            "HTTP/1.1 200 OK",
            "application/json",
            r#"{"id":"/subscriptions/x/resourceGroups/y/providers/Microsoft.Compute/virtualMachines/vm1"}"#
                .to_string(),
        )
        .await;

        let config = test_config();
        let client = ManagedIdentityClient::with_arm_base_url(&config, arm_base).expect("client");

        let result = client
            .assign_identity_to_resource(
                "/subscriptions/x/resourceGroups/y/providers/Microsoft.Compute/virtualMachines/vm1",
                "/subscriptions/x/resourceGroups/y/providers/Microsoft.ManagedIdentity/userAssignedIdentities/my-identity",
            )
            .await;

        assert!(result.is_ok());
    }

    #[test]
    fn test_resource_to_scope() {
        assert_eq!(
            resource_to_scope("https://storage.azure.com"),
            "https://storage.azure.com/.default"
        );
        assert_eq!(
            resource_to_scope("https://storage.azure.com/"),
            "https://storage.azure.com/.default"
        );
        assert_eq!(
            resource_to_scope("https://storage.azure.com/.default"),
            "https://storage.azure.com/.default"
        );
        assert_eq!(
            resource_to_scope("https://management.azure.com/"),
            "https://management.azure.com/.default"
        );
    }

    #[test]
    fn test_access_token() {
        let token = AccessToken {
            token: "test-token".to_string(),
            expires_on: chrono::Utc::now(),
        };

        assert_eq!(token.token, "test-token");
    }

    #[test]
    fn test_identity_info() {
        let info = IdentityInfo {
            name: "test-identity".to_string(),
            resource_id: "/subscriptions/123/...".to_string(),
            principal_id: "principal-123".to_string(),
            client_id: "client-123".to_string(),
            location: "eastus".to_string(),
        };

        assert_eq!(info.name, "test-identity");
        assert_eq!(info.location, "eastus");
    }
}
