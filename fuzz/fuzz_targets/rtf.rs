#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../support/round_trip.rs"]
mod round_trip;

// Read the file, keep it within range as opening does, then save it in
// each format and read that back, as a round trip through Quill would.
fuzz_target!(|data: &[u8]| {
    let Ok(mut doc) = quill_formats::rtf::read(data) else {
        return;
    };
    doc.keep_in_bounds();
    round_trip::check_imported(&doc);
});
