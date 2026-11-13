use super::*;

#[test]
fn redo_has_its_own_command_and_repeat_is_only_repeat() {
    let mut clip = Clipboard::new();
    let mut view = DocView::new(Document::new(), "t".into());
    view.type_text("hello");
    view.doc.break_typing();
    view.exec(Cmd::Undo, false, &mut clip);
    assert_eq!(view.doc.main.paras[0].text, "");
    assert!(view.can_redo());
    assert!(view.redo_label().starts_with("Redo"));
    view.exec(Cmd::Redo, false, &mut clip);
    assert_eq!(view.doc.main.paras[0].text, "hello");
    assert!(!view.can_redo());
    assert_eq!(view.redo_label(), "Can't Redo");
    // Repeat is only Repeat: its label never says Redo.
    view.exec(Cmd::Undo, false, &mut clip);
    assert!(!view.repeat_label().starts_with("Redo"));
}

#[test]
fn finds_tables_wider_than_their_column() {
    let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
        return;
    };
    // Two columns of about 2.9 in on a 6 in text width, as in the
    // report: four 1.5 in cells run over the second column; two
    // 1.25 in cells fit.
    for (cols, width, expected) in [(4, 2160, vec![1]), (2, 1800, vec![])] {
        let mut v = DocView::new(Document::new(), "t".into());
        v.type_text("Before");
        v.doc.section.columns = 2;
        v.doc.section.column_spacing = 360;
        v.doc
            .insert_table(MAIN, Position::new(0, 0), 2, cols, width);
        v.update_layout(&mut fonts);
        assert_eq!(v.wide_table_pages(), expected, "{cols} cells of {width}");
    }
    // In a single column the same wide table is left alone.
    let mut v = DocView::new(Document::new(), "t".into());
    v.doc.insert_table(MAIN, Position::new(0, 0), 2, 4, 2160);
    v.update_layout(&mut fonts);
    assert!(v.wide_table_pages().is_empty());
}

#[test]
fn secondary_click_preserves_backward_selection_and_moves_only_when_outside() {
    let mut view = DocView::new(Document::new(), "Context".into());
    view.type_text("one two three");
    let selected = Selection::new(Position::new(0, 7), Position::new(0, 4));
    view.sel = selected;
    view.context_at(Position::new(0, 5), false);
    assert_eq!(view.sel, selected);
    view.context_at(Position::new(0, 11), true);
    assert_eq!(view.sel, Selection::caret(Position::new(0, 11)));
    assert!(view.upstream);
    assert_eq!(view.doc.main.paras[0].text, "one two three");
    assert!(view.last_click.is_none());
    assert!(view.drag.is_none());
}

#[test]
fn hostile_text_and_extreme_settings_survive_layout_editing_and_saving() {
    let mut fonts = quill_layout::Fonts::new(quill_layout::Catalog::scan_system());
    if fonts.family_names().is_empty() {
        return;
    }
    let markers = "a\u{E000}b\u{E001}c\u{E002}d\u{E003}e\u{E004}f\u{FFFC}g\u{E0FF}\u{0}\u{7}";
    for case in 0..7 {
        let mut doc = quill_core::text::from_plain_text(&format!("x{markers}\nsecond {markers}"));
        let p = &mut doc.main.paras[0].pap;
        match case {
            0 => {}
            1 => {
                doc.props.margin_left = i32::MAX;
                doc.props.margin_right = i32::MAX;
            }
            2 => {
                p.left = i32::MAX;
                p.first = i32::MIN;
                p.right = i32::MAX;
            }
            3 => {
                p.before = i32::MAX;
                p.after = i32::MAX;
                p.line = quill_core::props::LineSpacing::Exactly(i32::MAX);
            }
            6 => {
                // 11 inches wide with 6-inch margins either side.
                doc.props.page_width = 15840;
                doc.props.margin_left = 8640;
                doc.props.margin_right = 8640;
            }
            4 => {
                doc.props.page_width = 1;
                doc.props.page_height = 1;
                doc.props.margin_top = i32::MAX;
            }
            _ => {
                for r in &mut doc.main.paras[0].runs {
                    r.chp.size = u16::MAX;
                    r.chp.position = i32::MAX;
                    r.chp.spacing = i32::MIN;
                }
            }
        }
        // As every reader does with what it reads.
        doc.keep_in_bounds();
        let mut view = DocView::new(doc, "Hostile".into());
        view.area = Rect::new(0.0, 0.0, 400.0, 300.0);
        for page_view in [false, true] {
            view.page_view = page_view;
            view.invalidate_layout();
            view.update_layout(&mut fonts);
            let mut px = vec![0u32; 400 * 300];
            let cache = &mut GlyphCache::default();
            view.draw(
                &mut Canvas::new(&mut px, 400, 300, 1.0),
                &fonts,
                cache,
                true,
            );
        }
        view.type_text(markers);
        let _ = view.doc.note_labels(None);
        for format in [
            quill_formats::Format::Quill,
            quill_formats::Format::Docx,
            quill_formats::Format::Rtf,
            quill_formats::Format::Odt,
            quill_formats::Format::Html,
            quill_formats::Format::Markdown,
            quill_formats::Format::Text,
        ] {
            let path = std::env::temp_dir().join(format!("quill-hostile-{case}.{format:?}"));
            quill_formats::save(&view.doc, &path, format).unwrap();
            let _ = quill_formats::load(&path);
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[test]
fn pastes_that_would_make_the_document_too_large_are_refused() {
    let mut view = DocView::new(Document::new(), "Paste".into());
    let huge = quill_core::limits::Size {
        paragraphs: quill_core::limits::MAX_PARAGRAPHS,
        ..Default::default()
    };
    assert!(!view.room_for(huge));
    assert!(
        view.status
            .as_deref()
            .is_some_and(|s| s.contains("too large"))
    );
    assert!(view.room_for(quill_core::limits::Size::of_text("a few words")));
}

#[test]
fn smart_quotes_open_and_close() {
    assert_eq!(
        editing::smart_quotes("\"Hi,\" she said. 'Don't'", None),
        "\u{201C}Hi,\u{201D} she said. \u{2018}Don\u{2019}t\u{2019}"
    );
    // Typed one character at a time, after a word.
    assert_eq!(editing::smart_quotes("'", Some('n')), "\u{2019}");
    assert_eq!(editing::smart_quotes("\"", Some('(')), "\u{201C}");
}

#[test]
fn page_break_rule_stays_on_the_line_that_ends_at_the_break() {
    let mut fonts = quill_layout::Fonts::new(quill_layout::Catalog::scan_system());
    if fonts.family_names().is_empty() {
        return;
    }
    let mut doc = Document::new();
    let at = doc.insert_section_break(Position::default());
    let chp = doc.main.paras[at.para].mark_chp;
    doc.insert_text(MAIN, at, "\u{000C}After", chp);
    let mut view = DocView::new(doc, "Breaks".into());
    view.update_layout(&mut fonts);
    let l = view.layouts[at.para].as_ref().unwrap();
    assert_eq!(l.lines.len(), 2);
    assert!(l.lines[0].hard_break);
    assert!(!l.lines[1].hard_break);

    let ys: Vec<_> = drawing::manual_page_break_rule_ys(l, 10.0).collect();
    assert_eq!(ys, [10.0 + l.lines[0].top + l.lines[0].height / 2.0]);
}

#[test]
fn a_selected_page_break_is_highlighted_only_across_its_own_column() {
    let mut fonts = quill_layout::Fonts::new(quill_layout::Catalog::scan_system());
    if fonts.family_names().is_empty() {
        return;
    }
    let mut view = DocView::new(Document::new(), "Breaks".into());
    view.type_text("HelloAfter");
    view.sel = Selection::caret(Position::new(0, 5));
    view.exec(Cmd::PageBreak, false, &mut Clipboard::new());
    view.update_layout(&mut fonts);
    // As if the paragraph were in a narrow column or table cell.
    view.widths[0] = 100.0;
    let l = view.layouts[0].as_ref().unwrap();
    let line = l.lines.iter().find(|x| x.hard_break).unwrap();
    let y = (line.top + line.height / 2.0) as usize;
    let mut px = vec![0u32; 400 * 100];
    view.draw_selection(
        &mut Canvas::new(&mut px, 400, 100, 1.0),
        0,
        l,
        0.0,
        0.0,
        Position::new(0, 5),
        Position::new(0, 6),
        true,
    );
    let at = |x: usize| px[y * 400 + x];
    assert_eq!(at(60), color::HIGHLIGHT);
    // Past the column's right edge nothing is drawn.
    assert_eq!(at(150), 0);
    assert_eq!(at(300), 0);
}

#[test]
fn tall_headers_push_at_least_margins_but_not_exact_ones() {
    let mut fonts = quill_layout::Fonts::new(quill_layout::Catalog::scan_system());
    if fonts.family_names().is_empty() {
        return;
    }
    let mut doc = Document::new();
    doc.props.page_height = 3_600;
    doc.props.margin_top = 400;
    doc.props.margin_bottom = 400;
    doc.section.header_from_top = 100;
    let chp = doc.main.paras[0].mark_chp;
    doc.insert_plain(
        StoryId::Header,
        Position::default(),
        "Header one\nHeader two\nHeader three\nHeader four\n",
        chp,
    );
    doc.insert_plain(
        StoryId::Footer,
        Position::default(),
        "Footer one\nFooter two\nFooter three\nFooter four\n",
        chp,
    );
    doc.insert_plain(
        MAIN,
        Position::default(),
        &(0..30).map(|_| "Body line\n").collect::<String>(),
        chp,
    );
    doc.forget_history();

    let mut at_least = DocView::new(doc.clone(), "At Least".into());
    at_least.update_layout(&mut fonts);
    assert!(at_least.page_body_extra[0].0 > 0.0);
    assert!(at_least.page_body_extra[0].1 > 0.0);

    doc.props.top_exact = true;
    doc.props.bottom_exact = true;
    let mut exact = DocView::new(doc, "Exact".into());
    exact.update_layout(&mut fonts);
    assert_eq!(exact.page_body_extra[0], (0.0, 0.0));
    assert!(at_least.pages.len() > exact.pages.len());
}
#[test]
fn sed_edits_the_document_keeps_formats_and_undoes_in_one_step() {
    let mut clip = Clipboard::new();
    let paras =
        |v: &DocView| -> Vec<String> { v.doc.main.paras.iter().map(|p| p.text.clone()).collect() };
    let mut view = DocView::new(Document::new(), "t".into());
    view.type_text("plain ");
    view.exec(Cmd::Bold, false, &mut clip);
    view.type_text("cat cat");
    view.exec(Cmd::Bold, false, &mut clip);
    view.exec(Cmd::NewParagraph, false, &mut clip);
    view.type_text("a cat, a dog");
    let before = paras(&view);
    assert_eq!(before, ["plain cat cat", "a cat, a dog"]);

    let sed = crate::advanced::Sed::parse("s/cat/bird/g; s/, /,\\n/").unwrap();
    assert_eq!(view.sed(&sed, false), Ok(4));
    assert_eq!(paras(&view), ["plain bird bird", "a bird,", "a dog"]);
    // The new words are bold where the old ones were.
    let p = &view.doc.main.paras[0];
    assert!(!p.chp_of_char(0).bold);
    assert!(p.chp_of_char(6).bold && p.chp_of_char(11).bold);
    assert_eq!(view.status.as_deref(), Some("4 Changes"));

    // One Undo puts it all back.
    view.exec(Cmd::Undo, false, &mut clip);
    assert_eq!(paras(&view), before);
    // Nothing to change is not an undo step.
    let none = crate::advanced::Sed::parse("s/zebra/x/").unwrap();
    assert_eq!(view.sed(&none, false), Ok(0));
    assert_eq!(view.status.as_deref(), Some("No Changes"));
    assert_eq!(paras(&view), before);
}

#[test]
fn sed_on_the_selection_changes_only_the_selection() {
    let mut clip = Clipboard::new();
    let mut view = DocView::new(Document::new(), "t".into());
    view.type_text("cat cat cat");
    // Select the middle word and the space after it: "cat ".
    view.sel = Selection::new(Position::new(0, 4), Position::new(0, 8));
    let sed = crate::advanced::Sed::parse("s/cat/dog/g; s/dog/ox/").unwrap();
    assert_eq!(view.sed(&sed, true), Ok(2));
    // The second command still sees the same selected text, though the
    // first changed its length.
    assert_eq!(view.doc.main.paras[0].text, "cat ox cat");
    view.exec(Cmd::Undo, false, &mut clip);
    assert_eq!(view.doc.main.paras[0].text, "cat cat cat");
}

#[test]
fn regex_search_finds_replaces_and_wraps() {
    let mut clip = Clipboard::new();
    let mut view = DocView::new(Document::new(), "t".into());
    view.type_text("cat 12 dog 345");
    view.exec(Cmd::NewParagraph, false, &mut clip);
    view.type_text("bird 6");
    view.set_selection(Selection::caret(Position::new(0, 0)));
    let mut d = crate::advanced_dialog::regex_search(&view, false);
    assert!(
        !d.items
            .iter()
            .any(|i| matches!(i.id, "with" | "replace" | "replaceall"))
    );
    d.set_field("pattern", "([a-z]+) (\\d+)");
    crate::advanced_dialog::rx_switch(&mut d, &view, true);
    assert_eq!(d.field_text("pattern"), "([a-z]+) (\\d+)");
    d.set_field("pattern", "([a-z]+) (\\d+)");
    d.set_field("with", "\\2 \\1");
    crate::advanced_dialog::rx_check(&mut d, true);
    assert!(d.is_enabled("findnext"));

    // Find Next walks the matches, then wraps.
    assert!(crate::advanced_dialog::rx_find_next(
        &mut d, &mut view, true
    ));
    assert_eq!(view.sel.range(), (Position::new(0, 0), Position::new(0, 6)));
    assert!(crate::advanced_dialog::rx_find_next(
        &mut d, &mut view, true
    ));
    assert_eq!(
        view.sel.range(),
        (Position::new(0, 7), Position::new(0, 14))
    );
    assert!(crate::advanced_dialog::rx_find_next(
        &mut d, &mut view, true
    ));
    assert_eq!(view.sel.start(), Position::new(1, 0));
    assert!(crate::advanced_dialog::rx_find_next(
        &mut d, &mut view, true
    ));
    assert_eq!(view.sel.start(), Position::new(0, 0));

    // Replace changes the selected match and moves to the next.
    crate::advanced_dialog::rx_replace(&mut d, &mut view);
    assert_eq!(view.doc.main.paras[0].text, "12 cat dog 345");
    assert_eq!(view.sel.start(), Position::new(0, 7));
    // Replace All is one Undo step.
    crate::advanced_dialog::rx_replace_all(&mut d, &mut view);
    assert_eq!(view.doc.main.paras[0].text, "12 cat 345 dog");
    assert_eq!(view.doc.main.paras[1].text, "6 bird");
    view.exec(Cmd::Undo, false, &mut clip);
    assert_eq!(view.doc.main.paras[0].text, "12 cat dog 345");
    assert_eq!(view.doc.main.paras[1].text, "bird 6");

    // A bad pattern turns the buttons off and says why.
    d.set_field("pattern", "(");
    crate::advanced_dialog::rx_check(&mut d, true);
    assert!(!d.is_enabled("findnext") && !d.is_enabled("replaceall"));
}

#[test]
fn sed_refuses_a_script_that_would_make_the_document_too_large() {
    use quill_core::limits::MAX_CHARACTERS;
    let mut clip = Clipboard::new();
    let mut view = DocView::new(Document::new(), "t".into());
    // Ten thousand letters, and a replacement of 3,900 for each.
    view.type_text(&"a".repeat(10_000));
    let before = view.doc.main.paras[0].text.clone();
    const { assert!(10_000 * 3_900 > MAX_CHARACTERS) };
    let script = format!("s/a/{}/g", "b".repeat(3_900));
    let sed = crate::advanced::Sed::parse(&script).unwrap();
    let err = view.sed(&sed, false).unwrap_err();
    assert!(err.contains("too large"), "{err}");
    assert!(view.doc.main.paras[0].text == before);
    assert!(view.doc.size().too_large().is_none());
    // Within the limit it runs, and a script whose later command is too
    // much changes nothing at all, the earlier commands included.
    let ok = crate::advanced::Sed::parse("s/a/bb/g").unwrap();
    assert_eq!(view.sed(&ok, false), Ok(10_000));
    view.exec(Cmd::Undo, false, &mut clip);
    assert!(view.doc.main.paras[0].text == before);
    let two = format!("s/a/b/g; s/b/{}/g", "c".repeat(3_900));
    let sed = crate::advanced::Sed::parse(&two).unwrap();
    assert!(view.sed(&sed, false).is_err());
    assert!(view.doc.main.paras[0].text == before);
}

#[test]
fn sed_counts_the_text_a_tracked_replacement_keeps() {
    use quill_core::limits::MAX_CHARACTERS;
    // Half the limit and a bit: replacing it all fits, but not with the
    // old text kept beside the new, struck through.
    let n = MAX_CHARACTERS / 2 + 1_000;
    let mut doc = Document::new();
    let chp = doc.main.paras[0].mark_chp;
    let pap = doc.main.paras[0].pap.clone();
    doc.main.paras = vec![Paragraph::with_text(&"a".repeat(n), chp, pap)];
    doc.forget_history();
    let mut view = DocView::new(doc, "t".into());
    view.doc.props.track_changes = true;
    let sed = crate::advanced::Sed::parse("s/a+/X&/").unwrap();
    let err = view.sed(&sed, false).unwrap_err();
    assert!(err.contains("too large"), "{err}");
    assert_eq!(view.doc.main.paras[0].text.len(), n);
    assert!(view.doc.size().too_large().is_none());
    assert!(view.doc.undo_label().is_none());
}

#[test]
fn a_refused_script_leaves_undo_and_redo_as_they_were() {
    use quill_core::limits::MAX_CHARACTERS;
    let mut clip = Clipboard::new();
    let mut view = DocView::new(Document::new(), "t".into());
    view.type_text(&"a".repeat(10_000));
    view.doc.break_typing();
    view.type_text("z");
    view.exec(Cmd::Undo, false, &mut clip);
    let (undo, redo) = (view.doc.undo_label(), view.doc.redo_label());
    assert!(redo.is_some());
    let before = view.doc.main.paras[0].text.clone();
    // The first command runs, the second is too much.
    const { assert!(10_000 * 3_900 > MAX_CHARACTERS) };
    let two = format!("s/a/b/g; s/b/{}/g", "c".repeat(3_900));
    let sed = crate::advanced::Sed::parse(&two).unwrap();
    assert!(view.sed(&sed, false).is_err());
    assert_eq!(view.doc.main.paras[0].text, before);
    assert_eq!((view.doc.undo_label(), view.doc.redo_label()), (undo, redo));
    // Redo still brings back what was undone.
    view.doc.redo();
    assert!(view.doc.main.paras[0].text.ends_with('z'));
}

#[test]
fn sed_leaves_tracked_deleted_text_alone() {
    let mut view = DocView::new(Document::new(), "t".into());
    view.type_text("one cat two");
    let sel = view.sel;
    view.doc.props.track_changes = true;
    view.doc.begin("Delete", sel);
    view.doc
        .delete(MAIN, Position::new(0, 4), Position::new(0, 7));
    view.doc.commit(sel);
    view.doc.props.track_changes = false;
    // The word is still in the text, marked as deleted.
    assert_eq!(view.doc.main.paras[0].text, "one cat two");
    assert_eq!(view.search_lines()[0].deleted, [(4, 7)]);
    let sed = crate::advanced::Sed::parse("s/cat/dog/").unwrap();
    assert_eq!(view.sed(&sed, false), Ok(0));
    assert_eq!(view.doc.main.paras[0].text, "one cat two");
}

#[test]
fn stale_grep_positions_never_split_a_character() {
    let mut view = DocView::new(Document::new(), "t".into());
    view.type_text("caf\u{e9} cat");
    let (mut d, mut st) = crate::advanced_dialog::grep(&view);
    d.set_field("pattern", "cat");
    crate::advanced_dialog::grep_search(&mut st, &mut d, &view);
    d.set_selected("hits", 0);
    // The text changes under the list: the match now points into the
    // middle of the accent, and then past the end of the paragraph.
    for (start, end) in [(4, 5), (500, 600)] {
        st.hits[0].start = start;
        st.hits[0].end = end;
        crate::advanced_dialog::grep_go(&mut st, &mut d, &mut view);
        let (a, b) = view.sel.range();
        let text = &view.doc.main.paras[0].text;
        assert!(text.is_char_boundary(a.offset) && text.is_char_boundary(b.offset));
        // Editing there does not panic.
        view.type_text("x");
    }
}

#[test]
fn grep_searches_again_instead_of_going_to_moved_text() {
    let mut clip = Clipboard::new();
    let mut view = DocView::new(Document::new(), "t".into());
    view.type_text("one fish");
    view.exec(Cmd::NewParagraph, false, &mut clip);
    view.type_text("red fish");
    let (mut d, mut st) = crate::advanced_dialog::grep(&view);
    d.set_field("pattern", "red");
    crate::advanced_dialog::grep_search(&mut st, &mut d, &view);
    assert_eq!(st.hits[0].para, 1);
    // A paragraph goes in at the top: row one's place now holds the
    // first fish, not the red one.
    view.sel = Selection::caret(Position::new(0, 0));
    view.type_text("Title");
    view.exec(Cmd::NewParagraph, false, &mut clip);
    let before = view.sel;
    d.set_selected("hits", 0);
    crate::advanced_dialog::grep_go(&mut st, &mut d, &mut view);
    assert_eq!(view.sel, before, "nothing selected from the old list");
    assert_eq!(st.hits[0].para, 2, "the list is up to date");
    // Now it goes to the red fish.
    crate::advanced_dialog::grep_go(&mut st, &mut d, &mut view);
    assert_eq!(view.sel.range(), (Position::new(2, 0), Position::new(2, 3)));
}

#[test]
fn an_unreadable_hidden_replacement_does_not_stop_a_search() {
    let view = DocView::new(Document::new(), "t".into());
    let mut d = crate::advanced_dialog::regex_search(&view, true);
    d.set_field("pattern", "a");
    // \\9 names a group the pattern does not have.
    d.set_field("with", "\\9");
    crate::advanced_dialog::rx_check(&mut d, true);
    assert!(!d.is_enabled("findnext"));
    // Hide the Replace part: the search works, and the text is kept.
    crate::advanced_dialog::rx_switch(&mut d, &view, false);
    crate::advanced_dialog::rx_check(&mut d, false);
    assert!(d.is_enabled("findnext"));
    crate::advanced_dialog::rx_switch(&mut d, &view, true);
    assert_eq!(d.field_text("with"), "\\9");
}

#[test]
fn grep_lists_paragraphs_and_goes_to_the_one_chosen() {
    let mut clip = Clipboard::new();
    let mut view = DocView::new(Document::new(), "t".into());
    view.type_text("one fish");
    view.exec(Cmd::NewParagraph, false, &mut clip);
    view.type_text("two");
    view.exec(Cmd::NewParagraph, false, &mut clip);
    view.type_text("red fish");
    let (mut d, mut st) = crate::advanced_dialog::grep(&view);
    d.set_field("pattern", "fi.h");
    crate::advanced_dialog::grep_search(&mut st, &mut d, &view);
    let found: Vec<usize> = st.hits.iter().map(|h| h.para).collect();
    assert_eq!(found, [0, 2]);
    d.select_row("hits", 1);
    crate::advanced_dialog::grep_go(&mut st, &mut d, &mut view);
    assert_eq!(view.sel.range(), (Position::new(2, 4), Position::new(2, 8)));
    // A bad pattern lists nothing and says so.
    d.set_field("pattern", "(");
    crate::advanced_dialog::grep_search(&mut st, &mut d, &view);
    assert!(st.hits.is_empty());
    assert!(
        d.item_label("status1")
            .starts_with("The pattern is not valid")
    );
}

#[test]
fn page_and_section_breaks_cut_paste_and_delete_like_characters() {
    let mut clip = Clipboard::new();
    let text = |v: &DocView| -> Vec<(String, Mark)> {
        v.doc
            .main
            .paras
            .iter()
            .map(|p| (p.text.clone(), p.mark))
            .collect()
    };
    let page = |t: &str| vec![(t.to_string(), Mark::Paragraph)];

    let mut view = DocView::new(Document::new(), "t".into());
    view.type_text("HelloAfter");
    view.sel = Selection::caret(Position::new(0, 5));
    view.exec(Cmd::PageBreak, false, &mut clip);
    assert_eq!(text(&view), page("Hello\u{c}After"));
    // Cut the break alone, then put it back.
    view.sel = Selection::new(Position::new(0, 5), Position::new(0, 6));
    view.exec(Cmd::Cut, false, &mut clip);
    assert_eq!(text(&view), page("HelloAfter"));
    view.exec(Cmd::Paste, false, &mut clip);
    assert_eq!(text(&view), page("Hello\u{c}After"));
    // Backspace after it and Delete before it both remove it.
    view.sel = Selection::caret(Position::new(0, 6));
    view.exec(Cmd::Backspace, false, &mut clip);
    assert_eq!(text(&view), page("HelloAfter"));
    view.exec(Cmd::Undo, false, &mut clip);
    view.sel = Selection::caret(Position::new(0, 5));
    view.exec(Cmd::DeleteForward, false, &mut clip);
    assert_eq!(text(&view), page("HelloAfter"));

    // A section break is the end of its paragraph; the same keys work.
    let mut view = DocView::new(Document::new(), "t".into());
    view.type_text("One");
    view.exec(Cmd::SectionBreak, false, &mut clip);
    view.type_text("Two");
    let split = vec![
        ("One".to_string(), Mark::SectionBreak),
        ("Two".to_string(), Mark::Paragraph),
    ];
    assert_eq!(text(&view), split);
    view.sel = Selection::new(Position::new(0, 3), Position::new(1, 0));
    view.exec(Cmd::Cut, false, &mut clip);
    assert_eq!(text(&view), page("OneTwo"));
    view.exec(Cmd::Paste, false, &mut clip);
    assert_eq!(text(&view), split);
    view.sel = Selection::caret(Position::new(1, 0));
    view.exec(Cmd::Backspace, false, &mut clip);
    assert_eq!(text(&view), page("OneTwo"));
    view.exec(Cmd::Undo, false, &mut clip);
    view.sel = Selection::caret(Position::new(0, 3));
    view.exec(Cmd::DeleteForward, false, &mut clip);
    assert_eq!(text(&view), page("OneTwo"));
}

#[test]
fn save_anyway_shows_what_the_file_holds_and_undo_takes_it_back() {
    let mut fonts = quill_layout::Fonts::new(quill_layout::Catalog::scan_system());
    if fonts.family_names().is_empty() {
        return;
    }
    let mut clip = Clipboard::new();
    let doc = quill_core::text::from_plain_text("One\nTwo\nThree");
    let mut view = DocView::new(doc.clone(), "t".into());
    view.update_layout(&mut fonts);
    // As if the file read back with the middle paragraph changed and a
    // paragraph added.
    let mut back = doc;
    back.main.paras[1].pap.left += 720;
    let extra = back.main.paras[2].clone();
    back.main.paras.push(extra);
    assert!(view.doc.match_saved(&back, "Save Anyway", view.sel));
    view.invalidate_layout();
    view.doc.mark_saved();
    view.update_layout(&mut fonts);
    assert_eq!(view.layouts.len(), 4);
    assert!(view.layouts.iter().all(Option::is_some));
    assert_eq!(view.doc.main, back.main);
    assert!(!view.doc.is_modified());
    view.exec(Cmd::Undo, false, &mut clip);
    view.update_layout(&mut fonts);
    assert_eq!(view.layouts.len(), 3);
    assert_eq!(view.doc.main.paras[1].pap.left, 0);
    assert!(view.doc.is_modified());
}

#[test]
fn a_page_break_at_the_end_shows_the_next_page_as_a_line_break_does() {
    let mut fonts = quill_layout::Fonts::new(quill_layout::Catalog::scan_system());
    if fonts.family_names().is_empty() {
        return;
    }
    let mut clip = Clipboard::new();
    let texts =
        |v: &DocView| -> Vec<String> { v.doc.main.paras.iter().map(|p| p.text.clone()).collect() };
    // A document ending in a page break, as an imported one can: the
    // break has an empty line after it, on a page of its own.
    let mut view = DocView::new(quill_core::text::from_plain_text("Hello\u{c}"), "t".into());
    view.update_layout(&mut fonts);
    let spans = |v: &DocView| -> Vec<(usize, usize, bool)> {
        v.layouts[0]
            .as_ref()
            .unwrap()
            .lines
            .iter()
            .map(|x| (x.start, x.end, x.hard_break))
            .collect()
    };
    assert_eq!(spans(&view), [(0, 5, false), (5, 6, true), (6, 6, false)]);
    assert_eq!(view.pages.len(), 2);
    // In the middle of a document the empty line takes no room: the
    // next paragraph starts the new page, at the top.
    let mut view = DocView::new(
        quill_core::text::from_plain_text("Hello\u{c}\nNext"),
        "t".into(),
    );
    view.update_layout(&mut fonts);
    assert_eq!(spans(&view), [(0, 5, false), (5, 6, true), (6, 6, false)]);
    assert_eq!(view.pages.len(), 2);
    assert_eq!((view.pages[1].para, view.pages[1].line), (1, 0));
    assert_eq!(view.pages[1].y, view.tops[1]);
    // Insert Page Break at the end of a paragraph adds no paragraph:
    // the insertion point is on that empty line, on the new page.
    let mut view = DocView::new(Document::new(), "t".into());
    view.type_text("Hello");
    view.exec(Cmd::PageBreak, false, &mut clip);
    assert_eq!(texts(&view), ["Hello\u{c}"]);
    assert_eq!(view.sel, Selection::caret(Position::new(0, 6)));
    view.update_layout(&mut fonts);
    assert_eq!(view.pages.len(), 2);
    view.type_text("After");
    assert_eq!(texts(&view), ["Hello\u{c}After"]);
    // One Undo takes the break back.
    view.sel = Selection::caret(Position::new(0, 11));
    view.exec(Cmd::PageBreak, false, &mut clip);
    view.exec(Cmd::Undo, false, &mut clip);
    assert_eq!(texts(&view), ["Hello\u{c}After"]);
}

#[test]
fn a_page_break_stands_on_its_own_line_between_the_text_around_it() {
    let mut fonts = quill_layout::Fonts::new(quill_layout::Catalog::scan_system());
    if fonts.family_names().is_empty() {
        return;
    }
    let mut clip = Clipboard::new();
    let mut view = DocView::new(Document::new(), "t".into());
    view.type_text("HelloAfter");
    view.sel = Selection::caret(Position::new(0, 5));
    view.exec(Cmd::PageBreak, false, &mut clip);
    view.update_layout(&mut fonts);
    let l = view.layouts[0].as_ref().unwrap();
    let spans: Vec<_> = l
        .lines
        .iter()
        .map(|x| (x.start, x.end, x.hard_break))
        .collect();
    assert_eq!(spans, [(0, 5, false), (5, 6, true), (6, 11, false)]);
}

#[test]
fn paragraph_formats_stop_at_a_selection_ending_at_a_paragraph_start() {
    // Selecting "Alpha" through the start of "Beta" centers only Alpha,
    // whichever way the selection was made.
    let mut clip = Clipboard::new();
    for backward in [false, true] {
        let mut view = DocView::new(Document::new(), "t".into());
        view.type_text("Alpha");
        view.exec(Cmd::NewParagraph, false, &mut clip);
        view.type_text("Beta");
        let (a, b) = (Position::new(0, 0), Position::new(1, 0));
        view.sel = if backward {
            Selection::new(b, a)
        } else {
            Selection::new(a, b)
        };
        view.exec(Cmd::AlignCenter, false, &mut clip);
        let align = |i: usize| view.doc.main.paras[i].pap.align;
        assert_ne!(align(0), align(1), "backward: {backward}");
    }
}

#[test]
fn undo_takes_back_a_format_chosen_with_nothing_typed() {
    let mut clip = Clipboard::new();
    let mut view = DocView::new(Document::new(), "t".into());
    view.exec(Cmd::Bold, false, &mut clip);
    assert_eq!(view.undo_label(), "Undo Formatting");
    view.exec(Cmd::Undo, false, &mut clip);
    view.type_text("abc");
    assert!(!view.doc.main.paras[0].chp_of_char(0).bold);
    // Once typed, Undo takes back the typing as usual.
    let mut view = DocView::new(Document::new(), "t".into());
    view.exec(Cmd::Bold, false, &mut clip);
    view.type_text("abc");
    assert!(view.doc.main.paras[0].chp_of_char(0).bold);
    view.exec(Cmd::Undo, false, &mut clip);
    assert_eq!(view.doc.main.paras[0].text, "");
}
#[test]
fn shift_f3_cycles_the_case() {
    let mut clip = Clipboard::new();
    let mut view = DocView::new(Document::new(), "t".into());
    view.type_text("big red dog");
    view.sel = Selection::new(Position::new(0, 0), Position::new(0, 11));
    let mut seen = Vec::new();
    for _ in 0..3 {
        view.exec(Cmd::CycleCase, false, &mut clip);
        seen.push(view.doc.main.paras[0].text.clone());
    }
    assert_eq!(seen, ["BIG RED DOG", "Big Red Dog", "big red dog"]);
    // With nothing selected, the word at the insertion point.
    view.sel = Selection::caret(Position::new(0, 5));
    view.exec(Cmd::CycleCase, false, &mut clip);
    assert_eq!(view.doc.main.paras[0].text, "big RED dog");
    assert_eq!(view.sel, Selection::caret(Position::new(0, 5)));
}
#[test]
fn a_font_chosen_at_the_insertion_point_carries_on_after_return() {
    let mut clip = Clipboard::new();
    let mut v = DocView::new(Document::new(), "t".into());
    let typewriter = v.doc.font_id("American Typewriter");
    let normal = v.doc.main.paras[0].mark_chp.font;
    assert_ne!(typewriter, normal);
    v.format_chars("Font", |c| c.font = typewriter);
    // A typo put right: Backspace forgets the chosen format, so Return
    // has to take it from the text before the insertion point.
    v.type_text("Helloo");
    v.exec(Cmd::Backspace, false, &mut clip);
    v.exec(Cmd::NewParagraph, false, &mut clip);
    v.type_text("World");
    assert_eq!(v.doc.main.paras[1].chp_of_char(0).font, typewriter);
    // Chosen and then Return straight away, before typing anything.
    v.exec(Cmd::NewParagraph, false, &mut clip);
    let bold = !v.current_chp().bold;
    v.format_chars("Bold", |c| c.bold = bold);
    v.exec(Cmd::NewParagraph, false, &mut clip);
    v.type_text("Again");
    let c = v.doc.main.paras[3].chp_of_char(0);
    assert_eq!((c.font, c.bold), (typewriter, bold));
    // Return in the middle of a paragraph splits it as before.
    v.sel = Selection::caret(Position::new(1, 2));
    v.exec(Cmd::NewParagraph, false, &mut clip);
    assert_eq!(v.doc.main.paras[2].text, "rld");
    assert_eq!(v.doc.main.paras[2].chp_of_char(0).font, typewriter);
}

/// "Before", a 2 by 2 table holding a, b, c, d, then "After".
fn with_table(clip: &mut Clipboard) -> DocView {
    let mut v = DocView::new(Document::new(), "t".into());
    v.type_text("Before");
    v.exec(Cmd::NewParagraph, false, clip);
    v.type_text("After");
    v.doc.insert_table(MAIN, Position::new(1, 0), 2, 2, 1440);
    for (p, t) in [(1, "a"), (2, "b"), (4, "c"), (5, "d")] {
        v.sel = Selection::caret(Position::new(p, 0));
        v.type_text(t);
    }
    v.doc.forget_history();
    v
}

/// Each paragraph's text, with T for a table paragraph and c or r for
/// a cell or row end.
fn shape(v: &DocView) -> String {
    v.doc
        .main
        .paras
        .iter()
        .map(|p| {
            let kind = match p.mark {
                Mark::CellEnd => "c",
                Mark::RowEnd => "r",
                _ => "",
            };
            let t = if p.pap.in_table { "T" } else { "" };
            format!("{}{t}{kind}", p.text)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A 3 by 3 table with "r0c0" and so on in its cells, then an empty
/// paragraph. Cell (r, k) is paragraph 4r + k.
fn grid() -> DocView {
    let mut v = DocView::new(Document::new(), "t".into());
    v.doc.insert_table(MAIN, Position::new(0, 0), 3, 3, 1440);
    for r in 0..3 {
        for k in 0..3 {
            v.sel = Selection::caret(Position::new(4 * r + k, 0));
            v.type_text(&format!("r{r}c{k}"));
        }
    }
    v.doc.forget_history();
    v
}

#[test]
fn dragging_across_cells_selects_a_rectangle_of_whole_cells() {
    let mut clip = Clipboard::new();
    let mut v = grid();
    // From the middle of r0c1 to the middle of r1c2.
    v.sel = Selection::new(Position::new(1, 2), Position::new(6, 1));
    let block = v.cell_block().unwrap();
    assert_eq!((block.rows.len(), block.from, block.to), (2, 1, 2));
    assert_eq!(block.cells(), [(1, 1), (2, 2), (5, 5), (6, 6)]);
    // Dragged up and to the left, it is the same rectangle.
    v.sel = Selection::new(Position::new(6, 1), Position::new(1, 2));
    assert_eq!(v.cell_block(), Some(block));
    // Within one cell it is ordinary text.
    v.sel = Selection::new(Position::new(1, 0), Position::new(1, 2));
    assert!(v.cell_block().is_none());

    // Bold goes on the selected cells only, all of their text.
    v.sel = Selection::new(Position::new(1, 2), Position::new(6, 1));
    v.exec(Cmd::Bold, false, &mut clip);
    let bold = |v: &DocView, p: usize| v.doc.main.paras[p].chp_at(0).bold;
    assert!([1, 2, 5, 6].iter().all(|&p| bold(&v, p)));
    assert!([0, 4, 8, 9, 10].iter().all(|&p| !bold(&v, p)));
    assert!(v.doc.main.paras[1].chp_of_char(0).bold);

    // Copy makes a 2 by 2 table of just those cells.
    let block = v.cell_block().unwrap();
    let frag = v
        .doc
        .copy_cells(MAIN, &block.rows, block.from, block.to, 9360);
    let rows: Vec<_> = frag
        .paras
        .iter()
        .filter(|p| p.mark == Mark::RowEnd)
        .collect();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|r| r.row.as_ref().unwrap().cells.len() == 2)
    );
    assert_eq!(frag.paras[0].text, "r0c1");

    // Delete empties them and leaves the table in shape.
    v.exec(Cmd::Backspace, false, &mut clip);
    let text = |v: &DocView, p: usize| v.doc.main.paras[p].text.clone();
    assert!([1, 2, 5, 6].iter().all(|&p| text(&v, p).is_empty()));
    assert_eq!(text(&v, 0), "r0c0");
    assert_eq!(text(&v, 10), "r2c2");
    assert_eq!(table::table_rows(&v.doc.main.paras, 0).len(), 3);
    assert_eq!(v.sel, Selection::caret(Position::new(1, 0)));
    v.exec(Cmd::Undo, false, &mut clip);
    assert_eq!(text(&v, 6), "r1c2");
}

#[test]
fn pasting_into_selected_cells_fills_them_one_for_one() {
    let mut clip = Clipboard::without_system();
    let mut v = grid();
    let text = |v: &DocView, p: usize| v.doc.main.paras[p].text.clone();
    // Copy the first row's first two cells, paste into a block that
    // starts at r1c0: the two cells land there, the rest is untouched.
    v.sel = Selection::new(Position::new(0, 1), Position::new(1, 1));
    v.exec(Cmd::Copy, false, &mut clip);
    v.sel = Selection::new(Position::new(4, 1), Position::new(10, 1));
    v.exec(Cmd::Paste, false, &mut clip);
    assert_eq!(text(&v, 4), "r0c0");
    assert_eq!(text(&v, 5), "r0c1");
    assert_eq!(text(&v, 6), "r1c2");
    assert_eq!(text(&v, 8), "r2c0");
    assert_eq!(table::table_rows(&v.doc.main.paras, 0).len(), 3);
    v.exec(Cmd::Undo, false, &mut clip);
    assert_eq!(text(&v, 4), "r1c0");

    // One value fills every selected cell.
    v.sel = Selection::new(Position::new(2, 0), Position::new(2, 4));
    v.exec(Cmd::Copy, false, &mut clip);
    v.sel = Selection::new(Position::new(4, 1), Position::new(9, 1));
    v.exec(Cmd::Paste, false, &mut clip);
    assert!([4, 5, 8, 9].iter().all(|&p| text(&v, p) == "r0c2"));
    assert_eq!(text(&v, 6), "r1c2");
    assert_eq!(text(&v, 10), "r2c2");

    // Text from another program: tabs across, line ends down.
    let mut other = Clipboard::without_system();
    other.put_text("x\ty\nz\tw");
    v.sel = Selection::new(Position::new(0, 1), Position::new(5, 1));
    v.exec(Cmd::Paste, false, &mut other);
    assert_eq!([0, 1, 4, 5].map(|p| text(&v, p)), ["x", "y", "z", "w"]);
    assert_eq!(text(&v, 2), "r0c2");
}

#[test]
fn pasting_cells_brings_their_notes() {
    let mut clip = Clipboard::without_system();
    let mut v = grid();
    // A footnote in r0c0, with text.
    let base = v.doc.main.paras[0].chp_at(0);
    let end = v.doc.main.paras[0].len();
    let id = v.doc.insert_note(Position::new(0, end), base, None);
    let at = v.doc.note_text_start(id).unwrap();
    let chp = v.doc.notes.paras[at.para].mark_chp;
    v.doc
        .insert_text(quill_core::StoryId::Notes, at, "Cell note.", chp);
    v.doc.forget_history();
    let refs = |v: &DocView| quill_core::notes::note_ids(&v.doc.main.paras).len();
    assert_eq!(refs(&v), 1);
    // Copy the first row's first two cells, and paste them one row down.
    v.sel = Selection::new(Position::new(0, 1), Position::new(1, 1));
    v.exec(Cmd::Copy, false, &mut clip);
    v.sel = Selection::new(Position::new(4, 1), Position::new(10, 1));
    v.exec(Cmd::Paste, false, &mut clip);
    // The copy has a note of its own, with the same text.
    assert_eq!(refs(&v), 2);
    let notes: Vec<&str> = v.doc.notes.paras.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(notes.iter().filter(|t| t.contains("Cell note.")).count(), 2);
}

#[test]
fn headers_are_laid_out_again_when_the_page_or_its_numbering_changes() {
    let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
        return;
    };
    let mut v = DocView::new(Document::new(), "t".into());
    v.type_text("Body");
    // A header of many words, and one that shows the page number.
    let long = "word ".repeat(120);
    let at = StoryId::Header.header_index();
    let para = |text: &str| Paragraph::with_text(text, CharProps::default(), ParaProps::default());
    v.doc.headers[at] = quill_core::Story {
        paras: vec![para(&long)],
    };
    v.update_layout(&mut fonts);
    let height = |v: &DocView| -> f32 {
        v.hf.values()
            .next()
            .map(|ls| ls.iter().map(|l| l.height).sum())
            .unwrap()
    };
    let before = height(&v);
    // A narrower page wraps the header onto more lines.
    v.edit_document_props("Page", |p| p.page_width -= 4000);
    v.update_layout(&mut fonts);
    assert!(height(&v) > before, "{} vs {before}", height(&v));

    // The page number's look: the same header, a different number.
    let number = format!("{}", chars::FIELD_PAGE);
    v.doc.headers[at] = quill_core::Story {
        paras: vec![para(&number)],
    };
    v.hf_dirty = true;
    v.update_layout(&mut fonts);
    let glyph = |v: &DocView| v.hf.values().next().unwrap()[0].fields[0].1[0].id;
    let arabic = glyph(&v);
    v.doc.section.page_number_format = quill_core::PageNumberFormat::UpperRoman;
    v.invalidate_layout();
    v.update_layout(&mut fonts);
    assert_ne!(glyph(&v), arabic);
}

#[test]
fn cell_alignment_goes_on_each_selected_cell() {
    use quill_core::props::VAlign;
    let mut v = grid();
    v.sel = Selection::new(Position::new(4, 1), Position::new(9, 1));
    v.align_cells(VAlign::Bottom, Align::Center);
    let paras = &v.doc.main.paras;
    let rows = table::table_rows(paras, 0);
    let valign = |r: usize, k: usize| table::row_props(paras, &rows[r], 9360).cells[k].valign;
    assert_eq!(valign(1, 0), VAlign::Bottom);
    assert_eq!(valign(2, 1), VAlign::Bottom);
    assert_eq!(valign(1, 2), VAlign::Top);
    assert_eq!(valign(0, 0), VAlign::Top);
    assert_eq!(paras[5].pap.align, Align::Center);
    assert_eq!(paras[6].pap.align, Align::Left);
    // With just an insertion point, its own cell.
    v.sel = Selection::caret(Position::new(10, 0));
    v.align_cells(VAlign::Center, Align::Right);
    assert_eq!(v.cell_alignment(), Some((VAlign::Center, Align::Right)));
    v.sel = Selection::caret(Position::new(4, 0));
    assert_eq!(v.cell_alignment(), Some((VAlign::Bottom, Align::Center)));
}

#[test]
fn centered_and_bottom_cells_sit_lower_in_a_tall_row() {
    use quill_core::props::VAlign;
    let mut fonts = quill_layout::Fonts::new(quill_layout::Catalog::scan_system());
    let mut v = grid();
    // Cell r0c0 gets three lines, so the first row is three lines tall.
    v.sel = Selection::caret(Position::new(0, 4));
    let mut clip = Clipboard::new();
    v.exec(Cmd::NewParagraph, false, &mut clip);
    v.exec(Cmd::NewParagraph, false, &mut clip);
    v.sel = Selection::caret(Position::new(3, 0));
    v.align_cells(VAlign::Center, Align::Left);
    v.sel = Selection::caret(Position::new(4, 0));
    v.align_cells(VAlign::Bottom, Align::Left);
    v.update_layout(&mut fonts);
    let line = v.tops[1] - v.tops[0];
    assert!((v.tops[3] - (v.tops[0] + line)).abs() < 1.5, "centered");
    assert!((v.tops[4] - (v.tops[0] + 2.0 * line)).abs() < 1.5, "bottom");
}

#[test]
fn a_right_click_in_any_selected_cell_keeps_the_selection() {
    let mut clip = Clipboard::new();
    let mut v = with_table(&mut clip);
    let (first, second) = {
        let rows = quill_core::table::table_rows(&v.doc.main.paras, 2);
        (rows[0].cells[0].0, rows[1].cells[0].0)
    };
    v.sel = Selection::new(Position::new(first, 0), Position::new(second, 0));
    let sel = v.sel;
    // The second cell is at the end of the range, and past it.
    v.context_at(Position::new(second, 1), false);
    assert_eq!(v.sel, sel);
    // Outside the block the selection goes, as anywhere else.
    v.context_at(Position::new(first + 1, 0), false);
    assert!(v.sel.is_empty());
}

#[test]
fn a_selected_table_has_no_hyperlink_in_its_context_menu() {
    let mut clip = Clipboard::new();
    let mut v = with_table(&mut clip);
    v.sel = Selection::caret(Position::new(2, 0));
    v.exec(Cmd::SelectTable, false, &mut clip);
    let (menu, _) = crate::app_menus::context_menu(&v, false, false);
    assert!(!menu.items.iter().any(|i| i.label.starts_with("Hyperlink")));
    assert!(
        menu.items
            .iter()
            .any(|i| i.label.starts_with("Borders and"))
    );
}

#[test]
fn whole_tables_cut_copy_and_paste_in_one_piece() {
    let mut clip = Clipboard::new();
    let table = "aTc bTc Tr cTc dTc Tr";
    let mut v = with_table(&mut clip);
    assert_eq!(shape(&v), format!("Before {table} After"));
    v.sel = Selection::caret(Position::new(2, 0));
    v.exec(Cmd::SelectTable, false, &mut clip);
    // Every cell shows as selected, though the selection ends past them.
    let block = v.selected_cells().unwrap();
    assert_eq!((block.rows.len(), block.from, block.to), (2, 0, 1));
    v.exec(Cmd::Cut, false, &mut clip);
    assert_eq!(shape(&v), "Before After");
    v.exec(Cmd::Undo, false, &mut clip);
    assert_eq!(shape(&v), format!("Before {table} After"));

    // Pasted in the middle of a paragraph, the table splits it.
    v.exec(Cmd::SelectTable, false, &mut clip);
    v.exec(Cmd::Copy, false, &mut clip);
    v.sel = Selection::caret(Position::new(0, 3));
    v.exec(Cmd::Paste, false, &mut clip);
    assert_eq!(shape(&v), format!("Bef {table} ore {table} After"));
    assert_eq!(v.sel, Selection::caret(Position::new(7, 0)));
    v.exec(Cmd::Undo, false, &mut clip);

    // At the start of a paragraph, it goes before it.
    v.sel = Selection::caret(Position::new(0, 0));
    v.exec(Cmd::Paste, false, &mut clip);
    assert_eq!(shape(&v), format!("{table} Before {table} After"));
    v.exec(Cmd::Undo, false, &mut clip);

    // Inside a cell, the table arrives as a table in that cell, after
    // the text before the insertion point and before the text after.
    v.sel = Selection::caret(Position::new(2, 1));
    v.exec(Cmd::Paste, false, &mut clip);
    assert_eq!(
        shape(&v),
        format!("Before aTc bT {table} Tc Tr cTc dTc Tr After")
    );
    let levels: Vec<u8> = v.doc.main.paras.iter().map(|p| p.pap.level()).collect();
    assert_eq!(levels, [0, 1, 1, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 0]);
    v.exec(Cmd::Undo, false, &mut clip);

    // Cells copied without the rest of their row paste as a table of
    // just those cells.
    v.sel = Selection::new(Position::new(1, 0), Position::new(2, 1));
    v.exec(Cmd::Copy, false, &mut clip);
    v.sel = Selection::caret(Position::new(7, 5));
    v.exec(Cmd::Paste, false, &mut clip);
    assert_eq!(shape(&v), format!("Before {table} After aTc bTc Tr "));
}

/// An outer table of one row and two cells; the first holds "before",
/// a table of one row and two cells ("in1", "in2") and "after".
fn nested_view() -> DocView {
    let mut doc = Document::new();
    doc.insert_table(MAIN, Position::new(0, 0), 1, 2, 4000);
    let chp = doc.styles.get(0).chp;
    doc.insert_text(MAIN, Position::new(0, 0), "before", chp);
    doc.split_paragraph(MAIN, Position::new(0, 6));
    doc.insert_table(MAIN, Position::new(1, 0), 1, 2, 1500);
    doc.insert_text(MAIN, Position::new(1, 0), "in1", chp);
    doc.insert_text(MAIN, Position::new(2, 0), "in2", chp);
    doc.insert_text(MAIN, Position::new(4, 0), "after", chp);
    doc.insert_text(MAIN, Position::new(5, 0), "side", chp);
    doc.forget_history();
    DocView::new(doc, "T".into())
}

#[test]
fn rows_of_a_merged_cell_grow_to_hold_its_text() {
    let mut fonts = quill_layout::Fonts::new(quill_layout::Catalog::scan_system());
    if fonts.family_names().is_empty() {
        return;
    }
    let mut doc = quill_core::text::from_plain_text("one\ttwo\nthree\tfour");
    doc.convert_to_table(MAIN, 0, 1, quill_core::table::Convert::Tabs, |_| 2000);
    let rows = quill_core::table::table_rows(&doc.main.paras, 0);
    doc.merge_cells_down(MAIN, &rows, &[0, 0], 8640);
    // Six lines in the merged cell, one in each of the others.
    let chp = doc.main.paras[0].chp_at(0);
    for i in 0..5 {
        doc.split_paragraph(MAIN, Position::new(i, doc.main.paras[i].len()));
        doc.insert_text(MAIN, Position::new(i + 1, 0), "more", chp);
    }
    doc.forget_history();
    let mut v = DocView::new(doc, "T".into());
    v.area = Rect::new(0.0, 0.0, 800.0, 600.0);
    v.update_layout(&mut fonts);
    let rows: Vec<&Block> = v.blocks.iter().filter(|b| b.row.is_some()).collect();
    assert_eq!(rows.len(), 2);
    let (top, under) = (rows[0].row.as_ref().unwrap(), rows[1].row.as_ref().unwrap());
    assert_eq!(top.joined_below, [true, false]);
    assert_eq!(under.joined_above, [true, false]);
    // The two rows together hold the merged cell's six paragraphs.
    let line = v.layout(1).height;
    assert!(rows[0].height + rows[1].height >= 6.0 * line - 0.5);
    assert!(rows[0].height < 2.0 * line + 0.5);
    // A click under the merge lands in the top cell.
    let x = top.cells[0].2 + 5.0;
    let (at, _) = v.hit_doc(x, rows[1].top + 1.0);
    assert!(at.para <= top.cells[0].1);
}

#[test]
fn a_table_two_levels_in_is_drawn_after_the_one_holding_it() {
    let mut fonts = quill_layout::Fonts::new(quill_layout::Catalog::scan_system());
    if fonts.family_names().is_empty() {
        return;
    }
    let mut v = nested_view();
    v.doc.insert_table(MAIN, Position::new(1, 0), 1, 2, 600);
    v.area = Rect::new(0.0, 0.0, 800.0, 600.0);
    v.update_layout(&mut fonts);
    // Rows are drawn in this order: the middle table's shading must go
    // down before the deepest table's lines.
    let row = v.blocks[0].row.as_ref().unwrap();
    let levels: Vec<u8> = row.nested.iter().map(|n| n.level).collect();
    assert_eq!(levels, [2, 3]);
}

#[test]
fn a_table_in_a_cell_is_laid_out_inside_the_cell() {
    let mut fonts = quill_layout::Fonts::new(quill_layout::Catalog::scan_system());
    if fonts.family_names().is_empty() {
        return;
    }
    let mut v = nested_view();
    v.area = Rect::new(0.0, 0.0, 800.0, 600.0);
    v.update_layout(&mut fonts);
    let levels: Vec<u8> = v.doc.main.paras.iter().map(|p| p.pap.level()).collect();
    assert_eq!(levels, [1, 2, 2, 2, 1, 1, 1, 0]);
    // One block for the outer row, with the inner row in it.
    let b = &v.blocks[0];
    let row = b.row.as_ref().unwrap();
    assert_eq!((b.first, b.last), (0, 6));
    assert_eq!(row.nested.len(), 1);
    let inner = &row.nested[0];
    let (outer_x0, outer_x1) = (row.cells[0].2, row.cells[0].3);
    assert!(inner.geom.cells[0].2 >= outer_x0);
    assert!(inner.geom.cells[1].3 <= outer_x1 + 0.01);
    // Text sits in order: before, the inner cells side by side, after.
    assert!(v.tops[0] < v.tops[1]);
    assert_eq!(v.tops[1], v.tops[2]);
    assert!(v.xs[1] < v.xs[2]);
    assert!(v.tops[4] >= inner.top + inner.height - 0.01);
    // The second outer cell starts at the row's top.
    assert_eq!(v.tops[5], b.top);
    assert!(b.height >= v.tops[4] + v.layout(4).height - b.top - 0.01);
    // A point in the second inner cell hits its text.
    let (x, y) = (v.xs[2] + 1.0, v.tops[2] + 1.0);
    assert_eq!(v.hit_doc(x, y).0.para, 2);
    let (x, y) = (v.xs[1] + 1.0, v.tops[1] + 1.0);
    assert_eq!(v.hit_doc(x, y).0.para, 1);
    let (x, y) = (v.xs[4] + 1.0, v.tops[4] + 1.0);
    assert_eq!(v.hit_doc(x, y).0.para, 4);
}

/// The x of a point well inside the first cell of the outer row.
fn row_x(v: &DocView) -> f32 {
    v.blocks[0].row.as_ref().unwrap().cells[0].2 + 12.0
}
#[test]
fn a_table_at_the_end_of_a_cell_leaves_only_a_sliver_below_it() {
    let mut fonts = quill_layout::Fonts::new(quill_layout::Catalog::scan_system());
    if fonts.family_names().is_empty() {
        return;
    }
    // An empty cell with a table in it: nothing before, nothing after.
    let mut doc = Document::new();
    doc.insert_table(MAIN, Position::new(0, 0), 1, 2, 4000);
    doc.insert_table(MAIN, Position::new(0, 0), 1, 1, 1500);
    doc.forget_history();
    let mut v = DocView::new(doc, "T".into());
    v.area = Rect::new(0.0, 0.0, 800.0, 600.0);
    v.update_layout(&mut fonts);
    let b = &v.blocks[0];
    let inner = &b.row.as_ref().unwrap().nested[0];
    // The row ends just under the inner row.
    let gap = b.top + b.height - (inner.top + inner.height);
    assert!(gap > 0.0 && gap <= 3.01, "{gap}");
    // Typing under the table makes room for the text.
    v.sel = Selection::caret(Position::new(2, 0));
    v.type_text("after");
    v.update_layout(&mut fonts);
    let b = &v.blocks[0];
    let inner = &b.row.as_ref().unwrap().nested[0];
    assert!(b.top + b.height - (inner.top + inner.height) > 8.0);
}

#[test]
fn dragging_after_a_double_click_grows_by_character() {
    let mut fonts = quill_layout::Fonts::new(quill_layout::Catalog::scan_system());
    if fonts.family_names().is_empty() {
        return;
    }
    let mut v = DocView::new(Document::new(), "Drag".into());
    v.area = Rect::new(0.0, 0.0, 800.0, 600.0);
    v.type_text("alpha bravo charlie");
    v.update_layout(&mut fonts);
    let at = |v: &DocView, offset: usize| {
        let (x, top, h) = v.caret_geometry(Position::new(0, offset), false);
        v.to_window(x + 1.0, top + h / 2.0)
    };
    // Double-click inside "alpha", then drag into the middle of "bravo".
    let (wx, wy) = at(&v, 2);
    v.mouse_down(wx, wy, false, false);
    v.mouse_down(wx, wy, false, false);
    assert_eq!(v.sel.range(), (Position::new(0, 0), Position::new(0, 6)));
    let (dx, dy) = at(&v, 8);
    v.mouse_drag(dx, dy);
    v.mouse_up();
    assert_eq!(v.sel.range(), (Position::new(0, 0), Position::new(0, 8)));
}

#[test]
fn dragging_a_cell_border_changes_cell_widths_and_row_heights() {
    let mut fonts = quill_layout::Fonts::new(quill_layout::Catalog::scan_system());
    if fonts.family_names().is_empty() {
        return;
    }
    let mut clip = Clipboard::new();
    let mut v = nested_view();
    v.area = Rect::new(0.0, 0.0, 800.0, 600.0);
    v.update_layout(&mut fonts);
    let widths = |v: &DocView, para: usize| -> Vec<i32> {
        let paras = &v.doc.main.paras;
        let row = table::row_at(paras, para).unwrap();
        let props = table::row_props(paras, &row, 9000);
        props.cells.iter().map(|c| c.width).collect()
    };
    assert_eq!(widths(&v, 1), [1500, 1500]);

    // The border between the two inner cells: 10 points is 200 twips,
    // taken from the cell on its right.
    let row = v.blocks[0].row.as_ref().unwrap();
    let (bx, top) = (row.nested[0].geom.cells[0].3, row.nested[0].top);
    let (wx, wy) = v.to_window(bx, top + 2.0);
    assert_eq!(v.border_cursor(wx, wy), Some(BorderKind::Column));
    v.mouse_down(wx, wy, false, false);
    assert!(v.dragging());
    v.mouse_drag(wx + 10.0, wy);
    v.mouse_up();
    assert_eq!(widths(&v, 1), [1700, 1300]);
    // The outer table is as it was.
    assert_eq!(widths(&v, 0), [4000, 4000]);
    // One step to undo.
    v.exec(Cmd::Undo, false, &mut clip);
    assert_eq!(widths(&v, 1), [1500, 1500]);
    v.update_layout(&mut fonts);

    // The bottom of the outer row: 20 points taller.
    let b = &v.blocks[0];
    let (wx, wy) = v.to_window(row_x(&v), b.top + b.height);
    assert_eq!(v.border_cursor(wx, wy), Some(BorderKind::Row));
    let tall = b.height;
    v.mouse_down(wx, wy, false, false);
    v.mouse_drag(wx, wy + 20.0);
    v.mouse_up();
    let paras = &v.doc.main.paras;
    let row = table::row_at_level(paras, 0, 1).unwrap();
    let props = table::row_props(paras, &row, 9000);
    assert_eq!(
        props.height,
        quill_core::RowHeight::AtLeast(((tall + 20.0) * 20.0).round() as i32)
    );
    v.update_layout(&mut fonts);
    assert!((v.blocks[0].height - (tall + 20.0)).abs() < 0.6);

    // Away from any border, a press is an ordinary click.
    let (wx, wy) = v.to_window(row_x(&v) + 5.0, v.tops[0] + 3.0);
    assert_eq!(v.border_cursor(wx, wy), None);
}

#[test]
fn a_break_in_a_table_cell_is_refused_with_a_reason() {
    let mut clip = Clipboard::new();
    let mut v = with_table(&mut clip);
    let before = v.doc.main.paras.clone();
    v.sel = Selection::caret(Position::new(1, 1));
    for cmd in [Cmd::PageBreak, Cmd::SectionBreak] {
        v.status = None;
        v.exec(cmd, false, &mut clip);
        assert_eq!(v.doc.main.paras, before);
        assert!(
            v.status
                .as_deref()
                .is_some_and(|s| s.contains("inside a table"))
        );
    }
    // In a cell, Insert is the table's: no breaks.
    let insert = |v: &DocView| {
        let (menu, _) = crate::app_menus::context_menu(v, false, false);
        let item = menu
            .items
            .into_iter()
            .find(|i| i.label == "Insert")
            .unwrap();
        let items = item.submenu.unwrap().items;
        items.into_iter().map(|i| i.label).collect::<Vec<_>>()
    };
    assert_eq!(insert(&v)[0], "Row Above");
    v.sel = Selection::caret(Position::new(0, 1));
    assert_eq!(insert(&v)[0], "Page Break");
}

#[test]
fn the_move_handle_of_a_table_at_the_top_of_a_page_is_not_the_header() {
    let mut fonts = quill_layout::Fonts::new(quill_layout::Catalog::scan_system());
    if fonts.family_names().is_empty() {
        return;
    }
    let mut v = DocView::new(Document::new(), "t".into());
    v.doc.insert_table(MAIN, Position::new(0, 0), 2, 2, 1440);
    v.sel = Selection::caret(Position::new(0, 0));
    v.page_view = true;
    v.area = Rect::new(0.0, 0.0, 800.0, 600.0);
    v.update_layout(&mut fonts);
    let (_, _, r) = v.table_handle().expect("a handle");
    let (hx, hy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    assert!(
        v.header_at(hx, hy + 30.0).is_none(),
        "the table, not the header"
    );
    assert!(
        v.header_at(hx, hy - 30.0).is_some(),
        "above it is the header"
    );
    assert!(v.header_at(hx, hy).is_none(), "the handle comes first");
    v.mouse_down(hx, hy, false, false);
    v.mouse_up();
    assert_eq!(
        v.sel.range(),
        (
            Position::new(0, 0),
            Position::new(v.doc.main.paras.len() - 1, 0)
        )
    );
}

#[test]
fn a_row_taller_than_the_page_carries_on_over_the_next() {
    let mut fonts = quill_layout::Fonts::new(quill_layout::Catalog::scan_system());
    if fonts.family_names().is_empty() {
        return;
    }
    let mut clip = Clipboard::new();
    let mut v = with_table(&mut clip);
    v.sel = Selection::caret(Position::new(2, 1));
    v.type_text(&"A line of the tall cell\n".repeat(120));
    v.page_view = true;
    v.area = Rect::new(0.0, 0.0, 800.0, 600.0);
    v.update_layout(&mut fonts);
    let p = &v.doc.props;
    let height = (p.page_height - p.margin_top - p.margin_bottom) as f32 / 20.0;
    assert!(v.pages.len() >= 3, "{} pages", v.pages.len());
    for b in &v.bands {
        assert!(b.end - b.y <= height + 0.5, "{b:?} runs off its page");
    }
}
