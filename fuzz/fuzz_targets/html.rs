#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = quill_formats::html::read_file(data);
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = quill_formats::html::read_clipboard(text);
    }
});
