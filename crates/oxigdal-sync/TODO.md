# TODO: oxigdal-sync

## High Priority
- [ ] Implement efficient Merkle tree synchronization protocol (anti-entropy)
- [ ] Add network transport layer for device-to-device sync (TCP/QUIC)
- [ ] Implement CRDT garbage collection (tombstone pruning)
- [ ] Add OR-Set CRDT for set-based spatial feature collections
- [ ] Implement device discovery via mDNS/DNS-SD
- [ ] Add delta encoding for bandwidth-efficient state transfer

## Medium Priority
- [ ] Implement causal broadcast protocol for multi-device pub/sub
- [ ] Add RGA (Replicated Growable Array) CRDT for ordered collections
- [ ] Implement operational transformation for concurrent geometry edits
- [ ] Add conflict visualization (show divergent states per device)
- [ ] Implement state snapshot and restore for fast sync bootstrap
- [ ] Add sync protocol authentication (device identity verification)
- [ ] Implement partial replication (sync only spatial region of interest)
- [ ] Add vector clock compaction for long-running sessions
- [ ] Implement hybrid logical clocks (HLC) for wall-clock ordering

## Low Priority / Future
- [ ] Add WebRTC data channel transport for browser-to-browser sync
- [ ] Implement Byzantine fault-tolerant consensus for critical metadata
- [ ] Add sync protocol fuzzing for correctness verification
- [ ] Implement sync bandwidth estimation and adaptive batching
- [ ] Add integration with oxigdal-offline for seamless offline/online transition
- [ ] Implement multi-master conflict resolution for PostGIS replication
