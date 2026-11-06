//! Write a sample .docx exercising most formatting: `cargo run -p
//! quill-formats --example sample out.docx`.

use quill_core::para::{Mark, Paragraph};
use quill_core::props::{Align, TabAlign, TabStop};
use quill_core::{Position, StoryId, text};

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "sample.docx".into());
    let mut doc = text::from_plain_text(
        "Quill Sample\nThis paragraph has bold, italic and underlined words, and it is justified so that it fills the column from margin to margin.\nName\tAmount\nCentered closing line",
    );
    let main = StoryId::Main;
    let h1 = doc.styles.find("heading 1").unwrap();
    doc.apply_style(main, 0, 0, h1);
    let p1 = &doc.main.paras[1].text;
    let find = |w: &str| p1.find(w).unwrap();
    let (b, i, u) = (find("bold"), find("italic"), find("underlined"));
    doc.format_chars(main, Position::new(1, b), Position::new(1, b + 4), |c| {
        c.bold = true
    });
    doc.format_chars(main, Position::new(1, i), Position::new(1, i + 6), |c| {
        c.italic = true
    });
    doc.format_chars(main, Position::new(1, u), Position::new(1, u + 10), |c| {
        c.underline = quill_core::Underline::Single
    });
    doc.format_paras(main, 1, 1, |p| p.align = Align::Justify);
    doc.format_paras(main, 2, 2, |p| {
        p.set_tab(TabStop {
            pos: 4320,
            align: TabAlign::Right,
            ..Default::default()
        })
    });
    doc.format_paras(main, 3, 3, |p| p.align = Align::Center);
    // A 2 x 2 table before the last paragraph.
    let normal = doc.styles.get(0).clone();
    let cell = |t: &str, mark| {
        let mut p = Paragraph::with_text(t, normal.chp, normal.pap.clone());
        p.pap.in_table = true;
        p.mark = mark;
        p
    };
    let rows = [
        cell("A1", Mark::CellEnd),
        cell("B1", Mark::CellEnd),
        cell("", Mark::RowEnd),
        cell("A2", Mark::CellEnd),
        cell("B2", Mark::CellEnd),
        cell("", Mark::RowEnd),
    ];
    doc.main.paras.splice(3..3, rows);
    doc.summary.title = "Quill Sample".into();
    quill_formats::save(
        &doc,
        std::path::Path::new(&out),
        quill_formats::Format::Docx,
    )
    .unwrap();
}
