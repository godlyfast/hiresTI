// rust_tidal_core: native TIDAL client for hiresTI
//
// FFI shape: Python passes UTF-8 JSON in, gets a heap-allocated UTF-8 JSON
// pointer back. Caller must release the buffer via `rtc_free_string`.
// Errors come back as `{"error": {...}}` JSON, not as integer status codes —
// that lets us thread structured TIDAL/HTTP errors through one channel.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;

use serde::Serialize;

const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize)]
struct VersionInfo {
    crate_name: &'static str,
    version: &'static str,
}

fn json_to_cstring<T: Serialize>(value: &T) -> *mut c_char {
    match serde_json::to_string(value) {
        Ok(s) => match CString::new(s) {
            Ok(cs) => cs.into_raw(),
            Err(_) => ptr::null_mut(),
        },
        Err(_) => ptr::null_mut(),
    }
}

/// Returns a JSON document describing the loaded crate. Useful for the Python
/// loader to verify the .so is the version it expects before doing anything
/// else. Caller frees with `rtc_free_string`.
#[no_mangle]
pub extern "C" fn rtc_version() -> *mut c_char {
    json_to_cstring(&VersionInfo {
        crate_name: "rust_tidal_core",
        version: CRATE_VERSION,
    })
}

/// Echoes the input JSON back. Phase 0 smoke-test hook — confirms the FFI
/// roundtrip (alloc/free, UTF-8 in/out) works end-to-end before we layer real
/// endpoints on top. Removed once Phase 1 lands real entry points.
#[no_mangle]
pub extern "C" fn rtc_echo_json(input: *const c_char) -> *mut c_char {
    if input.is_null() {
        return ptr::null_mut();
    }
    let bytes = unsafe { CStr::from_ptr(input) }.to_bytes();
    match CString::new(bytes) {
        Ok(cs) => cs.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

/// Frees a JSON string previously returned by any `rtc_*` function.
///
/// # Safety
/// `ptr` must have come from this crate (via `CString::into_raw`). Passing a
/// foreign pointer is undefined behavior. Null is permitted and ignored.
#[no_mangle]
pub unsafe extern "C" fn rtc_free_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    let _ = CString::from_raw(ptr);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_returns_valid_json() {
        let raw = rtc_version();
        assert!(!raw.is_null());
        let s = unsafe { CStr::from_ptr(raw) }.to_str().unwrap().to_owned();
        unsafe { rtc_free_string(raw) };
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["crate_name"], "rust_tidal_core");
        assert!(v["version"].is_string());
    }

    #[test]
    fn echo_roundtrips_utf8() {
        let input = CString::new(r#"{"hello":"世界"}"#).unwrap();
        let raw = rtc_echo_json(input.as_ptr());
        assert!(!raw.is_null());
        let s = unsafe { CStr::from_ptr(raw) }.to_str().unwrap().to_owned();
        unsafe { rtc_free_string(raw) };
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["hello"], "世界");
    }
}
