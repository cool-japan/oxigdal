/**
 * GeoVault — workstation.js
 *
 * The analysis side of the clean-room workstation: Leaflet map (no external
 * basemap — connect-src 'self' means the map shows exactly what you loaded),
 * GeoTIFF ingest via WasmCogViewer.openBytes, WasmTerrain relief analysis,
 * the WasmAnomaly workbench, measurement tools, and raster statistics.
 *
 * Every user action is appended to the tamper-evident session ledger via
 * vaultLog() from vault-ui.js. File identity is bound into the ledger as a
 * blake3 digest (fileDigestHex) at open time.
 */

import {
    WasmCogViewer,
    WasmTerrain,
    WasmAnomaly,
    WasmImageProcessor,
    fileDigestHex,
} from './pkg/oxigeo_wasm.js';
import { initVault, vaultLog, isSealed } from './vault-ui.js';

/* ------------------------------------------------------------------ */
/* State                                                               */
/* ------------------------------------------------------------------ */

const EARTH_RADIUS_M = 6371000;
const METERS_PER_DEG_LAT = 110540;
const METERS_PER_DEG_LON_EQ = 111320;

const app = {
    map: null,
    baseLayer: null,
    anomalyLayer: null,
    viewer: null,
    scene: null, // { name, width, height, elev, nodata, bounds, cellSize, elevMin, elevMax }
    canvas: null, // offscreen render target for the base layer
    terrain: {
        mode: 'raw',
        azimuth: 315,
        altitude: 45,
        zFactor: 1.0,
        rerenderTimer: null,
    },
    measure: {
        mode: null, // 'distance' | 'area' | null
        points: [],
        markers: [],
        shape: null,
    },
    moveLogTimer: null,
    busy: false,
};

/* ------------------------------------------------------------------ */
/* Small helpers                                                       */
/* ------------------------------------------------------------------ */

function $(id) {
    return document.getElementById(id);
}

function setHint(title, detail) {
    const hint = $('map-hint');
    if (!hint) return;
    if (!title) {
        hint.classList.add('hidden');
        return;
    }
    hint.classList.remove('hidden');
    hint.textContent = '';
    const t = document.createElement('div');
    t.className = 'map-hint-title';
    t.textContent = title;
    const d = document.createElement('div');
    d.textContent = detail || '';
    hint.appendChild(t);
    hint.appendChild(d);
}

/** Render `[[k, v], …]` pairs into a <dl class="kv"> and reveal its box. */
function renderKv(dlId, boxId, pairs) {
    const dl = $(dlId);
    if (!dl) return;
    dl.textContent = '';
    for (const [k, v] of pairs) {
        const dt = document.createElement('dt');
        dt.textContent = k;
        const dd = document.createElement('dd');
        dd.textContent = v;
        dl.appendChild(dt);
        dl.appendChild(dd);
    }
    const box = $(boxId);
    if (box) box.classList.remove('hidden');
}

function fmtMeters(m) {
    return m >= 1000 ? `${(m / 1000).toFixed(2)} km` : `${m.toFixed(1)} m`;
}

function fmtArea(m2) {
    return m2 >= 1e6 ? `${(m2 / 1e6).toFixed(3)} km²` : `${m2.toFixed(0)} m²`;
}

/* ------------------------------------------------------------------ */
/* Boot                                                                */
/* ------------------------------------------------------------------ */

async function boot() {
    try {
        await initVault();
    } catch (err) {
        setHint('Failed to start', String(err && err.message ? err.message : err));
        return;
    }

    setupMap();
    wireDatasetControls();
    wireTerrainControls();
    wireAnomalyControls();
    wireMeasureControls();
    wireStatsControls();
    document.addEventListener('geovault:sealed', onSealed);

    await loadExampleIndex();
}

function setupMap() {
    app.map = L.map('map', {
        center: [46.53, 63.49],
        zoom: 12,
        zoomControl: true,
        attributionControl: false,
        doubleClickZoom: false,
        // No basemap on purpose: the clean-room shows only what you loaded.
        maxZoom: 19,
        minZoom: 3,
    });
    L.control.scale({ imperial: false }).addTo(app.map);

    app.map.on('moveend', () => {
        if (!app.scene || isSealed()) return;
        clearTimeout(app.moveLogTimer);
        app.moveLogTimer = setTimeout(() => {
            const c = app.map.getCenter();
            vaultLog('view.move', {
                center: [Number(c.lat.toFixed(5)), Number(c.lng.toFixed(5))],
                zoom: app.map.getZoom(),
            });
        }, 600);
    });

    app.map.on('click', onMapClick);
    app.map.on('dblclick', onMapDoubleClick);
}

/* ------------------------------------------------------------------ */
/* Dataset loading                                                     */
/* ------------------------------------------------------------------ */

async function loadExampleIndex() {
    let data;
    try {
        const res = await fetch('./examples.json');
        data = await res.json();
    } catch {
        return; // card list stays empty; drag-drop still works
    }
    const host = $('dataset-list');
    if (!host || !data || !Array.isArray(data.datasets)) return;
    host.textContent = '';
    for (const ds of data.datasets) {
        const card = document.createElement('button');
        card.className = 'dataset-card';
        card.type = 'button';
        const name = document.createElement('div');
        name.className = 'card-name';
        name.textContent = ds.name;
        const desc = document.createElement('div');
        desc.className = 'card-desc';
        desc.textContent = ds.description || '';
        const tag = document.createElement('span');
        tag.className = 'card-tag';
        tag.textContent = 'SYNTHETIC · LOCAL';
        card.appendChild(name);
        card.appendChild(desc);
        card.appendChild(tag);
        card.addEventListener('click', () => loadExample(ds));
        host.appendChild(card);
    }
}

async function loadExample(ds) {
    if (app.busy || isSealed()) return;
    try {
        app.busy = true;
        setHint('Loading', `${ds.name}…`);
        const res = await fetch(ds.url);
        if (!res.ok) throw new Error(`HTTP ${res.status} for ${ds.url}`);
        const buffer = await res.arrayBuffer();
        await openScene(new Uint8Array(buffer), ds.name, 'sample');
    } catch (err) {
        setHint('Failed to load sample', String(err && err.message ? err.message : err));
    } finally {
        app.busy = false;
    }
}

function wireDatasetControls() {
    const dropZone = $('drop-zone');
    const fileInput = $('file-input');
    const mapEl = $('map');

    if (fileInput) {
        fileInput.addEventListener('change', (e) => {
            const file = e.target.files && e.target.files[0];
            if (file) handleLocalFile(file);
            e.target.value = '';
        });
    }
    for (const target of [dropZone, mapEl].filter(Boolean)) {
        target.addEventListener('dragover', (e) => {
            e.preventDefault();
            if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
            if (dropZone) dropZone.classList.add('drop-hover');
        });
        target.addEventListener('dragleave', () => {
            if (dropZone) dropZone.classList.remove('drop-hover');
        });
        target.addEventListener('drop', (e) => {
            e.preventDefault();
            if (dropZone) dropZone.classList.remove('drop-hover');
            const file = e.dataTransfer && e.dataTransfer.files && e.dataTransfer.files[0];
            if (file) handleLocalFile(file);
        });
    }
}

async function handleLocalFile(file) {
    if (app.busy || isSealed()) return;
    const name = file.name || 'local.tif';
    if (!/\.(tif|tiff)$/i.test(name)) {
        setHint('Unsupported file', 'Please choose a GeoTIFF (.tif / .tiff).');
        return;
    }
    try {
        app.busy = true;
        setHint('Decoding', `${name} — in-browser, nothing is uploaded…`);
        const buffer = await file.arrayBuffer();
        await openScene(new Uint8Array(buffer), name, 'local-file');
    } catch (err) {
        setHint('Failed to open file', String(err && err.message ? err.message : err));
    } finally {
        app.busy = false;
    }
}

/**
 * Shared ingest path for sample + dropped files: decode via openBytes,
 * bind the blake3 digest into the ledger, assemble the elevation grid,
 * and render the raw DEM.
 */
async function openScene(bytes, name, source) {
    const viewer = new WasmCogViewer();
    viewer.openBytes(bytes, name); // throws on invalid TIFF

    const digest = fileDigestHex(bytes);
    vaultLog('file.open', {
        name,
        size: bytes.length,
        blake3: digest,
        source,
    });

    const width = Number(viewer.width());
    const height = Number(viewer.height());
    const nodataInput = $('anomaly-nodata');
    const nodata = nodataInput ? Number(nodataInput.value) : NaN;

    const bounds = sceneBounds(viewer, width, height);
    const elev = await assembleElevation(viewer, width, height);

    // Valid-range scan (single pass): ignores NaN/±inf and the nodata value.
    let min = Infinity;
    let max = -Infinity;
    for (let i = 0; i < elev.length; i++) {
        const v = elev[i];
        if (!Number.isFinite(v) || v === nodata) continue;
        if (v < min) min = v;
        if (v > max) max = v;
    }
    if (min > max) {
        min = 0;
        max = 1;
    }

    app.viewer = viewer;
    app.scene = {
        name,
        width,
        height,
        elev,
        nodata,
        bounds,
        cellSize: cellSizeMeters(viewer, bounds),
        elevMin: min,
        elevMax: max,
        sizeBytes: bytes.length,
        digest,
    };
    app.canvas = document.createElement('canvas');
    app.canvas.width = width;
    app.canvas.height = height;

    clearAnomalyOverlay(false);
    clearMeasure(false);
    app.terrain.mode = 'raw';
    updateTerrainButtons();
    renderScene();
    app.map.fitBounds(bounds);
    setHint(null);

    renderKv('dataset-info', 'dataset-info-box', [
        ['name', name],
        ['size', `${bytes.length.toLocaleString()} B`],
        ['dims', `${width}×${height}·b${viewer.band_count()}`],
        ['epsg', String(viewer.epsg_code() ?? 'n/a')],
        ['overviews', String(viewer.overview_count())],
        ['elev', `${min.toFixed(1)}…${max.toFixed(1)} m`],
        ['blake3', `${digest.slice(0, 16)}…`],
    ]);
}

/** Geographic bounds from the geotransform (EPSG:4326 rasters). */
function sceneBounds(viewer, width, height) {
    const psx = viewer.pixel_scale_x();
    const psy = viewer.pixel_scale_y();
    const gx = viewer.tiepoint_geo_x();
    const gy = viewer.tiepoint_geo_y();
    if (psx === undefined || psy === undefined || gx === undefined || gy === undefined) {
        // Ungeoreferenced raster: place it on a unit-degree box at the equator.
        return [
            [0, 0],
            [height / 3600, width / 3600],
        ];
    }
    const west = gx;
    const north = gy;
    const east = gx + width * psx;
    const south = gy - height * Math.abs(psy);
    return [
        [south, west],
        [north, east],
    ];
}

/** Nominal ground cell size in meters at the scene's center latitude. */
function cellSizeMeters(viewer, bounds) {
    const psx = viewer.pixel_scale_x();
    if (psx === undefined) return 30;
    const midLat = (bounds[0][0] + bounds[1][0]) / 2;
    const mx = psx * METERS_PER_DEG_LON_EQ * Math.cos((midLat * Math.PI) / 180);
    const my = Math.abs(viewer.pixel_scale_y() ?? psx) * METERS_PER_DEG_LAT;
    return (mx + my) / 2;
}

/** Read every level-0 tile as f32 elevation and stitch the full grid. */
async function assembleElevation(viewer, width, height) {
    const tw = viewer.tile_width();
    const th = viewer.tile_height();
    const tilesX = Math.ceil(width / tw);
    const tilesY = Math.ceil(height / th);
    const out = new Float32Array(width * height);
    for (let ty = 0; ty < tilesY; ty++) {
        for (let tx = 0; tx < tilesX; tx++) {
            const tile = await viewer.readTileElevation(0, tx, ty);
            const copyW = Math.min(tw, width - tx * tw);
            const copyH = Math.min(th, height - ty * th);
            for (let row = 0; row < copyH; row++) {
                const src = row * tw;
                const dst = (ty * th + row) * width + tx * tw;
                out.set(tile.subarray(src, src + copyW), dst);
            }
        }
    }
    return out;
}

/* ------------------------------------------------------------------ */
/* Rendering                                                           */
/* ------------------------------------------------------------------ */

/** Draw ImageData to the scene canvas and swap it into the base overlay. */
function presentImageData(imageData) {
    const ctx = app.canvas.getContext('2d');
    ctx.putImageData(imageData, 0, 0);
    const dataUrl = app.canvas.toDataURL('image/png');
    if (app.baseLayer) {
        app.baseLayer.setUrl(dataUrl);
    } else {
        app.baseLayer = L.imageOverlay(dataUrl, app.scene.bounds, {
            opacity: 1,
            interactive: false,
            className: 'leaflet-overlay-crisp',
        }).addTo(app.map);
    }
    if (app.anomalyLayer) app.anomalyLayer.bringToFront();
}

/** Grayscale RGBA from a value grid with a linear lo..hi stretch. */
function grayImageData(values, width, height, lo, hi, nodata) {
    const rgba = new Uint8ClampedArray(width * height * 4);
    const range = hi > lo ? hi - lo : 1;
    for (let i = 0; i < values.length; i++) {
        const v = values[i];
        const o = i * 4;
        if (!Number.isFinite(v) || v === nodata) {
            rgba[o + 3] = 0;
            continue;
        }
        const g = Math.max(0, Math.min(255, Math.round(((v - lo) / range) * 255)));
        rgba[o] = g;
        rgba[o + 1] = g;
        rgba[o + 2] = g;
        rgba[o + 3] = 255;
    }
    return new ImageData(rgba, width, height);
}

/** Grayscale RGBA straight from 0..255 bytes (hillshade output). */
function bytesImageData(bytes, width, height) {
    const rgba = new Uint8ClampedArray(width * height * 4);
    for (let i = 0; i < bytes.length; i++) {
        const o = i * 4;
        rgba[o] = bytes[i];
        rgba[o + 1] = bytes[i];
        rgba[o + 2] = bytes[i];
        rgba[o + 3] = 255;
    }
    return new ImageData(rgba, width, height);
}

/** Re-render the base layer for the current terrain mode. */
function renderScene() {
    const s = app.scene;
    if (!s) return;
    const t = app.terrain;
    switch (t.mode) {
        case 'hillshade': {
            const shade = WasmTerrain.hillshade(
                s.elev, s.width, s.height, s.cellSize, t.azimuth, t.altitude, t.zFactor
            );
            presentImageData(bytesImageData(shade, s.width, s.height));
            break;
        }
        case 'multidir': {
            const shade = WasmTerrain.hillshadeMultidirectional(
                s.elev, s.width, s.height, s.cellSize, t.altitude, t.zFactor
            );
            presentImageData(bytesImageData(shade, s.width, s.height));
            break;
        }
        case 'slope': {
            const slope = WasmTerrain.slope(s.elev, s.width, s.height, s.cellSize, t.zFactor);
            presentImageData(grayImageData(slope, s.width, s.height, 0, 60, NaN));
            break;
        }
        case 'relief': {
            const img = WasmTerrain.colorReliefShaded(
                s.elev, s.width, s.height, s.cellSize, 'terrain',
                s.elevMin, s.elevMax, t.altitude, t.zFactor
            );
            presentImageData(img);
            break;
        }
        default:
            presentImageData(
                grayImageData(s.elev, s.width, s.height, s.elevMin, s.elevMax, s.nodata)
            );
    }
}

/* ------------------------------------------------------------------ */
/* Terrain controls                                                    */
/* ------------------------------------------------------------------ */

const TERRAIN_BUTTONS = {
    'terrain-raw': 'raw',
    'terrain-hillshade': 'hillshade',
    'terrain-multidir': 'multidir',
    'terrain-slope': 'slope',
    'terrain-relief': 'relief',
};

const TERRAIN_OPS = {
    raw: 'terrain.clear',
    hillshade: 'terrain.hillshade',
    multidir: 'terrain.hillshade_multidirectional',
    slope: 'terrain.slope',
    relief: 'terrain.color_relief',
};

function terrainParams(mode) {
    const s = app.scene;
    const t = app.terrain;
    const base = {
        cell_size_m: Number(s.cellSize.toFixed(2)),
        z_factor: t.zFactor,
    };
    if (mode === 'hillshade') {
        return { ...base, azimuth: t.azimuth, altitude: t.altitude };
    }
    if (mode === 'multidir') return { ...base, altitude: t.altitude };
    if (mode === 'relief') {
        return {
            ...base,
            palette: 'terrain',
            min_m: Number(s.elevMin.toFixed(1)),
            max_m: Number(s.elevMax.toFixed(1)),
            altitude: t.altitude,
        };
    }
    if (mode === 'raw') return {};
    return base;
}

function setTerrainMode(mode) {
    if (!app.scene || isSealed()) return;
    app.terrain.mode = mode;
    updateTerrainButtons();
    renderScene();
    vaultLog(TERRAIN_OPS[mode], terrainParams(mode));
}

function updateTerrainButtons() {
    for (const [id, mode] of Object.entries(TERRAIN_BUTTONS)) {
        const btn = $(id);
        if (btn) btn.classList.toggle('btn-active', app.terrain.mode === mode);
    }
}

function scheduleTerrainRerender() {
    clearTimeout(app.terrain.rerenderTimer);
    app.terrain.rerenderTimer = setTimeout(() => {
        if (app.scene && app.terrain.mode !== 'raw') renderScene();
    }, 120);
}

function wireTerrainControls() {
    for (const [id, mode] of Object.entries(TERRAIN_BUTTONS)) {
        const btn = $(id);
        if (btn) btn.addEventListener('click', () => setTerrainMode(mode));
    }

    const sliders = [
        ['terrain-azimuth', 'azimuth', (v) => `${v}°`, parseInt],
        ['terrain-altitude', 'altitude', (v) => `${v}°`, parseInt],
        ['terrain-zfactor', 'zFactor', (v) => v.toFixed(1), parseFloat],
    ];
    for (const [id, key, fmt, parse] of sliders) {
        const el = $(id);
        if (!el) continue;
        // Live re-render while dragging; one ledger entry on commit.
        el.addEventListener('input', (e) => {
            app.terrain[key] = parse(e.target.value, 10);
            const label = $(`${id}-value`);
            if (label) label.textContent = fmt(app.terrain[key]);
            scheduleTerrainRerender();
        });
        el.addEventListener('change', () => {
            if (!app.scene || app.terrain.mode === 'raw' || isSealed()) return;
            vaultLog(
                TERRAIN_OPS[app.terrain.mode],
                terrainParams(app.terrain.mode)
            );
        });
    }
}

/* ------------------------------------------------------------------ */
/* Anomaly workbench                                                   */
/* ------------------------------------------------------------------ */

const ANOMALY_DEFAULT_THRESHOLD = {
    zscore: 3.0,
    iqr: 1.5,
    modified_zscore: 3.5,
};

function runAnomaly() {
    const s = app.scene;
    if (!s || isSealed()) return;
    const method = $('anomaly-method').value;
    const threshold = Number($('anomaly-threshold').value);
    const nodata = Number($('anomaly-nodata').value);
    const sentinel = Number.isFinite(nodata) ? nodata : NaN;

    let mask;
    switch (method) {
        case 'iqr':
            mask = WasmAnomaly.iqrMask(s.elev, threshold, sentinel);
            break;
        case 'modified_zscore':
            mask = WasmAnomaly.modifiedZscoreMask(s.elev, threshold, sentinel);
            break;
        default:
            mask = WasmAnomaly.zscoreMask(s.elev, threshold, sentinel);
    }
    const summaryJson = WasmAnomaly.summaryJson(method, s.elev, threshold, sentinel);
    let summary;
    try {
        summary = JSON.parse(summaryJson);
    } catch {
        summary = { error: 'unparseable summary' };
    }
    if (summary.error) {
        renderKv('anomaly-result', 'anomaly-result-box', [['error', summary.error]]);
        return;
    }

    // The summary IS the ledger record (plus the sentinel used).
    vaultLog('anomaly.detect', { ...summary, nodata: sentinel });

    const img = WasmAnomaly.maskToImageData(mask, s.width, s.height, 224, 82, 82, 235);
    const cnv = document.createElement('canvas');
    cnv.width = s.width;
    cnv.height = s.height;
    cnv.getContext('2d').putImageData(img, 0, 0);
    const url = cnv.toDataURL('image/png');
    if (app.anomalyLayer) {
        app.anomalyLayer.setUrl(url);
    } else {
        app.anomalyLayer = L.imageOverlay(url, s.bounds, {
            opacity: 1,
            interactive: false,
            className: 'leaflet-overlay-crisp',
        }).addTo(app.map);
    }
    app.anomalyLayer.bringToFront();

    const boundsTxt =
        summary.lower_bound !== null && summary.upper_bound !== null
            ? `${summary.lower_bound.toFixed(1)}…${summary.upper_bound.toFixed(1)} m`
            : 'n/a';
    renderKv('anomaly-result', 'anomaly-result-box', [
        ['method', summary.method],
        ['threshold', String(summary.threshold)],
        ['flagged', `${summary.anomaly_count} px`],
        ['share', `${summary.anomaly_pct.toFixed(3)} %`],
        ['normal range', boundsTxt],
        ['valid px', String(summary.valid_count)],
    ]);
}

function clearAnomalyOverlay(log = true) {
    if (app.anomalyLayer) {
        app.map.removeLayer(app.anomalyLayer);
        app.anomalyLayer = null;
        if (log && !isSealed()) vaultLog('anomaly.clear', {});
    }
    const box = $('anomaly-result-box');
    if (box) box.classList.add('hidden');
}

function wireAnomalyControls() {
    const method = $('anomaly-method');
    if (method) {
        method.addEventListener('change', (e) => {
            const th = ANOMALY_DEFAULT_THRESHOLD[e.target.value];
            if (th !== undefined) $('anomaly-threshold').value = String(th);
        });
    }
    const run = $('anomaly-run');
    if (run) run.addEventListener('click', runAnomaly);
    const clear = $('anomaly-clear');
    if (clear) clear.addEventListener('click', () => clearAnomalyOverlay(true));
}

/* ------------------------------------------------------------------ */
/* Measure (haversine distances, spherical polygon areas)              */
/* ------------------------------------------------------------------ */

function haversineMeters(a, b) {
    const toRad = (d) => (d * Math.PI) / 180;
    const dLat = toRad(b.lat - a.lat);
    const dLon = toRad(b.lng - a.lng);
    const s =
        Math.sin(dLat / 2) ** 2 +
        Math.cos(toRad(a.lat)) * Math.cos(toRad(b.lat)) * Math.sin(dLon / 2) ** 2;
    return 2 * EARTH_RADIUS_M * Math.asin(Math.sqrt(s));
}

/** Spherical shoelace (same formula Leaflet.GeometryUtil uses). */
function sphericalAreaSqMeters(points) {
    if (points.length < 3) return 0;
    const toRad = (d) => (d * Math.PI) / 180;
    let sum = 0;
    for (let i = 0; i < points.length; i++) {
        const p1 = points[i];
        const p2 = points[(i + 1) % points.length];
        sum +=
            toRad(p2.lng - p1.lng) *
            (2 + Math.sin(toRad(p1.lat)) + Math.sin(toRad(p2.lat)));
    }
    return Math.abs((sum * EARTH_RADIUS_M * EARTH_RADIUS_M) / 2);
}

function setMeasureMode(mode) {
    if (!app.scene || isSealed()) return;
    clearMeasureShapes();
    app.measure.mode = mode;
    app.measure.points = [];
    $('measure-distance').classList.toggle('btn-active', mode === 'distance');
    $('measure-area').classList.toggle('btn-active', mode === 'area');
}

function onMapClick(e) {
    const m = app.measure;
    if (!m.mode || isSealed()) return;
    const last = m.points[m.points.length - 1];
    if (last && Math.abs(last.lat - e.latlng.lat) < 1e-9 && Math.abs(last.lng - e.latlng.lng) < 1e-9) {
        return; // dblclick fires a duplicate click at the same spot
    }
    m.points.push(e.latlng);
    m.markers.push(
        L.circleMarker(e.latlng, {
            radius: 4,
            color: '#5b8dbe',
            weight: 2,
            fillOpacity: 0.8,
        }).addTo(app.map)
    );
    redrawMeasureShape(false);
}

function onMapDoubleClick() {
    const m = app.measure;
    if (!m.mode || isSealed()) return;
    const need = m.mode === 'area' ? 3 : 2;
    if (m.points.length < need) return;
    redrawMeasureShape(true);

    if (m.mode === 'distance') {
        let total = 0;
        for (let i = 1; i < m.points.length; i++) {
            total += haversineMeters(m.points[i - 1], m.points[i]);
        }
        vaultLog('measure.distance', {
            vertices: m.points.length,
            meters: Number(total.toFixed(1)),
        });
        renderKv('measure-result', 'measure-result-box', [
            ['type', 'distance'],
            ['vertices', String(m.points.length)],
            ['length', fmtMeters(total)],
        ]);
    } else {
        const area = sphericalAreaSqMeters(m.points);
        vaultLog('measure.area', {
            vertices: m.points.length,
            sq_meters: Number(area.toFixed(1)),
        });
        renderKv('measure-result', 'measure-result-box', [
            ['type', 'area'],
            ['vertices', String(m.points.length)],
            ['area', fmtArea(area)],
        ]);
    }
    m.mode = null;
    m.points = [];
    $('measure-distance').classList.remove('btn-active');
    $('measure-area').classList.remove('btn-active');
}

function redrawMeasureShape(closed) {
    const m = app.measure;
    if (m.shape) {
        app.map.removeLayer(m.shape);
        m.shape = null;
    }
    if (m.points.length < 2) return;
    const style = { color: '#7fabd6', weight: 2, dashArray: closed ? null : '4 4' };
    m.shape =
        m.mode === 'area' && m.points.length >= 3
            ? L.polygon(m.points, { ...style, fillOpacity: 0.08 })
            : L.polyline(m.points, style);
    m.shape.addTo(app.map);
}

function clearMeasureShapes() {
    const m = app.measure;
    for (const marker of m.markers) app.map.removeLayer(marker);
    m.markers = [];
    if (m.shape) {
        app.map.removeLayer(m.shape);
        m.shape = null;
    }
}

function clearMeasure(log = true) {
    const m = app.measure;
    const hadSomething = m.markers.length > 0 || m.shape !== null;
    clearMeasureShapes();
    m.mode = null;
    m.points = [];
    const box = $('measure-result-box');
    if (box) box.classList.add('hidden');
    const bd = $('measure-distance');
    const ba = $('measure-area');
    if (bd) bd.classList.remove('btn-active');
    if (ba) ba.classList.remove('btn-active');
    if (log && hadSomething && !isSealed()) vaultLog('measure.clear', {});
}

function wireMeasureControls() {
    $('measure-distance').addEventListener('click', () => setMeasureMode('distance'));
    $('measure-area').addEventListener('click', () => setMeasureMode('area'));
    $('measure-clear').addEventListener('click', () => clearMeasure(true));
}

/* ------------------------------------------------------------------ */
/* Statistics + histogram                                              */
/* ------------------------------------------------------------------ */

function runStats() {
    const s = app.scene;
    if (!s || isSealed()) return;

    // Stats over the CURRENT rendering (what the analyst is looking at),
    // via the wasm image kernels; elevation range comes from the grid.
    const ctx = app.canvas.getContext('2d');
    const img = ctx.getImageData(0, 0, s.width, s.height);
    const rgba = new Uint8Array(img.data.buffer, 0, img.data.length);

    let stats;
    let hist;
    try {
        stats = JSON.parse(WasmImageProcessor.computeStats(rgba, s.width, s.height));
        hist = JSON.parse(WasmImageProcessor.computeHistogram(rgba, s.width, s.height));
    } catch (err) {
        renderKv('stats-result', 'stats-result-box', [
            ['error', String(err && err.message ? err.message : err)],
        ]);
        return;
    }

    const lum = hist.luminance;
    vaultLog('raster.stats', {
        view: app.terrain.mode,
        lum_mean: Number(lum.mean.toFixed(2)),
        lum_std: Number(lum.std_dev.toFixed(2)),
        lum_min: lum.min,
        lum_max: lum.max,
        elev_min_m: Number(s.elevMin.toFixed(1)),
        elev_max_m: Number(s.elevMax.toFixed(1)),
        total_px: hist.total_pixels,
    });

    renderKv('stats-result', 'stats-result-box', [
        ['view', app.terrain.mode],
        ['lum mean', lum.mean.toFixed(2)],
        ['lum σ', lum.std_dev.toFixed(2)],
        ['lum range', `${lum.min}…${lum.max}`],
        ['elev range', `${s.elevMin.toFixed(1)}…${s.elevMax.toFixed(1)} m`],
        ['pixels', String(hist.total_pixels)],
    ]);
    drawHistogram(lum.bins);
}

function drawHistogram(bins) {
    const canvas = $('histogram-canvas');
    if (!canvas) return;
    canvas.classList.remove('hidden');
    const ctx = canvas.getContext('2d');
    const { width, height } = canvas;
    ctx.clearRect(0, 0, width, height);
    const peak = Math.max(1, ...bins);
    const barW = width / bins.length;
    ctx.fillStyle = '#5b8dbe';
    for (let i = 0; i < bins.length; i++) {
        const h = Math.round((bins[i] / peak) * (height - 6));
        if (h > 0) ctx.fillRect(i * barW, height - h, Math.max(1, barW - 0.5), h);
    }
    ctx.fillStyle = '#2e3948';
    ctx.fillRect(0, height - 1, width, 1);
}

function wireStatsControls() {
    $('stats-run').addEventListener('click', runStats);
}

/* ------------------------------------------------------------------ */
/* Sealed state                                                        */
/* ------------------------------------------------------------------ */

function onSealed() {
    // The ledger is immutable; freeze every state-changing control. The map
    // stays browsable (pan/zoom no longer logs — the record is closed).
    const selectors = [
        '.tool-btn',
        '.dataset-card',
        '#file-input',
        '#anomaly-method',
        '#anomaly-threshold',
        '#anomaly-nodata',
        '#terrain-azimuth',
        '#terrain-altitude',
        '#terrain-zfactor',
    ];
    for (const sel of selectors) {
        for (const el of document.querySelectorAll(sel)) el.disabled = true;
    }
    app.measure.mode = null;
}

/* ------------------------------------------------------------------ */

boot();
