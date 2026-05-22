# TODO: oxigdal-cloud-enhanced

> **Purpose:** Deep cloud platform integrations for AWS, Azure, and GCP (analytics/Athena/Glue/ML/Cost beyond basic storage).
> **Status (2026-05-16):** 7,854 Rust LoC · 39 tests · 4 real-stub sites (Azure managed-identity 1, GCP workload-identity 3)
> **Roadmap:** v0.1.5 → v0.2.0 → v1.0.0

## High Priority (next slice — verified gaps)
- [ ] Replace placeholder Azure managed-identity token issuance with real `DeveloperToolsCredential::get_token`
  - **Verified gap:** `src/azure/managed_identity.rs:38` — `// For now, return a placeholder`, then `token: "placeholder-token".to_string()` at line 41.
  - **Goal:** Issue real Azure AD access tokens via IMDS (system-assigned identity) or workload-identity-federation; honor `resource` parameter.
  - **Design:** `azure_identity::DeveloperToolsCredential` is already instantiated at line 34 but unused. Replace placeholder block with `credential.get_token(&TokenRequestOptions { scopes: vec![format!("{resource}/.default")], ..Default::default() }).await`. Map `azure_core::credentials::AccessToken { token, expires_on }` → local `AccessToken`.
  - **Files:** `src/azure/managed_identity.rs` (~30 LoC delta).
  - **Tests:** (proposed) `test_managed_identity_real_token_acquired` (mocked IMDS responder), `test_managed_identity_resource_scope_propagates`, `test_managed_identity_expiry_parsed`.
  - **Risk:** `azure_identity` 0.34 API differs from earlier versions; pin variants.
  - **Prerequisites:** None.

- [ ] Replace placeholder GCP `generate_access_token` with real IAM Credentials API call
  - **Verified gap:** `src/gcp/workload_identity.rs:191` — `access_token: "token-placeholder".to_string()` inside `generate_access_token`.
  - **Goal:** Honest implementation of GCP IAM Credentials API v1 `projects.serviceAccounts.generateAccessToken` (POST `https://iamcredentials.googleapis.com/v1/projects/-/serviceAccounts/{email}:generateAccessToken`).
  - **Design:** `google_cloud_auth::Credentials` to bootstrap the caller token; then `reqwest::Client::post(url).bearer_auth(caller_token).json(&{ scope: scopes, lifetime: format!("{lifetime_seconds}s") }).send()`. Parse `{ accessToken, expireTime }` (RFC 3339).
  - **Files:** `src/gcp/workload_identity.rs:175-194` (~50 LoC).
  - **Tests:** (proposed) `test_gcp_generate_access_token_request_shape`, `test_gcp_generate_access_token_response_parse`, `test_gcp_lifetime_seconds_serialized`.
  - **Risk:** API expects scope list and `s`-suffix duration string; common bug source.
  - **Prerequisites:** None.

- [ ] Replace placeholder GCP `generate_id_token` with real IAM Credentials API call
  - **Verified gap:** `src/gcp/workload_identity.rs:214` — `Ok("id-token-placeholder".to_string())` inside `generate_id_token`.
  - **Goal:** Real `projects.serviceAccounts.generateIdToken` (POST `:generateIdToken`).
  - **Design:** Same HTTP path/auth as above, body `{ audience, includeEmail }`. Response `{ token }` returns the OIDC JWT.
  - **Files:** `src/gcp/workload_identity.rs:198-216` (~40 LoC).
  - **Tests:** (proposed) `test_gcp_id_token_audience_propagates`, `test_gcp_id_token_jwt_shape_smoke`, `test_gcp_id_token_include_email_flag`.
  - **Risk:** JWT verification not in scope; signature trust delegated to consumer.
  - **Prerequisites:** None.

- [ ] Enable BigQuery client behind a feature flag once Arrow conflict is resolved
  - **Verified gap:** `Cargo.toml:64-67` — `# TEMPORARY: Commented out due to arrow version incompatibility / google-cloud-bigquery 0.15 requires arrow ^53, which conflicts with chrono 0.4.43 / Will re-enable when google-cloud-bigquery updates to arrow 57+`. No `bigquery.rs` body exists today.
  - **Goal:** Restore the `bigquery` feature gate with a working `BigQueryClient` (run query, fetch result set as Arrow `RecordBatch`).
  - **Design:** Wait for `google-cloud-bigquery >= 0.16` w/ arrow ^57 (or vendor an HTTP-only client using the REST API `jobs.query` + `getQueryResults`). For 0.1.5: write the HTTP-only client to unblock now; switch to SDK once available.
  - **Files:** `src/gcp/bigquery.rs` (already exists as scaffold, expand), `Cargo.toml` (un-comment when ready).
  - **Tests:** (proposed) `test_bigquery_sync_query_returns_record_batch`, `test_bigquery_async_job_poll`, `test_bigquery_st_distance_geo_query`.
  - **Risk:** Quota/billing must be opt-in via env; do not hit live BQ in CI.
  - **Prerequisites:** None (HTTP fallback path).

- [ ] CloudWatch + Azure Monitor + GCP Monitoring metric **push** wiring
  - **Verified gap:** `src/aws/cloudwatch.rs`, `src/azure/monitor.rs`, `src/gcp/monitoring.rs` modules exist but only the AWS Athena/Glue/etc. layer has the SDK wired; metric push paths in `mod.rs` chains need verified-end-to-end coverage.
  - **Goal:** `MetricsClient::put_datum(namespace, metric, value, dimensions)` working on all three providers; one consistent API.
  - **Design:** AWS SDK `cloudwatch::Client::put_metric_data` (already in `Cargo.toml`). Azure Monitor → `azure_core` + `https://management.azure.com/.../microsoft.insights/metrics`. GCP → `monitoring.timeSeries.create` REST endpoint.
  - **Files:** `src/aws/cloudwatch.rs`, `src/azure/monitor.rs`, `src/gcp/monitoring.rs`.
  - **Tests:** (proposed) `test_cw_put_metric_data`, `test_azure_monitor_metric_post`, `test_gcp_monitoring_timeseries_create`.
  - **Risk:** Azure Monitor metric ingestion auth is region-scoped.
  - **Prerequisites:** Managed-identity / workload-identity wiring (above) for non-mocked tests.

## Medium Priority
- [ ] SageMaker inference invocation for raster ML inference
  - **Goal:** `SageMakerClient::invoke_endpoint(endpoint, body)` wrapping `aws-sdk-sagemakerruntime` (already in deps).
  - **Files:** `src/aws/sagemaker.rs`.
  - **Why deferred:** Useful pattern, but storage + analytics matter more for 0.1.5.

- [ ] Vertex AI online-prediction endpoint
  - **Goal:** REST `projects/.../locations/.../endpoints/{id}:predict`.
  - **Files:** `src/gcp/vertex_ai.rs`.
  - **Why deferred:** Same as SageMaker.

- [ ] Azure Synapse spatial SQL execution
  - **Goal:** Submit T-SQL job, poll, fetch result as Arrow.
  - **Files:** `src/azure/synapse.rs`.
  - **Why deferred:** Less commonly used than Athena in geospatial workflows.

- [ ] Lambda / Azure Functions / Cloud Functions invocation
  - **Goal:** Synchronous + async invocation paths.
  - **Files:** `src/aws/lambda.rs` (Cargo dep present), `src/azure/*.rs`, `src/gcp/*.rs`.
  - **Why deferred:** Out of scope for 0.1.5 surface-area-polish.

- [ ] Cost-explorer / Azure cost / GCP billing readback
  - **Goal:** Spend-by-tag / spend-by-resource aggregations.
  - **Files:** `src/aws/cost_optimizer.rs`, `src/azure/cost.rs`, `src/gcp/cost.rs`.
  - **Why deferred:** Cloud Cost Management UI is the primary consumer today.

- [ ] Tag-based resource discovery + lifecycle policy mgmt
  - **Goal:** `list_resources_by_tag`, `set_lifecycle_policy`.
  - **Files:** new helpers under each provider module.
  - **Why deferred:** Cross-cuts SDK surface; needs design.

## Low Priority / Future (one-liners)
- [ ] AWS Glue Data Catalog `get_partitions` with predicate pushdown.
- [ ] Azure Purview lineage events for read/write ops.
- [ ] GCP Data Catalog entry creation on cloud writes.
- [ ] AWS Step Functions / Azure Data Factory / GCP Dataflow workflow submission.
- [ ] Cross-provider identity federation (AWS STS ↔ Azure AD ↔ GCP workload-identity).
- [ ] Terraform / Pulumi resource export.
- [ ] Cloud-event triggers (S3 events, EventGrid, GCS PubSub).
- [ ] Multi-cloud anomaly-detection / budget alerts.

## Cross-crate dependencies
- **Blocks:** oxigdal-cluster (cost-aware scheduling needs cost API), oxigdal-services (metric push for observability).
- **Blocked by:** oxigdal-cloud (storage primitives), `google-cloud-bigquery >= 0.16` for BigQuery (or vendored HTTP path).

## Recently completed (verbatim)
- (None — existing TODO.md had no `[x]` items.)

---
*Last audited: 2026-05-16*
