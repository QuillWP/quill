#[path = "../../../fuzz/support/editing.rs"]
mod editing;
use editing::round_trip;

#[test]
fn generated_editing_programs_preserve_documents_and_history() {
    for seed in 0..128u64 {
        let mut state = seed + 1;
        let program: Vec<u8> = (0..6 * 64)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect();
        let result = std::panic::catch_unwind(|| editing::exercise(&program));
        assert!(result.is_ok(), "editing seed {seed}, program {program:?}");
    }
}

#[test]
fn generated_character_style_programs_preserve_documents_and_history() {
    for seed in 0..128u64 {
        let mut state = seed + 1;
        let program: Vec<u8> = (0..6 * 64)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect();
        let result = std::panic::catch_unwind(|| editing::character_styles::exercise(&program));
        assert!(result.is_ok(), "style seed {seed}, program {program:?}");
    }
}

#[test]
#[should_panic(expected = "changed")]
fn semantic_oracle_detects_silent_text_loss() {
    let doc = quill_core::text::from_plain_text("Keep this text");
    let back = quill_core::text::from_plain_text("Keep text");
    round_trip::assert_same(&doc, &back);
}

#[test]
fn restoring_original_format_keeps_the_tracked_editor_through_saves() {
    use quill_core::{CharProps, Position, StoryId};
    let mut doc = quill_core::text::from_plain_text("Edit me");
    doc.props.track_changes = true;
    doc.author = "Ada".into();
    let start = Position::new(0, 0);
    let end = Position::new(0, 4);
    doc.format_chars(StoryId::Main, start, end, |c| c.bold = true);
    // Tracking can be switched off before the appearance is restored.
    doc.props.track_changes = false;
    doc.format_chars(StoryId::Main, start, end, |c| c.bold = false);
    let c = doc.main.paras[0].chp_at(0);
    let revision = quill_core::revision::get(c.rev).unwrap();
    let (by, old) = revision.formatted.as_ref().unwrap();
    assert_eq!(by.author, "Ada");
    assert_eq!(*old, CharProps { rev: 0, ..c });
    round_trip::check_supported(&doc);
}

#[test]
#[should_panic(expected = "changed")]
fn semantic_oracle_detects_lost_revision_authorship() {
    use quill_core::{Position, StoryId};
    let mut doc = quill_core::text::from_plain_text("Original");
    doc.props.track_changes = true;
    doc.author = "Ada".into();
    doc.insert_text(
        StoryId::Main,
        Position::new(0, 0),
        "New",
        Default::default(),
    );
    let mut back = doc.clone();
    back.resolve_all_changes(true);
    round_trip::assert_same(&doc, &back);
}

#[test]
#[should_panic(expected = "lost")]
fn semantic_oracle_detects_lost_comment_text_and_author() {
    use quill_core::{Position, StoryId};
    let mut doc = quill_core::text::from_plain_text("About this");
    let id = doc.insert_annotation_on(
        Position::new(0, 0),
        Position::new(0, 5),
        Default::default(),
        quill_core::notes::AnnotationInfo {
            author: "Ada".into(),
            ..Default::default()
        },
    );
    let at = doc.annotation_text_start(id).unwrap();
    doc.insert_text(
        StoryId::Annotations,
        at,
        "Keep this comment",
        Default::default(),
    );
    let mut back = doc.clone();
    let mark = back.annotation_mark(id).unwrap();
    back.delete(
        StoryId::Main,
        mark,
        Position::new(mark.para, mark.offset + '\u{e004}'.len_utf8()),
    );
    round_trip::assert_same(&doc, &back);
}
