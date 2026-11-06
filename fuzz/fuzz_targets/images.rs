#![no_main]

use libfuzzer_sys::fuzz_target;
use quill_core::ImageFormat;

fuzz_target!(|data: &[u8]| {
    let _ = quill_core::Picture::from_bytes(data.to_vec());
    let _ = quill_formats::images::picture(data.to_vec());
    let _ = quill_formats::images::from_dib(data);
    let _ = quill_app_fuzz::image::decode_rgba(data, ImageFormat::Png);
    let _ = quill_app_fuzz::image::decode_rgba(data, ImageFormat::Jpeg);
});
