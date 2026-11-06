#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let entries = quill_app_fuzz::glossary_file::parse(data);
    let _ = quill_app_fuzz::glossary_file::serialize(&entries);
});
