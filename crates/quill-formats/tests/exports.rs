use quill_core::para::{Paragraph, chars};
use quill_core::props::{Align, CharProps, ParaProps, Rgb, Underline};
use quill_core::{Document, Position, StoryId};
use quill_formats::{Format, html, odt, xml, zip};

fn fixture() -> Document {
    let mut doc = Document::new();
    doc.summary.title = "Export <review> & café".into();
    let font = doc.font_id("Georgia");
    let chp = CharProps {
        font,
        size: 31,
        bold: true,
        color: Rgb(0x123456),
        ..Default::default()
    };
    let pap = ParaProps {
        align: Align::Center,
        before: 240,
        after: 120,
        ..Default::default()
    };
    let id = doc.add_style("Review & Notes", 0, None, pap, chp);
    let style = doc.styles.get(id).clone();
    let mut p = Paragraph::with_text("Styled café & <text> ", chp, style.pap);
    p.insert(
        p.len(),
        "red italic",
        CharProps {
            bold: false,
            italic: true,
            underline: Underline::Double,
            color: Rgb(0xff0000),
            size: 27,
            ..chp
        },
    );
    p.insert(
        p.len(),
        "\t two  spaces\u{000b}new line\u{000c}new page",
        chp,
    );
    doc.main.paras = vec![p, Paragraph::default()];
    doc.insert_table(StoryId::Main, Position::new(1, 0), 2, 2, 6000);
    let row = quill_core::table::row_at(&doc.main.paras, 1).unwrap();
    for (i, (a, _)) in row.cells.iter().enumerate() {
        doc.main.paras[*a].insert(0, &format!("Cell {}", i + 1), chp);
    }
    let picture =
        quill_core::Picture::from_bytes(include_bytes!("../../../assets/quill.png").to_vec())
            .unwrap()
            .scaled_to(720, 720);
    doc.insert_picture(StoryId::Main, Position::new(0, 0), picture, chp);
    let id = doc.insert_note(Position::new(0, 3), chp, None);
    let at = doc.note_text_start(id).unwrap();
    doc.insert_text(StoryId::Notes, at, "Footnote with café", chp);
    doc
}

#[test]
fn html_exports_styles_direct_formatting_and_safe_self_contained_content() {
    let doc = fixture();
    let html = String::from_utf8(html::write(&doc)).unwrap();
    let style = doc.styles.find("Review & Notes").unwrap();
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains(&format!(
        ".s{style} .run{{font-family:'Georgia';font-size:15.5pt;color:#123456;"
    )));
    assert!(html.contains("data-quill-style=\"Review &amp; Notes\""));
    assert!(html.contains("font-size:13.5pt;color:#ff0000;font-weight:normal;font-style:italic;"));
    assert!(html.contains("text-decoration-style:double"));
    assert!(html.contains("café &amp; &lt;text&gt;"));
    assert!(html.contains("<table><tbody>"));
    assert!(html.contains("Cell 1"));
    assert!(html.contains("src=\"data:image/png;base64,iVBOR"));
    assert!(html.contains("Footnote with café"));
    assert!(html.contains("<br class=\"page-break\">"));
    assert!(!html.contains(chars::FOOTNOTE));
    assert!(!html.contains("<script"));
}

#[test]
fn html_does_not_turn_font_names_or_document_text_into_markup() {
    let mut doc = Document::new();
    let font = doc.font_id("evil'</style><script>alert(1)</script>\\\"");
    doc.styles.styles[0].chp.font = font;
    doc.main.paras[0].insert(0, "<script>alert('x')</script>", doc.styles.styles[0].chp);
    let html = String::from_utf8(html::write(&doc)).unwrap();
    assert!(!html.contains("<script>"));
    assert_eq!(html.matches("</style>").count(), 1);
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("\\3c "));
}

#[test]
fn odt_is_a_real_package_with_styles_tables_images_notes_and_unicode() {
    let doc = fixture();
    let bytes = odt::write(&doc, 0);
    assert_eq!(&bytes[8..10], &[0, 0], "mimetype must be stored");
    assert_eq!(&bytes[30..38], b"mimetype", "mimetype must be first");
    assert_eq!(
        &bytes[28..30],
        &[0, 0],
        "mimetype must have no extra header"
    );
    let archive = zip::Archive::open(&bytes).unwrap();
    assert!(odt::is_odt(&bytes));
    for name in [
        "content.xml",
        "styles.xml",
        "meta.xml",
        "META-INF/manifest.xml",
    ] {
        xml::parse(&archive.read(name).unwrap().unwrap()).unwrap();
    }
    let content = String::from_utf8(archive.read("content.xml").unwrap().unwrap()).unwrap();
    let styles = String::from_utf8(archive.read("styles.xml").unwrap().unwrap()).unwrap();
    assert!(styles.contains("style:display-name=\"Review &amp; Notes\""));
    assert!(styles.contains("fo:font-size=\"15.5pt\""));
    assert!(content.contains("fo:font-size=\"13.5pt\""));
    assert!(content.contains("fo:font-weight=\"normal\""));
    assert!(content.contains("<text:tab/>"));
    assert!(content.contains("<text:s/><text:s/>"));
    assert!(content.contains("fo:break-before=\"page\""));
    assert!(content.contains("<table:table "));
    assert!(content.contains("<text:note "));
    assert!(content.contains("café"));
    assert!(archive.names().any(|n| n.starts_with("Pictures/")));
    if let Some(dir) = std::env::var_os("QUILL_EXPORT_OUTPUT") {
        let dir = std::path::Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("sample.odt"), bytes).unwrap();
        std::fs::write(dir.join("sample.html"), html::write(&doc)).unwrap();
        for name in [
            "content.xml",
            "styles.xml",
            "meta.xml",
            "META-INF/manifest.xml",
        ] {
            // Reopen the archive above after its bytes have moved into write.
            let bytes = std::fs::read(dir.join("sample.odt")).unwrap();
            let z = zip::Archive::open(&bytes).unwrap();
            std::fs::write(
                dir.join(name.replace('/', "-")),
                z.read(name).unwrap().unwrap(),
            )
            .unwrap();
        }
    }
}

#[test]
fn odt_and_html_save_atomically_and_open_again() {
    let dir = std::env::temp_dir().join(format!("quill-export-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let doc = fixture();
    for (name, format) in [
        ("out.odt", Format::Odt),
        ("out.HTML", Format::Html),
        ("out.htm", Format::Html),
    ] {
        let path = dir.join(name);
        assert_eq!(Format::from_path(&path), Some(format));
        quill_formats::save(&doc, &path, format).unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
        let loaded = quill_formats::load(&path).unwrap();
        assert!(loaded.main.paras[0].text.contains("Styled café"), "{name}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn odt_export_reads_back_with_its_text_formats_tables_pictures_and_notes() {
    let doc = fixture();
    let back = quill_formats::odt_read::read(&odt::write(&doc, 0)).unwrap();
    // Text, apart from the page break, which becomes a new paragraph that
    // starts a page.
    let text = |d: &Document| {
        d.main
            .paras
            .iter()
            .map(|p| p.text.replace(chars::PAGE_BREAK, ""))
            .collect::<Vec<_>>()
            .join("|")
    };
    let joined = text(&back);
    assert!(joined.contains("Styled café & <text> "));
    assert!(joined.contains("\t two  spaces\u{000b}new line"));
    assert!(joined.contains("new page"));
    assert!(back.main.paras.iter().any(|p| p.pap.page_break_before));
    // The style comes back by name with its formats.
    let style = back.styles.find("Review & Notes").unwrap();
    let st = back.styles.get(style);
    assert_eq!(st.pap.align, Align::Center);
    assert_eq!((st.pap.before, st.pap.after), (240, 120));
    assert!(st.chp.bold);
    assert_eq!(back.font_name(st.chp.font), "Georgia");
    // Direct formatting on the second run.
    let first = &back.main.paras[0];
    let at = first.text.find("red italic").unwrap();
    let red = first.chp_of_char(at);
    assert!(red.italic && !red.bold);
    assert_eq!(red.underline, Underline::Double);
    assert_eq!(red.color, Rgb(0xff0000));
    assert_eq!(red.size, 27);
    // The table's cells, the picture and the footnote.
    assert!(joined.contains("Cell 1") && joined.contains("Cell 2"));
    assert_eq!(
        back.main.paras.iter().filter(|p| p.row.is_some()).count(),
        2
    );
    assert!(first.text.contains(chars::OBJECT));
    let pic = back.picture(first.chp_of_char(0).object).unwrap();
    assert_eq!(pic.size, (720, 720));
    assert!(first.text.contains(chars::FOOTNOTE));
    assert!(
        back.notes
            .paras
            .iter()
            .any(|p| p.text.contains("Footnote with café"))
    );
    assert_eq!(back.summary.title, "Export <review> & café");
}

#[test]
fn links_export_to_html_and_odt_and_read_back() {
    let mut doc = Document::new();
    let base = doc.main.paras[0].mark_chp;
    doc.insert_text(StoryId::Main, Position::new(0, 0), "Go there now", base);
    let link = quill_core::link::intern(quill_core::Link {
        new_window: true,
        hover: Rgb(0xFF0000),
        ..quill_core::Link::new("https://example.com/?a=1&b=2")
    });
    doc.set_link(
        StoryId::Main,
        Position::new(0, 3),
        Position::new(0, 8),
        link,
        quill_core::link::BROWSER_BLUE,
    );

    let html = String::from_utf8(html::write(&doc)).unwrap();
    assert!(html.contains(
        r#"<a href="https://example.com/?a=1&amp;b=2" target="_blank" rel="noopener" class="lk l0">"#
    ));
    assert!(
        html.contains("a.l0{color:#0000ee;}a.l0:visited{color:#551a8b;}a.l0:hover{color:#ff0000;}")
    );
    let back = html::read_file(html.as_bytes()).unwrap().0;
    let p = &back.main.paras[0];
    assert_eq!(p.text, "Go there now");
    let c = p.chp_of_char(4);
    assert_eq!(
        quill_core::link::url(c.link),
        "https://example.com/?a=1&b=2"
    );
    assert!(quill_core::link::get(c.link).unwrap().new_window);
    assert_eq!(c.color, quill_core::link::BROWSER_BLUE);
    assert_eq!(p.chp_of_char(0).link, 0);
    assert_eq!(p.chp_of_char(9).link, 0);

    let back = quill_formats::odt_read::read(&odt::write(&doc, 0)).unwrap();
    let p = &back.main.paras[0];
    assert_eq!(p.text, "Go there now");
    let c = p.chp_of_char(4);
    assert_eq!(
        quill_core::link::url(c.link),
        "https://example.com/?a=1&b=2"
    );
    assert!(quill_core::link::get(c.link).unwrap().new_window);
    assert_eq!(c.underline, Underline::Single);
    assert_eq!(p.chp_of_char(9).link, 0);
}

#[test]
fn symbol_font_codes_survive_every_format_that_keeps_fonts() {
    // What Word gives Quill for a smiley, a ticked box and code 95 in
    // Wingdings: the codes, in the symbol font.
    let mut doc = Document::new();
    let chp = CharProps {
        font: doc.font_id("Wingdings"),
        ..Default::default()
    };
    doc.main.paras[0] = Paragraph::with_text("J\u{FE}\u{95}", chp, ParaProps::default());
    let dir = std::env::temp_dir().join(format!("quill-symbol-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, format) in [
        ("s.qwp", Format::Quill),
        ("s.docx", Format::Docx),
        ("s.rtf", Format::Rtf),
        ("s.odt", Format::Odt),
        ("s.html", Format::Html),
    ] {
        let path = dir.join(name);
        quill_formats::save(&doc, &path, format).unwrap();
        let back = quill_formats::load(&path).unwrap();
        let p = &back.main.paras[0];
        // HTML cannot hold code 95 itself, so it keeps Word's private-use
        // form, which draws the same symbol.
        let want = match format {
            Format::Html => "J\u{FE}\u{F095}",
            _ => "J\u{FE}\u{95}",
        };
        assert_eq!(p.text, want, "{name}");
        assert_eq!(back.font_name(p.chp_of_char(0).font), "Wingdings", "{name}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_wide_page_is_marked_landscape_for_word() {
    // Word reads orientation from its landscape flags, not from the sizes,
    // and calls a wide page without them portrait.
    let mut doc = Document::new();
    doc.props.page_width = 11906;
    doc.props.page_height = 8391;
    let dir = std::env::temp_dir().join(format!("quill-landscape-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, format) in [("l.docx", Format::Docx), ("l.rtf", Format::Rtf)] {
        let path = dir.join(name);
        quill_formats::save(&doc, &path, format).unwrap();
        let back = quill_formats::load(&path).unwrap();
        assert_eq!(
            (back.props.page_width, back.props.page_height),
            (11906, 8391),
            "{name}"
        );
    }
    let rtf = std::fs::read_to_string(dir.join("l.rtf")).unwrap();
    assert!(rtf.contains("\\paperw11906\\paperh8391\\landscape"));
    assert!(rtf.contains("\\lndscpsxn"));
    // A tall page carries neither flag.
    doc.props.page_width = 8391;
    doc.props.page_height = 11906;
    quill_formats::save(&doc, &dir.join("t.rtf"), Format::Rtf).unwrap();
    let tall = std::fs::read_to_string(dir.join("t.rtf")).unwrap();
    assert!(!tall.contains("landscape") && !tall.contains("lndscpsxn"));
    let _ = std::fs::remove_dir_all(dir);
}
