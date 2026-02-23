/**
 * Buffer tests
 */

const oxigdal = require('../');

describe('BufferWrapper', () => {
  test('create buffer', () => {
    const buffer = new oxigdal.BufferWrapper(10, 10, 'float32');
    expect(buffer.width).toBe(10);
    expect(buffer.height).toBe(10);
    expect(buffer.dataType).toBe('float32');
  });

  test('fill buffer', () => {
    const buffer = new oxigdal.BufferWrapper(5, 5, 'float32');
    buffer.fill(42.0);

    for (let y = 0; y < 5; y++) {
      for (let x = 0; x < 5; x++) {
        expect(buffer.getPixel(x, y)).toBe(42.0);
      }
    }
  });

  test('set and get pixel', () => {
    const buffer = new oxigdal.BufferWrapper(3, 3, 'float32');
    buffer.setPixel(1, 1, 123.45);
    expect(buffer.getPixel(1, 1)).toBeCloseTo(123.45);
  });

  test('statistics', () => {
    const buffer = new oxigdal.BufferWrapper(10, 10, 'float32');

    // Fill with values 0-99
    for (let i = 0; i < 100; i++) {
      buffer.setPixel(i % 10, Math.floor(i / 10), i);
    }

    const stats = buffer.statistics();
    expect(stats.count).toBe(100);
    expect(stats.min).toBe(0);
    expect(stats.max).toBe(99);
    expect(stats.mean).toBeCloseTo(49.5, 1);
  });

  test('clone buffer', () => {
    const buffer = new oxigdal.BufferWrapper(5, 5, 'float32');
    buffer.fill(100);

    const cloned = buffer.clone();
    expect(cloned.width).toBe(buffer.width);
    expect(cloned.height).toBe(buffer.height);
    expect(cloned.getPixel(0, 0)).toBe(100);

    // Modify clone shouldn't affect original
    cloned.setPixel(0, 0, 200);
    expect(buffer.getPixel(0, 0)).toBe(100);
    expect(cloned.getPixel(0, 0)).toBe(200);
  });

  test('out of bounds access', () => {
    const buffer = new oxigdal.BufferWrapper(5, 5, 'float32');

    expect(() => {
      buffer.getPixel(10, 0);
    }).toThrow();

    expect(() => {
      buffer.setPixel(0, 10, 42);
    }).toThrow();
  });

  test('byte size', () => {
    const buffer = new oxigdal.BufferWrapper(10, 10, 'float32');
    expect(buffer.byteSize).toBe(10 * 10 * 4); // float32 = 4 bytes
    expect(buffer.pixelCount).toBe(100);
  });
});
