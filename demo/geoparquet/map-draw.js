/**
 * map-draw.js — hand-rolled rectangle drawing for Leaflet (no leaflet-draw).
 *
 * One tool: press "draw" to arm, then mousedown-drag-mouseup on the map to
 * sweep a rectangle. While dragging, `onChange(bbox)` fires on every move so
 * the app can show a live query-cost preview; `onComplete(bbox)` fires on
 * mouseup. Map panning is disabled only while the tool is armed.
 *
 * bbox format everywhere: [minLon, minLat, maxLon, maxLat] (WGS84).
 */

/* global L */

const BOX_STYLE = {
    color: '#f59e0b',
    weight: 2,
    fillColor: '#f59e0b',
    fillOpacity: 0.08,
    interactive: false,
};

export class BoxDraw {
    /**
     * @param {L.Map} map        Leaflet map instance.
     * @param {object} handlers  { onChange(bbox), onComplete(bbox), onArmChange(armed) }
     */
    constructor(map, handlers = {}) {
        this.map = map;
        this.onChange = handlers.onChange || (() => {});
        this.onComplete = handlers.onComplete || (() => {});
        this.onArmChange = handlers.onArmChange || (() => {});
        this.armed = false;
        this.dragging = false;
        this.startLatLng = null;
        this.rect = null;
        this.bbox = null;

        this._onDown = (e) => this._handleDown(e);
        this._onMove = (e) => this._handleMove(e);
        this._onUp = (e) => this._handleUp(e);
        map.on('mousedown', this._onDown);
        map.on('mousemove', this._onMove);
        map.on('mouseup', this._onUp);
    }

    /** Arm or disarm the drawing tool (toggles map panning + cursor). */
    setArmed(armed) {
        if (this.armed === armed) return;
        this.armed = armed;
        const container = this.map.getContainer();
        if (armed) {
            this.map.dragging.disable();
            container.classList.add('drawing');
        } else {
            this.map.dragging.enable();
            container.classList.remove('drawing');
            this.dragging = false;
            this.startLatLng = null;
        }
        this.onArmChange(armed);
    }

    toggle() { this.setArmed(!this.armed); }

    /** Programmatically place the box (used by example presets). */
    setBox(bbox) {
        this.bbox = bbox.slice();
        this._render(bbox);
        this.onComplete(this.bbox.slice());
    }

    /** Remove the rectangle and forget the box. */
    clear() {
        if (this.rect) { this.rect.remove(); this.rect = null; }
        this.bbox = null;
    }

    _handleDown(e) {
        if (!this.armed) return;
        L.DomEvent.stop(e.originalEvent);
        this.dragging = true;
        this.startLatLng = e.latlng;
        this._render(this._bboxFrom(e.latlng));
    }

    _handleMove(e) {
        if (!this.armed || !this.dragging) return;
        const bbox = this._bboxFrom(e.latlng);
        this._render(bbox);
        this.onChange(bbox);
    }

    _handleUp(e) {
        if (!this.armed || !this.dragging) return;
        this.dragging = false;
        this.bbox = this._bboxFrom(e.latlng);
        this.setArmed(false);
        this.onComplete(this.bbox.slice());
    }

    /** bbox spanned by the press point and the current point. */
    _bboxFrom(latlng) {
        const a = this.startLatLng || latlng;
        return [
            Math.min(a.lng, latlng.lng),
            Math.min(a.lat, latlng.lat),
            Math.max(a.lng, latlng.lng),
            Math.max(a.lat, latlng.lat),
        ];
    }

    _render(bbox) {
        const bounds = L.latLngBounds([bbox[1], bbox[0]], [bbox[3], bbox[2]]);
        if (this.rect) {
            this.rect.setBounds(bounds);
        } else {
            this.rect = L.rectangle(bounds, BOX_STYLE).addTo(this.map);
        }
    }
}
