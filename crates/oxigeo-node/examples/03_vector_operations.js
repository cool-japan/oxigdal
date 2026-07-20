/**
 * Example 03: Vector Operations
 *
 * Demonstrates:
 * - Creating geometries
 * - GeoJSON I/O
 * - Buffer operations
 * - Area calculations
 * - Simplification
 */

const oxigeo = require('../');

async function main() {
  console.log('=== OxiGeo Vector Operations Example ===\n');

  // Create a feature collection
  const collection = new oxigeo.FeatureCollection();

  // Create point features
  console.log('Creating point features...');
  const cities = [
    { name: 'City A', x: -122.4, y: 37.8 },
    { name: 'City B', x: -122.2, y: 37.6 },
    { name: 'City C', x: -122.6, y: 37.9 }
  ];

  for (const city of cities) {
    const point = oxigeo.GeometryWrapper.point(city.x, city.y);
    const feature = new oxigeo.Feature(point);
    feature.setProperty('name', city.name);
    feature.setProperty('type', 'city');
    collection.addFeature(feature);
    console.log(`  Added ${city.name} at (${city.x}, ${city.y})`);
  }

  // Create polygon feature
  console.log('\nCreating polygon feature...');
  const polygonCoords = [
    [
      [-122.5, 37.5],
      [-122.3, 37.5],
      [-122.3, 37.7],
      [-122.5, 37.7],
      [-122.5, 37.5]
    ]
  ];
  const polygon = oxigeo.GeometryWrapper.polygon(polygonCoords);
  const polygonFeature = new oxigeo.Feature(polygon);
  polygonFeature.setProperty('name', 'District 1');
  polygonFeature.setProperty('type', 'district');
  collection.addFeature(polygonFeature);

  // Calculate area
  console.log('\nCalculating polygon area...');
  const areaPlanar = oxigeo.area(polygon, 'planar');
  const areaGeodetic = await oxigeo.areaAsync(polygon, 'geodetic');
  console.log(`  Planar area: ${areaPlanar.toFixed(6)} square degrees`);
  console.log(`  Geodetic area: ${areaGeodetic.toFixed(2)} square meters`);

  // Buffer a point
  console.log('\nBuffering point...');
  const cityPoint = oxigeo.GeometryWrapper.point(-122.4, 37.8);
  const buffered = await oxigeo.bufferAsync(cityPoint, 0.1, 32);
  const bufferedFeature = new oxigeo.Feature(buffered);
  bufferedFeature.setProperty('name', 'Buffer Zone');
  bufferedFeature.setProperty('radius', '0.1');
  collection.addFeature(bufferedFeature);
  console.log('  Buffer created');

  // Create linestring
  console.log('\nCreating linestring...');
  const lineCoords = [
    [-122.5, 37.5],
    [-122.45, 37.55],
    [-122.42, 37.58],
    [-122.38, 37.62],
    [-122.35, 37.65],
    [-122.32, 37.68],
    [-122.3, 37.7]
  ];
  const linestring = oxigeo.GeometryWrapper.linestring(lineCoords);
  const lineFeature = new oxigeo.Feature(linestring);
  lineFeature.setProperty('name', 'Route 1');
  lineFeature.setProperty('type', 'road');
  collection.addFeature(lineFeature);

  // Simplify linestring
  console.log('\nSimplifying linestring...');
  const simplified = await oxigeo.simplifyAsync(linestring, 0.02, 'douglas-peucker');
  const simplifiedFeature = new oxigeo.Feature(simplified);
  simplifiedFeature.setProperty('name', 'Route 1 (simplified)');
  simplifiedFeature.setProperty('type', 'road_simplified');
  collection.addFeature(simplifiedFeature);
  console.log('  Simplification complete');

  // Print collection info
  console.log(`\nFeature collection: ${collection.count} features`);
  for (let i = 0; i < collection.count; i++) {
    const feature = collection.getFeature(i);
    const geom = feature.getGeometry();
    console.log(`  Feature ${i + 1}: ${feature.getProperty('name')} (${geom.geometryType})`);
  }

  // Save as GeoJSON
  const outputPath = '/tmp/oxigeo_features.geojson';
  console.log(`\nSaving GeoJSON to ${outputPath}...`);
  await oxigeo.writeGeojsonAsync(outputPath, collection);

  // Reload and verify
  console.log('Verifying saved file...');
  const reloaded = await oxigeo.readGeojsonAsync(outputPath);
  console.log(`  Loaded ${reloaded.count} features`);

  // Print GeoJSON
  console.log('\nGeoJSON output (first feature):');
  const firstFeature = reloaded.getFeature(0);
  console.log(JSON.stringify(JSON.parse(firstFeature.toGeojson()), null, 2));

  console.log('\nVector operations complete!');
}

main().catch(console.error);
