//! Tables inside table cells: kept by every format that can hold them.

use quill_core::para::Mark;
use quill_core::props::CharProps;
use quill_core::{Document, Position, StoryId};
use quill_formats::{docx, rtf};

const MAIN: StoryId = StoryId::Main;

/// An outer table of one row and two cells. The first cell holds "before",
/// a table of two rows by two cells (a, b / c, d) and "after"; the second
/// holds "side".
fn nested() -> Document {
    let mut doc = Document::new();
    doc.insert_table(MAIN, Position::new(0, 0), 1, 2, 3000);
    doc.insert_text(MAIN, Position::new(0, 0), "before", CharProps::default());
    // End of the first cell's paragraph: split it, then make the table.
    doc.split_paragraph(MAIN, Position::new(0, 6));
    let at = doc.insert_table(MAIN, Position::new(1, 0), 2, 2, 1000);
    for (k, text) in ["a", "b", "c", "d"].iter().enumerate() {
        // Each inner row is cell, cell, row mark.
        let para = at.para + k / 2 * 3 + k % 2;
        doc.insert_text(MAIN, Position::new(para, 0), text, CharProps::default());
    }
    let after = doc
        .main
        .paras
        .iter()
        .position(|p| p.pap.level() == 1 && p.text.is_empty() && p.mark == Mark::CellEnd)
        .unwrap();
    doc.insert_text(MAIN, Position::new(after, 0), "after", CharProps::default());
    let side = doc
        .main
        .paras
        .iter()
        .position(|p| p.pap.level() == 1 && p.text.is_empty() && p.mark == Mark::CellEnd)
        .unwrap();
    doc.insert_text(MAIN, Position::new(side, 0), "side", CharProps::default());
    doc
}

/// Each paragraph as level, mark and text.
fn shape(doc: &Document) -> Vec<(u8, Mark, String)> {
    doc.main
        .paras
        .iter()
        .map(|p| (p.pap.level(), p.mark, p.text.clone()))
        .collect()
}

#[test]
fn the_test_document_is_what_it_says() {
    use Mark::*;
    let doc = nested();
    let want = [
        (1, Paragraph, "before"),
        (2, CellEnd, "a"),
        (2, CellEnd, "b"),
        (2, RowEnd, ""),
        (2, CellEnd, "c"),
        (2, CellEnd, "d"),
        (2, RowEnd, ""),
        (1, CellEnd, "after"),
        (1, CellEnd, "side"),
        (1, RowEnd, ""),
    ];
    let got = shape(&doc);
    assert_eq!(
        got[..want.len()],
        want.map(|(l, m, t)| (l, m, t.to_string()))
    );
}

#[test]
fn rtf_keeps_a_table_in_a_cell() {
    let doc = nested();
    let bytes = rtf::write(&doc);
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("\\itap2"));
    assert!(text.contains("\\nestcell"));
    assert!(text.contains("\\nestrow"));
    let back = rtf::read(&bytes).unwrap();
    assert_eq!(shape(&back), shape(&doc));
    // Both levels keep their own cell widths.
    let inner = quill_core::table::row_at(&back.main.paras, 1).unwrap();
    let p = quill_core::table::row_props(&back.main.paras, &inner, 9000);
    assert_eq!(
        p.cells.iter().map(|c| c.width).collect::<Vec<_>>(),
        [1000, 1000]
    );
    let outer = quill_core::table::row_at_level(&back.main.paras, 1, 1).unwrap();
    let p = quill_core::table::row_props(&back.main.paras, &outer, 9000);
    assert_eq!(
        p.cells.iter().map(|c| c.width).collect::<Vec<_>>(),
        [3000, 3000]
    );
    assert_eq!(outer.cells.len(), 2);
}

#[test]
fn rtf_from_word_reads_nested_cells_and_skips_the_flat_copy() {
    let rtf = br"{\rtf1\ansi \trowd\trgaph108\cellx5000\cellx9000
\pard\intbl before\par
\pard\intbl\itap2 a\nestcell b\nestcell{\*\nesttableprops\trowd\trgaph108\cellx1500\cellx3000\nestrow}{\nonesttables\par}
\pard\intbl after\cell side\cell\row
\pard end\par}";
    let doc = rtf::read(rtf).unwrap();
    use Mark::*;
    let want = [
        (1, Paragraph, "before"),
        (2, CellEnd, "a"),
        (2, CellEnd, "b"),
        (2, RowEnd, ""),
        (1, CellEnd, "after"),
        (1, CellEnd, "side"),
        (1, RowEnd, ""),
        (0, Paragraph, "end"),
    ];
    assert_eq!(
        shape(&doc),
        want.map(|(l, m, t)| (l, m, t.to_string())).to_vec()
    );
    let inner = quill_core::table::row_at(&doc.main.paras, 1).unwrap();
    let p = quill_core::table::row_props(&doc.main.paras, &inner, 9000);
    assert_eq!(
        p.cells.iter().map(|c| c.width).collect::<Vec<_>>(),
        [1500, 1500]
    );
}

#[test]
fn docx_keeps_a_table_in_a_cell() {
    let doc = nested();
    let bytes = docx::write(&doc, 0);
    let back = docx::read::read(&bytes).unwrap();
    // The reader adds the paragraph a document must end with.
    assert_eq!(shape(&back)[..10], shape(&doc)[..10]);
    let inner = quill_core::table::row_at(&back.main.paras, 1).unwrap();
    let p = quill_core::table::row_props(&back.main.paras, &inner, 9000);
    assert_eq!(
        p.cells.iter().map(|c| c.width).collect::<Vec<_>>(),
        [1000, 1000]
    );
    let outer = quill_core::table::row_at_level(&back.main.paras, 1, 1).unwrap();
    assert_eq!(outer.cells, [(0, 7), (8, 8)]);
}

#[test]
fn html_and_odt_keep_a_table_in_a_cell() {
    let doc = nested();
    let html = quill_formats::html::write(&doc);
    let text = String::from_utf8_lossy(&html);
    assert_eq!(text.matches("<table>").count(), 2);
    let back = quill_formats::html::read_clipboard(&text).unwrap();
    assert_eq!(shape(&back)[..10], shape(&doc)[..10]);

    let odt = quill_formats::odt::write(&doc, 0);
    let back = quill_formats::odt_read::read(&odt).unwrap();
    assert_eq!(shape(&back)[..10], shape(&doc)[..10]);
}
