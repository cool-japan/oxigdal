/**
 * rg-strip.js — one <canvas> strip visualising every Parquet row group.
 *
 * One cell per row group (9,533 for the live VIDA file, 3 for the offline
 * sample): grey = pruned from metadata alone (bytes never downloaded),
 * amber = survivor of the current plan, green = actually fetched by the
 * last query. When there are more row groups than device pixels, each
 * pixel column shows the *highest* state in its bucket so a lone survivor
 * among thousands of pruned groups stays visible.
 */

const STATE_PRUNED = 0;
const STATE_SURVIVOR = 1;
const STATE_FETCHED = 2;

const COLORS = ['#374151', '#f59e0b', '#10b981']; // pruned / survivor / fetched
const IDLE_COLOR = '#1f2937';                      // before any plan exists

export class RowGroupStrip {
    /** @param {HTMLCanvasElement} canvas */
    constructor(canvas) {
        this.canvas = canvas;
        this.total = 0;
        this.states = new Uint8Array(0);
        this.hasPlan = false;
        this._resize = () => this.draw();
        window.addEventListener('resize', this._resize);
    }

    /** Configure for a newly opened dataset with `total` row groups. */
    setTotal(total) {
        this.total = total;
        this.states = new Uint8Array(total);
        this.hasPlan = false;
        this.draw();
    }

    /** Show a plan: `survivors` (index iterable) turn amber, the rest grey. */
    setSurvivors(survivors) {
        this.states.fill(STATE_PRUNED);
        for (const idx of survivors) {
            if (idx < this.total) this.states[idx] = STATE_SURVIVOR;
        }
        this.hasPlan = true;
        this.draw();
    }

    /** Mark row groups whose bytes were downloaded by the last query. */
    markFetched(indices) {
        for (const idx of indices) {
            if (idx < this.total) this.states[idx] = STATE_FETCHED;
        }
        this.draw();
    }

    /** Forget plan/fetch state but keep the dataset size. */
    reset() {
        this.states.fill(STATE_PRUNED);
        this.hasPlan = false;
        this.draw();
    }

    /** Repaint the whole strip (cheap: one pass over the pixel columns). */
    draw() {
        const dpr = window.devicePixelRatio || 1;
        const cssWidth = this.canvas.clientWidth || 600;
        const cssHeight = this.canvas.clientHeight || 26;
        const width = Math.max(1, Math.round(cssWidth * dpr));
        const height = Math.max(1, Math.round(cssHeight * dpr));
        if (this.canvas.width !== width) this.canvas.width = width;
        if (this.canvas.height !== height) this.canvas.height = height;

        const ctx = this.canvas.getContext('2d');
        ctx.fillStyle = IDLE_COLOR;
        ctx.fillRect(0, 0, width, height);
        if (this.total === 0 || !this.hasPlan) return;

        if (this.total <= width) {
            // One or more pixels per cell: draw discrete cells with hairline gaps.
            const cell = width / this.total;
            const gap = cell > 3 * dpr ? dpr : 0;
            for (let i = 0; i < this.total; i++) {
                ctx.fillStyle = COLORS[this.states[i]];
                ctx.fillRect(i * cell, 0, Math.max(1, cell - gap), height);
            }
        } else {
            // More cells than pixels: bucket per pixel column, keep max state.
            for (let x = 0; x < width; x++) {
                const lo = Math.floor((x * this.total) / width);
                const hi = Math.max(lo + 1, Math.floor(((x + 1) * this.total) / width));
                let state = STATE_PRUNED;
                for (let i = lo; i < hi && i < this.total; i++) {
                    if (this.states[i] > state) state = this.states[i];
                }
                ctx.fillStyle = COLORS[state];
                ctx.fillRect(x, 0, 1, height);
            }
        }
    }
}
