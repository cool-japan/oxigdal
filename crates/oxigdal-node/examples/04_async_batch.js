/**
 * Example 04: Async Batch Processing
 *
 * Demonstrates:
 * - Batch processing multiple files
 * - Parallel operations
 * - Progress tracking
 * - Stream processing for large files
 */

const oxigdal = require('../');
const path = require('path');

async function createTestRasters(count) {
  console.log(`Creating ${count} test rasters...`);
  const paths = [];

  for (let i = 0; i < count; i++) {
    const width = 100 + i * 10;
    const height = 100 + i * 10;
    const dataset = oxigdal.createRaster(width, height, 1, 'float32');

    const buffer = new oxigdal.BufferWrapper(width, height, 'float32');
    buffer.fill(Math.random() * 100);
    dataset.writeBand(0, buffer);

    const filePath = `/tmp/oxigdal_test_${i}.tif`;
    await oxigdal.saveRasterAsync(dataset, filePath);
    paths.push(filePath);

    if ((i + 1) % 5 === 0) {
      console.log(`  Created ${i + 1}/${count} rasters`);
    }
  }

  return paths;
}

async function main() {
  console.log('=== OxiGDAL Async Batch Processing Example ===\n');

  // Create test rasters
  const numRasters = 10;
  const inputPaths = await createTestRasters(numRasters);
  console.log('');

  // Batch processing
  console.log('Batch processing rasters...');
  const startTime = Date.now();

  const processedPaths = await oxigdal.batchProcessRasters(
    inputPaths,
    '/tmp',
    'identity'
  );

  const totalTime = Date.now() - startTime;
  console.log(`Processed ${processedPaths.length} rasters in ${totalTime}ms`);
  console.log(`Average time per raster: ${(totalTime / processedPaths.length).toFixed(1)}ms`);

  // Parallel processing with config
  console.log('\nParallel processing with configuration...');
  const dataset = await oxigdal.openRasterAsync(inputPaths[0]);

  const config = {
    numThreads: 4,
    chunkSize: 1000,
    reportProgress: false
  };

  const parallelStart = Date.now();
  const result = await oxigdal.processRasterParallel(dataset, 'identity', config);
  const parallelTime = Date.now() - parallelStart;
  console.log(`Parallel processing completed in ${parallelTime}ms`);
  console.log(`Result: ${result.width}x${result.height}`);

  // Stream processing for large dataset
  console.log('\nStream processing demonstration...');
  const largeDataset = oxigdal.createRaster(500, 500, 1, 'float32');
  const largeBuffer = new oxigdal.BufferWrapper(500, 500, 'float32');

  // Fill with data
  for (let i = 0; i < 500 * 500; i++) {
    largeBuffer.setPixel(i % 500, Math.floor(i / 500), Math.random() * 255);
  }
  largeDataset.writeBand(0, largeBuffer);

  // Process in chunks
  const stream = new oxigdal.RasterStream(largeDataset, 100);
  let chunkCount = 0;
  let chunk;

  console.log('Processing stream...');
  while ((chunk = await stream.readNextChunk()) !== null) {
    chunkCount++;
    const progress = stream.progress();
    console.log(`  Chunk ${chunkCount}: ${chunk.width}x${chunk.height} (${(progress * 100).toFixed(1)}% complete)`);
  }

  console.log(`Stream processing complete: ${chunkCount} chunks processed`);

  // Cancellation token example
  console.log('\nCancellation token demonstration...');
  const token = new oxigdal.CancellationToken();

  // Simulate cancellation after 100ms
  setTimeout(() => {
    console.log('  Cancelling operation...');
    token.cancel();
  }, 100);

  const operations = [];
  for (let i = 0; i < 5; i++) {
    operations.push(
      oxigdal.openRasterAsync(inputPaths[i])
        .then(ds => {
          if (token.isCancelled()) {
            console.log(`  Operation ${i + 1}: Cancelled`);
            return null;
          }
          console.log(`  Operation ${i + 1}: Completed`);
          return ds;
        })
    );
  }

  const results = await Promise.all(operations);
  const successCount = results.filter(r => r !== null).length;
  console.log(`Completed ${successCount}/${operations.length} operations before cancellation`);

  // Cleanup
  console.log('\nCleaning up test files...');
  const fs = require('fs').promises;
  for (const filePath of inputPaths) {
    try {
      await fs.unlink(filePath);
    } catch (e) {
      // Ignore errors
    }
  }

  console.log('Batch processing example complete!');
}

main().catch(console.error);
