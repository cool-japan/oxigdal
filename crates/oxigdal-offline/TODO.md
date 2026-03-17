# TODO: oxigdal-offline

## High Priority
- [ ] Implement SQLite storage backend for native platforms
- [ ] Add IndexedDB storage backend for WASM (via web-sys)
- [ ] Implement three-way merge conflict resolution
- [ ] Add connectivity detection (online/offline state transitions)
- [ ] Implement background sync worker with configurable interval
- [ ] Add sync progress reporting with estimated time remaining

## Medium Priority
- [ ] Implement delta sync (only transfer changed bytes, not full records)
- [ ] Add tile cache management for offline map viewing
- [ ] Implement vector feature versioning with full history
- [ ] Add selective sync (choose which layers/areas to sync)
- [ ] Implement bandwidth-aware sync (throttle on metered connections)
- [ ] Add sync conflict UI helpers (serialize conflict info for display)
- [ ] Implement offline spatial queries using local R-tree index
- [ ] Add queue persistence across app restarts
- [ ] Implement sync protocol versioning for backward compatibility

## Low Priority / Future
- [ ] Add peer-to-peer sync without central server (via CRDTs from oxigdal-sync)
- [ ] Implement offline raster tile pyramid generation
- [ ] Add storage quota management with automatic eviction
- [ ] Implement sync analytics (track sync frequency, data volume, conflicts)
- [ ] Add encryption for local storage (at-rest encryption)
- [ ] Implement multi-user offline collaboration
