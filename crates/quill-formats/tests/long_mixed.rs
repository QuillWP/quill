//! A long document with every kind of content, for measuring speed and
//! memory on something more like real work than plain text: chapters with
//! headings and formatted text, footnotes, annotations, index entries, a
//! table and a picture in every chapter, framed pull quotes, sections
//! changing between one and two columns, and headers and footers.
//!
//! It is not run with the other tests. Make one with:
//!
//! ```text
//! QUILL_LONG_MIXED=/path/long.qwp QUILL_CHAPTERS=300 \
//!     cargo test -p quill-formats --test long_mixed -- --ignored --nocapture
//! ```

use quill_core::notes::AnnotationInfo;
use quill_core::para::chars;
use quill_core::props::{BorderStyle, Frame, FrameH, FrameRel, FrameV};
use quill_core::table::{row_at, row_props};
use quill_core::{CharProps, Document, Picture, Position, Rgb, Story, StoryId};

mod common;

/// A PNG of `w` x `h` pixels, a gradient tinted by `seed` so every
/// chapter's picture is its own.
fn png(w: u32, h: u32, seed: u32) -> Vec<u8> {
    common::png(w, h, |x, y| {
        [
            (x * 255 / w) as u8,
            (y * 255 / h) as u8,
            (seed * 37 % 256) as u8,
        ]
    })
}

/// Some words, different for each `seed`.
fn words(seed: usize, n: usize) -> String {
    const W: [&str; 16] = [
        "the",
        "report",
        "shows",
        "that",
        "quarterly",
        "results",
        "were",
        "strong",
        "across",
        "every",
        "region",
        "while",
        "costs",
        "fell",
        "and",
        "margins",
    ];
    (0..n)
        .map(|i| W[(seed * 7 + i * 3 + i / 5) % W.len()])
        .collect::<Vec<_>>()
        .join(" ")
}

struct Builder {
    doc: Document,
    base: CharProps,
}

impl Builder {
    /// The last paragraph, which is always empty and waiting for text.
    fn last(&self) -> usize {
        self.doc.main.paras.len() - 1
    }

    /// Add a paragraph of `text` in style `style`, returning its number.
    fn para(&mut self, text: &str, style: &str) -> usize {
        let i = self.last();
        let st = self.doc.styles.find(style).unwrap_or(0);
        self.doc.apply_style(StoryId::Main, i, i, st);
        let chp = self.doc.main.paras[i].mark_chp;
        self.doc
            .insert_text(StoryId::Main, Position::new(i, 0), text, chp);
        let end = self.doc.main.paras[i].len();
        self.doc
            .split_paragraph(StoryId::Main, Position::new(i, end));
        let normal = self.doc.styles.find("Normal").unwrap_or(0);
        let next = self.last();
        self.doc.apply_style(StoryId::Main, next, next, normal);
        i
    }

    /// Format bytes `a..b` of paragraph `i`.
    fn format(&mut self, i: usize, a: usize, b: usize, f: impl FnMut(&mut CharProps)) {
        self.doc
            .format_chars(StoryId::Main, Position::new(i, a), Position::new(i, b), f);
    }

    fn chapter(&mut self, k: usize, picture: &[u8]) {
        self.para(&format!("Chapter {k}: {}", words(k, 4)), "heading 1");
        for s in 0..3 {
            if s > 0 {
                self.para(&format!("Section {k}.{s}"), "heading 2");
            }
            for p in 0..2 {
                let seed = k * 10 + s * 3 + p;
                let i = self.para(&words(seed, 90 + seed % 60), "Normal");
                self.format(i, 4, 10, |c| c.bold = true);
                self.format(i, 30, 45, |c| c.italic = true);
                if p == 1 {
                    // An index entry, as hidden text.
                    let end = self.doc.main.paras[i].len();
                    let hidden = CharProps {
                        hidden: true,
                        ..self.base
                    };
                    self.doc.insert_text(
                        StoryId::Main,
                        Position::new(i, end),
                        &format!(".i.Topic {seed};"),
                        hidden,
                    );
                }
                if s == 1 && p == 0 {
                    let end = self.doc.main.paras[i].len();
                    let id = self.doc.insert_note(Position::new(i, end), self.base, None);
                    let at = self.doc.note_text_start(id).unwrap();
                    let chp = self.doc.notes.paras[at.para].mark_chp;
                    self.doc
                        .insert_text(StoryId::Notes, at, &words(seed, 25), chp);
                }
                if s == 2 && p == 1 && k.is_multiple_of(3) {
                    let end = self.doc.main.paras[i].len();
                    let id = self.doc.insert_annotation(
                        Position::new(i, end),
                        self.base,
                        AnnotationInfo {
                            initials: "RV".into(),
                            author: "Reviewer".into(),
                            date: "2026-10-15".into(),
                            ..Default::default()
                        },
                    );
                    let at = self.doc.annotation_text_start(id).unwrap();
                    let chp = self.doc.annotations.paras[at.para].mark_chp;
                    self.doc
                        .insert_text(StoryId::Annotations, at, "Check these figures.", chp);
                }
            }
        }
        // A pull quote in a frame, every fifth chapter.
        if k.is_multiple_of(5) {
            let i = self.para(&format!("\u{201c}{}\u{201d}", words(k, 18)), "Normal");
            self.doc.format_paras(StoryId::Main, i, i, |p| {
                p.frame = Some(Frame {
                    h: FrameH::Right,
                    h_rel: FrameRel::Margin,
                    v: FrameV::InLine,
                    v_rel: FrameRel::Margin,
                    width: Some(2880),
                    from_text: 180,
                    drop_lines: 0,
                    in_margin: false,
                });
                p.borders.top.style = BorderStyle::Single;
                p.borders.bottom.style = BorderStyle::Single;
            });
        }
        // A table: a header row, then figures; every fourth chapter merges
        // two cells of the last row.
        self.para(&format!("Table {k}. Figures by region."), "Normal");
        let at = self.last();
        self.doc
            .insert_table(StoryId::Main, Position::new(at, 0), 6, 4, 2160);
        let mut row = at;
        for r in 0..6 {
            let Some(rw) = row_at(&self.doc.main.paras, row) else {
                break;
            };
            for (c, &(a, _)) in rw.cells.iter().enumerate().rev() {
                let text = if r == 0 {
                    ["Region", "Q1", "Q2", "Q3"][c].to_string()
                } else {
                    format!("{}", (k * 31 + r * 7 + c * 13) % 1000)
                };
                let chp = CharProps {
                    bold: r == 0,
                    ..self.doc.main.paras[a].mark_chp
                };
                self.doc
                    .insert_text(StoryId::Main, Position::new(a, 0), &text, chp);
            }
            let rw = row_at(&self.doc.main.paras, row).unwrap();
            if r == 0 {
                let mut p = row_props(&self.doc.main.paras, &rw, 8640);
                for cell in &mut p.cells {
                    cell.shading = 150;
                    cell.borders.bottom.style = BorderStyle::Single;
                }
                self.doc.set_row(StoryId::Main, rw.end, p);
            }
            if r == 5 && k.is_multiple_of(4) {
                let p = row_props(&self.doc.main.paras, &rw, 8640);
                self.doc.merge_cells(StoryId::Main, &rw, &p, 1, 2);
            }
            let rw = row_at(&self.doc.main.paras, row).unwrap();
            row = rw.end + 1;
        }
        // A picture of its own.
        let mut pic = Picture::from_bytes(picture.to_vec()).unwrap();
        pic.size = (4320, 2880);
        let i = self.last();
        self.doc
            .insert_picture(StoryId::Main, Position::new(i, 0), pic, self.base);
        let end = self.doc.main.paras[i].len();
        self.doc
            .split_paragraph(StoryId::Main, Position::new(i, end));
        self.para(&format!("Figure {k}. {}", words(k + 3, 8)), "Normal");
        // Sections of ten chapters, alternating one and two columns.
        if (k + 1).is_multiple_of(10) {
            let i = self.last() - 1;
            let end = self.doc.main.paras[i].len();
            self.doc.insert_section_break(Position::new(i, end));
            let two = (k / 10).is_multiple_of(2);
            self.doc.format_sections(i, i, |s| {
                s.columns = if two { 2 } else { 1 };
                s.column_spacing = 360;
            });
        }
    }
}

#[test]
#[ignore]
fn make_long_mixed() {
    let Ok(path) = std::env::var("QUILL_LONG_MIXED") else {
        eprintln!("set QUILL_LONG_MIXED to the file to write");
        return;
    };
    let chapters: usize = std::env::var("QUILL_CHAPTERS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(300);
    let doc = quill_core::text::from_plain_text("");
    let base = doc.main.paras[0].mark_chp;
    let mut b = Builder { doc, base };
    for k in 1..=chapters {
        b.chapter(k, &png(240, 160, k as u32));
    }
    for (id, text) in [
        (
            StoryId::Header,
            format!("Annual report\tPage {}", chars::FIELD_PAGE),
        ),
        (StoryId::Footer, "Confidential".to_string()),
    ] {
        let mut story: Story = b.doc.story(id).clone();
        let chp = CharProps {
            color: Rgb(0x555555),
            ..story.paras[0].mark_chp
        };
        story.paras[0].insert(0, &text, chp);
        b.doc.set_story(id, story);
    }
    b.doc.forget_history();
    let bytes = quill_formats::native::write(&b.doc, 1_760_000_000);
    std::fs::write(&path, &bytes).unwrap();
    // Places to put the insertion point when measuring.
    let paras = &b.doc.main.paras;
    let mid = paras.len() / 2;
    let body = (mid..paras.len())
        .find(|&i| !paras[i].pap.in_table && paras[i].len() > 200)
        .unwrap();
    let cell = (mid..paras.len())
        .find(|&i| paras[i].pap.in_table && !paras[i].text.is_empty())
        .unwrap();
    println!(
        "wrote {path}: {} paragraphs, {} chapters, {} pictures, {} KB; \
         middle body paragraph {body}, middle table cell {cell}",
        paras.len(),
        chapters,
        b.doc.objects.len(),
        bytes.len() / 1024
    );
}
