# OxiGDAL iOS Integration Guide

This guide explains how to integrate OxiGDAL into your iOS application.

## Requirements

- iOS 13.0+
- Xcode 15.0+
- Swift 5.9+
- Rust toolchain with iOS targets

## Installation Methods

### Option 1: Swift Package Manager (Recommended)

Add OxiGDAL to your `Package.swift`:

```swift
dependencies: [
    .package(url: "https://github.com/cool-japan/oxigdal", from: "0.1.0")
]
```

Or add via Xcode:
1. File → Add Packages...
2. Enter repository URL: `https://github.com/cool-japan/oxigdal`
3. Select version and add to your target

### Option 2: CocoaPods

Add to your `Podfile`:

```ruby
pod 'OxiGDAL', '~> 0.1.0'
```

Then run:
```bash
pod install
```

### Option 3: Manual Integration

1. **Build Rust library for iOS**:

```bash
# Add iOS targets
rustup target add aarch64-apple-ios
rustup target add x86_64-apple-ios

# Build for device (ARM64)
cargo build --release --target aarch64-apple-ios

# Build for simulator (x86_64)
cargo build --release --target x86_64-apple-ios

# Create universal library (optional)
lipo -create \
    target/aarch64-apple-ios/release/liboxigdal_mobile.a \
    target/x86_64-apple-ios/release/liboxigdal_mobile.a \
    -output liboxigdal_mobile.a
```

2. **Add to Xcode project**:
   - Drag `liboxigdal_mobile.a` into your project
   - Add to "Link Binary With Libraries" in Build Phases
   - Add `OxiGDAL.swift` to your project
   - Create bridging header if using Objective-C

## Quick Start

### Basic Usage

```swift
import OxiGDAL
import UIKit

class MapViewController: UIViewController {

    override func viewDidLoad() {
        super.viewDidLoad()

        // Initialize OxiGDAL
        OxiGDAL.initialize()

        // Load and display map
        loadMap()
    }

    func loadMap() {
        do {
            // Open COG file
            let mapPath = Bundle.main.path(forResource: "map", ofType: "tif")!
            let dataset = try OxiGDAL.open(mapPath)

            // Get metadata
            let metadata = try dataset.metadata
            print("Map size: \(metadata.width)x\(metadata.height)")
            print("Bands: \(metadata.bandCount)")

            // Read entire image
            let image = try dataset.toImage()
            imageView.image = image

            // Clean up
            dataset.close()

        } catch let error as OxiGDAL.Error {
            print("Error: \(error.localizedDescription)")
        }
    }

    deinit {
        OxiGDAL.cleanup()
    }
}
```

### Reading Regions

```swift
// Read specific region
let dataset = try OxiGDAL.open("large_map.tif")

// Read 512x512 region starting at (1000, 1000)
let buffer = try dataset.readRegion(
    x: 1000,
    y: 1000,
    width: 512,
    height: 512,
    band: 1
)

let image = try buffer.toUIImage()
imageView.image = image
```

### Tile-Based Loading

```swift
// Load map tiles for offline viewing
class TileLoader {
    let dataset: OxiGDAL.Dataset

    init(path: String) throws {
        self.dataset = try OxiGDAL.open(path)
    }

    func loadTile(z: Int, x: Int, y: Int) throws -> UIImage {
        let buffer = try dataset.readTile(z: z, x: x, y: y, tileSize: 256)
        return try buffer.toUIImage()
    }
}

// Usage
let tileLoader = try TileLoader(path: "offline_map.tif")
let tileImage = try tileLoader.loadTile(z: 10, x: 512, y: 341)
```

### Image Enhancement

```swift
// Apply enhancements
let image = try dataset.toImage()

let enhanced = try OxiGDAL.enhance(
    image,
    params: OxiGDAL.EnhanceParams(
        brightness: 1.2,
        contrast: 1.5,
        saturation: 1.1,
        gamma: 0.9
    )
)

imageView.image = enhanced
```

## Memory Management

### Handling Memory Warnings

```swift
override func didReceiveMemoryWarning() {
    super.didReceiveMemoryWarning()

    // OxiGDAL will automatically reduce memory usage
    // Close unused datasets
    cachedDatasets.forEach { $0.close() }
    cachedDatasets.removeAll()
}
```

### Background Processing

```swift
func processInBackground() {
    DispatchQueue.global(qos: .userInitiated).async {
        do {
            let dataset = try OxiGDAL.open(self.largeMapPath)
            let image = try dataset.toImage()

            DispatchQueue.main.async {
                self.imageView.image = image
            }

            dataset.close()
        } catch {
            print("Background processing failed: \(error)")
        }
    }
}
```

## SwiftUI Integration

```swift
import SwiftUI
import OxiGDAL

struct MapView: View {
    @State private var mapImage: UIImage?
    @State private var error: String?
    let mapPath: String

    var body: some View {
        Group {
            if let image = mapImage {
                Image(uiImage: image)
                    .resizable()
                    .scaledToFit()
            } else if let error = error {
                Text("Error: \(error)")
                    .foregroundColor(.red)
            } else {
                ProgressView("Loading map...")
            }
        }
        .onAppear {
            loadMap()
        }
    }

    private func loadMap() {
        DispatchQueue.global().async {
            do {
                OxiGDAL.initialize()
                let dataset = try OxiGDAL.open(mapPath)
                let image = try dataset.toImage()
                dataset.close()

                DispatchQueue.main.async {
                    self.mapImage = image
                }
            } catch let error as OxiGDAL.Error {
                DispatchQueue.main.async {
                    self.error = error.localizedDescription
                }
            } catch {
                DispatchQueue.main.async {
                    self.error = error.localizedDescription
                }
            }
        }
    }
}

// Usage
struct ContentView: View {
    var body: some View {
        MapView(mapPath: "path/to/map.tif")
    }
}
```

## Best Practices

### 1. Initialization

Initialize OxiGDAL once at app startup:

```swift
@main
struct MyApp: App {
    init() {
        OxiGDAL.initialize()
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
        }
    }
}
```

### 2. Resource Management

Always close datasets when done:

```swift
// Using defer
func loadMap() throws {
    let dataset = try OxiGDAL.open("map.tif")
    defer { dataset.close() }

    // Use dataset...
    let image = try dataset.toImage()
}
```

### 3. Error Handling

Handle errors gracefully:

```swift
do {
    let dataset = try OxiGDAL.open(path)
    // Process dataset...
} catch OxiGDAL.Error.fileNotFound(let path) {
    print("Map file not found: \(path)")
} catch OxiGDAL.Error.unsupportedFormat(let format) {
    print("Unsupported format: \(format)")
} catch {
    print("Unknown error: \(error)")
}
```

### 4. Offline Maps

Store maps in app bundle or documents directory:

```swift
// Bundle resource
if let path = Bundle.main.path(forResource: "map", ofType: "tif") {
    let dataset = try OxiGDAL.open(path)
}

// Documents directory
let documentsPath = FileManager.default
    .urls(for: .documentDirectory, in: .userDomainMask)[0]
let mapURL = documentsPath.appendingPathComponent("map.tif")
let dataset = try OxiGDAL.open(mapURL.path)
```

### 5. Performance

For large datasets, read in tiles:

```swift
// Good: Read in chunks
let tileSize = 256
for y in stride(from: 0, to: height, by: tileSize) {
    for x in stride(from: 0, to: width, by: tileSize) {
        let tile = try dataset.readRegion(
            x: x, y: y,
            width: min(tileSize, width - x),
            height: min(tileSize, height - y),
            band: 1
        )
        processTile(tile)
    }
}

// Avoid: Reading entire large dataset
let image = try hugeDataset.toImage() // May cause memory issues
```

## Troubleshooting

### Linker Errors

If you see linker errors, ensure:
1. Library is added to "Link Binary With Libraries"
2. Build Settings → "Other Linker Flags" includes `-lc++` if needed
3. Correct architecture is selected (arm64 for device, x86_64 for simulator)

### Runtime Crashes

Check:
1. `OxiGDAL.initialize()` is called before use
2. Datasets are closed properly
3. File paths are correct and accessible
4. Sufficient memory is available

### Performance Issues

Optimize by:
1. Reading tiles instead of full images
2. Processing on background threads
3. Caching frequently used data
4. Using appropriate tile sizes (256 or 512 pixels)

## Example Project

See the complete example project in `examples/ios/OxiGDALDemo/`.

## Support

- GitHub Issues: https://github.com/cool-japan/oxigdal/issues
- Documentation: https://docs.rs/oxigdal
- License: Apache-2.0
