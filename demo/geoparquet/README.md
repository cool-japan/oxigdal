# GeoParquet Live — query 5.9 GB in your browser

Query a dataset bigger than your laptop, over the network, with no database.

This demo points the browser at the [VIDA google-microsoft-osm-open-buildings](https://source.coop/vida/google-microsoft-osm-open-buildings)
GeoParquet for Japan — **5.9 GB, 47.6 million building footprints, 9,533 Parquet
row groups** — and answers bounding-box + attribute queries by downloading only
the byte ranges that matter:

1. **Open** — the 17 MB Parquet footer is fetched once (Cache API makes reloads
   instant) and decoded inside WebAssembly.
2. **Plan** — while you drag a query box, row groups are pruned against their
   bbox and column statistics *from metadata alone*: the strip at the bottom
   shows every row group (grey = pruned, amber = plan survivor, green = bytes
   actually fetched).
3. **Query** — only the surviving column chunks are downloaded via coalesced
   HTTP range requests, decoded with the pure-Rust `parquet`/`arrow` stack
   (SNAPPY via pure-Rust `snap`), filtered, and rendered as confidence-colored
   GeoJSON on a Leaflet canvas.

Everything runs client-side: `dataset: 5.9 GB · fetched: a few MB · uploaded: 0 · server: none`.

## Run locally

```bash
./build.sh                      # wasm-pack build (once; ./pkg is a symlink)
python3 serve.py 8080           # range-request-capable static server
# open http://127.0.0.1:8080/
```

> **Why `serve.py`?** Stock `python3 -m http.server` ignores `Range` headers
> and returns the whole file with `200 OK`; the WASM fetch layer rejects that
> as a length mismatch (it never trusts more bytes than it asked for).
> `serve.py` is a ~100-line drop-in that answers `206 Partial Content`.
> Any production static host (S3, Cloudflare, nginx) already supports ranges.

### Offline sample

No network? A 210 KB, 3-row-group synthetic Tokyo sample ships with the demo:

```
http://127.0.0.1:8080/?src=./sample/jpn-sample.parquet
```

The sample mirrors the live file's layout (SNAPPY, plain `bbox` struct +
GeoParquet 1.1 `covering` metadata; extent lon 139.40–139.72, lat 35.43–35.72).
The **Shinjuku** example box hits exactly row group 0 of 3.

## Attribute filters

The filter box accepts a SQL `WHERE` fragment, parsed with `sqlparser` and
lowered to Parquet predicate pushdown. Supported grammar (from
`crates/oxigdal-wasm-geoparquet/src/filter_expr.rs`):

| Expression                    | Lowered to                 |
|-------------------------------|----------------------------|
| `col = lit` / `lit = col`     | equality filter            |
| `col <> lit`, `col != lit`    | comparison (`NotEq`)       |
| `col > lit`                   | comparison (`Gt`)          |
| `col >= lit`                  | comparison (`Ge`)          |
| `col < lit`                   | comparison (`Lt`)          |
| `col <= lit`                  | comparison (`Le`)          |
| `col BETWEEN lo AND hi`       | range filter               |
| `col IN (lit, ...)`           | set-membership filter      |
| `pred AND pred AND ...`       | flattened conjunction      |

Literals: integers, decimals (`0.8`, `1e3`), single-quoted strings
(`bf_source = 'google'`), `TRUE`/`FALSE`. Reversed operand order
(`1000 < area_in_meters`) is normalized automatically.

> **Numeric types:** the predicate engine coerces each numeric literal to its
> target column's type, so a bare integer compares correctly against a Float64
> column (`area_in_meters > 500`) and a whole-valued decimal compares against an
> integer column — write whichever reads naturally.

**Not supported** (rejected with an error naming the construct): `OR`, `NOT`,
`IS NULL`, function calls, column-to-column comparisons, subqueries, and
trailing clauses (`ORDER BY`, `LIMIT`, ...). Double quotes denote identifiers
in SQL — string literals need single quotes.

Useful columns in the VIDA file: `area_in_meters` (Float64), `confidence`
(Float64), `bf_source` (Utf8: `google` / `microsoft` / `osm`), `geohash`,
`s2_id`, `boundary_id`.

## Honesty guards

- `plan()` previews the exact row-group count, byte estimate, and request
  count *before* any data byte is fetched.
- Queries scanning more than **max row groups** (default 64) are refused with
  a `too_broad` error instead of silently downloading hundreds of MB.
- The row limit (default 60,000) shows an explicit "row limit reached" banner
  when it truncates — never a silently incomplete answer.
- The fetched-bytes badge counts every data request (footer, probes, chunks);
  app assets (WASM/JS/CSS) are excluded.

## Files

| File            | Purpose                                                  |
|-----------------|----------------------------------------------------------|
| `index.html`    | layout: sidebar, map, row-group strip, honesty badges    |
| `main.js`       | open/plan/query flow, Cache API footer cache, rendering  |
| `map-draw.js`   | hand-rolled rectangle drawing (no leaflet-draw)          |
| `rg-strip.js`   | canvas strip of all row groups (the pruning money shot)  |
| `examples.json` | preset boxes + filters (Shinjuku, Shibuya, Osaka, ...)  |
| `serve.py`      | local static server with HTTP Range support              |
| `build.sh`      | wasm-pack build + npm package name rewrite (idempotent)  |
| `pkg` → symlink | `crates/oxigdal-wasm-geoparquet/pkg` build output        |

Part of [OxiGDAL](https://github.com/cool-japan/oxigdal) — Pure-Rust geospatial
data access by COOLJAPAN OU (Team Kitasan). License: Apache-2.0.
