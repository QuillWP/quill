#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = quill_formats::odt_read::read(data);
    let _ = quill_formats::odt_read::read_flat(data);
});
