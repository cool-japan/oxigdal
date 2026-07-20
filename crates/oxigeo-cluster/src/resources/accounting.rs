//! Resource usage accounting and billing.

use crate::error::Result;
use crate::task_graph::{ResourceRequirements, TaskId};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// Account identifier.
pub type AccountId = String;

/// Resource usage record for accounting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    /// Task ID
    pub task_id: TaskId,
    /// Account ID
    pub account_id: AccountId,
    /// CPU core-seconds
    pub cpu_core_seconds: f64,
    /// Memory MB-seconds
    pub memory_mb_seconds: f64,
    /// GPU seconds
    pub gpu_seconds: f64,
    /// Disk MB-seconds
    pub disk_mb_seconds: f64,
    /// Start time
    pub start_time: SystemTime,
    /// End time
    pub end_time: SystemTime,
    /// Duration
    pub duration: Duration,
    /// Cost (if applicable)
    pub cost: Option<f64>,
}

/// Account usage summary.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountUsage {
    /// Total CPU core-seconds
    pub total_cpu_core_seconds: f64,
    /// Total memory MB-seconds
    pub total_memory_mb_seconds: f64,
    /// Total GPU seconds
    pub total_gpu_seconds: f64,
    /// Total disk MB-seconds
    pub total_disk_mb_seconds: f64,
    /// Total tasks
    pub total_tasks: u64,
    /// Total cost
    pub total_cost: f64,
    /// First usage timestamp
    pub first_usage: Option<SystemTime>,
    /// Last usage timestamp
    pub last_usage: Option<SystemTime>,
}

/// Pricing configuration for resource accounting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingConfig {
    /// Cost per CPU core-hour
    pub cpu_core_hour_price: f64,
    /// Cost per GB-hour of memory
    pub memory_gb_hour_price: f64,
    /// Cost per GPU-hour
    pub gpu_hour_price: f64,
    /// Cost per GB-hour of disk
    pub disk_gb_hour_price: f64,
}

impl Default for PricingConfig {
    fn default() -> Self {
        Self {
            cpu_core_hour_price: 0.05,
            memory_gb_hour_price: 0.01,
            gpu_hour_price: 0.50,
            disk_gb_hour_price: 0.001,
        }
    }
}

/// Resource accounting manager.
pub struct AccountingManager {
    /// Usage records
    records: Arc<DashMap<TaskId, UsageRecord>>,
    /// Account summaries
    account_usage: Arc<DashMap<AccountId, RwLock<AccountUsage>>>,
    /// Active task tracking (task_id -> (account_id, resources, start_time))
    active_tasks: Arc<DashMap<TaskId, (AccountId, ResourceRequirements, Instant)>>,
    /// Pricing configuration
    pricing: Arc<RwLock<PricingConfig>>,
    /// Statistics
    stats: Arc<RwLock<AccountingStats>>,
}

/// Accounting statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountingStats {
    /// Total number of usage records
    pub total_records: u64,
    /// Total number of accounts tracked
    pub total_accounts: usize,
    /// Total revenue generated
    pub total_revenue: f64,
    /// Average task execution duration
    pub average_task_duration: Duration,
    /// Total CPU core-hours consumed
    pub total_cpu_hours: f64,
    /// Total memory GB-hours consumed
    pub total_memory_gb_hours: f64,
}

impl AccountingManager {
    /// Create a new accounting manager.
    pub fn new(pricing: PricingConfig) -> Self {
        Self {
            records: Arc::new(DashMap::new()),
            account_usage: Arc::new(DashMap::new()),
            active_tasks: Arc::new(DashMap::new()),
            pricing: Arc::new(RwLock::new(pricing)),
            stats: Arc::new(RwLock::new(AccountingStats::default())),
        }
    }

    /// Start tracking a task.
    pub fn start_task(
        &self,
        task_id: TaskId,
        account_id: AccountId,
        resources: ResourceRequirements,
    ) -> Result<()> {
        self.active_tasks
            .insert(task_id, (account_id, resources, Instant::now()));
        Ok(())
    }

    /// Stop tracking a task and record usage.
    pub fn stop_task(&self, task_id: TaskId) -> Result<Option<UsageRecord>> {
        let (account_id, resources, start_instant) = match self.active_tasks.remove(&task_id) {
            Some((_, value)) => value,
            None => return Ok(None),
        };

        let duration = start_instant.elapsed();
        let duration_hours = duration.as_secs_f64() / 3600.0;

        // Calculate resource usage
        let cpu_core_seconds = resources.cpu_cores * duration.as_secs_f64();
        let memory_mb = resources.memory_bytes as f64 / 1024.0 / 1024.0;
        let memory_mb_seconds = memory_mb * duration.as_secs_f64();
        let gpu_count = if resources.gpu { 1.0 } else { 0.0 };
        let gpu_seconds = gpu_count * duration.as_secs_f64();
        let disk_mb = resources.storage_bytes as f64 / 1024.0 / 1024.0;
        let disk_mb_seconds = disk_mb * duration.as_secs_f64();

        // Calculate cost
        let pricing = self.pricing.read();
        let cpu_cost = resources.cpu_cores * duration_hours * pricing.cpu_core_hour_price;
        let memory_cost = (memory_mb / 1024.0) * duration_hours * pricing.memory_gb_hour_price;
        let gpu_cost = gpu_count * duration_hours * pricing.gpu_hour_price;
        let disk_cost = (disk_mb / 1024.0) * duration_hours * pricing.disk_gb_hour_price;
        let total_cost = cpu_cost + memory_cost + gpu_cost + disk_cost;

        let now = SystemTime::now();
        let start_time = now - duration;

        let record = UsageRecord {
            task_id,
            account_id: account_id.clone(),
            cpu_core_seconds,
            memory_mb_seconds,
            gpu_seconds,
            disk_mb_seconds,
            start_time,
            end_time: now,
            duration,
            cost: Some(total_cost),
        };

        // Store record
        self.records.insert(task_id, record.clone());

        // Update account usage
        self.update_account_usage(&account_id, &record);

        // Update statistics
        self.update_stats(&record);

        Ok(Some(record))
    }

    fn update_account_usage(&self, account_id: &AccountId, record: &UsageRecord) {
        let entry = self
            .account_usage
            .entry(account_id.clone())
            .or_insert_with(|| RwLock::new(AccountUsage::default()));
        let mut usage = entry.write();

        usage.total_cpu_core_seconds += record.cpu_core_seconds;
        usage.total_memory_mb_seconds += record.memory_mb_seconds;
        usage.total_gpu_seconds += record.gpu_seconds;
        usage.total_disk_mb_seconds += record.disk_mb_seconds;
        usage.total_tasks += 1;
        usage.total_cost += record.cost.unwrap_or(0.0);

        if usage.first_usage.is_none() {
            usage.first_usage = Some(record.start_time);
        }
        usage.last_usage = Some(record.end_time);
    }

    fn update_stats(&self, record: &UsageRecord) {
        let mut stats = self.stats.write();

        stats.total_records += 1;
        stats.total_accounts = self.account_usage.len();
        stats.total_revenue += record.cost.unwrap_or(0.0);

        let total_duration_secs =
            stats.average_task_duration.as_secs_f64() * (stats.total_records - 1) as f64;
        stats.average_task_duration = Duration::from_secs_f64(
            (total_duration_secs + record.duration.as_secs_f64()) / stats.total_records as f64,
        );

        stats.total_cpu_hours += record.cpu_core_seconds / 3600.0;
        stats.total_memory_gb_hours += (record.memory_mb_seconds / 1024.0) / 3600.0;
    }

    /// Get usage record for a task.
    pub fn get_record(&self, task_id: &TaskId) -> Option<UsageRecord> {
        self.records.get(task_id).map(|r| r.clone())
    }

    /// Get account usage summary.
    pub fn get_account_usage(&self, account_id: &AccountId) -> Option<AccountUsage> {
        self.account_usage.get(account_id).map(|u| u.read().clone())
    }

    /// List all usage records for an account.
    pub fn list_account_records(&self, account_id: &AccountId) -> Vec<UsageRecord> {
        self.records
            .iter()
            .filter(|entry| entry.value().account_id == *account_id)
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Get records within a time range.
    pub fn get_records_in_range(
        &self,
        account_id: Option<&AccountId>,
        start: SystemTime,
        end: SystemTime,
    ) -> Vec<UsageRecord> {
        self.records
            .iter()
            .filter(|entry| {
                let record = entry.value();
                let time_match = record.start_time >= start && record.end_time <= end;
                let account_match = account_id.is_none_or(|id| &record.account_id == id);
                time_match && account_match
            })
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Generate invoice for an account.
    pub fn generate_invoice(
        &self,
        account_id: &AccountId,
        start: SystemTime,
        end: SystemTime,
    ) -> Invoice {
        let records = self.get_records_in_range(Some(account_id), start, end);

        let mut total_cost = 0.0;
        let mut total_cpu_hours = 0.0;
        let mut total_memory_gb_hours = 0.0;
        let mut total_gpu_hours = 0.0;
        let mut total_disk_gb_hours = 0.0;

        for record in &records {
            total_cost += record.cost.unwrap_or(0.0);
            total_cpu_hours += record.cpu_core_seconds / 3600.0;
            total_memory_gb_hours += (record.memory_mb_seconds / 1024.0) / 3600.0;
            total_gpu_hours += record.gpu_seconds / 3600.0;
            total_disk_gb_hours += (record.disk_mb_seconds / 1024.0) / 3600.0;
        }

        Invoice {
            account_id: account_id.clone(),
            period_start: start,
            period_end: end,
            total_cost,
            total_cpu_hours,
            total_memory_gb_hours,
            total_gpu_hours,
            total_disk_gb_hours,
            task_count: records.len(),
            records,
        }
    }

    /// Get top accounts by cost.
    pub fn get_top_accounts(&self, limit: usize) -> Vec<(AccountId, AccountUsage)> {
        let mut accounts: Vec<_> = self
            .account_usage
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().read().clone()))
            .collect();

        accounts.sort_by(|a, b| {
            b.1.total_cost
                .partial_cmp(&a.1.total_cost)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        accounts.truncate(limit);
        accounts
    }

    /// Update pricing configuration.
    pub fn update_pricing(&self, pricing: PricingConfig) {
        *self.pricing.write() = pricing;
    }

    /// Get current pricing.
    pub fn get_pricing(&self) -> PricingConfig {
        self.pricing.read().clone()
    }

    /// Get accounting statistics.
    pub fn get_stats(&self) -> AccountingStats {
        self.stats.read().clone()
    }

    /// Export usage data for analysis.
    pub fn export_usage_data(&self) -> HashMap<AccountId, Vec<UsageRecord>> {
        let mut export: HashMap<AccountId, Vec<UsageRecord>> = HashMap::new();

        for entry in self.records.iter() {
            let record = entry.value().clone();
            export
                .entry(record.account_id.clone())
                .or_default()
                .push(record);
        }

        export
    }
}

/// Invoice for billing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invoice {
    /// Account this invoice is for
    pub account_id: AccountId,
    /// Start of billing period
    pub period_start: SystemTime,
    /// End of billing period
    pub period_end: SystemTime,
    /// Total cost for the period
    pub total_cost: f64,
    /// Total CPU core-hours used
    pub total_cpu_hours: f64,
    /// Total memory GB-hours used
    pub total_memory_gb_hours: f64,
    /// Total GPU hours used
    pub total_gpu_hours: f64,
    /// Total disk GB-hours used
    pub total_disk_gb_hours: f64,
    /// Number of tasks in the period
    pub task_count: usize,
    /// Detailed usage records
    pub records: Vec<UsageRecord>,
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::task_graph::TaskId;

    #[test]
    fn test_task_accounting() {
        let pricing = PricingConfig::default();
        let manager = AccountingManager::new(pricing);

        let task_id = TaskId(uuid::Uuid::new_v4());
        let account_id = "account1".to_string();
        let resources = ResourceRequirements {
            cpu_cores: 2.0,
            memory_bytes: 4096 * 1024 * 1024,
            gpu: true,
            storage_bytes: 10240 * 1024 * 1024,
        };

        let _ = manager.start_task(task_id, account_id.clone(), resources);

        // Simulate task execution
        std::thread::sleep(std::time::Duration::from_millis(100));

        let record = manager.stop_task(task_id).ok().flatten();
        assert!(record.is_some());

        let record = record.expect("record should exist");
        assert!(record.cost.expect("cost should exist") > 0.0);
        assert!(record.cpu_core_seconds > 0.0);

        let usage = manager.get_account_usage(&account_id);
        assert!(usage.is_some());

        let usage = usage.expect("usage should exist");
        assert_eq!(usage.total_tasks, 1);
        assert!(usage.total_cost > 0.0);
    }

    #[test]
    fn test_invoice_generation() {
        let pricing = PricingConfig::default();
        let manager = AccountingManager::new(pricing);

        let account_id = "account1".to_string();

        // Create some usage records
        for _i in 0..5 {
            let task_id = TaskId(uuid::Uuid::new_v4());
            let resources = ResourceRequirements {
                cpu_cores: 2.0,
                memory_bytes: 4096 * 1024 * 1024,
                gpu: false,
                storage_bytes: 10240 * 1024 * 1024,
            };

            let _ = manager.start_task(task_id, account_id.clone(), resources);
            std::thread::sleep(std::time::Duration::from_millis(10));
            let _ = manager.stop_task(task_id);
        }

        let start = SystemTime::now() - Duration::from_secs(60);
        let end = SystemTime::now();

        let invoice = manager.generate_invoice(&account_id, start, end);

        assert_eq!(invoice.task_count, 5);
        assert!(invoice.total_cost > 0.0);
        assert!(invoice.total_cpu_hours > 0.0);
    }
}
