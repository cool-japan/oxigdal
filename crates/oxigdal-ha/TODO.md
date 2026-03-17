# TODO: oxigdal-ha

## High Priority
- [ ] Implement WAL (Write-Ahead Log) writer and reader for point-in-time recovery
- [ ] Add actual network replication transport (currently in-memory simulation)
- [ ] Implement leader election protocol (Raft or Bully algorithm) for automatic failover
- [ ] Wire conflict resolution to real replicated data (CRDT or last-writer-wins)
- [ ] Implement health check HTTP/TCP probes against real service endpoints
- [ ] Add incremental backup with delta computation from last snapshot
- [ ] Implement failover state machine (detect failure -> elect leader -> redirect traffic)

## Medium Priority
- [ ] Add cross-region disaster recovery with configurable RPO/RTO targets
- [ ] Implement automated DR runbook execution (failover, DNS switch, traffic redirect)
- [ ] Add snapshot-based backup to cloud object storage (S3, GCS, Azure Blob)
- [ ] Implement split-brain detection and resolution for active-active clusters
- [ ] Add replication lag monitoring with configurable alerting thresholds
- [ ] Implement backup verification (restore to temporary instance and validate)
- [ ] Add connection draining during planned failover (zero-downtime maintenance)
- [ ] Implement read replicas with configurable consistency level (eventual, strong)

## Low Priority / Future
- [ ] Add multi-region active-active with conflict-free replicated data types (CRDTs)
- [ ] Implement blue-green deployment support with automated rollback
- [ ] Add canary deployment with progressive traffic shifting
- [ ] Implement chaos engineering hooks (inject failures for resilience testing)
- [ ] Add compliance reporting for HA SLA metrics (uptime percentage, MTTR, MTBF)
- [ ] Implement geo-fenced data residency with region-aware replication
- [ ] Add automated capacity planning based on historical failover patterns
