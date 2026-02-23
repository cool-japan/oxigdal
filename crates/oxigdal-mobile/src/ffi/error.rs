//! Error handling for FFI operations.
//!
//! Provides safe conversion between Rust errors and C-compatible error codes.

use super::types::OxiGdalErrorCode;
use std::ffi::CString;
use std::os::raw::c_char;
use std::ptr;
use std::sync::Mutex;

/// Thread-local storage for the last error message.
///
/// This allows FFI callers to retrieve detailed error messages after
/// a function returns an error code.
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

/// Sets the last error message for the current thread.
///
/// This is called internally when an error occurs in FFI functions.
pub fn set_last_error(message: String) {
    if let Ok(mut guard) = LAST_ERROR.lock() {
        *guard = Some(message);
    }
}

/// Clears the last error message.
pub fn clear_last_error() {
    if let Ok(mut guard) = LAST_ERROR.lock() {
        *guard = None;
    }
}

/// Gets the last error message and returns it as a C string.
///
/// The caller must free the returned string using `oxigdal_string_free`.
///
/// # Safety
/// This function is safe to call from FFI, but the returned pointer must
/// be properly freed.
#[unsafe(no_mangle)]
pub extern "C" fn oxigdal_get_last_error() -> *mut c_char {
    let error_msg = LAST_ERROR
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().cloned())
        .unwrap_or_else(|| "Unknown error".to_string());

    match CString::new(error_msg) {
        Ok(c_str) => c_str.into_raw(),
        Err(_) => {
            // Failed to create CString, return static error message
            match CString::new("Error creating error message") {
                Ok(c_str) => c_str.into_raw(),
                Err(_) => ptr::null_mut(),
            }
        }
    }
}

/// Frees a string returned by OxiGDAL functions.
///
/// # Safety
/// This function must only be called with strings returned by OxiGDAL functions.
/// The pointer must not be used after calling this function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_string_free(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: We know this was created by CString::into_raw()
        unsafe {
            drop(CString::from_raw(s));
        }
    }
}

/// Converts a Rust error into an FFI error code.
///
/// This function sets the last error message and returns the appropriate
/// error code.
pub fn handle_error<E: std::fmt::Display>(error: E) -> OxiGdalErrorCode {
    let message = error.to_string();

    // Determine error code based on message content
    let code = if message.contains("null pointer") || message.contains("NULL") {
        OxiGdalErrorCode::NullPointer
    } else if message.contains("invalid argument") || message.contains("Invalid") {
        OxiGdalErrorCode::InvalidArgument
    } else if message.contains("not found") || message.contains("No such file") {
        OxiGdalErrorCode::FileNotFound
    } else if message.contains("I/O") || message.contains("IO") {
        OxiGdalErrorCode::IoError
    } else if message.contains("unsupported") || message.contains("Unsupported") {
        OxiGdalErrorCode::UnsupportedFormat
    } else if message.contains("out of bounds") || message.contains("index") {
        OxiGdalErrorCode::OutOfBounds
    } else if message.contains("allocation") || message.contains("memory") {
        OxiGdalErrorCode::AllocationFailed
    } else if message.contains("UTF-8") || message.contains("encoding") {
        OxiGdalErrorCode::InvalidUtf8
    } else if message.contains("driver") || message.contains("Driver") {
        OxiGdalErrorCode::DriverError
    } else if message.contains("projection") || message.contains("CRS") {
        OxiGdalErrorCode::ProjectionError
    } else {
        OxiGdalErrorCode::Unknown
    };

    set_last_error(message);
    code
}

/// Macro to safely handle Result types in FFI functions.
///
/// Returns Success on Ok, or appropriate error code on Err.
#[macro_export]
macro_rules! ffi_result {
    ($expr:expr) => {
        match $expr {
            Ok(val) => val,
            Err(e) => {
                return $crate::ffi::error::handle_error(e);
            }
        }
    };
}

/// Macro to check for null pointers.
#[macro_export]
macro_rules! check_null {
    ($ptr:expr, $name:expr) => {
        if $ptr.is_null() {
            $crate::ffi::error::set_last_error(format!("Null pointer provided for {}", $name));
            return $crate::ffi::types::OxiGdalErrorCode::NullPointer;
        }
    };
}

/// Macro to wrap pointer dereferencing safely.
#[macro_export]
macro_rules! deref_ptr {
    ($ptr:expr, $type:ty, $name:expr) => {{
        $crate::check_null!($ptr, $name);
        // SAFETY: We just checked for null
        unsafe { &*($ptr as *const $type) }
    }};
}

/// Macro to wrap mutable pointer dereferencing safely.
#[macro_export]
macro_rules! deref_ptr_mut {
    ($ptr:expr, $type:ty, $name:expr) => {{
        $crate::check_null!($ptr, $name);
        // SAFETY: We just checked for null
        unsafe { &mut *($ptr as *mut $type) }
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_and_get_last_error() {
        clear_last_error();
        set_last_error("Test error message".to_string());

        let error_ptr = oxigdal_get_last_error();
        assert!(!error_ptr.is_null());

        unsafe {
            let error_cstr = std::ffi::CStr::from_ptr(error_ptr);
            let error_str = error_cstr.to_str().expect("valid UTF-8");
            assert_eq!(error_str, "Test error message");
            oxigdal_string_free(error_ptr);
        }
    }

    #[test]
    fn test_clear_last_error() {
        set_last_error("Error".to_string());
        clear_last_error();

        let error_ptr = oxigdal_get_last_error();
        assert!(!error_ptr.is_null());

        unsafe {
            let error_cstr = std::ffi::CStr::from_ptr(error_ptr);
            let error_str = error_cstr.to_str().expect("valid UTF-8");
            assert_eq!(error_str, "Unknown error");
            oxigdal_string_free(error_ptr);
        }
    }

    #[test]
    fn test_handle_error_classification() {
        clear_last_error();

        let code = handle_error("null pointer error");
        assert_eq!(code, OxiGdalErrorCode::NullPointer);

        let code = handle_error("file not found");
        assert_eq!(code, OxiGdalErrorCode::FileNotFound);

        let code = handle_error("I/O error occurred");
        assert_eq!(code, OxiGdalErrorCode::IoError);
    }

    #[test]
    fn test_string_free_null_safety() {
        // Should not panic on null pointer
        unsafe {
            oxigdal_string_free(std::ptr::null_mut());
        }
    }
}
