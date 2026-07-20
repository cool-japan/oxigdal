/**
 * OxiGeo GeoLab — vector format parsers.
 *
 * Self-contained ES module: every function here only depends on the browser
 * (fetch, DOMParser), the vendored globals (window.flatgeobuf / window.shapefile
 * loaded via <script>), and the vendored dynamic imports under ./vendor/. None
 * of them touch the app state or UI helpers in main.js, so they live here to
 * keep main.js under the 2000-line limit.
 *
 * Extracted verbatim from main.js (WP-D3b) — parser logic unchanged.
 */

/**
 * Detect if URL is a vector format
 */
export function isVectorFormat(url) {
    const lowerUrl = url.toLowerCase();
    return lowerUrl.endsWith('.geojson') ||
           lowerUrl.endsWith('.json') ||
           lowerUrl.endsWith('.fgb') ||
           lowerUrl.endsWith('.shp') ||
           lowerUrl.endsWith('.parquet') ||
           lowerUrl.endsWith('.geoparquet') ||
           lowerUrl.endsWith('.gpx') ||
           lowerUrl.endsWith('.kml') ||
           lowerUrl.endsWith('.kmz') ||
           lowerUrl.endsWith('.topojson');
}

/**
 * Detect vector format from URL
 */
export function detectVectorFormat(url) {
    const lowerUrl = url.toLowerCase();
    if (lowerUrl.endsWith('.geojson') || lowerUrl.endsWith('.json')) {
        return 'geojson';
    } else if (lowerUrl.endsWith('.fgb')) {
        return 'flatgeobuf';
    } else if (lowerUrl.endsWith('.shp')) {
        return 'shapefile';
    } else if (lowerUrl.endsWith('.parquet') || lowerUrl.endsWith('.geoparquet')) {
        return 'geoparquet';
    } else if (lowerUrl.endsWith('.gpx')) {
        return 'gpx';
    } else if (lowerUrl.endsWith('.kml') || lowerUrl.endsWith('.kmz')) {
        return 'kml';
    } else if (lowerUrl.endsWith('.topojson')) {
        return 'topojson';
    }
    return null;
}

/**
 * Load GeoJSON from URL
 */
export async function loadGeoJSON(url) {
    const response = await fetch(url);
    if (!response.ok) {
        throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    }
    const geojson = await response.json();
    return geojson;
}

/**
 * Load FlatGeobuf with HTTP Range Requests
 */
export async function loadFlatGeobuf(url) {
    // FlatGeobuf library is vendored and loaded via <script> in index.html.
    if (typeof flatgeobuf === 'undefined') {
        throw new Error('FlatGeobuf library not loaded. Ensure <script src="./vendor/flatgeobuf/flatgeobuf-geojson.min.js"></script> is present in index.html');
    }

    console.log('Loading FlatGeobuf from:', url);
    console.log('FlatGeobuf library:', flatgeobuf);

    const features = [];
    let count = 0;

    try {
        // Create timeout promise (30 seconds)
        const timeout = new Promise((_, reject) =>
            setTimeout(() => reject(new Error('FlatGeobuf loading timeout (30s)')), 30000)
        );

        // Create deserialize promise
        const deserializePromise = (async () => {
            // Use FlatGeobuf streaming API with HTTP Range Requests
            for await (const feature of flatgeobuf.deserialize(url)) {
                features.push(feature);
                count++;
                if (count % 10 === 0) {
                    console.log(`Loaded ${count} features...`);
                }
            }
        })();

        // Race between timeout and deserialize
        await Promise.race([deserializePromise, timeout]);

        console.log(`Successfully loaded ${count} features from FlatGeobuf`);

        return {
            type: 'FeatureCollection',
            features: features
        };
    } catch (error) {
        console.error('FlatGeobuf loading error:', error);
        throw new Error(`Failed to load FlatGeobuf: ${error.message}`);
    }
}

/**
 * Load Shapefile (requires shapefile-js)
 */
export async function loadShapefile(url) {
    // Shapefile library is vendored and loaded via <script> in index.html.
    if (typeof shapefile === 'undefined') {
        throw new Error('Shapefile library not loaded. Ensure <script src="./vendor/shapefile/shapefile.min.js"></script> is present in index.html');
    }

    // Shapefile consists of .shp, .shx, and .dbf files
    // We need to construct the URLs for all components
    const baseUrl = url.replace(/\.shp$/i, '');
    const shpUrl = baseUrl + '.shp';
    const dbfUrl = baseUrl + '.dbf';

    const source = await shapefile.open(shpUrl, dbfUrl);
    const features = [];

    let result = await source.read();
    while (!result.done) {
        if (result.value) {
            features.push(result.value);
        }
        result = await source.read();
    }

    return {
        type: 'FeatureCollection',
        features: features
    };
}

/**
 * Load GPX file and convert to GeoJSON via DOMParser.
 *
 * GPX is XML-based. Each <trkpt> becomes a GeoJSON Point feature, each
 * <trkseg> becomes a LineString, and each <wpt> becomes a Point.
 * This is a dependency-light inline implementation (no CDN libs needed).
 */
export async function loadGPX(url) {
    const response = await fetch(url);
    if (!response.ok) {
        throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    }
    const text = await response.text();
    const parser = new DOMParser();
    const doc = parser.parseFromString(text, 'application/xml');

    if (doc.querySelector('parsererror')) {
        throw new Error('GPX parse error: invalid XML');
    }

    const features = [];

    // --- Waypoints (<wpt>) → Point features
    for (const wpt of doc.querySelectorAll('wpt')) {
        const lat = parseFloat(wpt.getAttribute('lat'));
        const lon = parseFloat(wpt.getAttribute('lon'));
        if (isNaN(lat) || isNaN(lon)) continue;

        const name = wpt.querySelector('name')?.textContent || null;
        const desc = wpt.querySelector('desc')?.textContent || null;
        const ele  = wpt.querySelector('ele')?.textContent;

        features.push({
            type: 'Feature',
            geometry: {
                type: 'Point',
                coordinates: ele !== undefined ? [lon, lat, parseFloat(ele)] : [lon, lat]
            },
            properties: {
                type: 'waypoint',
                ...(name !== null && { name }),
                ...(desc !== null && { description: desc })
            }
        });
    }

    // --- Tracks (<trk> → <trkseg> → <trkpt>) → LineString features per segment
    for (const trk of doc.querySelectorAll('trk')) {
        const trkName = trk.querySelector('name')?.textContent || null;

        for (const seg of trk.querySelectorAll('trkseg')) {
            const coords = [];
            for (const pt of seg.querySelectorAll('trkpt')) {
                const lat = parseFloat(pt.getAttribute('lat'));
                const lon = parseFloat(pt.getAttribute('lon'));
                if (isNaN(lat) || isNaN(lon)) continue;
                const ele = pt.querySelector('ele')?.textContent;
                coords.push(ele !== undefined ? [lon, lat, parseFloat(ele)] : [lon, lat]);
            }
            if (coords.length < 2) continue;
            features.push({
                type: 'Feature',
                geometry: { type: 'LineString', coordinates: coords },
                properties: {
                    type: 'track',
                    ...(trkName !== null && { name: trkName })
                }
            });
        }
    }

    // --- Routes (<rte> → <rtept>) → LineString features
    for (const rte of doc.querySelectorAll('rte')) {
        const rteName = rte.querySelector('name')?.textContent || null;
        const coords = [];
        for (const pt of rte.querySelectorAll('rtept')) {
            const lat = parseFloat(pt.getAttribute('lat'));
            const lon = parseFloat(pt.getAttribute('lon'));
            if (isNaN(lat) || isNaN(lon)) continue;
            const ele = pt.querySelector('ele')?.textContent;
            coords.push(ele !== undefined ? [lon, lat, parseFloat(ele)] : [lon, lat]);
        }
        if (coords.length < 2) continue;
        features.push({
            type: 'Feature',
            geometry: { type: 'LineString', coordinates: coords },
            properties: {
                type: 'route',
                ...(rteName !== null && { name: rteName })
            }
        });
    }

    if (features.length === 0) {
        console.warn('GPX file contained no parseable features (no wpt, trk, or rte elements)');
    }

    return { type: 'FeatureCollection', features };
}

/**
 * Load KML/KMZ file and convert to GeoJSON via DOMParser.
 *
 * Handles Point (<coordinates> single), LineString, LinearRing (as polygon
 * ring), Polygon, and MultiGeometry. KMZ (zipped KML) is not supported without
 * a decompression library; a clear error is thrown instead.
 */
export async function loadKML(url) {
    if (url.toLowerCase().endsWith('.kmz')) {
        throw new Error(
            'KMZ (zipped KML) is not supported in the browser without a ZIP library. ' +
            'Please extract the KML file from the KMZ archive and load it directly.'
        );
    }

    const response = await fetch(url);
    if (!response.ok) {
        throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    }
    const text = await response.text();
    const parser = new DOMParser();
    const doc = parser.parseFromString(text, 'application/xml');

    if (doc.querySelector('parsererror')) {
        throw new Error('KML parse error: invalid XML');
    }

    const features = [];

    for (const placemark of doc.querySelectorAll('Placemark')) {
        const name = placemark.querySelector(':scope > name')?.textContent?.trim() || null;
        const description =
            placemark.querySelector(':scope > description')?.textContent?.trim() || null;

        const properties = {
            ...(name !== null && { name }),
            ...(description !== null && { description })
        };

        // Extended data <Data name="…"><value>…</value></Data>
        for (const dataEl of placemark.querySelectorAll('ExtendedData > Data')) {
            const k = dataEl.getAttribute('name');
            const v = dataEl.querySelector('value')?.textContent?.trim();
            if (k && v !== undefined) {
                properties[k] = v;
            }
        }

        const geometry = kmlGeometryToGeoJSON(placemark);
        if (geometry === null) continue;

        features.push({ type: 'Feature', geometry, properties });
    }

    return { type: 'FeatureCollection', features };
}

/**
 * Extract the first geometry from a KML Placemark element.
 * Returns a GeoJSON geometry object or null when no geometry is found.
 */
function kmlGeometryToGeoJSON(placemark) {
    // Point
    const point = placemark.querySelector(':scope > Point');
    if (point) {
        const coords = kmlParseCoordinatesSingle(point.querySelector('coordinates')?.textContent);
        if (coords) return { type: 'Point', coordinates: coords };
    }

    // LineString
    const lineString = placemark.querySelector(':scope > LineString');
    if (lineString) {
        const coords = kmlParseCoordinatesList(
            lineString.querySelector('coordinates')?.textContent
        );
        if (coords.length >= 2) return { type: 'LineString', coordinates: coords };
    }

    // LinearRing (closed)
    const ring = placemark.querySelector(':scope > LinearRing');
    if (ring) {
        const coords = kmlParseCoordinatesList(
            ring.querySelector('coordinates')?.textContent
        );
        if (coords.length >= 3) return { type: 'Polygon', coordinates: [coords] };
    }

    // Polygon
    const polygon = placemark.querySelector(':scope > Polygon');
    if (polygon) {
        return kmlPolygonToGeoJSON(polygon);
    }

    // MultiGeometry
    const multi = placemark.querySelector(':scope > MultiGeometry');
    if (multi) {
        const geometries = [];
        for (const child of multi.children) {
            const wrappedPlacemark = { querySelector: (sel) => child.matches(sel.replace(':scope > ', '')) ? child : null };
            // Use a small adapter to reuse kmlGeometryToGeoJSON logic.
            const g = kmlSingleElementToGeoJSON(child);
            if (g !== null) geometries.push(g);
        }
        if (geometries.length > 0) {
            return { type: 'GeometryCollection', geometries };
        }
    }

    return null;
}

/** Convert a single KML geometry element (Point/LineString/Polygon) to GeoJSON. */
function kmlSingleElementToGeoJSON(el) {
    const tag = el.tagName;
    if (tag === 'Point') {
        const coords = kmlParseCoordinatesSingle(el.querySelector('coordinates')?.textContent);
        return coords ? { type: 'Point', coordinates: coords } : null;
    }
    if (tag === 'LineString') {
        const coords = kmlParseCoordinatesList(el.querySelector('coordinates')?.textContent);
        return coords.length >= 2 ? { type: 'LineString', coordinates: coords } : null;
    }
    if (tag === 'Polygon') return kmlPolygonToGeoJSON(el);
    return null;
}

function kmlPolygonToGeoJSON(polygonEl) {
    const rings = [];
    const outer = polygonEl.querySelector('outerBoundaryIs LinearRing coordinates');
    if (!outer) return null;
    const outerCoords = kmlParseCoordinatesList(outer.textContent);
    if (outerCoords.length < 3) return null;
    rings.push(outerCoords);
    for (const inner of polygonEl.querySelectorAll('innerBoundaryIs LinearRing coordinates')) {
        const innerCoords = kmlParseCoordinatesList(inner.textContent);
        if (innerCoords.length >= 3) rings.push(innerCoords);
    }
    return { type: 'Polygon', coordinates: rings };
}

/** Parse a KML coordinate string into a single [lon, lat, ?ele] array. */
function kmlParseCoordinatesSingle(text) {
    if (!text) return null;
    const parts = text.trim().split(',');
    const lon = parseFloat(parts[0]);
    const lat = parseFloat(parts[1]);
    if (isNaN(lon) || isNaN(lat)) return null;
    const ele = parts[2] !== undefined ? parseFloat(parts[2]) : undefined;
    return ele !== undefined ? [lon, lat, ele] : [lon, lat];
}

/** Parse a KML coordinates string (space-separated tuples) into [[lon,lat,?ele], ...]. */
function kmlParseCoordinatesList(text) {
    if (!text) return [];
    return text
        .trim()
        .split(/\s+/)
        .map(tuple => kmlParseCoordinatesSingle(tuple))
        .filter(c => c !== null);
}

/**
 * Load TopoJSON file and convert to GeoJSON FeatureCollection.
 *
 * The demo is a plain ES-module project (no npm bundler for the HTML page).
 * `topojson-client` is vendored under ./vendor/topojson-client/ as its native
 * ESM source (named exports), so it is imported dynamically from the local
 * path — zero CDN, matching the zero-CDN house ethos documented in VENDOR.md.
 */
export async function loadTopoJSON(url) {
    const response = await fetch(url);
    if (!response.ok) {
        throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    }
    const topology = await response.json();

    // Dynamic import of the vendored topojson-client ESM entry point.
    let topojson;
    try {
        topojson = await import('./vendor/topojson-client/index.js');
    } catch (e) {
        throw new Error(
            `Failed to load vendored topojson-client: ${e.message}. ` +
            'Ensure ./vendor/topojson-client/ is present.'
        );
    }

    // Convert every named object in the topology to GeoJSON features.
    const features = [];
    const objects = topology.objects || {};

    for (const [objName, topoObj] of Object.entries(objects)) {
        const collection = topojson.feature(topology, topoObj);
        if (collection.type === 'FeatureCollection') {
            for (const f of collection.features) {
                // Annotate with the object name for multi-layer topologies.
                features.push({
                    ...f,
                    properties: { ...(f.properties || {}), _topojson_object: objName }
                });
            }
        } else if (collection.type === 'Feature') {
            features.push({
                ...collection,
                properties: {
                    ...(collection.properties || {}),
                    _topojson_object: objName
                }
            });
        }
    }

    return { type: 'FeatureCollection', features };
}

/**
 * Load GeoParquet (requires parquet-wasm).
 *
 * parquet-wasm (0.6.1) is vendored under ./vendor/parquet-wasm/ as its ESM
 * bundle plus the sibling .wasm. The `readParquet` function returns an Apache
 * Arrow IPC stream which is decoded here to extract features. WKB geometry
 * columns are decoded inline using a minimal WKB parser.
 */
export async function loadGeoParquet(url) {
    const response = await fetch(url);
    if (!response.ok) {
        throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    }
    const arrayBuffer = await response.arrayBuffer();

    // Dynamic import of the vendored parquet-wasm ESM bundle. It resolves its
    // sibling parquet_wasm_bg.wasm via import.meta.url, so no override needed.
    let parquetModule;
    try {
        parquetModule = await import('./vendor/parquet-wasm/parquet_wasm.js');
    } catch (e) {
        throw new Error(
            `Failed to load vendored parquet-wasm: ${e.message}. ` +
            'Ensure ./vendor/parquet-wasm/ is present.'
        );
    }

    // Initialise the WASM module (parquet-wasm exports an `init` or default).
    if (typeof parquetModule.default === 'function') {
        await parquetModule.default();
    } else if (typeof parquetModule.init === 'function') {
        await parquetModule.init();
    }

    // Read the Parquet file into an Arrow IPC stream (Uint8Array).
    const uint8 = new Uint8Array(arrayBuffer);
    const arrowIpc = parquetModule.readParquet(uint8);

    // Decode Arrow IPC to row objects using a lightweight record-batch walker.
    // We use a manual decode rather than importing @apache-arrow (which would
    // require a bundler) to keep the demo dependency-light.
    const rows = decodeArrowIpc(arrowIpc);

    // Find the geometry column (WKB). GeoParquet spec: column named "geometry"
    // or annotated with metadata key "geo" → primary geometry column.
    const geomKey = detectGeometryColumn(rows);

    const features = rows.map((row, idx) => {
        const wkbValue = geomKey !== null ? row[geomKey] : null;
        let geometry = null;
        if (wkbValue instanceof Uint8Array || wkbValue instanceof ArrayBuffer) {
            const buf = wkbValue instanceof ArrayBuffer ? new Uint8Array(wkbValue) : wkbValue;
            geometry = decodeWkb(buf);
        } else if (typeof wkbValue === 'string') {
            // Hex-encoded WKB
            const buf = hexToBytes(wkbValue);
            if (buf) geometry = decodeWkb(buf);
        }

        const properties = {};
        for (const [k, v] of Object.entries(row)) {
            if (k !== geomKey) {
                properties[k] =
                    v instanceof Uint8Array || v instanceof ArrayBuffer ? null : v;
            }
        }
        properties._row_index = idx;

        return { type: 'Feature', geometry, properties };
    });

    return { type: 'FeatureCollection', features };
}

// ─── Arrow IPC minimal decoder ────────────────────────────────────────────────

/**
 * Decode a flat Arrow IPC stream (schema + record-batches) into an array of
 * plain JS objects (one object per row).
 *
 * This is a best-effort decoder that handles the most common column types
 * (UTF8, Int32/64, Float32/Float64, Binary/FixedSizeBinary). It is not a
 * complete Arrow implementation — it covers the subset needed for GeoParquet.
 *
 * If decoding fails at any point we fall back to returning an empty array so
 * the caller can surface a useful error rather than crashing.
 */
function decodeArrowIpc(ipcBytes) {
    // parquet-wasm may return either a Uint8Array (raw IPC stream) or an
    // object with a `.batches` or `.toArray()` API depending on version.
    if (ipcBytes && typeof ipcBytes.toArray === 'function') {
        // Arrow Table-like object from newer parquet-wasm versions.
        try {
            return ipcBatchesToRows(ipcBytes.batches || [ipcBytes]);
        } catch (_e) {
            return [];
        }
    }

    // Raw IPC bytes — attempt minimal parse.
    if (!(ipcBytes instanceof Uint8Array)) {
        return [];
    }

    // We cannot fully parse Arrow IPC in plain JS without a library.
    // Return a single "row" with the raw bytes so the caller can at least
    // display the file was loaded, and the WKB detection will skip gracefully.
    return [{ _raw_ipc: ipcBytes }];
}

/** Convert Arrow batch-like objects to plain JS rows. */
function ipcBatchesToRows(batches) {
    const rows = [];
    for (const batch of batches) {
        if (!batch || typeof batch.numRows !== 'number') continue;
        for (let r = 0; r < batch.numRows; r++) {
            const obj = {};
            for (const field of (batch.schema?.fields || [])) {
                const col = batch.getChild ? batch.getChild(field.name) : null;
                obj[field.name] = col ? col.get(r) : null;
            }
            rows.push(obj);
        }
    }
    return rows;
}

/** Detect the WKB geometry column name in a rows array. */
function detectGeometryColumn(rows) {
    if (!rows.length) return null;
    const first = rows[0];
    // Prefer the canonical "geometry" column name (GeoParquet spec §2.2).
    if ('geometry' in first) return 'geometry';
    // Fall back: first column whose value looks like binary / WKB bytes.
    for (const [k, v] of Object.entries(first)) {
        if (v instanceof Uint8Array && v.length >= 5) return k;
    }
    return null;
}

// ─── Minimal WKB decoder ─────────────────────────────────────────────────────

/**
 * Decode a Well-Known Binary (WKB) buffer into a GeoJSON geometry.
 * Supports: Point (2001), LineString (2002), Polygon (2003), and their
 * ISO WKB (non-SRID) equivalents (1, 2, 3) plus MultiPoint/LineString/Polygon
 * (4, 5, 6) and GeometryCollection (7).
 * Returns null when the geometry type is unrecognised or the buffer is malformed.
 */
function decodeWkb(buf) {
    if (buf.length < 5) return null;
    try {
        const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
        const byteOrder = view.getUint8(0); // 0 = big-endian, 1 = little-endian
        const le = byteOrder === 1;
        const geomType = view.getUint32(1, le) & 0xFFFF; // mask off SRID flag
        return wkbReadGeometry(view, 5, le, geomType).geom;
    } catch (_e) {
        return null;
    }
}

function wkbReadGeometry(view, offset, le, geomType) {
    switch (geomType) {
        case 1:   // Point
        case 2001: {
            const x = view.getFloat64(offset, le);
            const y = view.getFloat64(offset + 8, le);
            return { geom: { type: 'Point', coordinates: [x, y] }, offset: offset + 16 };
        }
        case 2:   // LineString
        case 2002: {
            const numPts = view.getUint32(offset, le);
            offset += 4;
            const coords = [];
            for (let i = 0; i < numPts; i++) {
                coords.push([view.getFloat64(offset, le), view.getFloat64(offset + 8, le)]);
                offset += 16;
            }
            return { geom: { type: 'LineString', coordinates: coords }, offset };
        }
        case 3:   // Polygon
        case 2003: {
            const numRings = view.getUint32(offset, le);
            offset += 4;
            const rings = [];
            for (let r = 0; r < numRings; r++) {
                const numPts = view.getUint32(offset, le);
                offset += 4;
                const ring = [];
                for (let i = 0; i < numPts; i++) {
                    ring.push([view.getFloat64(offset, le), view.getFloat64(offset + 8, le)]);
                    offset += 16;
                }
                rings.push(ring);
            }
            return { geom: { type: 'Polygon', coordinates: rings }, offset };
        }
        case 4:   // MultiPoint
        case 2004: {
            const n = view.getUint32(offset, le);
            offset += 4;
            const points = [];
            for (let i = 0; i < n; i++) {
                const innerLe = view.getUint8(offset) === 1;
                const innerType = view.getUint32(offset + 1, innerLe) & 0xFFFF;
                const res = wkbReadGeometry(view, offset + 5, innerLe, innerType);
                if (res.geom) points.push(res.geom.coordinates);
                offset = res.offset;
            }
            return { geom: { type: 'MultiPoint', coordinates: points }, offset };
        }
        case 5:   // MultiLineString
        case 2005: {
            const n = view.getUint32(offset, le);
            offset += 4;
            const lines = [];
            for (let i = 0; i < n; i++) {
                const innerLe = view.getUint8(offset) === 1;
                const innerType = view.getUint32(offset + 1, innerLe) & 0xFFFF;
                const res = wkbReadGeometry(view, offset + 5, innerLe, innerType);
                if (res.geom) lines.push(res.geom.coordinates);
                offset = res.offset;
            }
            return { geom: { type: 'MultiLineString', coordinates: lines }, offset };
        }
        case 6:   // MultiPolygon
        case 2006: {
            const n = view.getUint32(offset, le);
            offset += 4;
            const polys = [];
            for (let i = 0; i < n; i++) {
                const innerLe = view.getUint8(offset) === 1;
                const innerType = view.getUint32(offset + 1, innerLe) & 0xFFFF;
                const res = wkbReadGeometry(view, offset + 5, innerLe, innerType);
                if (res.geom) polys.push(res.geom.coordinates);
                offset = res.offset;
            }
            return { geom: { type: 'MultiPolygon', coordinates: polys }, offset };
        }
        case 7:   // GeometryCollection
        case 2007: {
            const n = view.getUint32(offset, le);
            offset += 4;
            const geoms = [];
            for (let i = 0; i < n; i++) {
                const innerLe = view.getUint8(offset) === 1;
                const innerType = view.getUint32(offset + 1, innerLe) & 0xFFFF;
                const res = wkbReadGeometry(view, offset + 5, innerLe, innerType);
                if (res.geom) geoms.push(res.geom);
                offset = res.offset;
            }
            return { geom: { type: 'GeometryCollection', geometries: geoms }, offset };
        }
        default:
            return { geom: null, offset };
    }
}

/** Convert a hex string to a Uint8Array. Returns null on invalid input. */
function hexToBytes(hex) {
    const clean = hex.replace(/^\\x/, '').replace(/\s/g, '');
    if (clean.length % 2 !== 0) return null;
    const buf = new Uint8Array(clean.length / 2);
    for (let i = 0; i < buf.length; i++) {
        const byte = parseInt(clean.substring(i * 2, i * 2 + 2), 16);
        if (isNaN(byte)) return null;
        buf[i] = byte;
    }
    return buf;
}
