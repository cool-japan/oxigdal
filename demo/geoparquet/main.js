/**
 * OxiGeo GeoParquet Live — main application.
 *
 * A 5.9 GB GeoParquet is queried straight from the browser: the Parquet
 * footer is fetched once (with a Cache API fast path), bounding-box +
 * attribute predicates are planned against row-group metadata inside
 * WebAssembly, and only the surviving column chunks are downloaded via
 * HTTP range requests. No database, no server-side code.
 *
 * Error convention: the WASM layer rejects with JSON strings
 * `{code, message, detail}` (see crates/oxigeo-wasm-geoparquet/src/error.rs).
 */

import init, { RemoteGeoParquet } from './pkg/oxigeo_geoparquet.js';
import { BoxDraw } from './map-draw.js';
import { RowGroupStrip } from './rg-strip.js';

/* global L */

// ── Configuration ────────────────────────────────────────────────────────────

/** The live VIDA Google-Microsoft-OSM open-buildings file for Japan (5.9 GB). */
const LIVE_URL =
    'https://data.source.coop/vida/google-microsoft-osm-open-buildings/' +
    'geoparquet/by_country/country_iso=JPN/JPN.parquet';

/** Extra columns projected into feature properties (geometry + area are implicit). */
const EXTRA_COLUMNS = ['confidence'];

/** Cache API bucket for parquet footers. */
const FOOTER_CACHE = 'oxigeo-gpq-footer-v1';

/** Only this many features get click popups (contract: no handlers above 20k). */
const POPUP_LIMIT = 20000;

// ── Application state ────────────────────────────────────────────────────────

const app = {
    ds: null,                // RemoteGeoParquet session
    map: null,
    boxDraw: null,
    strip: null,
    geoLayer: null,
    canvasRenderer: null,
    bbox: null,              // [minLon, minLat, maxLon, maxLat]
    bytesFetched: 0,         // via the window.fetch wrapper (data only)
    planTimer: 0,
    busy: false,
    columnTypes: null,       // Map<name, physicalType> from footer_info()
};

const $ = (id) => document.getElementById(id);

// ── Fetch accounting (GeoLab shell pattern) ──────────────────────────────────
//
// Every data request — the trailer probe, the footer, and the WASM module's
// own range requests — goes through window.fetch, so one wrapper is the
// single source of truth for the "fetched" honesty badge. App assets
// (WASM, vendored JS/CSS, examples.json) are excluded.

function installFetchAccounting() {
    if (window.__oxigeoFetchWrapped) return;
    const originalFetch = window.fetch.bind(window);
    window.fetch = async (input, init2) => {
        const response = await originalFetch(input, init2);
        try {
            let url = '';
            if (typeof input === 'string') url = input;
            else if (input) url = input.url || input.href || String(input);
            if (!isAppAssetUrl(url)) {
                const len = response.headers.get('content-length');
                if (len) {
                    app.bytesFetched += parseInt(len, 10) || 0;
                    updateBadges();
                }
            }
        } catch (_e) { /* accounting must never break the fetch itself */ }
        return response;
    };
    window.__oxigeoFetchWrapped = true;
}

function isAppAssetUrl(url) {
    if (!url) return false;
    return /\/(pkg|vendor)\//.test(url) ||
        url.endsWith('.wasm') || url.endsWith('.js') ||
        url.endsWith('.css') || url.endsWith('examples.json');
}

// ── Formatting helpers ───────────────────────────────────────────────────────

function fmtBytes(n) {
    // Decimal units, matching how the 5.9 GB dataset is described upstream.
    if (!Number.isFinite(n)) return '—';
    if (n >= 1e9) return `${(n / 1e9).toFixed(1)} GB`;
    if (n >= 1e6) return `${(n / 1e6).toFixed(1)} MB`;
    if (n >= 1e3) return `${(n / 1e3).toFixed(1)} KB`;
    return `${n} B`;
}

const fmtInt = (n) => Number(n).toLocaleString('en-US');

function fmtArea(m2) {
    if (m2 >= 1e6) return `${(m2 / 1e6).toFixed(2)} km²`;
    return `${fmtInt(Math.round(m2))} m²`;
}

// ── Status / banner / badges ─────────────────────────────────────────────────

function setStatus(kind, text) {
    const dot = $('status-dot');
    dot.className = `status-dot status-${kind}`;
    $('status-text').textContent = text;
}

function showBanner(text, isError = false) {
    const banner = $('banner');
    banner.classList.remove('hidden');
    banner.classList.toggle('banner-error', isError);
    $('banner-text').textContent = text;
}

function hideBanner() { $('banner').classList.add('hidden'); }

function updateBadges() {
    $('badge-fetched').textContent = `fetched: ${fmtBytes(app.bytesFetched)}`;
    if (app.ds) {
        const info = app.ds.footer_info();
        $('badge-dataset').textContent = `dataset: ${fmtBytes(info.sizeBytes)}`;
    }
}

// ── Filter input ─────────────────────────────────────────────────────────────
//
// The WASM predicate engine coerces numeric literals to each column's arrow
// type, so a bare integer literal compares correctly against a Float64 column
// (`area_in_meters > 500`) and a whole-valued float compares against an integer
// column. No client-side literal rewriting is needed — the expression is sent
// to the `sqlparser` lowering verbatim.

/** The filter box contents as the `Option<String>` WASM argument. */
function currentFilter() {
    const raw = $('filter-input').value.trim();
    return raw || null;
}

/** Parse a WASM rejection into `{code, message, detail}` (C4 convention). */
function parseWasmError(e) {
    if (typeof e === 'string') {
        try { return JSON.parse(e); } catch (_x) { return { code: 'unknown', message: e, detail: null }; }
    }
    if (e && typeof e.message === 'string') return { code: 'js', message: e.message, detail: null };
    return { code: 'unknown', message: String(e), detail: null };
}

// ── Cache API footer cache ───────────────────────────────────────────────────
//
// Keyed per source URL on our own origin (fragments are ignored by the Cache
// API, so the logical "url#footer" key is encoded as a query parameter). The
// source's ETag / Last-Modified is stored alongside and must match on read.

function footerCacheKey(url) {
    return `./__footer_cache__?src=${encodeURIComponent(url)}`;
}

async function loadCachedFooter(url, validator) {
    try {
        if (!('caches' in window)) return null;
        const cache = await caches.open(FOOTER_CACHE);
        const hit = await cache.match(footerCacheKey(url));
        if (!hit) return null;
        if ((hit.headers.get('x-source-validator') || '') !== validator) {
            await cache.delete(footerCacheKey(url));
            return null;
        }
        return new Uint8Array(await hit.arrayBuffer());
    } catch (_e) { return null; }
}

async function storeCachedFooter(url, validator, bytes) {
    try {
        if (!('caches' in window)) return;
        const cache = await caches.open(FOOTER_CACHE);
        const resp = new Response(bytes, {
            headers: { 'x-source-validator': validator, 'content-type': 'application/octet-stream' },
        });
        await cache.put(footerCacheKey(url), resp);
    } catch (_e) { /* quota errors etc. — caching is best-effort */ }
}

// ── Dataset opening ──────────────────────────────────────────────────────────

function setProgress(visible, fraction, label) {
    $('open-progress').classList.toggle('hidden', !visible);
    if (visible) {
        $('open-progress-fill').style.width = `${Math.round(fraction * 100)}%`;
        $('open-progress-label').textContent = label;
    }
}

/** Read a Response body with progress callbacks; returns exactly the body bytes. */
async function readWithProgress(resp, expected, onProgress) {
    if (!resp.body || !resp.body.getReader) {
        return new Uint8Array(await resp.arrayBuffer());
    }
    const reader = resp.body.getReader();
    const chunks = [];
    let received = 0;
    for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        chunks.push(value);
        received += value.length;
        onProgress(received, expected);
    }
    const out = new Uint8Array(received);
    let off = 0;
    for (const c of chunks) { out.set(c, off); off += c.length; }
    return out;
}

/**
 * Open a remote GeoParquet: probe the 8-byte trailer with a suffix range,
 * fetch (or Cache-API-restore) the footer with live progress, then hand it
 * to WASM via `open_with_footer`.
 */
async function openDataset(url) {
    hideBanner();
    setStatus('loading', 'opening dataset…');
    setProgress(true, 0, 'probing file trailer…');
    $('open-btn').disabled = true;

    try {
        // 1. Trailer: [footer_len u32 LE]["PAR1"] via suffix range.
        const trailerResp = await fetch(url, { headers: { Range: 'bytes=-8' } });
        if (trailerResp.status !== 206) {
            throw new Error(`server did not honor HTTP range requests (status ${trailerResp.status}); ` +
                'a range-capable server is required — see README.md');
        }
        const contentRange = trailerResp.headers.get('content-range') || '';
        const total = parseInt(contentRange.split('/').pop(), 10);
        const trailer = new Uint8Array(await trailerResp.arrayBuffer());
        if (trailer.length !== 8 || String.fromCharCode(...trailer.subarray(4)) !== 'PAR1') {
            throw new Error('missing PAR1 magic — not a Parquet file');
        }
        const footerLen = new DataView(trailer.buffer, trailer.byteOffset).getUint32(0, true);
        const validator = trailerResp.headers.get('etag') ||
            trailerResp.headers.get('last-modified') || `len:${total}`;

        // 2. Footer: Cache API fast path, else streamed download with progress.
        let footer = await loadCachedFooter(url, validator);
        if (footer && footer.length === footerLen) {
            $('badge-cache').textContent = `footer cache: hit (${fmtBytes(footerLen)} skipped)`;
            setProgress(true, 1, `footer restored from cache (${fmtBytes(footerLen)})`);
        } else {
            footer = null;
            $('badge-cache').textContent = 'footer cache: miss';
            const start = total - 8 - footerLen;
            const footResp = await fetch(url, { headers: { Range: `bytes=${start}-${total - 9}` } });
            if (footResp.status !== 206) {
                throw new Error(`footer range request failed (status ${footResp.status})`);
            }
            footer = await readWithProgress(footResp, footerLen, (done, exp) => {
                setProgress(true, done / exp, `fetching footer: ${fmtBytes(done)} / ${fmtBytes(exp)}`);
            });
            if (footer.length !== footerLen) {
                throw new Error(`footer truncated: got ${footer.length} of ${footerLen} bytes`);
            }
            await storeCachedFooter(url, validator, footer);
        }

        // 3. Decode inside WASM (the heavy metadata stays behind one Arc there).
        setProgress(true, 1, 'decoding footer metadata…');
        await new Promise((r) => setTimeout(r, 0)); // let the label paint
        if (app.ds) { app.ds.free(); app.ds = null; }
        app.ds = await RemoteGeoParquet.open_with_footer(url, footer);

        // 4. Surface the facts.
        const info = app.ds.footer_info();
        app.columnTypes = new Map(info.columns.map((c) => [c.name, c.physicalType]));
        $('dataset-facts').classList.remove('hidden');
        $('fact-size').textContent = fmtBytes(info.sizeBytes);
        $('fact-rows').textContent = fmtInt(info.rows);
        $('fact-rowgroups').textContent = fmtInt(info.rowGroups);
        $('fact-footer').textContent = fmtBytes(info.footerBytes);
        $('fact-columns').textContent = `${info.columns.length} (${info.columns.slice(0, 4).map((c) => c.name).join(', ')}…)`;
        app.strip.setTotal(info.rowGroups);
        $('strip-counts').textContent = `${fmtInt(info.rowGroups)} total`;

        setProgress(false, 0, '');
        setStatus('ready', 'dataset open');
        $('draw-btn').disabled = false;
        updateBadges();
        refreshQueryButton();
    } catch (e) {
        const err = parseWasmError(e);
        setProgress(false, 0, '');
        setStatus('error', 'open failed');
        showBanner(`Could not open dataset: ${err.message}`, true);
    } finally {
        $('open-btn').disabled = false;
    }
}

// ── Plan preview (live while dragging) ───────────────────────────────────────

function schedulePlanPreview(bbox) {
    if (app.planTimer) return;
    app.planTimer = window.setTimeout(() => {
        app.planTimer = 0;
        runPlanPreview(bbox);
    }, 120);
}

function runPlanPreview(bbox) {
    if (!app.ds || !bbox) return;
    const filter = currentFilter();
    try {
        const plan = app.ds.plan(bbox[0], bbox[1], bbox[2], bbox[3], filter, EXTRA_COLUMNS);
        $('filter-error').classList.add('hidden');
        const el = $('plan-preview');
        el.textContent = `plan: ${fmtInt(plan.rowGroups.length)} / ${fmtInt(plan.totalRowGroups)} row groups · ` +
            `~${fmtBytes(plan.estimatedBytes)} · ${fmtInt(plan.requests)} requests`;
        const maxRg = parseInt($('maxrg-input').value, 10) || 64;
        el.classList.toggle('plan-hot', plan.rowGroups.length > maxRg);
        app.strip.setSurvivors(plan.rowGroups);
        $('strip-counts').textContent =
            `${fmtInt(plan.rowGroups.length)} survivors / ${fmtInt(plan.totalRowGroups)} total`;
    } catch (e) {
        const err = parseWasmError(e);
        if (err.code === 'filter_expr') {
            const fe = $('filter-error');
            fe.textContent = err.message;
            fe.classList.remove('hidden');
        } else {
            $('plan-preview').textContent = `plan failed: ${err.message}`;
        }
    }
}

// ── Query execution + rendering ──────────────────────────────────────────────

/** Confidence 0.5 → red, 0.75 → amber, 1.0 → green (thin canvas strokes). */
function confidenceColor(conf) {
    const t = Math.max(0, Math.min(1, (Number(conf ?? 0.75) - 0.5) / 0.5));
    return `hsl(${Math.round(t * 140)}, 78%, 45%)`;
}

function featureStyle(feature) {
    const color = confidenceColor(feature.properties && feature.properties.confidence);
    return { color, weight: 1, fillColor: color, fillOpacity: 0.35 };
}

async function runQuery() {
    if (!app.ds || !app.bbox || app.busy) return;
    app.busy = true;
    hideBanner();
    setStatus('loading', 'querying…');
    $('query-btn').disabled = true;

    const [minX, minY, maxX, maxY] = app.bbox;
    const filter = currentFilter();
    const limit = Math.max(1, parseInt($('limit-input').value, 10) || 60000);
    const maxRg = Math.max(1, parseInt($('maxrg-input').value, 10) || 64);

    try {
        const r = await app.ds.query(minX, minY, maxX, maxY, filter, EXTRA_COLUMNS, limit, maxRg);

        // Map layer (canvas renderer; popups only under the handler cap).
        const data = JSON.parse(r.geojson);
        if (app.geoLayer) { app.geoLayer.remove(); app.geoLayer = null; }
        const small = data.features.length <= POPUP_LIMIT;
        app.geoLayer = L.geoJSON(data, {
            renderer: app.canvasRenderer,
            style: featureStyle,
            interactive: small,
            onEachFeature: small
                ? (f, layer) => {
                    const p = f.properties || {};
                    const conf = Number(p.confidence);
                    layer.bindPopup(
                        `area: ${fmtArea(Number(p.area_in_meters) || 0)}<br>` +
                        `confidence: ${Number.isFinite(conf) ? conf.toFixed(2) : '—'}`);
                }
                : undefined,
        }).addTo(app.map);

        // Strip: survivors that were scanned are now green.
        app.strip.setSurvivors(r.survivors);
        app.strip.markFetched(r.survivors);
        $('strip-counts').textContent =
            `${fmtInt(r.rowGroupsScanned)} fetched / ${fmtInt(r.rowGroupsTotal)} total`;

        // Result panel.
        $('result-panel').classList.remove('hidden');
        $('res-matched').textContent = fmtInt(r.matched);
        $('res-area').textContent = fmtArea(r.totalAreaM2);
        $('res-rowgroups').textContent = `${fmtInt(r.rowGroupsScanned)} / ${fmtInt(r.rowGroupsTotal)}`;
        $('res-requests').textContent = fmtInt(r.requestsThisQuery);
        $('res-bytes').textContent = fmtBytes(r.bytesFetchedThisQuery);
        $('res-elapsed').textContent = `${Math.round(r.elapsedMs)} ms`;

        // Honest over-cap banner.
        if (r.matched >= limit) {
            showBanner(`Row limit reached — showing the first ${fmtInt(limit)} matching buildings. ` +
                'Tighten the box or the filter for a complete answer.');
        }

        setStatus('ready', `${fmtInt(r.matched)} buildings`);
        updateBadges();
    } catch (e) {
        const err = parseWasmError(e);
        setStatus('error', 'query failed');
        if (err.code === 'too_broad' && err.detail) {
            showBanner(`Query too broad: ${fmtInt(err.detail.rowGroups)} row groups ` +
                `(~${fmtBytes(err.detail.estimatedBytes)}) exceed the ${fmtInt(parseInt($('maxrg-input').value, 10) || 64)} ` +
                'row-group cap. Draw a smaller box, add a filter, or raise "max row groups".', true);
        } else if (err.code === 'filter_expr') {
            const fe = $('filter-error');
            fe.textContent = err.message;
            fe.classList.remove('hidden');
        } else {
            showBanner(`Query failed [${err.code}]: ${err.message}`, true);
        }
    } finally {
        app.busy = false;
        refreshQueryButton();
    }
}

function refreshQueryButton() {
    $('query-btn').disabled = !(app.ds && app.bbox) || app.busy;
}

// ── Examples ─────────────────────────────────────────────────────────────────

async function loadExamples() {
    try {
        const resp = await fetch('./examples.json');
        const spec = await resp.json();
        const list = $('example-list');
        list.textContent = '';
        for (const ex of spec.examples) {
            const btn = document.createElement('button');
            btn.className = 'btn example-btn';
            btn.appendChild(document.createTextNode(String(ex.name)));
            const note = document.createElement('span');
            note.className = 'example-note';
            note.textContent = `${ex.filter || 'no filter'} — ${ex.note}`;
            btn.appendChild(note);
            btn.addEventListener('click', () => applyExample(ex));
            list.appendChild(btn);
        }
    } catch (_e) {
        $('example-list').textContent = 'examples unavailable';
    }
}

function applyExample(ex) {
    $('filter-input').value = ex.filter || '';
    $('filter-error').classList.add('hidden');
    const [minX, minY, maxX, maxY] = ex.bbox;
    app.map.setView([(minY + maxY) / 2, (minX + maxX) / 2], ex.zoom || 14);
    app.boxDraw.setBox(ex.bbox);       // fires onComplete → bbox + plan preview
}

// ── Wiring ───────────────────────────────────────────────────────────────────

function setBbox(bbox) {
    app.bbox = bbox;
    $('bbox-readout').textContent =
        `[${bbox.map((v) => v.toFixed(4)).join(', ')}]  (lon/lat)`;
    refreshQueryButton();
    runPlanPreview(bbox);
}

function initMap() {
    app.map = L.map('map', { zoomControl: true }).setView([36.2, 137.5], 5);
    L.tileLayer('https://tile.openstreetmap.org/{z}/{x}/{y}.png', {
        maxZoom: 19,
        attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors',
    }).addTo(app.map);
    app.canvasRenderer = L.canvas({ padding: 0.2 });

    app.boxDraw = new BoxDraw(app.map, {
        onChange: (bbox) => {
            app.bbox = bbox;
            schedulePlanPreview(bbox);
        },
        onComplete: (bbox) => setBbox(bbox),
        onArmChange: (armed) => {
            const btn = $('draw-btn');
            btn.classList.toggle('btn-active', armed);
            btn.textContent = armed ? '▭ Drag on the map…' : '▭ Draw query box';
        },
    });
}

async function main() {
    installFetchAccounting();
    initMap();
    app.strip = new RowGroupStrip($('rg-strip'));
    app.strip.draw();
    loadExamples();

    // Source: ?src=… overrides the live 5.9 GB VIDA file (offline sample path).
    const params = new URLSearchParams(window.location.search);
    const src = params.get('src') || LIVE_URL;
    $('url-input').value = src;
    $('badge-dataset').textContent = src === LIVE_URL ? 'dataset: 5.9 GB' : 'dataset: —';

    $('open-btn').addEventListener('click', () => openDataset($('url-input').value.trim()));
    $('draw-btn').addEventListener('click', () => app.boxDraw.toggle());
    $('query-btn').addEventListener('click', runQuery);
    $('banner-close').addEventListener('click', hideBanner);
    $('filter-input').addEventListener('input', () => {
        $('filter-error').classList.add('hidden');
        if (app.bbox) schedulePlanPreview(app.bbox);
    });
    $('maxrg-input').addEventListener('input', () => {
        if (app.bbox) schedulePlanPreview(app.bbox);
    });

    setStatus('loading', 'loading WASM…');
    try {
        await init();
    } catch (e) {
        setStatus('error', 'WASM failed to load');
        showBanner(`WebAssembly module failed to load: ${parseWasmError(e).message}`, true);
        return;
    }
    setStatus('idle', 'ready — open a dataset');
    $('open-btn').disabled = false;

    // Auto-open for the local sample path (small file; the 5.9 GB live file
    // is only ever opened by an explicit click).
    if (params.get('src') || params.get('autoopen') === '1') {
        openDataset(src);
    }
}

main();
