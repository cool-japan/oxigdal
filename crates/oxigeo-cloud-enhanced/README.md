# oxigeo-cloud-enhanced

Deep cloud platform integrations for AWS, Azure, and GCP.

## Overview

This crate provides enhanced cloud platform integrations beyond basic storage, including:

- **AWS**: S3 Select, Athena, Glue, Lambda, SageMaker, CloudWatch, and cost optimization
- **Azure**: Data Lake Gen2, Synapse Analytics, Azure ML, Azure Monitor, Managed Identity, and cost management
- **GCP**: BigQuery GIS, Dataflow, Vertex AI, Cloud Monitoring, Workload Identity, and cost management

## Features

### AWS Integration

- **S3 Select**: Query data in-place on S3 without downloading
- **Athena**: SQL queries on S3 data with metadata catalog
- **Glue**: Data catalog and ETL job management
- **Lambda**: Serverless function execution and management
- **SageMaker**: ML model training, deployment, and inference
- **CloudWatch**: Metrics, logs, and monitoring
- **Cost Optimizer**: S3 Intelligent-Tiering, lifecycle policies, and cost tracking

### Azure Integration

- **Data Lake Gen2**: Hierarchical namespace storage with ACLs
- **Synapse Analytics**: SQL/Spark pool management (ARM), Spark jobs (Livy), and pipelines. Dedicated-SQL query execution speaks the TDS wire protocol and is out of scope for this REST client, so `execute_query` returns a typed `NotImplemented` error rather than a silent empty result.
- **Azure ML**: Compute/model/endpoint/job management via the v2 control-plane REST API. Online/batch scoring (data-plane) returns `NotImplemented`.
- **Azure Monitor**: Metrics, Log Analytics queries, metric alerts, action groups, activity log, and diagnostic settings (custom-metric ingestion returns `NotImplemented`)
- **Managed Identity**: Authentication and authorization
- **Cost Management**: Cost queries, forecasts, usage details, budgets, and Advisor recommendations (real REST). Cost alerts / exports that require information not present in the call return `NotImplemented`.

### GCP Integration

- **BigQuery GIS**: SQL queries with geospatial functions
- **Dataflow**: Template/flex-template launch, job status/list/metrics, cancel/drain (real Dataflow v1b3 REST)
- **Vertex AI**: Model upload, endpoint create/deploy, prediction, training pipelines, batch prediction (real aiplatform REST with long-running-operation polling)
- **Cloud Monitoring**: Metrics, alerts, and uptime checks
- **Workload Identity**: Service account management and IAM
- **Cost Management**: BigQuery-billing-export cost queries, Cloud Billing budgets, and Recommender/CUD recommendations (real REST). Storage-cost analysis, forecasting, and billing-export configuration return `NotImplemented`.

## Example Usage

```rust
use oxigeo_cloud_enhanced::aws::AwsClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create AWS client
    let client = AwsClient::new(Some("us-east-1".to_string())).await?;

    // Query data with S3 Select
    let options = Default::default();
    let result = client.s3_select()
        .query_csv("my-bucket", "data.csv", "SELECT * FROM S3Object LIMIT 10", options)
        .await?;

    // Execute Athena query
    let execution_id = client.athena()
        .execute_query(
            "SELECT COUNT(*) FROM my_table",
            Some("my_database"),
            "s3://my-bucket/results/",
            None,
        )
        .await?;

    Ok(())
}
```

## COOLJAPAN Compliance

- ✅ Pure Rust implementation
- ✅ No `unwrap()` calls
- ✅ All files < 2000 lines
- ✅ Workspace dependencies

## License

Apache-2.0
