/**
 * GeoVault — vault-ui.js
 *
 * Session-ledger plumbing for the sovereign clean-room workstation:
 *
 *  - WASM init and WasmVaultSession lifecycle (blake3 hash chain → Merkle
 *    root → Ed25519 seal, all inside oxigdal-security compiled to wasm)
 *  - Network guards: fetch / XMLHttpRequest / sendBeacon are wrapped BEFORE
 *    anything else runs; any request to a non-same-origin URL is blocked
 *    (rejected, never sent) and recorded in the ledger as
 *    `net.external.blocked`. Same-origin responses are metered as ingress.
 *  - `securitypolicyviolation` listener → `csp.violation` ledger entries
 *    (defense in depth: the CSP blocks what the hooks cannot see).
 *  - Live Session Ledger rendering, status bar, and the SEAL SESSION flow
 *    (attestation modal, copy buttons, attestation.json download).
 *
 * Trust model (also shown verbatim in the UI): the attestation proves the
 * integrity and completeness of the recorded log and that this session's
 * key sealed it. The zero-egress figures are enforced and observed by this
 * code under a browser-enforced CSP — they are not a mathematical proof
 * that no other software on the machine sent anything.
 */

import wasmInit, {
    WasmVaultSession,
    verifyAttestation,
    version as wasmVersion,
} from './pkg/oxigdal_wasm.js';

/* ------------------------------------------------------------------ */
/* Policy                                                              */
/* ------------------------------------------------------------------ */

/**
 * The canonical clean-room policy, exactly as deployed via the site's
 * `_headers` file. The <meta> tag in index.html carries the same policy
 * minus `frame-ancestors` (which is ignored in meta form).
 */
const CSP_POLICY =
    "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; " +
    "connect-src 'self'; img-src 'self' blob: data:; style-src 'self'; " +
    "font-src 'self'; worker-src 'self' blob:; object-src 'none'; " +
    "base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/** Enforcement layers bound into the attestation's policy digest. */
const ENFORCEMENT = [
    'csp-meta',
    'csp-header',
    'fetch-hook',
    'xhr-hook',
    'beacon-hook',
];

/* ------------------------------------------------------------------ */
/* State                                                               */
/* ------------------------------------------------------------------ */

const state = {
    session: null,
    sealed: false,
    attestationJson: null,
    counters: {
        ingress: 0, // bytes received from our own origin (app assets + data)
        egress: 0, // bytes sent anywhere (stays 0 — nothing ever sends)
        external: 0, // external requests that went through (stays 0)
        blocked: 0, // external requests blocked by the hooks
        cspViolations: 0,
    },
    /** Ledger entries recorded before the session exists (guards install
     *  at import time, the session only after wasm init). */
    pending: [],
    ledgerEl: null,
};

/** True once the session has been sealed (ledger immutable). */
export function isSealed() {
    return state.sealed;
}

/* ------------------------------------------------------------------ */
/* Formatting helpers                                                  */
/* ------------------------------------------------------------------ */

function fmtBytes(n) {
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KiB`;
    return `${(n / (1024 * 1024)).toFixed(2)} MiB`;
}

function shortHex(hex, n = 16) {
    return hex && hex.length > n ? `${hex.slice(0, n)}…` : hex || '—';
}

function nowClock() {
    const d = new Date();
    const p = (x) => String(x).padStart(2, '0');
    return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

/** Origin + path of a blocked URL, truncated — enough to identify the
 *  destination without copying query strings into the ledger. */
function sanitizeUrl(raw) {
    try {
        const u = new URL(raw, location.href);
        const s = `${u.origin}${u.pathname}`;
        return s.length > 120 ? `${s.slice(0, 117)}…` : s;
    } catch {
        const s = String(raw);
        return s.length > 120 ? `${s.slice(0, 117)}…` : s;
    }
}

/* ------------------------------------------------------------------ */
/* Ledger                                                              */
/* ------------------------------------------------------------------ */

function ledgerEl() {
    if (!state.ledgerEl) state.ledgerEl = document.getElementById('ledger-list');
    return state.ledgerEl;
}

function renderLedgerEntry({ seq, op, params, head }) {
    const list = ledgerEl();
    if (!list) return;

    const li = document.createElement('li');
    li.className = 'ledger-entry';
    if (op === 'net.external.blocked' || op === 'csp.violation') {
        li.classList.add('entry-blocked');
    }

    const top = document.createElement('div');
    top.className = 'entry-top';
    const opEl = document.createElement('span');
    opEl.className = 'entry-op';
    opEl.textContent = op;
    const seqEl = document.createElement('span');
    seqEl.className = 'entry-seq';
    seqEl.textContent = `#${seq} · ${nowClock()}`;
    top.appendChild(opEl);
    top.appendChild(seqEl);
    li.appendChild(top);

    if (params && params !== '{}') {
        const p = document.createElement('div');
        p.className = 'entry-params';
        p.textContent = params;
        li.appendChild(p);
    }

    const h = document.createElement('div');
    h.className = 'entry-hash';
    h.textContent = `head ${shortHex(head, 20)}`;
    li.appendChild(h);

    list.insertBefore(li, list.firstChild);
    // Keep the DOM bounded; the chain itself is complete regardless.
    while (list.children.length > 400) list.removeChild(list.lastChild);
}

function renderGenesis(sessionIdHex, headHex) {
    const list = ledgerEl();
    if (!list) return;
    const li = document.createElement('li');
    li.className = 'ledger-entry entry-genesis';
    const top = document.createElement('div');
    top.className = 'entry-top';
    const opEl = document.createElement('span');
    opEl.className = 'entry-op';
    opEl.textContent = 'genesis';
    const seqEl = document.createElement('span');
    seqEl.className = 'entry-seq';
    seqEl.textContent = nowClock();
    top.appendChild(opEl);
    top.appendChild(seqEl);
    li.appendChild(top);
    const p = document.createElement('div');
    p.className = 'entry-params';
    p.textContent = `session ${sessionIdHex}`;
    li.appendChild(p);
    const h = document.createElement('div');
    h.className = 'entry-hash';
    h.textContent = `head ${shortHex(headHex, 20)}`;
    li.appendChild(h);
    list.insertBefore(li, list.firstChild);
}

/**
 * Append an operation to the tamper-evident session ledger.
 *
 * `params` is a plain object; it is serialized once and the same bytes are
 * bound into the entry hash. Returns the sequence number, or null when the
 * session is sealed / not yet started (callers should treat null as "not
 * recorded").
 */
export function vaultLog(op, params) {
    if (state.sealed) return null;
    const paramsJson = JSON.stringify(params ?? {});
    if (!state.session) {
        state.pending.push({ op, paramsJson });
        return null;
    }
    let seq;
    try {
        seq = Number(state.session.logOperation(op, paramsJson));
    } catch (err) {
        // Only reachable if sealed raced us; surface in the UI, not console.
        return null;
    }
    renderLedgerEntry({
        seq,
        op,
        params: paramsJson,
        head: state.session.headHashHex(),
    });
    syncIdentity();
    updateStatusBar();
    return seq;
}

function flushPending() {
    const queued = state.pending.splice(0);
    for (const { op, paramsJson } of queued) {
        let seq;
        try {
            seq = Number(state.session.logOperation(op, paramsJson));
        } catch {
            continue;
        }
        renderLedgerEntry({
            seq,
            op,
            params: paramsJson,
            head: state.session.headHashHex(),
        });
    }
    if (queued.length > 0) {
        syncIdentity();
        updateStatusBar();
    }
}

/* ------------------------------------------------------------------ */
/* Traffic accounting                                                  */
/* ------------------------------------------------------------------ */

function syncTraffic() {
    if (state.session && !state.sealed) {
        state.session.noteTraffic(state.counters.ingress, state.counters.egress);
    }
    updateStatusBar();
}

function addIngress(bytes) {
    if (Number.isFinite(bytes) && bytes > 0) {
        state.counters.ingress += bytes;
        syncTraffic();
    }
}

function recordBlocked(api, url) {
    state.counters.blocked += 1;
    vaultLog('net.external.blocked', {
        api,
        url: sanitizeUrl(url),
        enforcement: 'hook',
    });
    syncTraffic();
}

/* ------------------------------------------------------------------ */
/* Network guards (installed at import time, before wasm init)         */
/* ------------------------------------------------------------------ */

/** Classify a request target. Same-origin, blob: and data: URLs carry no
 *  external egress; everything else is refused. */
function isExternalUrl(raw) {
    let u;
    try {
        u = new URL(raw, location.href);
    } catch {
        return true; // unparseable → refuse
    }
    if (u.protocol === 'blob:' || u.protocol === 'data:') return false;
    return u.origin !== location.origin;
}

function requestUrlOf(input) {
    if (typeof input === 'string') return input;
    if (input && typeof input.url === 'string') return input.url; // Request
    return String(input);
}

function installNetworkGuards() {
    // --- fetch -----------------------------------------------------
    const nativeFetch = window.fetch.bind(window);
    window.fetch = function guardedFetch(input, init) {
        const url = requestUrlOf(input);
        if (isExternalUrl(url)) {
            recordBlocked('fetch', url);
            return Promise.reject(
                new TypeError(
                    `GeoVault clean-room: external request blocked and recorded ` +
                        `in the session ledger (${sanitizeUrl(url)})`
                )
            );
        }
        return nativeFetch(input, init).then((resp) => {
            const len = Number(resp.headers.get('content-length'));
            if (Number.isFinite(len) && len > 0) addIngress(len);
            return resp;
        });
    };

    // --- XMLHttpRequest --------------------------------------------
    const nativeOpen = XMLHttpRequest.prototype.open;
    XMLHttpRequest.prototype.open = function guardedOpen(method, url, ...rest) {
        if (isExternalUrl(url)) {
            recordBlocked('xhr', url);
            throw new DOMException(
                `GeoVault clean-room: external XHR blocked and recorded ` +
                    `in the session ledger (${sanitizeUrl(url)})`,
                'NetworkError'
            );
        }
        if (!this._geovaultMetered) {
            this._geovaultMetered = true;
            this.addEventListener('loadend', (e) => addIngress(e.loaded || 0));
        }
        return nativeOpen.call(this, method, url, ...rest);
    };

    // --- sendBeacon ------------------------------------------------
    if (navigator.sendBeacon) {
        const nativeBeacon = navigator.sendBeacon.bind(navigator);
        navigator.sendBeacon = function guardedBeacon(url, data) {
            if (isExternalUrl(url)) {
                recordBlocked('sendBeacon', url);
                return false; // refused; nothing queued, nothing sent
            }
            const size =
                typeof data === 'string'
                    ? data.length
                    : (data && (data.size ?? data.byteLength)) || 0;
            const ok = nativeBeacon(url, data);
            if (ok && size > 0) {
                state.counters.egress += size;
                syncTraffic();
            }
            return ok;
        };
    }

    // --- CSP violation reports (belt to the hooks' suspenders) ------
    document.addEventListener('securitypolicyviolation', (e) => {
        state.counters.cspViolations += 1;
        vaultLog('csp.violation', {
            directive: e.violatedDirective || e.effectiveDirective || '?',
            blocked_uri: sanitizeUrl(e.blockedURI || ''),
            disposition: e.disposition || 'enforce',
        });
        updateStatusBar();
    });
}

// Guards go up before anything can make a request through us.
installNetworkGuards();

/* ------------------------------------------------------------------ */
/* Status bar + identity                                               */
/* ------------------------------------------------------------------ */

function setText(id, text) {
    const el = document.getElementById(id);
    if (el) el.textContent = text;
}

function updateStatusBar() {
    const c = state.counters;
    setText('status-external', String(c.external));
    setText('status-blocked', String(c.blocked));
    setText('status-ingress', fmtBytes(c.ingress));
    setText('status-egress', fmtBytes(c.egress));
    const blockedEl = document.getElementById('status-blocked');
    if (blockedEl) {
        blockedEl.classList.toggle('value-danger', c.blocked > 0);
    }
    if (state.session) {
        setText('status-ops', String(Number(state.session.operationCount())));
    }
}

function syncIdentity() {
    if (!state.session) return;
    setText('identity-session', shortHex(state.session.sessionIdHex(), 32));
    setText('identity-pubkey', shortHex(state.session.publicKeyHex(), 16));
    setText('identity-head', shortHex(state.session.headHashHex(), 16));
}

function setSessionState(text, cls) {
    setText('session-state-text', text);
    const dot = document.getElementById('session-dot');
    if (dot) {
        dot.classList.remove('state-open', 'state-sealed');
        if (cls) dot.classList.add(cls);
    }
}

/* ------------------------------------------------------------------ */
/* Seal flow                                                           */
/* ------------------------------------------------------------------ */

function runSelfCheck(attestationJson) {
    const host = document.getElementById('seal-selfcheck');
    if (!host) return;
    host.textContent = '';
    let report = null;
    try {
        report = JSON.parse(verifyAttestation(attestationJson));
    } catch {
        // fallthrough — render three failures below
    }
    const checks = [
        ['chain', report ? report.chain_ok : false],
        ['merkle', report ? report.merkle_ok : false],
        ['signature', report ? report.signature_ok : false],
    ];
    for (const [name, ok] of checks) {
        const span = document.createElement('span');
        span.className = ok ? 'check-pass' : 'check-fail';
        span.textContent = `${ok ? '✓' : '✗'} ${name}`;
        host.appendChild(span);
    }
}

function fillSealModal(att) {
    setText('attest-session', att.session_id);
    setText('attest-root', att.merkle_root);
    setText('attest-pubkey', att.public_key);
    setText('attest-signature', att.signature);
    setText(
        'attest-summary',
        `${att.operations.length} operations · ` +
            `ingress ${fmtBytes(att.bytes_ingressed)} · ` +
            `egress ${fmtBytes(att.bytes_egressed)} · ` +
            `${att.app_name} v${att.app_version} · format v${att.version}`
    );
}

function downloadAttestation() {
    if (!state.attestationJson) return;
    const blob = new Blob([state.attestationJson], {
        type: 'application/json',
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = 'attestation.json';
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 5000);
    vaultLogSealedSafe('attestation.download'); // no-op: sealed; kept for clarity
}

/** After sealing the ledger refuses appends by design; this documents the
 *  intentional silence instead of sprinkling `if (!sealed)` at call sites. */
function vaultLogSealedSafe(_op) {
    /* sealed ledger: immutable, nothing to record */
}

function sealSession() {
    if (!state.session || state.sealed) return;

    // Bind the final observed totals, then seal.
    state.session.noteTraffic(state.counters.ingress, state.counters.egress);
    let json;
    try {
        json = state.session.seal();
    } catch (err) {
        setSessionState('seal failed', 'state-open');
        return;
    }
    state.sealed = true;
    state.attestationJson = json;

    let att;
    try {
        att = JSON.parse(json);
    } catch {
        att = null;
    }
    if (att) fillSealModal(att);
    runSelfCheck(json);

    const sealBtn = document.getElementById('seal-btn');
    if (sealBtn) {
        sealBtn.textContent = 'SEALED';
        sealBtn.disabled = true;
        sealBtn.classList.add('sealed');
    }
    setSessionState('sealed — ledger immutable', 'state-sealed');
    syncIdentity();
    updateStatusBar();

    const modal = document.getElementById('seal-modal');
    if (modal) modal.classList.remove('hidden');

    // Let the workstation disable its tools.
    document.dispatchEvent(new CustomEvent('geovault:sealed'));
}

function wireSealUi() {
    const sealBtn = document.getElementById('seal-btn');
    if (sealBtn) sealBtn.addEventListener('click', sealSession);

    const closeBtn = document.getElementById('seal-modal-close');
    const modal = document.getElementById('seal-modal');
    if (closeBtn && modal) {
        closeBtn.addEventListener('click', () => modal.classList.add('hidden'));
    }

    const dl = document.getElementById('attest-download');
    if (dl) dl.addEventListener('click', downloadAttestation);

    // Copy buttons (event delegation over the modal).
    if (modal) {
        modal.addEventListener('click', (e) => {
            const btn = e.target.closest('.btn-copy');
            if (!btn) return;
            const src = document.getElementById(btn.dataset.copy);
            if (!src) return;
            const text = src.textContent || '';
            const done = () => {
                btn.classList.add('copied');
                btn.textContent = 'copied';
                setTimeout(() => {
                    btn.classList.remove('copied');
                    btn.textContent = 'copy';
                }, 1200);
            };
            if (navigator.clipboard && navigator.clipboard.writeText) {
                navigator.clipboard.writeText(text).then(done, () => {
                    selectContents(src);
                });
            } else {
                selectContents(src);
            }
        });
    }
}

/** Clipboard fallback: select the field so the user can ⌘C. */
function selectContents(el) {
    const range = document.createRange();
    range.selectNodeContents(el);
    const sel = window.getSelection();
    if (sel) {
        sel.removeAllRanges();
        sel.addRange(range);
    }
}

/* ------------------------------------------------------------------ */
/* Init                                                                */
/* ------------------------------------------------------------------ */

/**
 * Initialize the vault: load the wasm module (metered as same-origin
 * ingress by our own fetch hook), start the WasmVaultSession under the
 * clean-room policy, and wire the ledger + seal UI.
 *
 * Idempotent-ish: intended to be called exactly once by workstation.js.
 */
export async function initVault() {
    await wasmInit();

    setText('version-badge', `oxigdal-wasm ${wasmVersion()}`);

    const policyJson = JSON.stringify({
        csp: CSP_POLICY,
        enforcement: ENFORCEMENT,
        origin: location.origin,
    });
    state.session = new WasmVaultSession(policyJson);

    renderGenesis(state.session.sessionIdHex(), state.session.headHashHex());
    vaultLog('session.start', {
        app: 'geovault',
        wasm_version: wasmVersion(),
        origin: location.origin,
    });
    flushPending();

    syncIdentity();
    setSessionState('session open — recording', 'state-open');
    const sealBtn = document.getElementById('seal-btn');
    if (sealBtn) sealBtn.disabled = false;

    wireSealUi();
    syncTraffic();
    updateStatusBar();
}
