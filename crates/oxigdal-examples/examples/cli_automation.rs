//! CLI Automation Example - Batch Processing Pipeline
#![allow(missing_docs)]
//!
//! This example demonstrates automated batch processing:
//! 1. Scan directory for geospatial files
//! 2. Process files programmatically using OxiGDAL CLI
//! 3. Implement error handling and retry logic
//! 4. Monitor progress across batch
//! 5. Generate processing report
//!
//! This workflow is useful for:
//! - Automated data pipelines
//! - Scheduled batch processing
//! - Integration with other tools
//! - CI/CD workflows
//!
//! # Usage
//!
//! ```bash
//! cargo run --example cli_automation
//! ```
//!
//! # Workflow
//!
//! Scan Files → Batch Process → Error Handling → Progress Monitor → Report
//!
//! # Features
//!
//! - Parallel processing
//! - Automatic retry on failure
//! - Progress tracking
//! - Detailed logging
//! - Summary report

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use thiserror::Error;

/// Custom error types for automation
#[derive(Debug, Error)]
pub enum AutomationError {
    /// Command execution failed
    #[error("Command failed: {0}")]
    CommandFailed(String),

    /// File not found
    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    /// Processing error
    #[error("Processing error: {0}")]
    Processing(String),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Maximum retries exceeded
    #[error("Max retries exceeded for {0}")]
    MaxRetriesExceeded(String),
}

type Result<T> = std::result::Result<T, AutomationError>;

/// Processing operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operation {
    /// Convert to Cloud-Optimized GeoTIFF
    ConvertToCog,
    /// Reproject to different CRS
    Reproject,
    /// Generate overviews
    GenerateOverviews,
    /// Calculate statistics
    CalculateStats,
    /// Validate file
    Validate,
}

impl Operation {
    /// Get operation name
    pub fn name(&self) -> &str {
        match self {
            Self::ConvertToCog => "convert-to-cog",
            Self::Reproject => "reproject",
            Self::GenerateOverviews => "generate-overviews",
            Self::CalculateStats => "calculate-stats",
            Self::Validate => "validate",
        }
    }

    /// Get CLI command for operation
    pub fn cli_command(&self) -> &str {
        match self {
            Self::ConvertToCog => "translate",
            Self::Reproject => "warp",
            Self::GenerateOverviews => "addo",
            Self::CalculateStats => "info",
            Self::Validate => "info",
        }
    }
}

/// Processing task
#[derive(Debug, Clone)]
pub struct ProcessingTask {
    /// Input file path
    pub input: PathBuf,
    /// Output file path
    pub output: PathBuf,
    /// Operation to perform
    pub operation: Operation,
    /// Maximum retry attempts
    pub max_retries: usize,
    /// Task priority (higher = more important)
    pub priority: i32,
}

/// Processing result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingResult {
    /// Input file
    pub input: String,
    /// Output file
    pub output: String,
    /// Operation performed
    pub operation: String,
    /// Success status
    pub success: bool,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Processing time in seconds
    pub duration_secs: f64,
    /// Number of retry attempts
    pub retry_count: usize,
}

/// Batch processing statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchStatistics {
    /// Total tasks
    pub total_tasks: usize,
    /// Successful tasks
    pub successful: usize,
    /// Failed tasks
    pub failed: usize,
    /// Total processing time
    pub total_duration_secs: f64,
    /// Average processing time per task
    pub avg_duration_secs: f64,
    /// Operations breakdown
    pub operations: HashMap<String, usize>,
}

/// Automation pipeline
pub struct AutomationPipeline {
    /// Output directory
    output_dir: TempDir,
    /// Default retry count
    default_retries: usize,
    /// Retry delay in milliseconds
    retry_delay_ms: u64,
    /// Processing results
    results: Vec<ProcessingResult>,
}

impl AutomationPipeline {
    /// Create a new automation pipeline
    pub fn new(default_retries: usize, retry_delay_ms: u64) -> Result<Self> {
        println!("Initializing CLI automation pipeline...");
        println!("  Default retries: {}", default_retries);
        println!("  Retry delay: {}ms", retry_delay_ms);

        let output_dir = TempDir::new()?;

        Ok(Self {
            output_dir,
            default_retries,
            retry_delay_ms,
            results: Vec::new(),
        })
    }

    /// Generate sample input files
    fn generate_sample_files(&self, count: usize) -> Result<Vec<PathBuf>> {
        println!("\nGenerating {} sample files...", count);

        let input_dir = self.output_dir.path().join("inputs");
        std::fs::create_dir_all(&input_dir)?;

        let mut files = Vec::new();

        for i in 0..count {
            let filename = format!("sample_{:03}.tif", i);
            let filepath = input_dir.join(&filename);

            // Create a dummy file
            std::fs::write(&filepath, b"GeoTIFF placeholder")?;

            files.push(filepath);

            if (i + 1) % 10 == 0 {
                println!("  Created {} files...", i + 1);
            }
        }

        println!("  ✓ Created {} files", files.len());

        Ok(files)
    }

    /// Create batch tasks from input files
    fn create_batch_tasks(&self, input_files: &[PathBuf]) -> Result<Vec<ProcessingTask>> {
        println!("\nCreating batch tasks...");

        let output_dir = self.output_dir.path().join("outputs");
        std::fs::create_dir_all(&output_dir)?;

        let operations = [
            Operation::Validate,
            Operation::CalculateStats,
            Operation::ConvertToCog,
            Operation::GenerateOverviews,
        ];

        let mut tasks = Vec::new();

        for (i, input_file) in input_files.iter().enumerate() {
            let operation = operations[i % operations.len()];

            let output_filename = format!(
                "{}_{}{}",
                input_file
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("output"),
                operation.name(),
                input_file
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| format!(".{}", s))
                    .unwrap_or_default()
            );

            let output_file = output_dir.join(&output_filename);

            tasks.push(ProcessingTask {
                input: input_file.clone(),
                output: output_file,
                operation,
                max_retries: self.default_retries,
                priority: (i % 3) as i32, // Vary priority
            });
        }

        println!("  ✓ Created {} tasks", tasks.len());
        println!("    Operations breakdown:");
        let mut op_counts: HashMap<String, usize> = HashMap::new();
        for task in &tasks {
            *op_counts
                .entry(task.operation.name().to_string())
                .or_insert(0) += 1;
        }
        for (op, count) in &op_counts {
            println!("      {}: {} tasks", op, count);
        }

        Ok(tasks)
    }

    /// Execute a single task with retry logic
    fn execute_task(&self, task: &ProcessingTask) -> ProcessingResult {
        let start = Instant::now();
        let mut retry_count = 0;
        let mut last_error = None;

        // Try executing the task with retries
        for attempt in 0..=task.max_retries {
            if attempt > 0 {
                retry_count = attempt;
                println!("    Retry attempt {} for {}", attempt, task.input.display());
                std::thread::sleep(Duration::from_millis(self.retry_delay_ms));
            }

            match self.run_cli_command(task) {
                Ok(_) => {
                    let duration = start.elapsed();
                    return ProcessingResult {
                        input: task.input.display().to_string(),
                        output: task.output.display().to_string(),
                        operation: task.operation.name().to_string(),
                        success: true,
                        error: None,
                        duration_secs: duration.as_secs_f64(),
                        retry_count,
                    };
                }
                Err(e) => {
                    last_error = Some(e.to_string());
                }
            }
        }

        // All retries failed
        let duration = start.elapsed();
        ProcessingResult {
            input: task.input.display().to_string(),
            output: task.output.display().to_string(),
            operation: task.operation.name().to_string(),
            success: false,
            error: last_error,
            duration_secs: duration.as_secs_f64(),
            retry_count,
        }
    }

    /// Run CLI command (simulated)
    fn run_cli_command(&self, task: &ProcessingTask) -> Result<()> {
        // Simulate CLI command execution
        // In production, this would actually call the oxigdal CLI:
        //   Command::new("oxigdal")
        //       .arg(task.operation.cli_command())
        //       .arg(&task.input)
        //       .arg(&task.output)
        //       .output()?

        // Simulate processing time
        let processing_time_ms = match task.operation {
            Operation::Validate => 10,
            Operation::CalculateStats => 50,
            Operation::ConvertToCog => 200,
            Operation::GenerateOverviews => 150,
            Operation::Reproject => 300,
        };

        std::thread::sleep(Duration::from_millis(processing_time_ms));

        // Simulate 10% failure rate for demonstration
        let filename = task
            .input
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        if filename.contains("7") {
            // Simulate failure for files with '7' in name
            return Err(AutomationError::Processing(format!(
                "Simulated failure for {}",
                filename
            )));
        }

        // Create output file
        std::fs::write(&task.output, b"Processed output")?;

        Ok(())
    }

    /// Process batch with progress monitoring
    fn process_batch(&mut self, tasks: Vec<ProcessingTask>) -> Result<()> {
        println!("\nProcessing batch ({} tasks)...", tasks.len());

        let total_tasks = tasks.len();
        let mut completed = 0;
        let mut failed = 0;

        let start = Instant::now();

        // Process tasks (in production, this could be parallelized)
        for (i, task) in tasks.iter().enumerate() {
            let task_num = i + 1;
            println!(
                "  [{}/{}] Processing: {} ({})",
                task_num,
                total_tasks,
                task.input
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown"),
                task.operation.name()
            );

            let result = self.execute_task(task);

            if result.success {
                println!("    ✓ Success ({:.2}s)", result.duration_secs);
                completed += 1;
            } else {
                println!(
                    "    ✗ Failed: {}",
                    result.error.as_deref().unwrap_or("unknown error")
                );
                failed += 1;
            }

            self.results.push(result);

            // Progress update every 10 tasks
            if task_num % 10 == 0 {
                let progress = (task_num as f64 / total_tasks as f64) * 100.0;
                let elapsed = start.elapsed();
                let rate = task_num as f64 / elapsed.as_secs_f64();
                println!(
                    "\n  Progress: {:.0}% ({} done, {} failed, {:.1} tasks/sec)\n",
                    progress, completed, failed, rate
                );
            }
        }

        let elapsed = start.elapsed();
        println!("\n  Batch processing complete!");
        println!("    Total time: {:.2}s", elapsed.as_secs_f64());
        println!("    Success: {}", completed);
        println!("    Failed: {}", failed);
        println!(
            "    Rate: {:.1} tasks/sec",
            total_tasks as f64 / elapsed.as_secs_f64()
        );

        Ok(())
    }

    /// Generate batch statistics
    fn generate_statistics(&self) -> BatchStatistics {
        let total_tasks = self.results.len();
        let successful = self.results.iter().filter(|r| r.success).count();
        let failed = total_tasks - successful;

        let total_duration: f64 = self.results.iter().map(|r| r.duration_secs).sum();

        let avg_duration = if total_tasks > 0 {
            total_duration / total_tasks as f64
        } else {
            0.0
        };

        let mut operations: HashMap<String, usize> = HashMap::new();
        for result in &self.results {
            *operations.entry(result.operation.clone()).or_insert(0) += 1;
        }

        BatchStatistics {
            total_tasks,
            successful,
            failed,
            total_duration_secs: total_duration,
            avg_duration_secs: avg_duration,
            operations,
        }
    }

    /// Generate processing report
    fn generate_report(&self, stats: &BatchStatistics) -> Result<PathBuf> {
        println!("\nGenerating processing report...");

        let report_path = self.output_dir.path().join("batch_report.json");

        let report = serde_json::json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "statistics": stats,
            "results": self.results,
        });

        let report_json = serde_json::to_string_pretty(&report)?;
        std::fs::write(&report_path, report_json)?;

        println!("  ✓ Report saved: {}", report_path.display());

        // Also create a human-readable summary
        let summary_path = self.output_dir.path().join("batch_summary.txt");
        let mut summary = String::new();
        summary.push_str("=== Batch Processing Summary ===\n\n");
        summary.push_str(&format!("Total Tasks: {}\n", stats.total_tasks));
        summary.push_str(&format!(
            "Successful: {} ({:.1}%)\n",
            stats.successful,
            (stats.successful as f64 / stats.total_tasks as f64) * 100.0
        ));
        summary.push_str(&format!(
            "Failed: {} ({:.1}%)\n",
            stats.failed,
            (stats.failed as f64 / stats.total_tasks as f64) * 100.0
        ));
        summary.push_str(&format!(
            "Total Duration: {:.2}s\n",
            stats.total_duration_secs
        ));
        summary.push_str(&format!(
            "Average Duration: {:.2}s\n",
            stats.avg_duration_secs
        ));
        summary.push_str("\nOperations:\n");
        for (op, count) in &stats.operations {
            summary.push_str(&format!("  {}: {}\n", op, count));
        }

        summary.push_str("\nFailed Tasks:\n");
        for result in &self.results {
            if !result.success {
                summary.push_str(&format!(
                    "  - {} ({}): {}\n",
                    result.input,
                    result.operation,
                    result.error.as_deref().unwrap_or("unknown")
                ));
            }
        }

        std::fs::write(&summary_path, summary)?;
        println!("  ✓ Summary saved: {}", summary_path.display());

        Ok(report_path)
    }

    /// Run the complete automation pipeline
    pub fn run(&mut self, file_count: usize) -> Result<BatchStatistics> {
        let start = Instant::now();
        println!("=== CLI Automation Pipeline ===\n");

        // Step 1: Generate sample files
        let input_files = self.generate_sample_files(file_count)?;

        // Step 2: Create batch tasks
        let tasks = self.create_batch_tasks(&input_files)?;

        // Step 3: Process batch
        self.process_batch(tasks)?;

        // Step 4: Generate statistics
        let stats = self.generate_statistics();

        // Step 5: Generate report
        let _report_path = self.generate_report(&stats)?;

        let elapsed = start.elapsed();

        println!("\n=== Pipeline Complete ===");
        println!("Total time: {:.2}s", elapsed.as_secs_f64());
        println!(
            "Success rate: {:.1}%",
            (stats.successful as f64 / stats.total_tasks as f64) * 100.0
        );

        Ok(stats)
    }
}

fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("CLI Automation Example - Batch Processing Pipeline\n");

    // Create pipeline with retry configuration
    let mut pipeline = AutomationPipeline::new(
        3,   // Max 3 retries per task
        100, // 100ms delay between retries
    )?;

    // Run pipeline with 50 files
    let _stats = pipeline.run(50)?;

    println!("\nExample completed successfully!");
    println!("This demonstrates automated batch processing:");
    println!("  - Directory scanning");
    println!("  - Batch task creation");
    println!("  - Error handling and retry logic");
    println!("  - Progress monitoring");
    println!("  - Statistics and reporting");
    println!("\nThis workflow can be integrated into:");
    println!("  - CI/CD pipelines");
    println!("  - Scheduled cron jobs");
    println!("  - Data processing workflows");
    println!("  - Quality assurance systems");

    Ok(())
}
