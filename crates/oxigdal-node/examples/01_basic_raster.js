/**
 * Example 01: Basic Raster Operations
 *
 * Demonstrates:
 * - Opening a raster dataset
 * - Reading metadata
 * - Reading bands
 * - Basic statistics
 * - Writing output
 */

const oxigdal = require('../');
const path = require('path');

async function main() {
  console.log('=== OxiGDAL Basic Raster Example ===\n');

  // Display version info
  const info = oxigdal.getInfo();
  console.log(`Version: ${info.version}`);
  console.log(`Formats: ${info.formats.join(', ')}\n`);

  // Create a simple test raster
  console.log('Creating test raster...');
  const width = 100;
  const height = 100;
  const dataset = oxigdal.createRaster(width, height, 3, 'float32');

  // Set geo transform
  dataset.setGeoTransform([
    -180.0,  // origin X
    3.6,     // pixel width
    0.0,     // rotation X
    90.0,    // origin Y
    0.0,     // rotation Y
    -1.8     // pixel height
  ]);

  // Set CRS
  dataset.crs = 'EPSG:4326';

  // Create and fill bands with data
  for (let bandIdx = 0; bandIdx < 3; bandIdx++) {
    const buffer = new oxigdal.BufferWrapper(width, height, 'float32');

    // Fill with a pattern
    for (let y = 0; y < height; y++) {
      for (let x = 0; x < width; x++) {
        const value = Math.sin(x / 10.0) * Math.cos(y / 10.0) * (bandIdx + 1) * 100;
        buffer.setPixel(x, y, value);
      }
    }

    dataset.writeBand(bandIdx, buffer);
    console.log(`Band ${bandIdx + 1} filled with data`);
  }

  // Read metadata
  const metadata = dataset.getMetadata();
  console.log('\nDataset Metadata:');
  console.log(`  Size: ${metadata.width}x${metadata.height}`);
  console.log(`  Bands: ${metadata.bandCount}`);
  console.log(`  Data Type: ${metadata.dataType}`);
  console.log(`  CRS: ${metadata.crs}`);

  if (metadata.bounds) {
    console.log(`  Bounds: [${metadata.bounds.minX}, ${metadata.bounds.minY}, ${metadata.bounds.maxX}, ${metadata.bounds.maxY}]`);
  }

  // Read first band and compute statistics
  console.log('\nBand 1 Statistics:');
  const band = dataset.readBand(0);
  const stats = band.statistics();
  console.log(`  Min: ${stats.min.toFixed(2)}`);
  console.log(`  Max: ${stats.max.toFixed(2)}`);
  console.log(`  Mean: ${stats.mean.toFixed(2)}`);
  console.log(`  StdDev: ${stats.stddev.toFixed(2)}`);
  console.log(`  Count: ${stats.count}`);

  // Test pixel access
  console.log('\nSample Pixels (band 1):');
  const samplePoints = [[0, 0], [50, 50], [99, 99]];
  for (const [x, y] of samplePoints) {
    const value = band.getPixel(x, y);
    const geo = dataset.pixelToGeo(x, y);
    console.log(`  Pixel(${x},${y}) = ${value.toFixed(2)}, Geo(${geo.x.toFixed(2)}, ${geo.y.toFixed(2)})`);
  }

  // Save to file (requires temporary directory)
  const outputPath = '/tmp/oxigdal_test.tif';
  console.log(`\nSaving to ${outputPath}...`);
  dataset.save(outputPath);
  console.log('Done!');

  // Reopen and verify
  console.log('\nVerifying saved file...');
  const reopened = oxigdal.openRaster(outputPath);
  console.log(`  Size: ${reopened.width}x${reopened.height}`);
  console.log(`  Bands: ${reopened.bandCount}`);
  console.log(`  Type: ${reopened.dataType}`);
  console.log('Verification successful!');
}

main().catch(console.error);
