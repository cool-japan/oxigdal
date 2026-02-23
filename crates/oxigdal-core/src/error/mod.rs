//! Error types for `OxiGDAL`
//!
//! This module provides a comprehensive error hierarchy for all `OxiGDAL` operations.
//! All error types implement [`std::error::Error`] via [`thiserror`].
//!
//! # Error Codes
//!
//! Each error variant has an associated error code (e.g., E001, E002) for easier
//! debugging and documentation. Error codes are stable across versions.
//!
//! # Helper Methods
//!
//! All error types provide:
//! - `code()` - Returns the error code
//! - `suggestion()` - Returns helpful hints for fixing the error
//! - `context()` - Returns additional context about the error
//!
//! # Builder Pattern
//!
//! For simple errors, use the direct constructors:
//!
//! ```ignore
//! use oxigdal_core::error::OxiGdalError;
//!
//! let err = OxiGdalError::io_error("Cannot read file");
//! ```
//!
//! For errors with rich context, use the builder pattern via [`ErrorBuilder`]:
//!
//! ```ignore
//! use oxigdal_core::error::OxiGdalError;
//!
//! let err = OxiGdalError::io_error_builder("Cannot read file")
//!     .with_path("/data/file.tif")
//!     .with_operation("read_raster")
//!     .with_suggestion("Check file permissions")
//!     .build();
//! ```
//!
//! # When to Use Which Error Type
//!
//! - **IoError**: File I/O, network operations, HTTP requests
//! - **FormatError**: File format parsing, magic number validation, header parsing
//! - **CrsError**: Coordinate system operations, transformations, WKT/EPSG handling
//! - **CompressionError**: Compression/decompression operations
//! - **InvalidParameter**: Parameter validation failures
//! - **NotSupported**: Unsupported features or operations
//! - **OutOfBounds**: Index or range validation failures
//! - **Internal**: Internal invariant violations, allocation failures
//!
//! # Examples of Rich Error Messages
//!
//! ## File Not Found with Context
//!
//! ```ignore
//! use oxigdal_core::error::OxiGdalError;
//! use std::path::Path;
//!
//! fn read_geotiff(path: &Path) -> Result<(), OxiGdalError> {
//!     if !path.exists() {
//!         return Err(OxiGdalError::io_error_builder("GeoTIFF file not found")
//!             .with_path(path)
//!             .with_operation("read_geotiff")
//!             .with_suggestion("Verify the file path and ensure the file exists")
//!             .build());
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ## Parameter Validation with Constraints
//!
//! ```ignore
//! use oxigdal_core::error::{OxiGdalError, Result};
//!
//! fn create_raster(width: usize, height: usize) -> Result<()> {
//!     if width == 0 || width > 65535 {
//!         return Err(OxiGdalError::invalid_parameter_builder("width", "must be between 1 and 65535")
//!             .with_parameter("value", width.to_string())
//!             .with_parameter("min", "1")
//!             .with_parameter("max", "65535")
//!             .with_operation("create_raster")
//!             .build());
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ## Format Error with Details
//!
//! ```ignore
//! use oxigdal_core::error::{OxiGdalError, FormatError};
//!
//! fn parse_header(data: &[u8]) -> Result<(), OxiGdalError> {
//!     if data.len() < 4 {
//!         return Err(FormatError::InvalidHeader {
//!             message: format!("Header too short: expected at least 4 bytes, got {}", data.len())
//!         }.into());
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ## CRS Transformation Error
//!
//! ```ignore
//! use oxigdal_core::error::{OxiGdalError, CrsError};
//!
//! fn transform_coordinates(src_epsg: u32, dst_epsg: u32) -> Result<(), OxiGdalError> {
//!     if src_epsg == dst_epsg {
//!         return Err(CrsError::TransformationError {
//!             source_crs: format!("EPSG:{}", src_epsg),
//!             target_crs: format!("EPSG:{}", dst_epsg),
//!             message: "Source and target CRS are identical".to_string(),
//!         }.into());
//!     }
//!     Ok(())
//! }
//! ```

pub mod builder;
pub mod extensions;
pub mod methods;
pub mod types;

pub use builder::*;
pub use extensions::*;
pub use types::*;

/// The main result type for `OxiGDAL` operations
pub type Result<T> = core::result::Result<T, OxiGdalError>;

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::useless_vec)]

    use super::*;

    #[test]
    fn test_error_display() {
        let err = OxiGdalError::InvalidParameter {
            parameter: "width",
            message: "must be positive".to_string(),
        };
        assert!(err.to_string().contains("width"));
        assert!(err.to_string().contains("must be positive"));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = IoError::NotFound {
            path: "/test/path".to_string(),
        };
        let gdal_err: OxiGdalError = io_err.into();
        assert!(matches!(
            gdal_err,
            OxiGdalError::Io(IoError::NotFound { .. })
        ));
    }

    #[test]
    fn test_format_error_conversion() {
        let format_err = FormatError::InvalidMagic {
            expected: &[0x49, 0x49],
            actual: [0x00, 0x00, 0x00, 0x00],
        };
        let gdal_err: OxiGdalError = format_err.into();
        assert!(matches!(
            gdal_err,
            OxiGdalError::Format(FormatError::InvalidMagic { .. })
        ));
    }

    #[test]
    fn test_error_codes() {
        let err = OxiGdalError::InvalidParameter {
            parameter: "test",
            message: "test message".to_string(),
        };
        assert_eq!(err.code(), "E001");

        let err = OxiGdalError::NotSupported {
            operation: "test".to_string(),
        };
        assert_eq!(err.code(), "E002");

        let io_err = IoError::NotFound {
            path: "/test".to_string(),
        };
        assert_eq!(io_err.code(), "E100");
    }

    #[test]
    fn test_error_suggestions() {
        let err = OxiGdalError::InvalidParameter {
            parameter: "test",
            message: "test message".to_string(),
        };
        assert!(err.suggestion().is_some());
        assert!(err.suggestion().is_some_and(|s| s.contains("parameter")));

        let io_err = IoError::NotFound {
            path: "/test".to_string(),
        };
        assert!(io_err.suggestion().is_some());
        assert!(io_err.suggestion().is_some_and(|s| s.contains("file")));
    }

    #[test]
    fn test_error_context() {
        let err = OxiGdalError::InvalidParameter {
            parameter: "test_param",
            message: "test message".to_string(),
        };
        let ctx = err.context();
        assert_eq!(ctx.category, "parameter_validation");

        let io_err = IoError::NotFound {
            path: "/test/path".to_string(),
        };
        let ctx = io_err.context();
        assert_eq!(ctx.category, "file_not_found");
    }

    #[test]
    fn test_error_aggregator() {
        let mut agg = ErrorAggregator::new();
        assert!(!agg.has_errors());
        assert_eq!(agg.count(), 0);

        agg.add(OxiGdalError::InvalidParameter {
            parameter: "test1",
            message: "error 1".to_string(),
        });
        assert!(agg.has_errors());
        assert_eq!(agg.count(), 1);

        agg.add(OxiGdalError::InvalidParameter {
            parameter: "test2",
            message: "error 2".to_string(),
        });
        assert_eq!(agg.count(), 2);

        let result = agg.into_result();
        assert!(result.is_err());
    }

    #[test]
    fn test_error_aggregator_with_results() {
        let mut agg = ErrorAggregator::new();

        let ok_result: Result<i32> = Ok(42);
        let value = agg.add_result(ok_result);
        assert_eq!(value, Some(42));
        assert!(!agg.has_errors());

        let err_result: Result<i32> = Err(OxiGdalError::InvalidParameter {
            parameter: "test",
            message: "error".to_string(),
        });
        let value = agg.add_result(err_result);
        assert_eq!(value, None);
        assert!(agg.has_errors());
        assert_eq!(agg.count(), 1);
    }

    #[test]
    fn test_result_ext_context() {
        let result: Result<i32> = Err(OxiGdalError::InvalidParameter {
            parameter: "test",
            message: "original".to_string(),
        });

        let with_ctx = result.context("added context");
        assert!(with_ctx.is_err());
        if let Err(e) = with_ctx {
            assert!(matches!(e, OxiGdalError::Internal { .. }));
        }
    }

    #[test]
    fn test_result_ext_with_context() {
        let result: Result<i32> = Err(OxiGdalError::InvalidParameter {
            parameter: "test",
            message: "original".to_string(),
        });

        let with_ctx = result.with_context(|| "lazy context".to_string());
        assert!(with_ctx.is_err());
        if let Err(e) = with_ctx {
            assert!(matches!(e, OxiGdalError::Internal { .. }));
        }
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_from_path() {
        use std::path::Path;

        let path = Path::new("/test/file.tif");
        let err = OxiGdalError::from_path(path, std::io::ErrorKind::NotFound);
        assert!(matches!(err, OxiGdalError::Io(IoError::NotFound { .. })));

        let err = OxiGdalError::from_path(path, std::io::ErrorKind::PermissionDenied);
        assert!(matches!(
            err,
            OxiGdalError::Io(IoError::PermissionDenied { .. })
        ));
    }

    #[test]
    fn test_error_builder_basic() {
        let builder = OxiGdalError::io_error_builder("Test error");
        let err = builder.build();
        assert!(matches!(err, OxiGdalError::Io(IoError::Read { .. })));
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_error_builder_with_path() {
        use std::path::Path;

        let builder = OxiGdalError::io_error_builder("Cannot read file")
            .with_path(Path::new("/data/test.tif"));

        assert_eq!(builder.file_path(), Some(Path::new("/data/test.tif")));

        let err = builder.build();
        assert!(matches!(err, OxiGdalError::Io(IoError::Read { .. })));
    }

    #[test]
    fn test_error_builder_with_operation() {
        let builder = OxiGdalError::io_error_builder("Test error").with_operation("read_raster");

        assert_eq!(builder.operation_name(), Some("read_raster"));

        let err = builder.build();
        assert!(matches!(err, OxiGdalError::Io(IoError::Read { .. })));
    }

    #[test]
    fn test_error_builder_with_parameters() {
        let builder = OxiGdalError::invalid_parameter_builder("width", "must be positive")
            .with_parameter("value", "-10")
            .with_parameter("minimum", "1");

        let params = builder.parameters();
        assert_eq!(params.get("value"), Some(&"-10".to_string()));
        assert_eq!(params.get("minimum"), Some(&"1".to_string()));

        let err = builder.build();
        assert!(matches!(err, OxiGdalError::InvalidParameter { .. }));
    }

    #[test]
    fn test_error_builder_with_suggestion() {
        let builder = OxiGdalError::io_error_builder("Cannot read file")
            .with_suggestion("Check file permissions and ensure the file exists");

        let suggestion = builder.suggestion();
        assert_eq!(
            suggestion,
            Some("Check file permissions and ensure the file exists".to_string())
        );

        let err = builder.build();
        assert!(matches!(err, OxiGdalError::Io(IoError::Read { .. })));
    }

    #[test]
    fn test_error_builder_custom_suggestion_overrides_default() {
        let builder = OxiGdalError::invalid_parameter_builder("test", "invalid")
            .with_suggestion("Custom suggestion");

        let suggestion = builder.suggestion();
        assert_eq!(suggestion, Some("Custom suggestion".to_string()));
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_error_builder_fluent_api() {
        use std::path::Path;

        let builder = OxiGdalError::io_error_builder("Cannot read file")
            .with_path(Path::new("/data/test.tif"))
            .with_operation("read_raster")
            .with_parameter("band", "1")
            .with_parameter("window", "0,0,512,512")
            .with_suggestion("Verify file exists and is accessible");

        assert_eq!(builder.file_path(), Some(Path::new("/data/test.tif")));
        assert_eq!(builder.operation_name(), Some("read_raster"));
        assert_eq!(builder.parameters().get("band"), Some(&"1".to_string()));
        assert_eq!(
            builder.parameters().get("window"),
            Some(&"0,0,512,512".to_string())
        );
        assert!(builder.suggestion().is_some());

        let err = builder.build();
        assert!(matches!(err, OxiGdalError::Io(IoError::Read { .. })));
    }

    #[test]
    fn test_error_builder_context() {
        let builder = OxiGdalError::invalid_parameter_builder("width", "must be positive")
            .with_parameter("value", "-10")
            .with_operation("create_raster");

        let ctx = builder.build_context();
        assert_eq!(ctx.category, "parameter_validation");
        assert!(ctx.operation.is_some());
        assert_eq!(ctx.operation.as_deref(), Some("create_raster"));
        assert!(!ctx.parameters.is_empty());
    }

    #[test]
    fn test_error_builder_into_error() {
        let builder = OxiGdalError::io_error_builder("Test error");
        let err = builder.into_error();
        assert!(matches!(err, OxiGdalError::Io(IoError::Read { .. })));
    }

    #[test]
    fn test_error_builder_error_ref() {
        let builder = OxiGdalError::io_error_builder("Test error");
        let err_ref = builder.error();
        assert!(matches!(err_ref, OxiGdalError::Io(IoError::Read { .. })));
    }

    #[test]
    fn test_error_builder_with_multiple_parameters() {
        let mut builder = OxiGdalError::invalid_parameter_builder("size", "invalid");
        builder = builder.with_parameter("width", "1024");
        builder = builder.with_parameter("height", "768");
        builder = builder.with_parameter("bands", "3");

        let params = builder.parameters();
        assert_eq!(params.len(), 3);
        assert_eq!(params.get("width"), Some(&"1024".to_string()));
        assert_eq!(params.get("height"), Some(&"768".to_string()));
        assert_eq!(params.get("bands"), Some(&"3".to_string()));
    }

    #[test]
    fn test_error_builder_allocation_error() {
        let builder = OxiGdalError::allocation_error_builder("Failed to allocate buffer");
        let err = builder.build();
        assert!(matches!(err, OxiGdalError::Internal { .. }));
        assert!(err.to_string().contains("Allocation error"));
    }

    #[test]
    fn test_error_builder_invalid_state() {
        let builder = OxiGdalError::invalid_state_builder("Dataset already closed");
        let err = builder.build();
        assert!(matches!(err, OxiGdalError::Internal { .. }));
        assert!(err.to_string().contains("Invalid state"));
    }

    #[test]
    fn test_error_builder_not_supported() {
        let builder = OxiGdalError::not_supported_builder("write_compressed_tiff");
        let err = builder.build();
        assert!(matches!(err, OxiGdalError::NotSupported { .. }));
    }

    #[test]
    fn test_error_builder_edge_cases() {
        // Empty operation name
        let builder = OxiGdalError::io_error_builder("test").with_operation("");
        assert_eq!(builder.operation_name(), Some(""));

        // Empty parameter values
        let builder = OxiGdalError::io_error_builder("test").with_parameter("key", "");
        assert_eq!(builder.parameters().get("key"), Some(&"".to_string()));

        // Empty suggestion
        let builder = OxiGdalError::io_error_builder("test").with_suggestion("");
        assert_eq!(builder.suggestion(), Some("".to_string()));
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_error_context_with_builder_fields() {
        use std::path::Path;

        let builder = OxiGdalError::io_error_builder("Test")
            .with_path(Path::new("/test"))
            .with_operation("test_op")
            .with_parameter("key", "value")
            .with_suggestion("test suggestion");

        let ctx = builder.build_context();
        assert!(ctx.file_path.is_some());
        assert_eq!(ctx.file_path.as_deref(), Some(Path::new("/test")));
        assert_eq!(ctx.operation.as_deref(), Some("test_op"));
        assert_eq!(ctx.parameters.get("key"), Some(&"value".to_string()));
        assert_eq!(ctx.custom_suggestion.as_deref(), Some("test suggestion"));
    }

    // Integration tests for error code consistency
    #[test]
    fn test_error_code_consistency_io_errors() {
        // Verify all I/O error codes are unique and in expected range
        let errors = vec![
            IoError::NotFound {
                path: "test".to_string(),
            },
            IoError::PermissionDenied {
                path: "test".to_string(),
            },
            IoError::Network {
                message: "test".to_string(),
            },
            IoError::UnexpectedEof { offset: 0 },
            IoError::Read {
                message: "test".to_string(),
            },
            IoError::Write {
                message: "test".to_string(),
            },
            IoError::Seek { position: 0 },
            IoError::Http {
                status: 404,
                message: "test".to_string(),
            },
        ];

        let codes: Vec<&str> = errors.iter().map(|e| e.code()).collect();

        // All codes should start with E1 (I/O error range)
        for code in &codes {
            assert!(code.starts_with("E1"));
        }

        // All codes should be unique
        for (i, code1) in codes.iter().enumerate() {
            for (j, code2) in codes.iter().enumerate() {
                if i != j {
                    assert_ne!(code1, code2, "Duplicate error codes found");
                }
            }
        }
    }

    #[test]
    fn test_error_code_consistency_format_errors() {
        // Verify all format error codes are unique and in expected range
        let errors = vec![
            FormatError::InvalidMagic {
                expected: &[0x49],
                actual: [0, 0, 0, 0],
            },
            FormatError::InvalidHeader {
                message: "test".to_string(),
            },
            FormatError::UnsupportedVersion { version: 1 },
            FormatError::InvalidTag {
                tag: 256,
                message: "test".to_string(),
            },
            FormatError::MissingTag { tag: "test" },
            FormatError::InvalidDataType { type_id: 1 },
            FormatError::CorruptData {
                offset: 0,
                message: "test".to_string(),
            },
            FormatError::InvalidGeoKey {
                key_id: 1024,
                message: "test".to_string(),
            },
        ];

        let codes: Vec<&str> = errors.iter().map(|e| e.code()).collect();

        // All codes should start with E2 (format error range)
        for code in &codes {
            assert!(code.starts_with("E2"));
        }

        // All codes should be unique
        for (i, code1) in codes.iter().enumerate() {
            for (j, code2) in codes.iter().enumerate() {
                if i != j {
                    assert_ne!(code1, code2, "Duplicate error codes found");
                }
            }
        }
    }

    #[test]
    fn test_error_code_consistency_crs_errors() {
        let errors = vec![
            CrsError::UnknownCrs {
                identifier: "test".to_string(),
            },
            CrsError::InvalidWkt {
                message: "test".to_string(),
            },
            CrsError::InvalidEpsg { code: 0 },
            CrsError::TransformationError {
                source_crs: "EPSG:4326".to_string(),
                target_crs: "EPSG:3857".to_string(),
                message: "test".to_string(),
            },
            CrsError::DatumNotFound {
                datum: "WGS84".to_string(),
            },
        ];

        let codes: Vec<&str> = errors.iter().map(|e| e.code()).collect();

        // All codes should start with E3 (CRS error range)
        for code in &codes {
            assert!(code.starts_with("E3"));
        }

        // All codes should be unique
        for (i, code1) in codes.iter().enumerate() {
            for (j, code2) in codes.iter().enumerate() {
                if i != j {
                    assert_ne!(code1, code2, "Duplicate error codes found");
                }
            }
        }
    }

    #[test]
    fn test_error_code_consistency_compression_errors() {
        let errors = vec![
            CompressionError::UnknownMethod { method: 99 },
            CompressionError::DecompressionFailed {
                message: "test".to_string(),
            },
            CompressionError::CompressionFailed {
                message: "test".to_string(),
            },
            CompressionError::InvalidData {
                message: "test".to_string(),
            },
        ];

        let codes: Vec<&str> = errors.iter().map(|e| e.code()).collect();

        // All codes should start with E4 (compression error range)
        for code in &codes {
            assert!(code.starts_with("E4"));
        }

        // All codes should be unique
        for (i, code1) in codes.iter().enumerate() {
            for (j, code2) in codes.iter().enumerate() {
                if i != j {
                    assert_ne!(code1, code2, "Duplicate error codes found");
                }
            }
        }
    }

    #[test]
    fn test_error_code_consistency_top_level_errors() {
        let errors = vec![
            OxiGdalError::InvalidParameter {
                parameter: "test",
                message: "test".to_string(),
            },
            OxiGdalError::NotSupported {
                operation: "test".to_string(),
            },
            OxiGdalError::OutOfBounds {
                message: "test".to_string(),
            },
            OxiGdalError::Internal {
                message: "test".to_string(),
            },
        ];

        let codes: Vec<&str> = errors.iter().map(|e| e.code()).collect();

        // All codes should start with E0 (top-level error range)
        for code in &codes {
            assert!(code.starts_with("E0"));
        }

        // All codes should be unique
        for (i, code1) in codes.iter().enumerate() {
            for (j, code2) in codes.iter().enumerate() {
                if i != j {
                    assert_ne!(code1, code2, "Duplicate error codes found");
                }
            }
        }
    }

    // Integration tests for suggestion quality
    #[test]
    fn test_suggestion_quality_io_errors() {
        let test_cases = vec![
            (
                IoError::NotFound {
                    path: "/test".to_string(),
                },
                vec!["file", "path", "exist"],
            ),
            (
                IoError::PermissionDenied {
                    path: "/test".to_string(),
                },
                vec!["permission"],
            ),
            (
                IoError::Network {
                    message: "timeout".to_string(),
                },
                vec!["network", "connectivity"],
            ),
            (
                IoError::UnexpectedEof { offset: 100 },
                vec!["truncated", "corrupted"],
            ),
            (
                IoError::Http {
                    status: 404,
                    message: "Not Found".to_string(),
                },
                vec!["not found", "resource"],
            ),
            (
                IoError::Http {
                    status: 403,
                    message: "Forbidden".to_string(),
                },
                vec!["forbidden", "authentication", "credentials"],
            ),
            (
                IoError::Http {
                    status: 500,
                    message: "Server Error".to_string(),
                },
                vec!["server", "later"],
            ),
        ];

        for (error, keywords) in test_cases {
            let suggestion = error.suggestion();
            assert!(
                suggestion.is_some(),
                "Error should have a suggestion: {:?}",
                error
            );

            let suggestion_text = suggestion.expect("Expected suggestion").to_lowercase();
            let has_keyword = keywords.iter().any(|kw| suggestion_text.contains(kw));
            assert!(
                has_keyword,
                "Suggestion '{}' should contain at least one keyword from {:?}",
                suggestion_text, keywords
            );
        }
    }

    #[test]
    fn test_suggestion_quality_format_errors() {
        let test_cases = vec![
            (
                FormatError::InvalidMagic {
                    expected: &[0x49, 0x49],
                    actual: [0, 0, 0, 0],
                },
                vec!["format", "file type", "verify"],
            ),
            (
                FormatError::UnsupportedVersion { version: 999 },
                vec!["version", "supported", "converting"],
            ),
            (
                FormatError::MissingTag { tag: "ImageWidth" },
                vec!["required", "missing", "incomplete", "corrupted"],
            ),
            (
                FormatError::CorruptData {
                    offset: 1024,
                    message: "checksum mismatch".to_string(),
                },
                vec!["corruption", "backup", "recovering"],
            ),
        ];

        for (error, keywords) in test_cases {
            let suggestion = error.suggestion();
            assert!(
                suggestion.is_some(),
                "Error should have a suggestion: {:?}",
                error
            );

            let suggestion_text = suggestion.expect("Expected suggestion").to_lowercase();
            let has_keyword = keywords.iter().any(|kw| suggestion_text.contains(kw));
            assert!(
                has_keyword,
                "Suggestion '{}' should contain at least one keyword from {:?}",
                suggestion_text, keywords
            );
        }
    }

    #[test]
    fn test_suggestion_quality_crs_errors() {
        let test_cases = vec![
            (
                CrsError::UnknownCrs {
                    identifier: "CUSTOM:123".to_string(),
                },
                vec!["verify", "epsg", "identifier"],
            ),
            (
                CrsError::InvalidWkt {
                    message: "parse error".to_string(),
                },
                vec!["wkt", "syntax", "bracket"],
            ),
            (
                CrsError::InvalidEpsg { code: 999999 },
                vec!["valid", "epsg.io"],
            ),
            (
                CrsError::TransformationError {
                    source_crs: "EPSG:4326".to_string(),
                    target_crs: "CUSTOM:1".to_string(),
                    message: "no transformation path".to_string(),
                },
                vec!["compatible", "transformation", "parameters"],
            ),
        ];

        for (error, keywords) in test_cases {
            let suggestion = error.suggestion();
            assert!(
                suggestion.is_some(),
                "Error should have a suggestion: {:?}",
                error
            );

            let suggestion_text = suggestion.expect("Expected suggestion").to_lowercase();
            let has_keyword = keywords.iter().any(|kw| suggestion_text.contains(kw));
            assert!(
                has_keyword,
                "Suggestion '{}' should contain at least one keyword from {:?}",
                suggestion_text, keywords
            );
        }
    }

    #[test]
    fn test_suggestion_quality_top_level_errors() {
        let test_cases = vec![
            (
                OxiGdalError::InvalidParameter {
                    parameter: "width",
                    message: "must be positive".to_string(),
                },
                vec!["parameter", "documentation", "valid"],
            ),
            (
                OxiGdalError::NotSupported {
                    operation: "write_jp2".to_string(),
                },
                vec!["feature", "enabled", "alternative"],
            ),
            (
                OxiGdalError::OutOfBounds {
                    message: "index out of range".to_string(),
                },
                vec!["verify", "indices", "range", "valid"],
            ),
            (
                OxiGdalError::Internal {
                    message: "unexpected null pointer".to_string(),
                },
                vec!["bug", "report"],
            ),
        ];

        for (error, keywords) in test_cases {
            let suggestion = error.suggestion();
            assert!(
                suggestion.is_some(),
                "Error should have a suggestion: {:?}",
                error
            );

            let suggestion_text = suggestion.expect("Expected suggestion").to_lowercase();
            let has_keyword = keywords.iter().any(|kw| suggestion_text.contains(kw));
            assert!(
                has_keyword,
                "Suggestion '{}' should contain at least one keyword from {:?}",
                suggestion_text, keywords
            );
        }
    }

    // Integration tests for context propagation
    #[test]
    fn test_context_propagation_io_errors() {
        let error = IoError::NotFound {
            path: "/data/test.tif".to_string(),
        };
        let context = error.context();

        assert_eq!(context.category, "file_not_found");
        assert!(!context.details.is_empty());

        let path_detail = context.details.iter().find(|(k, _)| k == "path");
        assert!(path_detail.is_some());
        assert_eq!(
            path_detail.expect("Expected path detail").1,
            "/data/test.tif"
        );
    }

    #[test]
    fn test_context_propagation_format_errors() {
        let error = FormatError::InvalidTag {
            tag: 256,
            message: "unsupported tag type".to_string(),
        };
        let context = error.context();

        assert_eq!(context.category, "invalid_tag");
        assert!(!context.details.is_empty());

        let tag_detail = context.details.iter().find(|(k, _)| k == "tag");
        assert!(tag_detail.is_some());
        assert_eq!(tag_detail.expect("Expected tag detail").1, "256");

        let message_detail = context.details.iter().find(|(k, _)| k == "message");
        assert!(message_detail.is_some());
        assert_eq!(
            message_detail.expect("Expected message detail").1,
            "unsupported tag type"
        );
    }

    #[test]
    fn test_context_propagation_crs_errors() {
        let error = CrsError::TransformationError {
            source_crs: "EPSG:4326".to_string(),
            target_crs: "EPSG:3857".to_string(),
            message: "datum shift required".to_string(),
        };
        let context = error.context();

        assert_eq!(context.category, "transformation_error");
        assert!(!context.details.is_empty());

        let src_detail = context.details.iter().find(|(k, _)| k == "source_crs");
        assert!(src_detail.is_some());
        assert_eq!(
            src_detail.expect("Expected source_crs detail").1,
            "EPSG:4326"
        );

        let tgt_detail = context.details.iter().find(|(k, _)| k == "target_crs");
        assert!(tgt_detail.is_some());
        assert_eq!(
            tgt_detail.expect("Expected target_crs detail").1,
            "EPSG:3857"
        );
    }

    #[test]
    fn test_context_propagation_through_conversion() {
        let io_error = IoError::Network {
            message: "connection timeout".to_string(),
        };
        let gdal_error: OxiGdalError = io_error.into();

        let context = gdal_error.context();
        assert_eq!(context.category, "network_error");

        let msg_detail = context.details.iter().find(|(k, _)| k == "message");
        assert!(msg_detail.is_some());
        assert_eq!(
            msg_detail.expect("Expected message detail").1,
            "connection timeout"
        );
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_context_propagation_with_error_builder() {
        use std::path::Path;

        let builder = OxiGdalError::io_error_builder("Cannot read GeoTIFF")
            .with_path(Path::new("/data/terrain.tif"))
            .with_operation("read_geotiff")
            .with_parameter("band", "1")
            .with_parameter("window", "0,0,512,512");

        let context = builder.build_context();

        // Verify file path is propagated
        assert_eq!(
            context.file_path,
            Some(Path::new("/data/terrain.tif").to_path_buf())
        );

        // Verify operation is propagated
        assert_eq!(context.operation.as_deref(), Some("read_geotiff"));

        // Verify parameters are propagated
        assert_eq!(context.parameters.get("band"), Some(&"1".to_string()));
        assert_eq!(
            context.parameters.get("window"),
            Some(&"0,0,512,512".to_string())
        );
    }

    #[test]
    fn test_error_builder_context_with_custom_suggestion() {
        let builder = OxiGdalError::invalid_parameter_builder("buffer_size", "must be power of 2")
            .with_parameter("value", "1000")
            .with_suggestion("Use 512, 1024, 2048, or 4096");

        let context = builder.build_context();

        // Verify custom suggestion is in context
        assert_eq!(
            context.custom_suggestion.as_deref(),
            Some("Use 512, 1024, 2048, or 4096")
        );
    }

    #[test]
    fn test_error_context_detail_chain() {
        let mut context = ErrorContext::new("test_category");
        context = context
            .with_detail("key1", "value1")
            .with_detail("key2", "value2")
            .with_detail("key3", "value3");

        assert_eq!(context.category, "test_category");
        assert_eq!(context.details.len(), 3);

        // Verify order is preserved
        assert_eq!(
            context.details[0],
            ("key1".to_string(), "value1".to_string())
        );
        assert_eq!(
            context.details[1],
            ("key2".to_string(), "value2".to_string())
        );
        assert_eq!(
            context.details[2],
            ("key3".to_string(), "value3".to_string())
        );
    }

    #[test]
    fn test_error_builder_into_conversion() {
        let builder = OxiGdalError::io_error_builder("Test error");
        let error: OxiGdalError = builder.into();

        assert!(matches!(error, OxiGdalError::Io(IoError::Read { .. })));
    }

    #[test]
    fn test_comprehensive_error_workflow() {
        // This test simulates a complete error handling workflow

        // Step 1: Create an error with full context
        #[cfg(feature = "std")]
        let error = {
            use std::path::Path;
            OxiGdalError::io_error_builder("Cannot open GeoTIFF file")
                .with_path(Path::new("/data/terrain.tif"))
                .with_operation("open_geotiff")
                .with_parameter("mode", "read")
                .with_suggestion("Check if file exists and is readable")
                .build()
        };

        #[cfg(not(feature = "std"))]
        let error = OxiGdalError::io_error("Cannot open GeoTIFF file");

        // Step 2: Verify error code
        assert_eq!(error.code(), "E104");

        // Step 3: Verify suggestion is present
        let suggestion = error.suggestion();
        assert!(suggestion.is_some());

        // Step 4: Get context
        let context = error.context();
        assert!(!context.details.is_empty());

        // Step 5: Verify error can be displayed
        let error_string = error.to_string();
        assert!(!error_string.is_empty());
    }
}
