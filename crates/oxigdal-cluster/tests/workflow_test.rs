//! Workflow engine tests.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use oxigdal_cluster::workflow::*;
use std::collections::HashMap;
use std::time::SystemTime;

#[test]
fn test_workflow_creation() {
    let workflow = Workflow {
        id: uuid::Uuid::new_v4(),
        name: "test-workflow".to_string(),
        version: "1.0.0".to_string(),
        description: Some("Test workflow".to_string()),
        steps: vec![],
        variables: HashMap::new(),
        created_at: SystemTime::now(),
        updated_at: SystemTime::now(),
    };

    let engine = WorkflowEngine::new();
    let result = engine.register_workflow(workflow.clone());

    assert!(result.is_ok());
    assert_eq!(
        result.expect("Failed to get workflow ID from registration result"),
        workflow.id
    );
}

#[test]
fn test_workflow_execution() {
    let workflow = Workflow {
        id: uuid::Uuid::new_v4(),
        name: "test-workflow".to_string(),
        version: "1.0.0".to_string(),
        description: None,
        steps: vec![],
        variables: HashMap::new(),
        created_at: SystemTime::now(),
        updated_at: SystemTime::now(),
    };

    let engine = WorkflowEngine::new();
    let workflow_id = engine
        .register_workflow(workflow)
        .expect("Failed to register workflow");

    let execution_id = engine
        .start_execution(workflow_id)
        .expect("Failed to start workflow execution");
    let execution = engine.get_execution(execution_id);

    assert!(execution.is_some());
    assert_eq!(
        execution.expect("Failed to get workflow execution").status,
        WorkflowStatus::Running
    );
}

#[test]
fn test_workflow_pause_resume() {
    let workflow = Workflow {
        id: uuid::Uuid::new_v4(),
        name: "test-workflow".to_string(),
        version: "1.0.0".to_string(),
        description: None,
        steps: vec![],
        variables: HashMap::new(),
        created_at: SystemTime::now(),
        updated_at: SystemTime::now(),
    };

    let engine = WorkflowEngine::new();
    let workflow_id = engine
        .register_workflow(workflow)
        .expect("Failed to register workflow for pause/resume test");
    let execution_id = engine
        .start_execution(workflow_id)
        .expect("Failed to start workflow execution for pause/resume test");

    // Pause
    engine.pause_execution(execution_id).ok();
    let execution = engine
        .get_execution(execution_id)
        .expect("Failed to get execution after pause");
    assert_eq!(execution.status, WorkflowStatus::Paused);

    // Resume
    engine.resume_execution(execution_id).ok();
    let execution = engine
        .get_execution(execution_id)
        .expect("Failed to get execution after resume");
    assert_eq!(execution.status, WorkflowStatus::Running);
}

#[test]
fn test_workflow_template() {
    let template = WorkflowTemplate {
        name: "test-template".to_string(),
        version: "1.0.0".to_string(),
        parameters: vec![TemplateParameter {
            name: "input".to_string(),
            param_type: "string".to_string(),
            required: true,
            default: None,
            description: Some("Input parameter".to_string()),
        }],
        steps: vec![],
    };

    let engine = WorkflowEngine::new();
    engine.register_template(template.clone()).ok();

    let mut params = HashMap::new();
    params.insert("input".to_string(), serde_json::json!("test-value"));

    let workflow = engine.create_from_template("test-template", params);
    assert!(workflow.is_ok());
}
