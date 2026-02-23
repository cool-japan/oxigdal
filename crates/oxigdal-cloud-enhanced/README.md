# oxigdal-cloud-enhanced

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
- **Synapse Analytics**: SQL and Spark analytics
- **Azure ML**: Training jobs, model deployment, and batch inference
- **Azure Monitor**: Metrics, logs, and alerts
- **Managed Identity**: Authentication and authorization
- **Cost Management**: Budgets, forecasts, and recommendations

### GCP Integration

- **BigQuery GIS**: SQL queries with geospatial functions
- **Dataflow**: Pipeline management and execution
- **Vertex AI**: Model training, deployment, and predictions
- **Cloud Monitoring**: Metrics, alerts, and uptime checks
- **Workload Identity**: Service account management and IAM
- **Cost Management**: Budgets, billing exports, and recommendations

## Example Usage

```rust
use oxigdal_cloud_enhanced::aws::AwsClient;

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
