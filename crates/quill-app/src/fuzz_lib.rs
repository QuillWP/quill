//! Developer-only access to parsers that normally live behind Quill's binary.
//! The release application does not link this library.

#![cfg(feature = "fuzzing")]

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
