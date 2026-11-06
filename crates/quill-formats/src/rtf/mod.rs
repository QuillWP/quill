//! Rich Text Format, the original's interchange format.

mod codepages;
pub mod read;
pub mod write;

pub use read::read;
pub use write::write;

/// An inline selection has no trailing paragraph mark.
pub fn write_clipboard(doc: &quill_core::Document, ends_with_mark: bool) -> Vec<u8> {
    let mut bytes = write(doc);
    if !ends_with_mark && bytes.ends_with(b"\\par\n}") {
        bytes.truncate(bytes.len() - 6);
        bytes.push(b'}');
    }
    bytes
}
