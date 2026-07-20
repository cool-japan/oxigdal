# oxigeo-wasm-geoparquet

WebAssembly bindings for querying remote GeoParquet files directly from the
browser — predicate pushdown over HTTP range requests, no server component.

Part of [OxiGeo](https://github.com/cool-japan/oxigeo), the Pure-Rust
geospatial data library by COOLJAPAN OU (Team Kitasan).

## What it does

- Fetches only the Parquet footer, then plans bounding-box and attribute
  predicates against row-group metadata (GeoParquet 1.1 `covering.bbox`
  aware, with plain `bbox` struct fallback).
- Downloads only the surviving column chunks, coalescing byte ranges into
  a handful of HTTP range requests.
- Decodes with the pure-Rust `parquet`/`arrow` stack (SNAPPY via the
  pure-Rust `snap` codec) and returns GeoJSON to JavaScript.

## Build

```bash
wasm-pack build crates/oxigeo-wasm-geoparquet --target web --release
```

## License

Apache-2.0
