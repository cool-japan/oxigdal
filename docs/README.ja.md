# OxiGDAL ドキュメント

> English version: [README.md](README.md)

## Getting Started

- [Quickstart](QUICKSTART.md) — 5分で始める OxiGDAL
- [Getting Started](GETTING_STARTED.md) — インストールから基本操作まで

## Guides

- [Architecture](ARCHITECTURE.md) — システム全体のアーキテクチャ
- [Drivers](DRIVERS.md) — 対応フォーマット (GeoTIFF/COG, GeoJSON, GeoParquet, Zarr, FlatGeobuf, Shapefile, NetCDF, HDF5, GRIB, JPEG2000, VRT — 全11種対応)
- [Algorithms](ALGORITHMS.md) — リサンプリング、ラスタ/ベクタ演算
- [Performance Guide](PERFORMANCE_GUIDE.md) — パフォーマンス最適化
- [WASM Guide](WASM_GUIDE.md) — WebAssembly ビルドとブラウザ利用
- [Best Practices](BEST_PRACTICES.md) — エラーハンドリング、メモリ管理等

## Migration

- [Migration from GDAL](MIGRATION_FROM_GDAL.md) — GDAL/OGR からの移行ガイド
- [API Comparison](API_COMPARISON.md) — GDAL C++/Python vs OxiGDAL 対応表
- [Python to Rust](PYTHON_TO_RUST.md) — Python ジオ開発者向け Rust 入門

## Cookbook

- [Raster Recipes](cookbook/raster_recipes.md)
- [Vector Recipes](cookbook/vector_recipes.md)
- [Cloud Recipes](cookbook/cloud_recipes.md)
- [Format Conversion](cookbook/format_conversion.md)

## Tutorials

1. [Getting Started](tutorials/01_getting_started.md)
2. [Reading Rasters](tutorials/02_reading_rasters.md)
3. [Raster Operations](tutorials/03_raster_operations.md)
4. [Vector Data](tutorials/04_vector_data.md)
5. [Projections](tutorials/05_projections.md)
6. [Cloud Storage](tutorials/06_cloud_storage.md)

## Troubleshooting

- [Troubleshooting](TROUBLESHOOTING.md) — よくある問題と解決策

## Developer Tooling

- [Fail Test Detection](FAIL_TEST_DETECTION.md) — 自動テスト失敗検出・修正システム
- [Fail Test Quick Start](FAIL_TEST_DETECTION_QUICKSTART.md) — 5分でセットアップ
