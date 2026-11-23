use super::{position, round_trip, valid};
use quill_core::contents::TocOptions;
use quill_core::{Document, Mark, Position, SectionStart, Selection, StoryId};

const MAIN: StoryId = StoryId::Main;
const KINDS: [SectionStart; 5] = [
    SectionStart::NoBreak,
    SectionStart::NewPage,
    SectionStart::OddPage,
    SectionStart::EvenPage,
    SectionStart::NewColumn,
];

/// The text of every paragraph outside `skip`, with the paragraph breaks
/// left out: putting a table of contents in, or splitting a paragraph for
/// it, must not change a single character of it.
fn text_outside(doc: &Document, skip: &std::ops::Range<usize>) -> String {
    doc.main
        .paras
        .iter()
        .enumerate()
        .filter(|(i, _)| !skip.contains(i))
        .map(|(_, p)| p.text.as_str())
        .collect()
}

/// Every character of the main text, with the paragraph breaks left out.
fn all_text(doc: &Document) -> String {
    doc.main.paras.iter().map(|p| p.text.as_str()).collect()
}

/// Section breaks of every kind, changing a break's kind, putting a table of
/// contents in at any point, deleting across them, and Undo and Redo, each
/// checked against what it promises. Shared by libFuzzer and the test suite.
pub fn exercise(data: &[u8]) {
    let mut doc =
        quill_core::text::from_plain_text("Cover page\nChapter One\nbody text\nChapter Two");
    let heading = doc.styles.find("heading 1").unwrap();
    doc.apply_style(MAIN, 1, 1, heading);
    doc.apply_style(MAIN, 3, 3, heading);
    doc.forget_history();
    let mut undo = Vec::new();
    let mut redo = Vec::new();
    for op in data.as_chunks::<6>().0.iter().take(64) {
        valid(&doc);
        let a = position(&doc, op[1], op[2]);
        let b = position(&doc, op[3], op[4]);
        let selection = Selection::new(a, b);
        let (start, end) = selection.range();
        match op[0] % 10 {
            6 => {
                if let Some(expected) = undo.pop() {
                    let current = round_trip::snapshot(&doc);
                    doc.undo().expect("model has an undo step");
                    assert_eq!(
                        round_trip::snapshot(&doc),
                        expected,
                        "Undo restores exactly"
                    );
                    redo.push(current);
                }
            }
            7 => {
                if let Some(expected) = redo.pop() {
                    let current = round_trip::snapshot(&doc);
                    doc.redo().expect("model has a redo step");
                    assert_eq!(
                        round_trip::snapshot(&doc),
                        expected,
                        "Redo restores exactly"
                    );
                    undo.push(current);
                }
            }
            8 => round_trip::check_supported(&doc),
            action => {
                let before = round_trip::snapshot(&doc);
                let everything = all_text(&doc);
                let breaks = doc
                    .main
                    .paras
                    .iter()
                    .filter(|p| p.mark == Mark::SectionBreak)
                    .count();
                doc.mark_saved();
                doc.begin("Fuzz section edit", selection);
                let mut caret = start;
                match action {
                    0 => {
                        // Insert > Break > Section Break, of any kind.
                        caret = doc.insert_section_break(start);
                        let kind = KINDS[usize::from(op[5]) % KINDS.len()];
                        doc.format_sections(caret.para, caret.para, |s| s.start = kind);
                        assert_eq!(doc.section_props_at(caret.para).start, kind);
                        assert_eq!(
                            doc.main
                                .paras
                                .iter()
                                .filter(|p| p.mark == Mark::SectionBreak)
                                .count(),
                            breaks + 1
                        );
                        assert_eq!(
                            all_text(&doc),
                            everything,
                            "a break must not change the text"
                        );
                    }
                    1 => {
                        // Right-click a break: change the kind of the
                        // sections the selection touches, and nothing else.
                        let kind = KINDS[usize::from(op[5]) % KINDS.len()];
                        let sections = doc.sections().len();
                        let others: Vec<_> = (0..doc.main.paras.len())
                            .filter(|&i| i < start.para || i > end.para)
                            .map(|i| (i, doc.section_at(i)))
                            .collect();
                        let before_kinds: Vec<_> =
                            (0..sections).map(|k| doc.nth_section(k).start).collect();
                        doc.format_sections(start.para, end.para, |s| s.start = kind);
                        assert_eq!(doc.sections().len(), sections);
                        assert_eq!(doc.section_props_at(start.para).start, kind);
                        let touched = doc.section_at(start.para)..=doc.section_at(end.para);
                        for (k, was) in before_kinds.iter().enumerate() {
                            if !touched.contains(&k) {
                                assert_eq!(doc.nth_section(k).start, *was, "section {k}");
                            }
                        }
                        let _ = others;
                        assert_eq!(all_text(&doc), everything);
                    }
                    2 => {
                        // Insert > Table of Contents at the insertion point.
                        let existing_before = doc.existing_toc();
                        let text = text_outside(&doc, &existing_before.clone().unwrap_or(0..0));
                        let o = TocOptions::default();
                        let entries: Vec<_> = doc
                            .toc_entries(&o)
                            .into_iter()
                            .map(|e| (e, "1".to_string()))
                            .collect();
                        let paras = doc.toc_paragraphs(&entries, true);
                        let range = doc.place_toc(paras, start);
                        assert_eq!(
                            doc.main.paras[range.end - 1].mark,
                            Mark::SectionBreak,
                            "the contents end their section"
                        );
                        // Contents made before are replaced where they are.
                        if let Some(old) = &existing_before {
                            assert_eq!(range.start, old.start, "replaced in place");
                            assert_eq!(doc.existing_toc(), Some(range.clone()));
                        }
                        // Text the person left in contents styles before it
                        // can look like contents too, but there is always some.
                        assert!(doc.existing_toc().is_some());
                        assert_eq!(
                            text_outside(&doc, &range),
                            text,
                            "contents must not touch the text"
                        );
                        caret = Position::new(range.start, 0);
                    }
                    3 => caret = doc.delete(MAIN, start, end),
                    4 => {
                        let words = ["x", "é", "\u{1f980}", "a\nb"][usize::from(op[5]) % 4];
                        caret = doc.insert_plain(MAIN, start, words, Default::default());
                    }
                    5 => {
                        let style = doc.styles.find("heading 1").unwrap();
                        doc.apply_style(MAIN, start.para, start.para, style);
                    }
                    _ => {
                        caret = doc.insert_plain(MAIN, start, "\n", Default::default());
                    }
                }
                let caret = doc.main.clamp(caret);
                doc.commit(Selection::caret(caret));
                if doc.is_modified() {
                    undo.push(before);
                    redo.clear();
                }
            }
        }
        valid(&doc);
        assert_eq!(doc.undo_label().is_some(), !undo.is_empty());
        assert_eq!(doc.redo_label().is_some(), !redo.is_empty());
    }
    // Every kind and every placement must survive a save in each format.
    round_trip::check_supported(&doc);
}
