//! Satellite imagery processing workflow example.

use oxigdal_workflow::{
    WorkflowDefinition,
    dag::{ResourceRequirements, RetryPolicy, TaskEdge, TaskNode, WorkflowDag},
    scheduler::{ScheduleType, Scheduler, SchedulerConfig},
};
use std::collections::HashMap;

fn create_task(id: &str, name: &str) -> TaskNode {
    TaskNode {
        id: id.to_string(),
        name: name.to_string(),
        description: None,
        config: serde_json::json!({}),
        retry: RetryPolicy::default(),
        timeout_secs: Some(60),
        resources: ResourceRequirements::default(),
        metadata: HashMap::new(),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Satellite Imagery Processing Workflow Example");
    println!("=============================================\n");

    // Create a workflow DAG
    let mut dag = WorkflowDag::new();

    // Add tasks
    dag.add_task(create_task("download_imagery", "Download Imagery"))?;
    dag.add_task(create_task(
        "atmospheric_correction",
        "Atmospheric Correction",
    ))?;
    dag.add_task(create_task("cloud_masking", "Cloud Masking"))?;
    dag.add_task(create_task("calculate_ndvi", "Calculate NDVI"))?;
    dag.add_task(create_task("export_results", "Export Results"))?;

    // Add dependencies
    dag.add_dependency(
        "download_imagery",
        "atmospheric_correction",
        TaskEdge::default(),
    )?;
    dag.add_dependency(
        "atmospheric_correction",
        "cloud_masking",
        TaskEdge::default(),
    )?;
    dag.add_dependency("cloud_masking", "calculate_ndvi", TaskEdge::default())?;
    dag.add_dependency("calculate_ndvi", "export_results", TaskEdge::default())?;

    // Create a workflow definition
    let workflow = WorkflowDefinition {
        id: "satellite-processing".to_string(),
        name: "Satellite Imagery Processing".to_string(),
        description: Some("Process Sentinel-2 satellite imagery".to_string()),
        version: "1.0.0".to_string(),
        dag,
    };

    println!("Created workflow: {}", workflow.name);
    println!(
        "Tasks: {:?}",
        workflow
            .dag
            .tasks()
            .iter()
            .map(|t| &t.id)
            .collect::<Vec<_>>()
    );
    println!();

    // Create scheduler
    let config = SchedulerConfig {
        max_concurrent_executions: 10,
        handle_missed_executions: true,
        max_missed_executions: 5,
        execution_timeout_secs: 3600,
        enable_persistence: false,
        persistence_path: None,
        tick_interval_ms: 100,
        timezone: "UTC".to_string(),
    };

    let scheduler = Scheduler::new(config);

    // Schedule the workflow to run daily
    let schedule_id = scheduler
        .add_schedule(
            workflow.clone(),
            ScheduleType::Cron {
                expression: "0 0 * * *".to_string(), // Daily at midnight
            },
        )
        .await?;

    println!("Scheduled workflow with ID: {}", schedule_id);

    // Trigger a manual execution
    let execution_id = scheduler.trigger_manual(&schedule_id).await?;
    println!("Triggered manual execution: {}", execution_id);

    // Get schedule information
    if let Some(schedule) = scheduler.get_schedule(&schedule_id) {
        println!("\nSchedule Information:");
        println!("  ID: {}", schedule.schedule_id);
        println!("  Enabled: {}", schedule.enabled);
        println!("  Type: {:?}", schedule.schedule_type);
        if let Some(next) = schedule.next_execution {
            println!("  Next execution: {}", next);
        }
    }

    println!("\nWorkflow scheduled successfully!");

    Ok(())
}
