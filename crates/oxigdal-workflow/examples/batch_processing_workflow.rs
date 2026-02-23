//! Batch processing workflow example.

use oxigdal_workflow::{
    WorkflowDefinition,
    dag::{
        ResourceRequirements, RetryPolicy, TaskEdge, TaskNode, WorkflowDag, create_execution_plan,
    },
    monitoring::MonitoringService,
    scheduler::{ScheduleType, Scheduler},
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
    println!("Batch Processing Workflow Example");
    println!("==================================\n");

    // Create a DAG for parallel batch processing
    let mut dag = WorkflowDag::new();

    // Add source task
    dag.add_task(create_task("list_files", "List Input Files"))?;

    // Add parallel processing tasks
    for i in 0..8 {
        dag.add_task(create_task(
            &format!("process_batch_{}", i),
            &format!("Process Batch {}", i),
        ))?;
    }

    // Add merge task
    dag.add_task(create_task("merge_results", "Merge Results"))?;

    // Add export task
    dag.add_task(create_task("export", "Export Final Results"))?;

    // Add dependencies
    for i in 0..8 {
        dag.add_dependency(
            "list_files",
            &format!("process_batch_{}", i),
            TaskEdge::default(),
        )?;
    }

    for i in 0..8 {
        dag.add_dependency(
            &format!("process_batch_{}", i),
            "merge_results",
            TaskEdge::default(),
        )?;
    }

    dag.add_dependency("merge_results", "export", TaskEdge::default())?;

    println!("Created DAG with {} nodes", dag.task_count());

    // Get execution plan (levels for parallel execution)
    let execution_plan = create_execution_plan(&dag)?;
    println!("\nExecution order (by level):");
    for (level_idx, level) in execution_plan.iter().enumerate() {
        println!(
            "  Level {}: {} tasks ({})",
            level_idx,
            level.len(),
            level.join(", ")
        );
    }

    // Create workflow definition
    let workflow = WorkflowDefinition {
        id: "batch-processing".to_string(),
        name: "Batch Processing Workflow".to_string(),
        description: Some("Process multiple files in parallel".to_string()),
        version: "1.0.0".to_string(),
        dag,
    };

    // Set up monitoring
    let _monitoring = MonitoringService::new();
    println!("\nMonitoring service initialized");

    // Schedule the workflow
    let scheduler = Scheduler::with_defaults();

    let schedule_id = scheduler
        .add_schedule(
            workflow,
            ScheduleType::Interval {
                interval_secs: 3600, // Every hour
            },
        )
        .await?;

    println!("Scheduled workflow with ID: {}", schedule_id);

    // Get scheduler statistics
    let schedules = scheduler.get_schedules();
    println!("\nTotal scheduled workflows: {}", schedules.len());

    println!("\nBatch processing workflow configured successfully!");
    println!(
        "Expected parallelism: {} concurrent tasks in level 1",
        execution_plan
            .get(1)
            .map(|g: &Vec<String>| g.len())
            .unwrap_or(0)
    );

    Ok(())
}
