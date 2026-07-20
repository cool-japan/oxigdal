/**
 * Algorithm tests
 */

const oxigeo = require('../');

describe('Algorithms', () => {
  test('resample', () => {
    const buffer = new oxigeo.BufferWrapper(100, 100, 'float32');
    buffer.fill(42.0);

    const resampled = oxigeo.resample(
      buffer,
      50,
      50,
      oxigeo.ResamplingMethod.NearestNeighbor
    );

    expect(resampled.width).toBe(50);
    expect(resampled.height).toBe(50);
    expect(resampled.getPixel(0, 0)).toBeCloseTo(42.0, 1);
  });

  test('hillshade', () => {
    const dem = new oxigeo.BufferWrapper(50, 50, 'float32');

    // Create simple slope
    for (let y = 0; y < 50; y++) {
      for (let x = 0; x < 50; x++) {
        dem.setPixel(x, y, x * 2 + y * 2);
      }
    }

    const hillshade = oxigeo.hillshade(dem, 315, 45, 1.0, 1.0);
    expect(hillshade.width).toBe(50);
    expect(hillshade.height).toBe(50);
    expect(hillshade.dataType).toBe('uint8');

    const stats = hillshade.statistics();
    expect(stats.min).toBeGreaterThanOrEqual(0);
    expect(stats.max).toBeLessThanOrEqual(255);
  });

  test('slope', () => {
    const dem = new oxigeo.BufferWrapper(50, 50, 'float32');

    // Create simple slope
    for (let y = 0; y < 50; y++) {
      for (let x = 0; x < 50; x++) {
        dem.setPixel(x, y, y * 10);
      }
    }

    const slope = oxigeo.slope(dem, 1.0, 1.0, false);
    expect(slope.width).toBe(50);
    expect(slope.height).toBe(50);
    expect(slope.dataType).toBe('float32');

    // Slope should be relatively uniform for this simple case
    const stats = slope.statistics();
    expect(stats.mean).toBeGreaterThan(0);
  });

  test('aspect', () => {
    const dem = new oxigeo.BufferWrapper(50, 50, 'float32');

    // Create slope in one direction
    for (let y = 0; y < 50; y++) {
      for (let x = 0; x < 50; x++) {
        dem.setPixel(x, y, x * 10);
      }
    }

    const aspect = oxigeo.aspect(dem, 1.0);
    expect(aspect.width).toBe(50);
    expect(aspect.height).toBe(50);

    // Aspect should indicate east-facing slope
    const stats = aspect.statistics();
    expect(stats.min).toBeGreaterThanOrEqual(-1);
    expect(stats.max).toBeLessThanOrEqual(360);
  });

  test('zonal statistics', () => {
    const raster = new oxigeo.BufferWrapper(10, 10, 'float32');
    const zones = new oxigeo.BufferWrapper(10, 10, 'int32');

    // Create two zones with different values
    for (let y = 0; y < 10; y++) {
      for (let x = 0; x < 10; x++) {
        if (x < 5) {
          zones.setPixel(x, y, 1);
          raster.setPixel(x, y, 10);
        } else {
          zones.setPixel(x, y, 2);
          raster.setPixel(x, y, 20);
        }
      }
    }

    const stats = oxigeo.zonalStats(raster, zones);
    expect(stats).toHaveLength(2);

    const zone1 = stats.find(s => s.zoneId === 1);
    const zone2 = stats.find(s => s.zoneId === 2);

    expect(zone1).toBeDefined();
    expect(zone2).toBeDefined();
    expect(zone1.mean).toBeCloseTo(10, 1);
    expect(zone2.mean).toBeCloseTo(20, 1);
    expect(zone1.count).toBe(50);
    expect(zone2.count).toBe(50);
  });
});

describe('Vector Algorithms', () => {
  test('buffer point', () => {
    const point = oxigeo.GeometryWrapper.point(0, 0);
    const buffered = oxigeo.buffer(point, 1.0, 16);

    expect(buffered.geometryType).toBe('Polygon');
  });

  test('area calculation', () => {
    // Create a simple square polygon
    const coords = [
      [
        [0, 0],
        [1, 0],
        [1, 1],
        [0, 1],
        [0, 0]
      ]
    ];
    const polygon = oxigeo.GeometryWrapper.polygon(coords);
    const area = oxigeo.area(polygon, 'planar');

    expect(area).toBeCloseTo(1.0, 2);
  });

  test('simplify linestring', () => {
    const coords = [
      [0, 0],
      [0.1, 0.05],
      [0.2, 0.1],
      [1, 0]
    ];
    const linestring = oxigeo.GeometryWrapper.linestring(coords);
    const simplified = oxigeo.simplify(linestring, 0.1, 'douglas-peucker');

    expect(simplified.geometryType).toBe('LineString');
  });
});
