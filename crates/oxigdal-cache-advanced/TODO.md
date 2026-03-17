# TODO: oxigdal-cache-advanced

## High Priority
- [ ] Implement L1 (memory) cache with LRU/LFU/ARC eviction policies
- [ ] Add L2 (SSD/disk) cache with memory-mapped file backing
- [ ] Implement cache coherency protocol for multi-process access
- [ ] Add adaptive compression (zstd level selection based on data entropy)
- [ ] Implement predictive prefetching using access pattern analysis
- [ ] Add cache warming from cold start (preload frequently accessed tiles)

## Medium Priority
- [ ] Implement distributed cache protocol (consistent hashing, gossip)
- [ ] Add tile-aware cache partitioning (partition by zoom level/region)
- [ ] Implement write-through and write-back policies
- [ ] Add cache eviction analytics (track eviction reasons, hit/miss patterns)
- [ ] Implement TTL-based expiration with lazy cleanup
- [ ] Add L3 (network) cache tier with Redis/Memcached backend
- [ ] Implement cache size auto-tuning based on available system memory
- [ ] Add per-dataset cache isolation (prevent one dataset from evicting another)
- [ ] Implement bloom filter for negative cache (avoid repeated misses)

## Low Priority / Future
- [ ] Add ML-based cache admission policy (predict item reuse probability)
- [ ] Implement cache migration between tiers based on access frequency
- [ ] Add cache snapshot/restore for fast cold start
- [ ] Implement cache deduplication across datasets (content-addressed storage)
- [ ] Add observability integration (Prometheus metrics for cache performance)
- [ ] Implement geographic-aware caching (prioritize tiles near viewport center)
