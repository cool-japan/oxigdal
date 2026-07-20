# TODO: oxigeo-cloud-enhanced

> **Purpose:** Deep cloud platform integrations for AWS, Azure, and GCP (analytics/Athena/Glue/ML/Cost beyond basic storage).
> **Status (2026-07-17):** GCP IAM/Monitoring/BigQuery-cost, Azure Data Lake, and Azure Managed Identity management surfaces now make real API calls (see "Recently completed"). `bigquery.rs` module removed (dead code behind a permanently-disabled feature; see below).
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (next slice — verified gaps)
- [ ] CloudWatch + Azure Monitor metric **push** wiring (GCP side done — see "Recently completed")
  - **Verified gap:** `src/aws/cloudwatch.rs`, `src/azure/monitor.rs` modules exist but need verified-end-to-end coverage for the metric push paths.
  - **Goal:** `MetricsClient::put_datum(namespace, metric, value, dimensions)` working on all three providers; one consistent API.
  - **Design:** AWS SDK `cloudwatch::Client::put_metric_data` (already in `Cargo.toml`). Azure Monitor → `azure_core` + `https://management.azure.com/.../microsoft.insights/metrics`.
  - **Files:** `src/aws/cloudwatch.rs`, `src/azure/monitor.rs`.
  - **Tests:** (proposed) `test_cw_put_metric_data`, `test_azure_monitor_metric_post`.
  - **Risk:** Azure Monitor metric ingestion auth is region-scoped.
  - **Prerequisites:** None.

- [ ] GCP Billing Budgets API / Recommender API wiring
  - **Verified gap:** `src/gcp/cost.rs` — `create_budget`, `delete_budget`, `list_budgets`, `get_recommendations`, `get_cud_recommendations`, `create_cost_alert`, `configure_billing_export`, `get_cost_forecast`, `analyze_storage_costs` all now return `CloudEnhancedError::NotImplemented` (no live backend wired) rather than a fabricated success — an explicit, honest gap rather than a silent one.
  - **Goal:** Wire these against the real Cloud Billing Budgets API (`billingbudgets.googleapis.com`) and Recommender API (`recommender.googleapis.com`), following the same bearer-token-via-`WorkloadIdentityClient` + `reqwest` pattern already used by `query_costs`/`get_costs_by_*` in the same file.
  - **Files:** `src/gcp/cost.rs`.
  - **Why deferred:** Out of scope for this pass; `query_costs`/`get_costs_by_service`/`get_costs_by_project`/`get_costs_by_sku` (the primary FinOps-dashboard consumers) are already real BigQuery billing-export queries.

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

- [ ] Cost-explorer / Azure cost readback (GCP billing readback done — see "Recently completed")
  - **Goal:** Spend-by-tag / spend-by-resource aggregations.
  - **Files:** `src/aws/cost_optimizer.rs`, `src/azure/cost.rs`.
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
- **Blocks:** oxigeo-cluster (cost-aware scheduling needs cost API), oxigeo-services (metric push for observability).
- **Blocked by:** oxigeo-cloud (storage primitives). BigQuery SDK integration remains blocked by `google-cloud-bigquery`'s `arrow` pin (see "Recently completed" note on `gcp/cost.rs`); the REST fallback unblocks the cost-query use case without it.

## Recently completed (verbatim)
- [x] GCP IAM Credentials management surface (`src/gcp/workload_identity.rs`): `get_iam_policy`/`set_iam_policy` now call the real `iam.googleapis.com` `getIamPolicy`/`setIamPolicy` RPCs (read-modify-write with etag preserved); `create/delete/list/get_service_account`, `create/delete_service_account_key`, and `bind_workload_identity` (adds a `roles/iam.workloadIdentityUser` binding via the same read-modify-write path) are real IAM API calls. Previously all returned hardcoded/empty results.
- [x] GCP Cloud Monitoring (`src/gcp/monitoring.rs`): `write_time_series`, `list_time_series`, `create/delete/list_alert_policies`, `create/delete_notification_channel`, `create/delete_uptime_check` now call the real Cloud Monitoring v3 REST API (`monitoring.googleapis.com`), authenticated via `WorkloadIdentityClient`. Previously `create_alert_policy` returned a hardcoded `"policy-123"` and list/write paths were no-ops.
- [x] GCP billing-export cost queries (`src/gcp/cost.rs`): `query_costs`, `get_costs_by_service`, `get_costs_by_project`, `get_costs_by_sku` now execute real BigQuery `jobs.query` REST calls (with `jobs.getQueryResults` polling for async jobs) against the standard `gcp_billing_export_v1_*` billing-export table, authenticated via `WorkloadIdentityClient`. `google-cloud-bigquery` remains un-vendored (see dependency note in `Cargo.toml`); the REST path avoids the arrow/chrono conflict entirely. The remaining budget/recommendation/forecast endpoints in this file now return `CloudEnhancedError::NotImplemented` instead of a fabricated `Ok(0.0)`/empty result (tracked above under High Priority).
- [x] `src/gcp/bigquery.rs` removed: it was dead code (its `pub mod bigquery` was commented out in `gcp/mod.rs`, guarded by a non-existent `bigquery` feature) built against `google_cloud_bigquery::client::Client`, which is not a dependency of this crate (commented out in `Cargo.toml` due to an unresolved `arrow`/`chrono` version conflict with `google-cloud-bigquery` 0.15 -- verified directly via a `cargo check` dependency-resolution attempt). `gcp/cost.rs` now covers the billing-export query use case via the BigQuery REST API instead.
- [x] Azure Data Lake Storage Gen2 (`src/azure/data_lake.rs`): every method (`create/delete_filesystem`, `list_filesystems`, `create/delete_directory`, `upload_file`, `download_file`, `list_paths`, `get/set_file_properties`/`metadata`, `rename_file`, `set/get_acl`, `append_file`, `flush_file`) now wired to the real `azure_storage_datalake` SDK instead of no-op/empty stubs. Required bridging a real cross-version `azure_core` conflict: `azure_storage_datalake` 0.21 depends on `azure_core` 0.21's `TokenCredential`, incompatible with this crate's directly-declared `azure_core` 1.x `TokenCredential` -- solved by minting a bearer token from the 1.x credential and handing the raw string to `StorageCredentials::bearer_token` (see the module doc comment in `data_lake.rs` for the full explanation). `time` added as an optional dependency (gated by the `azure` feature) to convert SDK response timestamps.
- [x] Azure Managed Identity ARM management surface (`src/azure/managed_identity.rs`): `create/delete/list/get_user_assigned_identity`, `assign/remove_identity_to/from_resource`, `create/delete/list_federated_credential` now call the real Azure Resource Manager REST API (`management.azure.com`), authenticated via the crate's `AzureConfig` credential (distinct from `get_token`'s `ManagedIdentityCredential`, which mints data-plane tokens for downstream resources). Previously these all returned hardcoded/fabricated IDs or empty lists.
- [x] docs.rs now builds with `features = ["aws", "azure", "gcp"]` (`Cargo.toml`) so the feature-gated modules (most of this crate's API surface) appear in published documentation; previously only the empty default-feature surface was documented.

---
*Last audited: 2026-05-16*
