//! Native files use only the Quill identity.

use quill_core::text::{from_plain_text, to_plain_text};
use quill_formats::{Format, native, zip};
use std::path::Path;

#[test]
fn native_files_use_the_quill_identity() {
    let doc = from_plain_text("Document with accented text: café");
    let bytes = native::write(&doc, 0);
    let archive = zip::Archive::open(&bytes).unwrap();
    assert_eq!(
        archive.read("mimetype").unwrap().unwrap(),
        b"application/x-quillwp"
    );
    let bin = archive.read("document.bin").unwrap().unwrap();
    assert_eq!(&bin[..8], b"QUILLDOC");
    let reopened = native::read(&bytes).unwrap();
    assert_eq!(to_plain_text(&reopened, "\n"), to_plain_text(&doc, "\n"));
    assert_eq!(Format::from_path(Path::new("new.QWP")), Some(Format::Quill));
    assert_eq!(Format::Quill.extensions(), &["qwp"]);
}
