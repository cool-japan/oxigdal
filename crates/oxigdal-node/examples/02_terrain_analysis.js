/**
 * Example 02: Terrain Analysis
 *
 * Demonstrates:
 * - DEM processing
 * - Hillshade computation
 * - Slope and aspect calculation
 * - Async operations
 */

const oxigdal = require('../');

async function main() {
  console.log('=== OxiGDAL Terrain Analysis Example ===\n');

  // Create synthetic DEM
  console.log('Creating synthetic DEM...');
  const width = 200;
  const height = 200;
  const dem = new oxigdal.BufferWrapper(width, height, 'float32');

  // Generate elevation data with multiple peaks
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const dx1 = x - width * 0.3;
      const dy1 = y - height * 0.3;
      const dist1 = Math.sqrt(dx1 * dx1 + dy1 * dy1);

      const dx2 = x - width * 0.7;
      const dy2 = y - height * 0.7;
      const dist2 = Math.sqrt(dx2 * dx2 + dy2 * dy2);

      const elevation =
        1000 * Math.exp(-dist1 * dist1 / 2000) +
        800 * Math.exp(-dist2 * dist2 / 1500);

      dem.setPixel(x, y, elevation);
    }
  }

  console.log('DEM Statistics:');
  const demStats = dem.statistics();
  console.log(`  Elevation range: ${demStats.min.toFixed(1)}m - ${demStats.max.toFixed(1)}m`);
  console.log(`  Mean elevation: ${demStats.mean.toFixed(1)}m`);

  // Compute hillshade (synchronous)
  console.log('\nComputing hillshade (sync)...');
  const hillshadeStart = Date.now();
  const hillshade = oxigdal.hillshade(dem, 315, 45, 1.0);
  const hillshadeTime = Date.now() - hillshadeStart;
  console.log(`  Completed in ${hillshadeTime}ms`);
  console.log(`  Output range: ${hillshade.statistics().min.toFixed(0)} - ${hillshade.statistics().max.toFixed(0)}`);

  // Compute slope (async)
  console.log('\nComputing slope (async, degrees)...');
  const slopeStart = Date.now();
  const slope = await oxigdal.slopeAsync(dem, 1.0, false);
  const slopeTime = Date.now() - slopeStart;
  console.log(`  Completed in ${slopeTime}ms`);
  const slopeStats = slope.statistics();
  console.log(`  Slope range: ${slopeStats.min.toFixed(2)}° - ${slopeStats.max.toFixed(2)}°`);
  console.log(`  Mean slope: ${slopeStats.mean.toFixed(2)}°`);

  // Compute slope as percent
  console.log('\nComputing slope (percent)...');
  const slopePercent = await oxigdal.slopeAsync(dem, 1.0, true);
  const slopePercentStats = slopePercent.statistics();
  console.log(`  Slope range: ${slopePercentStats.min.toFixed(2)}% - ${slopePercentStats.max.toFixed(2)}%`);
  console.log(`  Mean slope: ${slopePercentStats.mean.toFixed(2)}%`);

  // Compute aspect
  console.log('\nComputing aspect...');
  const aspectStart = Date.now();
  const aspect = await oxigdal.aspectAsync(dem);
  const aspectTime = Date.now() - aspectStart;
  console.log(`  Completed in ${aspectTime}ms`);
  const aspectStats = aspect.statistics();
  console.log(`  Aspect range: ${aspectStats.min.toFixed(1)}° - ${aspectStats.max.toFixed(1)}°`);

  // Save all outputs
  console.log('\nSaving outputs...');
  const outputs = [
    { buffer: dem, name: 'dem.tif', type: 'float32' },
    { buffer: hillshade, name: 'hillshade.tif', type: 'uint8' },
    { buffer: slope, name: 'slope.tif', type: 'float32' },
    { buffer: aspect, name: 'aspect.tif', type: 'float32' }
  ];

  for (const output of outputs) {
    const dataset = oxigdal.createRaster(width, height, 1, output.type);
    dataset.writeBand(0, output.buffer);
    const path = `/tmp/oxigdal_${output.name}`;
    await oxigdal.saveRasterAsync(dataset, path);
    console.log(`  Saved ${output.name}`);
  }

  console.log('\nTerrain analysis complete!');
}

main().catch(console.error);
