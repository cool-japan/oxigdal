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

        // Define each task's callable. When the task's `config` carries a `"command"`
        // string (see `TaskNode::command`), emit a real subprocess invocation of it,
        // mirroring `TemporalIntegration::export_workflow`; otherwise fall back to a
        // placeholder body that also surfaces the task's raw config so the gap is
        // visible in the generated source rather than silently discarded.
        let tasks = workflow.dag.tasks();
        for (idx, task) in tasks.iter().enumerate() {
            python_code.push_str(&format!("def _task_{}_callable():\n", idx));
            if let Some(command) = task.command() {
                python_code.push_str("    import subprocess\n");
                python_code.push_str(&format!(
                    "    result = subprocess.run(['sh', '-c', {}], capture_output=True, text=True)\n",
                    Self::python_string_literal(command)
                ));
                python_code.push_str("    if result.returncode != 0:\n");
                python_code.push_str(&format!(
                    "        raise RuntimeError(f\"Task '{}' failed (exit {{result.returncode}}): {{result.stderr}}\")\n",
                    Self::escape_python_fstring(&task.id)
                ));
                python_code.push_str("    print(result.stdout)\n\n");
            } else {
                python_code.push_str(&format!(
                    "    # Placeholder: task '{}' has no 'command' in its config, so there is\n",
                    Self::escape_python_comment(&task.id)
                ));
                python_code.push_str(&format!(
                    "    # nothing to execute here. Original config: {}\n",
                    Self::escape_python_comment(&task.config.to_string())
                ));
                python_code.push_str("    print('Task executed')\n\n");
            }
        }

        // Define tasks, recording each task id -> operator variable index so
        // dependency edges can reference the correct operators.
        let mut task_index: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::with_capacity(tasks.len());
        for (idx, task) in tasks.iter().enumerate() {
            task_index.insert(task.id.as_str(), idx);
            python_code.push_str(&format!("task{} = PythonOperator(\n", idx));
            python_code.push_str(&format!("    task_id='task_{}',\n", idx));
            python_code.push_str(&format!("    python_callable=_task_{}_callable,\n", idx));
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

    /// Renders `s` as a Python double-quoted string literal (including the surrounding
    /// quotes), suitable for embedding inside generated Python source.
    ///
    /// JSON string escaping (backslash, double quote, control characters, `\uXXXX`) is a
    /// subset of Python's double-quoted string escaping, so serializing through
    /// `serde_json` produces a literal Python parses identically.
    fn python_string_literal(s: &str) -> String {
        serde_json::to_string(s)
            .unwrap_or_else(|_| format!("\"{}\"", Self::escape_python_fstring(s)))
    }

    /// Escapes a string for safe embedding inside a Python f-string literal body
    /// (i.e. between the surrounding quotes of an `f"..."` string).
    fn escape_python_fstring(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('{', "{{")
            .replace('}', "}}")
    }

    /// Escapes a string for safe embedding inside a Python `#` comment (strips
    /// newlines so the comment cannot spill onto a following line of generated code).
    fn escape_python_comment(s: &str) -> String {
        s.replace(['\n', '\r'], " ")
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

    #[test]
    fn test_export_runs_configured_command_instead_of_placeholder() {
        use crate::dag::graph::{ResourceRequirements, RetryPolicy, TaskNode};
        use std::collections::HashMap;

        let task_with_command = TaskNode {
            id: "download".to_string(),
            name: "download".to_string(),
            description: None,
            config: serde_json::json!({ "command": "curl -O https://example.com/scene.tif" }),
            retry: RetryPolicy::default(),
            timeout_secs: Some(60),
            resources: ResourceRequirements::default(),
            metadata: HashMap::new(),
        };

        let mut dag = WorkflowDag::new();
        dag.add_task(task_with_command).expect("add task");

        let workflow = WorkflowDefinition {
            id: "download-workflow".to_string(),
            name: "Download Workflow".to_string(),
            description: None,
            version: "1.0.0".to_string(),
            dag,
        };

        let code = AirflowIntegration::export_workflow(&workflow).expect("export");

        // The task's real command must be embedded and actually executed via
        // subprocess, not silently discarded for a no-op print.
        assert!(code.contains("import subprocess"));
        assert!(code.contains("subprocess.run(['sh', '-c'"));
        assert!(code.contains("curl -O https://example.com/scene.tif"));
        assert!(code.contains("raise RuntimeError"));
        assert!(!code.contains("lambda: print('Task executed')"));
    }

    #[test]
    fn test_export_placeholder_body_surfaces_task_config() {
        use crate::dag::graph::{ResourceRequirements, RetryPolicy, TaskNode};
        use std::collections::HashMap;

        let task_without_command = TaskNode {
            id: "cloud-mask".to_string(),
            name: "cloud-mask".to_string(),
            description: None,
            config: serde_json::json!({ "algorithm": "fmask", "threshold": 0.4 }),
            retry: RetryPolicy::default(),
            timeout_secs: Some(60),
            resources: ResourceRequirements::default(),
            metadata: HashMap::new(),
        };

        let mut dag = WorkflowDag::new();
        dag.add_task(task_without_command).expect("add task");

        let workflow = WorkflowDefinition {
            id: "mask-workflow".to_string(),
            name: "Mask Workflow".to_string(),
            description: None,
            version: "1.0.0".to_string(),
            dag,
        };

        let code = AirflowIntegration::export_workflow(&workflow).expect("export");

        // No command configured -> placeholder body, but the gap must be visible:
        // the task's real config is embedded as a comment rather than silently
        // vanishing behind an unqualified print statement.
        assert!(code.contains("Placeholder"));
        assert!(code.contains("cloud-mask"));
        assert!(code.contains("fmask"));
    }
}
