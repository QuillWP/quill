#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../support/round_trip.rs"]
mod round_trip;

fuzz_target!(|data: &[u8]| {
    for doc in [
        quill_formats::odt_read::read(data),
        quill_formats::odt_read::read_flat(data),
    ] {
        if let Ok(mut doc) = doc {
            doc.keep_in_bounds();
            round_trip::check_imported(&doc);
        }
    }
});
