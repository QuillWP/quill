//! Repeated style import/export, including optional files made by real Word.

use quill_core::Document;
use quill_formats::{docx, zip};

fn word_style_fixture() -> Vec<u8> {
    let base = docx::write(&Document::new(), 1_760_000_000);
    let archive = zip::Archive::open(&base).unwrap();
    let styles = br#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
      <w:style w:type="paragraph" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
      <w:style w:type="character" w:styleId="FootnoteReference"><w:name w:val="Footnote Reference"/><w:rPr><w:vertAlign w:val="superscript"/></w:rPr></w:style>
      <w:style w:type="paragraph" w:styleId="FootnoteReference1"><w:name w:val="Footnote Reference1"/></w:style>
      <w:style w:type="paragraph" w:styleId="PageNumber1"><w:name w:val="Page Number1"/></w:style>
      <w:style w:type="paragraph" w:styleId="LineNumber1"><w:name w:val="Line Number1"/></w:style>
      <w:style w:type="paragraph" w:styleId="UserStyle1"><w:name w:val="User Style1"/><w:rPr><w:b/></w:rPr></w:style>
    </w:styles>"#;
    let body = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
      <w:p><w:pPr><w:pStyle w:val="FootnoteReference1"/></w:pPr><w:r><w:t>Reference</w:t></w:r></w:p>
      <w:p><w:pPr><w:pStyle w:val="UserStyle1"/></w:pPr><w:r><w:t>Custom style</w:t></w:r></w:p>
      <w:sectPr/>
    </w:body></w:document>"#;
    let mut writer = zip::Writer::new(1_760_000_000);
    for name in archive.names() {
        let bytes = archive.read(name).unwrap().unwrap();
        writer.add(
            name,
            match name {
                "word/styles.xml" => styles,
                "word/document.xml" => body,
                _ => &bytes,
            },
        );
    }
    writer.finish()
}

fn inventory(doc: &Document) -> Vec<String> {
    let mut names: Vec<_> = doc
        .styles
        .styles
        .iter()
        .map(|style| style.name.clone())
        .collect();
    names.sort();
    names
}

#[test]
fn repeated_docx_cycles_do_not_accumulate_styles() {
    let input = std::env::var_os("QUILL_STYLE_INPUT");
    let bytes = input
        .as_ref()
        .map_or_else(word_style_fixture, |path| std::fs::read(path).unwrap());
    let mut doc = docx::read(&bytes).unwrap();
    let before = inventory(&doc);
    let mut unique: Vec<_> = before.iter().map(|name| name.to_lowercase()).collect();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), before.len(), "duplicate imported style names");
    if input.is_none() {
        assert!(
            before.contains(&"User Style1".into()),
            "user suffixes must survive"
        );
        for name in ["Footnote Reference", "Page Number", "Line Number"] {
            assert!(
                before.iter().any(|value| value.eq_ignore_ascii_case(name)),
                "missing {name}: {before:?}"
            );
            assert!(
                !before
                    .iter()
                    .any(|value| value.eq_ignore_ascii_case(&format!("{name}1")))
            );
        }
    }
    let text = doc
        .main
        .paras
        .iter()
        .map(|p| p.text.clone())
        .collect::<Vec<_>>();
    for cycle in 1..=5 {
        doc = docx::read(&docx::write(&doc, 1_760_000_000)).unwrap();
        assert_eq!(
            inventory(&doc),
            before,
            "style inventory changed in cycle {cycle}"
        );
        assert_eq!(
            doc.main
                .paras
                .iter()
                .map(|p| p.text.clone())
                .collect::<Vec<_>>(),
            text
        );
    }
    if let Some(path) = std::env::var_os("QUILL_STYLE_OUTPUT") {
        std::fs::write(path, docx::write(&doc, 1_760_000_000)).unwrap();
    }
    println!(
        "{} unique styles stable across five DOCX cycles",
        before.len()
    );
}
