#![no_main]
use libfuzzer_sys::fuzz_target;
#[path = "../support/editing.rs"]
mod editing;
fuzz_target!(|data: &[u8]| editing::character_styles::exercise(data));
