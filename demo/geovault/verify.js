/**
 * GeoVault — verify.js
 *
 * Independent attestation verifier. Loads an attestation.json (file drop,
 * file picker, or pasted text) and re-verifies it from the JSON alone with
 * `verifyAttestation` — the same oxigeo-security code the workstation
 * used, recomputing the blake3 hash chain, the Merkle root, and the
 * Ed25519 seal signature. Entirely client-side; this page makes no network
 * requests beyond its own same-origin assets.
 */

import wasmInit, { verifyAttestation, version as wasmVersion } from './pkg/oxigeo_wasm.js';

function $(id) {
    return document.getElementById(id);
}

function setCheck(id, ok) {
    const row = $(id);
    if (!row) return;
    row.classList.remove('pass', 'fail');
    row.classList.add(ok ? 'pass' : 'fail');
    const mark = row.querySelector('.check-mark');
    if (mark) mark.textContent = ok ? '✓' : '✗';
}

function resetChecks() {
    for (const id of ['check-chain', 'check-merkle', 'check-signature']) {
        const row = $(id);
        if (!row) continue;
        row.classList.remove('pass', 'fail');
        const mark = row.querySelector('.check-mark');
        if (mark) mark.textContent = '·';
    }
}

function renderReport(pairs) {
    const dl = $('verify-report');
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
}

function renderOps(attestation) {
    const body = $('ops-body');
    if (!body) return;
    body.textContent = '';
    const ops = attestation && Array.isArray(attestation.operations)
        ? attestation.operations
        : [];
    if (ops.length === 0) {
        const tr = document.createElement('tr');
        const td = document.createElement('td');
        td.colSpan = 5;
        td.textContent = attestation ? 'empty log (sealed with zero operations)' : '—';
        tr.appendChild(td);
        body.appendChild(tr);
        return;
    }
    for (const op of ops) {
        const tr = document.createElement('tr');
        const cells = [
            String(op.seq),
            new Date(Number(op.ts_ms)).toISOString().replace('T', ' ').slice(0, 19),
            op.op,
            op.params && op.params.length > 96 ? `${op.params.slice(0, 93)}…` : op.params,
            `${String(op.entry_hash).slice(0, 16)}…`,
        ];
        cells.forEach((text, i) => {
            const td = document.createElement('td');
            if (i !== 2) td.classList.add('mono');
            td.textContent = text;
            tr.appendChild(td);
        });
        body.appendChild(tr);
    }
}

function setVerdict(text, cls) {
    const v = $('verify-verdict');
    if (!v) return;
    v.classList.remove('hidden', 'pass', 'fail');
    if (cls) v.classList.add(cls);
    v.textContent = text;
}

function runVerification(json) {
    resetChecks();

    let report;
    try {
        report = JSON.parse(verifyAttestation(json));
    } catch (err) {
        setCheck('check-chain', false);
        setCheck('check-merkle', false);
        setCheck('check-signature', false);
        setVerdict('✗ NOT A VALID ATTESTATION', 'fail');
        renderReport([
            ['status', 'malformed input'],
            ['detail', String(err && err.message ? err.message : err)],
        ]);
        renderOps(null);
        return;
    }

    setCheck('check-chain', report.chain_ok);
    setCheck('check-merkle', report.merkle_ok);
    setCheck('check-signature', report.signature_ok);

    const allOk = report.chain_ok && report.merkle_ok && report.signature_ok;
    setVerdict(
        allOk
            ? '✓ RECORD INTACT — chain, Merkle root and signature all verify'
            : '✗ RECORD TAMPERED OR CORRUPT — at least one check failed',
        allOk ? 'pass' : 'fail'
    );

    renderReport([
        ['session', report.session_id],
        ['entries', String(report.entry_count)],
        ['bytes egressed (reported)', String(report.bytes_egressed)],
        ['public key', report.public_key],
        ['chain', report.chain_ok ? 'ok' : 'BROKEN'],
        ['merkle root', report.merkle_ok ? 'ok' : 'MISMATCH'],
        ['signature', report.signature_ok ? 'ok' : 'INVALID'],
    ]);

    // The operations table is rendered from the (untrusted) attestation
    // body itself — the checks above are what make it trustworthy.
    let attestation = null;
    try {
        attestation = JSON.parse(json);
    } catch {
        /* unreachable: verifyAttestation parsed it already */
    }
    renderOps(attestation);
}

function currentText() {
    const ta = $('verify-text');
    return ta ? ta.value.trim() : '';
}

async function handleFile(file) {
    const text = await file.text();
    const ta = $('verify-text');
    if (ta) ta.value = text;
    $('verify-run').disabled = text.trim().length === 0;
    runVerification(text.trim());
}

async function boot() {
    await wasmInit();
    const badge = $('version-badge');
    if (badge) badge.textContent = `oxigeo-wasm ${wasmVersion()}`;

    const runBtn = $('verify-run');
    const ta = $('verify-text');
    const fileInput = $('verify-file');
    const drop = $('verify-drop');

    if (ta) {
        ta.addEventListener('input', () => {
            runBtn.disabled = currentText().length === 0;
        });
    }
    runBtn.addEventListener('click', () => {
        const text = currentText();
        if (text) runVerification(text);
    });

    if (fileInput) {
        fileInput.addEventListener('change', (e) => {
            const file = e.target.files && e.target.files[0];
            if (file) handleFile(file);
            e.target.value = '';
        });
    }
    if (drop) {
        drop.addEventListener('dragover', (e) => {
            e.preventDefault();
            drop.classList.add('drop-hover');
        });
        drop.addEventListener('dragleave', () => drop.classList.remove('drop-hover'));
        drop.addEventListener('drop', (e) => {
            e.preventDefault();
            drop.classList.remove('drop-hover');
            const file = e.dataTransfer && e.dataTransfer.files && e.dataTransfer.files[0];
            if (file) handleFile(file);
        });
    }
}

boot();
