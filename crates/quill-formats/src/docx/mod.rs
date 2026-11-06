//! Office Open XML word processing documents (.docx).

pub mod names;
pub mod read;
pub mod write;

pub use read::read;
pub use write::write;

/// Read a document whose body is `body_xml`, for tests.
#[cfg(test)]
pub fn tests_read_body(body_xml: &str) -> quill_core::Document {
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body_xml}</w:body></w:document>"#
    );
    let mut z = crate::zip::Writer::new(0);
    z.add("word/document.xml", doc.as_bytes());
    read(&z.finish()).expect("test document reads")
}
