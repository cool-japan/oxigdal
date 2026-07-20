//! Cron-based workflow scheduling.

use crate::error::{Result, WorkflowError};
use crate::scheduler::SchedulerConfig;
use chrono::{DateTime, Utc};
use cron::Schedule as CronScheduleParser;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Cron schedule definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronSchedule {
    /// Cron expression (standard 5-field or 6-field with seconds).
    pub expression: String,
    /// Time zone for evaluation.
    pub timezone: String,
    /// Description of the schedule.
    pub description: Option<String>,
}

impl CronSchedule {
    /// Create a new cron schedule.
    pub fn new<S: Into<String>>(expression: S) -> Result<Self> {
        let expr = expression.into();

        // Validate the cron expression
        CronScheduleParser::from_str(&expr).map_err(|e| {
            WorkflowError::cron_expression(format!("Invalid cron expression '{}': {}", expr, e))
        })?;

        Ok(Self {
            expression: expr,
            timezone: "UTC".to_string(),
            description: None,
        })
    }

    /// Set the timezone for this schedule.
    pub fn with_timezone<S: Into<String>>(mut self, timezone: S) -> Self {
        self.timezone = timezone.into();
        self
    }

    /// Set the description for this schedule.
    pub fn with_description<S: Into<String>>(mut self, description: S) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Calculate the next execution time from the given datetime.
    pub fn next_execution_from(&self, from: DateTime<Utc>) -> Result<Option<DateTime<Utc>>> {
        let schedule = CronScheduleParser::from_str(&self.expression).map_err(|e| {
            WorkflowError::cron_expression(format!("Invalid cron expression: {}", e))
        })?;

        Ok(schedule.after(&from).next())
    }

    /// Calculate all execution times within a given range.
    pub fn executions_in_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        max_count: usize,
    ) -> Result<Vec<DateTime<Utc>>> {
        let schedule = CronScheduleParser::from_str(&self.expression).map_err(|e| {
            WorkflowError::cron_expression(format!("Invalid cron expression: {}", e))
        })?;

        let mut executions = Vec::new();
        for datetime in schedule.after(&start).take(max_count) {
            if datetime > end {
                break;
            }
            executions.push(datetime);
        }

        Ok(executions)
    }

    /// Check if this schedule should execute at the given time (within 1 second tolerance).
    pub fn should_execute_at(&self, time: DateTime<Utc>) -> Result<bool> {
        let next = self.next_execution_from(
            time - chrono::Duration::try_seconds(2)
                .ok_or_else(|| WorkflowError::internal("Duration overflow"))?,
        )?;

        if let Some(next_time) = next {
            let diff = (next_time - time).num_seconds().abs();
            Ok(diff <= 1)
        } else {
            Ok(false)
        }
    }
}

/// Cron scheduler for managing cron-based workflow executions.
pub struct CronScheduler {
    config: SchedulerConfig,
}

impl CronScheduler {
    /// Create a new cron scheduler.
    pub fn new(config: SchedulerConfig) -> Self {
        Self { config }
    }

    /// Calculate the next execution time for a cron expression.
    pub fn calculate_next_execution(
        &self,
        expression: &str,
        from: DateTime<Utc>,
    ) -> Result<Option<DateTime<Utc>>> {
        let schedule = CronScheduleParser::from_str(expression).map_err(|e| {
            WorkflowError::cron_expression(format!(
                "Invalid cron expression '{}': {}",
                expression, e
            ))
        })?;

        Ok(schedule.after(&from).next())
    }

    /// Calculate missed executions between two times.
    pub fn calculate_missed_executions(
        &self,
        expression: &str,
        last_execution: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<Vec<DateTime<Utc>>> {
        if !self.config.handle_missed_executions {
            return Ok(Vec::new());
        }

        let schedule = CronScheduleParser::from_str(expression).map_err(|e| {
            WorkflowError::cron_expression(format!("Invalid cron expression: {}", e))
        })?;

        let mut missed = Vec::new();
        for datetime in schedule
            .after(&last_execution)
            .take(self.config.max_missed_executions)
        {
            if datetime >= now {
                break;
            }
            missed.push(datetime);
        }

        Ok(missed)
    }

    /// Validate a cron expression.
    pub fn validate_expression(expression: &str) -> Result<()> {
        CronScheduleParser::from_str(expression).map_err(|e| {
            WorkflowError::cron_expression(format!(
                "Invalid cron expression '{}': {}",
                expression, e
            ))
        })?;
        Ok(())
    }

    /// Get human-readable description of a cron expression.
    pub fn describe_expression(expression: &str) -> Result<String> {
        // Validate first
        Self::validate_expression(expression)?;

        // Simple description (could be enhanced with a cron descriptor library)
        Ok(format!("Cron schedule: {}", expression))
    }
}

/// Common cron schedule patterns.
pub struct CronPatterns;

impl CronPatterns {
    /// Every minute.
    pub fn every_minute() -> &'static str {
        "0 * * * * *"
    }

    /// Every 5 minutes.
    pub fn every_5_minutes() -> &'static str {
        "0 */5 * * * *"
    }

    /// Every 15 minutes.
    pub fn every_15_minutes() -> &'static str {
        "0 */15 * * * *"
    }

    /// Every 30 minutes.
    pub fn every_30_minutes() -> &'static str {
        "0 */30 * * * *"
    }

    /// Every hour.
    pub fn every_hour() -> &'static str {
        "0 0 * * * *"
    }

    /// Every day at midnight.
    pub fn daily() -> &'static str {
        "0 0 0 * * *"
    }

    /// Every day at noon.
    pub fn daily_at_noon() -> &'static str {
        "0 0 12 * * *"
    }

    /// Every week on Sunday at midnight.
    pub fn weekly() -> &'static str {
        "0 0 0 * * 0"
    }

    /// Every month on the 1st at midnight.
    pub fn monthly() -> &'static str {
        "0 0 0 1 * *"
    }

    /// Every year on January 1st at midnight.
    pub fn yearly() -> &'static str {
        "0 0 0 1 1 *"
    }

    /// Weekdays only at 9 AM.
    pub fn weekdays_at_9am() -> &'static str {
        "0 0 9 * * 1-5"
    }

    /// Weekends only at 10 AM.
    pub fn weekends_at_10am() -> &'static str {
        "0 0 10 * * 0,6"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cron_schedule_creation() {
        let schedule = CronSchedule::new("0 0 0 * * *").expect("Failed to create schedule");
        assert_eq!(schedule.expression, "0 0 0 * * *");
        assert_eq!(schedule.timezone, "UTC");
    }

    #[test]
    fn test_invalid_cron_expression() {
        let result = CronSchedule::new("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_next_execution() {
        let schedule = CronSchedule::new("0 0 0 * * *").expect("Failed to create schedule");
        let now = Utc::now();
        let next = schedule
            .next_execution_from(now)
            .expect("Failed to calculate next execution");
        assert!(next.is_some());
    }

    #[test]
    fn test_cron_patterns() {
        assert_eq!(CronPatterns::every_minute(), "0 * * * * *");
        assert_eq!(CronPatterns::daily(), "0 0 0 * * *");
        assert_eq!(CronPatterns::weekly(), "0 0 0 * * 0");
    }

    #[test]
    fn test_validate_expression() {
        assert!(CronScheduler::validate_expression("0 0 0 * * *").is_ok());
        assert!(CronScheduler::validate_expression("invalid").is_err());
    }

    #[test]
    fn test_executions_in_range() {
        let schedule = CronSchedule::new("0 0 * * * *").expect("Failed to create schedule");
        let start = Utc::now();
        let end = start + chrono::Duration::try_hours(5).expect("Duration overflow");

        let executions = schedule
            .executions_in_range(start, end, 10)
            .expect("Failed to get executions");

        assert!(!executions.is_empty());
        assert!(executions.len() <= 10);
    }
}
