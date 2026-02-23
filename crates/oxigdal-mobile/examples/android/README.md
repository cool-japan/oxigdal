# OxiGDAL Android Integration Guide

This guide explains how to integrate OxiGDAL into your Android application.

## Requirements

- Android 7.0 (API 24)+
- Android Studio Arctic Fox+
- Kotlin 1.9+
- Rust toolchain with Android NDK

## Installation

### Option 1: Gradle (Recommended)

Add to your `build.gradle.kts` (Module level):

```kotlin
dependencies {
    implementation("com.cooljapan:oxigdal:0.1.0")
}
```

### Option 2: AAR Library

1. Download `oxigdal-mobile.aar` from releases
2. Place in `app/libs/`
3. Add to `build.gradle.kts`:

```kotlin
dependencies {
    implementation(files("libs/oxigdal-mobile.aar"))
}
```

### Option 3: Manual Integration

1. **Set up Android NDK**:

```bash
# Install Android NDK via Android Studio or sdkmanager
# Set environment variables
export ANDROID_NDK_HOME=$HOME/Android/Sdk/ndk/26.0.10792818
```

2. **Add Android targets**:

```bash
rustup target add aarch64-linux-android
rustup target add armv7-linux-androideabi
rustup target add x86_64-linux-android
```

3. **Create cargo config** (`.cargo/config.toml`):

```toml
[target.aarch64-linux-android]
ar = "aarch64-linux-android-ar"
linker = "aarch64-linux-android21-clang"

[target.armv7-linux-androideabi]
ar = "armv7a-linux-androideabi-ar"
linker = "armv7a-linux-androideabi21-clang"

[target.x86_64-linux-android]
ar = "x86_64-linux-android-ar"
linker = "x86_64-linux-android21-clang"
```

4. **Build for Android**:

```bash
# ARM64 (most devices)
cargo build --release --target aarch64-linux-android

# ARMv7 (older devices)
cargo build --release --target armv7-linux-androideabi

# x86_64 (emulator)
cargo build --release --target x86_64-linux-android
```

5. **Copy libraries to Android project**:

```bash
mkdir -p app/src/main/jniLibs/arm64-v8a
mkdir -p app/src/main/jniLibs/armeabi-v7a
mkdir -p app/src/main/jniLibs/x86_64

cp target/aarch64-linux-android/release/liboxigdal_mobile.so \
   app/src/main/jniLibs/arm64-v8a/

cp target/armv7-linux-androideabi/release/liboxigdal_mobile.so \
   app/src/main/jniLibs/armeabi-v7a/

cp target/x86_64-linux-android/release/liboxigdal_mobile.so \
   app/src/main/jniLibs/x86_64/
```

6. **Add Kotlin wrapper**:
   - Copy `OxiGDAL.kt` to your project's source directory

## Quick Start

### Application Setup

Initialize OxiGDAL in your Application class:

```kotlin
import android.app.Application
import com.cooljapan.oxigdal.OxiGDAL

class MyApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        OxiGDAL.initialize()
    }

    override fun onLowMemory() {
        super.onLowMemory()
        // OxiGDAL will handle memory cleanup automatically
    }
}
```

Register in `AndroidManifest.xml`:

```xml
<application
    android:name=".MyApplication"
    ...>
</application>
```

### Basic Usage

```kotlin
import com.cooljapan.oxigdal.OxiGDAL
import android.graphics.Bitmap

class MapActivity : AppCompatActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_map)

        loadMap()
    }

    private fun loadMap() {
        try {
            // Open COG file
            val mapPath = File(filesDir, "map.tif").absolutePath
            val dataset = OxiGDAL.open(mapPath)

            // Get metadata
            val metadata = dataset.metadata
            Log.d("Map", "Size: ${metadata.width}x${metadata.height}")
            Log.d("Map", "Bands: ${metadata.bandCount}")

            // Read entire image
            val bitmap = dataset.toBitmap()
            imageView.setImageBitmap(bitmap)

            // Clean up
            dataset.close()

        } catch (e: OxiGDAL.OxiGDALException) {
            Log.e("Map", "Error: ${e.message}")
        }
    }
}
```

### Using with Resource Management

```kotlin
// Automatic resource cleanup with 'use'
OxiGDAL.open(mapPath).use { dataset ->
    val bitmap = dataset.toBitmap()
    imageView.setImageBitmap(bitmap)
} // Dataset automatically closed
```

### Reading Regions

```kotlin
val dataset = OxiGDAL.open("large_map.tif")

// Read 512x512 region starting at (1000, 1000)
val buffer = dataset.readRegion(
    x = 1000,
    y = 1000,
    width = 512,
    height = 512,
    band = 1
)

val bitmap = buffer.toBitmap()
imageView.setImageBitmap(bitmap)

dataset.close()
```

### Coroutine Support

```kotlin
import kotlinx.coroutines.*

class MapViewModel : ViewModel() {
    private val _mapBitmap = MutableLiveData<Bitmap>()
    val mapBitmap: LiveData<Bitmap> = _mapBitmap

    fun loadMap(path: String) {
        viewModelScope.launch {
            try {
                val bitmap = withContext(Dispatchers.IO) {
                    OxiGDAL.open(path).use { dataset ->
                        dataset.toBitmap()
                    }
                }
                _mapBitmap.value = bitmap
            } catch (e: OxiGDAL.OxiGDALException) {
                Log.e("MapViewModel", "Error loading map", e)
            }
        }
    }
}
```

### Tile-Based Loading

```kotlin
class TileLoader(private val dataset: OxiGDAL.Dataset) {
    private val tileCache = LruCache<Triple<Int, Int, Int>, Bitmap>(100)

    fun loadTile(z: Int, x: Int, y: Int, tileSize: Int = 256): Bitmap {
        val key = Triple(z, x, y)

        return tileCache.get(key) ?: run {
            val buffer = dataset.readTile(z, x, y, tileSize)
            val bitmap = buffer.toBitmap()
            tileCache.put(key, bitmap)
            bitmap
        }
    }
}

// Usage
val dataset = OxiGDAL.open("offline_map.tif")
val tileLoader = TileLoader(dataset)
val tile = tileLoader.loadTile(z = 10, x = 512, y = 341)
```

### Image Enhancement

```kotlin
val bitmap = dataset.toBitmap()

val enhanced = OxiGDAL.enhance(
    bitmap,
    OxiGDAL.EnhanceParams(
        brightness = 1.2f,
        contrast = 1.5f,
        saturation = 1.1f,
        gamma = 0.9f
    )
)

imageView.setImageBitmap(enhanced)
```

## Jetpack Compose Integration

```kotlin
import androidx.compose.runtime.*
import androidx.compose.foundation.Image
import androidx.compose.ui.graphics.asImageBitmap

@Composable
fun MapView(mapPath: String) {
    var bitmap by remember { mutableStateOf<Bitmap?>(null) }
    var error by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(mapPath) {
        withContext(Dispatchers.IO) {
            try {
                val loaded = OxiGDAL.open(mapPath).use { dataset ->
                    dataset.toBitmap()
                }
                bitmap = loaded
            } catch (e: OxiGDAL.OxiGDALException) {
                error = e.message
            }
        }
    }

    when {
        bitmap != null -> {
            Image(
                bitmap = bitmap!!.asImageBitmap(),
                contentDescription = "Map",
                modifier = Modifier.fillMaxSize()
            )
        }
        error != null -> {
            Text(
                text = "Error: $error",
                color = Color.Red
            )
        }
        else -> {
            CircularProgressIndicator()
        }
    }
}

// Usage
@Composable
fun MapScreen() {
    MapView(mapPath = "/sdcard/maps/map.tif")
}
```

## Memory Management

### Handling Low Memory

Implement in your Application class:

```kotlin
class MyApplication : Application() {
    override fun onLowMemory() {
        super.onLowMemory()
        // Clear caches
        bitmapCache.evictAll()
    }

    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        when (level) {
            ComponentCallbacks2.TRIM_MEMORY_UI_HIDDEN,
            ComponentCallbacks2.TRIM_MEMORY_BACKGROUND,
            ComponentCallbacks2.TRIM_MEMORY_MODERATE -> {
                // Clear some caches
                bitmapCache.trimToSize(bitmapCache.size() / 2)
            }
            ComponentCallbacks2.TRIM_MEMORY_RUNNING_LOW,
            ComponentCallbacks2.TRIM_MEMORY_COMPLETE -> {
                // Clear all caches
                bitmapCache.evictAll()
            }
        }
    }
}
```

### Efficient Bitmap Loading

```kotlin
// Load bitmap with appropriate size
fun loadScaledBitmap(dataset: OxiGDAL.Dataset, maxSize: Int): Bitmap {
    val width = dataset.width
    val height = dataset.height

    // Calculate scale factor
    val scale = minOf(
        maxSize.toFloat() / width,
        maxSize.toFloat() / height,
        1.0f
    )

    val scaledWidth = (width * scale).toInt()
    val scaledHeight = (height * scale).toInt()

    // Read at target size
    val buffer = dataset.readRegion(0, 0, scaledWidth, scaledHeight)
    return buffer.toBitmap()
}
```

## Best Practices

### 1. Permissions

Add required permissions to `AndroidManifest.xml`:

```xml
<uses-permission android:name="android.permission.READ_EXTERNAL_STORAGE" />
<uses-permission android:name="android.permission.WRITE_EXTERNAL_STORAGE"
    android:maxSdkVersion="28" />
```

Request at runtime (Android 6.0+):

```kotlin
if (ContextCompat.checkSelfPermission(this, Manifest.permission.READ_EXTERNAL_STORAGE)
    != PackageManager.PERMISSION_GRANTED) {
    ActivityCompat.requestPermissions(
        this,
        arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE),
        REQUEST_CODE
    )
}
```

### 2. Background Processing

Always process large files on background threads:

```kotlin
// Good: Background thread
lifecycleScope.launch(Dispatchers.IO) {
    val bitmap = OxiGDAL.open(path).use { it.toBitmap() }
    withContext(Dispatchers.Main) {
        imageView.setImageBitmap(bitmap)
    }
}

// Bad: UI thread (will cause ANR)
val bitmap = OxiGDAL.open(path).use { it.toBitmap() }
```

### 3. Error Handling

Handle specific exceptions:

```kotlin
try {
    val dataset = OxiGDAL.open(path)
    // Process...
} catch (e: OxiGDAL.FileNotFoundException) {
    showError("Map file not found")
} catch (e: OxiGDAL.UnsupportedFormatException) {
    showError("Unsupported map format")
} catch (e: OxiGDAL.AllocationFailedException) {
    showError("Not enough memory")
} catch (e: OxiGDAL.OxiGDALException) {
    showError("Error: ${e.message}")
}
```

### 4. File Locations

Store maps in appropriate locations:

```kotlin
// Internal storage (private to app)
val internalFile = File(context.filesDir, "map.tif")

// External storage (shared)
val externalFile = File(
    Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS),
    "maps/map.tif"
)

// Cache directory (may be cleared)
val cacheFile = File(context.cacheDir, "temp_map.tif")
```

### 5. ProGuard Rules

Add to `proguard-rules.pro`:

```proguard
# Keep OxiGDAL native methods
-keep class com.cooljapan.oxigdal.OxiGDAL { *; }
-keepclassmembers class com.cooljapan.oxigdal.OxiGDAL {
    native <methods>;
}
```

## Performance Tips

### 1. Tile Loading

Load large maps in tiles:

```kotlin
suspend fun loadTiledMap(dataset: OxiGDAL.Dataset, tileSize: Int = 256) = coroutineScope {
    val width = dataset.width
    val height = dataset.height
    val tiles = mutableListOf<Deferred<Bitmap>>()

    for (y in 0 until height step tileSize) {
        for (x in 0 until width step tileSize) {
            tiles.add(async(Dispatchers.Default) {
                val w = minOf(tileSize, width - x)
                val h = minOf(tileSize, height - y)
                dataset.readRegion(x, y, w, h).toBitmap()
            })
        }
    }

    tiles.awaitAll()
}
```

### 2. Caching

Use LruCache for frequently accessed data:

```kotlin
private val bitmapCache = object : LruCache<String, Bitmap>(
    // Cache size: 25% of available memory
    (Runtime.getRuntime().maxMemory() / 4).toInt()
) {
    override fun sizeOf(key: String, bitmap: Bitmap): Int {
        return bitmap.byteCount
    }
}

fun getCachedBitmap(path: String): Bitmap {
    return bitmapCache.get(path) ?: run {
        val bitmap = OxiGDAL.open(path).use { it.toBitmap() }
        bitmapCache.put(path, bitmap)
        bitmap
    }
}
```

## Troubleshooting

### UnsatisfiedLinkError

If you see `UnsatisfiedLinkError`, check:
1. `.so` files are in correct `jniLibs/` directories
2. Library name matches (`liboxigdal_mobile.so`)
3. Correct ABI is included (arm64-v8a for most devices)

### Out of Memory

For large files:
1. Read in smaller regions
2. Use smaller tile sizes
3. Process in background
4. Implement proper caching with limits

### File Not Found

Verify:
1. File path is absolute
2. File exists and is readable
3. Permissions are granted
4. External storage is mounted

## Example Project

See the complete example project in `examples/android/OxiGDALDemo/`.

Build and run:

```bash
cd examples/android/OxiGDALDemo
./gradlew assembleDebug
./gradlew installDebug
```

## Support

- GitHub Issues: https://github.com/cool-japan/oxigdal/issues
- Documentation: https://docs.rs/oxigdal
- License: Apache-2.0
