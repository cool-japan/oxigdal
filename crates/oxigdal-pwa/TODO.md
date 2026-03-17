# TODO: oxigdal-pwa

## High Priority
- [ ] Implement actual Service Worker registration via web_sys APIs
- [ ] Add IndexedDB-backed tile cache for offline geospatial data access
- [ ] Implement Cache API integration for network request interception
- [ ] Add real push notification subscription with VAPID key support
- [ ] Implement background sync queue that retries failed uploads when online

## Medium Priority
- [ ] Add tile prefetch strategy that downloads adjacent tiles during idle time
- [ ] Implement bandwidth estimation to select optimal tile quality level
- [ ] Add periodic background sync for STAC catalog updates
- [ ] Implement app update detection with user prompt for new versions
- [ ] Add share target API support for receiving geospatial files
- [ ] Implement file handling API for opening .tif/.geojson from OS file picker
- [ ] Add Web Share API integration for sharing map views and screenshots
- [ ] Implement storage estimation and quota management with cleanup policies

## Low Priority / Future
- [ ] Add Workbox integration for declarative caching route configuration
- [ ] Implement payment request API for premium tile layer subscriptions
- [ ] Add Web Bluetooth integration for field sensor data collection
- [ ] Implement credential management API for tile server authentication
- [ ] Add screen wake lock for continuous GPS tracking mode
- [ ] Implement content indexing API for offline search of cached datasets
- [ ] Add badging API to show unsynced edit count on app icon
