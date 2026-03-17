# TODO: oxigdal-distributed

## High Priority
- [ ] Implement Arrow Flight server with actual gRPC transport (currently data-model only)
- [ ] Wire Coordinator task scheduling to real worker nodes over the network
- [ ] Implement Worker task execution loop (receive, execute, report results)
- [ ] Add Arrow RecordBatch serialization over Flight for zero-copy data transfer
- [ ] Implement partition assignment strategy (assign partitions to workers by locality)
- [ ] Add shuffle data exchange over network between workers
- [ ] Implement coordinator failure detection and worker re-assignment

## Medium Priority
- [ ] Add spatial partitioning with R-tree index for partition pruning
- [ ] Implement hash shuffle with configurable hash function and partition count
- [ ] Add broadcast join optimization (small table replicated to all workers)
- [ ] Implement progress reporting with per-partition completion tracking
- [ ] Add data spill to disk when shuffle buffers exceed memory limits
- [ ] Implement speculative task execution for straggler mitigation
- [ ] Add coordinator HA with leader election (standby coordinator)
- [ ] Implement result aggregation with ordered merge for sorted outputs
- [ ] Add task dependency tracking (DAG execution with topological ordering)

## Low Priority / Future
- [ ] Implement adaptive repartitioning based on data skew detection
- [ ] Add distributed sort with external merge for large datasets
- [ ] Implement pipeline parallelism (overlap I/O, compute, and shuffle)
- [ ] Add Kubernetes-native deployment with pod-per-worker topology
- [ ] Implement cross-datacenter distribution with WAN-aware scheduling
- [ ] Add lineage tracking for provenance of distributed computation results
- [ ] Implement distributed GeoTIFF mosaic assembly from worker-produced tiles
