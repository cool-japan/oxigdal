//! AWS CloudWatch integration for monitoring and logs.

use crate::error::{CloudEnhancedError, Result};
use aws_sdk_cloudwatch::Client as AwsCloudWatchClient;
use aws_sdk_cloudwatch::types::{Dimension, MetricDatum, StandardUnit, Statistic};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// CloudWatch client for metrics and monitoring.
#[derive(Debug, Clone)]
pub struct CloudWatchClient {
    client: Arc<AwsCloudWatchClient>,
}

impl CloudWatchClient {
    /// Creates a new CloudWatch client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created.
    pub fn new(config: &super::AwsConfig) -> Result<Self> {
        let client = AwsCloudWatchClient::new(config.sdk_config());
        Ok(Self {
            client: Arc::new(client),
        })
    }

    /// Puts a custom metric to CloudWatch.
    ///
    /// # Errors
    ///
    /// Returns an error if the metric cannot be published.
    pub async fn put_metric(
        &self,
        namespace: &str,
        metric_name: &str,
        value: f64,
        unit: StandardUnit,
        dimensions: Vec<MetricDimension>,
    ) -> Result<()> {
        let dims: Vec<Dimension> = dimensions
            .into_iter()
            .map(|d| Dimension::builder().name(d.name).value(d.value).build())
            .collect();

        let metric = MetricDatum::builder()
            .metric_name(metric_name)
            .value(value)
            .unit(unit)
            .set_dimensions(if dims.is_empty() { None } else { Some(dims) })
            .timestamp(aws_smithy_types::DateTime::from_secs(
                Utc::now().timestamp(),
            ))
            .build();

        self.client
            .put_metric_data()
            .namespace(namespace)
            .metric_data(metric)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!("Failed to put metric data: {}", e))
            })?;

        Ok(())
    }

    /// Puts multiple metrics to CloudWatch in batch.
    ///
    /// # Errors
    ///
    /// Returns an error if the metrics cannot be published.
    pub async fn put_metrics(&self, namespace: &str, metrics: Vec<MetricData>) -> Result<()> {
        let metric_data: Vec<MetricDatum> = metrics
            .into_iter()
            .map(|m| {
                let dims: Vec<Dimension> = m
                    .dimensions
                    .into_iter()
                    .map(|d| Dimension::builder().name(d.name).value(d.value).build())
                    .collect();

                MetricDatum::builder()
                    .metric_name(m.metric_name)
                    .value(m.value)
                    .unit(m.unit)
                    .set_dimensions(if dims.is_empty() { None } else { Some(dims) })
                    .timestamp(aws_smithy_types::DateTime::from_secs(
                        m.timestamp.timestamp(),
                    ))
                    .build()
            })
            .collect();

        self.client
            .put_metric_data()
            .namespace(namespace)
            .set_metric_data(Some(metric_data))
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!("Failed to put metrics data: {}", e))
            })?;

        Ok(())
    }

    /// Gets metric statistics.
    ///
    /// # Errors
    ///
    /// Returns an error if the statistics cannot be retrieved.
    #[allow(clippy::too_many_arguments)]
    pub async fn get_metric_statistics(
        &self,
        namespace: &str,
        metric_name: &str,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        period: i32,
        statistics: Vec<Statistic>,
        dimensions: Vec<MetricDimension>,
    ) -> Result<Vec<DataPoint>> {
        let dims: Vec<Dimension> = dimensions
            .into_iter()
            .map(|d| Dimension::builder().name(d.name).value(d.value).build())
            .collect();

        let response = self
            .client
            .get_metric_statistics()
            .namespace(namespace)
            .metric_name(metric_name)
            .start_time(aws_smithy_types::DateTime::from_secs(
                start_time.timestamp(),
            ))
            .end_time(aws_smithy_types::DateTime::from_secs(end_time.timestamp()))
            .period(period)
            .set_statistics(Some(statistics))
            .set_dimensions(if dims.is_empty() { None } else { Some(dims) })
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!("Failed to get metric statistics: {}", e))
            })?;

        Ok(response
            .datapoints
            .unwrap_or_default()
            .into_iter()
            .map(|dp| DataPoint {
                timestamp: dp
                    .timestamp
                    .map(|t| DateTime::from_timestamp(t.secs(), 0).unwrap_or_default()),
                sample_count: dp.sample_count,
                average: dp.average,
                sum: dp.sum,
                minimum: dp.minimum,
                maximum: dp.maximum,
                unit: dp.unit,
            })
            .collect())
    }

    /// Lists metrics in a namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the metrics cannot be listed.
    pub async fn list_metrics(
        &self,
        namespace: Option<&str>,
        metric_name: Option<&str>,
    ) -> Result<Vec<MetricInfo>> {
        let mut request = self.client.list_metrics();

        if let Some(ns) = namespace {
            request = request.namespace(ns);
        }

        if let Some(name) = metric_name {
            request = request.metric_name(name);
        }

        let response = request.send().await.map_err(|e| {
            CloudEnhancedError::monitoring(format!("Failed to list metrics: {}", e))
        })?;

        Ok(response
            .metrics
            .unwrap_or_default()
            .into_iter()
            .map(|m| MetricInfo {
                namespace: m.namespace.unwrap_or_default(),
                metric_name: m.metric_name.unwrap_or_default(),
                dimensions: m
                    .dimensions
                    .unwrap_or_default()
                    .into_iter()
                    .map(|d| MetricDimension {
                        name: d.name.unwrap_or_default(),
                        value: d.value.unwrap_or_default(),
                    })
                    .collect(),
            })
            .collect())
    }

    /// Creates a metric alarm.
    ///
    /// # Errors
    ///
    /// Returns an error if the alarm cannot be created.
    pub async fn create_alarm(&self, config: AlarmConfig) -> Result<()> {
        let dims: Vec<Dimension> = config
            .dimensions
            .into_iter()
            .map(|d| Dimension::builder().name(d.name).value(d.value).build())
            .collect();

        let mut request = self
            .client
            .put_metric_alarm()
            .alarm_name(&config.alarm_name)
            .comparison_operator(config.comparison_operator.parse().map_err(|_| {
                CloudEnhancedError::invalid_argument("Invalid comparison operator".to_string())
            })?)
            .evaluation_periods(config.evaluation_periods)
            .metric_name(&config.metric_name)
            .namespace(&config.namespace)
            .period(config.period)
            .statistic(config.statistic)
            .threshold(config.threshold)
            .set_dimensions(if dims.is_empty() { None } else { Some(dims) });

        if let Some(desc) = config.alarm_description {
            request = request.alarm_description(desc);
        }

        if !config.alarm_actions.is_empty() {
            request = request.set_alarm_actions(Some(config.alarm_actions));
        }

        request.send().await.map_err(|e| {
            CloudEnhancedError::monitoring(format!("Failed to create alarm: {}", e))
        })?;

        Ok(())
    }

    /// Deletes a metric alarm.
    ///
    /// # Errors
    ///
    /// Returns an error if the alarm cannot be deleted.
    pub async fn delete_alarm(&self, alarm_name: &str) -> Result<()> {
        self.client
            .delete_alarms()
            .alarm_names(alarm_name)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!("Failed to delete alarm: {}", e))
            })?;

        Ok(())
    }

    /// Lists metric alarms.
    ///
    /// # Errors
    ///
    /// Returns an error if the alarms cannot be listed.
    pub async fn list_alarms(&self, max_records: Option<i32>) -> Result<Vec<String>> {
        let mut request = self.client.describe_alarms();

        if let Some(max) = max_records {
            request = request.max_records(max);
        }

        let response = request
            .send()
            .await
            .map_err(|e| CloudEnhancedError::monitoring(format!("Failed to list alarms: {}", e)))?;

        Ok(response
            .metric_alarms
            .unwrap_or_default()
            .into_iter()
            .filter_map(|a| a.alarm_name)
            .collect())
    }

    /// Gets dashboard information.
    ///
    /// # Errors
    ///
    /// Returns an error if the dashboard cannot be retrieved.
    pub async fn get_dashboard(&self, dashboard_name: &str) -> Result<String> {
        let response = self
            .client
            .get_dashboard()
            .dashboard_name(dashboard_name)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!("Failed to get dashboard: {}", e))
            })?;

        response.dashboard_body.ok_or_else(|| {
            CloudEnhancedError::not_found(format!("Dashboard {} not found", dashboard_name))
        })
    }

    /// Creates or updates a dashboard.
    ///
    /// # Errors
    ///
    /// Returns an error if the dashboard cannot be created or updated.
    pub async fn put_dashboard(&self, dashboard_name: &str, dashboard_body: &str) -> Result<()> {
        self.client
            .put_dashboard()
            .dashboard_name(dashboard_name)
            .dashboard_body(dashboard_body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!("Failed to put dashboard: {}", e))
            })?;

        Ok(())
    }

    /// Lists dashboards.
    ///
    /// # Errors
    ///
    /// Returns an error if the dashboards cannot be listed.
    pub async fn list_dashboards(&self) -> Result<Vec<String>> {
        let response = self.client.list_dashboards().send().await.map_err(|e| {
            CloudEnhancedError::monitoring(format!("Failed to list dashboards: {}", e))
        })?;

        Ok(response
            .dashboard_entries
            .unwrap_or_default()
            .into_iter()
            .filter_map(|e| e.dashboard_name)
            .collect())
    }
}

/// Metric dimension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricDimension {
    /// Dimension name
    pub name: String,
    /// Dimension value
    pub value: String,
}

/// Metric data for batch operations.
#[derive(Debug, Clone)]
pub struct MetricData {
    /// Metric name
    pub metric_name: String,
    /// Metric value
    pub value: f64,
    /// Metric unit
    pub unit: StandardUnit,
    /// Dimensions
    pub dimensions: Vec<MetricDimension>,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
}

/// CloudWatch data point.
#[derive(Debug, Clone)]
pub struct DataPoint {
    /// Timestamp
    pub timestamp: Option<DateTime<Utc>>,
    /// Sample count
    pub sample_count: Option<f64>,
    /// Average
    pub average: Option<f64>,
    /// Sum
    pub sum: Option<f64>,
    /// Minimum
    pub minimum: Option<f64>,
    /// Maximum
    pub maximum: Option<f64>,
    /// Unit
    pub unit: Option<StandardUnit>,
}

/// Metric information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricInfo {
    /// Namespace
    pub namespace: String,
    /// Metric name
    pub metric_name: String,
    /// Dimensions
    pub dimensions: Vec<MetricDimension>,
}

/// Alarm configuration.
#[derive(Debug, Clone)]
pub struct AlarmConfig {
    /// Alarm name
    pub alarm_name: String,
    /// Alarm description
    pub alarm_description: Option<String>,
    /// Metric name
    pub metric_name: String,
    /// Namespace
    pub namespace: String,
    /// Statistic
    pub statistic: Statistic,
    /// Period in seconds
    pub period: i32,
    /// Evaluation periods
    pub evaluation_periods: i32,
    /// Threshold
    pub threshold: f64,
    /// Comparison operator
    pub comparison_operator: String,
    /// Dimensions
    pub dimensions: Vec<MetricDimension>,
    /// Alarm actions (SNS topic ARNs)
    pub alarm_actions: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_dimension() {
        let dim = MetricDimension {
            name: "InstanceId".to_string(),
            value: "i-1234567890abcdef0".to_string(),
        };

        assert_eq!(dim.name, "InstanceId");
        assert_eq!(dim.value, "i-1234567890abcdef0");
    }

    #[test]
    fn test_alarm_config() {
        let config = AlarmConfig {
            alarm_name: "HighCPU".to_string(),
            alarm_description: Some("CPU > 80%".to_string()),
            metric_name: "CPUUtilization".to_string(),
            namespace: "AWS/EC2".to_string(),
            statistic: Statistic::Average,
            period: 300,
            evaluation_periods: 2,
            threshold: 80.0,
            comparison_operator: "GreaterThanThreshold".to_string(),
            dimensions: vec![],
            alarm_actions: vec![],
        };

        assert_eq!(config.alarm_name, "HighCPU");
        assert_eq!(config.threshold, 80.0);
    }
}
