# Testing Checklist - OxiGDAL COG Viewer

Comprehensive testing guide to ensure the COG viewer works correctly across all browsers and scenarios.

## Pre-Deployment Testing

### Build Verification

- [ ] WASM package builds without errors
- [ ] WASM binary size is reasonable (< 500KB for release)
- [ ] JavaScript bindings are generated
- [ ] TypeScript definitions are present
- [ ] No Rust warnings during build
- [ ] Release build optimizations applied

```bash
cd ../../crates/oxigdal-wasm
wasm-pack build --target web --release --out-dir ../../demo/pkg
```

### File Verification

- [ ] All HTML files are valid (W3C validator)
- [ ] JavaScript syntax is correct
- [ ] CSS has no errors
- [ ] All required files present (use `./verify.sh`)
- [ ] File permissions are correct
- [ ] No absolute paths in code

```bash
cd demo/cog-viewer
./verify.sh
```

## Functional Testing

### Application Initialization

- [ ] Page loads without errors
- [ ] WASM module initializes successfully
- [ ] Leaflet map displays correctly
- [ ] Version badge shows correct version
- [ ] Status indicator shows "Ready"
- [ ] No console errors on page load

### COG Loading

#### Example Datasets

- [ ] Sentinel-2 RGB loads successfully
- [ ] OpenAerialMap Haiti loads successfully
- [ ] Hurricane Harvey loads successfully
- [ ] Loading spinner appears during load
- [ ] Progress bar updates correctly
- [ ] Status changes to "Loading"
- [ ] Metadata displays after load
- [ ] Map fits to COG bounds

#### Custom URL

- [ ] Can enter URL in input field
- [ ] Enter key triggers load
- [ ] Load button triggers load
- [ ] Invalid URL shows error message
- [ ] CORS errors are handled gracefully
- [ ] Network errors are caught and displayed

### Map Interaction

#### Pan

- [ ] Click and drag pans the map
- [ ] Pan is smooth and responsive
- [ ] Cursor changes during drag
- [ ] Pan works with touch gestures (mobile)

#### Zoom

- [ ] Zoom in button increases zoom
- [ ] Zoom out button decreases zoom
- [ ] Mouse wheel zooms correctly
- [ ] Pinch gesture zooms (mobile)
- [ ] Zoom level display updates
- [ ] Tiles load at correct zoom levels

#### Bounds

- [ ] "Fit to Bounds" centers COG
- [ ] Bounds calculation is correct
- [ ] Works with different CRS

### Visualization Controls

#### Band Mode

- [ ] RGB mode displays correctly
- [ ] Grayscale mode shows band 1
- [ ] NIR mode works (if applicable)
- [ ] Custom mode enables band inputs
- [ ] Custom band numbers apply correctly
- [ ] Invalid band numbers are handled

#### Image Adjustments

- [ ] Opacity slider changes transparency (0-100%)
- [ ] Opacity value display updates
- [ ] Brightness slider adjusts image (-50 to +50)
- [ ] Brightness value display updates
- [ ] Contrast slider scales image (0-200%)
- [ ] Contrast value display updates
- [ ] Changes apply in real-time
- [ ] Reset button restores defaults

### Metadata Display

- [ ] Image dimensions are correct
- [ ] Tile size is accurate
- [ ] Band count matches COG
- [ ] Overview count is correct
- [ ] EPSG code displays (if available)
- [ ] URL is shown and readable
- [ ] All metadata updates on new COG load

### Performance Metrics

- [ ] Load time is recorded and displayed
- [ ] Render time updates correctly
- [ ] Data transfer shows bytes downloaded
- [ ] Tile count updates as tiles load
- [ ] Cached tile count increases on revisit
- [ ] Metrics are accurate

### Tile Management

- [ ] Only visible tiles are loaded
- [ ] Tiles are cached in memory
- [ ] Cached tiles are reused on pan
- [ ] Tile loading is asynchronous
- [ ] Failed tiles show error gracefully
- [ ] No duplicate tile requests

### Error Handling

- [ ] Invalid COG URL shows error overlay
- [ ] CORS errors display helpful message
- [ ] Network timeouts are handled
- [ ] WASM errors are caught
- [ ] Error overlay can be dismissed
- [ ] Application remains usable after error

## Browser Compatibility

### Chrome/Edge

- [ ] Application loads and runs
- [ ] All features work correctly
- [ ] No console errors
- [ ] Performance is acceptable
- [ ] DevTools show no issues

**Test on:**
- [ ] Chrome 90+
- [ ] Chrome Latest
- [ ] Edge 90+
- [ ] Edge Latest

### Firefox

- [ ] Application loads and runs
- [ ] All features work correctly
- [ ] No console errors
- [ ] Performance is acceptable
- [ ] WASM loads correctly

**Test on:**
- [ ] Firefox 88+
- [ ] Firefox Latest

### Safari

- [ ] Application loads and runs
- [ ] All features work correctly
- [ ] No console errors
- [ ] WASM loads correctly
- [ ] CORS works properly

**Test on:**
- [ ] Safari 14+
- [ ] Safari Latest (macOS)
- [ ] Safari iOS Latest

### Mobile Browsers

- [ ] Touch gestures work
- [ ] Pinch zoom functions
- [ ] Layout is responsive
- [ ] Performance is acceptable
- [ ] No memory issues

**Test on:**
- [ ] Chrome Mobile
- [ ] Safari iOS
- [ ] Firefox Mobile

## Performance Testing

### Load Performance

- [ ] Initial page load < 3s (on fast connection)
- [ ] WASM initialization < 1s
- [ ] First COG load < 10s (medium-size COG)
- [ ] Subsequent loads faster (caching)
- [ ] Lighthouse score > 90

```bash
npx lighthouse http://localhost:8080 --view
```

### Runtime Performance

- [ ] Smooth pan (60 FPS)
- [ ] Smooth zoom (60 FPS)
- [ ] No jank during tile loading
- [ ] Memory usage stays reasonable
- [ ] No memory leaks on repeated loads
- [ ] CPU usage is acceptable

**Tools:**
- Chrome DevTools Performance tab
- Firefox Performance Monitor
- Safari Web Inspector

### Network Performance

- [ ] HTTP range requests are used
- [ ] Only needed bytes are downloaded
- [ ] Concurrent requests are reasonable
- [ ] No redundant requests
- [ ] Caching works correctly
- [ ] Compression is enabled (if available)

**Monitor in DevTools Network tab:**
- Request count
- Total bytes transferred
- Request timing
- Cache hits

### Memory Testing

- [ ] Initial memory usage < 100MB
- [ ] Memory grows reasonably with tiles
- [ ] Memory is released on cache clear
- [ ] No memory leaks detected
- [ ] Works with large COGs (> 10GB)

**Test scenarios:**
1. Load COG, pan around, check memory
2. Load multiple COGs, check memory growth
3. Rapid zoom in/out, check for leaks
4. Long session (30+ min), check stability

## Accessibility Testing

- [ ] Keyboard navigation works
- [ ] Tab order is logical
- [ ] Focus indicators are visible
- [ ] ARIA labels are present
- [ ] Screen reader compatible
- [ ] Color contrast meets WCAG AA
- [ ] No keyboard traps

**Tools:**
- axe DevTools
- Lighthouse Accessibility Audit
- Screen reader (NVDA, JAWS, VoiceOver)

## Security Testing

### CORS

- [ ] CORS headers are correct
- [ ] Cross-origin requests work
- [ ] Range requests allowed
- [ ] No security warnings

### Content Security Policy

- [ ] CSP headers are set (if configured)
- [ ] No inline script violations
- [ ] WASM is allowed
- [ ] External resources (Leaflet) work

### HTTPS

- [ ] Works over HTTPS
- [ ] No mixed content warnings
- [ ] SSL certificate is valid
- [ ] Secure cookies (if used)

## Integration Testing

### COG Compatibility

Test with various COG types:

- [ ] **Grayscale**: Single band
- [ ] **RGB**: 3 bands
- [ ] **RGBA**: 4 bands with alpha
- [ ] **Multispectral**: 8+ bands
- [ ] **Large files**: > 1GB
- [ ] **Small tiles**: 128×128
- [ ] **Large tiles**: 512×512
- [ ] **Compressed**: LZW, DEFLATE
- [ ] **Uncompressed**: Raw data
- [ ] **With overviews**: Multiple levels
- [ ] **No overviews**: Single level
- [ ] **Different CRS**: 4326, 3857, etc.

### Data Sources

- [ ] AWS S3 hosted COGs
- [ ] Google Cloud Storage
- [ ] Azure Blob Storage
- [ ] OpenAerialMap
- [ ] Planet Labs
- [ ] Custom HTTP server

### Error Scenarios

- [ ] **Invalid URL**: Non-existent file
- [ ] **Non-TIFF**: JPEG, PNG file
- [ ] **Corrupted file**: Partial data
- [ ] **No CORS**: Server blocks origin
- [ ] **No range support**: Server issue
- [ ] **Slow network**: Timeout handling
- [ ] **Large file**: Memory handling

## Regression Testing

After code changes:

- [ ] All previous tests still pass
- [ ] No new console errors
- [ ] Performance hasn't degraded
- [ ] No visual regressions
- [ ] Build still succeeds

**Automated checks:**
```bash
# Build check
cd ../../crates/oxigdal-wasm
cargo build --release --target wasm32-unknown-unknown

# Lint check
cargo clippy -- -D warnings

# Format check
cargo fmt -- --check
```

## Deployment Verification

After deployment to production:

- [ ] URL is accessible
- [ ] HTTPS works correctly
- [ ] DNS resolves properly
- [ ] CDN caching works
- [ ] Compression enabled
- [ ] CORS headers present
- [ ] Example COGs load
- [ ] No 404 errors
- [ ] Analytics tracking (if configured)

**Test in multiple regions:**
- [ ] North America
- [ ] Europe
- [ ] Asia
- [ ] Australia

## User Acceptance Testing

### First-Time User Experience

- [ ] Page loads quickly
- [ ] UI is intuitive
- [ ] Example datasets work
- [ ] Instructions are clear
- [ ] Metadata is understandable
- [ ] No confusing errors

### Power User Features

- [ ] Custom URLs work smoothly
- [ ] Advanced controls are accessible
- [ ] Performance metrics are useful
- [ ] Keyboard shortcuts (if any) work

### Edge Cases

- [ ] Very large COG (> 10GB)
- [ ] Very small COG (< 1MB)
- [ ] COG with many bands (> 10)
- [ ] COG with unusual CRS
- [ ] Slow network connection
- [ ] High latency connection

## Documentation Testing

- [ ] README is accurate
- [ ] QUICKSTART works
- [ ] DEPLOYMENT guide is correct
- [ ] All commands execute successfully
- [ ] Examples are valid
- [ ] Links work
- [ ] Code snippets are correct

## Automated Testing

### Unit Tests (Future)

```javascript
// Example test structure
describe('COG Viewer', () => {
    it('should initialize WASM', async () => {
        await initializeApp();
        expect(app.wasmInitialized).toBe(true);
    });

    it('should load COG from URL', async () => {
        await loadCog('https://example.com/cog.tif');
        expect(app.currentCog.url).toBe('https://example.com/cog.tif');
    });
});
```

### End-to-End Tests (Future)

Using Playwright or Cypress:

```javascript
// Example E2E test
test('should load and display example COG', async ({ page }) => {
    await page.goto('http://localhost:8080');
    await page.click('[data-name="Sentinel-2 RGB"]');
    await page.waitForSelector('#map-container canvas');
    // Assert tiles are visible
});
```

## Test Results Template

### Test Session Info

- **Date**: YYYY-MM-DD
- **Tester**: Name
- **Environment**: Development / Staging / Production
- **Build Version**: vX.Y.Z
- **WASM Size**: XXX KB

### Browser Matrix

| Browser | Version | Status | Notes |
|---------|---------|--------|-------|
| Chrome  | 120     | ✅ Pass | All tests passed |
| Firefox | 121     | ✅ Pass | Minor CSS issue |
| Safari  | 17      | ⚠️ Partial | CORS issue |
| Edge    | 120     | ✅ Pass | All tests passed |

### Test Summary

- **Total Tests**: XX
- **Passed**: XX
- **Failed**: XX
- **Blocked**: XX
- **Pass Rate**: XX%

### Issues Found

| ID | Severity | Description | Status |
|----|----------|-------------|--------|
| 1  | High     | CORS fails in Safari | Open |
| 2  | Low      | Tooltip misaligned  | Fixed |

### Recommendations

- Fix Safari CORS issue before production
- Add loading indicator for slow networks
- Improve error messages for clarity

---

## Testing Checklist Summary

**Before Release:**
- [ ] All functional tests pass
- [ ] Tested on Chrome, Firefox, Safari
- [ ] Performance is acceptable
- [ ] No critical errors
- [ ] Documentation is accurate
- [ ] Security checks pass
- [ ] Deployment verified

**Sign-off:**
- Tester: ________________
- Date: ________________
- Ready for Production: Yes / No

---

**Comprehensive Testing Complete!** The COG viewer is ready for deployment.
