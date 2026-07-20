/**
 * GeoSentinel — Leaflet map controller.
 *
 * Owns the base map, the AOI rectangle tool, and every result layer:
 *   - true-color image overlays for dates A and B (crossfaded by a slider),
 *   - the NDVI diff heatmap overlay (toggleable),
 *   - the red change-polygon GeoJSON layer.
 *
 * Leaflet is loaded globally (vendored ./vendor/leaflet/leaflet.js), so this
 * module uses the `L` global rather than an import.
 */

/* global L */

/** WGS84 bbox [west, south, east, north] → Leaflet [[south, west], [north, east]]. */
export function bboxToLeafletBounds(bbox) {
    return [[bbox[1], bbox[0]], [bbox[3], bbox[2]]];
}

export class SentinelMap {
    /**
     * @param {string} containerId - DOM id of the map div.
     * @param {(bbox: number[]) => void} onAoiDrawn - called with a WGS84 bbox
     *   [w, s, e, n] when the user finishes drawing a rectangle.
     */
    constructor(containerId, onAoiDrawn) {
        this.onAoiDrawn = onAoiDrawn;
        this.map = L.map(containerId, {
            center: [20.89, -156.65], // Lahaina, the flagship example
            zoom: 12,
            zoomControl: true,
        });

        L.tileLayer('https://tile.openstreetmap.org/{z}/{x}/{y}.png', {
            maxZoom: 19,
            attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors',
        }).addTo(this.map);

        // Result layers.
        this.aoiRect = null;
        this.overlayA = null;
        this.overlayB = null;
        this.diffOverlay = null;
        this.changeLayer = null;

        // Layer state.
        this.crossfade = 0;      // 0 = all A, 1 = all B
        this.imageOpacity = 1.0; // master opacity for the true-color pair
        this.diffVisible = false;
        this.changeOpacity = 0.85;

        // AOI drawing state.
        this.drawing = false;
        this.drawStart = null;
        this.drawRect = null;
        this.boundDrawHandlers = null;
    }

    // ── AOI rectangle tool ───────────────────────────────────────────────

    /** Toggle rectangle-draw mode. Returns the new mode (true = drawing). */
    toggleAoiDraw() {
        if (this.drawing) {
            this.cancelAoiDraw();
            return false;
        }
        this.drawing = true;
        this.map.dragging.disable();
        this.map.getContainer().classList.add('aoi-drawing');

        const onDown = (e) => {
            this.drawStart = e.latlng;
            if (this.drawRect) {
                this.map.removeLayer(this.drawRect);
            }
            this.drawRect = L.rectangle(L.latLngBounds(e.latlng, e.latlng), {
                color: '#2563eb',
                weight: 2,
                dashArray: '6 4',
                fillOpacity: 0.05,
            }).addTo(this.map);
        };
        const onMove = (e) => {
            if (this.drawStart && this.drawRect) {
                this.drawRect.setBounds(L.latLngBounds(this.drawStart, e.latlng));
            }
        };
        const onUp = (e) => {
            if (!this.drawStart) {
                return;
            }
            const bounds = L.latLngBounds(this.drawStart, e.latlng);
            this.cancelAoiDraw();
            if (bounds.getWest() === bounds.getEast() || bounds.getSouth() === bounds.getNorth()) {
                return; // A click without a drag is not an AOI.
            }
            const bbox = [bounds.getWest(), bounds.getSouth(), bounds.getEast(), bounds.getNorth()];
            this.setAoi(bbox);
            this.onAoiDrawn(bbox);
        };

        this.boundDrawHandlers = { onDown, onMove, onUp };
        this.map.on('mousedown', onDown);
        this.map.on('mousemove', onMove);
        this.map.on('mouseup', onUp);
        return true;
    }

    /** Leave draw mode and restore normal map interaction. */
    cancelAoiDraw() {
        if (this.boundDrawHandlers) {
            this.map.off('mousedown', this.boundDrawHandlers.onDown);
            this.map.off('mousemove', this.boundDrawHandlers.onMove);
            this.map.off('mouseup', this.boundDrawHandlers.onUp);
            this.boundDrawHandlers = null;
        }
        if (this.drawRect) {
            this.map.removeLayer(this.drawRect);
            this.drawRect = null;
        }
        this.drawStart = null;
        this.drawing = false;
        this.map.dragging.enable();
        this.map.getContainer().classList.remove('aoi-drawing');
    }

    /** The current viewport as a WGS84 bbox [w, s, e, n]. */
    currentViewBbox() {
        const b = this.map.getBounds();
        return [b.getWest(), b.getSouth(), b.getEast(), b.getNorth()];
    }

    /** Show (or move) the persistent AOI rectangle. */
    setAoi(bbox) {
        const bounds = bboxToLeafletBounds(bbox);
        if (this.aoiRect) {
            this.aoiRect.setBounds(bounds);
        } else {
            this.aoiRect = L.rectangle(bounds, {
                color: '#2563eb',
                weight: 2,
                fill: false,
                dashArray: '6 4',
                interactive: false,
            }).addTo(this.map);
        }
    }

    /** Fit the view to a WGS84 bbox. */
    fitBbox(bbox, padded = true) {
        this.map.fitBounds(bboxToLeafletBounds(bbox), padded ? { padding: [24, 24] } : {});
    }

    // ── Result overlays ──────────────────────────────────────────────────

    /** Remove all result layers (keeps the AOI rectangle). */
    clearResults() {
        for (const key of ['overlayA', 'overlayB', 'diffOverlay', 'changeLayer']) {
            if (this[key]) {
                this.map.removeLayer(this[key]);
                this[key] = null;
            }
        }
    }

    /**
     * Install the true-color overlays for slots A and B.
     * @param {string|null} urlA - data URL for date A (or null when no TCI).
     * @param {string|null} urlB - data URL for date B.
     * @param {number[]} bbox - WGS84 bounds of both images.
     */
    setTrueColor(urlA, urlB, bbox) {
        const bounds = bboxToLeafletBounds(bbox);
        if (this.overlayA) {
            this.map.removeLayer(this.overlayA);
            this.overlayA = null;
        }
        if (this.overlayB) {
            this.map.removeLayer(this.overlayB);
            this.overlayB = null;
        }
        if (urlA) {
            this.overlayA = L.imageOverlay(urlA, bounds, { interactive: false }).addTo(this.map);
        }
        if (urlB) {
            this.overlayB = L.imageOverlay(urlB, bounds, { interactive: false }).addTo(this.map);
        }
        this.applyCrossfade();
    }

    /** Crossfade position: 0 shows date A, 1 shows date B. */
    setCrossfade(t) {
        this.crossfade = Math.min(1, Math.max(0, t));
        this.applyCrossfade();
    }

    applyCrossfade() {
        // A stays fully visible underneath; B fades in on top. This reads as a
        // true crossfade without the mid-slider dip of a dual-fade.
        if (this.overlayA) {
            this.overlayA.setOpacity(this.imageOpacity);
        }
        if (this.overlayB) {
            this.overlayB.setOpacity(this.imageOpacity * this.crossfade);
        }
    }

    /** Install the NDVI diff heatmap overlay (hidden until toggled on). */
    setDiffOverlay(url, bbox) {
        if (this.diffOverlay) {
            this.map.removeLayer(this.diffOverlay);
            this.diffOverlay = null;
        }
        this.diffUrl = url;
        this.diffBbox = bbox;
        if (this.diffVisible) {
            this.showDiff(true);
        }
    }

    /** Show or hide the diff heatmap. */
    showDiff(visible) {
        this.diffVisible = visible;
        if (visible && !this.diffOverlay && this.diffUrl) {
            this.diffOverlay = L.imageOverlay(this.diffUrl, bboxToLeafletBounds(this.diffBbox), {
                opacity: 0.75,
                interactive: false,
            }).addTo(this.map);
        } else if (!visible && this.diffOverlay) {
            this.map.removeLayer(this.diffOverlay);
            this.diffOverlay = null;
        }
    }

    /** Install the red change-polygon layer from a GeoJSON FeatureCollection. */
    setChangePolygons(fc) {
        if (this.changeLayer) {
            this.map.removeLayer(this.changeLayer);
            this.changeLayer = null;
        }
        if (!fc || !Array.isArray(fc.features) || fc.features.length === 0) {
            return;
        }
        this.changeLayer = L.geoJSON(fc, {
            style: () => ({
                color: '#ef4444',
                weight: 2,
                fillColor: '#ef4444',
                fillOpacity: 0.25 * this.changeOpacity,
                opacity: this.changeOpacity,
            }),
            onEachFeature: (feature, layer) => {
                const props = feature.properties || {};
                const ha = typeof props.area_ha === 'number' ? props.area_ha.toFixed(2) : '?';
                layer.bindPopup(
                    `<div class="change-popup"><strong>${ha} ha</strong> changed<br>` +
                    `NDVI drop ≥ threshold between dates A and B</div>`
                );
            },
        }).addTo(this.map);
    }

    /** Master opacity for the change-polygon layer (0..1). */
    setChangeOpacity(v) {
        this.changeOpacity = Math.min(1, Math.max(0, v));
        if (this.changeLayer) {
            this.changeLayer.setStyle({
                opacity: this.changeOpacity,
                fillOpacity: 0.25 * this.changeOpacity,
            });
        }
    }
}
