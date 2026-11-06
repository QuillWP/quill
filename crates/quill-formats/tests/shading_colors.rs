//! Exact solid fills, including those inside nested table cells.
use quill_core::{CharProps, Document, Position, Rgb, StoryId};
use quill_formats::{docx, html, native, odt, odt_read, rtf};

fn fixture() -> Document {
    let mut doc = quill_core::text::from_plain_text("Colored paragraph\nTables follow");
    doc.main.paras[0].pap.shading_color = Some(Rgb(0xEBF0FA));
    doc.insert_table(StoryId::Main, Position::new(1, 0), 1, 2, 3000);
    doc.insert_text(
        StoryId::Main,
        Position::new(1, 0),
        "Outer",
        CharProps::default(),
    );
    doc.split_paragraph(StoryId::Main, Position::new(1, 5));
    doc.insert_table(StoryId::Main, Position::new(2, 0), 1, 2, 1000);
    for p in &mut doc.main.paras {
        let level = p.pap.level();
        if let Some(row) = &mut p.row {
            row.cells[0].shading_color = Some(Rgb(if level == 1 { 0x123456 } else { 0xFEDCBA }));
            row.cells[1].shading_color = Some(Rgb(0x000000));
        }
    }
    doc
}

fn fills(doc: &Document) -> Vec<(u8, Vec<Option<Rgb>>)> {
    doc.main
        .paras
        .iter()
        .filter_map(|p| {
            Some((
                p.pap.level(),
                p.row
                    .as_ref()?
                    .cells
                    .iter()
                    .map(|c| c.shading_color)
                    .collect(),
            ))
        })
        .collect()
}

#[test]
fn solid_colors_survive_native_docx_rtf_odt_and_html_cycles() {
    let doc = fixture();
    let expected = fills(&doc);
    assert_eq!(expected.len(), 2);
    let path = std::env::temp_dir().join(format!("quill-shading-{}.html", std::process::id()));
    std::fs::write(&path, html::write(&doc)).unwrap();
    let html_back = quill_formats::load(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let versions = [
        native::read(&native::write(&doc, 0)).unwrap(),
        docx::read(&docx::write(&doc, 0)).unwrap(),
        rtf::read(&rtf::write(&doc)).unwrap(),
        odt_read::read(&odt::write(&doc, 0)).unwrap(),
        html_back,
    ];
    for (format, back) in ["native", "docx", "rtf", "odt", "html"]
        .into_iter()
        .zip(versions)
    {
        assert_eq!(
            back.main.paras[0].pap.shading_color,
            Some(Rgb(0xEBF0FA)),
            "{format}"
        );
        assert_eq!(fills(&back), expected, "{format}");
    }
}

#[test]
fn standard_rtf_cell_backgrounds_do_not_depend_on_private_metadata() {
    let bytes = br"{\rtf1{\colortbl;\red18\green52\blue86;}\trowd\clcbpat1\cellx3000\pard\intbl Filled\cell\row}";
    let doc = rtf::read(bytes).unwrap();
    assert_eq!(fills(&doc), [(1, vec![Some(Rgb(0x123456))])]);
}

#[test]
fn docx_explicit_white_overrides_a_colored_paragraph_style() {
    let mut doc = Document::new();
    let mut pap = doc.styles.get(0).pap.clone();
    pap.shading_color = Some(Rgb(0xCC2200));
    let id = doc.add_style("Colored", 0, None, pap, CharProps::default());
    doc.apply_style(StoryId::Main, 0, 0, id);
    doc.main.paras[0].pap.shading_color = Some(Rgb(0xFFFFFF));
    let back = docx::read(&docx::write(&doc, 0)).unwrap();
    assert_eq!(back.main.paras[0].pap.shading_color, Some(Rgb(0xFFFFFF)));
    doc.main.paras[0].pap.shading_color = None;
    let back = docx::read(&docx::write(&doc, 0)).unwrap();
    assert_eq!(back.main.paras[0].pap.shading_color, None);
}
