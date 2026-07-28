// OxiGeo PWA Service Worker Template
// This template provides basic service worker functionality for PWA

const CACHE_VERSION = 'v1';
const STATIC_CACHE = `oxigeo-static-${CACHE_VERSION}`;
const DYNAMIC_CACHE = `oxigeo-dynamic-${CACHE_VERSION}`;
const TILE_CACHE = `oxigeo-tiles-${CACHE_VERSION}`;

// Static assets to cache on install
const STATIC_ASSETS = [
    '/',
    '/index.html',
    '/manifest.json',
    '/icons/icon-192x192.png',
    '/icons/icon-512x512.png',
];

// Install event - cache static assets
self.addEventListener('install', (event) => {
    console.log('[ServiceWorker] Install');

    event.waitUntil(
        caches.open(STATIC_CACHE)
            .then((cache) => {
                console.log('[ServiceWorker] Caching static assets');
                return cache.addAll(STATIC_ASSETS);
            })
            .then(() => self.skipWaiting())
    );
});

// Activate event - clean up old caches
self.addEventListener('activate', (event) => {
    console.log('[ServiceWorker] Activate');

    event.waitUntil(
        caches.keys().then((cacheNames) => {
            return Promise.all(
                cacheNames
                    .filter((cacheName) => {
                        return cacheName.startsWith('oxigeo-') &&
                            cacheName !== STATIC_CACHE &&
                            cacheName !== DYNAMIC_CACHE &&
                            cacheName !== TILE_CACHE;
                    })
                    .map((cacheName) => {
                        console.log('[ServiceWorker] Deleting old cache:', cacheName);
                        return caches.delete(cacheName);
                    })
            );
        }).then(() => self.clients.claim())
    );
});

// Fetch event - implement caching strategies
self.addEventListener('fetch', (event) => {
    const { request } = event;
    const url = new URL(request.url);

    // Tile requests - cache first strategy
    if (isTileRequest(url)) {
        event.respondWith(cacheFirstStrategy(request, TILE_CACHE));
        return;
    }

    // API requests - network first strategy
    if (isApiRequest(url)) {
        event.respondWith(networkFirstStrategy(request, DYNAMIC_CACHE));
        return;
    }

    // Static assets - cache first strategy
    if (isStaticAsset(url)) {
        event.respondWith(cacheFirstStrategy(request, STATIC_CACHE));
        return;
    }

    // Default - network first with cache fallback
    event.respondWith(networkFirstStrategy(request, DYNAMIC_CACHE));
});

// Message event - handle commands from clients
self.addEventListener('message', (event) => {
    const { data } = event;

    if (data.type === 'SKIP_WAITING') {
        self.skipWaiting();
    }

    if (data.type === 'CLAIM_CLIENTS') {
        self.clients.claim();
    }

    if (data.type === 'CLEAR_CACHES') {
        event.waitUntil(
            caches.keys().then((cacheNames) => {
                return Promise.all(
                    cacheNames.map((cacheName) => caches.delete(cacheName))
                );
            }).then(() => {
                // Notify client
                event.ports[0].postMessage({ success: true });
            })
        );
    }

    if (data.type === 'GET_CACHE_NAMES') {
        event.waitUntil(
            caches.keys().then((cacheNames) => {
                event.ports[0].postMessage({
                    success: true,
                    data: cacheNames
                });
            })
        );
    }

    if (data.type === 'PREFETCH_RESOURCES') {
        event.waitUntil(
            caches.open(DYNAMIC_CACHE).then((cache) => {
                return cache.addAll(data.payload.urls);
            }).then(() => {
                event.ports[0].postMessage({ success: true });
            })
        );
    }
});

// Background sync event
self.addEventListener('sync', (event) => {
    console.log('[ServiceWorker] Background sync:', event.tag);

    if (event.tag.startsWith('sync-')) {
        event.waitUntil(handleBackgroundSync(event.tag));
    }
});

// Push event - handle push notifications
self.addEventListener('push', (event) => {
    console.log('[ServiceWorker] Push notification received');

    let notificationData = {
        title: 'OxiGeo PWA',
        body: 'You have a new notification',
        icon: '/icons/icon-192x192.png',
        badge: '/icons/badge-72x72.png',
    };

    if (event.data) {
        try {
            notificationData = event.data.json();
        } catch (e) {
            notificationData.body = event.data.text();
        }
    }

    event.waitUntil(
        self.registration.showNotification(notificationData.title, {
            body: notificationData.body,
            icon: notificationData.icon,
            badge: notificationData.badge,
            tag: notificationData.tag || 'default',
            requireInteraction: notificationData.requireInteraction || false,
        })
    );
});

// Notification click event
self.addEventListener('notificationclick', (event) => {
    console.log('[ServiceWorker] Notification click:', event.notification.tag);

    event.notification.close();

    event.waitUntil(
        clients.matchAll({ type: 'window' }).then((clientList) => {
            // If a window is already open, focus it
            for (let client of clientList) {
                if (client.url === '/' && 'focus' in client) {
                    return client.focus();
                }
            }
            // Otherwise, open a new window
            if (clients.openWindow) {
                return clients.openWindow('/');
            }
        })
    );
});

// Helper functions

function isTileRequest(url) {
    // Match tile URLs like /tiles/{z}/{x}/{y}
    return url.pathname.match(/\/tiles\/\d+\/\d+\/\d+/);
}

function isApiRequest(url) {
    return url.pathname.startsWith('/api/');
}

function isStaticAsset(url) {
    const staticExtensions = ['.html', '.css', '.js', '.png', '.jpg', '.svg', '.woff', '.woff2'];
    return staticExtensions.some(ext => url.pathname.endsWith(ext));
}

async function cacheFirstStrategy(request, cacheName) {
    const cache = await caches.open(cacheName);
    const cached = await cache.match(request);

    if (cached) {
        return cached;
    }

    try {
        const response = await fetch(request);
        if (response.ok) {
            cache.put(request, response.clone());
        }
        return response;
    } catch (error) {
        console.error('[ServiceWorker] Cache first strategy failed:', error);
        throw error;
    }
}

async function networkFirstStrategy(request, cacheName) {
    const cache = await caches.open(cacheName);

    try {
        const response = await fetch(request);
        if (response.ok) {
            cache.put(request, response.clone());
        }
        return response;
    } catch (error) {
        console.log('[ServiceWorker] Network failed, trying cache');
        const cached = await cache.match(request);
        if (cached) {
            return cached;
        }
        throw error;
    }
}

async function staleWhileRevalidateStrategy(request, cacheName) {
    const cache = await caches.open(cacheName);
    const cached = await cache.match(request);

    const fetchPromise = fetch(request).then((response) => {
        if (response.ok) {
            cache.put(request, response.clone());
        }
        return response;
    });

    return cached || fetchPromise;
}

// IndexedDB schema shared with the Rust side (see
// `src/sync.rs::persistence` in the oxigeo-pwa crate). MUST be kept in sync:
// changing any of these three constants without updating the Rust side (or
// vice versa) breaks background sync replay silently.
const SYNC_DB_NAME = 'oxigeo-pwa-sync';
const SYNC_DB_VERSION = 1;
const SYNC_STORE_NAME = 'sync_operations';

// Open (never upgrades here -- the Rust side owns schema creation via
// `persistence::persist_operation`; if the store doesn't exist yet there is
// simply nothing queued for this tag).
function openSyncDb() {
    return new Promise((resolve, reject) => {
        const request = indexedDB.open(SYNC_DB_NAME, SYNC_DB_VERSION);
        request.onupgradeneeded = () => {
            // Nothing was ever persisted (the page never called
            // `enqueue_operation`), so there's no schema to migrate --
            // create the store defensively so a subsequent `getAll()` on a
            // fresh database doesn't reject.
            const db = request.result;
            if (!db.objectStoreNames.contains(SYNC_STORE_NAME)) {
                db.createObjectStore(SYNC_STORE_NAME);
            }
        };
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
    });
}

function idbRequestToPromise(request) {
    return new Promise((resolve, reject) => {
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
    });
}

// Fetch every persisted operation belonging to `queueName` (the
// `queue_name` field set by `persistence::persist_operation`).
async function getQueuedOperations(db, queueName) {
    const txn = db.transaction(SYNC_STORE_NAME, 'readonly');
    const store = txn.objectStore(SYNC_STORE_NAME);
    const all = await idbRequestToPromise(store.getAll());
    return (all || []).filter((op) => op && op.queue_name === queueName);
}

async function deleteQueuedOperation(db, operationId) {
    const txn = db.transaction(SYNC_STORE_NAME, 'readwrite');
    const store = txn.objectStore(SYNC_STORE_NAME);
    await idbRequestToPromise(store.delete(operationId));
}

// Replay a single queued operation against its recorded endpoint. Returns
// normally on success (2xx/3xx response); throws on any failure (network
// error or non-ok response) so the caller can decide whether to retry.
async function replayOperation(operation) {
    const response = await fetch(operation.endpoint, {
        method: operation.method || 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(operation.data),
    });

    if (!response.ok) {
        throw new Error(
            `replay of operation ${operation.id} failed: HTTP ${response.status}`
        );
    }
}

async function handleBackgroundSync(tag) {
    console.log('[ServiceWorker] Handling background sync:', tag);

    // `persistence::tag_for_queue` in the Rust side formats tags as
    // `sync-{queue_name}` -- recover the queue name by stripping that
    // fixed prefix. Non-matching tags (registered by unrelated code) are
    // intentionally ignored rather than errored on.
    if (!tag.startsWith('sync-')) {
        console.log('[ServiceWorker] Ignoring unrecognized sync tag:', tag);
        return;
    }
    const queueName = tag.slice('sync-'.length);

    const db = await openSyncDb();
    let operations;
    try {
        operations = await getQueuedOperations(db, queueName);
    } finally {
        db.close();
    }

    if (operations.length === 0) {
        console.log('[ServiceWorker] No queued operations for tag:', tag);
        return;
    }

    console.log(
        `[ServiceWorker] Replaying ${operations.length} queued operation(s) for`,
        tag
    );

    const failures = [];
    for (const operation of operations) {
        try {
            await replayOperation(operation);

            // Success: remove the persisted operation so it isn't replayed
            // again on the next sync event.
            const db2 = await openSyncDb();
            try {
                await deleteQueuedOperation(db2, operation.id);
            } finally {
                db2.close();
            }
            console.log('[ServiceWorker] Replayed operation:', operation.id);
        } catch (error) {
            // Leave the operation persisted so it's retried next time; the
            // browser's own SyncManager retry/backoff (triggered by
            // rethrowing below) governs when that next attempt happens.
            console.error(
                '[ServiceWorker] Failed to replay operation:',
                operation.id,
                error
            );
            failures.push({ operation, error });
        }
    }

    if (failures.length > 0) {
        // Rethrow so the browser knows this sync attempt was not fully
        // successful and should be retried (per the Background Sync API
        // contract), rather than silently reporting success while
        // operations remain un-replayed.
        throw new Error(
            `${failures.length} of ${operations.length} queued operation(s) failed to replay for tag ${tag}`
        );
    }

    console.log('[ServiceWorker] Background sync completed:', tag);
}

console.log('[ServiceWorker] Service worker loaded');
