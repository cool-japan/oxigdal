/**
 * Raster dataset tests
 */

const oxigdal = require('../');
const path = require('path');
const fs = require('fs');

describe('Dataset', () => {
  test('create dataset', () => {
    const ds = oxigdal.createRaster(100, 100, 3, 'float32');
    expect(ds.width).toBe(100);
    expect(ds.height).toBe(100);
    expect(ds.bandCount).toBe(3);
    expect(ds.dataType).toBe('float32');
  });

  test('set and get geo transform', () => {
    const ds = oxigdal.createRaster(100, 100, 1, 'uint8');
    const gt = [-180, 0.1, 0, 90, 0, -0.1];
    ds.setGeoTransform(gt);

    const retrieved = ds.getGeoTransform();
    expect(retrieved).toHaveLength(6);
    for (let i = 0; i < 6; i++) {
      expect(retrieved[i]).toBeCloseTo(gt[i]);
    }
  });

  test('set and get CRS', () => {
    const ds = oxigdal.createRaster(100, 100, 1, 'uint8');
    ds.crs = 'EPSG:4326';
    expect(ds.crs).toBe('EPSG:4326');
  });

  test('set and get nodata', () => {
    const ds = oxigdal.createRaster(100, 100, 1, 'float32');
    ds.nodata = -9999;
    expect(ds.nodata).toBe(-9999);
  });

  test('get metadata', () => {
    const ds = oxigdal.createRaster(200, 150, 2, 'float32');
    ds.crs = 'EPSG:3857';
    ds.nodata = -1;

    const metadata = ds.getMetadata();
    expect(metadata.width).toBe(200);
    expect(metadata.height).toBe(150);
    expect(metadata.bandCount).toBe(2);
    expect(metadata.dataType).toBe('float32');
    expect(metadata.crs).toBe('EPSG:3857');
    expect(metadata.nodata).toBe(-1);
  });

  test('read and write band', () => {
    const ds = oxigdal.createRaster(50, 50, 1, 'float32');
    const buffer = new oxigdal.BufferWrapper(50, 50, 'float32');
    buffer.fill(42.0);

    ds.writeBand(0, buffer);
    const readBuffer = ds.readBand(0);

    expect(readBuffer.width).toBe(50);
    expect(readBuffer.height).toBe(50);
    expect(readBuffer.getPixel(0, 0)).toBe(42.0);
    expect(readBuffer.getPixel(25, 25)).toBe(42.0);
  });

  test('read window', () => {
    const ds = oxigdal.createRaster(100, 100, 1, 'float32');
    const buffer = new oxigdal.BufferWrapper(100, 100, 'float32');

    // Fill with coordinates
    for (let y = 0; y < 100; y++) {
      for (let x = 0; x < 100; x++) {
        buffer.setPixel(x, y, x + y * 100);
      }
    }
    ds.writeBand(0, buffer);

    // Read 10x10 window at (5, 5)
    const window = ds.readWindow(0, 5, 5, 10, 10);
    expect(window.width).toBe(10);
    expect(window.height).toBe(10);
    expect(window.getPixel(0, 0)).toBe(5 + 5 * 100);
  });

  test('pixel to geo conversion', () => {
    const ds = oxigdal.createRaster(100, 100, 1, 'float32');
    ds.setGeoTransform([-180, 3.6, 0, 90, 0, -1.8]);

    const geo = ds.pixelToGeo(50, 50);
    expect(geo.x).toBeCloseTo(0, 1); // Should be near 0 longitude
    expect(geo.y).toBeCloseTo(0, 1); // Should be near 0 latitude
  });

  test('geo to pixel conversion', () => {
    const ds = oxigdal.createRaster(100, 100, 1, 'float32');
    ds.setGeoTransform([-180, 3.6, 0, 90, 0, -1.8]);

    const pixel = ds.geoToPixel(0, 0);
    expect(pixel.x).toBeCloseTo(50, 0);
    expect(pixel.y).toBeCloseTo(50, 0);
  });

  test('save and reopen', () => {
    const tmpFile = '/tmp/oxigdal_test_save.tif';

    // Create and save
    const ds = oxigdal.createRaster(50, 50, 1, 'float32');
    const buffer = new oxigdal.BufferWrapper(50, 50, 'float32');
    buffer.fill(123.45);
    ds.writeBand(0, buffer);
    ds.crs = 'EPSG:4326';

    ds.save(tmpFile);

    // Reopen and verify
    const reopened = oxigdal.openRaster(tmpFile);
    expect(reopened.width).toBe(50);
    expect(reopened.height).toBe(50);
    expect(reopened.crs).toBe('EPSG:4326');

    const readBuffer = reopened.readBand(0);
    expect(readBuffer.getPixel(0, 0)).toBeCloseTo(123.45, 1);

    // Cleanup
    try {
      fs.unlinkSync(tmpFile);
    } catch (e) {
      // Ignore
    }
  });

  test('get bounds', () => {
    const ds = oxigdal.createRaster(100, 100, 1, 'float32');
    ds.setGeoTransform([-180, 3.6, 0, 90, 0, -1.8]);

    const bounds = ds.getBounds();
    expect(bounds).not.toBeNull();
    expect(bounds.minX).toBeCloseTo(-180, 1);
    expect(bounds.maxX).toBeCloseTo(180, 1);
    expect(bounds.maxY).toBeCloseTo(90, 1);
    expect(bounds.minY).toBeCloseTo(-90, 1);
  });
});
