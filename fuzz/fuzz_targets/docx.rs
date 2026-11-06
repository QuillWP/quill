#![no_main]

use libfuzzer_sys::fuzz_target;

// Read the file, keep it within range as opening does, then save it in
// each format and read that back, as a round trip through Quill would.
fuzz_target!(|data: &[u8]| {
    let Ok(mut doc) = quill_formats::docx::read(data) else {
        return;
    };
    doc.keep_in_bounds();
    let _ = quill_formats::docx::read(&quill_formats::docx::write(&doc, 0));
    let _ = quill_formats::rtf::read(&quill_formats::rtf::write(&doc));
    let _ = quill_formats::native::read(&quill_formats::native::write(&doc, 0));
});
