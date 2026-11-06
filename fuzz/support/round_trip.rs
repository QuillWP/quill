use quill_core::Document;

#[path = "../../crates/quill-formats/tests/common/mod.rs"]
mod common;

/// Arbitrary imported documents may contain intentionally unsupported exchange
/// features. Every export must reopen; native saves must preserve the full model.
#[allow(dead_code)]
pub fn check_imported(doc: &Document) {
    let native = quill_formats::native::read(&quill_formats::native::write(doc, 0))
        .expect("Quill must reopen its own native save");
    assert_same_named(doc, &native, "native");
    quill_formats::docx::read(&quill_formats::docx::write(doc, 0))
        .expect("Quill must reopen its own DOCX export");
    quill_formats::rtf::read(&quill_formats::rtf::write(doc))
        .expect("Quill must reopen its own RTF export");
    quill_formats::html::read_file(&quill_formats::html::write(doc))
        .expect("Quill must reopen its own HTML save");
    quill_formats::odt_read::read(&quill_formats::odt::write(doc, 0))
        .expect("Quill must reopen its own ODT save");
}

#[allow(dead_code)]
pub fn assert_same(doc: &Document, back: &Document) {
    assert_same_named(doc, back, "semantic comparison");
}

fn assert_same_named(doc: &Document, back: &Document, format: &str) {
    let differences = common::differences(doc, back);
    assert!(
        differences.is_empty(),
        "{format}: {}",
        differences.join("\n")
    );
}

/// Documents generated from supported editing operations must also retain their
/// semantics in exchange formats. Revision ids are interned in the same process.
#[allow(dead_code)]
pub fn check_supported(doc: &Document) {
    check_imported(doc);
    // Quill exports each comment as a continuous first-to-last range. Edits
    // can leave gaps in its internal per-character tags; that representation
    // is lossless in native saves but not an exchange-format feature.
    let mut expected = doc.clone();
    let edges = comment_spans(doc);
    for (id, start, end) in edges {
        expected.set_comment_range(start, end, id);
    }
    let docx = quill_formats::docx::read(&quill_formats::docx::write(doc, 0)).unwrap();
    assert_same_named(&expected, &docx, "DOCX");
    let rtf = quill_formats::rtf::read(&quill_formats::rtf::write(doc)).unwrap();
    assert_same_named(&expected, &rtf, "RTF");
}

#[allow(dead_code)]
pub fn snapshot(doc: &Document) -> std::collections::BTreeMap<String, String> {
    common::describe(doc)
}

fn comment_spans(doc: &Document) -> Vec<(u32, quill_core::Position, quill_core::Position)> {
    let mut spans = std::collections::BTreeMap::new();
    let live = quill_core::notes::annotation_ids(&doc.main.paras);
    for (i, para) in doc.main.paras.iter().enumerate() {
        for (a, b, c) in para.run_bounds() {
            if c.comment != 0 && live.contains(&c.comment) {
                spans
                    .entry(c.comment)
                    .or_insert((
                        quill_core::Position::new(i, a),
                        quill_core::Position::new(i, b),
                    ))
                    .1 = quill_core::Position::new(i, b);
            }
        }
    }
    spans.into_iter().map(|(id, (a, b))| (id, a, b)).collect()
}
