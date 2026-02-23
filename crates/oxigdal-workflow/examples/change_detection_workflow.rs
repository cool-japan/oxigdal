//! Change detection workflow example.

use oxigdal_workflow::{
    WorkflowDefinition,
    dag::{ResourceRequirements, RetryPolicy, TaskEdge, TaskNode, WorkflowDag},
    scheduler::{ScheduleType, Scheduler},
    templates::{TemplateCategory, WorkflowTemplateLibrary},
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
    println!("Change Detection Workflow Example");
    println!("==================================\n");

    // Create template library and get change detection template
    let library = WorkflowTemplateLibrary::new();

    println!("Available templates:");
    for template in library.list_templates() {
        println!("  - {} ({})", template.name, template.id);
    }
    println!();

    // Get change detection templates
    let change_templates = library.list_by_category(&TemplateCategory::ChangeDetection);

    if let Some(template) = change_templates.first() {
        println!("Using template: {}", template.name);
        println!("Description: {}", template.description);
        println!("Parameters:");
        for param in &template.parameters {
            println!(
                "  - {} ({}): {}",
                param.name,
                if param.required {
                    "required"
                } else {
                    "optional"
                },
                param.description
            );
        }
        println!();

        // Create a workflow DAG for demonstration
        let mut dag = WorkflowDag::new();

        // Add tasks
        dag.add_task(create_task("load_before_image", "Load Before Image"))?;
        dag.add_task(create_task("load_after_image", "Load After Image"))?;
        dag.add_task(create_task("preprocess_images", "Preprocess Images"))?;
        dag.add_task(create_task("calculate_difference", "Calculate Difference"))?;
        dag.add_task(create_task("apply_threshold", "Apply Threshold"))?;
        dag.add_task(create_task("generate_change_map", "Generate Change Map"))?;

        // Add dependencies
        dag.add_dependency(
            "load_before_image",
            "preprocess_images",
            TaskEdge::default(),
        )?;
        dag.add_dependency("load_after_image", "preprocess_images", TaskEdge::default())?;
        dag.add_dependency(
            "preprocess_images",
            "calculate_difference",
            TaskEdge::default(),
        )?;
        dag.add_dependency(
            "calculate_difference",
            "apply_threshold",
            TaskEdge::default(),
        )?;
        dag.add_dependency(
            "apply_threshold",
            "generate_change_map",
            TaskEdge::default(),
        )?;

        let workflow = WorkflowDefinition {
            id: "change-detection-demo".to_string(),
            name: "Change Detection Demo".to_string(),
            description: Some("Detect changes between two time periods".to_string()),
            version: "1.0.0".to_string(),
            dag,
        };

        println!("Created workflow with {} tasks", workflow.dag.task_count());

        // Schedule the workflow
        let scheduler = Scheduler::with_defaults();

        let schedule_id = scheduler
            .add_schedule(
                workflow,
                ScheduleType::Interval {
                    interval_secs: 86400, // Daily
                },
            )
            .await?;

        println!("Scheduled workflow with ID: {}", schedule_id);

        // Trigger immediate execution
        let execution_id = scheduler.trigger_manual(&schedule_id).await?;
        println!("Started execution: {}", execution_id);

        println!("\nChange detection workflow configured successfully!");
    } else {
        println!("No change detection templates found in library");
    }

    Ok(())
}
