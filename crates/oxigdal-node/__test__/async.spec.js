/**
 * Async operations tests
 */

const oxigdal = require('../');
const fs = require('fs').promises;

describe('Async Operations', () => {
  const testFile = '/tmp/oxigdal_async_test.tif';

  afterEach(async () => {
    try {
      await fs.unlink(testFile);
    } catch (e) {
      // Ignore
    }
  });

  test('async open and save', async () => {
    // Create and save
    const ds = oxigdal.createRaster(50, 50, 1, 'float32');
    const buffer = new oxigdal.BufferWrapper(50, 50, 'float32');
    buffer.fill(99.0);
    ds.writeBand(0, buffer);

    await oxigdal.saveRasterAsync(ds, testFile);

    // Reopen async
    const reopened = await oxigdal.openRasterAsync(testFile);
    expect(reopened.width).toBe(50);
    expect(reopened.height).toBe(50);

    const readBuffer = reopened.readBand(0);
    expect(readBuffer.getPixel(0, 0)).toBeCloseTo(99.0, 1);
  });

  test('async resample', async () => {
    const buffer = new oxigdal.BufferWrapper(100, 100, 'float32');
    buffer.fill(42.0);

    const resampled = await oxigdal.resampleAsync(
      buffer,
      50,
      50,
      oxigdal.ResamplingMethod.Bilinear
    );

    expect(resampled.width).toBe(50);
    expect(resampled.height).toBe(50);
  });

  test('async terrain analysis', async () => {
    const dem = new oxigdal.BufferWrapper(50, 50, 'float32');

    for (let y = 0; y < 50; y++) {
      for (let x = 0; x < 50; x++) {
        dem.setPixel(x, y, x + y);
      }
    }

    const hillshade = await oxigdal.hillshadeAsync(dem, 315, 45, 1.0);
    expect(hillshade.width).toBe(50);

    const slope = await oxigdal.slopeAsync(dem, 1.0, false);
    expect(slope.width).toBe(50);

    const aspect = await oxigdal.aspectAsync(dem);
    expect(aspect.width).toBe(50);
  });

  test('async zonal stats', async () => {
    const raster = new oxigdal.BufferWrapper(10, 10, 'float32');
    const zones = new oxigdal.BufferWrapper(10, 10, 'int32');

    for (let i = 0; i < 100; i++) {
      const x = i % 10;
      const y = Math.floor(i / 10);
      zones.setPixel(x, y, x < 5 ? 1 : 2);
      raster.setPixel(x, y, i);
    }

    const stats = await oxigdal.zonalStatsAsync(raster, zones);
    expect(stats).toHaveLength(2);
  });

  test('cancellation token', () => {
    const token = new oxigdal.CancellationToken();
    expect(token.isCancelled()).toBe(false);

    token.cancel();
    expect(token.isCancelled()).toBe(true);

    token.reset();
    expect(token.isCancelled()).toBe(false);
  });

  test('raster stream', async () => {
    const ds = oxigdal.createRaster(100, 100, 1, 'float32');
    const buffer = new oxigdal.BufferWrapper(100, 100, 'float32');
    buffer.fill(1.0);
    ds.writeBand(0, buffer);

    const stream = new oxigdal.RasterStream(ds, 20);
    let chunkCount = 0;
    let chunk;

    while ((chunk = await stream.readNextChunk()) !== null) {
      chunkCount++;
      expect(chunk.width).toBe(100);
      expect(chunk.height).toBeLessThanOrEqual(20);
    }

    expect(chunkCount).toBe(5); // 100 / 20 = 5 chunks
    expect(stream.progress()).toBe(1.0);

    // Reset and try again
    stream.reset();
    expect(stream.progress()).toBe(0);

    const firstChunk = await stream.readNextChunk();
    expect(firstChunk).not.toBeNull();
  });
});
