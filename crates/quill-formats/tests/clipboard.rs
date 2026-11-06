use quill_core::para::chars;
use quill_core::props::{Align, CharProps, Rgb, Underline};
use quill_core::{Document, Position, StoryId};
use quill_formats::{html, rtf};

#[test]
fn browser_html_preserves_inherited_and_direct_formatting_and_paragraphs() {
    let doc = html::read_clipboard(r#"<div style="font-family:Georgia;font-size:16px;color:rgb(20,40,60)"><p style="text-align:center"><b>Bold &amp; café</b> <span style="font-size:18pt;font-style:italic;color:#f00;text-decoration:underline;text-decoration-style:double">red</span> plain</p><p>Next &#x2764;&#xfe0f; &eacute;</p></div>"#).unwrap();
    assert_eq!(doc.main.paras.len(), 2);
    let p = &doc.main.paras[0];
    assert_eq!(p.text, "Bold & café red plain");
    assert_eq!(p.pap.align, Align::Center);
    let bold = p.chp_of_char(0);
    assert!(bold.bold);
    assert_eq!(bold.size, 24);
    assert_eq!(bold.color, Rgb(0x14283c));
    assert_eq!(doc.font_name(bold.font), "Georgia");
    let red = p.chp_of_char(p.text.find("red").unwrap());
    assert!(red.italic);
    assert!(!red.bold);
    assert_eq!(red.size, 36);
    assert_eq!(red.color, Rgb(0xff0000));
    assert_eq!(red.underline, Underline::Double);
    assert!(!p.chp_of_char(p.text.find("plain").unwrap()).italic);
    assert_eq!(doc.main.paras[1].text, "Next \u{2764}\u{fe0f} é");
}

#[test]
fn html_preserves_line_breaks_and_preformatted_spaces_but_ignores_active_content() {
    let doc = html::read_clipboard("<head><script>secret()</script><style>.x{color:red}</style></head><p>A<br>B<span style='display:none'>hidden</span><iframe src='https://invalid.example/'>remote</iframe></p><pre> x  y\r\nz</pre>").unwrap();
    assert_eq!(doc.main.paras.len(), 2);
    assert_eq!(doc.main.paras[0].text, format!("A{}B", chars::LINE_BREAK));
    assert_eq!(
        doc.main.paras[1].text,
        format!(" x  y{}z", chars::LINE_BREAK)
    );
}

#[test]
fn clipboard_html_export_is_self_contained_inline_styling() {
    let mut doc = Document::new();
    let font = doc.font_id("Georgia");
    let c = CharProps {
        font,
        size: 31,
        bold: true,
        color: Rgb(0x123456),
        ..Default::default()
    };
    doc.main.paras[0].insert(0, "Styled café", c);
    let markup = html::write_clipboard(&doc);
    assert!(!markup.contains("<style>"));
    assert!(markup.contains("font-size:15.5pt"));
    let imported = html::read_clipboard(&markup).unwrap();
    assert_eq!(imported.main.paras.len(), 1);
    assert_eq!(imported.main.paras[0].text, "Styled café");
    let back = imported.main.paras[0].chp_of_char(0);
    assert_eq!(imported.font_name(back.font), "Georgia");
    assert_eq!(
        (back.size, back.bold, back.color),
        (31, true, Rgb(0x123456))
    );
}

#[test]
fn rtf_inline_clip_does_not_add_a_paragraph_mark() {
    let mut doc = Document::new();
    doc.insert_plain(
        StoryId::Main,
        Position::new(0, 0),
        "Café \u{2764}\u{fe0f}",
        CharProps::default(),
    );
    let inline = rtf::write_clipboard(&doc, false);
    assert!(!inline.ends_with(b"\\par\n}"));
    let paragraphs = rtf::write_clipboard(&doc, true);
    assert!(paragraphs.ends_with(b"\\par\n}"));
    let back = rtf::read(&inline).unwrap();
    assert_eq!(back.main.paras[0].text, "Café \u{2764}\u{fe0f}");
}

#[test]
fn hostile_or_oversized_html_is_bounded_and_malformed_attributes_are_tolerated() {
    assert!(html::read_clipboard("Text <span style='unfinished").is_err());
    assert!(html::read_clipboard(&"<span>".repeat(140)).is_err());
    assert!(html::read_clipboard(&"x".repeat((8 << 20) + 1)).is_err());
    let doc =
        html::read_clipboard("<p><font face='A > B' color=blue><b>Text</font> tail</p>").unwrap();
    assert_eq!(doc.main.paras[0].text, "Text tail");
    assert_eq!(
        doc.font_name(doc.main.paras[0].chp_of_char(0).font),
        "A > B"
    );
}

#[test]
fn margin_shorthand_sets_paragraph_spacing_and_indents() {
    let doc = html::read_clipboard(r#"<p style="margin:6pt 0 12pt 36pt">one</p><p style="margin:3pt 18pt">two</p><p style="margin:auto">three</p>"#).unwrap();
    let one = &doc.main.paras[0].pap;
    assert_eq!(
        (one.before, one.right, one.after, one.left),
        (120, 0, 240, 720)
    );
    let two = &doc.main.paras[1].pap;
    assert_eq!(
        (two.before, two.right, two.after, two.left),
        (60, 360, 60, 360)
    );
    let three = &doc.main.paras[2].pap;
    assert_eq!((three.before, three.left), (0, 0));

    // Quill's own clipboard HTML writes only the shorthand.
    let mut source = Document::new();
    source.main.paras[0].insert(0, "Indented", CharProps::default());
    source.main.paras[0].pap.left = 720;
    source.main.paras[0].pap.first = -360;
    source.main.paras[0].pap.before = 120;
    source.main.paras[0].pap.after = 240;
    let back = html::read_clipboard(&html::write_clipboard(&source)).unwrap();
    let pap = &back.main.paras[0].pap;
    assert_eq!(
        (pap.left, pap.first, pap.before, pap.after),
        (720, -360, 120, 240)
    );
}

#[test]
fn font_lists_skip_generic_families_and_system_aliases() {
    let doc = html::read_clipboard(r#"<p><span style='font-family:-apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica'>web</span> <span style="font-family:system-ui, sans-serif">generic</span> <font face="inherit, Georgia">old</font></p>"#).unwrap();
    let p = &doc.main.paras[0];
    let font = |word: &str| doc.font_name(p.chp_of_char(p.text.find(word).unwrap()).font);
    assert_eq!(font("web"), "Segoe UI");
    assert_eq!(font("generic"), doc.font_name(doc.styles.get(0).chp.font));
    assert_eq!(font("old"), "Georgia");
    assert!(doc.fonts.iter().all(|name| !name.starts_with('-')));
}

#[test]
fn table_rows_keep_columns_when_cells_are_empty() {
    let doc = html::read_clipboard(
        "<table><tr><td>a</td> <td></td>\n<td>c</td></tr><tr><th>x</th> <th>y</th></tr></table>",
    )
    .unwrap();
    let texts: Vec<&str> = doc.main.paras.iter().map(|p| p.text.as_str()).collect();
    // Each cell is a paragraph, then each row's end.
    assert_eq!(texts, ["a", "", "c", "", "x", "y", "", ""]);
    let row = |k: usize| doc.main.paras[k].row.as_ref().unwrap().cells.len();
    assert_eq!((row(3), row(6)), (3, 2));
}

#[test]
fn clipboard_tables_join_borders_and_keep_the_same_padding_in_html_and_rtf() {
    let mut doc = Document::new();
    doc.insert_table(StoryId::Main, Position::new(0, 0), 2, 2, 6000);
    for p in &mut doc.main.paras {
        if let Some(row) = p.row.as_mut() {
            row.gap = 216;
            row.indent = 0;
        }
    }
    let markup = html::write_clipboard(&doc);
    assert!(markup.contains("cellspacing=\"0\""));
    assert!(markup.contains("border-collapse:collapse;border-spacing:0"));
    assert_eq!(markup.matches(";padding:0pt 5.4pt 0pt 5.4pt").count(), 4);
    let bytes = rtf::write_clipboard(&doc, true);
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.contains("\\trgaph108\\trleft-108"));
    // Exercise the standard RTF reader without Quill's private row data.
    let standard = text
        .split("{\\*\\quillrow ")
        .enumerate()
        .map(|(i, part)| {
            if i == 0 {
                part
            } else {
                part.split_once('}').unwrap().1
            }
        })
        .collect::<String>();
    let back = rtf::read(standard.as_bytes()).unwrap();
    for row in back.main.paras.iter().filter_map(|p| p.row.as_ref()) {
        assert_eq!((row.gap, row.indent), (216, 0));
        assert_eq!(row.cells.len(), 2);
    }
    assert_eq!(
        back.main.paras.iter().filter(|p| p.row.is_some()).count(),
        2
    );
}

#[test]
fn word_cell_padding_overrides_row_defaults_and_resets_between_cells() {
    let source = br"{\rtf1
\trowd\trgaph15\trleft5\trpaddl15\trpaddr15\trpaddfl3\trpaddfr3
\clpadl0\clpadr79\clpadfl3\clpadfr3\cellx4325
\clpadl0\clpadr79\clpadfl3\clpadfr3\cellx8645
\pard\intbl One\cell Two\cell\row
\trowd\trgaph15\trpaddl50\trpaddr50\trpaddfl3\trpaddfr3
\clpadl900\clpadr900\clpadfl0\clpadfr0\cellx4320\cellx8640
\pard\intbl Three\cell Four\cell\row}";
    let doc = rtf::read(source).unwrap();
    let rows: Vec<_> = doc
        .main
        .paras
        .iter()
        .filter_map(|p| p.row.as_ref())
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].gap, rows[0].indent), (158, 84));
    assert_eq!(rows[1].gap, 100);
    assert_eq!(
        (rows[0].cells[0].width, rows[0].cells[1].width),
        (4320, 4320)
    );
}

#[test]
fn word_table_paragraphs_keep_inherited_space_after_and_explicit_overrides() {
    let bytes = br"{\rtf1{\stylesheet{\s0\sa160 Normal;}{\s1\sb40\sa240 Spaced;}}
\trowd\cellx3000\cellx6000
\pard\intbl normal\cell
\pard\intbl\s1 spaced\cell\row
\trowd\cellx3000\cellx6000
\pard\intbl\sa0 overridden\cell
\pard\intbl restored\cell\row}";
    let doc = rtf::read(bytes).unwrap();
    let cells: Vec<_> = doc
        .main
        .paras
        .iter()
        .filter(|p| !p.text.is_empty())
        .collect();
    assert_eq!(
        cells.iter().map(|p| p.pap.after).collect::<Vec<_>>(),
        [160, 240, 0, 160]
    );
    assert_eq!(cells[1].pap.before, 40);
    assert!(cells.iter().all(|p| p.pap.in_table));
    let back = rtf::read(&rtf::write_clipboard(&doc, true)).unwrap();
    for (before, after) in doc.main.paras.iter().zip(&back.main.paras) {
        assert_eq!(
            (before.pap.before, before.pap.after),
            (after.pap.before, after.pap.after)
        );
    }
}
