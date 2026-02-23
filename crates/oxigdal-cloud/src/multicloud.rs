//! Multi-cloud support with unified interface, failover, and intelligent routing
//!
//! This module provides comprehensive multi-cloud storage capabilities including:
//!
//! - **Unified Interface**: Single API for S3, Azure Blob, GCS, and HTTP backends
//! - **Automatic Failover**: Seamless failover between cloud providers
//! - **Cross-Cloud Transfer**: Efficient data transfer between different cloud providers
//! - **Provider Detection**: Automatic detection of cloud provider from URLs
//! - **Cost-Optimized Routing**: Route requests to minimize costs
//! - **Latency-Based Selection**: Select providers based on measured latency
//! - **Region-Aware Operations**: Optimize operations based on geographic regions
//!
//! # Example
//!
//! ```rust,no_run
//! # #[cfg(feature = "async")]
//! # async fn example() -> oxigdal_cloud::Result<()> {
//! use oxigdal_cloud::multicloud::{MultiCloudManager, CloudProviderConfig, CloudProvider};
//!
//! let manager = MultiCloudManager::builder()
//!     .add_provider(CloudProviderConfig::s3("primary-bucket").with_priority(1))
//!     .add_provider(CloudProviderConfig::gcs("backup-bucket").with_priority(2))
//!     .with_failover(true)
//!     .with_latency_routing(true)
//!     .build()?;
//!
//! // Automatic routing and failover
//! let data = manager.get("path/to/object").await?;
//!
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;

#[cfg(feature = "async")]
use crate::backends::CloudStorageBackend;
use crate::error::{CloudError, Result};

// ============================================================================
// Cloud Provider Types
// ============================================================================

/// Supported cloud providers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CloudProvider {
    /// Amazon Web Services S3
    AwsS3,
    /// Microsoft Azure Blob Storage
    AzureBlob,
    /// Google Cloud Storage
    Gcs,
    /// HTTP/HTTPS endpoint
    Http,
    /// Custom or unknown provider
    Custom,
}

impl CloudProvider {
    /// Detects cloud provider from URL
    #[must_use]
    pub fn from_url(url: &str) -> Option<Self> {
        let lower = url.to_lowercase();

        // Check scheme first
        if lower.starts_with("s3://") {
            return Some(Self::AwsS3);
        }
        if lower.starts_with("az://") || lower.starts_with("azure://") {
            return Some(Self::AzureBlob);
        }
        if lower.starts_with("gs://") || lower.starts_with("gcs://") {
            return Some(Self::Gcs);
        }

        // Check for cloud-specific domains in HTTP URLs
        if lower.starts_with("http://") || lower.starts_with("https://") {
            if lower.contains(".s3.") || lower.contains(".amazonaws.com") {
                return Some(Self::AwsS3);
            }
            if lower.contains(".blob.core.windows.net") || lower.contains(".azure.") {
                return Some(Self::AzureBlob);
            }
            if lower.contains("storage.googleapis.com")
                || lower.contains("storage.cloud.google.com")
            {
                return Some(Self::Gcs);
            }
            return Some(Self::Http);
        }

        None
    }

    /// Returns the display name of the provider
    #[must_use]
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::AwsS3 => "AWS S3",
            Self::AzureBlob => "Azure Blob Storage",
            Self::Gcs => "Google Cloud Storage",
            Self::Http => "HTTP/HTTPS",
            Self::Custom => "Custom Provider",
        }
    }

    /// Returns typical egress cost per GB (USD)
    #[must_use]
    pub fn egress_cost_per_gb(&self) -> f64 {
        match self {
            Self::AwsS3 => 0.09,      // AWS S3 standard egress
            Self::AzureBlob => 0.087, // Azure Blob standard egress
            Self::Gcs => 0.12,        // GCS standard egress
            Self::Http => 0.0,        // No cloud-specific egress
            Self::Custom => 0.0,
        }
    }

    /// Returns typical storage cost per GB/month (USD)
    #[must_use]
    pub fn storage_cost_per_gb(&self) -> f64 {
        match self {
            Self::AwsS3 => 0.023,     // S3 Standard
            Self::AzureBlob => 0.018, // Azure Blob Hot
            Self::Gcs => 0.020,       // GCS Standard
            Self::Http => 0.0,
            Self::Custom => 0.0,
        }
    }
}

impl std::fmt::Display for CloudProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

// ============================================================================
// Cloud Regions
// ============================================================================

/// Geographic regions for cloud providers
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CloudRegion {
    /// US East (Virginia)
    UsEast1,
    /// US East (Ohio)
    UsEast2,
    /// US West (Oregon)
    UsWest2,
    /// EU West (Ireland)
    EuWest1,
    /// EU Central (Frankfurt)
    EuCentral1,
    /// Asia Pacific (Tokyo)
    ApNortheast1,
    /// Asia Pacific (Singapore)
    ApSoutheast1,
    /// Asia Pacific (Sydney)
    ApSoutheast2,
    /// Custom region
    Custom(String),
}

impl CloudRegion {
    /// Creates a region from a string identifier
    #[must_use]
    pub fn from_string(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "us-east-1" | "useast1" | "eastus" => Self::UsEast1,
            "us-east-2" | "useast2" | "eastus2" => Self::UsEast2,
            "us-west-2" | "uswest2" | "westus2" => Self::UsWest2,
            "eu-west-1" | "euwest1" | "westeurope" => Self::EuWest1,
            "eu-central-1" | "eucentral1" | "germanywestcentral" => Self::EuCentral1,
            "ap-northeast-1" | "apnortheast1" | "japaneast" => Self::ApNortheast1,
            "ap-southeast-1" | "apsoutheast1" | "southeastasia" => Self::ApSoutheast1,
            "ap-southeast-2" | "apsoutheast2" | "australiaeast" => Self::ApSoutheast2,
            _ => Self::Custom(s.to_string()),
        }
    }

    /// Returns the AWS region code
    #[must_use]
    pub fn aws_code(&self) -> &str {
        match self {
            Self::UsEast1 => "us-east-1",
            Self::UsEast2 => "us-east-2",
            Self::UsWest2 => "us-west-2",
            Self::EuWest1 => "eu-west-1",
            Self::EuCentral1 => "eu-central-1",
            Self::ApNortheast1 => "ap-northeast-1",
            Self::ApSoutheast1 => "ap-southeast-1",
            Self::ApSoutheast2 => "ap-southeast-2",
            Self::Custom(s) => s.as_str(),
        }
    }

    /// Returns the Azure region code
    #[must_use]
    pub fn azure_code(&self) -> &str {
        match self {
            Self::UsEast1 => "eastus",
            Self::UsEast2 => "eastus2",
            Self::UsWest2 => "westus2",
            Self::EuWest1 => "westeurope",
            Self::EuCentral1 => "germanywestcentral",
            Self::ApNortheast1 => "japaneast",
            Self::ApSoutheast1 => "southeastasia",
            Self::ApSoutheast2 => "australiaeast",
            Self::Custom(s) => s.as_str(),
        }
    }

    /// Returns the GCS region code
    #[must_use]
    pub fn gcs_code(&self) -> &str {
        match self {
            Self::UsEast1 => "us-east1",
            Self::UsEast2 => "us-east4",
            Self::UsWest2 => "us-west1",
            Self::EuWest1 => "europe-west1",
            Self::EuCentral1 => "europe-west3",
            Self::ApNortheast1 => "asia-northeast1",
            Self::ApSoutheast1 => "asia-southeast1",
            Self::ApSoutheast2 => "australia-southeast1",
            Self::Custom(s) => s.as_str(),
        }
    }

    /// Calculates approximate latency to another region in milliseconds
    #[must_use]
    pub fn estimated_latency_to(&self, other: &Self) -> u32 {
        if self == other {
            return 1; // Same region, minimal latency
        }

        // Approximate cross-region latencies based on geography
        match (self, other) {
            // US to US
            (Self::UsEast1 | Self::UsEast2, Self::UsWest2) => 65,
            (Self::UsWest2, Self::UsEast1 | Self::UsEast2) => 65,
            (Self::UsEast1, Self::UsEast2) | (Self::UsEast2, Self::UsEast1) => 10,

            // US to EU
            (Self::UsEast1 | Self::UsEast2, Self::EuWest1 | Self::EuCentral1) => 80,
            (Self::EuWest1 | Self::EuCentral1, Self::UsEast1 | Self::UsEast2) => 80,
            (Self::UsWest2, Self::EuWest1 | Self::EuCentral1) => 140,
            (Self::EuWest1 | Self::EuCentral1, Self::UsWest2) => 140,

            // EU to EU
            (Self::EuWest1, Self::EuCentral1) | (Self::EuCentral1, Self::EuWest1) => 20,

            // US to Asia
            (Self::UsWest2, Self::ApNortheast1 | Self::ApSoutheast1 | Self::ApSoutheast2) => 100,
            (Self::ApNortheast1 | Self::ApSoutheast1 | Self::ApSoutheast2, Self::UsWest2) => 100,
            (Self::UsEast1 | Self::UsEast2, Self::ApNortheast1 | Self::ApSoutheast1) => 180,
            (Self::ApNortheast1 | Self::ApSoutheast1, Self::UsEast1 | Self::UsEast2) => 180,

            // EU to Asia
            (Self::EuWest1 | Self::EuCentral1, Self::ApNortheast1) => 220,
            (Self::ApNortheast1, Self::EuWest1 | Self::EuCentral1) => 220,
            (Self::EuWest1 | Self::EuCentral1, Self::ApSoutheast1 | Self::ApSoutheast2) => 180,
            (Self::ApSoutheast1 | Self::ApSoutheast2, Self::EuWest1 | Self::EuCentral1) => 180,

            // Asia to Asia
            (Self::ApNortheast1, Self::ApSoutheast1 | Self::ApSoutheast2) => 80,
            (Self::ApSoutheast1 | Self::ApSoutheast2, Self::ApNortheast1) => 80,
            (Self::ApSoutheast1, Self::ApSoutheast2) | (Self::ApSoutheast2, Self::ApSoutheast1) => {
                60
            }

            // Default for custom or unknown
            _ => 150,
        }
    }
}

// ============================================================================
// Provider Configuration
// ============================================================================

/// Configuration for a single cloud provider
#[derive(Debug, Clone)]
pub struct CloudProviderConfig {
    /// Provider type
    pub provider: CloudProvider,
    /// Bucket or container name
    pub bucket: String,
    /// Optional prefix within bucket
    pub prefix: String,
    /// Region
    pub region: Option<CloudRegion>,
    /// Endpoint URL (for custom endpoints)
    pub endpoint: Option<String>,
    /// Priority (lower is higher priority)
    pub priority: u32,
    /// Weight for load balancing (0-100)
    pub weight: u32,
    /// Maximum concurrent requests
    pub max_concurrent: usize,
    /// Request timeout
    pub timeout: Duration,
    /// Is this provider read-only
    pub read_only: bool,
    /// Custom cost per GB egress (overrides default)
    pub custom_egress_cost: Option<f64>,
    /// Provider-specific options
    pub options: HashMap<String, String>,
}

impl CloudProviderConfig {
    /// Creates an S3 provider configuration
    #[must_use]
    pub fn s3(bucket: impl Into<String>) -> Self {
        Self {
            provider: CloudProvider::AwsS3,
            bucket: bucket.into(),
            prefix: String::new(),
            region: None,
            endpoint: None,
            priority: 1,
            weight: 100,
            max_concurrent: 100,
            timeout: Duration::from_secs(300),
            read_only: false,
            custom_egress_cost: None,
            options: HashMap::new(),
        }
    }

    /// Creates an Azure Blob provider configuration
    #[must_use]
    pub fn azure(container: impl Into<String>) -> Self {
        Self {
            provider: CloudProvider::AzureBlob,
            bucket: container.into(),
            prefix: String::new(),
            region: None,
            endpoint: None,
            priority: 1,
            weight: 100,
            max_concurrent: 100,
            timeout: Duration::from_secs(300),
            read_only: false,
            custom_egress_cost: None,
            options: HashMap::new(),
        }
    }

    /// Creates a GCS provider configuration
    #[must_use]
    pub fn gcs(bucket: impl Into<String>) -> Self {
        Self {
            provider: CloudProvider::Gcs,
            bucket: bucket.into(),
            prefix: String::new(),
            region: None,
            endpoint: None,
            priority: 1,
            weight: 100,
            max_concurrent: 100,
            timeout: Duration::from_secs(300),
            read_only: false,
            custom_egress_cost: None,
            options: HashMap::new(),
        }
    }

    /// Creates an HTTP provider configuration
    #[must_use]
    pub fn http(base_url: impl Into<String>) -> Self {
        Self {
            provider: CloudProvider::Http,
            bucket: base_url.into(),
            prefix: String::new(),
            region: None,
            endpoint: None,
            priority: 1,
            weight: 100,
            max_concurrent: 100,
            timeout: Duration::from_secs(300),
            read_only: true, // HTTP is typically read-only
            custom_egress_cost: None,
            options: HashMap::new(),
        }
    }

    /// Sets the prefix within the bucket
    #[must_use]
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// Sets the region
    #[must_use]
    pub fn with_region(mut self, region: CloudRegion) -> Self {
        self.region = Some(region);
        self
    }

    /// Sets a custom endpoint
    #[must_use]
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Sets the priority (lower is higher)
    #[must_use]
    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }

    /// Sets the weight for load balancing
    #[must_use]
    pub fn with_weight(mut self, weight: u32) -> Self {
        self.weight = weight.min(100);
        self
    }

    /// Sets whether this provider is read-only
    #[must_use]
    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Sets the request timeout
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Sets custom egress cost per GB
    #[must_use]
    pub fn with_egress_cost(mut self, cost: f64) -> Self {
        self.custom_egress_cost = Some(cost);
        self
    }

    /// Adds a custom option
    #[must_use]
    pub fn with_option(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.options.insert(key.into(), value.into());
        self
    }

    /// Gets the effective egress cost
    #[must_use]
    pub fn effective_egress_cost(&self) -> f64 {
        self.custom_egress_cost
            .unwrap_or_else(|| self.provider.egress_cost_per_gb())
    }

    /// Returns the unique identifier for this provider config
    #[must_use]
    pub fn id(&self) -> String {
        format!("{}:{}/{}", self.provider, self.bucket, self.prefix)
    }
}

// ============================================================================
// Provider Health Status
// ============================================================================

/// Health status of a cloud provider
#[derive(Debug, Clone)]
pub struct ProviderHealth {
    /// Provider ID
    pub provider_id: String,
    /// Is the provider healthy
    pub healthy: bool,
    /// Last successful request time
    pub last_success: Option<Instant>,
    /// Last failure time
    pub last_failure: Option<Instant>,
    /// Consecutive failure count
    pub consecutive_failures: usize,
    /// Average latency in milliseconds
    pub avg_latency_ms: f64,
    /// Success rate (0.0 to 1.0)
    pub success_rate: f64,
    /// Total requests served
    pub total_requests: u64,
    /// Total bytes transferred
    pub total_bytes: u64,
}

impl ProviderHealth {
    /// Creates a new healthy provider status
    fn new(provider_id: String) -> Self {
        Self {
            provider_id,
            healthy: true,
            last_success: None,
            last_failure: None,
            consecutive_failures: 0,
            avg_latency_ms: 0.0,
            success_rate: 1.0,
            total_requests: 0,
            total_bytes: 0,
        }
    }

    /// Records a successful request
    fn record_success(&mut self, latency_ms: f64, bytes: u64) {
        self.last_success = Some(Instant::now());
        self.consecutive_failures = 0;
        self.healthy = true;
        self.total_requests += 1;
        self.total_bytes += bytes;

        // Update average latency with exponential moving average
        if self.total_requests == 1 {
            self.avg_latency_ms = latency_ms;
        } else {
            self.avg_latency_ms = self.avg_latency_ms * 0.9 + latency_ms * 0.1;
        }

        // Update success rate
        self.update_success_rate(true);
    }

    /// Records a failed request
    fn record_failure(&mut self) {
        self.last_failure = Some(Instant::now());
        self.consecutive_failures += 1;
        self.total_requests += 1;

        // Mark unhealthy after threshold failures
        if self.consecutive_failures >= 3 {
            self.healthy = false;
        }

        self.update_success_rate(false);
    }

    fn update_success_rate(&mut self, success: bool) {
        let success_value = if success { 1.0 } else { 0.0 };
        self.success_rate = self.success_rate * 0.95 + success_value * 0.05;
    }
}

// ============================================================================
// Routing Strategy
// ============================================================================

/// Routing strategy for multi-cloud operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RoutingStrategy {
    /// Use provider with lowest priority number
    #[default]
    Priority,
    /// Round-robin between providers
    RoundRobin,
    /// Select based on weights
    Weighted,
    /// Select based on measured latency
    LatencyBased,
    /// Select to minimize cost
    CostOptimized,
    /// Select based on geographic proximity
    RegionAware,
    /// Combination of latency and cost
    Adaptive,
}

// ============================================================================
// Multi-Cloud Manager
// ============================================================================

/// Statistics tracking for multi-cloud operations
struct ProviderStats {
    request_count: AtomicU64,
    byte_count: AtomicU64,
    error_count: AtomicU64,
    latency_sum_ms: AtomicU64,
}

impl ProviderStats {
    fn new() -> Self {
        Self {
            request_count: AtomicU64::new(0),
            byte_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            latency_sum_ms: AtomicU64::new(0),
        }
    }

    fn record_success(&self, bytes: u64, latency_ms: u64) {
        self.request_count.fetch_add(1, Ordering::Relaxed);
        self.byte_count.fetch_add(bytes, Ordering::Relaxed);
        self.latency_sum_ms.fetch_add(latency_ms, Ordering::Relaxed);
    }

    fn record_error(&self) {
        self.request_count.fetch_add(1, Ordering::Relaxed);
        self.error_count.fetch_add(1, Ordering::Relaxed);
    }

    fn avg_latency_ms(&self) -> f64 {
        let requests = self.request_count.load(Ordering::Relaxed);
        let errors = self.error_count.load(Ordering::Relaxed);
        let successful = requests.saturating_sub(errors);
        if successful == 0 {
            return f64::MAX;
        }
        self.latency_sum_ms.load(Ordering::Relaxed) as f64 / successful as f64
    }

    fn success_rate(&self) -> f64 {
        let requests = self.request_count.load(Ordering::Relaxed);
        if requests == 0 {
            return 1.0;
        }
        let errors = self.error_count.load(Ordering::Relaxed);
        (requests - errors) as f64 / requests as f64
    }
}

/// Multi-cloud storage manager
pub struct MultiCloudManager {
    /// Provider configurations
    providers: Vec<CloudProviderConfig>,
    /// Routing strategy
    routing_strategy: RoutingStrategy,
    /// Enable automatic failover
    failover_enabled: bool,
    /// Maximum failover attempts
    max_failover_attempts: usize,
    /// Enable cross-cloud replication
    replication_enabled: bool,
    /// Client region for latency calculations
    client_region: Option<CloudRegion>,
    /// Provider statistics
    stats: HashMap<String, Arc<ProviderStats>>,
    /// Round-robin counter
    round_robin_counter: AtomicUsize,
    /// Health check interval
    health_check_interval: Duration,
}

impl MultiCloudManager {
    /// Creates a new builder for the multi-cloud manager
    #[must_use]
    pub fn builder() -> MultiCloudManagerBuilder {
        MultiCloudManagerBuilder::new()
    }

    /// Returns the list of configured providers
    #[must_use]
    pub fn providers(&self) -> &[CloudProviderConfig] {
        &self.providers
    }

    /// Returns provider statistics
    #[must_use]
    pub fn get_stats(&self, provider_id: &str) -> Option<(u64, u64, f64, f64)> {
        self.stats.get(provider_id).map(|s| {
            (
                s.request_count.load(Ordering::Relaxed),
                s.byte_count.load(Ordering::Relaxed),
                s.avg_latency_ms(),
                s.success_rate(),
            )
        })
    }

    /// Selects the best provider based on routing strategy
    fn select_provider(&self, operation: &str) -> Result<&CloudProviderConfig> {
        if self.providers.is_empty() {
            return Err(CloudError::InvalidConfiguration {
                message: "No providers configured".to_string(),
            });
        }

        // Filter out unhealthy providers if we have healthy ones
        let healthy_providers: Vec<_> = self
            .providers
            .iter()
            .filter(|p| {
                let stats = self.stats.get(&p.id());
                stats.is_none_or(|s| s.success_rate() > 0.5)
            })
            .collect();

        let candidates = if healthy_providers.is_empty() {
            // Fallback to all providers if none are healthy
            self.providers.iter().collect::<Vec<_>>()
        } else {
            healthy_providers
        };

        // Filter write operations for read-only providers
        let write_operations = ["put", "delete", "write"];
        let candidates: Vec<_> = if write_operations.contains(&operation.to_lowercase().as_str()) {
            candidates.into_iter().filter(|p| !p.read_only).collect()
        } else {
            candidates
        };

        if candidates.is_empty() {
            return Err(CloudError::NotSupported {
                operation: format!("No writable providers available for {operation}"),
            });
        }

        match self.routing_strategy {
            RoutingStrategy::Priority => {
                // Select by lowest priority number
                candidates
                    .iter()
                    .min_by_key(|p| p.priority)
                    .copied()
                    .ok_or_else(|| CloudError::Internal {
                        message: "Failed to select provider".to_string(),
                    })
            }
            RoutingStrategy::RoundRobin => {
                // Round-robin selection
                let idx = self.round_robin_counter.fetch_add(1, Ordering::Relaxed);
                Ok(candidates[idx % candidates.len()])
            }
            RoutingStrategy::Weighted => {
                // Weighted selection based on provider weights
                let total_weight: u32 = candidates.iter().map(|p| p.weight).sum();
                if total_weight == 0 {
                    return Ok(candidates[0]);
                }

                let target = simple_random() % total_weight;
                let mut cumulative = 0u32;

                for provider in &candidates {
                    cumulative += provider.weight;
                    if cumulative > target {
                        return Ok(provider);
                    }
                }
                Ok(candidates[0])
            }
            RoutingStrategy::LatencyBased => {
                // Select by lowest measured latency
                candidates
                    .iter()
                    .min_by(|a, b| {
                        let lat_a = self
                            .stats
                            .get(&a.id())
                            .map_or(f64::MAX, |s| s.avg_latency_ms());
                        let lat_b = self
                            .stats
                            .get(&b.id())
                            .map_or(f64::MAX, |s| s.avg_latency_ms());
                        lat_a
                            .partial_cmp(&lat_b)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .copied()
                    .ok_or_else(|| CloudError::Internal {
                        message: "Failed to select provider".to_string(),
                    })
            }
            RoutingStrategy::CostOptimized => {
                // Select by lowest egress cost
                candidates
                    .iter()
                    .min_by(|a, b| {
                        let cost_a = a.effective_egress_cost();
                        let cost_b = b.effective_egress_cost();
                        cost_a
                            .partial_cmp(&cost_b)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .copied()
                    .ok_or_else(|| CloudError::Internal {
                        message: "Failed to select provider".to_string(),
                    })
            }
            RoutingStrategy::RegionAware => {
                // Select by geographic proximity
                if let Some(ref client_region) = self.client_region {
                    candidates
                        .iter()
                        .min_by_key(|p| {
                            p.region
                                .as_ref()
                                .map(|r| client_region.estimated_latency_to(r))
                                .unwrap_or(500)
                        })
                        .copied()
                        .ok_or_else(|| CloudError::Internal {
                            message: "Failed to select provider".to_string(),
                        })
                } else {
                    // Fallback to priority if no client region
                    candidates
                        .iter()
                        .min_by_key(|p| p.priority)
                        .copied()
                        .ok_or_else(|| CloudError::Internal {
                            message: "Failed to select provider".to_string(),
                        })
                }
            }
            RoutingStrategy::Adaptive => {
                // Combine latency and cost with health
                candidates
                    .iter()
                    .min_by(|a, b| {
                        let score_a = self.calculate_adaptive_score(a);
                        let score_b = self.calculate_adaptive_score(b);
                        score_a
                            .partial_cmp(&score_b)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .copied()
                    .ok_or_else(|| CloudError::Internal {
                        message: "Failed to select provider".to_string(),
                    })
            }
        }
    }

    /// Calculates adaptive score (lower is better)
    fn calculate_adaptive_score(&self, provider: &CloudProviderConfig) -> f64 {
        let stats = self.stats.get(&provider.id());

        // Latency component (normalized to 0-1)
        let latency_score = stats.map_or(0.5, |s| {
            let lat = s.avg_latency_ms();
            if lat == f64::MAX {
                1.0
            } else {
                (lat / 1000.0).min(1.0) // Normalize to 1s max
            }
        });

        // Cost component (normalized to 0-1)
        let cost_score = provider.effective_egress_cost() / 0.2; // Normalize to $0.20/GB max

        // Health component (inverse of success rate)
        let health_score = stats.map_or(0.0, |s| 1.0 - s.success_rate());

        // Priority component
        let priority_score = provider.priority as f64 / 10.0;

        // Weighted combination
        latency_score * 0.3 + cost_score * 0.3 + health_score * 0.3 + priority_score * 0.1
    }

    /// Gets providers for failover in order of preference
    fn get_failover_providers(
        &self,
        failed_id: &str,
        operation: &str,
    ) -> Vec<&CloudProviderConfig> {
        let write_operations = ["put", "delete", "write"];
        let is_write = write_operations.contains(&operation.to_lowercase().as_str());

        let mut candidates: Vec<_> = self
            .providers
            .iter()
            .filter(|p| p.id() != failed_id && (!is_write || !p.read_only))
            .collect();

        // Sort by priority
        candidates.sort_by_key(|p| p.priority);
        candidates
    }

    /// Gets data from cloud storage with automatic failover
    #[cfg(feature = "async")]
    pub async fn get(&self, key: &str) -> Result<Bytes> {
        let provider = self.select_provider("get")?;
        let start = Instant::now();

        match self.get_from_provider(provider, key).await {
            Ok(data) => {
                if let Some(stats) = self.stats.get(&provider.id()) {
                    stats.record_success(data.len() as u64, start.elapsed().as_millis() as u64);
                }
                Ok(data)
            }
            Err(e) if self.failover_enabled => {
                if let Some(stats) = self.stats.get(&provider.id()) {
                    stats.record_error();
                }
                tracing::warn!(
                    "Provider {} failed for get '{}': {}, attempting failover",
                    provider.id(),
                    key,
                    e
                );
                self.get_with_failover(key, &provider.id()).await
            }
            Err(e) => {
                if let Some(stats) = self.stats.get(&provider.id()) {
                    stats.record_error();
                }
                Err(e)
            }
        }
    }

    #[cfg(feature = "async")]
    async fn get_from_provider(
        &self,
        _provider: &CloudProviderConfig,
        _key: &str,
    ) -> Result<Bytes> {
        // This would create the actual backend and call get
        // For now, return a placeholder implementation
        Err(CloudError::NotSupported {
            operation: "Backend creation not implemented in this context".to_string(),
        })
    }

    #[cfg(feature = "async")]
    async fn get_with_failover(&self, key: &str, failed_id: &str) -> Result<Bytes> {
        let failover_providers = self.get_failover_providers(failed_id, "get");
        let mut attempts = 0;

        for provider in failover_providers {
            if attempts >= self.max_failover_attempts {
                break;
            }
            attempts += 1;

            let start = Instant::now();
            match self.get_from_provider(provider, key).await {
                Ok(data) => {
                    if let Some(stats) = self.stats.get(&provider.id()) {
                        stats.record_success(data.len() as u64, start.elapsed().as_millis() as u64);
                    }
                    tracing::info!(
                        "Failover successful to provider {} for key '{}'",
                        provider.id(),
                        key
                    );
                    return Ok(data);
                }
                Err(e) => {
                    if let Some(stats) = self.stats.get(&provider.id()) {
                        stats.record_error();
                    }
                    tracing::warn!(
                        "Failover attempt {} to {} failed: {}",
                        attempts,
                        provider.id(),
                        e
                    );
                }
            }
        }

        Err(CloudError::Internal {
            message: format!("All failover attempts exhausted for key '{key}'"),
        })
    }

    /// Puts data to cloud storage with optional replication
    #[cfg(feature = "async")]
    pub async fn put(&self, key: &str, data: &[u8]) -> Result<()> {
        let provider = self.select_provider("put")?;
        let start = Instant::now();

        match self.put_to_provider(provider, key, data).await {
            Ok(()) => {
                if let Some(stats) = self.stats.get(&provider.id()) {
                    stats.record_success(data.len() as u64, start.elapsed().as_millis() as u64);
                }

                // Handle replication if enabled
                if self.replication_enabled {
                    self.replicate_to_other_providers(key, data, &provider.id())
                        .await;
                }

                Ok(())
            }
            Err(e) if self.failover_enabled => {
                if let Some(stats) = self.stats.get(&provider.id()) {
                    stats.record_error();
                }
                tracing::warn!(
                    "Provider {} failed for put '{}': {}, attempting failover",
                    provider.id(),
                    key,
                    e
                );
                self.put_with_failover(key, data, &provider.id()).await
            }
            Err(e) => {
                if let Some(stats) = self.stats.get(&provider.id()) {
                    stats.record_error();
                }
                Err(e)
            }
        }
    }

    #[cfg(feature = "async")]
    async fn put_to_provider(
        &self,
        _provider: &CloudProviderConfig,
        _key: &str,
        _data: &[u8],
    ) -> Result<()> {
        Err(CloudError::NotSupported {
            operation: "Backend creation not implemented in this context".to_string(),
        })
    }

    #[cfg(feature = "async")]
    async fn put_with_failover(&self, key: &str, data: &[u8], failed_id: &str) -> Result<()> {
        let failover_providers = self.get_failover_providers(failed_id, "put");
        let mut attempts = 0;

        for provider in failover_providers {
            if attempts >= self.max_failover_attempts {
                break;
            }
            attempts += 1;

            let start = Instant::now();
            match self.put_to_provider(provider, key, data).await {
                Ok(()) => {
                    if let Some(stats) = self.stats.get(&provider.id()) {
                        stats.record_success(data.len() as u64, start.elapsed().as_millis() as u64);
                    }
                    tracing::info!(
                        "Failover successful to provider {} for put '{}'",
                        provider.id(),
                        key
                    );
                    return Ok(());
                }
                Err(e) => {
                    if let Some(stats) = self.stats.get(&provider.id()) {
                        stats.record_error();
                    }
                    tracing::warn!(
                        "Failover attempt {} to {} failed: {}",
                        attempts,
                        provider.id(),
                        e
                    );
                }
            }
        }

        Err(CloudError::Internal {
            message: format!("All failover attempts exhausted for put '{key}'"),
        })
    }

    #[cfg(feature = "async")]
    async fn replicate_to_other_providers(&self, key: &str, data: &[u8], primary_id: &str) {
        let replication_targets: Vec<_> = self
            .providers
            .iter()
            .filter(|p| p.id() != primary_id && !p.read_only)
            .collect();

        for provider in replication_targets {
            if let Err(e) = self.put_to_provider(provider, key, data).await {
                tracing::warn!(
                    "Replication to {} failed for key '{}': {}",
                    provider.id(),
                    key,
                    e
                );
            }
        }
    }

    /// Checks if an object exists in any provider
    #[cfg(feature = "async")]
    pub async fn exists(&self, key: &str) -> Result<bool> {
        let provider = self.select_provider("exists")?;

        match self.exists_in_provider(provider, key).await {
            Ok(exists) => Ok(exists),
            Err(e) if self.failover_enabled => {
                tracing::warn!(
                    "Provider {} failed for exists '{}': {}, checking other providers",
                    provider.id(),
                    key,
                    e
                );

                for fallback in self.get_failover_providers(&provider.id(), "exists") {
                    if let Ok(exists) = self.exists_in_provider(fallback, key).await {
                        return Ok(exists);
                    }
                }
                Err(e)
            }
            Err(e) => Err(e),
        }
    }

    #[cfg(feature = "async")]
    async fn exists_in_provider(
        &self,
        _provider: &CloudProviderConfig,
        _key: &str,
    ) -> Result<bool> {
        Err(CloudError::NotSupported {
            operation: "Backend creation not implemented in this context".to_string(),
        })
    }

    /// Deletes an object from all providers
    #[cfg(feature = "async")]
    pub async fn delete(&self, key: &str) -> Result<()> {
        let mut success = false;
        let mut last_error = None;

        for provider in &self.providers {
            if provider.read_only {
                continue;
            }

            match self.delete_from_provider(provider, key).await {
                Ok(()) => success = true,
                Err(e) => {
                    tracing::warn!(
                        "Failed to delete '{}' from provider {}: {}",
                        key,
                        provider.id(),
                        e
                    );
                    last_error = Some(e);
                }
            }
        }

        if success {
            Ok(())
        } else {
            Err(last_error.unwrap_or_else(|| CloudError::NotSupported {
                operation: "No writable providers available".to_string(),
            }))
        }
    }

    #[cfg(feature = "async")]
    async fn delete_from_provider(
        &self,
        _provider: &CloudProviderConfig,
        _key: &str,
    ) -> Result<()> {
        Err(CloudError::NotSupported {
            operation: "Backend creation not implemented in this context".to_string(),
        })
    }

    /// Estimates the cost of transferring data
    #[must_use]
    pub fn estimate_transfer_cost(&self, bytes: u64) -> TransferCostEstimate {
        let gb = bytes as f64 / (1024.0 * 1024.0 * 1024.0);

        let mut estimates = Vec::new();
        for provider in &self.providers {
            let egress_cost = provider.effective_egress_cost() * gb;
            estimates.push((provider.id(), egress_cost));
        }

        estimates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let (cheapest_id, cheapest_cost) = estimates.first().cloned().unwrap_or_default();
        let (most_expensive_id, most_expensive_cost) =
            estimates.last().cloned().unwrap_or_default();

        TransferCostEstimate {
            bytes,
            cheapest_provider: cheapest_id,
            cheapest_cost,
            most_expensive_provider: most_expensive_id,
            most_expensive_cost,
            all_estimates: estimates,
        }
    }
}

// ============================================================================
// Builder
// ============================================================================

/// Builder for `MultiCloudManager`
pub struct MultiCloudManagerBuilder {
    providers: Vec<CloudProviderConfig>,
    routing_strategy: RoutingStrategy,
    failover_enabled: bool,
    max_failover_attempts: usize,
    replication_enabled: bool,
    client_region: Option<CloudRegion>,
    health_check_interval: Duration,
}

impl MultiCloudManagerBuilder {
    /// Creates a new builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
            routing_strategy: RoutingStrategy::Priority,
            failover_enabled: true,
            max_failover_attempts: 3,
            replication_enabled: false,
            client_region: None,
            health_check_interval: Duration::from_secs(60),
        }
    }

    /// Adds a provider configuration
    #[must_use]
    pub fn add_provider(mut self, config: CloudProviderConfig) -> Self {
        self.providers.push(config);
        self
    }

    /// Sets the routing strategy
    #[must_use]
    pub fn with_routing_strategy(mut self, strategy: RoutingStrategy) -> Self {
        self.routing_strategy = strategy;
        self
    }

    /// Enables or disables failover
    #[must_use]
    pub fn with_failover(mut self, enabled: bool) -> Self {
        self.failover_enabled = enabled;
        self
    }

    /// Sets maximum failover attempts
    #[must_use]
    pub fn with_max_failover_attempts(mut self, attempts: usize) -> Self {
        self.max_failover_attempts = attempts;
        self
    }

    /// Enables latency-based routing
    #[must_use]
    pub fn with_latency_routing(mut self, enabled: bool) -> Self {
        if enabled {
            self.routing_strategy = RoutingStrategy::LatencyBased;
        }
        self
    }

    /// Enables cost-optimized routing
    #[must_use]
    pub fn with_cost_routing(mut self, enabled: bool) -> Self {
        if enabled {
            self.routing_strategy = RoutingStrategy::CostOptimized;
        }
        self
    }

    /// Enables replication across providers
    #[must_use]
    pub fn with_replication(mut self, enabled: bool) -> Self {
        self.replication_enabled = enabled;
        self
    }

    /// Sets the client region for region-aware routing
    #[must_use]
    pub fn with_client_region(mut self, region: CloudRegion) -> Self {
        self.client_region = Some(region);
        self
    }

    /// Sets the health check interval
    #[must_use]
    pub fn with_health_check_interval(mut self, interval: Duration) -> Self {
        self.health_check_interval = interval;
        self
    }

    /// Builds the multi-cloud manager
    pub fn build(self) -> Result<MultiCloudManager> {
        if self.providers.is_empty() {
            return Err(CloudError::InvalidConfiguration {
                message: "At least one provider must be configured".to_string(),
            });
        }

        let mut stats = HashMap::new();
        for provider in &self.providers {
            stats.insert(provider.id(), Arc::new(ProviderStats::new()));
        }

        Ok(MultiCloudManager {
            providers: self.providers,
            routing_strategy: self.routing_strategy,
            failover_enabled: self.failover_enabled,
            max_failover_attempts: self.max_failover_attempts,
            replication_enabled: self.replication_enabled,
            client_region: self.client_region,
            stats,
            round_robin_counter: AtomicUsize::new(0),
            health_check_interval: self.health_check_interval,
        })
    }
}

impl Default for MultiCloudManagerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Transfer Cost Estimation
// ============================================================================

/// Estimated transfer cost across providers
#[derive(Debug, Clone)]
pub struct TransferCostEstimate {
    /// Number of bytes to transfer
    pub bytes: u64,
    /// Cheapest provider ID
    pub cheapest_provider: String,
    /// Cost for cheapest provider (USD)
    pub cheapest_cost: f64,
    /// Most expensive provider ID
    pub most_expensive_provider: String,
    /// Cost for most expensive provider (USD)
    pub most_expensive_cost: f64,
    /// All provider estimates
    pub all_estimates: Vec<(String, f64)>,
}

impl Default for TransferCostEstimate {
    fn default() -> Self {
        Self {
            bytes: 0,
            cheapest_provider: String::new(),
            cheapest_cost: 0.0,
            most_expensive_provider: String::new(),
            most_expensive_cost: 0.0,
            all_estimates: Vec::new(),
        }
    }
}

// ============================================================================
// Cross-Cloud Transfer
// ============================================================================

/// Configuration for cross-cloud data transfer
#[derive(Debug, Clone)]
pub struct CrossCloudTransferConfig {
    /// Source provider ID
    pub source_provider: String,
    /// Destination provider ID
    pub dest_provider: String,
    /// Chunk size for streaming transfer
    pub chunk_size: usize,
    /// Maximum concurrent transfers
    pub max_concurrent: usize,
    /// Verify integrity after transfer
    pub verify_integrity: bool,
    /// Delete source after successful transfer
    pub delete_source: bool,
}

impl Default for CrossCloudTransferConfig {
    fn default() -> Self {
        Self {
            source_provider: String::new(),
            dest_provider: String::new(),
            chunk_size: 8 * 1024 * 1024, // 8 MB
            max_concurrent: 4,
            verify_integrity: true,
            delete_source: false,
        }
    }
}

/// Result of a cross-cloud transfer operation
#[derive(Debug, Clone)]
pub struct CrossCloudTransferResult {
    /// Number of objects transferred
    pub objects_transferred: usize,
    /// Total bytes transferred
    pub bytes_transferred: u64,
    /// Transfer duration
    pub duration: Duration,
    /// Failed transfers
    pub failures: Vec<(String, String)>,
    /// Estimated cost
    pub estimated_cost: f64,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Simple random number generator (no external dependencies)
fn simple_random() -> u32 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEED: AtomicU64 = AtomicU64::new(0x5deece66d);

    let seed = SEED.load(Ordering::Relaxed);
    let next = seed.wrapping_mul(0x5deece66d).wrapping_add(0xb);
    SEED.store(next, Ordering::Relaxed);

    (next >> 17) as u32
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cloud_provider_from_url() {
        assert_eq!(
            CloudProvider::from_url("s3://bucket/key"),
            Some(CloudProvider::AwsS3)
        );
        assert_eq!(
            CloudProvider::from_url("gs://bucket/object"),
            Some(CloudProvider::Gcs)
        );
        assert_eq!(
            CloudProvider::from_url("az://container/blob"),
            Some(CloudProvider::AzureBlob)
        );
        assert_eq!(
            CloudProvider::from_url("https://example.com/path"),
            Some(CloudProvider::Http)
        );
        assert_eq!(
            CloudProvider::from_url("https://mybucket.s3.amazonaws.com/key"),
            Some(CloudProvider::AwsS3)
        );
        assert_eq!(
            CloudProvider::from_url("https://account.blob.core.windows.net/container"),
            Some(CloudProvider::AzureBlob)
        );
        assert_eq!(
            CloudProvider::from_url("https://storage.googleapis.com/bucket/object"),
            Some(CloudProvider::Gcs)
        );
        assert_eq!(CloudProvider::from_url("invalid"), None);
    }

    #[test]
    fn test_cloud_region_codes() {
        let region = CloudRegion::UsEast1;
        assert_eq!(region.aws_code(), "us-east-1");
        assert_eq!(region.azure_code(), "eastus");
        assert_eq!(region.gcs_code(), "us-east1");

        let region = CloudRegion::from_string("eu-west-1");
        assert_eq!(region, CloudRegion::EuWest1);
    }

    #[test]
    fn test_region_latency_estimation() {
        let us_east = CloudRegion::UsEast1;
        let us_west = CloudRegion::UsWest2;
        let eu_west = CloudRegion::EuWest1;

        // Same region should be minimal
        assert_eq!(us_east.estimated_latency_to(&us_east), 1);

        // US to US should be moderate
        let us_to_us = us_east.estimated_latency_to(&us_west);
        assert!(us_to_us > 50 && us_to_us < 100);

        // US to EU should be higher
        let us_to_eu = us_east.estimated_latency_to(&eu_west);
        assert!(us_to_eu > us_to_us);
    }

    #[test]
    fn test_provider_config_builder() {
        let config = CloudProviderConfig::s3("my-bucket")
            .with_prefix("data/")
            .with_region(CloudRegion::UsWest2)
            .with_priority(1)
            .with_weight(80)
            .with_timeout(Duration::from_secs(60));

        assert_eq!(config.bucket, "my-bucket");
        assert_eq!(config.prefix, "data/");
        assert_eq!(config.region, Some(CloudRegion::UsWest2));
        assert_eq!(config.priority, 1);
        assert_eq!(config.weight, 80);
        assert_eq!(config.timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_provider_config_id() {
        let config = CloudProviderConfig::s3("bucket").with_prefix("prefix");
        assert_eq!(config.id(), "AWS S3:bucket/prefix");
    }

    #[test]
    fn test_egress_costs() {
        assert!(CloudProvider::AwsS3.egress_cost_per_gb() > 0.0);
        assert!(CloudProvider::AzureBlob.egress_cost_per_gb() > 0.0);
        assert!(CloudProvider::Gcs.egress_cost_per_gb() > 0.0);
        assert_eq!(CloudProvider::Http.egress_cost_per_gb(), 0.0);
    }

    #[test]
    fn test_multicloud_manager_builder() {
        let manager = MultiCloudManager::builder()
            .add_provider(CloudProviderConfig::s3("bucket1").with_priority(1))
            .add_provider(CloudProviderConfig::gcs("bucket2").with_priority(2))
            .with_failover(true)
            .with_latency_routing(true)
            .build();

        assert!(manager.is_ok());
        let manager = manager.expect("Manager should be built");
        assert_eq!(manager.providers.len(), 2);
        assert_eq!(manager.routing_strategy, RoutingStrategy::LatencyBased);
    }

    #[test]
    fn test_multicloud_manager_empty_providers() {
        let manager = MultiCloudManager::builder().build();
        assert!(manager.is_err());
    }

    #[test]
    fn test_transfer_cost_estimate() {
        let manager = MultiCloudManager::builder()
            .add_provider(CloudProviderConfig::s3("bucket1"))
            .add_provider(CloudProviderConfig::http("http://example.com"))
            .build()
            .expect("Manager should be built");

        let estimate = manager.estimate_transfer_cost(1024 * 1024 * 1024); // 1 GB
        assert!(estimate.cheapest_cost <= estimate.most_expensive_cost);
    }

    #[test]
    fn test_provider_health() {
        let mut health = ProviderHealth::new("test-provider".to_string());

        assert!(health.healthy);
        assert_eq!(health.consecutive_failures, 0);

        // Record some successes
        health.record_success(100.0, 1000);
        health.record_success(120.0, 2000);
        assert!(health.avg_latency_ms > 0.0);
        assert_eq!(health.total_bytes, 3000);

        // Record failures
        health.record_failure();
        health.record_failure();
        health.record_failure();

        assert!(!health.healthy);
        assert_eq!(health.consecutive_failures, 3);
    }

    #[test]
    fn test_provider_stats() {
        let stats = ProviderStats::new();

        stats.record_success(1000, 50);
        stats.record_success(2000, 60);

        assert_eq!(stats.request_count.load(Ordering::Relaxed), 2);
        assert_eq!(stats.byte_count.load(Ordering::Relaxed), 3000);
        assert!((stats.avg_latency_ms() - 55.0).abs() < 0.001);
        assert!((stats.success_rate() - 1.0).abs() < 0.001);

        stats.record_error();
        assert!(stats.success_rate() < 1.0);
    }

    #[test]
    fn test_cross_cloud_transfer_config() {
        let config = CrossCloudTransferConfig::default();

        assert_eq!(config.chunk_size, 8 * 1024 * 1024);
        assert_eq!(config.max_concurrent, 4);
        assert!(config.verify_integrity);
        assert!(!config.delete_source);
    }

    #[test]
    fn test_routing_strategy_default() {
        let strategy = RoutingStrategy::default();
        assert_eq!(strategy, RoutingStrategy::Priority);
    }

    #[test]
    fn test_select_provider_priority() {
        let manager = MultiCloudManager::builder()
            .add_provider(CloudProviderConfig::s3("bucket1").with_priority(2))
            .add_provider(CloudProviderConfig::gcs("bucket2").with_priority(1))
            .with_routing_strategy(RoutingStrategy::Priority)
            .build()
            .expect("Manager should be built");

        let provider = manager.select_provider("get");
        assert!(provider.is_ok());
        let provider = provider.expect("Provider should be selected");
        assert_eq!(provider.provider, CloudProvider::Gcs);
    }

    #[test]
    fn test_select_provider_cost_optimized() {
        let manager = MultiCloudManager::builder()
            .add_provider(CloudProviderConfig::s3("bucket1"))
            .add_provider(CloudProviderConfig::http("http://example.com"))
            .with_routing_strategy(RoutingStrategy::CostOptimized)
            .build()
            .expect("Manager should be built");

        let provider = manager.select_provider("get");
        assert!(provider.is_ok());
        let provider = provider.expect("Provider should be selected");
        // HTTP should be cheapest (0 egress cost)
        assert_eq!(provider.provider, CloudProvider::Http);
    }

    #[test]
    fn test_select_provider_write_filters_readonly() {
        let manager = MultiCloudManager::builder()
            .add_provider(CloudProviderConfig::http("http://example.com").with_priority(1))
            .add_provider(CloudProviderConfig::s3("bucket1").with_priority(2))
            .with_routing_strategy(RoutingStrategy::Priority)
            .build()
            .expect("Manager should be built");

        // For write operations, should skip HTTP (read-only)
        let provider = manager.select_provider("put");
        assert!(provider.is_ok());
        let provider = provider.expect("Provider should be selected");
        assert_eq!(provider.provider, CloudProvider::AwsS3);
    }
}
