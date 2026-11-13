//! Checks for things a young word processor can get wrong on import and
//! export, run against Quill's readers and writers.

use quill_core::props::{BorderLine, BorderStyle};
use quill_core::{Document, Position, StoryId};
use quill_formats::{odt_read, rtf};

#[test]
fn rtf_keeps_unicode_title_author_and_link_targets() {
    let mut doc = Document::new();
    doc.summary.title = "Titre \u{e9}t\u{e9} \u{4e2d}\u{6587}".into();
    doc.summary.author = "Zo\u{eb} \u{41c}".into();
    let base = doc.main.paras[0].mark_chp;
    doc.insert_text(StoryId::Main, Position::new(0, 0), "Go there now", base);
    let url = "https://example.com/caf\u{e9}?q=\u{4e2d}&b={x}\\y";
    let link = quill_core::link::intern(quill_core::Link::new(url));
    doc.set_link(
        StoryId::Main,
        Position::new(0, 3),
        Position::new(0, 8),
        link,
        quill_core::link::BROWSER_BLUE,
    );
    let back = rtf::read(&rtf::write(&doc)).unwrap();
    assert_eq!(back.summary.title, doc.summary.title);
    assert_eq!(back.summary.author, doc.summary.author);
    let c = back.main.paras[0].chp_of_char(4);
    assert_eq!(quill_core::link::url(c.link), url);
}

#[test]
fn cell_shading_and_borders_survive_docx_and_rtf() {
    let mut doc = Document::new();
    doc.insert_table(StoryId::Main, Position::new(0, 0), 2, 1, 6000);
    let row = quill_core::table::row_at(&doc.main.paras, 0).unwrap();
    let mut props = quill_core::table::row_props(&doc.main.paras, &row, 6000);
    let line = BorderLine {
        style: BorderStyle::Thick,
        space: 0,
        ..Default::default()
    };
    for c in &mut props.cells {
        c.shading = 500;
        c.borders.top = line;
    }
    doc.set_row(StoryId::Main, row.end, props);
    let docx = quill_formats::docx::read::read(&quill_formats::docx::write(&doc, 0)).unwrap();
    let rtf = rtf::read(&rtf::write(&doc)).unwrap();
    for back in [docx, rtf] {
        let row = quill_core::table::row_at(&back.main.paras, 0).unwrap();
        let p = quill_core::table::row_props(&back.main.paras, &row, 6000);
        assert_eq!(p.cells[0].shading, 500);
        assert_eq!(p.cells[0].borders.top.style, BorderStyle::Thick);
    }
}

/// An ODT table cell with a background color and a border: the color stays
/// as the cell's background.
#[test]
fn odt_cells_read_background_and_border() {
    let x = r##"<?xml version="1.0" encoding="UTF-8"?>
<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" office:version="1.2" office:mimetype="application/vnd.oasis.opendocument.text">
<office:automatic-styles>
<style:style style:name="Tbl" style:family="table"><style:table-properties style:width="6in"/></style:style>
<style:style style:name="Red" style:family="table-cell"><style:table-cell-properties fo:background-color="#ff0000" fo:border="0.5pt solid #000000"/></style:style>
<style:style style:name="Plain" style:family="table-cell"><style:table-cell-properties fo:border="none"/></style:style>
</office:automatic-styles>
<office:body><office:text>
<table:table table:name="T" table:style-name="Tbl"><table:table-column table:number-columns-repeated="2"/>
<table:table-row><table:table-cell table:style-name="Red" office:value-type="string"><text:p>red</text:p></table:table-cell><table:table-cell table:style-name="Plain" office:value-type="string"><text:p>plain</text:p></table:table-cell></table:table-row></table:table>
<text:p>after</text:p></office:text></office:body></office:document>"##;
    let d = odt_read::read_flat_reported(x.as_bytes()).unwrap().0;
    let row = quill_core::table::row_at(&d.main.paras, 0).unwrap();
    let p = quill_core::table::row_props(&d.main.paras, &row, 6000);
    assert_eq!(p.cells[0].shading_color, Some(quill_core::Rgb(0xFF0000)));
    assert_eq!(p.cells[0].shading, 0);
    assert_eq!(p.cells[0].borders.top.style, BorderStyle::Single);
    assert_eq!(p.cells[1].shading, 0);
    assert_eq!(p.cells[1].borders.top.style, BorderStyle::None);
}

#[test]
fn odt_character_formats_come_through_common_and_derived_paragraph_styles() {
    let x = r##"<?xml version="1.0" encoding="UTF-8"?>
<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" office:version="1.2" office:mimetype="application/vnd.oasis.opendocument.text">
<office:styles>
<style:style style:name="Emph" style:family="paragraph"><style:text-properties fo:font-weight="bold" fo:font-style="italic" fo:font-size="18pt" fo:color="#ff0000"/></style:style>
<style:style style:name="Sub" style:family="paragraph" style:parent-style-name="Emph"><style:text-properties style:text-underline-style="solid"/></style:style>
</office:styles>
<office:automatic-styles>
<style:style style:name="P1" style:family="paragraph" style:parent-style-name="Sub"><style:paragraph-properties fo:text-align="center"/></style:style>
</office:automatic-styles>
<office:body><office:text>
<text:p text:style-name="Emph">common</text:p>
<text:p text:style-name="Sub">child</text:p>
<text:p text:style-name="P1">auto</text:p>
</office:text></office:body></office:document>"##;
    let d = odt_read::read_flat_reported(x.as_bytes()).unwrap().0;
    for p in &d.main.paras[..3] {
        let c = p.chp_of_char(0);
        assert!(c.bold && c.italic, "{:?}", p.text);
        assert_eq!((c.size, c.color.0), (36, 0xff0000), "{:?}", p.text);
    }
    assert_eq!(
        d.main.paras[1].chp_of_char(0).underline,
        quill_core::Underline::Single
    );
    assert_eq!(
        d.main.paras[2].chp_of_char(0).underline,
        quill_core::Underline::Single
    );
}

/// A two-cell table whose first cell is thick, shaded and bottom-aligned
/// and whose second has no borders at all.
fn mixed_cell_table() -> Document {
    let mut doc = Document::new();
    doc.insert_table(StoryId::Main, Position::new(0, 0), 1, 2, 6000);
    let row = quill_core::table::row_at(&doc.main.paras, 0).unwrap();
    let mut props = quill_core::table::row_props(&doc.main.paras, &row, 6000);
    let thick = BorderLine {
        style: BorderStyle::Thick,
        space: 0,
        ..Default::default()
    };
    let c = &mut props.cells[0];
    c.shading = 500;
    c.valign = quill_core::props::VAlign::Bottom;
    c.borders.top = thick;
    c.borders.bottom = thick;
    c.borders.left = thick;
    c.borders.right = thick;
    props.cells[1].borders = Default::default();
    doc.set_row(StoryId::Main, row.end, props);
    doc
}

#[test]
fn odt_export_keeps_cell_borders_shading_and_alignment() {
    let doc = mixed_cell_table();
    let back = odt_read::read(&quill_formats::odt::write(&doc, 0)).unwrap();
    let row = quill_core::table::row_at(&back.main.paras, 0).unwrap();
    let p = quill_core::table::row_props(&back.main.paras, &row, 6000);
    // ODT has no percentages, so 50% comes back as the gray it looks like.
    assert_eq!(
        quill_core::props::shading_fill(p.cells[0].shading, p.cells[0].shading_color),
        Some(quill_core::Rgb(0x808080))
    );
    assert_eq!(p.cells[0].borders.top.style, BorderStyle::Thick);
    assert_eq!(p.cells[0].borders.left.style, BorderStyle::Thick);
    assert_eq!(p.cells[0].valign, quill_core::props::VAlign::Bottom);
    assert_eq!(p.cells[1].shading, 0);
    assert!(p.cells[1].borders.is_none());
}

#[test]
fn html_export_writes_each_cells_borders_and_shading() {
    let doc = mixed_cell_table();
    let html = String::from_utf8(quill_formats::html::write(&doc)).unwrap();
    let cells: Vec<&str> = html.split("<td style=\"").skip(1).collect();
    assert_eq!(cells.len(), 2);
    let first = cells[0].split('"').next().unwrap();
    let second = cells[1].split('"').next().unwrap();
    assert!(first.contains("border:1.5pt solid"), "{first}");
    assert!(first.contains("background-color:#808080"), "{first}");
    // The page style gives every td a gray border; a borderless cell must say so.
    assert!(second.contains("border:none"), "{second}");
    assert!(!second.contains("background-color"), "{second}");
}
