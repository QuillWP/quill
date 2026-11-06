#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let settings = quill_app_fuzz::settings::Settings::from_text(&text);
    let _ = settings.to_text();
});
