#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let document = quill_core::text::from_plain_text(&text);
    let _ = quill_core::merge::Data::from_document(&document);
});
