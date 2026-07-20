# Vendored Third-Party Assets (WP-D3a)

Zero-CDN house ethos: the demo must not fetch runtime code from `unpkg.com` or
`cdn.jsdelivr.net`. This directory contains every third-party file the demo
needs, downloaded with `npm pack <pkg>@<version>` from the official npm
registry tarball, extracted, and copied here unmodified (see "Integrity"
below). Nothing in `vendor/` was hand-edited except `topojson-client`, which
required a build-format decision explained in its own section.

This document is the map the wiring agent (the one editing `index.html` /
`main.js`) needs: for every existing CDN URL, the exact local path that
replaces it.

Method used for all five packages:

```
npm pack <pkg>@<version>        # downloaded into the session scratchpad, NOT into the repo
tar -xzf <pkg>-<version>.tgz
cp <only the needed dist/esm files> demo/cog-viewer/vendor/<pkg>/
```

No `package.json`, `node_modules/`, or build tooling was added to the repo —
`demo/cog-viewer` stays a plain ES-module project with no bundler, matching
`package.json`'s existing `"type": "module"` / no-devDependency-bundler
posture and the comment already in `main.js` (search
`plain ES-module project`).

---

## 1. Leaflet 1.9.4

- Source: `https://registry.npmjs.org/leaflet/-/leaflet-1.9.4.tgz`
- shasum: `23fae724e282fa25745aff82ca4d394748db7d8d`
- License: BSD-2-Clause (`vendor/leaflet/LICENSE`, copied from the package)
- Files copied (from the tarball's `dist/`):
  - `leaflet.js` (148 KB) — `dist/leaflet.js`
  - `leaflet.css` (16 KB) — `dist/leaflet.css`
  - `images/layers.png`, `images/layers-2x.png`, `images/marker-icon.png`,
    `images/marker-icon-2x.png`, `images/marker-shadow.png` — `dist/images/*`
    (all 5 copied; `leaflet.css` only `url()`-references 3 of them —
    `layers.png`, `layers-2x.png`, `marker-icon.png` — but
    `marker-icon-2x.png` and `marker-shadow.png` are referenced from
    `L.Icon.Default` in `leaflet.js` itself, so all 5 are required)
- Verified byte-identical to the npm tarball via `diff -q`.

**URL replacement map:**

| Old CDN URL | New local path |
|---|---|
| `https://unpkg.com/leaflet@1.9.4/dist/leaflet.css` (index.html:9, has `integrity`/`crossorigin`) | `./vendor/leaflet/leaflet.css` — **drop** the `integrity`/`crossorigin` attributes, they only apply to cross-origin fetches |
| `https://unpkg.com/leaflet@1.9.4/dist/leaflet.js` (index.html:352, has `integrity`/`crossorigin`) | `./vendor/leaflet/leaflet.js` — same, drop `integrity`/`crossorigin` |

No JS changes needed beyond the `<link>`/`<script src>` attribute swap —
`leaflet.js` is a classic UMD build that sets `window.L`, loaded via a plain
(non-module) `<script>` tag exactly as today.

---

## 2. FlatGeobuf 4.4.0

- Source: `https://registry.npmjs.org/flatgeobuf/-/flatgeobuf-4.4.0.tgz`
- shasum: `f2067f359ed35a5bd6a19e0cde1a3cd2d16d6c37`
- License: BSD-3-Clause (`vendor/flatgeobuf/LICENSE`)
- Version confirmed to match `index.html:356` exactly (`flatgeobuf@4.4.0`) —
  no change needed to the pin.
- File copied: `flatgeobuf-geojson.min.js` (56 KB) —
  `dist/flatgeobuf-geojson.min.js`
- Verified byte-identical via `diff -q`.

**URL replacement map:**

| Old CDN URL | New local path |
|---|---|
| `https://unpkg.com/flatgeobuf@4.4.0/dist/flatgeobuf-geojson.min.js` (index.html:356) | `./vendor/flatgeobuf/flatgeobuf-geojson.min.js` |

Also update the two hint strings in `main.js` (lines ~1228, `loadFlatGeobuf`)
that tell the user which `<script>` tag to add if the library is missing —
purely cosmetic error-message text, not executed code.

UMD build, sets `window.flatgeobuf`, loaded via plain `<script>` — no format
conversion needed.

---

## 3. Shapefile 0.6.6

- Source: `https://registry.npmjs.org/shapefile/-/shapefile-0.6.6.tgz`
- shasum: `6fee152b9fb2b1c85f690285b692fb68c95a5f4f`
- License: BSD-3-Clause (`vendor/shapefile/LICENSE.txt`, upstream's own
  filename)
- File copied: `shapefile.min.js` (8 KB) — `dist/shapefile.min.js`
- Verified byte-identical via `diff -q`.

**URL replacement map:**

| Old CDN URL | New local path |
|---|---|
| `https://unpkg.com/shapefile@0.6.6/dist/shapefile.min.js` (index.html:357) | `./vendor/shapefile/shapefile.min.js` |

Also update the hint string in `main.js` (line ~1276, `loadShapefile`).

UMD build, sets `window.shapefile`, loaded via plain `<script>` — no format
conversion needed.

---

## 4. topojson-client 3 — DEVIATION FROM THE LITERAL WP-D3a WORDING

WP-D3a asked for "the client ESM file" vendored so it's importable as
`import('./vendor/topojson-client.esm.js')`. After inspecting the actual npm
package, **that file does not exist upstream** and the literal request can't
be satisfied by a straight download — see below. I implemented the closest
correct equivalent instead. Recording this as a deviation per instructions.

**What the npm package actually ships** (checked `topojson-client@3.1.0`,
the newest 3.x, since `main.js` pins only the major, `@3`):
- `dist/topojson-client.js` / `dist/topojson-client.min.js` — a **UMD**
  bundle (`(function(global,factory){...}(this, function(exports){...`),
  not an ES module. It has zero `export` statements.
- `src/index.js` + 12 sibling files — the *real* ESM source
  (`package.json`'s `"module": "src/index.js"` field), completely
  self-contained (no dependency on `commander`, which is only used by the
  package's CLI `bin/` scripts, not by the client API).
- There is no single bundled `*.esm.js` file in the published package at
  all — that filename was never published by upstream for this package.

**Why the current CDN code (`main.js` lines ~1586-1594) is already broken,
independent of CDN-vs-vendor:** it does
`topojson = await import('https://cdn.jsdelivr.net/.../dist/topojson-client.min.js')`.
Dynamic `import()` treats the fetched file as an ES module. The UMD file has
no `export` statements, so the resulting module namespace object has no
`feature` property — `topojson.feature(...)` on the next line would throw
`TypeError: topojson.feature is not a function`. (The UMD wrapper's fallback
branch sets `self.topojson = {...}` as a side effect instead, since
top-level `this` is `undefined` in a module and falls through to
`global || self`.) This is a pre-existing bug unrelated to vendoring — vendoring
the UMD file verbatim would just reproduce the same bug locally.

**What I vendored instead:** the unmodified `src/*.js` ESM sources (13
files, byte-identical to the tarball, verified via `diff -q`), which are
natively browser-loadable via relative `import` — no bundler needed, since
every cross-file import in the package is a same-directory relative path
(`./bbox.js`, `./transform.js`, etc.). This mirrors exactly what upstream's
own `"module"` entry point designates for ESM consumers.

Files in `vendor/topojson-client/`: `index.js` (entry), `bbox.js`,
`bisect.js`, `feature.js`, `identity.js`, `merge.js`, `mesh.js`,
`neighbors.js`, `quantize.js`, `reverse.js`, `stitch.js`, `transform.js`,
`untransform.js`, `LICENSE` (ISC).

- Source: `https://registry.npmjs.org/topojson-client/-/topojson-client-3.1.0.tgz`
- shasum: `22e8b1ed08a2b922feeb4af6f53b6ef09a467b99`

**URL replacement map:**

| Old CDN URL | New local path |
|---|---|
| `https://cdn.jsdelivr.net/npm/topojson-client@3/dist/topojson-client.min.js` (main.js:1587, primary) | `./vendor/topojson-client/index.js` |
| `https://unpkg.com/topojson-client@3/dist/topojson-client.min.js` (main.js:1593, fallback) | delete the fallback branch entirely — there is only one local copy now |

The wiring agent must change
`import('https://cdn.jsdelivr.net/...')` to
`import('./vendor/topojson-client/index.js')` — a **named-export** ESM
module (`export {default as bbox} from ...`, etc.), so
`topojson.feature(topology, topoObj)` (main.js:1608) keeps working exactly
as the code already assumes; that call was already coded against a
named-export shape, it just never actually got one from the CDN UMD file
before now.

---

## 5. parquet-wasm — DEVIATION FROM THE LITERAL WP-D3a WORDING

WP-D3a said "pin a concrete 0.6.x version; vendor `esm/arrow2.js` +
`arrow2_bg.wasm`". I pinned **0.6.1** (the newest stable 0.6.x on npm), but
that version's ESM entry point is **not** named `arrow2.js` — recording why
below, since this changes the import filename the wiring agent must use.

**Package layout history** (checked every 0.6.x tag on npm):
- `parquet-wasm@0.4.x` / `0.5.0` / `0.6.0-beta.1`: the package shipped
  **two parallel Rust-side Arrow implementations** as separate ESM bundles,
  `esm/arrow1.js` (+`arrow1_bg.wasm`, `arrow` crate) and `esm/arrow2.js`
  (+`arrow2_bg.wasm`, the now-abandoned `arrow2` crate) — this is where the
  `esm/arrow2.js` filename in the WP-D3a brief and in `main.js`'s current
  CDN URL (`parquet-wasm@latest/esm/arrow2.js`) comes from.
- Starting at `0.6.0-beta.2` and continuing through stable `0.6.0`, `0.6.1`,
  and every version through current `0.7.2`, upstream **consolidated to a
  single ESM bundle**: `esm/parquet_wasm.js` + `esm/parquet_wasm_bg.wasm`.
  `arrow2.js` does not exist in `0.6.0-beta.2` or any release after it.
- Consequence: `main.js`'s current CDN URL
  (`https://cdn.jsdelivr.net/npm/parquet-wasm@latest/esm/arrow2.js`) is
  **already broken today** — `@latest` resolves to `0.7.2`, which 404s on
  `esm/arrow2.js`. This is a live, independently-verifiable bug (confirmed
  by listing the tarball contents of every 0.6.x/0.7.x release), not
  something introduced by vendoring.

**Version choice:** the only "0.6.x"-tagged release that still has
`arrow2.js` is the prerelease `0.6.0-beta.1`. I did **not** vendor that —
pinning a demo to an abandoned beta (superseded by the stable `0.6.0`
release three versions later) contradicts the "pin a concrete, real
version" intent of the brief. I vendored **`0.6.1`** instead: the newest
*stable* 0.6.x, using its actual (and only) ESM entry point.

- Source: `https://registry.npmjs.org/parquet-wasm/-/parquet-wasm-0.6.1.tgz`
- shasum: `0877bc5a1f48546c63b4d47eb450e98e367bd5ea`
- License: MIT OR Apache-2.0 (`vendor/parquet-wasm/LICENSE_MIT`,
  `LICENSE_APACHE`, both copied from the package)
- Files copied (from the tarball's `esm/`):
  - `parquet_wasm.js` (112 KB) — `esm/parquet_wasm.js`
  - `parquet_wasm_bg.wasm` (**5.2 MB**) — `esm/parquet_wasm_bg.wasm`
  - `.d.ts` type-declaration files were **not** copied (TypeScript-only,
    irrelevant to this plain-JS demo, would add dead weight)
- Verified byte-identical via `diff -q`.

**DECISION GATE — WASM SIZE (flagged prominently as instructed):**
`parquet_wasm_bg.wasm` is **5.2 MB uncompressed**, well over the ~2 MB
threshold in the brief. I vendored it anyway per the brief's own fallback
instruction ("still vendor it ... Cloudflare serves brotli"). For context:
upstream's own README states the full read+write+all-codecs build is
"1.2 MB brotli-compressed" and a minimal read-only build can be as small as
"456 KB brotli-compressed" — so the wire size under Cloudflare's brotli
transport should land well under 2 MB even though the file on disk here is
5.2 MB. The staging/wiring agent should decide whether:
(a) ship 0.6.1 as-is (simplest, matches what's vendored here), or
(b) request a custom minimal build (upstream supports building a
read-only/no-codec bundle, see its README "Custom builds" section) to
shrink this further — that would require a Rust+`wasm-pack` build step this
work package (vendoring only) does not perform.
Every other 0.6.x/beta build I checked (including the `arrow2_bg.wasm` from
`0.6.0-beta.1`, ~4.8 MB) is in the same multi-MB range, so version choice
does not materially change this trade-off.

**URL replacement map:**

| Old CDN URL | New local path |
|---|---|
| `https://cdn.jsdelivr.net/npm/parquet-wasm@latest/esm/arrow2.js` (main.js:1653, primary) | `./vendor/parquet-wasm/parquet_wasm.js` — **note the filename change**, not `arrow2.js` |
| `https://unpkg.com/parquet-wasm/esm/arrow2.js` (main.js:1658, fallback) | delete the fallback branch entirely — there is only one local copy now |

API compatibility for the wiring agent: `0.6.1`'s `esm/parquet_wasm.js`
still exports a default init function (`export default function __wbg_init`)
and `readParquet(uint8Array)` (returns a `Table`), matching the shape
`main.js` already expects (`parquetModule.default` check at main.js:1669,
`parquetModule.readParquet(uint8)` at main.js:1677) — no call-site logic
changes needed beyond the import path itself.

The vendored `parquet_wasm.js` resolves its own `.wasm` file via
`new URL('parquet_wasm_bg.wasm', import.meta.url)` (see line 3314) when
`wasmInit()` is called with no arguments — since both files sit side by
side in `vendor/parquet-wasm/`, **no explicit wasm URL override is
needed**, `await parquetModule.default()` will just work.

---

## Integrity verification performed

```
find vendor -type f -empty                 # → no output (nothing empty)
diff -q <npm tarball file> <vendor file>    # → run for every copied file, all identical
```

All checks passed; see file-by-file notes above for shasums of the source
tarballs.

## Summary table

| Package | Pinned version | Was `@latest`/unpinned before? | Files | On-disk size |
|---|---|---|---|---|
| leaflet | 1.9.4 (unchanged) | no, already pinned | 8 | 176 KB |
| flatgeobuf | 4.4.0 (unchanged) | no, already pinned | 2 | 60 KB |
| shapefile | 0.6.6 (unchanged) | no, already pinned | 2 | 12 KB |
| topojson-client | 3.1.0 (newly pinned to exact patch) | pinned to `@3` (floating minor/patch) | 14 | 56 KB |
| parquet-wasm | 0.6.1 (newly pinned) | **yes, `@latest`** | 4 | 5.3 MB |

Grand total: `du -sh vendor/` → **5.7 MB**.

## Not yet done (explicitly out of scope for this work package)

`index.html` and `main.js` still point at the CDN URLs above. Per WP-D3a
scope, this package only vendors/documents — it does not edit those two
files. The wiring agent should apply the "URL replacement map" tables
above.
