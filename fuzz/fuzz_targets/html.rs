#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../support/round_trip.rs"]
mod round_trip;

fuzz_target!(|data: &[u8]| {
    if let Ok((mut doc, _)) = quill_formats::html::read_file(data) {
        doc.keep_in_bounds();
        round_trip::check_imported(&doc);
    }
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = quill_formats::html::read_clipboard(text);
    }
});
