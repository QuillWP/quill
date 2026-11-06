//! Convert between formats: `cargo run -p quill-formats --example convert
//! in.docx out.rtf`. The output format follows the extension.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (input, output) = (
        std::path::Path::new(&args[1]),
        std::path::Path::new(&args[2]),
    );
    let doc = quill_formats::load(input).unwrap_or_else(|e| panic!("{e}"));
    let format = quill_formats::Format::from_path(output).expect("unknown output extension");
    quill_formats::save(&doc, output, format).unwrap_or_else(|e| panic!("{e}"));
}
