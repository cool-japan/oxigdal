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
        python_code.push_str("    'owner': 'oxigeo',\n");
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

        // Define tasks, recording each task id -> operator variable index so
        // dependency edges can reference the correct operators.
        let tasks = workflow.dag.tasks();
        let mut task_index: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::with_capacity(tasks.len());
        for (idx, task) in tasks.iter().enumerate() {
            task_index.insert(task.id.as_str(), idx);
            python_code.push_str(&format!("task{} = PythonOperator(\n", idx));
            python_code.push_str(&format!("    task_id='task_{}',\n", idx));
            python_code.push_str("    python_callable=lambda: print('Task executed'),\n");
            python_code.push_str("    dag=dag,\n");
            python_code.push_str(")\n\n");
        }

        // Define dependencies: emit `taskN.set_downstream(taskM)` for every edge
        // in the DAG, mapping task ids back to their operator variables.
        if workflow.dag.dependency_count() > 0 {
            python_code.push_str("# Define task dependencies\n");
            for (from_id, to_id, _edge) in workflow.dag.edges() {
                if let (Some(&from_idx), Some(&to_idx)) =
                    (task_index.get(from_id), task_index.get(to_id))
                {
                    python_code.push_str(&format!(
                        "task{}.set_downstream(task{})\n",
                        from_idx, to_idx
                    ));
                }
            }
            python_code.push('\n');
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

        let status = response.status();

        let body = response.text().await.map_err(|e| {
            WorkflowError::integration("airflow", format!("Failed to read response: {}", e))
        })?;

        if !status.is_success() {
            return Err(WorkflowError::integration(
                "airflow",
                format!("HTTP {}: {}", status, body),
            ));
        }

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

    #[test]
    fn test_export_emits_dependency_edges() {
        use crate::dag::graph::{ResourceRequirements, RetryPolicy, TaskEdge, TaskNode};
        use std::collections::HashMap;

        let make_task = |id: &str| TaskNode {
            id: id.to_string(),
            name: id.to_string(),
            description: None,
            config: serde_json::json!({}),
            retry: RetryPolicy::default(),
            timeout_secs: Some(60),
            resources: ResourceRequirements::default(),
            metadata: HashMap::new(),
        };

        let mut dag = WorkflowDag::new();
        dag.add_task(make_task("extract")).expect("add extract");
        dag.add_task(make_task("transform")).expect("add transform");
        dag.add_dependency("extract", "transform", TaskEdge::default())
            .expect("add dependency");

        let workflow = WorkflowDefinition {
            id: "etl".to_string(),
            name: "ETL".to_string(),
            description: None,
            version: "1.0.0".to_string(),
            dag,
        };

        let code = AirflowIntegration::export_workflow(&workflow).expect("export");

        // The dependency block must be emitted (guard was previously unreachable).
        assert!(code.contains("# Define task dependencies"));

        // Exactly one set_downstream edge for the single dependency.
        let downstream_lines: Vec<&str> = code
            .lines()
            .filter(|l| l.contains(".set_downstream("))
            .collect();
        assert_eq!(
            downstream_lines.len(),
            1,
            "expected one dependency edge, got: {:?}",
            downstream_lines
        );
        assert!(downstream_lines[0].starts_with("task"));
        assert!(downstream_lines[0].contains(".set_downstream(task"));
    }
}
