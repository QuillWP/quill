//! Developer-only access to parsers that normally live behind Quill's binary.
//! The release application does not link this library.

#![cfg(feature = "fuzzing")]

// The application binary supplies these logging macros through events.rs.
// Fuzz targets link only this small library and deliberately keep no log.
#[macro_export]
macro_rules! ev {
    ($($arg:tt)*) => {};
}

#[macro_export]
macro_rules! ev_all {
    ($($arg:tt)*) => {};
}

/// Settings reports write failures through the application in production.
/// Fuzzing never enables persistence, so this is only a link-time stub.
pub mod app {
    pub fn alert(_message: &str, _detail: &str) {}
}

#[path = "docview/clipboard_rich.rs"]
pub mod clipboard_rich;
pub mod glossary_file;
pub mod image;
pub use quill_formats::png;
pub mod regional;
pub mod settings;
