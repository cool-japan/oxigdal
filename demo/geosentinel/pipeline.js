/**
 * GeoSentinel — detection pipeline orchestration.
 *
 * Drives the Rust/WASM API end to end:
 *   WasmStacClient.searchPair → GeoSentinel.loadScene ×2 →
 *   GeoSentinel.detectChanges → overlayInfo / diffOverlayRgba /
 *   trueColorRgba ×2
 * and converts the returned RGBA buffers to data URLs for Leaflet
 * image overlays. All JSON shapes follow the design contract and the
 * actual #[wasm_bindgen] exports in crates/oxigeo-wasm/src/sentinel/.
 */

/** Longest-side pixel budget for every read (contract default). */
export const MAX_DIM = 1024;

/**
 * Map a raw Rust error string to a friendly, actionable message.
 * The Rust side rejects with plain strings (typed enums rendered via
 * Display), so we match on stable substrings.
 */
export function friendlyError(err) {
    const msg = err instanceof Error ? err.message : String(err ?? 'unknown error');
    if (msg.includes('no MGRS grid tile has usable scenes on both dates')) {
        return 'Your area straddles more than one Sentinel-2 tile, or the matching tiles were '
            + 'too cloudy. Zoom in further and draw a smaller area, or raise the cloud limit.';
    }
    if (msg.includes('no usable scene for date')) {
        const side = msg.includes('date A') ? 'the "before" date' : 'the "after" date';
        return `No clear Sentinel-2 scene was found near ${side}. Widen the search window `
            + 'or raise the max cloud cover.';
    }
    if (msg.includes('AOI does not intersect the scene')) {
        return 'The selected area falls outside the matched scene. Draw the area again over the '
            + 'imagery, or pick different dates.';
    }
    if (msg.includes('scene CRS mismatch') || msg.includes('grid-origin mismatch')
        || msg.includes('pixel-scale mismatch')) {
        return 'The two scenes are on different satellite grids and cannot be compared '
            + 'pixel-for-pixel. Try the alternate scenes, or a smaller area.';
    }
    if (msg.includes('scene slot') && msg.includes('not loaded')) {
        return 'Scenes are still loading — run the detection again in a moment.';
    }
    if (msg.includes('invalid bbox')) {
        return 'Please select an area first (draw a rectangle or use the current view).';
    }
    if (msg.includes('invalid date')) {
        return 'Please pick two valid dates.';
    }
    if (msg.toLowerCase().includes('network') || msg.includes('HTTP ') || msg.includes('fetch')) {
        return `A network request failed (${msg}). Check your connection and try again.`;
    }
    return msg;
}

/** Paint a flat RGBA buffer onto a canvas and return a PNG data URL. */
export function rgbaToDataUrl(rgba, width, height) {
    const canvas = document.createElement('canvas');
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext('2d');
    const image = new ImageData(new Uint8ClampedArray(rgba), width, height);
    ctx.putImageData(image, 0, 0);
    return canvas.toDataURL('image/png');
}

/**
 * Search the STAC catalog for a same-grid scene pair.
 * @returns {Promise<{pair: {a, b}, alternatesA: [], alternatesB: []}>}
 */
export async function searchPair(stac, bbox, dateA, dateB, windowDays, maxCloud, onProgress) {
    onProgress(`Searching Sentinel-2 catalog (±${windowDays} days, ≤${maxCloud}% cloud)…`);
    const json = await stac.searchPair(
        JSON.stringify(bbox), dateA, dateB, windowDays, maxCloud);
    return JSON.parse(json);
}

/**
 * Open the red / NIR / true-color COGs of one candidate into a scene slot.
 * @returns {Promise<object>} the loadScene JSON summary.
 */
export async function loadScene(sentinel, slot, candidate, onProgress) {
    const tag = slot === 0 ? 'A' : 'B';
    onProgress(`Opening scene ${tag} (${candidate.id})…`);
    const summary = await sentinel.loadScene(
        slot,
        candidate.redHref,
        candidate.nirHref,
        candidate.visualHref ?? null,
        candidate.boaOffsetApplied === true,
    );
    return JSON.parse(summary);
}

/**
 * Run detection over the loaded pair and assemble everything the map needs.
 *
 * @returns {Promise<{result, overlay: {width, height, boundsWgs84},
 *   diffUrl, trueColorUrlA, trueColorUrlB}>}
 */
export async function detect(sentinel, params, hasVisual, onProgress) {
    onProgress('Reading AOI windows & computing NDVI change…');
    const result = JSON.parse(await sentinel.detectChanges(JSON.stringify({
        bbox: params.bbox,
        maxDim: MAX_DIM,
        threshold: params.threshold,
        useOtsu: params.useOtsu,
        minAreaHa: params.minAreaHa,
    })));

    const overlay = JSON.parse(sentinel.overlayInfo());
    const diffUrl = rgbaToDataUrl(sentinel.diffOverlayRgba(), overlay.width, overlay.height);

    let trueColorUrlA = null;
    let trueColorUrlB = null;
    if (hasVisual) {
        onProgress('Fetching true-color imagery (date A)…');
        trueColorUrlA = await readTrueColor(sentinel, 0);
        onProgress('Fetching true-color imagery (date B)…');
        trueColorUrlB = await readTrueColor(sentinel, 1);
    }

    return { result, overlay, diffUrl, trueColorUrlA, trueColorUrlB };
}

/**
 * Read one slot's true-color window and rasterize it, tolerating a missing
 * TCI asset. `TrueColorImage` self-describes its own pixel dimensions (the
 * TCI's own pyramid level, which may differ from the reflectance bands'), so
 * no guessing/factorization against the overlay's dims is needed.
 */
async function readTrueColor(sentinel, slot) {
    let img;
    try {
        img = await sentinel.trueColorRgba(slot);
    } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        if (msg.includes('no true-colour asset')) {
            return null; // Graceful: overlays are optional.
        }
        throw err;
    }
    if (!img.width || !img.height) {
        return null; // Empty window; skip the overlay rather than fail the run.
    }
    return rgbaToDataUrl(img.data, img.width, img.height);
}

/**
 * Full run: search → load ×2 → detect. Returns everything the UI and map
 * consume. `onProgress(text)` is called at each stage boundary.
 */
export async function runDetection(stac, sentinel, params, onProgress) {
    const pairResult = await searchPair(
        stac, params.bbox, params.dateA, params.dateB,
        params.windowDays, params.maxCloud, onProgress);

    const { a, b } = pairResult.pair;
    await loadScene(sentinel, 0, a, onProgress);
    await loadScene(sentinel, 1, b, onProgress);

    const hasVisual = Boolean(a.visualHref) && Boolean(b.visualHref);
    const detection = await detect(sentinel, params, hasVisual, onProgress);

    return { pairResult, detection };
}

/**
 * Re-run after swapping one side for an alternate scene (same grid, so
 * alignment is preserved by construction).
 */
export async function rerunWithAlternate(sentinel, slot, candidate, params, pair, onProgress) {
    await loadScene(sentinel, slot, candidate, onProgress);
    const other = slot === 0 ? pair.b : pair.a;
    const hasVisual = Boolean(candidate.visualHref) && Boolean(other.visualHref);
    return detect(sentinel, params, hasVisual, onProgress);
}
