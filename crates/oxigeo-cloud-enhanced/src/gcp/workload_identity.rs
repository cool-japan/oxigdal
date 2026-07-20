//! Google Cloud Workload Identity integration.

use crate::error::{CloudEnhancedError, Result};
use serde::{Deserialize, Serialize};

/// Default GCE metadata server base URL.
const DEFAULT_METADATA_BASE_URL: &str = "http://metadata.google.internal";

/// Scope requested for the source token used to call the IAM Credentials API
/// when impersonating another service account.
const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

/// Header required by the GCE metadata server on every request.
const METADATA_FLAVOR_HEADER: &str = "Metadata-Flavor";
const METADATA_FLAVOR_VALUE: &str = "Google";

/// Default base URL of the IAM API (`iam.googleapis.com`), overridable for
/// tests via [`WorkloadIdentityClient::with_urls`].
const DEFAULT_IAM_BASE_URL: &str = "https://iam.googleapis.com";

/// Resolves the default metadata server base URL, honoring the
/// `GCE_METADATA_HOST` environment variable used by Google's own client
/// libraries (e.g. google-auth-library) to redirect metadata requests, most
/// commonly in tests or non-GCE sandboxes.
fn default_metadata_base_url() -> String {
    match std::env::var("GCE_METADATA_HOST") {
        Ok(host) if !host.is_empty() => {
            if host.starts_with("http://") || host.starts_with("https://") {
                host
            } else {
                format!("http://{host}")
            }
        }
        _ => DEFAULT_METADATA_BASE_URL.to_string(),
    }
}

/// Response body returned by the GCE metadata server's
/// `service-accounts/{account}/token` endpoint.
#[derive(Debug, Deserialize)]
struct MetadataTokenResponse {
    access_token: String,
    expires_in: i64,
}

/// Request body for the IAM Credentials `generateAccessToken` RPC.
#[derive(Debug, Serialize)]
struct GenerateAccessTokenRequest {
    scope: Vec<String>,
    lifetime: String,
}

/// Response body for the IAM Credentials `generateAccessToken` RPC.
#[derive(Debug, Deserialize)]
struct GenerateAccessTokenResponse {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "expireTime")]
    expire_time: String,
}

/// Request body for the IAM Credentials `generateIdToken` RPC.
#[derive(Debug, Serialize)]
struct GenerateIdTokenRequest {
    audience: String,
    #[serde(rename = "includeEmail")]
    include_email: bool,
}

/// Response body for the IAM Credentials `generateIdToken` RPC.
#[derive(Debug, Deserialize)]
struct GenerateIdTokenResponse {
    token: String,
}

/// Request body for the IAM `serviceAccounts.create` RPC.
#[derive(Debug, Serialize)]
struct CreateServiceAccountRequest {
    #[serde(rename = "accountId")]
    account_id: String,
    #[serde(rename = "serviceAccount")]
    service_account: CreateServiceAccountRequestInner,
}

#[derive(Debug, Serialize)]
struct CreateServiceAccountRequestInner {
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

/// Subset of the IAM `ServiceAccount` resource returned by the create/get/
/// list RPCs that this client needs.
#[derive(Debug, Deserialize)]
struct ServiceAccountResource {
    name: String,
    email: String,
    #[serde(rename = "displayName", default)]
    display_name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(rename = "uniqueId", default)]
    unique_id: String,
}

impl From<ServiceAccountResource> for ServiceAccountInfo {
    fn from(resource: ServiceAccountResource) -> Self {
        Self {
            name: resource.name,
            email: resource.email,
            display_name: resource.display_name,
            description: resource.description,
            unique_id: resource.unique_id,
        }
    }
}

/// Response body for the IAM `serviceAccounts.list` RPC.
#[derive(Debug, Deserialize)]
struct ListServiceAccountsResponse {
    #[serde(default)]
    accounts: Vec<ServiceAccountResource>,
}

/// Request body for the IAM `serviceAccounts.keys.create` RPC.
#[derive(Debug, Serialize)]
struct CreateServiceAccountKeyRequest {
    #[serde(rename = "privateKeyType")]
    private_key_type: String,
    #[serde(rename = "keyAlgorithm")]
    key_algorithm: String,
}

/// Response body for the IAM `serviceAccounts.keys.create` RPC.
#[derive(Debug, Deserialize)]
struct ServiceAccountKeyResource {
    name: String,
    #[serde(rename = "privateKeyType")]
    private_key_type: String,
    #[serde(rename = "privateKeyData")]
    private_key_data: String,
}

impl From<ServiceAccountKeyResource> for ServiceAccountKey {
    fn from(resource: ServiceAccountKeyResource) -> Self {
        Self {
            name: resource.name,
            private_key_data: resource.private_key_data,
            private_key_type: resource.private_key_type,
        }
    }
}

/// IAM policy binding as transmitted on the wire.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct IamPolicyBindingWire {
    role: String,
    #[serde(default)]
    members: Vec<String>,
}

/// The `Policy` resource shape shared by `getIamPolicy` and `setIamPolicy`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct IamPolicyWire {
    #[serde(default)]
    version: Option<i32>,
    #[serde(default)]
    etag: Option<String>,
    #[serde(default)]
    bindings: Vec<IamPolicyBindingWire>,
}

/// Request body for the IAM `setIamPolicy` RPC.
#[derive(Debug, Serialize)]
struct SetIamPolicyRequest {
    policy: IamPolicyWire,
}

/// Workload Identity client.
#[derive(Debug, Clone)]
pub struct WorkloadIdentityClient {
    project_id: String,
    /// Base URL of the GCE metadata server (overridable for tests).
    metadata_base_url: String,
    /// Base URL of the IAM API (overridable for tests).
    iam_base_url: String,
    http_client: reqwest::Client,
}

impl WorkloadIdentityClient {
    /// Creates a new Workload Identity client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created.
    pub fn new(config: &super::GcpConfig) -> Result<Self> {
        Self::with_urls(
            config,
            default_metadata_base_url(),
            DEFAULT_IAM_BASE_URL.to_string(),
        )
    }

    /// Creates a new Workload Identity client pointed at a custom metadata
    /// server base URL.
    ///
    /// This is primarily intended for tests, which spin up a local mock
    /// metadata server rather than talking to the real
    /// `metadata.google.internal` endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn with_metadata_base_url(
        config: &super::GcpConfig,
        metadata_base_url: impl Into<String>,
    ) -> Result<Self> {
        Self::with_urls(config, metadata_base_url, DEFAULT_IAM_BASE_URL.to_string())
    }

    /// Creates a new Workload Identity client pointed at custom metadata
    /// server and IAM API base URLs.
    ///
    /// This is primarily intended for tests, which spin up local mock
    /// servers rather than talking to the real `metadata.google.internal`
    /// and `iam.googleapis.com` endpoints.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn with_urls(
        config: &super::GcpConfig,
        metadata_base_url: impl Into<String>,
        iam_base_url: impl Into<String>,
    ) -> Result<Self> {
        let http_client = reqwest::Client::builder().build().map_err(|e| {
            CloudEnhancedError::configuration(format!("Failed to build HTTP client: {e}"))
        })?;

        Ok(Self {
            project_id: config.project_id().to_string(),
            metadata_base_url: metadata_base_url.into(),
            iam_base_url: iam_base_url.into(),
            http_client,
        })
    }

    /// Obtains a bearer token for authenticating to the IAM API, using the
    /// instance's attached service account.
    async fn iam_bearer_token(&self) -> Result<String> {
        let token = self
            .fetch_attached_access_token(&[CLOUD_PLATFORM_SCOPE.to_string()])
            .await?;
        Ok(token.access_token)
    }

    /// Creates a service account.
    ///
    /// # Errors
    ///
    /// Returns an error if the account cannot be created.
    pub async fn create_service_account(
        &self,
        account_id: &str,
        display_name: &str,
        description: Option<&str>,
    ) -> Result<String> {
        tracing::info!(
            "Creating service account: {} (display: {}, description: {:?})",
            account_id,
            display_name,
            description
        );

        let token = self.iam_bearer_token().await?;
        let url = format!(
            "{}/v1/projects/{}/serviceAccounts",
            self.iam_base_url, self.project_id
        );
        let request_body = CreateServiceAccountRequest {
            account_id: account_id.to_string(),
            service_account: CreateServiceAccountRequestInner {
                display_name: display_name.to_string(),
                description: description.map(str::to_string),
            },
        };

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&token)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::gcp_service(format!(
                    "IAM serviceAccounts.create request failed: {e}"
                ))
            })?;

        let resource: ServiceAccountResource =
            parse_iam_response(response, "create service account").await?;
        Ok(resource.name)
    }

    /// Deletes a service account.
    ///
    /// # Errors
    ///
    /// Returns an error if the account cannot be deleted.
    pub async fn delete_service_account(&self, email: &str) -> Result<()> {
        tracing::info!("Deleting service account: {}", email);

        let token = self.iam_bearer_token().await?;
        let url = format!(
            "{}/v1/projects/{}/serviceAccounts/{}",
            self.iam_base_url, self.project_id, email
        );

        let response = self
            .http_client
            .delete(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::gcp_service(format!(
                    "IAM serviceAccounts.delete request failed: {e}"
                ))
            })?;

        ensure_success(response, "delete service account").await?;
        Ok(())
    }

    /// Lists service accounts.
    ///
    /// # Errors
    ///
    /// Returns an error if the accounts cannot be listed.
    pub async fn list_service_accounts(&self) -> Result<Vec<ServiceAccountInfo>> {
        tracing::info!("Listing service accounts");

        let token = self.iam_bearer_token().await?;
        let url = format!(
            "{}/v1/projects/{}/serviceAccounts",
            self.iam_base_url, self.project_id
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::gcp_service(format!(
                    "IAM serviceAccounts.list request failed: {e}"
                ))
            })?;

        let body: ListServiceAccountsResponse =
            parse_iam_response(response, "list service accounts").await?;
        Ok(body.accounts.into_iter().map(Into::into).collect())
    }

    /// Gets a service account.
    ///
    /// # Errors
    ///
    /// Returns an error if the account cannot be retrieved.
    pub async fn get_service_account(&self, email: &str) -> Result<ServiceAccountInfo> {
        tracing::info!("Getting service account: {}", email);

        let token = self.iam_bearer_token().await?;
        let url = format!(
            "{}/v1/projects/{}/serviceAccounts/{}",
            self.iam_base_url, self.project_id, email
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::gcp_service(format!(
                    "IAM serviceAccounts.get request failed: {e}"
                ))
            })?;

        let resource: ServiceAccountResource =
            parse_iam_response(response, "get service account").await?;
        Ok(resource.into())
    }

    /// Creates a service account key.
    ///
    /// # Errors
    ///
    /// Returns an error if the key cannot be created.
    pub async fn create_service_account_key(
        &self,
        service_account_email: &str,
        key_algorithm: KeyAlgorithm,
    ) -> Result<ServiceAccountKey> {
        tracing::info!(
            "Creating service account key for: {} (algorithm: {:?})",
            service_account_email,
            key_algorithm
        );

        let token = self.iam_bearer_token().await?;
        let url = format!(
            "{}/v1/projects/{}/serviceAccounts/{}/keys",
            self.iam_base_url, self.project_id, service_account_email
        );
        let request_body = CreateServiceAccountKeyRequest {
            private_key_type: "TYPE_GOOGLE_CREDENTIALS_FILE".to_string(),
            key_algorithm: key_algorithm.as_api_str().to_string(),
        };

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&token)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::gcp_service(format!(
                    "IAM serviceAccounts.keys.create request failed: {e}"
                ))
            })?;

        let resource: ServiceAccountKeyResource =
            parse_iam_response(response, "create service account key").await?;
        Ok(resource.into())
    }

    /// Deletes a service account key.
    ///
    /// # Errors
    ///
    /// Returns an error if the key cannot be deleted.
    pub async fn delete_service_account_key(&self, key_name: &str) -> Result<()> {
        tracing::info!("Deleting service account key: {}", key_name);

        let token = self.iam_bearer_token().await?;
        let url = format!(
            "{}/v1/{}",
            self.iam_base_url,
            key_name.trim_start_matches('/')
        );

        let response = self
            .http_client
            .delete(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::gcp_service(format!(
                    "IAM serviceAccounts.keys.delete request failed: {e}"
                ))
            })?;

        ensure_success(response, "delete service account key").await?;
        Ok(())
    }

    /// Enables Workload Identity for a Kubernetes service account by adding
    /// the Kubernetes service account as a member of the
    /// `roles/iam.workloadIdentityUser` binding on the target Google
    /// service account's IAM policy.
    ///
    /// # Errors
    ///
    /// Returns an error if the binding cannot be created.
    pub async fn bind_workload_identity(
        &self,
        service_account_email: &str,
        namespace: &str,
        k8s_service_account: &str,
    ) -> Result<()> {
        tracing::info!(
            "Binding Workload Identity: {} -> {}/{}",
            service_account_email,
            namespace,
            k8s_service_account
        );

        let member = format!(
            "serviceAccount:{}.svc.id.goog[{namespace}/{k8s_service_account}]",
            self.project_id
        );

        let (mut policy, etag) = self.fetch_iam_policy_wire(service_account_email).await?;

        const ROLE: &str = "roles/iam.workloadIdentityUser";
        if let Some(binding) = policy.bindings.iter_mut().find(|b| b.role == ROLE) {
            if !binding.members.iter().any(|m| m == &member) {
                binding.members.push(member);
            }
        } else {
            policy.bindings.push(IamPolicyBindingWire {
                role: ROLE.to_string(),
                members: vec![member],
            });
        }
        policy.etag = etag;

        self.set_iam_policy_wire(service_account_email, policy)
            .await
    }

    /// Sets IAM policy for a service account.
    ///
    /// # Errors
    ///
    /// Returns an error if the policy cannot be set.
    pub async fn set_iam_policy(
        &self,
        service_account_email: &str,
        bindings: Vec<IamBinding>,
    ) -> Result<()> {
        tracing::info!(
            "Setting IAM policy for: {} ({} bindings)",
            service_account_email,
            bindings.len()
        );

        // Preserve the current etag for optimistic concurrency, per the IAM
        // API's documented read-modify-write pattern.
        let (_, etag) = self.fetch_iam_policy_wire(service_account_email).await?;

        let policy = IamPolicyWire {
            version: Some(3),
            etag,
            bindings: bindings
                .into_iter()
                .map(|b| IamPolicyBindingWire {
                    role: b.role,
                    members: b.members,
                })
                .collect(),
        };

        self.set_iam_policy_wire(service_account_email, policy)
            .await
    }

    /// Gets IAM policy for a service account.
    ///
    /// # Errors
    ///
    /// Returns an error if the policy cannot be retrieved.
    pub async fn get_iam_policy(&self, service_account_email: &str) -> Result<Vec<IamBinding>> {
        tracing::info!("Getting IAM policy for: {}", service_account_email);

        let (policy, _etag) = self.fetch_iam_policy_wire(service_account_email).await?;
        Ok(policy
            .bindings
            .into_iter()
            .map(|b| IamBinding {
                role: b.role,
                members: b.members,
            })
            .collect())
    }

    /// Fetches the raw IAM policy (including its etag, needed for
    /// optimistic-concurrency writes) for `service_account_email`.
    async fn fetch_iam_policy_wire(
        &self,
        service_account_email: &str,
    ) -> Result<(IamPolicyWire, Option<String>)> {
        let token = self.iam_bearer_token().await?;
        let url = format!(
            "{}/v1/projects/-/serviceAccounts/{service_account_email}:getIamPolicy",
            self.iam_base_url
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::gcp_service(format!("IAM getIamPolicy request failed: {e}"))
            })?;

        let policy: IamPolicyWire = parse_iam_response(response, "get IAM policy").await?;
        let etag = policy.etag.clone();
        Ok((policy, etag))
    }

    /// Writes `policy` as the IAM policy for `service_account_email`.
    async fn set_iam_policy_wire(
        &self,
        service_account_email: &str,
        policy: IamPolicyWire,
    ) -> Result<()> {
        let token = self.iam_bearer_token().await?;
        let url = format!(
            "{}/v1/projects/-/serviceAccounts/{service_account_email}:setIamPolicy",
            self.iam_base_url
        );
        let request_body = SetIamPolicyRequest { policy };

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&token)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::gcp_service(format!("IAM setIamPolicy request failed: {e}"))
            })?;

        ensure_success(response, "set IAM policy").await?;
        Ok(())
    }

    /// Gets an access token, either for the instance's attached service
    /// account (`service_account_email` is `"default"` or empty) or by
    /// impersonating another service account via the IAM Credentials API.
    ///
    /// # Errors
    ///
    /// Returns an error if the token cannot be generated.
    pub async fn generate_access_token(
        &self,
        service_account_email: &str,
        scopes: Vec<String>,
        lifetime_seconds: i32,
    ) -> Result<AccessToken> {
        tracing::info!(
            "Generating access token for: {} ({} scopes, {}s lifetime)",
            service_account_email,
            scopes.len(),
            lifetime_seconds
        );

        if is_attached_service_account(service_account_email) {
            self.fetch_attached_access_token(&scopes).await
        } else {
            self.impersonate_access_token(service_account_email, &scopes, lifetime_seconds)
                .await
        }
    }

    /// Generates an ID token, either for the instance's attached service
    /// account or by impersonating another service account.
    ///
    /// # Errors
    ///
    /// Returns an error if the token cannot be generated.
    pub async fn generate_id_token(
        &self,
        service_account_email: &str,
        audience: &str,
        include_email: bool,
    ) -> Result<String> {
        tracing::info!(
            "Generating ID token for: {} (audience: {}, include_email: {})",
            service_account_email,
            audience,
            include_email
        );

        if is_attached_service_account(service_account_email) {
            self.fetch_attached_id_token(audience, include_email).await
        } else {
            self.impersonate_id_token(service_account_email, audience, include_email)
                .await
        }
    }

    /// Fetches an access token for the instance's attached service account
    /// directly from the GCE metadata server.
    async fn fetch_attached_access_token(&self, scopes: &[String]) -> Result<AccessToken> {
        let mut url = format!(
            "{}/computeMetadata/v1/instance/service-accounts/default/token",
            self.metadata_base_url
        );
        if !scopes.is_empty() {
            url.push_str("?scopes=");
            url.push_str(&scopes.join(","));
        }

        let response = self
            .http_client
            .get(&url)
            .header(METADATA_FLAVOR_HEADER, METADATA_FLAVOR_VALUE)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::authentication(format!("Metadata server request failed: {e}"))
            })?;

        if !response.status().is_success() {
            return Err(CloudEnhancedError::authentication(format!(
                "Metadata server returned status {} while fetching access token",
                response.status()
            )));
        }

        let body: MetadataTokenResponse = response.json().await.map_err(|e| {
            CloudEnhancedError::authentication(format!(
                "Failed to parse metadata server token response: {e}"
            ))
        })?;

        Ok(AccessToken {
            access_token: body.access_token,
            expire_time: chrono::Utc::now() + chrono::Duration::seconds(body.expires_in),
        })
    }

    /// Obtains an access token for `service_account_email` by impersonating
    /// it through the IAM Credentials API, authenticated with the attached
    /// service account's own token.
    async fn impersonate_access_token(
        &self,
        service_account_email: &str,
        scopes: &[String],
        lifetime_seconds: i32,
    ) -> Result<AccessToken> {
        let source_token = self
            .fetch_attached_access_token(&[CLOUD_PLATFORM_SCOPE.to_string()])
            .await?;

        let effective_scopes = if scopes.is_empty() {
            vec![CLOUD_PLATFORM_SCOPE.to_string()]
        } else {
            scopes.to_vec()
        };

        let url = format!(
            "https://iamcredentials.googleapis.com/v1/projects/-/serviceAccounts/{service_account_email}:generateAccessToken"
        );
        let request_body = GenerateAccessTokenRequest {
            scope: effective_scopes,
            lifetime: format!("{}s", lifetime_seconds.max(1)),
        };

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&source_token.access_token)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::authentication(format!(
                    "IAM Credentials generateAccessToken request failed: {e}"
                ))
            })?;

        if !response.status().is_success() {
            return Err(CloudEnhancedError::authentication(format!(
                "IAM Credentials API returned status {} while impersonating '{service_account_email}'",
                response.status()
            )));
        }

        let body: GenerateAccessTokenResponse = response.json().await.map_err(|e| {
            CloudEnhancedError::authentication(format!(
                "Failed to parse generateAccessToken response: {e}"
            ))
        })?;

        let expire_time = chrono::DateTime::parse_from_rfc3339(&body.expire_time)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .map_err(|e| {
                CloudEnhancedError::authentication(format!(
                    "Invalid expireTime '{}' in generateAccessToken response: {e}",
                    body.expire_time
                ))
            })?;

        Ok(AccessToken {
            access_token: body.access_token,
            expire_time,
        })
    }

    /// Fetches an identity token for the instance's attached service account
    /// directly from the GCE metadata server.
    async fn fetch_attached_id_token(&self, audience: &str, include_email: bool) -> Result<String> {
        let mut url = url::Url::parse(&format!(
            "{}/computeMetadata/v1/instance/service-accounts/default/identity",
            self.metadata_base_url
        ))
        .map_err(|e| {
            CloudEnhancedError::authentication(format!("Invalid metadata server URL: {e}"))
        })?;
        url.query_pairs_mut()
            .append_pair("audience", audience)
            .append_pair("format", if include_email { "full" } else { "standard" });

        let response = self
            .http_client
            .get(url)
            .header(METADATA_FLAVOR_HEADER, METADATA_FLAVOR_VALUE)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::authentication(format!("Metadata server request failed: {e}"))
            })?;

        if !response.status().is_success() {
            return Err(CloudEnhancedError::authentication(format!(
                "Metadata server returned status {} while fetching identity token",
                response.status()
            )));
        }

        response.text().await.map_err(|e| {
            CloudEnhancedError::authentication(format!(
                "Failed to read metadata server identity token response: {e}"
            ))
        })
    }

    /// Obtains an identity token for `service_account_email` by
    /// impersonating it through the IAM Credentials API.
    async fn impersonate_id_token(
        &self,
        service_account_email: &str,
        audience: &str,
        include_email: bool,
    ) -> Result<String> {
        let source_token = self
            .fetch_attached_access_token(&[CLOUD_PLATFORM_SCOPE.to_string()])
            .await?;

        let url = format!(
            "https://iamcredentials.googleapis.com/v1/projects/-/serviceAccounts/{service_account_email}:generateIdToken"
        );
        let request_body = GenerateIdTokenRequest {
            audience: audience.to_string(),
            include_email,
        };

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&source_token.access_token)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::authentication(format!(
                    "IAM Credentials generateIdToken request failed: {e}"
                ))
            })?;

        if !response.status().is_success() {
            return Err(CloudEnhancedError::authentication(format!(
                "IAM Credentials API returned status {} while impersonating '{service_account_email}'",
                response.status()
            )));
        }

        let body: GenerateIdTokenResponse = response.json().await.map_err(|e| {
            CloudEnhancedError::authentication(format!(
                "Failed to parse generateIdToken response: {e}"
            ))
        })?;

        Ok(body.token)
    }
}

/// Returns `true` when `service_account_email` refers to the instance's
/// attached (default) service account rather than one to impersonate.
fn is_attached_service_account(service_account_email: &str) -> bool {
    service_account_email.is_empty() || service_account_email == "default"
}

/// Verifies that `response` carries a success status, mapping non-success
/// statuses to a descriptive [`CloudEnhancedError::gcp_service`].
async fn ensure_success(response: reqwest::Response, action: &str) -> Result<reqwest::Response> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "<unreadable response body>".to_string());
    Err(CloudEnhancedError::gcp_service(format!(
        "IAM API returned status {status} while trying to {action}: {body}"
    )))
}

/// Verifies `response` is a success and deserializes its JSON body as `T`.
async fn parse_iam_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
    action: &str,
) -> Result<T> {
    let response = ensure_success(response, action).await?;
    response.json::<T>().await.map_err(|e| {
        CloudEnhancedError::gcp_service(format!(
            "Failed to parse IAM API response while trying to {action}: {e}"
        ))
    })
}

/// Service account information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAccountInfo {
    /// Resource name
    pub name: String,
    /// Email
    pub email: String,
    /// Display name
    pub display_name: String,
    /// Description
    pub description: Option<String>,
    /// Unique ID
    pub unique_id: String,
}

/// Service account key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAccountKey {
    /// Key name
    pub name: String,
    /// Private key data (base64 encoded)
    pub private_key_data: String,
    /// Private key type
    pub private_key_type: String,
}

/// Key algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAlgorithm {
    /// RSA 2048
    Rsa2048,
    /// RSA 4096
    Rsa4096,
}

impl KeyAlgorithm {
    /// Returns the IAM API's string representation of this algorithm.
    fn as_api_str(self) -> &'static str {
        match self {
            Self::Rsa2048 => "KEY_ALG_RSA_2048",
            Self::Rsa4096 => "KEY_ALG_RSA_4096",
        }
    }
}

/// IAM binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IamBinding {
    /// Role (e.g., "roles/iam.workloadIdentityUser")
    pub role: String,
    /// Members (e.g., "serviceAccount:my-sa@...")
    pub members: Vec<String>,
}

/// Access token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessToken {
    /// Access token
    pub access_token: String,
    /// Expiration time
    pub expire_time: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Spawns a minimal HTTP/1.1 mock server on an ephemeral local port that
    /// replies to every accepted connection with `body`, and returns its
    /// base URL (`http://127.0.0.1:PORT`), suitable for
    /// [`WorkloadIdentityClient::with_metadata_base_url`].
    async fn spawn_mock_metadata_server(
        status_line: &str,
        content_type: &str,
        body: String,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock metadata server");
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
                    // Drain (part of) the request so the client sees a clean response.
                    let _ = socket.read(&mut buf).await;
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.flush().await;
                });
            }
        });

        format!("http://{addr}")
    }

    #[tokio::test]
    async fn test_generate_access_token_attached_service_account() {
        let body =
            r#"{"access_token":"mock-access-token","expires_in":3599,"token_type":"Bearer"}"#
                .to_string();
        let base_url =
            spawn_mock_metadata_server("HTTP/1.1 200 OK", "application/json", body).await;

        let config =
            crate::gcp::GcpConfig::new("test-project".to_string(), None).expect("gcp config");
        let client = WorkloadIdentityClient::with_metadata_base_url(&config, base_url)
            .expect("workload identity client");

        let token = client
            .generate_access_token(
                "default",
                vec!["https://www.googleapis.com/auth/cloud-platform".to_string()],
                3600,
            )
            .await
            .expect("access token");

        assert_eq!(token.access_token, "mock-access-token");
        assert!(token.expire_time > chrono::Utc::now());
    }

    #[tokio::test]
    async fn test_generate_access_token_empty_email_uses_attached_identity() {
        let body = r#"{"access_token":"mock-access-token-2","expires_in":60}"#.to_string();
        let base_url =
            spawn_mock_metadata_server("HTTP/1.1 200 OK", "application/json", body).await;

        let config =
            crate::gcp::GcpConfig::new("test-project".to_string(), None).expect("gcp config");
        let client = WorkloadIdentityClient::with_metadata_base_url(&config, base_url)
            .expect("workload identity client");

        let token = client
            .generate_access_token("", vec![], 60)
            .await
            .expect("access token");

        assert_eq!(token.access_token, "mock-access-token-2");
    }

    #[tokio::test]
    async fn test_generate_id_token_attached_service_account() {
        let jwt = "header.payload.signature".to_string();
        let base_url =
            spawn_mock_metadata_server("HTTP/1.1 200 OK", "text/plain", jwt.clone()).await;

        let config =
            crate::gcp::GcpConfig::new("test-project".to_string(), None).expect("gcp config");
        let client = WorkloadIdentityClient::with_metadata_base_url(&config, base_url)
            .expect("workload identity client");

        let token = client
            .generate_id_token("default", "https://example.com", true)
            .await
            .expect("identity token");

        assert_eq!(token, jwt);
    }

    #[tokio::test]
    async fn test_generate_access_token_metadata_server_error_status() {
        let base_url = spawn_mock_metadata_server(
            "HTTP/1.1 404 Not Found",
            "text/plain",
            "not found".to_string(),
        )
        .await;

        let config =
            crate::gcp::GcpConfig::new("test-project".to_string(), None).expect("gcp config");
        let client = WorkloadIdentityClient::with_metadata_base_url(&config, base_url)
            .expect("workload identity client");

        let result = client.generate_access_token("default", vec![], 60).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_is_attached_service_account() {
        assert!(is_attached_service_account(""));
        assert!(is_attached_service_account("default"));
        assert!(!is_attached_service_account(
            "sa@project.iam.gserviceaccount.com"
        ));
    }

    #[test]
    fn test_default_metadata_base_url_honors_env_override() {
        match std::env::var("GCE_METADATA_HOST") {
            Ok(host) if !host.is_empty() => {
                assert!(default_metadata_base_url().contains(&host));
            }
            _ => assert_eq!(default_metadata_base_url(), DEFAULT_METADATA_BASE_URL),
        }
    }

    #[test]
    fn test_key_algorithm() {
        assert_eq!(KeyAlgorithm::Rsa2048, KeyAlgorithm::Rsa2048);
        assert_ne!(KeyAlgorithm::Rsa2048, KeyAlgorithm::Rsa4096);
    }

    #[test]
    fn test_service_account_info() {
        let info = ServiceAccountInfo {
            name: "projects/test/serviceAccounts/test@test.iam.gserviceaccount.com".to_string(),
            email: "test@test.iam.gserviceaccount.com".to_string(),
            display_name: "Test Account".to_string(),
            description: None,
            unique_id: "123456789".to_string(),
        };

        assert_eq!(info.email, "test@test.iam.gserviceaccount.com");
    }

    /// Standard metadata-server mock response body used by the IAM API
    /// tests below (they need a token before they can call the IAM API).
    const MOCK_METADATA_TOKEN_BODY: &str = r#"{"access_token":"mock-token","expires_in":3600}"#;

    #[tokio::test]
    async fn test_get_iam_policy_parses_bindings() {
        let metadata_base = spawn_mock_metadata_server(
            "HTTP/1.1 200 OK",
            "application/json",
            MOCK_METADATA_TOKEN_BODY.to_string(),
        )
        .await;
        let iam_body = r#"{"version":1,"etag":"BwAB","bindings":[{"role":"roles/iam.serviceAccountUser","members":["user:alice@example.com"]}]}"#.to_string();
        let iam_base =
            spawn_mock_metadata_server("HTTP/1.1 200 OK", "application/json", iam_body).await;

        let config =
            crate::gcp::GcpConfig::new("test-project".to_string(), None).expect("gcp config");
        let client = WorkloadIdentityClient::with_urls(&config, metadata_base, iam_base)
            .expect("workload identity client");

        let bindings = client
            .get_iam_policy("sa@test-project.iam.gserviceaccount.com")
            .await
            .expect("iam policy");

        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].role, "roles/iam.serviceAccountUser");
        assert_eq!(
            bindings[0].members,
            vec!["user:alice@example.com".to_string()]
        );
    }

    #[tokio::test]
    async fn test_get_iam_policy_error_status_is_not_swallowed() {
        let metadata_base = spawn_mock_metadata_server(
            "HTTP/1.1 200 OK",
            "application/json",
            MOCK_METADATA_TOKEN_BODY.to_string(),
        )
        .await;
        let iam_base = spawn_mock_metadata_server(
            "HTTP/1.1 403 Forbidden",
            "application/json",
            r#"{"error":"permission denied"}"#.to_string(),
        )
        .await;

        let config =
            crate::gcp::GcpConfig::new("test-project".to_string(), None).expect("gcp config");
        let client = WorkloadIdentityClient::with_urls(&config, metadata_base, iam_base)
            .expect("workload identity client");

        let result = client
            .get_iam_policy("sa@test-project.iam.gserviceaccount.com")
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_set_iam_policy_succeeds() {
        let metadata_base = spawn_mock_metadata_server(
            "HTTP/1.1 200 OK",
            "application/json",
            MOCK_METADATA_TOKEN_BODY.to_string(),
        )
        .await;
        let iam_body = r#"{"version":1,"etag":"BwAB","bindings":[]}"#.to_string();
        let iam_base =
            spawn_mock_metadata_server("HTTP/1.1 200 OK", "application/json", iam_body).await;

        let config =
            crate::gcp::GcpConfig::new("test-project".to_string(), None).expect("gcp config");
        let client = WorkloadIdentityClient::with_urls(&config, metadata_base, iam_base)
            .expect("workload identity client");

        let result = client
            .set_iam_policy(
                "sa@test-project.iam.gserviceaccount.com",
                vec![IamBinding {
                    role: "roles/iam.workloadIdentityUser".to_string(),
                    members: vec!["user:alice@example.com".to_string()],
                }],
            )
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_bind_workload_identity_reads_then_writes_policy() {
        let metadata_base = spawn_mock_metadata_server(
            "HTTP/1.1 200 OK",
            "application/json",
            MOCK_METADATA_TOKEN_BODY.to_string(),
        )
        .await;
        let iam_body = r#"{"version":1,"etag":"BwAB","bindings":[]}"#.to_string();
        let iam_base =
            spawn_mock_metadata_server("HTTP/1.1 200 OK", "application/json", iam_body).await;

        let config =
            crate::gcp::GcpConfig::new("test-project".to_string(), None).expect("gcp config");
        let client = WorkloadIdentityClient::with_urls(&config, metadata_base, iam_base)
            .expect("workload identity client");

        let result = client
            .bind_workload_identity(
                "sa@test-project.iam.gserviceaccount.com",
                "default",
                "my-ksa",
            )
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_list_service_accounts_parses_accounts() {
        let metadata_base = spawn_mock_metadata_server(
            "HTTP/1.1 200 OK",
            "application/json",
            MOCK_METADATA_TOKEN_BODY.to_string(),
        )
        .await;
        let iam_body = r#"{"accounts":[{"name":"projects/test-project/serviceAccounts/sa@test-project.iam.gserviceaccount.com","email":"sa@test-project.iam.gserviceaccount.com","displayName":"SA","uniqueId":"123456"}]}"#.to_string();
        let iam_base =
            spawn_mock_metadata_server("HTTP/1.1 200 OK", "application/json", iam_body).await;

        let config =
            crate::gcp::GcpConfig::new("test-project".to_string(), None).expect("gcp config");
        let client = WorkloadIdentityClient::with_urls(&config, metadata_base, iam_base)
            .expect("workload identity client");

        let accounts = client.list_service_accounts().await.expect("accounts");
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].email, "sa@test-project.iam.gserviceaccount.com");
        assert_eq!(accounts[0].unique_id, "123456");
    }

    #[tokio::test]
    async fn test_create_service_account_key_parses_response() {
        let metadata_base = spawn_mock_metadata_server(
            "HTTP/1.1 200 OK",
            "application/json",
            MOCK_METADATA_TOKEN_BODY.to_string(),
        )
        .await;
        let iam_body = r#"{"name":"projects/test-project/serviceAccounts/sa@test-project.iam.gserviceaccount.com/keys/abc123","privateKeyType":"TYPE_GOOGLE_CREDENTIALS_FILE","privateKeyData":"ZmFrZS1rZXk="}"#.to_string();
        let iam_base =
            spawn_mock_metadata_server("HTTP/1.1 200 OK", "application/json", iam_body).await;

        let config =
            crate::gcp::GcpConfig::new("test-project".to_string(), None).expect("gcp config");
        let client = WorkloadIdentityClient::with_urls(&config, metadata_base, iam_base)
            .expect("workload identity client");

        let key = client
            .create_service_account_key(
                "sa@test-project.iam.gserviceaccount.com",
                KeyAlgorithm::Rsa2048,
            )
            .await
            .expect("key");

        assert!(key.name.ends_with("/keys/abc123"));
        assert_eq!(key.private_key_data, "ZmFrZS1rZXk=");
    }

    #[test]
    fn test_key_algorithm_as_api_str() {
        assert_eq!(KeyAlgorithm::Rsa2048.as_api_str(), "KEY_ALG_RSA_2048");
        assert_eq!(KeyAlgorithm::Rsa4096.as_api_str(), "KEY_ALG_RSA_4096");
    }
}
