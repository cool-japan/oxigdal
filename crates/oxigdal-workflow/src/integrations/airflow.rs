//! Apache Airflow integration.

use crate::engine::WorkflowDefinition;
use crate::error::{Result, WorkflowError};

/// Apache Airflow integration.
pub struct AirflowIntegration;

impl AirflowIntegration {
    /// Export workflow to Airflow DAG format (Python).
    pub fn export_workflow(workflow: &WorkflowDefinition) -> Result<String> {
        let mut python_code = String::new();

        // Add imports
        python_code.push_str("from airflow import DAG\n");
        python_code.push_str("from airflow.operators.python import PythonOperator\n");
        python_code.push_str("from datetime import datetime, timedelta\n\n");

        // Define default args
        python_code.push_str("default_args = {\n");
        python_code.push_str("    'owner': 'oxigdal',\n");
        python_code.push_str("    'depends_on_past': False,\n");
        python_code.push_str("    'retries': 1,\n");
        python_code.push_str("    'retry_delay': timedelta(minutes=5),\n");
        python_code.push_str("}\n\n");

        // Define DAG
        python_code.push_str(&format!(
            "dag = DAG(\n    '{}',\n    default_args=default_args,\n",
            Self::sanitize_id(&workflow.id)
        ));
        python_code.push_str(&format!(
            "    description='{}',\n",
            workflow.description.as_deref().unwrap_or("")
        ));
        python_code.push_str("    schedule_interval=None,\n");
        python_code.push_str("    start_date=datetime(2024, 1, 1),\n");
        python_code.push_str(")\n\n");

        // Define tasks
        for (idx, _task) in workflow.dag.tasks().iter().enumerate() {
            python_code.push_str(&format!("task{} = PythonOperator(\n", idx));
            python_code.push_str(&format!("    task_id='task_{}',\n", idx));
            python_code.push_str("    python_callable=lambda: print('Task executed'),\n");
            python_code.push_str("    dag=dag,\n");
            python_code.push_str(")\n\n");
        }

        // Define dependencies
        if !workflow.dag.dependency_count() == 0 {
            python_code.push_str("# Define task dependencies\n");
            // Dependencies would be mapped here
        }

        Ok(python_code)
    }

    /// Import workflow from Airflow DAG.
    pub fn import_workflow(_dag_code: &str) -> Result<WorkflowDefinition> {
        Err(WorkflowError::integration(
            "airflow",
            "Import from Airflow not yet implemented",
        ))
    }

    /// Sanitize ID for Airflow compatibility.
    fn sanitize_id(id: &str) -> String {
        id.replace(['-', ' '], "_")
    }

    /// Trigger an Airflow DAG via REST API.
    #[cfg(feature = "integrations")]
    pub async fn trigger_dag(
        base_url: &str,
        dag_id: &str,
        api_key: Option<&str>,
    ) -> Result<String> {
        use reqwest::Client;

        let url = format!("{}/api/v1/dags/{}/dagRuns", base_url, dag_id);
        let client = Client::new();

        let mut request = client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "conf": {}
            }));

        if let Some(key) = api_key {
            request = request.bearer_auth(key);
        }

        let response = request
            .send()
            .await
            .map_err(|e| WorkflowError::integration("airflow", format!("Request failed: {}", e)))?;

        let body = response.text().await.map_err(|e| {
            WorkflowError::integration("airflow", format!("Failed to read response: {}", e))
        })?;

        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::WorkflowDag;

    #[test]
    fn test_export_to_airflow() {
        let workflow = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            description: Some("Test description".to_string()),
            version: "1.0.0".to_string(),
            dag: WorkflowDag::new(),
        };

        let result = AirflowIntegration::export_workflow(&workflow);
        assert!(result.is_ok());

        let python_code = result.expect("Failed to export");
        assert!(python_code.contains("from airflow import DAG"));
        assert!(python_code.contains("test_workflow"));
    }

    #[test]
    fn test_sanitize_id() {
        assert_eq!(
            AirflowIntegration::sanitize_id("test-workflow-id"),
            "test_workflow_id"
        );
    }
}
