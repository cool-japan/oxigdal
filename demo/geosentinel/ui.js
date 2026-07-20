/**
 * GeoSentinel — DOM layer.
 *
 * Grabs every element once, wires the static controls (sliders showing
 * their value, error dismiss), and exposes small render helpers so main.js
 * stays orchestration-only. No WASM and no Leaflet in this module.
 */

export const els = {};

const IDS = [
    'version-badge', 'status-dot', 'status-text',
    'fetch-badge', 'upload-badge',
    'aoi-draw-btn', 'aoi-view-btn', 'aoi-label',
    'date-a', 'date-b', 'window-days', 'max-cloud', 'max-cloud-value',
    'threshold', 'threshold-value', 'use-otsu', 'min-area',
    'run-btn', 'progress-line', 'progress-text',
    'results-section', 'result-ha', 'result-count', 'result-meta',
    'scene-chip-a', 'scene-chip-b', 'alt-a', 'alt-b', 'export-btn',
    'example-grid',
    'loading-overlay', 'loading-message',
    'error-overlay', 'error-message', 'dismiss-error',
    'layer-bar', 'crossfade', 'crossfade-label-a', 'crossfade-label-b',
    'change-opacity', 'diff-toggle',
];

/** Cache elements and wire self-contained controls. Call once at startup. */
export function initUi() {
    for (const id of IDS) {
        els[id.replace(/-([a-z])/g, (_, c) => c.toUpperCase())] = document.getElementById(id);
    }

    els.maxCloud.addEventListener('input', () => {
        els.maxCloudValue.textContent = `${els.maxCloud.value}%`;
    });
    els.threshold.addEventListener('input', () => {
        els.thresholdValue.textContent = Number(els.threshold.value).toFixed(2);
    });
    els.useOtsu.addEventListener('change', () => {
        els.threshold.disabled = els.useOtsu.checked;
    });
    els.dismissError.addEventListener('click', hideError);
}

// ── Status & progress ────────────────────────────────────────────────────

export function setStatus(state, text) {
    els.statusDot.className = `status-dot status-${state}`;
    els.statusText.textContent = text;
}

export function showLoading(message) {
    els.loadingMessage.textContent = message;
    els.loadingOverlay.style.display = 'flex';
}

export function hideLoading() {
    els.loadingOverlay.style.display = 'none';
}

export function setProgress(text) {
    if (text) {
        els.progressLine.hidden = false;
        els.progressText.textContent = text;
    } else {
        els.progressLine.hidden = true;
        els.progressText.textContent = '';
    }
}

export function setRunning(running) {
    els.runBtn.disabled = running || !els.runBtn.dataset.aoiReady;
    els.altA.disabled = running || els.altA.options.length <= 1;
    els.altB.disabled = running || els.altB.options.length <= 1;
    if (!running) {
        setProgress(null);
    }
}

// ── Errors ────────────────────────────────────────────────────────────────

export function showError(message) {
    els.errorMessage.textContent = message;
    els.errorOverlay.style.display = 'flex';
    setStatus('error', 'Error');
}

export function hideError() {
    els.errorOverlay.style.display = 'none';
    setStatus('ready', 'Ready');
}

// ── Form access ───────────────────────────────────────────────────────────

/** Read the whole detection form. `bbox` is injected by main.js state. */
export function readParams(bbox) {
    return {
        bbox,
        dateA: els.dateA.value,
        dateB: els.dateB.value,
        windowDays: parseInt(els.windowDays.value, 10),
        maxCloud: parseFloat(els.maxCloud.value),
        threshold: parseFloat(els.threshold.value),
        useOtsu: els.useOtsu.checked,
        minAreaHa: Math.max(0, parseFloat(els.minArea.value) || 0),
    };
}

/** Push example / preset values into the form controls. */
export function applyPreset(preset) {
    if (preset.dateA) { els.dateA.value = preset.dateA; }
    if (preset.dateB) { els.dateB.value = preset.dateB; }
    if (preset.windowDays) {
        ensureSelectValue(els.windowDays, String(preset.windowDays), `± ${preset.windowDays} days`);
    }
    if (typeof preset.maxCloud === 'number') {
        els.maxCloud.value = String(preset.maxCloud);
        els.maxCloudValue.textContent = `${preset.maxCloud}%`;
    }
    if (typeof preset.threshold === 'number') {
        els.threshold.value = String(preset.threshold);
        els.thresholdValue.textContent = preset.threshold.toFixed(2);
    }
    if (typeof preset.minAreaHa === 'number') {
        els.minArea.value = String(preset.minAreaHa);
    }
    els.useOtsu.checked = false;
    els.threshold.disabled = false;
}

function ensureSelectValue(select, value, label) {
    if (![...select.options].some((o) => o.value === value)) {
        const opt = document.createElement('option');
        opt.value = value;
        opt.textContent = label;
        select.appendChild(opt);
    }
    select.value = value;
}

/** Reflect the selected AOI in the sidebar and arm the Run button. */
export function setAoiLabel(bbox) {
    const fmt = (v) => v.toFixed(4);
    els.aoiLabel.textContent =
        `W ${fmt(bbox[0])}, S ${fmt(bbox[1])} → E ${fmt(bbox[2])}, N ${fmt(bbox[3])}`;
    els.aoiLabel.classList.add('aoi-set');
    els.runBtn.dataset.aoiReady = '1';
    els.runBtn.disabled = false;
}

// ── Results ───────────────────────────────────────────────────────────────

/** Build a scene chip via DOM methods (STAC fields are untrusted input). */
function fillSceneChip(chip, tag, cand) {
    chip.replaceChildren();
    const tagSpan = document.createElement('span');
    tagSpan.className = 'chip-tag';
    tagSpan.textContent = tag;
    chip.appendChild(tagSpan);
    chip.appendChild(document.createTextNode(cand.id));
    chip.appendChild(document.createElement('br'));
    const date = (cand.datetime || '').slice(0, 10);
    chip.appendChild(document.createTextNode(`${date} · `));
    const cloudSpan = document.createElement('span');
    cloudSpan.className = 'chip-cloud';
    cloudSpan.textContent = `☁ ${cand.cloud.toFixed(1)}%`;
    chip.appendChild(cloudSpan);
    chip.appendChild(document.createTextNode(` · ${cand.gridCode}`));
}

export function renderResults(result, pair) {
    els.resultsSection.hidden = false;
    els.resultHa.textContent = result.totalHa >= 100
        ? Math.round(result.totalHa).toLocaleString()
        : result.totalHa.toFixed(2);
    els.resultCount.textContent = String(result.polygonCount);
    els.resultMeta.textContent =
        `threshold ${result.thresholdUsed.toFixed(3)} · pyramid level ${result.level} · ` +
        `window ${result.windowPx[2]}×${result.windowPx[3]} px`;
    fillSceneChip(els.sceneChipA, 'A', pair.a);
    fillSceneChip(els.sceneChipB, 'B', pair.b);
}

/**
 * Fill one alternates dropdown. `current` is the active candidate;
 * `alternates` come from the pair search (same MGRS grid, cloud-ascending).
 */
export function renderAlternates(select, current, alternates, onPick) {
    select.replaceChildren();
    const head = document.createElement('option');
    head.value = '';
    head.textContent = `${current.id} (current, ☁ ${current.cloud.toFixed(1)}%)`;
    select.appendChild(head);
    for (let i = 0; i < alternates.length; i++) {
        const alt = alternates[i];
        const opt = document.createElement('option');
        opt.value = String(i);
        opt.textContent = `${alt.id} (☁ ${alt.cloud.toFixed(1)}%)`;
        select.appendChild(opt);
    }
    select.disabled = alternates.length === 0;
    select.onchange = () => {
        const idx = select.value;
        if (idx !== '') {
            onPick(alternates[parseInt(idx, 10)]);
        }
    };
}

/** Show the floating layer bar and label the crossfade ends with the dates. */
export function showLayerBar(dateA, dateB) {
    els.layerBar.hidden = false;
    els.crossfadeLabelA.textContent = dateA;
    els.crossfadeLabelB.textContent = dateB;
    els.crossfade.value = '0';
    els.diffToggle.checked = false;
}

// ── Examples ──────────────────────────────────────────────────────────────

/** Render example cards; `onPick(example)` runs when a card is clicked. */
export function renderExamples(examples, onPick) {
    els.exampleGrid.replaceChildren();
    for (const example of examples) {
        const card = document.createElement('button');
        card.className = 'example-card';
        card.type = 'button';
        for (const [cls, text] of [
            ['card-icon', example.icon || '🛰️'],
            ['card-title', example.name],
            ['card-description', example.description || ''],
        ]) {
            const div = document.createElement('div');
            div.className = cls;
            div.textContent = text;
            card.appendChild(div);
        }
        card.addEventListener('click', () => onPick(example));
        els.exampleGrid.appendChild(card);
    }
}

// ── Badges ────────────────────────────────────────────────────────────────

/** Human-readable byte count (KB below 1 MB, MB above). */
export function formatBytes(bytes) {
    const mb = bytes / (1024 * 1024);
    if (mb >= 1) {
        return `${mb.toFixed(1)} MB`;
    }
    return `${(bytes / 1024).toFixed(1)} KB`;
}

export function updateNetworkBadges(bytesFetched) {
    els.fetchBadge.textContent = `⬇ ${formatBytes(bytesFetched)} imagery fetched`;
    els.uploadBadge.textContent = '⬆ 0 bytes uploaded';
}
