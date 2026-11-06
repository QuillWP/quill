#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(archive) = quill_formats::zip::Archive::open(data) {
        let names: Vec<String> = archive.names().map(str::to_owned).collect();
        for name in names {
            let _ = archive.read(&name);
        }
    }
});
