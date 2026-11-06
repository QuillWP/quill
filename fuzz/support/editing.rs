use quill_core::notes::AnnotationInfo;
use quill_core::{CharProps, Document, Position, Selection, StoryId};

#[path = "character_styles.rs"]
pub mod character_styles;
#[path = "round_trip.rs"]
pub(crate) mod round_trip;
const MAIN: StoryId = StoryId::Main;

fn position(doc: &Document, a: u8, b: u8) -> Position {
    let para = usize::from(a) % doc.main.paras.len();
    let text = &doc.main.paras[para].text;
    let offsets: Vec<_> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain([text.len()])
        .collect();
    Position::new(para, offsets[usize::from(b) % offsets.len()])
}

pub fn valid(doc: &Document) {
    for story in [&doc.main, &doc.notes, &doc.annotations]
        .into_iter()
        .chain(doc.headers.iter())
    {
        assert!(!story.paras.is_empty());
        for para in &story.paras {
            let mut offset = 0;
            for run in &para.runs {
                assert!(run.len > 0);
                offset += run.len;
                assert!(para.text.is_char_boundary(offset));
                if run.chp.rev != 0 {
                    assert!(quill_core::revision::get(run.chp.rev).is_some());
                }
            }
            assert_eq!(offset, para.text.len());
            assert!(para.runs.windows(2).all(|pair| pair[0].chp != pair[1].chp));
        }
    }
}

/// Bounded programs of real document operations, shared by libFuzzer and the
/// deterministic test suite. Coordinates always land on UTF-8 boundaries.
pub fn exercise(data: &[u8]) {
    exercise_plain(data);
    let mut doc = quill_core::text::from_plain_text("First café\nSecond \u{1f980} paragraph");
    doc.forget_history();
    let mut undo = Vec::new();
    let mut redo = Vec::new();
    for op in data.as_chunks::<6>().0.iter().take(96) {
        valid(&doc);
        let a = position(&doc, op[1], op[2]);
        let b = position(&doc, op[3], op[4]);
        let mut selection = Selection::new(a, b);
        let (start, end) = selection.range();
        match op[0] % 13 {
            10 => {
                if let Some(expected) = undo.pop() {
                    let current = round_trip::snapshot(&doc);
                    selection = doc.undo().expect("model has an undo step");
                    assert_eq!(
                        round_trip::snapshot(&doc),
                        expected,
                        "Undo must restore the exact saved document"
                    );
                    redo.push(current);
                    assert_eq!(doc.main.clamp(selection.anchor), selection.anchor);
                    assert_eq!(doc.main.clamp(selection.focus), selection.focus);
                }
            }
            11 => {
                if let Some(expected) = redo.pop() {
                    let current = round_trip::snapshot(&doc);
                    let restored = doc.redo().expect("model has a redo step");
                    assert_eq!(doc.main.clamp(restored.anchor), restored.anchor);
                    assert_eq!(doc.main.clamp(restored.focus), restored.focus);
                    assert_eq!(
                        round_trip::snapshot(&doc),
                        expected,
                        "Redo must restore the exact saved document"
                    );
                    undo.push(current);
                }
            }
            12 => round_trip::check_supported(&doc),
            action => {
                let before = round_trip::snapshot(&doc);
                doc.mark_saved();
                doc.begin("Fuzz edit", selection);
                let chp = CharProps {
                    object: 0,
                    ..doc.main.paras[start.para].chp_at(start.offset)
                };
                match action {
                    0 | 1 => {
                        let text =
                            ["a", "é", "\u{1f980}", "\n", "one\ntwo", " "][usize::from(op[5]) % 6];
                        let at = doc.insert_plain(MAIN, start, text, chp);
                        selection = Selection::caret(at);
                    }
                    2 => {
                        selection = Selection::caret(doc.delete(MAIN, start, end));
                    }
                    3 => doc.format_chars(MAIN, start, end, |c| c.bold = !c.bold),
                    4 => {
                        let mut props = doc.props.clone();
                        props.track_changes = !props.track_changes;
                        doc.set_document_props("Fuzz tracking", props, selection);
                    }
                    5 => {
                        doc.author = ["Ada", "Grace", "Linus"][usize::from(op[5]) % 3].into();
                    }
                    6 => {
                        selection = Selection::caret(doc.accept_changes(MAIN, start, end));
                    }
                    7 => {
                        selection = Selection::caret(doc.reject_changes(MAIN, start, end));
                    }
                    8 => {
                        // Exchange formats support one continuous range per
                        // comment. Keep generated ranges disjoint; overlapping
                        // ranges are a separate compatibility case.
                        let clear = start.para == end.para
                            && doc.main.paras[start.para]
                                .run_bounds()
                                .filter(|(a, b, _)| *a < end.offset && *b > start.offset)
                                .all(|(_, _, c)| c.comment == 0 && c.object == 0);
                        if !clear {
                            doc.commit(selection);
                            continue;
                        }
                        let info = AnnotationInfo {
                            author: "Reviewer".into(),
                            initials: "R".into(),
                            date: "2026-10-24T12:00:00Z".into(),
                            ..Default::default()
                        };
                        let id = doc.insert_annotation_on(start, end, chp, info);
                        if let Some(at) = doc.annotation_text_start(id) {
                            doc.insert_text(
                                StoryId::Annotations,
                                at,
                                "Comment é",
                                CharProps::default(),
                            );
                        }
                    }
                    9 => {
                        let mut ids: Vec<_> = doc.annotation_info.keys().copied().collect();
                        ids.sort_unstable();
                        if let Some(id) = ids.get(usize::from(op[5]) % ids.len().max(1)) {
                            doc.set_annotation_done(*id, !doc.annotation_info[id].done);
                        }
                    }
                    _ => unreachable!(),
                }
                selection.anchor = doc.main.clamp(selection.anchor);
                selection.focus = doc.main.clamp(selection.focus);
                doc.commit(selection);
                // Resolution metadata is deliberately outside text Undo in the
                // current editor. Treat it as a history boundary in this model.
                if action == 9 {
                    doc.forget_history();
                    undo.clear();
                    redo.clear();
                    round_trip::check_supported(&doc);
                    continue;
                }
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
    round_trip::check_supported(&doc);
}

/// Independent String model: a wrong insert/delete must fail even if all file
/// writers faithfully serialize the same wrong result.
fn exercise_plain(data: &[u8]) {
    let mut doc = quill_core::text::from_plain_text("Café\nRust \u{1f980}");
    doc.forget_history();
    let mut text = "Café\nRust \u{1f980}".to_string();
    let mut undo = Vec::new();
    let mut redo = Vec::new();
    for op in data.as_chunks::<6>().0.iter().take(96) {
        let a = position(&doc, op[1], op[2]);
        let b = position(&doc, op[3], op[4]);
        let selection = Selection::new(a, b);
        let (start, end) = selection.range();
        let offset = |at: Position| {
            doc.main.paras[..at.para]
                .iter()
                .map(|p| p.len() + 1)
                .sum::<usize>()
                + at.offset
        };
        let (s, e) = (offset(start), offset(end));
        match op[0] % 4 {
            0 | 1 => {
                let before = text.clone();
                doc.mark_saved();
                doc.begin("Fuzz plain edit", selection);
                if op[0] % 4 == 0 {
                    let inserted = ["x", "é", "\u{1f980}", "a\nb"][usize::from(op[5]) % 4];
                    text.insert_str(s, inserted);
                    doc.insert_plain(MAIN, start, inserted, CharProps::default());
                } else {
                    text.replace_range(s..e, "");
                    doc.delete(MAIN, start, end);
                }
                doc.commit(Selection::caret(doc.main.clamp(start)));
                if doc.is_modified() {
                    undo.push(before);
                    redo.clear();
                }
            }
            2 => {
                if let Some(before) = undo.pop() {
                    doc.undo().expect("plain model has Undo");
                    redo.push(std::mem::replace(&mut text, before));
                }
            }
            3 => {
                if let Some(after) = redo.pop() {
                    doc.redo().expect("plain model has Redo");
                    undo.push(std::mem::replace(&mut text, after));
                }
            }
            _ => unreachable!(),
        }
        let actual = doc
            .main
            .paras
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            actual, text,
            "plain edits must match the independent String model"
        );
        valid(&doc);
        assert_eq!(doc.undo_label().is_some(), !undo.is_empty());
        assert_eq!(doc.redo_label().is_some(), !redo.is_empty());
    }
}
