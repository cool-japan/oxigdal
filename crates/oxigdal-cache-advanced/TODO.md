# TODO: oxigdal-cache-advanced

> **Purpose:** Multi-tier caching for OxiGDAL — L1 in-memory, L2 on-disk, optional L3 network; eviction policies (LRU/LFU/ARC/W-TinyLFU), adaptive compression (lz4/zstd/snappy via `oxiarc-*`), predictive prefetching, coherency, partitioning, warming, write policies.
> **Status (2026-05-16):** 7,270 LoC · 31 tests · 1 real stub (`distributed.rs:212` remote lookup returns `None` placeholder; rest of distributed path conceptual).
> **Roadmap:** v0.1.5 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] L1 (memory) cache with LRU + LFU + ARC + W-TinyLFU eviction policies
  - **Verified gap:** Previous TODO Item 1 + clean source tree: `src/eviction.rs` (16 KB) exists with skeleton but tests count = 31 across whole crate suggests no end-to-end policy regression coverage.
  - **Goal:** All four canonical policies implemented behind a common `EvictionPolicy` trait, with crisp asymptotic complexity guarantees:
    - LRU: doubly-linked list + `HashMap`, O(1) get/put.
    - LFU: min-heap of frequency counters, O(log n) get/put (or O(1) with the constant-time LFU of Shah, Mitra & Matani 2010).
    - ARC (Adaptive Replacement Cache, Megiddo & Modha 2003, FAST): two LRU lists T1/T2 plus ghost lists B1/B2, adaptive parameter `p`.
    - W-TinyLFU (Einziger, Friedman & Manes 2017, ACM TOS): Count-Min Sketch admission filter + Window-LRU + SLRU main, ~1% memory overhead, dominates LRU on Zipfian workloads.
  - **Design:** `trait EvictionPolicy<K, V> { fn on_get(&mut self, k: &K); fn on_put(&mut self, k: &K) -> Option<K> /* evicted */; }`. Generic over `K: Hash + Eq + Clone`. CMS for W-TinyLFU sized to `~capacity * 4` counters at 4 bits each (paper §4.2).
  - **Files:** `crates/oxigdal-cache-advanced/src/eviction.rs` (extend), new `src/eviction/{lru,lfu,arc,wtinylfu}.rs` modules.
  - **Tests:** (proposed) `test_lru_evicts_least_recent`, `test_lfu_evicts_least_frequent`, `test_arc_adapts_to_recency_vs_frequency`, `test_wtinylfu_admission_filter_rejects_one_hit_wonders`, `test_wtinylfu_better_than_lru_on_zipfian_alpha_0_99`, `test_policy_thread_safety_under_concurrent_get`.
  - **Risk:** ARC has IBM patent expired 2024-12; W-TinyLFU is patent-free.
  - **Prerequisites:** None.

- [ ] L2 (SSD/disk) cache with memory-mapped file backing
  - **Goal:** Disk-backed second tier with mmap'd index + segment files; reads served from `mmap`-backed slice; writes append to an active segment and roll at 256 MiB.
  - **Design:** Segment file format: `[magic: u32][version: u8][entry_count: u32][entries...]` where each entry is `{key_hash: u64, offset: u64, length: u32, ttl_secs: u32}`. Index file mirrors this in mmap layout. Eviction triggers segment compaction (rewrite live entries to a new segment, drop old). Use `memmap2 = "0.9"` (Pure Rust).
  - **Files:** `crates/oxigdal-cache-advanced/src/tiering/l2_disk.rs` (new ~700 LoC).
  - **Tests:** (proposed) `test_l2_persistent_across_restarts`, `test_l2_segment_roll_at_256mib`, `test_l2_compaction_drops_evicted_entries`, `test_l2_corrupted_segment_skipped`, `test_l2_mmap_read_zero_copy`, `test_l2_concurrent_read_during_compaction`.
  - **Risk:** Mmap+truncate races on Windows — gate behind `cfg(unix)` or use `File::sync_all` discipline.
  - **Prerequisites:** Item 1 (eviction trait).

- [ ] Real remote-lookup path in distributed cache (replace `None` placeholder)
  - **Verified gap:** `src/distributed.rs:211-216` — `// Remote lookup (would use network RPC in production) / // For now, return None / let mut stats = self.stats.write().await; / stats.misses += 1; / return Ok(None);`
  - **Goal:** Actual gRPC `CacheGet` RPC to the node identified by the consistent-hash ring for the key; uses `tonic` (already a dep).
  - **Design:** New `src/distributed/transport.rs` with `cache_proto::CacheServiceClient`; `DistributedCache::get(key)` dispatches to the local store if `ring.get_nodes(key, 1)[0] == self.local_node` else uses the per-peer client (kept in `DashMap<NodeId, CacheServiceClient<Channel>>`). Replication factor `n` already in the design.
  - **Files:** `crates/oxigdal-cache-advanced/src/distributed.rs` (replace lines 200-220 path), new `src/distributed/transport.rs`, `proto/cache.proto` (new).
  - **Tests:** (proposed) `test_distributed_get_hits_remote_when_not_owner`, `test_distributed_put_replicates_to_n_nodes`, `test_distributed_quorum_read_majority`, `test_distributed_partition_returns_partial`, `test_distributed_consistent_hash_routing`.
  - **Risk:** `tonic` adds tokio dependency to the cache crate (already a dep). Coordinate transport definitions with oxigdal-cluster's own `cluster.proto` to avoid divergence.
  - **Prerequisites:** None.

- [ ] Adaptive compression — zstd level selection by entropy
  - **Goal:** Pick zstd level 1-19 per entry based on Shannon entropy of the first 4 KiB of the value, balancing compression ratio vs CPU.
  - **Design:** Compute `H = -Σ p_i·log₂(p_i)` over byte histogram of sample; map `H ∈ [0, 8]` to level via piecewise table: H>7.5 → level 1 (incompressible, skip); 6<H≤7.5 → level 3; 4<H≤6 → level 9; H≤4 → level 19. Always use `oxiarc-zstd` (workspace dep at `Cargo.toml:33`).
  - **Files:** `crates/oxigdal-cache-advanced/src/compression.rs` (extend).
  - **Tests:** (proposed) `test_entropy_random_data_close_to_8`, `test_entropy_repeated_byte_zero`, `test_compression_level_high_entropy_picks_low_level`, `test_compression_level_low_entropy_picks_high_level`, `test_compression_skip_when_incompressible`.
  - **Risk:** Sampling 4 KiB on every put adds latency — apply only when value > 16 KiB.
  - **Prerequisites:** None.

- [ ] Predictive prefetching using access-pattern analysis
  - **Goal:** Learn sequential / strided / tile-neighbour access patterns from a sliding window of past keys and prefetch likely-next keys into L1.
  - **Design:** `PredictiveModel` trait with three implementations:
    - `Sequential`: detects monotone increasing keys (tile_id+1).
    - `Strided`: detects fixed-delta patterns.
    - `TileNeighbour`: parses `(z, x, y)` tile keys and prefetches the 8 neighbours when a tile is hit (per typical pan/zoom workflow).
  - **Files:** `crates/oxigdal-cache-advanced/src/predictive/mod.rs` (extend).
  - **Tests:** (proposed) `test_sequential_pattern_detected_after_three_hits`, `test_strided_pattern_detected_with_negative_stride`, `test_tile_neighbour_prefetch_8_around_hit`, `test_prefetch_does_not_evict_hot_keys`, `test_prefetch_budget_bounded_per_second`.
  - **Risk:** Over-prefetching can evict useful keys — budget capped at 25% of L1 capacity per second.
  - **Prerequisites:** Item 1 (eviction integration).

- [ ] Cache warming from cold start
  - **Goal:** On startup, preload frequently accessed entries from a persisted "warmth manifest" (written periodically by the analytics module).
  - **Design:** `WarmingPlan { entries: Vec<(CacheKey, Priority)> }` loaded from `<cache_dir>/warmth.json` on `MultiTierCache::open()`; sequentially fetches each entry into L1 with respect to L1 capacity. Manifest is written every 5 min by the `analytics` module based on hit-rate aggregation.
  - **Files:** `crates/oxigdal-cache-advanced/src/warming.rs` (extend; file already 15.7 KB).
  - **Tests:** (proposed) `test_warming_manifest_roundtrip`, `test_warming_skips_when_l1_full`, `test_warming_priority_ordering`, `test_warming_manifest_corrupted_fails_gracefully`.
  - **Risk:** Stale manifest entries waste cycles — TTL on each entry; drop entries older than 24h.
  - **Prerequisites:** Item 1 (L1 capacity API).

## Medium Priority
- [ ] Distributed cache coherency protocol (consistent hashing + gossip for membership).
  - **Files:** `src/coherency/mod.rs` (extend).
  - **Why deferred:** Needs transport (Item 3).
- [ ] Tile-aware cache partitioning (zoom-level / region).
  - **Files:** `src/partitioning.rs` (extend).
  - **Why deferred:** Generic partitioner exists; tile-aware is niche.
- [ ] Write-through and write-back policies.
  - **Files:** `src/write_policy.rs` (extend; file already 12.3 KB).
  - **Why deferred:** Existing skeleton functional; needs L2 (Item 2) for write-back path.
- [ ] Eviction analytics — track eviction reasons + hit/miss pattern.
  - **Files:** `src/analytics.rs` (extend).
  - **Why deferred:** Needs Item 1 eviction trait to emit events.
- [ ] TTL-based expiration with lazy cleanup.
  - **Files:** `src/multi_tier.rs` (extend).
  - **Why deferred:** Simple `expires_at: Instant`; defer until L2 lands.
- [ ] L3 (network) tier with Redis / Memcached backend.
  - **Files:** `src/tiering/l3_network.rs` (new).
  - **Why deferred:** Pure-Rust Redis client (`redis-rs`) is workspace-acceptable; defer.
- [ ] Cache size auto-tuning from system memory (`sysinfo` is available via oxigdal-core deps).
  - **Files:** `src/multi_tier.rs` (extend `CacheConfig::auto_size`).
  - **Why deferred:** Static config covers most users.
- [ ] Per-dataset isolation (prevent one dataset evicting another).
  - **Files:** `src/partitioning.rs`.
  - **Why deferred:** Niche; defer.
- [ ] Bloom-filter negative cache to avoid repeated misses.
  - **Files:** `src/multi_tier.rs` (extend).
  - **Why deferred:** Useful for read-heavy workloads with high miss ratio; defer.

## Low Priority / Future (one-liners)
- [ ] ML-based admission policy (predict item reuse probability).
- [ ] Cache migration between tiers based on access frequency.
- [ ] Cache snapshot / restore for fast cold start.
- [ ] Cache deduplication across datasets (content-addressed storage via SHA-256).
- [ ] Prometheus metrics for cache performance (`observability.rs` scaffold exists).
- [ ] Geographic-aware caching (prioritize tiles near viewport).

## Cross-crate dependencies
- **Blocks:** oxigdal-services (cached tile serving), oxigdal-cluster (distributed cache uses this crate's coherency contract).
- **Blocked by:** `oxiarc-{lz4,zstd,snappy}` (workspace deps), `tonic` (distributed transport).

## Recently completed (verbatim)
*(No `[x]` entries on previous TODO.)*

---
*Last audited: 2026-05-17*
