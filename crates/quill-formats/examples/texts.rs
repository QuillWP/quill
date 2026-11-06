//! Print each story's paragraphs, escaped, for checking what a reader
//! kept: `cargo run -p quill-formats --example texts FILE`.

use quill_core::StoryId;

fn main() {
    let path = std::env::args().nth(1).expect("usage: texts FILE");
    let doc = quill_formats::load(std::path::Path::new(&path)).unwrap_or_else(|e| panic!("{e}"));
    let p = &doc.props;
    println!(
        "page {}x{} margins t{} b{} l{} r{}, columns {}",
        p.page_width,
        p.page_height,
        p.margin_top,
        p.margin_bottom,
        p.margin_left,
        p.margin_right,
        doc.section.columns
    );
    for (i, para) in doc.main.paras.iter().enumerate() {
        println!("{i}: {:?}", para.text);
    }
    for id in StoryId::HEADERS {
        let story = doc.story(id);
        if story.paras.iter().any(|p| !p.text.is_empty()) {
            for para in &story.paras {
                println!("{id:?}: {:?}", para.text);
            }
        }
    }
    for para in &doc.notes.paras {
        println!("note: {:?}", para.text);
    }
    for para in &doc.annotations.paras {
        println!("annotation: {:?}", para.text);
    }
}
