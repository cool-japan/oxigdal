//! AWS cost optimization for S3 and resource management.

use crate::error::{CloudEnhancedError, Result};
use aws_sdk_costexplorer::Client as CostExplorerClient;
use aws_sdk_costexplorer::types::{DateInterval, Granularity, GroupDefinition};
use aws_sdk_s3::Client as S3Client;
use aws_sdk_s3::types::{
    IntelligentTieringAccessTier, IntelligentTieringConfiguration, IntelligentTieringFilter,
    IntelligentTieringStatus, LifecycleExpiration, LifecycleRule, LifecycleRuleFilter, Tiering,
    Transition, TransitionStorageClass,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Cost optimizer for AWS resources.
#[derive(Debug, Clone)]
pub struct CostOptimizer {
    s3_client: Arc<S3Client>,
    cost_explorer_client: Arc<CostExplorerClient>,
}

impl CostOptimizer {
    /// Creates a new cost optimizer.
    ///
    /// # Errors
    ///
    /// Returns an error if the clients cannot be created.
    pub fn new(config: &super::AwsConfig) -> Result<Self> {
        let s3_client = S3Client::new(config.sdk_config());
        let cost_explorer_client = CostExplorerClient::new(config.sdk_config());

        Ok(Self {
            s3_client: Arc::new(s3_client),
            cost_explorer_client: Arc::new(cost_explorer_client),
        })
    }

    /// Sets up S3 Intelligent-Tiering for a bucket.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration cannot be applied.
    pub async fn enable_intelligent_tiering(
        &self,
        bucket: &str,
        config_id: &str,
        prefix: Option<String>,
    ) -> Result<()> {
        let tierings = vec![
            Tiering::builder()
                .days(90)
                .access_tier(IntelligentTieringAccessTier::ArchiveAccess)
                .build()
                .map_err(|e| {
                    CloudEnhancedError::cost_optimization(format!(
                        "Failed to build archive tiering: {}",
                        e
                    ))
                })?,
            Tiering::builder()
                .days(180)
                .access_tier(IntelligentTieringAccessTier::DeepArchiveAccess)
                .build()
                .map_err(|e| {
                    CloudEnhancedError::cost_optimization(format!(
                        "Failed to build deep archive tiering: {}",
                        e
                    ))
                })?,
        ];

        let mut config_builder = IntelligentTieringConfiguration::builder()
            .id(config_id)
            .status(IntelligentTieringStatus::Enabled)
            .set_tierings(Some(tierings));

        if let Some(p) = prefix {
            let filter = IntelligentTieringFilter::builder().prefix(p).build();
            config_builder = config_builder.filter(filter);
        }

        let config = config_builder.build().map_err(|e| {
            CloudEnhancedError::cost_optimization(format!(
                "Failed to build intelligent tiering config: {}",
                e
            ))
        })?;

        self.s3_client
            .put_bucket_intelligent_tiering_configuration()
            .bucket(bucket)
            .id(config_id)
            .intelligent_tiering_configuration(config)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::cost_optimization(format!(
                    "Failed to enable intelligent tiering: {}",
                    e
                ))
            })?;

        Ok(())
    }

    /// Creates an S3 lifecycle policy for cost optimization.
    ///
    /// # Errors
    ///
    /// Returns an error if the policy cannot be applied.
    pub async fn create_lifecycle_policy(
        &self,
        bucket: &str,
        policy: LifecyclePolicy,
    ) -> Result<()> {
        let mut rule_builder =
            LifecycleRule::builder()
                .id(&policy.id)
                .status(policy.status.parse().map_err(|_| {
                    CloudEnhancedError::invalid_argument("Invalid lifecycle status".to_string())
                })?);

        if let Some(prefix) = policy.prefix {
            let filter = LifecycleRuleFilter::builder().prefix(prefix).build();
            rule_builder = rule_builder.filter(filter);
        }

        if let Some(days) = policy.transition_to_ia_days {
            let transition = Transition::builder()
                .days(days)
                .storage_class(TransitionStorageClass::StandardIa)
                .build();
            rule_builder = rule_builder.transitions(transition);
        }

        if let Some(days) = policy.transition_to_glacier_days {
            let transition = Transition::builder()
                .days(days)
                .storage_class(TransitionStorageClass::Glacier)
                .build();
            rule_builder = rule_builder.transitions(transition);
        }

        if let Some(days) = policy.transition_to_deep_archive_days {
            let transition = Transition::builder()
                .days(days)
                .storage_class(TransitionStorageClass::DeepArchive)
                .build();
            rule_builder = rule_builder.transitions(transition);
        }

        if let Some(days) = policy.expiration_days {
            let expiration = LifecycleExpiration::builder().days(days).build();
            rule_builder = rule_builder.expiration(expiration);
        }

        let rule = rule_builder.build().map_err(|e| {
            CloudEnhancedError::cost_optimization(format!("Failed to build lifecycle rule: {}", e))
        })?;

        let configuration = aws_sdk_s3::types::BucketLifecycleConfiguration::builder()
            .rules(rule)
            .build()
            .map_err(|e| {
                CloudEnhancedError::cost_optimization(format!(
                    "Failed to build lifecycle configuration: {}",
                    e
                ))
            })?;

        self.s3_client
            .put_bucket_lifecycle_configuration()
            .bucket(bucket)
            .lifecycle_configuration(configuration)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::cost_optimization(format!(
                    "Failed to create lifecycle policy: {}",
                    e
                ))
            })?;

        Ok(())
    }

    /// Gets the lifecycle configuration for a bucket.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration cannot be retrieved.
    pub async fn get_lifecycle_configuration(&self, bucket: &str) -> Result<Vec<LifecycleRule>> {
        let response = self
            .s3_client
            .get_bucket_lifecycle_configuration()
            .bucket(bucket)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::cost_optimization(format!(
                    "Failed to get lifecycle configuration: {}",
                    e
                ))
            })?;

        Ok(response.rules.unwrap_or_default())
    }

    /// Deletes the lifecycle configuration for a bucket.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration cannot be deleted.
    pub async fn delete_lifecycle_configuration(&self, bucket: &str) -> Result<()> {
        self.s3_client
            .delete_bucket_lifecycle()
            .bucket(bucket)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::cost_optimization(format!(
                    "Failed to delete lifecycle configuration: {}",
                    e
                ))
            })?;

        Ok(())
    }

    /// Gets cost and usage data.
    ///
    /// # Errors
    ///
    /// Returns an error if the cost data cannot be retrieved.
    pub async fn get_cost_and_usage(
        &self,
        start_date: &str,
        end_date: &str,
        granularity: CostGranularity,
        metrics: Vec<CostMetric>,
        group_by: Option<Vec<CostGrouping>>,
    ) -> Result<CostReport> {
        let time_period = DateInterval::builder()
            .start(start_date)
            .end(end_date)
            .build()
            .map_err(|e| {
                CloudEnhancedError::cost_optimization(format!(
                    "Failed to build date interval: {}",
                    e
                ))
            })?;

        let granularity_type = match granularity {
            CostGranularity::Daily => Granularity::Daily,
            CostGranularity::Monthly => Granularity::Monthly,
            CostGranularity::Hourly => Granularity::Hourly,
        };

        let metric_types: Vec<String> = metrics
            .into_iter()
            .map(|m| match m {
                CostMetric::UnblendedCost => "UnblendedCost".to_string(),
                CostMetric::BlendedCost => "BlendedCost".to_string(),
                CostMetric::UsageQuantity => "UsageQuantity".to_string(),
                CostMetric::AmortizedCost => "AmortizedCost".to_string(),
            })
            .collect();

        let mut request = self
            .cost_explorer_client
            .get_cost_and_usage()
            .time_period(time_period)
            .granularity(granularity_type)
            .set_metrics(Some(metric_types));

        if let Some(groups) = group_by {
            let group_defs: Vec<GroupDefinition> = groups
                .into_iter()
                .map(|g| {
                    GroupDefinition::builder()
                        .r#type(match g {
                            CostGrouping::Service => {
                                aws_sdk_costexplorer::types::GroupDefinitionType::Dimension
                            }
                            CostGrouping::UsageType => {
                                aws_sdk_costexplorer::types::GroupDefinitionType::Dimension
                            }
                            CostGrouping::LinkedAccount => {
                                aws_sdk_costexplorer::types::GroupDefinitionType::Dimension
                            }
                        })
                        .key(match g {
                            CostGrouping::Service => "SERVICE",
                            CostGrouping::UsageType => "USAGE_TYPE",
                            CostGrouping::LinkedAccount => "LINKED_ACCOUNT",
                        })
                        .build()
                })
                .collect();

            request = request.set_group_by(Some(group_defs));
        }

        let response = request.send().await.map_err(|e| {
            CloudEnhancedError::cost_optimization(format!("Failed to get cost and usage: {}", e))
        })?;

        let results = response.results_by_time().to_vec();
        let entries: Vec<CostEntry> = results
            .into_iter()
            .map(|result| {
                let start = result
                    .time_period
                    .as_ref()
                    .map(|tp| tp.start.clone())
                    .unwrap_or_default();
                let end = result
                    .time_period
                    .as_ref()
                    .map(|tp| tp.end.clone())
                    .unwrap_or_default();

                let mut costs = HashMap::new();
                if let Some(total) = result.total {
                    for (key, metric) in total {
                        if let Some(amount) = metric.amount {
                            costs.insert(key.clone(), amount.parse::<f64>().unwrap_or(0.0));
                        }
                    }
                }

                CostEntry {
                    start_date: start,
                    end_date: end,
                    costs,
                }
            })
            .collect();

        Ok(CostReport { entries })
    }

    /// Gets a cost forecast.
    ///
    /// # Errors
    ///
    /// Returns an error if the forecast cannot be retrieved.
    pub async fn get_cost_forecast(
        &self,
        start_date: &str,
        end_date: &str,
        metric: CostMetric,
    ) -> Result<f64> {
        let time_period = DateInterval::builder()
            .start(start_date)
            .end(end_date)
            .build()
            .map_err(|e| {
                CloudEnhancedError::cost_optimization(format!(
                    "Failed to build date interval: {}",
                    e
                ))
            })?;

        let metric_str = match metric {
            CostMetric::UnblendedCost => "UNBLENDED_COST",
            CostMetric::BlendedCost => "BLENDED_COST",
            CostMetric::UsageQuantity => "USAGE_QUANTITY",
            CostMetric::AmortizedCost => "AMORTIZED_COST",
        };

        let metric_type = metric_str
            .parse()
            .map_err(|_| CloudEnhancedError::invalid_argument("Invalid metric type".to_string()))?;

        let response = self
            .cost_explorer_client
            .get_cost_forecast()
            .time_period(time_period)
            .granularity(Granularity::Monthly)
            .metric(metric_type)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::cost_optimization(format!("Failed to get cost forecast: {}", e))
            })?;

        let total = response
            .total
            .and_then(|t| t.amount)
            .unwrap_or_else(|| "0".to_string());

        total
            .parse::<f64>()
            .map_err(|e| CloudEnhancedError::serialization(format!("Invalid cost value: {}", e)))
    }

    /// Analyzes S3 bucket costs and suggests optimizations.
    ///
    /// # Errors
    ///
    /// Returns an error if the analysis cannot be performed.
    pub async fn analyze_s3_costs(&self, bucket: &str) -> Result<S3CostAnalysis> {
        // Get bucket size and object count
        let objects = self
            .s3_client
            .list_objects_v2()
            .bucket(bucket)
            .max_keys(1000)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::cost_optimization(format!("Failed to list objects: {}", e))
            })?;

        let mut total_size: u64 = 0;
        let mut object_count: u64 = 0;

        for obj in objects.contents.unwrap_or_default() {
            total_size += obj.size.unwrap_or(0) as u64;
            object_count += 1;
        }

        let recommendations = self.generate_recommendations(total_size, object_count);

        Ok(S3CostAnalysis {
            bucket: bucket.to_string(),
            total_size_bytes: total_size,
            object_count,
            recommendations,
        })
    }

    /// Generates cost optimization recommendations.
    fn generate_recommendations(&self, total_size: u64, object_count: u64) -> Vec<String> {
        let mut recommendations = Vec::new();

        if total_size > 1024 * 1024 * 1024 * 100 {
            // > 100 GB
            recommendations.push(
                "Consider enabling S3 Intelligent-Tiering to automatically optimize storage costs"
                    .to_string(),
            );
        }

        if object_count > 10000 {
            recommendations.push(
                "High object count detected. Consider lifecycle policies to archive or delete old objects"
                    .to_string(),
            );
        }

        if recommendations.is_empty() {
            recommendations.push("Bucket is optimized for cost".to_string());
        }

        recommendations
    }
}

/// S3 lifecycle policy configuration.
#[derive(Debug, Clone)]
pub struct LifecyclePolicy {
    /// Policy ID
    pub id: String,
    /// Policy status (Enabled/Disabled)
    pub status: String,
    /// Prefix filter
    pub prefix: Option<String>,
    /// Days until transition to IA
    pub transition_to_ia_days: Option<i32>,
    /// Days until transition to Glacier
    pub transition_to_glacier_days: Option<i32>,
    /// Days until transition to Deep Archive
    pub transition_to_deep_archive_days: Option<i32>,
    /// Days until expiration
    pub expiration_days: Option<i32>,
}

/// Cost granularity.
#[derive(Debug, Clone, Copy)]
pub enum CostGranularity {
    /// Daily granularity
    Daily,
    /// Monthly granularity
    Monthly,
    /// Hourly granularity
    Hourly,
}

/// Cost metric type.
#[derive(Debug, Clone, Copy)]
pub enum CostMetric {
    /// Unblended cost
    UnblendedCost,
    /// Blended cost
    BlendedCost,
    /// Usage quantity
    UsageQuantity,
    /// Amortized cost
    AmortizedCost,
}

/// Cost grouping dimension.
#[derive(Debug, Clone, Copy)]
pub enum CostGrouping {
    /// Group by service
    Service,
    /// Group by usage type
    UsageType,
    /// Group by linked account
    LinkedAccount,
}

/// Cost report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostReport {
    /// Cost entries
    pub entries: Vec<CostEntry>,
}

/// Cost entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEntry {
    /// Start date
    pub start_date: String,
    /// End date
    pub end_date: String,
    /// Costs by metric
    pub costs: HashMap<String, f64>,
}

/// S3 cost analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3CostAnalysis {
    /// Bucket name
    pub bucket: String,
    /// Total size in bytes
    pub total_size_bytes: u64,
    /// Object count
    pub object_count: u64,
    /// Optimization recommendations
    pub recommendations: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lifecycle_policy() {
        let policy = LifecyclePolicy {
            id: "archive-old-data".to_string(),
            status: "Enabled".to_string(),
            prefix: Some("archive/".to_string()),
            transition_to_ia_days: Some(30),
            transition_to_glacier_days: Some(90),
            transition_to_deep_archive_days: Some(180),
            expiration_days: Some(365),
        };

        assert_eq!(policy.id, "archive-old-data");
        assert_eq!(policy.transition_to_ia_days, Some(30));
    }

    #[test]
    fn test_s3_cost_analysis() {
        let analysis = S3CostAnalysis {
            bucket: "test-bucket".to_string(),
            total_size_bytes: 1024 * 1024 * 1024,
            object_count: 1000,
            recommendations: vec!["Test recommendation".to_string()],
        };

        assert_eq!(analysis.bucket, "test-bucket");
        assert_eq!(analysis.object_count, 1000);
    }
}
