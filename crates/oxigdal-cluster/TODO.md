# TODO: oxigdal-cluster

## High Priority
- [ ] Implement actual network transport for inter-node communication (currently in-process only)
- [ ] Add Raft consensus log persistence to disk (coordinator currently uses in-memory state)
- [ ] Implement real work-stealing protocol over the network between worker nodes
- [ ] Add task serialization/deserialization for network transmission
- [ ] Implement checkpoint persistence to durable storage (local disk or cloud)
- [ ] Wire autoscaler to cloud provider APIs (AWS ASG, GCP MIG, Azure VMSS)
- [ ] Add cluster membership protocol (gossip or SWIM) for node discovery

## Medium Priority
- [ ] Implement distributed cache invalidation over network (currently local simulation)
- [ ] Add replication data transfer between replicas over TCP/gRPC
- [ ] Implement speculative execution with result deduplication
- [ ] Add resource quota enforcement across distributed workers
- [ ] Implement gang scheduling for tightly-coupled geospatial operations
- [ ] Add topology-aware scheduling using actual network latency measurements
- [ ] Implement workflow engine persistence (resume workflows after coordinator restart)
- [ ] Add alert delivery (email, Slack webhook, PagerDuty) for monitoring alerts
- [ ] Implement RBAC policy enforcement at task submission

## Low Priority / Future
- [ ] Add Kubernetes operator for cluster lifecycle management
- [ ] Implement multi-cluster federation for geo-distributed processing
- [ ] Add GPU resource scheduling for ML inference tasks
- [ ] Implement priority preemption (evict low-priority tasks for urgent ones)
- [ ] Add cost-aware scheduling (prefer spot/preemptible instances)
- [ ] Implement cluster state snapshots for disaster recovery
- [ ] Add built-in benchmarking suite for cluster performance profiling
