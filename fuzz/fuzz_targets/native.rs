#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../support/editing.rs"]
mod editing;

// Read a .qwp file, keep it within range as opening does, then save it in
// every format and read that back.
fuzz_target!(|data: &[u8]| {
    let Ok(mut doc) = quill_formats::native::read(data) else {
        return;
    };
    doc.keep_in_bounds();
    editing::valid(&doc);
    editing::round_trip::check_imported(&doc);
});
