//! Temporal.io integration.

use crate::engine::WorkflowDefinition;
use crate::error::{Result, WorkflowError};

/// Temporal.io integration.
pub struct TemporalIntegration;

impl TemporalIntegration {
    /// Export workflow to Temporal workflow format (Go).
    pub fn export_workflow(workflow: &WorkflowDefinition) -> Result<String> {
        let mut go_code = String::new();

        // Add package and imports
        go_code.push_str("package workflows\n\n");
        go_code.push_str("import (\n");
        go_code.push_str("    \"time\"\n");
        go_code.push_str("    \"go.temporal.io/sdk/workflow\"\n");
        go_code.push_str(")\n\n");

        // Define activity interfaces
        for (idx, _task) in workflow.dag.tasks().iter().enumerate() {
            go_code.push_str(&format!(
                "func Task{}Activity(ctx workflow.Context) error {{\n",
                idx
            ));
            go_code.push_str("    // TODO: Implement activity logic\n");
            go_code.push_str("    return nil\n");
            go_code.push_str("}\n\n");
        }

        // Define workflow
        go_code.push_str(&format!(
            "func {}Workflow(ctx workflow.Context) error {{\n",
            Self::to_camel_case(&workflow.id)
        ));

        for (idx, _task) in workflow.dag.tasks().iter().enumerate() {
            go_code.push_str("    ao := workflow.ActivityOptions{\n");
            go_code.push_str("        StartToCloseTimeout: 1 * time.Minute,\n");
            go_code.push_str("    }\n");
            go_code.push_str(&format!(
                "    ctx{} := workflow.WithActivityOptions(ctx, ao)\n",
                idx
            ));
            go_code.push_str(&format!(
                "    err{} := workflow.ExecuteActivity(ctx{}, Task{}Activity).Get(ctx{}, nil)\n",
                idx, idx, idx, idx
            ));
            go_code.push_str(&format!("    if err{} != nil {{\n", idx));
            go_code.push_str(&format!("        return err{}\n", idx));
            go_code.push_str("    }\n\n");
        }

        go_code.push_str("    return nil\n");
        go_code.push_str("}\n");

        Ok(go_code)
    }

    /// Import workflow from Temporal workflow.
    pub fn import_workflow(_workflow_code: &str) -> Result<WorkflowDefinition> {
        Err(WorkflowError::integration(
            "temporal",
            "Import from Temporal not yet implemented",
        ))
    }

    /// Convert to CamelCase for Go naming.
    fn to_camel_case(s: &str) -> String {
        s.split(['-', '_'])
            .filter(|s| !s.is_empty())
            .enumerate()
            .map(|(i, s)| {
                if i == 0 {
                    s.chars()
                        .enumerate()
                        .map(|(j, c)| if j == 0 { c.to_ascii_uppercase() } else { c })
                        .collect()
                } else {
                    let mut chars = s.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(first) => first.to_uppercase().chain(chars).collect(),
                    }
                }
            })
            .collect()
    }

    /// Start a Temporal workflow via API.
    #[cfg(feature = "integrations")]
    pub async fn start_workflow(
        base_url: &str,
        namespace: &str,
        workflow_id: &str,
        workflow_type: &str,
    ) -> Result<String> {
        use reqwest::Client;

        let url = format!(
            "{}/api/v1/namespaces/{}/workflows/{}",
            base_url, namespace, workflow_id
        );
        let client = Client::new();

        let response = client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "workflowId": workflow_id,
                "workflowType": {
                    "name": workflow_type
                },
                "input": {}
            }))
            .send()
            .await
            .map_err(|e| {
                WorkflowError::integration("temporal", format!("Request failed: {}", e))
            })?;

        let body = response.text().await.map_err(|e| {
            WorkflowError::integration("temporal", format!("Failed to read response: {}", e))
        })?;

        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::WorkflowDag;

    #[test]
    fn test_export_to_temporal() {
        let workflow = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            description: None,
            version: "1.0.0".to_string(),
            dag: WorkflowDag::new(),
        };

        let result = TemporalIntegration::export_workflow(&workflow);
        assert!(result.is_ok());

        let go_code = result.expect("Failed to export");
        assert!(go_code.contains("package workflows"));
        assert!(go_code.contains("go.temporal.io/sdk/workflow"));
    }

    #[test]
    fn test_to_camel_case() {
        assert_eq!(
            TemporalIntegration::to_camel_case("test-workflow-id"),
            "TestWorkflowId"
        );
        assert_eq!(
            TemporalIntegration::to_camel_case("my_workflow"),
            "MyWorkflow"
        );
    }
}
