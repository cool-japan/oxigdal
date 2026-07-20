/**
 * GeoSentinel — application bootstrap and orchestration.
 *
 * In-browser Sentinel-2 change detection on the OxiGeo WASM stack:
 * STAC pair search, COG range reads, NDVI differencing, polygonization and
 * geodesic areas all execute client-side. The only network traffic is the
 * Earth Search catalog POSTs and sentinel-cogs HTTP range reads — counted
 * live in the "imagery fetched" badge. Nothing is ever uploaded, and no
 * server of ours learns which place is being watched.
 */

import init, { GeoSentinel, WasmStacClient, version } from './pkg/oxigeo_wasm.js';
import { SentinelMap } from './map.js';
import {
    friendlyError,
    runDetection,
    rerunWithAlternate,
} from './pipeline.js';
import * as ui from './ui.js';

// ── Application state ────────────────────────────────────────────────────

const app = {
    stac: null,        // WasmStacClient
    sentinel: null,    // GeoSentinel (two persistent scene slots)
    map: null,         // SentinelMap

    aoiBbox: null,     // [w, s, e, n] chosen by the user
    running: false,

    // Last successful run (drives export, alternates and layer controls).
    lastParams: null,
    lastDetection: null,
    currentA: null,
    currentB: null,
    candidatesA: [],   // primary + alternates for slot A (same MGRS grid)
    candidatesB: [],

    // Honest accounting: live sum of Content-Length over imagery/catalog
    // fetches. Uploaded bytes are literally always zero.
    bytesFetched: 0,
};

// ── Network byte accounting ──────────────────────────────────────────────

/**
 * Wrap `window.fetch` to tally imagery bytes (Content-Length only — the
 * body is never touched, so the WASM reader's consumption is unaffected).
 *
 * Excluded from the tally:
 *   - the app's own assets (pkg/, vendor/, *.js, *.css, *.wasm, *.json),
 *   - OpenStreetMap base tiles (tile.openstreetmap.org) — they are base
 *     cartography, not the satellite imagery this badge accounts for,
 *   - HEAD probes: the COG reader HEADs each file once for its size, and a
 *     HEAD's Content-Length is the full object size (hundreds of MB that
 *     are never transferred) — counting it would wreck the honest tally.
 */
function installFetchAccounting() {
    if (typeof window === 'undefined' || window.__oxigeoFetchWrapped) {
        return;
    }
    const originalFetch = window.fetch.bind(window);
    window.fetch = async (input, init) => {
        const response = await originalFetch(input, init);
        try {
            let url = '';
            let method = 'GET';
            if (typeof input === 'string') {
                url = input;
            } else if (input) {
                url = input.url || input.href || String(input);
                if (input.method) {
                    method = input.method;
                }
            }
            if (init && init.method) {
                method = init.method;
            }
            if (method.toUpperCase() !== 'HEAD' && !isExcludedFromAccounting(url)) {
                const len = response.headers.get('content-length');
                if (len) {
                    app.bytesFetched += parseInt(len, 10) || 0;
                    ui.updateNetworkBadges(app.bytesFetched);
                }
            }
        } catch (_e) {
            // Header inspection must never break the actual fetch.
        }
        return response;
    };
    window.__oxigeoFetchWrapped = true;
}

/** True for app assets and OSM base tiles (excluded from the imagery tally). */
function isExcludedFromAccounting(url) {
    if (!url) {
        return true;
    }
    if (url.includes('tile.openstreetmap.org')) {
        return true;
    }
    return /\/(pkg|vendor)\//.test(url) ||
        url.endsWith('.wasm') ||
        url.endsWith('.js') ||
        url.endsWith('.css') ||
        url.endsWith('.json');
}

// ── AOI selection ────────────────────────────────────────────────────────

function setAoi(bbox) {
    app.aoiBbox = bbox;
    app.map.setAoi(bbox);
    ui.setAoiLabel(bbox);
}

function onAoiDrawn(bbox) {
    setAoi(bbox);
    ui.els.aoiDrawBtn.classList.remove('btn-active');
    ui.setStatus('ready', 'Area selected');
}

function wireAoiButtons() {
    ui.els.aoiDrawBtn.addEventListener('click', () => {
        const drawing = app.map.toggleAoiDraw();
        ui.els.aoiDrawBtn.classList.toggle('btn-active', drawing);
        ui.setStatus(drawing ? 'loading' : 'ready',
            drawing ? 'Drag a rectangle on the map' : 'Ready');
    });
    ui.els.aoiViewBtn.addEventListener('click', () => {
        app.map.cancelAoiDraw();
        ui.els.aoiDrawBtn.classList.remove('btn-active');
        setAoi(app.map.currentViewBbox());
        ui.setStatus('ready', 'Area selected');
    });
}

// ── Detection runs ───────────────────────────────────────────────────────

async function run() {
    if (app.running) {
        return;
    }
    if (!app.aoiBbox) {
        ui.showError('Please select an area first — draw a rectangle or use the current view.');
        return;
    }
    const params = ui.readParams(app.aoiBbox);
    if (!params.dateA || !params.dateB) {
        ui.showError('Please pick both dates.');
        return;
    }

    app.running = true;
    ui.setRunning(true);
    ui.setStatus('loading', 'Analyzing…');
    try {
        const { pairResult, detection } = await runDetection(
            app.stac, app.sentinel, params, ui.setProgress);

        app.lastParams = params;
        app.currentA = pairResult.pair.a;
        app.currentB = pairResult.pair.b;
        app.candidatesA = [pairResult.pair.a, ...pairResult.alternatesA];
        app.candidatesB = [pairResult.pair.b, ...pairResult.alternatesB];

        applyDetection(detection);
        renderAlternateDropdowns();
        ui.setStatus('ready', 'Done');
    } catch (err) {
        ui.showError(friendlyError(err));
    } finally {
        app.running = false;
        ui.setRunning(false);
    }
}

/** Push one detection result onto the map and into the results panel. */
function applyDetection(detection) {
    const { result, overlay, diffUrl, trueColorUrlA, trueColorUrlB } = detection;
    app.lastDetection = detection;

    app.map.clearResults();
    if (trueColorUrlA || trueColorUrlB) {
        app.map.setTrueColor(trueColorUrlA, trueColorUrlB, result.boundsWgs84);
    }
    app.map.setDiffOverlay(diffUrl, overlay.boundsWgs84);
    app.map.setChangePolygons(result.fc);
    app.map.fitBbox(result.boundsWgs84);

    ui.renderResults(result, { a: app.currentA, b: app.currentB });
    ui.showLayerBar(
        (app.currentA.datetime || '').slice(0, 10) || 'A',
        (app.currentB.datetime || '').slice(0, 10) || 'B');
    app.map.setCrossfade(0);
    app.map.showDiff(false);
}

function renderAlternateDropdowns() {
    ui.renderAlternates(
        ui.els.altA, app.currentA,
        app.candidatesA.filter((c) => c.id !== app.currentA.id),
        (alt) => swapScene(0, alt));
    ui.renderAlternates(
        ui.els.altB, app.currentB,
        app.candidatesB.filter((c) => c.id !== app.currentB.id),
        (alt) => swapScene(1, alt));
}

/** Reload one slot with an alternate same-grid scene and re-detect. */
async function swapScene(slot, candidate) {
    if (app.running || !app.lastParams) {
        return;
    }
    app.running = true;
    ui.setRunning(true);
    ui.setStatus('loading', 'Swapping scene…');
    try {
        const detection = await rerunWithAlternate(
            app.sentinel, slot, candidate, app.lastParams,
            { a: app.currentA, b: app.currentB }, ui.setProgress);
        if (slot === 0) {
            app.currentA = candidate;
        } else {
            app.currentB = candidate;
        }
        applyDetection(detection);
        renderAlternateDropdowns();
        ui.setStatus('ready', 'Done');
    } catch (err) {
        ui.showError(friendlyError(err));
        renderAlternateDropdowns(); // Reset the dropdown to the active scene.
    } finally {
        app.running = false;
        ui.setRunning(false);
    }
}

// ── Export ───────────────────────────────────────────────────────────────

function exportGeoJson() {
    if (!app.lastDetection) {
        return;
    }
    const fc = app.lastDetection.result.fc;
    const blob = new Blob([JSON.stringify(fc)], { type: 'application/geo+json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    const dateA = (app.currentA?.datetime || 'a').slice(0, 10);
    const dateB = (app.currentB?.datetime || 'b').slice(0, 10);
    a.href = url;
    a.download = `geosentinel-changes-${dateA}-to-${dateB}.geojson`;
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
}

// ── Layer controls ───────────────────────────────────────────────────────

function wireLayerBar() {
    ui.els.crossfade.addEventListener('input', () => {
        app.map.setCrossfade(parseInt(ui.els.crossfade.value, 10) / 100);
    });
    ui.els.changeOpacity.addEventListener('input', () => {
        app.map.setChangeOpacity(parseInt(ui.els.changeOpacity.value, 10) / 100);
    });
    ui.els.diffToggle.addEventListener('change', () => {
        app.map.showDiff(ui.els.diffToggle.checked);
    });
}

// ── Examples ─────────────────────────────────────────────────────────────

async function loadExamples() {
    try {
        const response = await fetch('./examples.json');
        if (!response.ok) {
            return;
        }
        const data = await response.json();
        ui.renderExamples(data.examples || [], onExamplePicked);
    } catch (_e) {
        // Examples are sugar; the app works without them.
    }
}

function onExamplePicked(example) {
    if (app.running) {
        return;
    }
    ui.applyPreset(example);
    setAoi(example.bbox);
    app.map.fitBbox(example.bbox);
    run();
}

// ── Bootstrap ────────────────────────────────────────────────────────────

async function initializeApp() {
    ui.initUi();
    installFetchAccounting();
    ui.updateNetworkBadges(0);

    app.map = new SentinelMap('map-container', onAoiDrawn);

    wireAoiButtons();
    wireLayerBar();
    ui.els.runBtn.addEventListener('click', run);
    ui.els.exportBtn.addEventListener('click', exportGeoJson);

    ui.showLoading('Initializing WebAssembly…');
    try {
        await init();
        app.stac = new WasmStacClient();
        app.sentinel = new GeoSentinel();
        ui.els.versionBadge.textContent = `v${version()} · WASM`;
        ui.setStatus('ready', 'Ready');
    } catch (err) {
        ui.els.versionBadge.textContent = 'WASM failed';
        ui.showError(`WebAssembly initialization failed: ${friendlyError(err)}`);
        ui.setStatus('error', 'Init failed');
        return;
    } finally {
        ui.hideLoading();
    }

    await loadExamples();
}

window.addEventListener('DOMContentLoaded', initializeApp);
