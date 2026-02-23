//! Prefect integration.

use crate::engine::WorkflowDefinition;
use crate::error::{Result, WorkflowError};

/// Prefect integration.
pub struct PrefectIntegration;

impl PrefectIntegration {
    /// Export workflow to Prefect flow format (Python).
    pub fn export_workflow(workflow: &WorkflowDefinition) -> Result<String> {
        let mut python_code = String::new();

        // Add imports
        python_code.push_str("from prefect import flow, task\n");
        python_code.push_str("from datetime import timedelta\n\n");

        // Define tasks
        for (idx, _task) in workflow.dag.tasks().iter().enumerate() {
            python_code.push_str(&format!("@task(name='task_{}')\n", idx));
            python_code.push_str(&format!("def task_{}():\n", idx));
            python_code.push_str("    print('Task executed')\n");
            python_code.push_str("    return True\n\n");
        }

        // Define flow
        python_code.push_str(&format!("@flow(name='{}')\n", workflow.name));
        python_code.push_str(&format!("def {}():\n", Self::sanitize_id(&workflow.id)));

        for (idx, _task) in workflow.dag.tasks().iter().enumerate() {
            python_code.push_str(&format!("    result_{} = task_{}()\n", idx, idx));
        }

        python_code.push('\n');
        python_code.push_str("if __name__ == '__main__':\n");
        python_code.push_str(&format!("    {}()\n", Self::sanitize_id(&workflow.id)));

        Ok(python_code)
    }

    /// Import workflow from Prefect flow.
    pub fn import_workflow(_flow_code: &str) -> Result<WorkflowDefinition> {
        Err(WorkflowError::integration(
            "prefect",
            "Import from Prefect not yet implemented",
        ))
    }

    /// Sanitize ID for Prefect compatibility.
    fn sanitize_id(id: &str) -> String {
        id.replace(['-', ' '], "_")
    }

    /// Trigger a Prefect flow via API.
    #[cfg(feature = "integrations")]
    pub async fn trigger_flow(
        base_url: &str,
        flow_id: &str,
        api_key: Option<&str>,
    ) -> Result<String> {
        use reqwest::Client;

        let url = format!("{}/api/flows/{}/runs", base_url, flow_id);
        let client = Client::new();

        let mut request = client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "parameters": {}
            }));

        if let Some(key) = api_key {
            request = request.bearer_auth(key);
        }

        let response = request
            .send()
            .await
            .map_err(|e| WorkflowError::integration("prefect", format!("Request failed: {}", e)))?;

        let body = response.text().await.map_err(|e| {
            WorkflowError::integration("prefect", format!("Failed to read response: {}", e))
        })?;

        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::WorkflowDag;

    #[test]
    fn test_export_to_prefect() {
        let workflow = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            description: None,
            version: "1.0.0".to_string(),
            dag: WorkflowDag::new(),
        };

        let result = PrefectIntegration::export_workflow(&workflow);
        assert!(result.is_ok());

        let python_code = result.expect("Failed to export");
        assert!(python_code.contains("from prefect import flow, task"));
        assert!(python_code.contains("@flow"));
    }

    #[test]
    fn test_sanitize_id() {
        assert_eq!(
            PrefectIntegration::sanitize_id("test-workflow-id"),
            "test_workflow_id"
        );
    }
}
