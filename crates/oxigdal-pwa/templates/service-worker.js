// OxiGDAL PWA Service Worker Template
// This template provides basic service worker functionality for PWA

const CACHE_VERSION = 'v1';
const STATIC_CACHE = `oxigdal-static-${CACHE_VERSION}`;
const DYNAMIC_CACHE = `oxigdal-dynamic-${CACHE_VERSION}`;
const TILE_CACHE = `oxigdal-tiles-${CACHE_VERSION}`;

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
                        return cacheName.startsWith('oxigdal-') &&
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
        title: 'OxiGDAL PWA',
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

async function handleBackgroundSync(tag) {
    console.log('[ServiceWorker] Handling background sync:', tag);

    // Implement your background sync logic here
    // For example, upload queued data to the server

    try {
        // Example: Get queued operations from IndexedDB
        // const queue = await getQueueFromIndexedDB(tag);
        //
        // for (const operation of queue) {
        //     await fetch(operation.url, operation.options);
        // }

        console.log('[ServiceWorker] Background sync completed:', tag);
    } catch (error) {
        console.error('[ServiceWorker] Background sync failed:', tag, error);
        throw error;
    }
}

console.log('[ServiceWorker] Service worker loaded');
