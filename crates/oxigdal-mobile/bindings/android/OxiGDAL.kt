// OxiGDAL Kotlin Bindings
// Copyright (c) 2026 COOLJAPAN OU (Team Kitasan)
// Licensed under Apache-2.0

package com.cooljapan.oxigdal

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import java.io.File
import java.nio.ByteBuffer

/**
 * OxiGDAL - Pure Rust geospatial library for Android
 *
 * This class provides a Kotlin-friendly API over the OxiGDAL native library.
 *
 * # Example
 *
 * ```kotlin
 * // Initialize library
 * OxiGDAL.initialize()
 *
 * // Open dataset
 * val dataset = OxiGDAL.open("/path/to/map.tif")
 *
 * // Read as bitmap
 * val bitmap = dataset.toBitmap()
 * imageView.setImageBitmap(bitmap)
 *
 * // Cleanup
 * dataset.close()
 * OxiGDAL.cleanup()
 * ```
 */
object OxiGDAL {

    // MARK: - Error Types

    /**
     * Base exception for OxiGDAL errors
     */
    sealed class OxiGDALException(message: String) : Exception(message)

    class NullPointerException : OxiGDALException("Null pointer encountered")
    class InvalidArgumentException(msg: String = "Invalid argument") : OxiGDALException(msg)
    class FileNotFoundException(path: String) : OxiGDALException("File not found: $path")
    class IOErrorException(msg: String) : OxiGDALException("I/O error: $msg")
    class UnsupportedFormatException(format: String) : OxiGDALException("Unsupported format: $format")
    class OutOfBoundsException : OxiGDALException("Index out of bounds")
    class AllocationFailedException : OxiGDALException("Memory allocation failed")
    class InvalidUtf8Exception : OxiGDALException("Invalid UTF-8 string")
    class DriverErrorException(msg: String) : OxiGDALException("Driver error: $msg")
    class ProjectionErrorException(msg: String) : OxiGDALException("Projection error: $msg")
    class UnknownException(msg: String) : OxiGDALException("Unknown error: $msg")

    // Error code constants
    private const val SUCCESS = 0
    private const val NULL_POINTER = 1
    private const val INVALID_ARGUMENT = 2
    private const val FILE_NOT_FOUND = 3
    private const val IO_ERROR = 4
    private const val UNSUPPORTED_FORMAT = 5
    private const val OUT_OF_BOUNDS = 6
    private const val ALLOCATION_FAILED = 7
    private const val INVALID_UTF8 = 8
    private const val DRIVER_ERROR = 9
    private const val PROJECTION_ERROR = 10

    init {
        System.loadLibrary("oxigdal_mobile")
    }

    // MARK: - Initialization

    /**
     * Initializes the OxiGDAL library.
     *
     * This should be called once at application startup.
     */
    fun initialize() {
        val result = nativeInit()
        checkError(result)
    }

    /**
     * Cleans up OxiGDAL resources.
     *
     * This should be called at application shutdown.
     */
    fun cleanup() {
        // Native cleanup if needed
    }

    /**
     * Gets the OxiGDAL version string.
     */
    val version: String
        get() = nativeGetVersion() ?: "Unknown"

    /**
     * Checks if a file format is supported.
     *
     * @param path File path to check
     * @return true if format is supported
     */
    fun isFormatSupported(path: String): Boolean {
        return File(path).extension.lowercase() in listOf(
            "tif", "tiff", "geotiff",
            "json", "geojson",
            "shp", "shapefile",
            "gpkg", "geopackage",
            "png", "jpg", "jpeg"
        )
    }

    /**
     * Opens a dataset from a file path.
     *
     * @param path Path to the dataset file
     * @return Dataset handle
     * @throws OxiGDALException if opening fails
     */
    fun open(path: String): Dataset {
        val ptr = nativeOpenDataset(path)
        if (ptr == 0L) {
            throw FileNotFoundException(path)
        }
        return Dataset(ptr)
    }

    // MARK: - Dataset Class

    /**
     * Represents an opened geospatial dataset.
     */
    class Dataset internal constructor(private val handle: Long) : AutoCloseable {
        private var isClosed = false

        /**
         * Closes the dataset and frees resources.
         */
        override fun close() {
            if (!isClosed) {
                nativeCloseDataset(handle)
                isClosed = true
            }
        }

        /**
         * Gets the dataset metadata.
         */
        val metadata: Metadata
            get() {
                checkClosed()
                return Metadata(
                    width = nativeGetWidth(handle),
                    height = nativeGetHeight(handle),
                    bandCount = nativeGetBandCount(handle),
                    dataType = nativeGetDataType(handle),
                    epsgCode = nativeGetEpsgCode(handle)
                )
            }

        /**
         * Gets the dataset width in pixels.
         */
        val width: Int
            get() {
                checkClosed()
                return nativeGetWidth(handle)
            }

        /**
         * Gets the dataset height in pixels.
         */
        val height: Int
            get() {
                checkClosed()
                return nativeGetHeight(handle)
            }

        /**
         * Reads a region from the dataset.
         *
         * @param x X offset in pixels
         * @param y Y offset in pixels
         * @param width Width to read
         * @param height Height to read
         * @param band Band number (1-indexed)
         * @return Image buffer
         */
        fun readRegion(
            x: Int,
            y: Int,
            width: Int,
            height: Int,
            band: Int = 1
        ): ImageBuffer {
            checkClosed()

            val data = nativeReadRegion(handle, x, y, width, height, band)
                ?: throw AllocationFailedException()

            return ImageBuffer(
                data = data,
                width = width,
                height = height,
                channels = 3
            )
        }

        /**
         * Reads a map tile in XYZ scheme.
         *
         * @param z Zoom level
         * @param x Tile column
         * @param y Tile row
         * @param tileSize Tile size in pixels (default: 256)
         * @return Tile image buffer
         */
        fun readTile(
            z: Int,
            x: Int,
            y: Int,
            tileSize: Int = 256
        ): ImageBuffer {
            checkClosed()

            if (z < 0 || x < 0 || y < 0) {
                throw InvalidArgumentException("Tile coordinates must be non-negative")
            }

            if (tileSize <= 0 || tileSize > 4096) {
                throw InvalidArgumentException("Tile size must be between 1 and 4096")
            }

            val data = nativeReadTile(handle, z, x, y, tileSize)
                ?: throw IOErrorException("Failed to read tile")

            return ImageBuffer(
                data = data,
                width = tileSize,
                height = tileSize,
                channels = 3
            )
        }

        /**
         * Converts the entire dataset to a Bitmap.
         *
         * This reads the full dataset at its native resolution.
         * For large datasets, consider using readRegion instead.
         */
        fun toBitmap(): Bitmap {
            checkClosed()
            val buffer = readRegion(0, 0, width, height, 1)
            return buffer.toBitmap()
        }

        /**
         * Converts a region to a Bitmap.
         */
        fun toBitmap(x: Int, y: Int, width: Int, height: Int): Bitmap {
            checkClosed()
            val buffer = readRegion(x, y, width, height, 1)
            return buffer.toBitmap()
        }

        private fun checkClosed() {
            if (isClosed) {
                throw IllegalStateException("Dataset is closed")
            }
        }
    }

    // MARK: - Metadata

    /**
     * Dataset metadata.
     */
    data class Metadata(
        val width: Int,
        val height: Int,
        val bandCount: Int,
        val dataType: Int,
        val epsgCode: Int
    )

    // MARK: - Image Buffer

    /**
     * Image buffer containing pixel data.
     */
    data class ImageBuffer(
        val data: ByteArray,
        val width: Int,
        val height: Int,
        val channels: Int
    ) {
        /**
         * Converts the buffer to a Bitmap.
         */
        fun toBitmap(): Bitmap {
            require(channels == 3 || channels == 4) {
                "Only RGB and RGBA images supported"
            }

            val config = if (channels == 4) {
                Bitmap.Config.ARGB_8888
            } else {
                Bitmap.Config.RGB_565
            }

            val bitmap = Bitmap.createBitmap(width, height, config)

            // Convert RGB to ARGB if needed
            val argbData = if (channels == 3) {
                ByteArray(width * height * 4).also { argb ->
                    var srcIdx = 0
                    var dstIdx = 0
                    repeat(width * height) {
                        argb[dstIdx++] = 0xFF.toByte()        // A
                        argb[dstIdx++] = data[srcIdx++]       // R
                        argb[dstIdx++] = data[srcIdx++]       // G
                        argb[dstIdx++] = data[srcIdx++]       // B
                    }
                }
            } else {
                data
            }

            val buffer = ByteBuffer.wrap(argbData)
            bitmap.copyPixelsFromBuffer(buffer)

            return bitmap
        }

        override fun equals(other: Any?): Boolean {
            if (this === other) return true
            if (javaClass != other?.javaClass) return false

            other as ImageBuffer

            if (!data.contentEquals(other.data)) return false
            if (width != other.width) return false
            if (height != other.height) return false
            if (channels != other.channels) return false

            return true
        }

        override fun hashCode(): Int {
            var result = data.contentHashCode()
            result = 31 * result + width
            result = 31 * result + height
            result = 31 * result + channels
            return result
        }
    }

    // MARK: - Image Enhancement

    /**
     * Enhancement parameters.
     */
    data class EnhanceParams(
        val brightness: Float = 1.0f,
        val contrast: Float = 1.0f,
        val saturation: Float = 1.0f,
        val gamma: Float = 1.0f
    )

    /**
     * Enhances an image with brightness, contrast, saturation, and gamma adjustments.
     *
     * @param bitmap Source bitmap
     * @param params Enhancement parameters
     * @return Enhanced bitmap
     */
    fun enhance(bitmap: Bitmap, params: EnhanceParams): Bitmap {
        // TODO: Implement image enhancement via native code
        // For now, return copy of original
        return bitmap.copy(bitmap.config, true)
    }

    // MARK: - Helper Functions

    private fun checkError(code: Int, defaultMessage: String = "Unknown error") {
        when (code) {
            SUCCESS -> return
            NULL_POINTER -> throw NullPointerException()
            INVALID_ARGUMENT -> throw InvalidArgumentException()
            FILE_NOT_FOUND -> throw FileNotFoundException(defaultMessage)
            IO_ERROR -> throw IOErrorException(defaultMessage)
            UNSUPPORTED_FORMAT -> throw UnsupportedFormatException(defaultMessage)
            OUT_OF_BOUNDS -> throw OutOfBoundsException()
            ALLOCATION_FAILED -> throw AllocationFailedException()
            INVALID_UTF8 -> throw InvalidUtf8Exception()
            DRIVER_ERROR -> throw DriverErrorException(defaultMessage)
            PROJECTION_ERROR -> throw ProjectionErrorException(defaultMessage)
            else -> throw UnknownException(defaultMessage)
        }
    }

    // MARK: - Native Methods

    @JvmStatic
    private external fun nativeInit(): Int

    @JvmStatic
    private external fun nativeGetVersion(): String?

    @JvmStatic
    private external fun nativeOpenDataset(path: String): Long

    @JvmStatic
    private external fun nativeCloseDataset(datasetPtr: Long)

    @JvmStatic
    private external fun nativeGetWidth(datasetPtr: Long): Int

    @JvmStatic
    private external fun nativeGetHeight(datasetPtr: Long): Int

    @JvmStatic
    private external fun nativeGetBandCount(datasetPtr: Long): Int

    @JvmStatic
    private external fun nativeGetDataType(datasetPtr: Long): Int

    @JvmStatic
    private external fun nativeGetEpsgCode(datasetPtr: Long): Int

    @JvmStatic
    private external fun nativeReadRegion(
        datasetPtr: Long,
        x: Int,
        y: Int,
        width: Int,
        height: Int,
        band: Int
    ): ByteArray?

    @JvmStatic
    private external fun nativeReadTile(
        datasetPtr: Long,
        z: Int,
        x: Int,
        y: Int,
        tileSize: Int
    ): ByteArray?
}

// MARK: - Extension Functions

/**
 * Extension function to use Dataset with 'use' for automatic resource management.
 */
inline fun <R> OxiGDAL.Dataset.use(block: (OxiGDAL.Dataset) -> R): R {
    try {
        return block(this)
    } finally {
        close()
    }
}
