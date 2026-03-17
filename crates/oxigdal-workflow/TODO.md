# TODO: oxigdal-workflow

## High Priority
- [ ] Implement DAG cycle detection with detailed error reporting
- [ ] Add retry policies per task (exponential backoff, max attempts)
- [ ] Implement task timeout enforcement with cancellation
- [ ] Add persistent state storage (SQLite-backed) for crash recovery
- [ ] Implement dynamic DAG modification (add/remove tasks at runtime)
- [ ] Add resource limits per task (memory, CPU, disk I/O)

## Medium Priority
- [ ] Implement Airflow DAG export/import (Python DAG generation)
- [ ] Add Prefect flow integration via REST API
- [ ] Implement Temporal.io workflow definition export
- [ ] Add webhook triggers for event-driven workflow execution
- [ ] Implement workflow parameterization with variable substitution
- [ ] Add task dependency visualization (Mermaid/DOT graph export)
- [ ] Implement sub-workflow (nested DAG) execution
- [ ] Add map/reduce pattern for parallel batch processing
- [ ] Implement SLA monitoring with deadline-based alerting

## Low Priority / Future
- [ ] Add workflow marketplace/registry for sharing reusable pipelines
- [ ] Implement A/B testing workflow variants with metric comparison
- [ ] Add cost estimation for cloud-executed workflows (AWS/GCP pricing)
- [ ] Implement workflow diff/versioning with semantic change detection
- [ ] Add Kubernetes Job/CronJob manifest generation
- [ ] Implement data lineage tracking across workflow executions
- [ ] Add interactive workflow builder (JSON/TOML definition files)
