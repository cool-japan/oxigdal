/**
 * OxiGeo GeoLab — pure terrain raster helpers.
 *
 * Self-contained ES module: these functions turn WasmTerrain outputs into
 * RGBA `ImageData` (or a human label). They depend only on the browser's
 * `ImageData` global and their arguments — no app state, no Leaflet, no DOM —
 * so they live here to keep main.js under the 2000-line limit.
 */

/** Human-readable label for a terrain mode. */
export function terrainLabel(mode) {
    return {
        hillshade: 'Hillshade',
        multidirectional: 'Multidirectional hillshade',
        slope: 'Slope',
        colorrelief: 'Color relief',
    }[mode] || mode;
}

/** Convert single-channel grayscale bytes to an RGBA ImageData. */
export function grayToImageData(gray, width, height) {
    const rgba = new Uint8ClampedArray(width * height * 4);
    for (let i = 0; i < width * height; i++) {
        const g = gray[i] || 0;
        rgba[i * 4] = g;
        rgba[i * 4 + 1] = g;
        rgba[i * 4 + 2] = g;
        rgba[i * 4 + 3] = 255;
    }
    return new ImageData(rgba, width, height);
}

/**
 * Convert slope degrees to an RGBA green→yellow→red ramp, normalised to the
 * observed maximum so gentle terrain still shows full contrast.
 */
export function slopeToImageData(slopeDeg, width, height) {
    let maxSlope = 0;
    for (let i = 0; i < slopeDeg.length; i++) {
        const s = slopeDeg[i];
        if (isFinite(s) && s > maxSlope) {
            maxSlope = s;
        }
    }
    if (maxSlope <= 0) {
        maxSlope = 1;
    }
    const rgba = new Uint8ClampedArray(width * height * 4);
    for (let i = 0; i < width * height; i++) {
        const s = isFinite(slopeDeg[i]) ? slopeDeg[i] : 0;
        const t = Math.min(1, s / maxSlope);
        let r;
        let g;
        const b = 0;
        if (t < 0.5) {
            // green (0,160,0) → yellow (255,255,0)
            const u = t / 0.5;
            r = Math.round(255 * u);
            g = Math.round(160 + (255 - 160) * u);
        } else {
            // yellow (255,255,0) → red (220,0,0)
            const u = (t - 0.5) / 0.5;
            r = Math.round(255 - (255 - 220) * u);
            g = Math.round(255 * (1 - u));
        }
        rgba[i * 4] = r;
        rgba[i * 4 + 1] = g;
        rgba[i * 4 + 2] = b;
        rgba[i * 4 + 3] = 255;
    }
    return new ImageData(rgba, width, height);
}

/** Set alpha to 0 on nodata cells (mask value 0). */
export function applyNodataAlpha(imageData, mask) {
    const data = imageData.data;
    for (let i = 0; i < mask.length; i++) {
        if (!mask[i]) {
            data[i * 4 + 3] = 0;
        }
    }
}
