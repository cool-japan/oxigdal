# TODO: oxigdal-mobile-enhanced

## High Priority
- [ ] Implement real battery level reading via platform APIs (currently simulated)
- [ ] Add actual network type detection (WiFi/cellular/offline) via platform APIs
- [ ] Implement adaptive tile quality based on network bandwidth measurement
- [ ] Add background task scheduling that integrates with iOS BGTaskScheduler / Android WorkManager
- [ ] Implement storage quota management with LRU eviction for tile cache

## Medium Priority
- [ ] Add iOS Metal GPU acceleration hints for raster processing
- [ ] Implement Android RenderScript fallback for image processing on older devices
- [ ] Add memory pressure handler that responds to iOS didReceiveMemoryWarning
- [ ] Implement adaptive compression level based on available CPU and battery
- [ ] Add geofencing-aware prefetch (pre-download tiles for known routes)
- [ ] Implement incremental sync protocol for partial dataset updates
- [ ] Add thermal state monitoring to throttle processing on hot devices
- [ ] Implement offline-first vector tile rendering with local cache

## Low Priority / Future
- [ ] Add Bluetooth/UWB peer-to-peer data sharing between devices
- [ ] Implement AR overlay pipeline for camera-based geospatial visualization
- [ ] Add on-device ML model management (download, update, rollback)
- [ ] Implement location-aware cache prewarming using movement prediction
- [ ] Add accessibility features (VoiceOver/TalkBack support for map data)
- [ ] Implement delta sync for large raster datasets over cellular
- [ ] Add watchOS/Wear OS companion data relay
