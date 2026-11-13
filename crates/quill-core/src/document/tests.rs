use super::*;
use crate::props::Align;

const MAIN: StoryId = StoryId::Main;

#[test]
fn matching_a_saved_copy_is_one_step_that_undoes() {
    let mut doc = crate::text::from_plain_text("One\nTwo\nThree");
    doc.mark_saved();
    let original = doc.clone();
    let mut saved = doc.clone();
    saved.main.paras[1].pap.left += 360;
    saved.props.margin_left += 20;
    saved.section.columns = 2;
    let sel = Selection::caret(p(1, 0));
    assert!(doc.match_saved(&saved, "Save Anyway", sel));
    let same = |a: &Document, b: &Document| {
        a.main == b.main && a.props == b.props && a.section == b.section && a.styles == b.styles
    };
    assert!(same(&doc, &saved));
    assert_eq!(doc.undo_label(), Some("Save Anyway"));
    doc.undo();
    assert!(same(&doc, &original));
    assert_eq!(texts(&doc), ["One", "Two", "Three"]);
    doc.redo();
    assert!(same(&doc, &saved));
    // Fonts cannot be put back by undo, so nothing changes.
    saved.fonts.push("Nowhere".into());
    let before = doc.clone();
    assert!(!doc.match_saved(&saved, "Save Anyway", sel));
    assert!(same(&doc, &before));
    assert_eq!(doc.undo_label(), Some("Save Anyway"));
}

fn p(para: usize, offset: usize) -> Position {
    Position::new(para, offset)
}

fn texts(doc: &Document) -> Vec<&str> {
    doc.main.paras.iter().map(|p| p.text.as_str()).collect()
}

fn type_text(doc: &mut Document, at: Position, text: &str) -> Position {
    doc.begin_typing(Selection::caret(at));
    let chp = doc.main.paras[at.para].chp_at(at.offset);
    let end = doc.insert_plain(MAIN, at, text, chp);
    doc.commit(Selection::caret(end));
    end
}

fn doc_with(paras: &[&str]) -> Document {
    let mut d = Document::new();
    type_text(&mut d, p(0, 0), &paras.join("\n"));
    d.history = History::default();
    d.saved_state = 0;
    d
}

#[test]
fn new_document_is_new_york_12() {
    let d = Document::new();
    assert_eq!(d.main.paras.len(), 1);
    let chp = d.main.paras[0].mark_chp;
    assert_eq!(d.font_name(chp.font), "New York");
    assert_eq!(chp.size, 24);
    assert!(!d.is_modified());
}

#[test]
fn typing_paragraphs() {
    let d = doc_with(&["Hello", "World"]);
    assert_eq!(texts(&d), ["Hello", "World"]);
}

#[test]
fn trailing_newline_leaves_empty_paragraph() {
    let mut d = Document::new();
    let end = type_text(&mut d, p(0, 0), "One\n");
    assert_eq!(texts(&d), ["One", ""]);
    assert_eq!(end, p(1, 0));
}

#[test]
fn return_splits_and_keeps_format() {
    let mut d = doc_with(&["Hello world"]);
    d.format_paras(MAIN, 0, 0, |pp| pp.align = Align::Center);
    let at = d.split_paragraph(MAIN, p(0, 5));
    assert_eq!(at, p(1, 0));
    assert_eq!(texts(&d), ["Hello", " world"]);
    assert_eq!(d.main.paras[1].pap.align, Align::Center);
}

#[test]
fn return_after_heading_gives_normal() {
    let mut d = doc_with(&["Title"]);
    let h1 = d.styles.find("heading 1").unwrap();
    d.apply_style(MAIN, 0, 0, h1);
    d.split_paragraph(MAIN, p(0, 5));
    assert_eq!(d.main.paras[0].pap.style, h1);
    assert_eq!(d.main.paras[1].pap.style, 0);
    assert!(!d.main.paras[1].mark_chp.bold);
}

#[test]
fn deleting_mark_takes_following_format() {
    let mut d = doc_with(&["First", "Second"]);
    d.format_paras(MAIN, 1, 1, |pp| pp.align = Align::Right);
    d.delete(MAIN, p(0, 5), p(1, 0));
    assert_eq!(texts(&d), ["FirstSecond"]);
    assert_eq!(d.main.paras[0].pap.align, Align::Right);
}

#[test]
fn delete_across_paragraphs() {
    let mut d = doc_with(&["Hello there", "big", "wide world"]);
    let at = d.delete(MAIN, p(0, 5), p(2, 4));
    assert_eq!(at, p(0, 5));
    assert_eq!(texts(&d), ["Hello world"]);
}

#[test]
fn delete_whole_paragraph_with_mark() {
    let mut d = doc_with(&["One", "Two", "Three"]);
    d.delete(MAIN, p(1, 0), p(2, 0));
    assert_eq!(texts(&d), ["One", "Three"]);
}

#[test]
fn undo_and_redo() {
    let mut d = doc_with(&["Hello"]);
    d.begin("Clear", Selection::new(p(0, 0), p(0, 5)));
    d.delete(MAIN, p(0, 0), p(0, 5));
    d.commit(Selection::caret(p(0, 0)));
    assert_eq!(texts(&d), [""]);
    assert_eq!(d.undo_label(), Some("Clear"));
    let sel = d.undo().unwrap();
    assert_eq!(sel, Selection::new(p(0, 0), p(0, 5)));
    assert_eq!(texts(&d), ["Hello"]);
    assert_eq!(d.redo_label(), Some("Clear"));
    let sel = d.redo().unwrap();
    assert_eq!(sel, Selection::caret(p(0, 0)));
    assert_eq!(texts(&d), [""]);
}

#[test]
fn undo_keeps_at_most_one_hundred_recent_actions() {
    let mut d = Document::new();
    let mut at = p(0, 0);
    for _ in 0..120 {
        d.begin("Insert", Selection::caret(at));
        let chp = d.main.paras[0].chp_at(at.offset);
        at = d.insert_plain(MAIN, at, "x", chp);
        d.commit(Selection::caret(at));
    }

    let mut undone = 0;
    while d.undo().is_some() {
        undone += 1;
    }

    assert_eq!(undone, 100);
    assert_eq!(d.main.paras[0].text.len(), 20);
    assert!(d.is_modified());
}

#[test]
fn undo_memory_budget_never_discards_the_newest_twenty_five_actions() {
    let mut d = Document::new();
    let paragraph = || {
        Paragraph::with_text(
            &"x".repeat(1024),
            CharProps::default(),
            ParaProps::default(),
        )
    };
    d.history.undo = (1..=30)
        .map(|id| Step {
            label: "Large edit",
            records: vec![Record {
                story: MAIN,
                first: 0,
                old: vec![paragraph()],
                new_len: 1,
                sheet: None,
                section: None,
                props: None,
                summary: None,
                hf_section: 0,
            }],
            before: Selection::default(),
            after: Selection::default(),
            id,
        })
        .collect();

    d.trim_history(25, 100, 0, false);

    assert_eq!(d.history.undo.len(), 25);
    assert_eq!(d.history.base_id, 5);
    assert!(d.undo_owned_bytes() > 0);
}

#[test]
fn one_action_larger_than_the_budget_is_still_undoable() {
    let mut d = Document::new();
    let old = Paragraph::with_text(
        &"x".repeat(1024 * 1024),
        CharProps::default(),
        ParaProps::default(),
    );
    d.history.undo.push(Step {
        label: "Large edit",
        records: vec![Record {
            story: MAIN,
            first: 0,
            old: vec![old],
            new_len: 1,
            sheet: None,
            section: None,
            props: None,
            summary: None,
            hf_section: 0,
        }],
        before: Selection::default(),
        after: Selection::default(),
        id: 1,
    });

    d.trim_history(25, 100, 0, false);

    assert_eq!(d.history.undo.len(), 1);
    assert_eq!(d.undo_label(), Some("Large edit"));
}

#[test]
fn pruning_history_releases_its_unplaced_picture_data() {
    use crate::picture::{Crop, ImageFormat};
    use std::sync::Arc;

    let mut d = Document::new();
    let data: Arc<[u8]> = vec![7; 4096].into();
    let preview: Arc<[u8]> = vec![8; 1024].into();
    let weak_data = Arc::downgrade(&data);
    let weak_preview = Arc::downgrade(&preview);
    let picture = Picture {
        data: data.clone(),
        preview: Some(preview.clone()),
        preview_pixels: Some((1, 1)),
        format: ImageFormat::Png,
        pixels: (1, 1),
        natural: (20, 20),
        size: (20, 20),
        crop: Crop::default(),
    };
    drop(data);
    drop(preview);
    let chp = d.main.paras[0].mark_chp;
    let end = d.insert_picture(MAIN, p(0, 0), picture, chp);
    d.commit(Selection::caret(end));
    d.begin("Delete picture", Selection::caret(p(0, 0)));
    d.delete(MAIN, p(0, 0), end);
    d.commit(Selection::caret(p(0, 0)));
    d.reclaim_unreachable_objects();
    assert!(weak_data.upgrade().is_some());
    assert!(weak_preview.upgrade().is_some());

    let mut at = p(0, 0);
    for _ in 0..100 {
        d.begin("Insert", Selection::caret(at));
        at = d.insert_plain(MAIN, at, "x", chp);
        d.commit(Selection::caret(at));
    }

    assert!(weak_data.upgrade().is_none());
    assert!(weak_preview.upgrade().is_none());
    assert_eq!(d.object(1), None);
    assert_eq!(d.objects[0], Object::Vacant);
}

#[test]
fn typing_coalesces_into_one_undo() {
    let mut d = Document::new();
    let mut at = p(0, 0);
    for c in ["a", "b", "c"] {
        at = type_text(&mut d, at, c);
    }
    assert_eq!(texts(&d), ["abc"]);
    d.undo();
    assert_eq!(texts(&d), [""]);
    assert!(d.undo_label().is_none());
}

#[test]
fn moving_the_caret_breaks_typing() {
    let mut d = Document::new();
    let at = type_text(&mut d, p(0, 0), "ab");
    d.break_typing();
    type_text(&mut d, at, "c");
    d.undo();
    assert_eq!(texts(&d), ["ab"]);
}

#[test]
fn multi_paragraph_undo_restores_everything() {
    let mut d = doc_with(&["One", "Two", "Three"]);
    let before = d.main.clone();
    d.begin("Cut", Selection::default());
    d.delete(MAIN, p(0, 1), p(2, 2));
    d.insert_plain(MAIN, p(0, 1), "x\ny\nz", CharProps::default());
    d.commit(Selection::default());
    d.undo();
    assert_eq!(d.main, before);
    d.redo();
    assert_eq!(texts(&d), ["Ox", "y", "zree"]);
}

#[test]
fn modified_tracks_saves_and_undo() {
    let mut d = Document::new();
    assert!(!d.is_modified());
    type_text(&mut d, p(0, 0), "a");
    assert!(d.is_modified());
    d.mark_saved();
    assert!(!d.is_modified());
    d.undo();
    assert!(d.is_modified());
    d.redo();
    assert!(!d.is_modified());
}

#[test]
fn typing_after_saving_is_modified_and_undo_returns_to_the_saved_text() {
    let mut d = Document::new();
    let at = type_text(&mut d, p(0, 0), "Saved");
    d.mark_saved();
    let at = type_text(&mut d, at, " and edited");
    type_text(&mut d, at, " again");
    assert!(d.is_modified());
    assert_eq!(texts(&d), ["Saved and edited again"]);
    d.undo();
    assert_eq!(texts(&d), ["Saved"]);
    assert!(!d.is_modified());
    d.redo();
    assert_eq!(texts(&d), ["Saved and edited again"]);
    assert!(d.is_modified());
}

#[test]
fn document_settings_and_summary_are_undoable() {
    let mut d = Document::new();
    let at = type_text(&mut d, p(0, 0), "x");
    let sel = Selection::caret(at);
    let original_props = d.props.clone();
    let mut props = original_props.clone();
    props.margin_left = 2160;
    assert!(d.set_document_props("Document", props.clone(), sel));
    assert_eq!(d.undo_label(), Some("Document"));

    let summary = SummaryInfo {
        title: "A title".into(),
        ..SummaryInfo::default()
    };
    assert!(d.set_summary(summary.clone(), sel));
    assert_eq!(d.undo_label(), Some("Summary Info"));
    assert!(d.is_modified());

    assert_eq!(d.undo(), Some(sel));
    assert_eq!(d.summary, SummaryInfo::default());
    assert_eq!(d.props, props);
    assert_eq!(d.undo(), Some(sel));
    assert_eq!(d.props, original_props);
    assert_eq!(d.main.paras[0].text, "x", "typing is a separate step");
    assert_eq!(d.redo(), Some(sel));
    assert_eq!(d.props, props);
    assert_eq!(d.redo(), Some(sel));
    assert_eq!(d.summary, summary);
}

#[test]
fn copy_and_paste_within_paragraph() {
    let mut d = doc_with(&["Hello world"]);
    d.format_chars(MAIN, p(0, 6), p(0, 11), |c| c.bold = true);
    let frag = d.copy(MAIN, p(0, 6), p(0, 11));
    assert!(!frag.ends_with_mark);
    let end = d.insert_fragment(MAIN, p(0, 0), &frag);
    assert_eq!(end, p(0, 5));
    assert_eq!(texts(&d), ["worldHello world"]);
    assert!(d.main.paras[0].chp_of_char(0).bold);
    assert!(!d.main.paras[0].chp_of_char(5).bold);
}

#[test]
fn copy_and_paste_paragraphs() {
    let mut d = doc_with(&["Alpha", "Beta", "Gamma"]);
    d.format_paras(MAIN, 1, 1, |pp| pp.align = Align::Center);
    let frag = d.copy(MAIN, p(0, 2), p(1, 2));
    assert_eq!(frag.plain_text(), "pha\nBe");
    let end = d.insert_fragment(MAIN, p(2, 1), &frag);
    assert_eq!(texts(&d), ["Alpha", "Beta", "Gpha", "Beamma"]);
    assert_eq!(end, p(3, 2));
    // The "Be" piece joined Gamma's remainder and keeps Gamma's format.
    assert_eq!(d.main.paras[3].pap.align, Align::Left);
}

#[test]
fn paste_whole_paragraphs_with_mark() {
    let mut d = doc_with(&["One", "Two"]);
    let frag = d.copy(MAIN, p(0, 0), p(1, 0));
    assert!(frag.ends_with_mark);
    let end = d.insert_fragment(MAIN, p(1, 0), &frag);
    assert_eq!(texts(&d), ["One", "One", "Two"]);
    assert_eq!(end, p(2, 0));
}

#[test]
fn set_link_and_remove_it() {
    let mut d = doc_with(&["One two", "three"]);
    let id = crate::link::intern(crate::link::Link::new("https://example.com/"));
    d.set_link(MAIN, p(0, 4), p(1, 2), id, crate::props::Rgb(0x0000EE));
    assert_eq!(d.main.paras[0].chp_of_char(4).link, id);
    assert_eq!(d.main.paras[1].chp_of_char(1).link, id);
    assert_eq!(d.main.paras[1].chp_of_char(2).link, 0);
    assert_eq!(d.main.paras[0].mark_chp.link, 0, "marks are never linked");
    assert_eq!(
        d.main.paras[0].chp_of_char(5).underline,
        crate::props::Underline::Single
    );
    d.set_link(MAIN, p(0, 0), p(1, 5), 0, crate::props::Rgb::BLACK);
    let back = d.main.paras[0].chp_of_char(5);
    assert_eq!(back.link, 0);
    assert_eq!(back.underline, d.main.paras[0].mark_chp.underline);
    assert_eq!(back.color, d.main.paras[0].mark_chp.color);
}

#[test]
fn format_chars_includes_crossed_marks() {
    let mut d = doc_with(&["One", "Two"]);
    d.format_chars(MAIN, p(0, 1), p(1, 1), |c| c.italic = true);
    assert!(d.main.paras[0].mark_chp.italic);
    assert!(!d.main.paras[1].mark_chp.italic);
    assert!(d.main.paras[1].chp_of_char(0).italic);
    assert!(!d.main.paras[1].chp_of_char(1).italic);
}

#[test]
fn apply_style_keeps_hand_formatting() {
    let mut d = doc_with(&["Some words"]);
    d.format_chars(MAIN, p(0, 0), p(0, 4), |c| c.italic = true);
    let h1 = d.styles.find("heading 1").unwrap();
    d.apply_style(MAIN, 0, 0, h1);
    let para = &d.main.paras[0];
    assert!(para.chp_of_char(0).bold, "style's bold applied");
    assert!(para.chp_of_char(0).italic, "hand italic kept");
    assert!(!para.chp_of_char(5).italic);
    assert_eq!(d.font_name(para.chp_of_char(5).font), "Helvetica");
    d.revert_to_style(MAIN, 0, 0);
    assert!(!d.main.paras[0].chp_of_char(0).italic);
}

#[test]
fn revert_to_style_keeps_what_the_text_is() {
    let mut d = doc_with(&["Some words"]);
    d.format_chars(MAIN, p(0, 0), p(0, 4), |c| {
        c.italic = true;
        c.link = 5;
        c.rev = 7;
        c.comment = 9;
    });
    d.format_paras(MAIN, 0, 0, |pp| {
        pp.in_table = true;
        pp.nest = 1;
        pp.align = Align::Right;
    });
    d.revert_to_style(MAIN, 0, 0);
    let para = &d.main.paras[0];
    let first = para.chp_of_char(0);
    // The look goes back to the style's.
    assert!(!first.italic);
    assert_eq!(para.pap.align, Align::Left);
    // The link, the tracked change and the comment stay.
    assert_eq!((first.link, first.rev, first.comment), (5, 7, 9));
    // The paragraph is still in its table.
    assert!(para.pap.in_table);
    assert_eq!(para.pap.nest, 1);
}

#[test]
fn formatting_that_changes_nothing_is_not_an_undo_step() {
    let mut d = doc_with(&["Plain"]);
    d.begin("Formatting", Selection::default());
    d.format_chars(MAIN, p(0, 0), p(0, 5), |c| c.bold = false);
    d.format_paras(MAIN, 0, 0, |pp| pp.align = Align::Left);
    d.commit(Selection::default());
    assert!(d.undo_label().is_none());
    assert!(!d.is_modified());
}

#[test]
fn unchanged_style_actions_preserve_saved_point_and_redo() {
    let mut d = doc_with(&["Plain"]);
    let sel = Selection::default();
    d.take_changes();
    // Repetition must not fill history or evict useful earlier actions.
    for _ in 0..150 {
        d.begin("Style", sel);
        d.apply_style(MAIN, 0, 0, 0);
        d.commit(sel);
        d.begin("Revert To Style", sel);
        d.revert_to_style(MAIN, 0, 0);
        d.commit(sel);
    }
    assert!(!d.is_modified());
    assert_eq!(d.undo_label(), None);
    assert!(d.take_changes().is_empty());
    d.begin("Bold", sel);
    d.format_chars(MAIN, p(0, 0), p(0, 5), |c| c.bold = true);
    d.commit(sel);
    d.undo();
    let redo = d.redo_label();
    assert!(redo.is_some());
    d.begin("Style", sel);
    d.apply_style(MAIN, 0, 0, 0);
    d.revert_to_style(MAIN, 0, 0);
    d.commit(sel);
    assert!(!d.is_modified());
    assert_eq!(d.redo_label(), redo);
    d.redo();
    assert!(d.main.paras[0].chp_at(0).bold);
    d.mark_saved();
    d.begin("Revert To Style", sel);
    d.revert_to_style(MAIN, 0, 0);
    d.commit(sel);
    assert!(!d.main.paras[0].chp_at(0).bold);
    assert!(d.is_modified());
    d.undo();
    assert!(d.main.paras[0].chp_at(0).bold);
    assert!(!d.is_modified());
}

#[test]
fn changes_are_logged() {
    let mut d = doc_with(&["a"]);
    d.take_changes();
    d.split_paragraph(MAIN, p(0, 1));
    assert_eq!(
        d.take_changes(),
        [Change {
            story: MAIN,
            first: 0,
            removed: 1,
            inserted: 2
        }]
    );
    d.undo();
    assert_eq!(
        d.take_changes(),
        [Change {
            story: MAIN,
            first: 0,
            removed: 2,
            inserted: 1
        }]
    );
}

#[test]
fn defining_a_style_can_be_undone() {
    let mut d = doc_with(&["One", "Two"]);
    let n = d.styles.styles.len();
    let pap = d.main.paras[0].pap.clone();
    let chp = d.main.paras[0].chp_at(0);
    d.begin("Define Style", Selection::default());
    let id = d.add_style("Quote", 0, None, pap, chp);
    d.commit(Selection::default());
    assert_eq!(d.styles.get(id).name, "Quote");
    assert_eq!(d.styles.get(id).next, id);
    d.undo();
    assert_eq!(d.styles.styles.len(), n);
    d.redo();
    assert_eq!(d.styles.find("Quote"), Some(id));
}

#[test]
fn redefining_shading_keeps_direct_fills_and_gray_overrides() {
    let mut d = doc_with(&["Style", "Direct color", "Direct gray"]);
    d.main.paras[1].pap.shading_color = Some(crate::Rgb(0x123456));
    d.main.paras[2].pap.shading = 250;
    let mut pap = d.styles.get(0).pap.clone();
    pap.shading_color = Some(crate::Rgb(0xEBF0FA));
    d.begin("Style", Selection::default());
    d.redefine_style(0, pap, d.styles.get(0).chp);
    d.commit(Selection::default());
    assert_eq!(
        d.main.paras[0].pap.shading_color,
        Some(crate::Rgb(0xEBF0FA))
    );
    assert_eq!(
        d.main.paras[1].pap.shading_color,
        Some(crate::Rgb(0x123456))
    );
    assert_eq!(d.main.paras[2].pap.shading_color, None);
    assert_eq!(d.main.paras[2].pap.shading, 250);
    d.undo();
    assert_eq!(d.main.paras[0].pap.shading_color, None);
    d.redo();
    assert_eq!(
        d.main.paras[0].pap.shading_color,
        Some(crate::Rgb(0xEBF0FA))
    );
}

#[test]
fn redefining_follows_paragraphs_and_based_on_styles() {
    let mut d = doc_with(&["Plain", "Head"]);
    let h1 = d.styles.find("heading 1").unwrap();
    d.apply_style(MAIN, 1, 1, h1);
    // Hand formatting: italic on "Pl".
    d.format_chars(MAIN, p(0, 0), p(0, 2), |c| c.italic = true);
    d.commit(Selection::default());
    let mut pap = d.styles.get(0).pap.clone();
    pap.left = 720;
    let mut chp = d.styles.get(0).chp;
    chp.italic = false;
    chp.size = 20;
    d.begin("Style", Selection::default());
    d.redefine_style(0, pap, chp);
    d.commit(Selection::default());
    let para = &d.main.paras[0];
    assert_eq!(para.pap.left, 720);
    assert_eq!(para.chp_at(3).size, 20);
    assert!(para.chp_at(1).italic, "hand formatting stays");
    // heading 1 sets its own font, so it keeps it, but takes the
    // indent and the size it shared with Normal.
    let h = d.styles.get(h1);
    assert_eq!(h.pap.left, 720);
    assert_eq!(h.chp.size, 20);
    assert_eq!(d.font_name(h.chp.font), "Helvetica");
    assert_eq!(d.main.paras[1].pap.left, 720);
    d.undo();
    assert_eq!(d.main.paras[0].pap.left, 0);
    assert_eq!(d.styles.get(0).pap.left, 0);
}

#[test]
fn deleting_a_style_changes_every_story_and_undo_restores_them() {
    let mut d = doc_with(&["A", "B"]);
    let sel = Selection::default();
    let pap = d.main.paras[0].pap.clone();
    let chp = d.main.paras[0].chp_at(0);
    d.begin("Style", sel);
    let a = d.add_style("Alpha", 0, None, pap.clone(), chp);
    let b = d.add_style("Beta", a, None, pap, chp);
    d.commit(sel);
    // The first section's header uses Beta and its footer Alpha; the
    // notes use Alpha; a second section has headers of its own with
    // both.
    d.headers[StoryId::Header.header_index()].paras[0].pap.style = b;
    d.headers[StoryId::Footer.header_index()].paras[0].pap.style = a;
    d.notes.paras[0].pap.style = a;
    d.begin("Break", sel);
    d.insert_section_break(Position::new(0, 1));
    d.commit(sel);
    d.begin("Own headers", sel);
    let mut set = vec![Story::new(d.main.paras[0].clone()); 6];
    set[0].paras[0].pap.style = b;
    set[1].paras[0].pap.style = a;
    d.format_sections(1, 1, |s| s.headers = Some(std::sync::Arc::new(set.clone())));
    d.commit(sel);
    let styles = |d: &Document| {
        let own = d.headers_of(1);
        (
            d.headers[0].paras[0].pap.style,
            d.headers[1].paras[0].pap.style,
            d.notes.paras[0].pap.style,
            own[0].paras[0].pap.style,
            own[1].paras[0].pap.style,
        )
    };
    assert_eq!(styles(&d), (b, a, a, b, a));
    d.begin("Delete Style", sel);
    d.delete_style(a);
    d.commit(sel);
    // Beta moved down to take Alpha's number; Alpha's users are Normal.
    let beta = d.styles.find("Beta").unwrap();
    assert_eq!(beta, b - 1);
    assert_eq!(styles(&d), (beta, 0, 0, beta, 0));
    d.undo();
    assert_eq!(styles(&d), (b, a, a, b, a));
}

#[test]
fn replacing_fonts_reaches_every_sections_headers_and_undo_takes_it_back() {
    let mut d = doc_with(&["A", "B"]);
    let sel = Selection::default();
    let (from, to) = (d.font_id("Zed"), d.font_id("Ink"));
    let chp = CharProps {
        font: from,
        ..d.main.paras[0].chp_at(0)
    };
    let pap = d.main.paras[0].pap.clone();
    let header = Story::new(Paragraph::with_text("Head", chp, pap));
    d.headers[StoryId::Header.header_index()] = header.clone();
    d.begin("Break", sel);
    d.insert_section_break(Position::new(0, 1));
    d.commit(sel);
    d.begin("Own headers", sel);
    d.format_sections(1, 1, |s| {
        s.headers = Some(std::sync::Arc::new(vec![header.clone(); 6]));
    });
    d.commit(sel);
    let fonts = |d: &Document| {
        (
            d.headers[0].paras[0].chp_of_char(0).font,
            d.headers_of(1)[0].paras[0].chp_of_char(0).font,
        )
    };
    assert_eq!(fonts(&d), (from, from));
    d.begin("Replace Fonts", sel);
    d.replace_font(&[from], to);
    d.commit(sel);
    assert_eq!(fonts(&d), (to, to));
    d.undo();
    assert_eq!(fonts(&d), (from, from));
}

#[test]
fn deleting_a_style_returns_its_paragraphs_to_normal() {
    let mut d = doc_with(&["A", "B", "C"]);
    let pap = d.main.paras[0].pap.clone();
    let mut chp = d.main.paras[0].chp_at(0);
    chp.bold = true;
    d.begin("Style", Selection::default());
    let a = d.add_style("Alpha", 0, None, pap.clone(), chp);
    let b = d.add_style("Beta", a, None, pap, chp);
    d.apply_style(MAIN, 0, 0, a);
    d.apply_style(MAIN, 2, 2, b);
    d.commit(Selection::default());
    d.begin("Delete Style", Selection::default());
    d.delete_style(a);
    d.commit(Selection::default());
    assert_eq!(d.main.paras[0].pap.style, 0);
    assert!(!d.main.paras[0].chp_at(0).bold);
    let beta = d.styles.find("Beta").unwrap();
    assert_eq!(beta, b - 1);
    assert_eq!(d.main.paras[2].pap.style, beta);
    assert_eq!(d.styles.get(beta).based_on, Some(0));
    assert_eq!(d.styles.get(beta).pap.style, beta);
    d.undo();
    assert_eq!(d.styles.find("Alpha"), Some(a));
    assert_eq!(d.main.paras[0].pap.style, a);
    assert_eq!(d.main.paras[2].pap.style, b);
}

#[test]
fn deleting_a_style_can_give_its_paragraphs_another_style() {
    let mut d = doc_with(&["A", "B", "C"]);
    let pap = d.main.paras[0].pap.clone();
    let mut chp = d.main.paras[0].chp_at(0);
    d.begin("Style", Selection::default());
    let a = d.add_style("Alpha", 0, None, pap.clone(), chp);
    chp.italic = true;
    let b = d.add_style("Beta", 0, None, pap, chp);
    d.apply_style(MAIN, 0, 1, a);
    d.commit(Selection::default());
    assert_eq!(d.style_uses(a), 2);
    assert_eq!(d.style_uses(b), 0);
    // The empty note and comment stories are not counted as Normal.
    assert_eq!(d.style_uses(0), 1);
    d.begin("Delete Style", Selection::default());
    d.delete_style_replacing(a, b);
    d.commit(Selection::default());
    // Beta moved down a number, and both paragraphs follow it there.
    let beta = d.styles.find("Beta").unwrap();
    assert_eq!(beta, b - 1);
    assert_eq!(d.main.paras[0].pap.style, beta);
    assert_eq!(d.main.paras[1].pap.style, beta);
    assert!(
        d.main.paras[0].chp_at(0).italic,
        "Beta's formatting applied"
    );
    assert_eq!(d.style_uses(beta), 2);
    d.undo();
    assert_eq!(d.main.paras[0].pap.style, a);
    assert_eq!(d.style_uses(a), 2);
}

#[test]
fn importing_a_story_maps_fonts_by_name() {
    let mut d = doc_with(&["Body"]);
    let mut other = d.clone();
    let geneva = other.font_id("Geneva");
    let mut header = other.story(StoryId::Header).clone();
    header.paras[0].text = "Head".into();
    header.paras[0].runs = vec![crate::para::Run {
        len: 4,
        chp: CharProps {
            font: geneva,
            ..d.main.paras[0].chp_at(0)
        },
    }];
    // The real document meanwhile gained a different font.
    d.font_id("Monaco");
    d.import_story(StoryId::Header, &header, &other.fonts, &other.objects);
    let p = &d.story(StoryId::Header).paras[0];
    assert_eq!(p.text, "Head");
    assert_eq!(d.font_name(p.chp_at(0).font), "Geneva");
    assert!(d.is_modified());
    assert!(!d.story_is_empty(StoryId::Header));
    assert!(d.story_is_empty(StoryId::Footer));
}

#[test]
fn sections_have_their_own_headers_or_share() {
    let mut d = crate::text::from_plain_text("One\nTwo");
    let chp = d.main.paras[0].mark_chp;
    d.insert_text(StoryId::Header, Position::new(0, 0), "First", chp);
    d.forget_history();
    // A new section shares the one before's headers.
    d.begin("Break", Selection::default());
    d.insert_section_break(Position::new(1, 0));
    d.commit(Selection::default());
    assert!(!d.has_own_headers(1));
    assert_eq!(d.header_story(1, StoryId::Header).paras[0].text, "First");
    // Typing into its header gives it its own; the first is untouched.
    d.header_section = 1;
    d.begin("Typing", Selection::default());
    d.insert_text(StoryId::Header, Position::new(0, 0), "Second ", chp);
    d.commit(Selection::default());
    assert!(d.has_own_headers(1));
    assert_eq!(
        d.header_story(1, StoryId::Header).paras[0].text,
        "Second First"
    );
    assert_eq!(d.header_story(0, StoryId::Header).paras[0].text, "First");
    // Undo puts the second section's text back, wherever the pane is.
    d.header_section = 0;
    d.undo();
    assert_eq!(d.header_story(1, StoryId::Header).paras[0].text, "First");
    assert_eq!(d.header_story(0, StoryId::Header).paras[0].text, "First");
    d.redo();
    // Same As Previous shares the first section's again, undoably.
    d.begin("Same As Previous", Selection::default());
    d.link_headers(1);
    d.commit(Selection::default());
    assert!(!d.has_own_headers(1));
    assert_eq!(d.header_story(1, StoryId::Header).paras[0].text, "First");
    d.undo();
    assert_eq!(
        d.header_story(1, StoryId::Header).paras[0].text,
        "Second First"
    );
}

#[test]
fn importing_a_story_keeps_the_ids_on_comment_marks() {
    let mut other = Document::new();
    let mut doc = Document::new();
    let id = other.insert_annotation(
        Position::new(0, 0),
        other.main.paras[0].mark_chp,
        crate::notes::AnnotationInfo::default(),
    );
    // A picture list the id could be mistaken for a place in: the
    // other document's first picture is this one's second.
    let png = |w: u8| {
        let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0".to_vec();
        v.extend_from_slice(&[w, 0, 0, 0, 1, 8, 2, 0, 0, 0]);
        crate::Picture::from_bytes(v).unwrap()
    };
    other.add_object(Object::Picture(png(3)));
    doc.add_object(Object::Picture(png(2)));
    doc.add_object(Object::Picture(png(3)));
    let mark_story = Story {
        paras: vec![other.main.paras[0].clone()],
    };
    doc.import_story(
        StoryId::Annotations,
        &mark_story,
        &other.fonts,
        &other.objects,
    );
    let p = &doc.annotations.paras[0];
    assert!(p.text.starts_with(chars::ANNOTATION));
    assert_eq!(p.chp_of_char(0).object, id);
}

#[test]
fn style_changes_reach_every_sections_headers_once() {
    let mut d = crate::text::from_plain_text("A\nB");
    let chp = d.main.paras[0].mark_chp;
    d.insert_text(StoryId::Header, Position::new(0, 0), "Zero", chp);
    let normal = d.styles.get(0).clone();
    let a = d.add_style("A", 0, None, normal.pap.clone(), normal.chp);
    let b = d.add_style("B", 0, None, normal.pap.clone(), normal.chp);
    d.apply_style(StoryId::Header, 0, 0, b);
    d.forget_history();
    d.begin("Break", Selection::default());
    d.insert_section_break(Position::new(1, 0));
    d.commit(Selection::default());
    // The second section gets a header of its own, and the pane is
    // left showing it, as after editing it.
    d.header_section = 1;
    d.begin("Typing", Selection::default());
    d.insert_text(StoryId::Header, Position::new(0, 0), "One ", chp);
    d.commit(Selection::default());
    assert!(d.has_own_headers(1));
    let style_of = |d: &Document, k: usize| {
        let s = d.header_story(k, StoryId::Header).paras[0].pap.style;
        d.styles.get(s).name.clone()
    };
    assert_eq!((style_of(&d, 0), style_of(&d, 1)), ("B".into(), "B".into()));
    // Deleting a style before B moves B down by one, in both headers,
    // once each.
    d.begin("Delete Style", Selection::default());
    d.delete_style(a);
    d.commit(Selection::default());
    assert_eq!(d.header_section, 1);
    assert_eq!((style_of(&d, 0), style_of(&d, 1)), ("B".into(), "B".into()));
    d.undo();
    assert_eq!((style_of(&d, 0), style_of(&d, 1)), ("B".into(), "B".into()));
    // Redefining B reaches both headers, as one step.
    let b = d.styles.find("B").unwrap();
    let mut bigger = normal.chp;
    bigger.size = 40;
    d.begin("Define Style", Selection::default());
    d.redefine_style(b, normal.pap.clone(), bigger);
    d.commit(Selection::default());
    for k in 0..2 {
        let p = &d.header_story(k, StoryId::Header).paras[0];
        assert_eq!(p.chp_of_char(0).size, 40, "section {k}");
    }
    d.undo();
    for k in 0..2 {
        let p = &d.header_story(k, StoryId::Header).paras[0];
        assert_eq!(p.chp_of_char(0).size, normal.chp.size, "section {k}");
    }
    assert_eq!(d.header_section, 1);
}

#[test]
fn header_lookups_agree_for_every_section() {
    let mut d = crate::text::from_plain_text("A\nB\nC\nD\nE");
    let chp = d.main.paras[0].mark_chp;
    d.insert_text(StoryId::Header, Position::new(0, 0), "Zero", chp);
    d.forget_history();
    for para in [3, 2, 1] {
        d.begin("Break", Selection::default());
        d.insert_section_break(Position::new(para, 0));
        d.commit(Selection::default());
    }
    // Sections 1 and 3 get headers of their own; 2 shares the one before's.
    for (k, text) in [(1, "One "), (3, "Three ")] {
        d.header_section = k;
        d.begin("Typing", Selection::default());
        d.insert_text(StoryId::Header, Position::new(0, 0), text, chp);
        d.commit(Selection::default());
    }
    let ranges = d.sections();
    assert_eq!(ranges.len(), 4);
    let sets = d.header_sets();
    assert_eq!(sets.len(), ranges.len());
    for k in 0..ranges.len() + 2 {
        // The slow way: walk down from section k.
        let mut want = k.min(ranges.len() - 1);
        while want > 0 && d.nth_section(want).headers.is_none() {
            want -= 1;
        }
        assert_eq!(d.header_source(k), want, "source of {k}");
        if let Some((source, set)) = sets.get(k) {
            assert_eq!(*source, want, "set source of {k}");
            assert_eq!(set[0].paras[0].text, d.headers_of(k)[0].paras[0].text);
        }
    }
    assert_eq!(d.header_story(2, StoryId::Header).paras[0].text, "One Zero");
    assert_eq!(
        d.header_story(3, StoryId::Header).paras[0].text,
        "Three One Zero"
    );
    for (k, r) in ranges.iter().enumerate() {
        let last = k + 1 == ranges.len();
        let want = if last {
            &d.section
        } else {
            d.main.paras[*r.end()]
                .section
                .as_deref()
                .unwrap_or(&d.section)
        };
        assert!(std::ptr::eq(d.nth_section(k), want), "section {k}");
    }
}

#[test]
fn importing_a_story_brings_its_pictures() {
    let mut d = Document::new();
    // The header pane's copy, where a picture goes into the header.
    let mut pane = d.clone();
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x02\0\0\0\x01\x08\x02\0\0\0".to_vec();
    let pic = crate::Picture::from_bytes(png).unwrap();
    let chp = pane.main.paras[0].mark_chp;
    pane.main = pane.story(StoryId::Header).clone();
    pane.insert_picture(StoryId::Main, Position::new(0, 0), pic.clone(), chp);
    // The real document gains a picture of its own meanwhile.
    let other = crate::Picture {
        size: (100, 100),
        ..pic.clone()
    };
    d.insert_picture(StoryId::Main, Position::new(0, 0), other, chp);
    d.import_story(StoryId::Header, &pane.main, &pane.fonts, &pane.objects);
    let id = d.story(StoryId::Header).paras[0].chp_of_char(0).object;
    assert_eq!(d.picture(id), Some(&pic));
    // Importing again does not add it twice.
    let n = d.objects.len();
    d.import_story(StoryId::Header, &pane.main, &pane.fonts, &pane.objects);
    assert_eq!(d.objects.len(), n);
}

#[test]
fn pane_copy_contains_only_the_chosen_story_and_no_history() {
    let mut d = Document::new();
    type_text(&mut d, Position::new(0, 0), "A long body");
    let chp = d.main.paras[0].mark_chp;
    d.story_mut(StoryId::Header).paras[0].insert(0, "Header", chp);

    let pane = d.copy_story_for_edit(StoryId::Header);
    assert_eq!(pane.main.paras[0].text, "Header");
    assert!(!pane.main.paras[0].text.contains("long body"));
    assert_eq!(pane.undo_label(), None);
    assert!(!pane.is_modified());
}
