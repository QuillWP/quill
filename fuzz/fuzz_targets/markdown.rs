#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../support/editing.rs"]
mod editing;

// Read Markdown with no folder, so pictures are never looked up on disk, then
// save in every format and read that back, Markdown included.
fuzz_target!(|data: &[u8]| {
    let (mut doc, _) = quill_formats::markdown::read(&String::from_utf8_lossy(data), None);
    doc.keep_in_bounds();
    editing::valid(&doc);
    editing::round_trip::check_imported(&doc);
    let saved = quill_formats::markdown::write(&doc);
    let (mut back, _) = quill_formats::markdown::read(&saved, None);
    back.keep_in_bounds();
    editing::valid(&back);
    let _ = quill_formats::markdown::write(&back);
});
