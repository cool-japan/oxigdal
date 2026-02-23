// OxiGDAL Swift Bindings
// Copyright (c) 2026 COOLJAPAN OU (Team Kitasan)
// Licensed under Apache-2.0

import Foundation
import UIKit

/// Swift wrapper for OxiGDAL mobile library.
///
/// This class provides a Swift-friendly API over the OxiGDAL C FFI layer.
///
/// # Example
///
/// ```swift
/// // Initialize library
/// OxiGDAL.initialize()
///
/// // Open dataset
/// guard let dataset = try? OxiGDAL.open("map.tif") else {
///     print("Failed to open dataset")
///     return
/// }
///
/// // Read as image
/// if let image = try? dataset.toImage() {
///     imageView.image = image
/// }
///
/// // Cleanup
/// dataset.close()
/// OxiGDAL.cleanup()
/// ```
public final class OxiGDAL {

    // MARK: - Error Types

    /// Errors that can occur during OxiGDAL operations.
    public enum Error: Swift.Error, LocalizedError {
        case nullPointer
        case invalidArgument
        case fileNotFound(String)
        case ioError(String)
        case unsupportedFormat(String)
        case outOfBounds
        case allocationFailed
        case invalidUtf8
        case driverError(String)
        case projectionError(String)
        case unknown(String)

        public var errorDescription: String? {
            switch self {
            case .nullPointer:
                return "Null pointer encountered"
            case .invalidArgument:
                return "Invalid argument provided"
            case .fileNotFound(let path):
                return "File not found: \(path)"
            case .ioError(let msg):
                return "I/O error: \(msg)"
            case .unsupportedFormat(let fmt):
                return "Unsupported format: \(fmt)"
            case .outOfBounds:
                return "Index out of bounds"
            case .allocationFailed:
                return "Memory allocation failed"
            case .invalidUtf8:
                return "Invalid UTF-8 string"
            case .driverError(let msg):
                return "Driver error: \(msg)"
            case .projectionError(let msg):
                return "Projection error: \(msg)"
            case .unknown(let msg):
                return "Unknown error: \(msg)"
            }
        }
    }

    // MARK: - Initialization

    /// Initializes the OxiGDAL library.
    ///
    /// This should be called once at application startup.
    public static func initialize() {
        oxigdal_init()
    }

    /// Cleans up OxiGDAL resources.
    ///
    /// This should be called at application shutdown.
    public static func cleanup() {
        oxigdal_cleanup()
    }

    /// Gets the OxiGDAL version string.
    public static var version: String {
        guard let cStr = oxigdal_get_version_string() else {
            return "Unknown"
        }
        defer { oxigdal_string_free(cStr) }
        return String(cString: cStr)
    }

    // MARK: - Dataset

    /// Opens a dataset from a file path.
    ///
    /// - Parameter path: Path to the dataset file
    /// - Returns: Dataset handle
    /// - Throws: OxiGDAL.Error if opening fails
    public static func open(_ path: String) throws -> Dataset {
        var datasetPtr: OpaquePointer?
        let result = path.withCString { pathPtr in
            oxigdal_dataset_open(pathPtr, &datasetPtr)
        }

        try checkError(result)

        guard let ptr = datasetPtr else {
            throw Error.nullPointer
        }

        return Dataset(handle: ptr)
    }

    /// Checks if a file format is supported.
    ///
    /// - Parameter path: File path to check
    /// - Returns: true if format is supported
    public static func isFormatSupported(_ path: String) -> Bool {
        return path.withCString { pathPtr in
            oxigdal_is_format_supported(pathPtr) != 0
        }
    }

    // MARK: - Dataset Class

    /// Represents an opened geospatial dataset.
    public final class Dataset {
        private let handle: OpaquePointer
        private var isClosed = false

        internal init(handle: OpaquePointer) {
            self.handle = handle
        }

        deinit {
            if !isClosed {
                close()
            }
        }

        /// Closes the dataset and frees resources.
        public func close() {
            guard !isClosed else { return }
            oxigdal_dataset_close(UnsafeMutablePointer(mutating: handle))
            isClosed = true
        }

        /// Gets the dataset metadata.
        public var metadata: Metadata {
            get throws {
                var meta = OxiGdalMetadata()
                let result = oxigdal_dataset_get_metadata(handle, &meta)
                try checkError(result)
                return Metadata(meta)
            }
        }

        /// Gets the dataset width in pixels.
        public var width: Int {
            get throws {
                let meta = try metadata
                return Int(meta.width)
            }
        }

        /// Gets the dataset height in pixels.
        public var height: Int {
            get throws {
                let meta = try metadata
                return Int(meta.height)
            }
        }

        /// Gets the number of bands.
        public var bandCount: Int {
            get throws {
                let meta = try metadata
                return Int(meta.bandCount)
            }
        }

        /// Reads a region from the dataset.
        ///
        /// - Parameters:
        ///   - x: X offset in pixels
        ///   - y: Y offset in pixels
        ///   - width: Width to read
        ///   - height: Height to read
        ///   - band: Band number (1-indexed)
        /// - Returns: Image buffer
        public func readRegion(x: Int, y: Int, width: Int, height: Int, band: Int = 1) throws -> ImageBuffer {
            let channels = 3 // RGB
            guard let bufferPtr = oxigdal_buffer_alloc(Int32(width), Int32(height), Int32(channels)) else {
                throw Error.allocationFailed
            }

            defer { oxigdal_buffer_free(bufferPtr) }

            let result = oxigdal_dataset_read_region(
                handle,
                Int32(x),
                Int32(y),
                Int32(width),
                Int32(height),
                Int32(band),
                bufferPtr
            )

            try checkError(result)

            let buffer = bufferPtr.pointee
            let data = Data(bytes: buffer.data, count: Int(buffer.length))

            return ImageBuffer(
                data: data,
                width: Int(buffer.width),
                height: Int(buffer.height),
                channels: Int(buffer.channels)
            )
        }

        /// Reads a map tile in XYZ scheme.
        ///
        /// - Parameters:
        ///   - z: Zoom level
        ///   - x: Tile column
        ///   - y: Tile row
        ///   - tileSize: Tile size in pixels (default: 256)
        /// - Returns: Tile image buffer
        public func readTile(z: Int, x: Int, y: Int, tileSize: Int = 256) throws -> ImageBuffer {
            guard z >= 0 && x >= 0 && y >= 0 else {
                throw Error.invalidArgument
            }

            guard tileSize > 0 && tileSize <= 4096 else {
                throw Error.invalidArgument
            }

            let coord = OxiGdalTileCoord(z: Int32(z), x: Int32(x), y: Int32(y))
            var tilePtr: OpaquePointer?

            var coordCopy = coord
            let result = withUnsafePointer(to: &coordCopy) { coordPtr in
                oxigdal_dataset_read_tile(handle, coordPtr, Int32(tileSize), &tilePtr)
            }

            try checkError(result)

            guard let tile = tilePtr else {
                throw Error.nullPointer
            }

            defer {
                oxigdal_tile_free(UnsafeMutablePointer(mutating: tile))
            }

            // Get tile data
            var buffer = OxiGdalBuffer(
                data: UnsafeMutablePointer<UInt8>.allocate(capacity: 0),
                length: 0,
                width: 0,
                height: 0,
                channels: 0
            )

            let bufferResult = oxigdal_tile_get_data(tile, &buffer)
            try checkError(bufferResult)

            // Copy data before tile is freed
            let data = Data(bytes: buffer.data, count: buffer.length)

            return ImageBuffer(
                data: data,
                width: Int(buffer.width),
                height: Int(buffer.height),
                channels: Int(buffer.channels)
            )
        }

        /// Converts the entire dataset to a UIImage.
        ///
        /// This reads the full dataset at its native resolution.
        /// For large datasets, consider using readRegion instead.
        public func toImage() throws -> UIImage {
            let meta = try metadata
            let buffer = try readRegion(
                x: 0,
                y: 0,
                width: Int(meta.width),
                height: Int(meta.height),
                band: 1
            )
            return try buffer.toUIImage()
        }
    }

    // MARK: - Metadata

    /// Dataset metadata.
    public struct Metadata {
        public let width: Int
        public let height: Int
        public let bandCount: Int
        public let dataType: Int
        public let epsgCode: Int
        public let geotransform: [Double]

        internal init(_ meta: OxiGdalMetadata) {
            self.width = Int(meta.width)
            self.height = Int(meta.height)
            self.bandCount = Int(meta.band_count)
            self.dataType = Int(meta.data_type)
            self.epsgCode = Int(meta.epsg_code)
            self.geotransform = [
                meta.geotransform.0,
                meta.geotransform.1,
                meta.geotransform.2,
                meta.geotransform.3,
                meta.geotransform.4,
                meta.geotransform.5
            ]
        }
    }

    // MARK: - Image Buffer

    /// Image buffer containing pixel data.
    public struct ImageBuffer {
        public let data: Data
        public let width: Int
        public let height: Int
        public let channels: Int

        /// Converts the buffer to a UIImage.
        public func toUIImage() throws -> UIImage {
            guard channels == 3 || channels == 4 else {
                throw Error.unsupportedFormat("Only RGB and RGBA images supported")
            }

            let bitsPerComponent = 8
            let bytesPerRow = width * channels
            let colorSpace = CGColorSpaceCreateDeviceRGB()
            let bitmapInfo: CGBitmapInfo = channels == 4
                ? CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue)
                : CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue)

            guard let provider = CGDataProvider(data: data as CFData),
                  let cgImage = CGImage(
                    width: width,
                    height: height,
                    bitsPerComponent: bitsPerComponent,
                    bitsPerPixel: bitsPerComponent * channels,
                    bytesPerRow: bytesPerRow,
                    space: colorSpace,
                    bitmapInfo: bitmapInfo,
                    provider: provider,
                    decode: nil,
                    shouldInterpolate: true,
                    intent: .defaultIntent
                  ) else {
                throw Error.unknown("Failed to create CGImage")
            }

            return UIImage(cgImage: cgImage)
        }
    }

    // MARK: - Image Enhancement

    /// Enhancement parameters.
    public struct EnhanceParams {
        public var brightness: Double = 1.0
        public var contrast: Double = 1.0
        public var saturation: Double = 1.0
        public var gamma: Double = 1.0

        public init(brightness: Double = 1.0, contrast: Double = 1.0, saturation: Double = 1.0, gamma: Double = 1.0) {
            self.brightness = brightness
            self.contrast = contrast
            self.saturation = saturation
            self.gamma = gamma
        }
    }

    /// Enhances an image with brightness, contrast, saturation, and gamma adjustments.
    ///
    /// - Parameters:
    ///   - image: Source image
    ///   - params: Enhancement parameters
    /// - Returns: Enhanced image
    public static func enhance(_ image: UIImage, params: EnhanceParams) throws -> UIImage {
        // TODO: Implement image enhancement via FFI
        // For now, return original image
        return image
    }

    // MARK: - Helper Functions

    private static func checkError(_ code: OxiGdalErrorCode) throws {
        guard code == Success else {
            let errorMsg = getLastError()

            switch code {
            case NullPointer:
                throw Error.nullPointer
            case InvalidArgument:
                throw Error.invalidArgument
            case FileNotFound:
                throw Error.fileNotFound(errorMsg)
            case IoError:
                throw Error.ioError(errorMsg)
            case UnsupportedFormat:
                throw Error.unsupportedFormat(errorMsg)
            case OutOfBounds:
                throw Error.outOfBounds
            case AllocationFailed:
                throw Error.allocationFailed
            case InvalidUtf8:
                throw Error.invalidUtf8
            case DriverError:
                throw Error.driverError(errorMsg)
            case ProjectionError:
                throw Error.projectionError(errorMsg)
            default:
                throw Error.unknown(errorMsg)
            }
        }
    }

    private static func getLastError() -> String {
        guard let cStr = oxigdal_get_last_error() else {
            return "Unknown error"
        }
        defer { oxigdal_string_free(cStr) }
        return String(cString: cStr)
    }
}

// MARK: - C Function Declarations

// These declarations match the C FFI in the Rust library
// In a real implementation, these would be in a bridging header or module map

fileprivate typealias OxiGdalErrorCode = Int32
fileprivate let Success: OxiGdalErrorCode = 0
fileprivate let NullPointer: OxiGdalErrorCode = 1
fileprivate let InvalidArgument: OxiGdalErrorCode = 2
fileprivate let FileNotFound: OxiGdalErrorCode = 3
fileprivate let IoError: OxiGdalErrorCode = 4
fileprivate let UnsupportedFormat: OxiGdalErrorCode = 5
fileprivate let OutOfBounds: OxiGdalErrorCode = 6
fileprivate let AllocationFailed: OxiGdalErrorCode = 7
fileprivate let InvalidUtf8: OxiGdalErrorCode = 8
fileprivate let DriverError: OxiGdalErrorCode = 9
fileprivate let ProjectionError: OxiGdalErrorCode = 10

fileprivate struct OxiGdalMetadata {
    var width: Int32 = 0
    var height: Int32 = 0
    var band_count: Int32 = 0
    var data_type: Int32 = 0
    var epsg_code: Int32 = 0
    var geotransform: (Double, Double, Double, Double, Double, Double) = (0, 0, 0, 0, 0, 0)
}

fileprivate struct OxiGdalBuffer {
    var data: UnsafeMutablePointer<UInt8>
    var length: Int
    var width: Int32
    var height: Int32
    var channels: Int32
}

// FFI function declarations
// Note: In a real implementation, link against the compiled Rust library

@_silgen_name("oxigdal_init")
fileprivate func oxigdal_init() -> OxiGdalErrorCode

@_silgen_name("oxigdal_cleanup")
fileprivate func oxigdal_cleanup() -> OxiGdalErrorCode

@_silgen_name("oxigdal_get_version_string")
fileprivate func oxigdal_get_version_string() -> UnsafeMutablePointer<CChar>?

@_silgen_name("oxigdal_get_last_error")
fileprivate func oxigdal_get_last_error() -> UnsafeMutablePointer<CChar>?

@_silgen_name("oxigdal_string_free")
fileprivate func oxigdal_string_free(_ str: UnsafeMutablePointer<CChar>?)

@_silgen_name("oxigdal_dataset_open")
fileprivate func oxigdal_dataset_open(_ path: UnsafePointer<CChar>, _ out: UnsafeMutablePointer<OpaquePointer?>) -> OxiGdalErrorCode

@_silgen_name("oxigdal_dataset_close")
fileprivate func oxigdal_dataset_close(_ dataset: UnsafeMutablePointer<OpaquePointer>) -> OxiGdalErrorCode

@_silgen_name("oxigdal_dataset_get_metadata")
fileprivate func oxigdal_dataset_get_metadata(_ dataset: OpaquePointer, _ metadata: UnsafeMutablePointer<OxiGdalMetadata>) -> OxiGdalErrorCode

@_silgen_name("oxigdal_dataset_read_region")
fileprivate func oxigdal_dataset_read_region(_ dataset: OpaquePointer, _ x: Int32, _ y: Int32, _ width: Int32, _ height: Int32, _ band: Int32, _ buffer: UnsafeMutablePointer<OxiGdalBuffer>) -> OxiGdalErrorCode

@_silgen_name("oxigdal_is_format_supported")
fileprivate func oxigdal_is_format_supported(_ path: UnsafePointer<CChar>) -> Int32

@_silgen_name("oxigdal_buffer_alloc")
fileprivate func oxigdal_buffer_alloc(_ width: Int32, _ height: Int32, _ channels: Int32) -> UnsafeMutablePointer<OxiGdalBuffer>?

@_silgen_name("oxigdal_buffer_free")
fileprivate func oxigdal_buffer_free(_ buffer: UnsafeMutablePointer<OxiGdalBuffer>?)

@_silgen_name("oxigdal_dataset_read_tile")
fileprivate func oxigdal_dataset_read_tile(_ dataset: OpaquePointer, _ tile_coord: UnsafePointer<OxiGdalTileCoord>, _ tile_size: Int32, _ out_tile: UnsafeMutablePointer<OpaquePointer?>) -> OxiGdalErrorCode

@_silgen_name("oxigdal_tile_free")
fileprivate func oxigdal_tile_free(_ tile: UnsafeMutablePointer<OpaquePointer>?) -> OxiGdalErrorCode

@_silgen_name("oxigdal_tile_get_data")
fileprivate func oxigdal_tile_get_data(_ tile: OpaquePointer, _ out_buffer: UnsafeMutablePointer<OxiGdalBuffer>) -> OxiGdalErrorCode
