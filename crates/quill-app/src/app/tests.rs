use super::*;

#[test]
fn the_window_icon_picture_decodes_to_a_square_with_alpha() {
    // Windows and Linux windows build their icon from this file.
    let png = include_bytes!("../../../../assets/quill-64.png");
    let (w, h, rgba) =
        crate::image::decode_rgba(png, quill_core::ImageFormat::Png).expect("icon decodes");
    assert_eq!((w, h), (64, 64));
    assert_eq!(rgba.len(), 64 * 64 * 4);
    assert!(rgba.chunks(4).any(|p| p[3] == 255));
}

#[cfg(target_os = "macos")]
#[test]
fn toolbar_and_formatting_pop_ups_are_handed_to_the_system_menu_on_the_mac() {
    let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
        return;
    };
    let families = fonts.family_info();
    let mut cache = GlyphCache::new();
    let mut label = Label {
        fonts: &mut fonts,
        cache: &mut cache,
    };
    let env = Env {
        font_names: &[],
        font_families: &families,
        recent: &[],
        windows: &[],
    };
    let mut state = WinState::new(DocView::new(Document::new(), "Pop-ups".into()));
    state.width = 900.0;
    state.height = 700.0;
    state.ui = 1.5;
    let toolbar_menu = (0..64)
        .find(|i| toolbar::command(*i) == Some(Cmd::ToolbarMenu))
        .expect("the toolbar has a Toolbar Menu button");
    state.open_toolbar_menu(toolbar_menu, &mut label);
    let (menu, actions, _) = state.take_popup_menu().expect("the toolbar's menu");
    assert!(state.menu.is_none(), "Quill draws nothing itself");
    assert!(
        menu.items
            .iter()
            .any(|it| actions.get(it.id as usize) == Some(&Action::ToolbarPosition(1)))
    );
    for hit in [Hit::FontPopup, Hit::SizePopup, Hit::StylePopup] {
        state.open_popup(hit, &mut label, &env);
        let rect = state.menu.as_ref().expect("a pop-up opens").open.rect;
        let (menu, _, at) = state.take_popup_menu().expect("the pop-up's menu");
        assert!(state.menu.is_none(), "{hit:?}: Quill draws nothing itself");
        assert!(!menu.items.is_empty(), "{hit:?}");
        if hit == Hit::FontPopup {
            // Font names keep their own faces in the native menu.
            assert!(menu.items.iter().any(|it| it.font.is_some()));
        }
        assert_eq!(at, ((rect.x * 1.5) as f64, (rect.y * 1.5) as f64));
    }
}
use quill_core::{Position, Selection};

#[test]
fn paragraph_styles_lists_styles_and_its_buttons_make_edit_and_delete_them() {
    let doc = quill_core::text::from_plain_text("One\nTwo");
    let mut state = WinState::new(DocView::new(doc, "Doc".into()));
    let env = Env {
        font_names: &[],
        font_families: &[],
        recent: &[],
        windows: &[],
    };
    let mut clip = Clipboard::new();
    state.open_dialog(DialogId::Style, &env);
    assert!(matches!(state.dialogs[0].0, Which::ParagraphStyles(_)));
    // New opens the editor; OK saves the style without applying it, and
    // the list comes back on it.
    state.dialog_outcome(Outcome::Button("new"), &mut clip);
    assert!(matches!(&state.dialogs[1].0, Which::Style(st) if st.selected == 0));
    state.dialogs[1].1.set_field("name", "Quote");
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert_eq!(state.dialogs.len(), 1);
    let quote = state.view.doc.styles.find("Quote").unwrap();
    assert_eq!(state.view.doc.main.paras[0].pap.style, 0);
    let selected = |state: &WinState| match &state.dialogs[0] {
        (Which::ParagraphStyles(st), d) => crate::style_dialog::list_selected(st, d),
        _ => None,
    };
    assert_eq!(selected(&state), Some(quote));
    // Edit opens the editor on the chosen style.
    state.dialog_outcome(Outcome::Button("edit"), &mut clip);
    assert!(matches!(
        &state.dialogs[1].0,
        Which::Style(st) if crate::style_dialog::editing(st) == Some(quote)
    ));
    state.dialog_outcome(Outcome::Cancel, &mut clip);
    // Delete on a style in use asks; Cancel leaves it.
    state
        .view
        .set_selection(Selection::caret(Position::new(1, 0)));
    state.view.set_style(quote);
    state.dialog_outcome(Outcome::Button("delete"), &mut clip);
    assert!(matches!(state.dialogs[1].0, Which::DeleteStyle { .. }));
    state.dialog_outcome(Outcome::Cancel, &mut clip);
    assert!(state.view.doc.styles.find("Quote").is_some());
    // Delete there sets the paragraph to No Style and the list moves on.
    state.dialog_outcome(Outcome::Button("delete"), &mut clip);
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert_eq!(state.dialogs.len(), 1);
    assert!(state.view.doc.styles.find("Quote").is_none());
    assert_eq!(state.view.doc.main.paras[1].pap.style, 0);
    assert!(selected(&state).is_some());
}

/// Save what the style editor in front shows, keeping it open.
fn define_in_editor(state: &mut WinState) {
    let Some((Which::Style(st), d)) = state.dialogs.last_mut() else {
        panic!("the style editor is in front");
    };
    crate::style_dialog::define(st, d, &mut state.view);
}

#[test]
fn the_style_dialogs_character_style_menu_makes_a_new_style_and_comes_back_to_it() {
    let doc = quill_core::text::from_plain_text("Body text");
    let mut state = WinState::new(DocView::new(doc, "Doc".into()));
    let (d, st) = crate::style_dialog::editor(&state.view, Some(state.view.current_pap().style));
    state.dialogs.push((Which::Style(st), d));
    let mut clip = Clipboard::new();
    let none = state.view.doc.styles.characters.len();
    let last = none + 1;
    // The menu's last choice is the one that makes a style.
    assert_eq!(
        state.dialogs[0]
            .1
            .options("character_style")
            .last()
            .map(String::as_str),
        Some("New Character Style\u{2026}")
    );
    state.dialogs[0].1.set_selected("character_style", last);
    state.dialog_outcome(Outcome::Changed("character_style"), &mut clip);
    assert!(matches!(
        state.dialogs[1].0,
        Which::CharacterStyleEdit(None)
    ));
    // The menu goes back to what the style had until the new one is saved.
    assert_eq!(state.dialogs[0].1.selected("character_style"), 0);
    state.dialogs[1].1.set_field("name", "Aside");
    state.dialogs[1].1.set_on("bold", true);
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert_eq!(state.dialogs.len(), 1, "back in the Style dialog");
    let id = state.view.doc.styles.find_character("Aside").unwrap();
    let d = &state.dialogs[0].1;
    assert_eq!(d.selected("character_style"), id as usize);
    assert!(d.options("character_style").iter().any(|o| o == "Aside"));
    let Which::Style(st) = &state.dialogs[0].0 else {
        panic!("the Style dialog");
    };
    assert_eq!(st.character, id);
    // Cancelling the editor leaves everything as it was.
    let count = state.view.doc.styles.characters.len();
    state.dialogs[0]
        .1
        .set_selected("character_style", count + 1);
    state.dialog_outcome(Outcome::Changed("character_style"), &mut clip);
    state.dialog_outcome(Outcome::Cancel, &mut clip);
    assert_eq!(state.dialogs.len(), 1);
    assert_eq!(state.view.doc.styles.characters.len(), count);
    assert_eq!(state.dialogs[0].1.selected("character_style"), id as usize);
}

#[test]
fn character_styles_window_edits_and_adds_styles_through_its_editor() {
    let mut doc = quill_core::text::from_plain_text("Body text");
    doc.add_character_style(
        "Callout",
        quill_core::character_styles::CharacterFormat::default(),
    )
    .unwrap();
    doc.forget_history();
    let original = quill_formats::native::write(&doc, 0);
    let mut state = WinState::new(DocView::new(doc, "Doc".into()));
    let d = crate::character_style_dialog::dialog(&state.view);
    state.dialogs.push((Which::CharacterStyles, d));
    let mut clip = Clipboard::new();

    // Cancel leaves the style and the document alone.
    state.dialog_outcome(Outcome::Button("edit"), &mut clip);
    assert!(matches!(
        state.dialogs[1].0,
        Which::CharacterStyleEdit(Some(1))
    ));
    state.dialogs[1].1.set_field("size", "18");
    state.dialog_outcome(Outcome::Cancel, &mut clip);
    assert_eq!(state.dialogs.len(), 1);
    assert_eq!(quill_formats::native::write(&state.view.doc, 0), original);

    // OK saves what the editor shows into the style; the text is untouched.
    state.dialog_outcome(Outcome::Button("edit"), &mut clip);
    let editor = &mut state.dialogs[1].1;
    editor.set_field("size", "18");
    editor.set_on("bold", true);
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert_eq!(state.dialogs.len(), 1, "back in Character Styles");
    let format = state.view.doc.styles.character(1).unwrap().format;
    assert_eq!(format.mask, (1 << 1) | (1 << 2));
    assert_eq!(format.props.size, 36);
    assert_eq!(
        state.view.doc.main.paras[0].chp_of_char(0).character_style,
        0
    );
    assert_eq!(
        state.dialogs[0].1.item_label("description"),
        "Sets: 18 pt, Bold"
    );

    // + makes a new style; a name already in use keeps the editor open.
    state.dialog_outcome(Outcome::Button("new"), &mut clip);
    assert!(matches!(
        state.dialogs[1].0,
        Which::CharacterStyleEdit(None)
    ));
    state.dialogs[1].1.set_field("name", "Callout");
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert!(state.dialogs.len() > 2, "an alert over the editor");
    state.dialogs.truncate(2);
    state.dialogs[1].1.set_field("name", "Aside");
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert_eq!(state.dialogs.len(), 1);
    assert_eq!(state.view.doc.styles.find_character("Aside"), Some(2));
    assert_eq!(
        crate::character_style_dialog::selected_style(&state.dialogs[0].1),
        2
    );
    state.view.doc.undo();
    state.view.doc.undo();
    assert_eq!(quill_formats::native::write(&state.view.doc, 0), original);
}

#[test]
fn character_style_dialog_edits_stay_pending_and_cancel_does_not_add_fonts() {
    let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
    let original = quill_formats::native::write(&state.view.doc, 0);
    let (d, st) = crate::style_dialog::editor(&state.view, Some(state.view.current_pap().style));
    state.dialogs.push((Which::Style(st), d));
    let mut clip = Clipboard::new();
    state.dialog_outcome(Outcome::Button("character"), &mut clip);
    let editor = &mut state.dialogs.last_mut().unwrap().1;
    editor.set_options("font", vec!["A pending font".into()], 0);
    editor.set_field("size", "18");
    editor.set_on("bold", true);
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert!(matches!(&state.dialogs[0].0, Which::Style(st)
        if st.chp.bold && st.chp.size == 36 && st.fonts.contains(&"A pending font".into())));
    assert_eq!(quill_formats::native::write(&state.view.doc, 0), original);
    state.dialog_outcome(Outcome::Cancel, &mut clip);
    assert_eq!(quill_formats::native::write(&state.view.doc, 0), original);
    assert!(!state.view.doc.is_modified());

    let (d, st) = crate::style_dialog::editor(&state.view, Some(state.view.current_pap().style));
    state.dialogs.push((Which::Style(st), d));
    state.dialog_outcome(Outcome::Button("character"), &mut clip);
    state.dialogs.last_mut().unwrap().1.set_field("size", "22");
    state.dialog_outcome(Outcome::Cancel, &mut clip);
    assert_eq!(quill_formats::native::write(&state.view.doc, 0), original);
    assert!(!state.view.doc.is_modified());
}

#[test]
fn character_editor_defines_manual_defaults_and_apply_applies_the_defined_style() {
    let doc = quill_core::text::from_plain_text("Body text");
    let mut state = WinState::new(DocView::new(doc, "Doc".into()));
    let (d, st) = crate::style_dialog::editor(&state.view, None);
    state.dialogs.push((Which::Style(st), d));
    let mut clip = Clipboard::new();
    state
        .dialogs
        .last_mut()
        .unwrap()
        .1
        .set_field("name", "Large body");
    state.dialog_outcome(Outcome::Button("character"), &mut clip);
    let editor = &mut state.dialogs.last_mut().unwrap().1;
    editor.set_field("size", "18");
    editor.set_on("bold", true);
    editor.set_options("font", vec!["A new style font".into()], 0);
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert!(!state.view.doc.fonts.contains(&"A new style font".into()));
    define_in_editor(&mut state);
    let id = state.view.doc.styles.find("Large body").unwrap();
    assert_eq!(state.view.doc.styles.get(id).chp.size, 36);
    assert!(state.view.doc.styles.get(id).chp.bold);
    assert_eq!(
        state
            .view
            .doc
            .font_name(state.view.doc.styles.get(id).chp.font),
        "A new style font"
    );
    assert_eq!(
        state.view.doc.main.paras[0].pap.style, 0,
        "Define does not apply the new style"
    );
    assert_eq!(state.dialogs.len(), 1, "Define keeps the dialog open");
    state.dialog_outcome(Outcome::Apply, &mut clip);
    assert_eq!(state.dialogs.len(), 1, "Apply keeps the editor open");
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert!(state.dialogs.is_empty());
    assert_eq!(state.view.doc.main.paras[0].pap.style, id);
    assert_eq!(state.view.doc.main.paras[0].chp_of_char(0).size, 36);
    assert!(state.view.doc.main.paras[0].chp_of_char(0).bold);
    let bytes = quill_formats::native::write(&state.view.doc, 0);
    let saved = quill_formats::native::read(&bytes).unwrap();
    assert_eq!(saved.styles.get(id).chp, state.view.doc.styles.get(id).chp);
}

#[test]
fn choosing_a_character_style_in_format_style_links_it_and_no_style_unlinks() {
    let mut doc = quill_core::text::from_plain_text("Body text");
    doc.add_character_style(
        "Emphasis",
        quill_core::character_styles::CharacterFormat::new(
            quill_core::CharProps {
                italic: true,
                ..Default::default()
            },
            1 << 3,
        ),
    )
    .unwrap();
    doc.forget_history();
    let original = quill_formats::native::write(&doc, 0);
    let mut state = WinState::new(DocView::new(doc, "Doc".into()));
    let (mut d, st) =
        crate::style_dialog::editor(&state.view, Some(state.view.current_pap().style));
    let manual = st.chp;
    assert_eq!(
        d.options("character_style"),
        &["No Style", "Emphasis", "New Character Style\u{2026}"]
    );
    d.set_selected("character_style", 1);
    state.dialogs.push((Which::Style(st), d));
    let mut clip = Clipboard::new();
    state.dialog_outcome(Outcome::Changed("character_style"), &mut clip);
    let struck = |state: &WinState| {
        state.dialogs[0]
            .1
            .items
            .iter()
            .filter(|item| item.id.starts_with("char") && item.id.len() == 5)
            .all(|item| item.struck)
    };
    assert!(struck(&state), "the manual values show, struck through");
    assert!(
        state.dialogs[0]
            .1
            .items
            .iter()
            .filter(|item| ["ok", "apply", "define"].contains(&item.id))
            .all(|item| item.enabled),
        "saving works with a style chosen"
    );
    assert!(
        state.dialogs[0]
            .1
            .item_label("character_status")
            .starts_with("Linked to Emphasis (Italic).")
    );

    // Define saves the link; the text in that style takes Italic.
    define_in_editor(&mut state);
    let normal = state.view.doc.styles.get(0);
    assert_eq!(normal.character, 1);
    assert_eq!(normal.own(), manual);
    assert!(state.view.doc.main.paras[0].chp_of_char(0).italic);
    // Choosing the style again shows its link.
    let Which::Style(st) = &state.dialogs[0].0 else {
        unreachable!()
    };
    assert_eq!(st.character, 1);
    assert_eq!(state.dialogs[0].1.selected("character_style"), 1);

    // No Style brings the manual formatting back.
    state.dialogs[0].1.set_selected("character_style", 0);
    state.dialog_outcome(Outcome::Changed("character_style"), &mut clip);
    assert!(!struck(&state));
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert!(state.dialogs.is_empty());
    assert_eq!(state.view.doc.styles.get(0).character, 0);
    assert!(!state.view.doc.main.paras[0].chp_of_char(0).italic);
    state.view.doc.undo();
    state.view.doc.undo();
    assert_eq!(quill_formats::native::write(&state.view.doc, 0), original);
}

#[test]
fn paragraph_style_editor_stages_changes_and_cancel_keeps_the_document_clean() {
    let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
    let (d, st) = crate::style_dialog::editor(&state.view, Some(state.view.current_pap().style));
    state.dialogs.push((Which::Style(st), d));
    let original = state.view.doc.styles.clone();
    let mut clip = Clipboard::new();
    state.dialog_outcome(Outcome::Button("paragraph"), &mut clip);
    state
        .dialogs
        .last_mut()
        .unwrap()
        .1
        .set_field("before", "18 pt");
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert_eq!(state.dialogs.len(), 1);
    assert_eq!(state.view.doc.styles, original);
    assert!(!state.view.doc.is_modified());
    assert!(matches!(&state.dialogs[0].0, Which::Style(st) if st.pap.before == 360));
    state.dialog_outcome(Outcome::Cancel, &mut clip);
    assert!(state.dialogs.is_empty());
    assert_eq!(state.view.doc.styles, original);
    assert!(!state.view.doc.is_modified());

    let (d, st) = crate::style_dialog::editor(&state.view, Some(state.view.current_pap().style));
    state.dialogs.push((Which::Style(st), d));
    state.dialog_outcome(Outcome::Button("paragraph"), &mut clip);
    state
        .dialogs
        .last_mut()
        .unwrap()
        .1
        .set_field("before", "18 pt");
    state.dialog_outcome(Outcome::Cancel, &mut clip);
    assert_eq!(state.view.doc.styles, original);
    assert!(!state.view.doc.is_modified());
    assert!(state.view.doc.undo_label().is_none());
}

#[test]
fn defining_paragraph_format_changes_the_chosen_style_and_undo_restores_it() {
    let mut doc = quill_core::text::from_plain_text("Body\nHeading one\nHeading two");
    let heading = doc.styles.find("heading 1").unwrap();
    doc.apply_style(quill_core::StoryId::Main, 1, 2, heading);
    doc.forget_history();
    let original = quill_formats::native::write(&doc, 0);
    let mut state = WinState::new(DocView::new(doc, "Doc".into()));
    let (d, st) = crate::style_dialog::editor(&state.view, Some(heading));
    state.dialogs.push((Which::Style(st), d));
    let mut clip = Clipboard::new();
    let body = state.view.doc.main.paras[0].clone();
    let font = state.view.doc.styles.get(heading).chp;
    state.dialog_outcome(Outcome::Button("paragraph"), &mut clip);
    let editor = &mut state.dialogs.last_mut().unwrap().1;
    for (id, value) in [
        ("before", "18 pt"),
        ("after", "6 pt"),
        ("left", "0.5 in"),
        ("right", "0.25 in"),
        ("first", "0.125 in"),
        ("line_by", "18"),
    ] {
        editor.set_field(id, value);
    }
    editor.set_selected("line", 2);
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert_eq!(state.view.doc.main.paras[0], body);
    assert!(!state.view.doc.is_modified());
    // OK saves the style and closes, without restyling the selection.
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert!(state.dialogs.is_empty());
    let style = state.view.doc.styles.get(heading);
    assert_eq!(style.chp, font);
    assert_eq!(
        (
            style.pap.before,
            style.pap.after,
            style.pap.left,
            style.pap.right,
            style.pap.first
        ),
        (360, 120, 720, 360, 180)
    );
    assert_eq!(style.pap.line, quill_core::LineSpacing::Exactly(360));
    for para in &state.view.doc.main.paras[1..] {
        assert_eq!(para.pap, style.pap);
    }
    assert_eq!(state.view.doc.main.paras[0], body);
    assert!(state.view.doc.is_modified());
    state.view.doc.undo();
    assert_eq!(quill_formats::native::write(&state.view.doc, 0), original);
    assert!(!state.view.doc.is_modified());
}

#[test]
fn global_character_style_changes_refresh_inactive_headers_without_copying_old_text_back() {
    use quill_core::character_styles::CharacterFormat;
    let mut doc = Document::new();
    doc.headers[0] = quill_core::text::from_plain_text("Header").main;
    let format = CharacterFormat::new(
        quill_core::CharProps {
            italic: true,
            ..Default::default()
        },
        1 << 3,
    );
    let id = doc.add_character_style("Emphasis", format).unwrap();
    doc.apply_character_style(
        quill_core::StoryId::Header,
        Position::new(0, 0),
        Position::new(0, 6),
        id,
    );
    let mut state = WinState::new(DocView::new(doc, "Test".into()));
    state.open_pane(quill_core::StoryId::Header);
    state.focus_main();
    let format = CharacterFormat::new(
        quill_core::CharProps {
            italic: true,
            color: quill_core::Rgb(0xCC2200),
            ..Default::default()
        },
        (1 << 3) | (1 << 11),
    );
    state.view.doc.redefine_character_style(id, format);
    state.sync_pane();
    state.focus_pane();
    assert_eq!(
        state.view.doc.main.paras[0].chp_of_char(0).color,
        quill_core::Rgb(0xCC2200)
    );
}

#[test]
fn pasted_character_styles_in_a_header_are_imported_with_their_definitions() {
    use quill_core::character_styles::CharacterFormat;
    let mut doc = Document::new();
    doc.headers[0] = quill_core::text::from_plain_text("Header").main;
    let mut state = WinState::new(DocView::new(doc, "Test".into()));
    state.open_pane(quill_core::StoryId::Header);
    let id = state
        .view
        .doc
        .add_character_style(
            "Pasted",
            CharacterFormat::new(
                quill_core::CharProps {
                    italic: true,
                    ..Default::default()
                },
                1 << 3,
            ),
        )
        .unwrap();
    state.view.doc.apply_character_style(
        quill_core::StoryId::Main,
        Position::new(0, 0),
        Position::new(0, 6),
        id,
    );
    state.sync_pane();
    let main = state.main_view();
    let id = main.doc.styles.find_character("Pasted").unwrap();
    assert_eq!(
        main.doc.headers[0].paras[0].chp_of_char(0).character_style,
        id
    );
    assert!(main.doc.headers[0].paras[0].chp_of_char(0).italic);
}

#[test]
fn a_recovered_window_is_saved_under_its_own_name() {
    let title = format!("Long.qwp{RECOVERED}");
    assert_eq!(title, "Long.qwp - Recovered");
    assert_eq!(strip_ext(&title), "Long");
    assert_eq!(strip_ext(&format!("Untitled1{RECOVERED}")), "Untitled1");
    assert_eq!(strip_ext("Notes.docx"), "Notes");
}

fn paint_editor(state: &mut WinState, width: u32, height: u32, fonts: &mut Fonts) {
    let mut cache = GlyphCache::new();
    let env = Env {
        font_names: &[],
        font_families: &[],
        recent: &[],
        windows: &[],
    };
    let mut pixels = vec![0; (width * height) as usize];
    state.paint(
        &mut Canvas::new(&mut pixels, width, height, 1.0),
        fonts,
        &mut cache,
        &env,
        true,
    );
}

#[test]
fn comment_sidebar_resizes_scrolls_independently_and_keeps_edits() {
    use quill_core::{StoryId, notes::AnnotationInfo};
    let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
        return;
    };
    let mut doc = quill_core::text::from_plain_text(&"Body paragraph\n".repeat(100));
    let id = doc.insert_annotation(
        Position::new(0, 4),
        Default::default(),
        AnnotationInfo {
            author: "Lucas".into(),
            initials: "L".into(),
            ..Default::default()
        },
    );
    let at = doc.annotation_text_start(id).unwrap();
    doc.insert_text(
        StoryId::Annotations,
        at,
        &"Long comment text. ".repeat(150),
        Default::default(),
    );
    let mut state = WinState::new(DocView::new(doc, "Review".into()));
    state.document_zoom = 1.0;
    state.open_note_in(StoryId::Annotations, id);
    paint_editor(&mut state, 900, 700, &mut fonts);
    let panel = state.pane.as_ref().unwrap().bounds;
    assert_eq!(panel.right(), 900.0 - BAR);
    assert_eq!(panel.y, state.text_top());
    assert!(state.main_view().area.right() <= panel.x);
    assert!(state.pane_view().unwrap().max_scroll() > 0.0);
    state.pointer = (20.0, 200.0);
    state.wheel(60.0);
    let body_scroll = state.main_view().scroll_y;
    assert!(body_scroll > 0.0);
    assert_eq!(state.pane_view().unwrap().scroll_y, 0.0);
    state.pointer = (panel.x + 20.0, panel.y + 140.0);
    state.wheel(60.0);
    assert!(state.pane_view().unwrap().scroll_y > 0.0);
    assert_eq!(state.main_view().scroll_y, body_scroll);
    state.view.type_text("Edited comment. ");
    state.focus_main();
    assert!(
        state
            .main_view()
            .doc
            .annotation_text(id)
            .contains("Edited comment.")
    );
    paint_editor(&mut state, 480, 700, &mut fonts);
    let panel = state.pane.as_ref().unwrap().bounds;
    assert_eq!(panel.right(), 480.0 - BAR);
    assert!(state.pane_view().unwrap().area.w < 240.0);
    assert_eq!(
        pane::hit(
            panel,
            StoryId::Annotations,
            false,
            panel.right() - 20.0,
            panel.y + 16.0
        ),
        PaneHit::Close
    );
    state.native_scrollbars = true;
    paint_editor(&mut state, 480, 700, &mut fonts);
    assert!(state.pane_view().unwrap().area.right() <= state.vscroll_other.x);
    assert!(state.main_view().area.right() <= state.vscroll.x);
    state.close_pane();
    paint_editor(&mut state, 480, 700, &mut fonts);
    assert!(state.pane.is_none());
    assert!(
        state
            .view
            .doc
            .annotation_text(id)
            .contains("Edited comment.")
    );
}

#[test]
fn shared_review_filters_preserve_comment_edits_and_badges_reopen_the_pane() {
    use quill_core::notes::AnnotationInfo;
    let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
        return;
    };
    let mut doc = quill_core::text::from_plain_text("first second");
    let comment = doc.insert_annotation_on(
        Position::new(0, 0),
        Position::new(0, 5),
        Default::default(),
        AnnotationInfo {
            author: "Alex".into(),
            ..Default::default()
        },
    );
    let at = doc.annotation_text_start(comment).unwrap();
    doc.insert_text(
        quill_core::StoryId::Annotations,
        at,
        "Original comment",
        Default::default(),
    );
    for (word, author) in [("first", "Ada"), ("second", "Grace")] {
        let a = doc.main.paras[0].text.find(word).unwrap();
        let b = a + word.len();
        let id = quill_core::revision::intern(quill_core::revision::Revision {
            inserted: Some(quill_core::revision::Stamp::new(author, "2026-10-23")),
            ..Default::default()
        });
        doc.format_chars(
            quill_core::StoryId::Main,
            Position::new(0, a),
            Position::new(0, b),
            |c| c.rev = id,
        );
    }
    let mut state = WinState::new(DocView::new(doc, "Review".into()));
    paint_editor(&mut state, 900, 700, &mut fonts);
    assert_eq!(
        state.pane.as_ref().unwrap().story,
        quill_core::StoryId::Annotations
    );
    let panel = state.pane.as_ref().unwrap().bounds;
    assert!(state.pane_view().unwrap().area.y > panel.y + 100.0);
    state.focus_pane();
    state.view.set_selection(Selection::caret(at));
    state.view.insert_text("Edited ");
    state.revision_click(panel.x + panel.w * 0.85, panel.y + 45.0);
    assert!(state.review_filter == revisions::Filter::Changes);
    paint_editor(&mut state, 900, 700, &mut fonts);
    assert_eq!(state.pane_view().unwrap().area.h, 0.0);
    assert!(
        state
            .main_view()
            .doc
            .annotation_text(comment)
            .contains("Edited Original")
    );
    state.revision_click(panel.x + panel.w * 0.5, panel.y + 45.0);
    paint_editor(&mut state, 900, 700, &mut fonts);
    assert!(state.review_filter == revisions::Filter::Comments);
    assert!(state.pane_view().unwrap().area.h > 0.0);
    assert_eq!(state.revision_badges.len(), 1);
    assert!(state.revision_badges[0].tip.contains("Ada"));
    assert!(state.revision_badges[0].tip.contains("Grace"));
    state.revision_hover = Some(0);
    state.retip(0.0, 0.0);
    assert!(state.tip.as_ref().unwrap().text.contains("Inserted by Ada"));
    assert!(
        state
            .tip
            .as_ref()
            .unwrap()
            .text
            .contains("Inserted by Grace")
    );
    let badge = state.revision_badges[0].rect;
    state.revision_click(badge.x + 5.0, badge.y + 5.0);
    assert!(state.review_filter == revisions::Filter::All);
    let first = state.main_view().change_here().unwrap();
    state.revision_click(badge.x + 5.0, badge.y + 5.0);
    assert_ne!(state.main_view().change_here().unwrap(), first);
    state.close_pane();
    paint_editor(&mut state, 480, 700, &mut fonts);
    assert!(state.pane.is_none());
    let badge = state.revision_badges[0].rect;
    assert!(badge.x >= state.main_view().area.right() * state.document_zoom / state.ui);
    state.revision_click(badge.x + 5.0, badge.y + 5.0);
    paint_editor(&mut state, 480, 700, &mut fonts);
    assert!(state.pane.is_some());
    assert!(
        state
            .main_view()
            .doc
            .annotation_text(comment)
            .contains("Edited Original")
    );
    state.main_view_mut().show_changes = false;
    paint_editor(&mut state, 480, 700, &mut fonts);
    assert!(state.revision_badges.is_empty());
}

#[test]
fn review_accept_reject_keeps_comments_and_is_undoable() {
    let mut view = DocView::new(
        quill_core::text::from_plain_text("Anchor "),
        "Review".into(),
    );
    view.set_selection(Selection::caret(Position::new(0, 7)));
    view.doc.props.track_changes = true;
    view.doc.author = "Ada".into();
    view.insert_text("inserted");
    view.doc.props.track_changes = false;
    let id = view.doc.insert_annotation(
        Position::new(0, 0),
        Default::default(),
        quill_core::notes::AnnotationInfo {
            author: "Grace".into(),
            ..Default::default()
        },
    );
    let at = view.doc.annotation_text_start(id).unwrap();
    view.doc.insert_text(
        quill_core::StoryId::Annotations,
        at,
        "Keep this discussion",
        Default::default(),
    );
    view.doc.forget_history();
    let mut state = WinState::new(view);
    state.width = 900.0;
    state.height = 700.0;
    state.ensure_review();
    let r = state.revisions_bounds().unwrap();
    let buttons = revisions::review_buttons(r);
    let center = |b: Rect| (b.x + b.w / 2.0, b.y + b.h / 2.0);
    let (x, y) = center(buttons[2]);
    state.revision_click(x, y);
    assert!(!state.main_view().doc.has_changes());
    assert_eq!(
        state.main_view().doc.annotation_text(id),
        "Keep this discussion"
    );
    assert!(state.main_view_mut().doc.undo().is_some());
    assert!(state.main_view().doc.has_changes());
    let (x, y) = center(buttons[3]);
    state.revision_click(x, y);
    assert!(!state.main_view().doc.has_changes());
    assert!(
        state.main_view().doc.main.paras[0]
            .text
            .contains(quill_core::para::chars::ANNOTATION)
    );
    assert_eq!(
        state.main_view().doc.annotation_text(id),
        "Keep this discussion"
    );
}

#[test]
fn review_divider_drag_resizes_both_sides_without_editing() {
    let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
        return;
    };
    for ui in [1.0, 1.5] {
        let mut doc = quill_core::text::from_plain_text("Document body");
        let id = doc.insert_annotation(Position::new(0, 0), Default::default(), Default::default());
        let at = doc.annotation_text_start(id).unwrap();
        doc.insert_text(
            quill_core::StoryId::Annotations,
            at,
            "Comment text",
            Default::default(),
        );
        doc.forget_history();
        let mut state = WinState::new(DocView::new(doc, "Review".into()));
        state.ui = ui;
        state.document_zoom = 1.25;
        paint_editor(&mut state, 900, 700, &mut fonts);
        let before = state.pane.as_ref().unwrap().bounds;
        let body_width = state.main_view().area.w;
        let main = state.main_view().doc.main.clone();
        let modified = state.main_view().doc.is_modified();
        let env = Env {
            font_names: &[],
            font_families: &[],
            recent: &[],
            windows: &[],
        };
        let mut cache = GlyphCache::new();
        let mut label = Label {
            fonts: &mut fonts,
            cache: &mut cache,
        };
        let mut clip = Clipboard::new();
        let x = before.x * ui;
        let y = (before.y + 120.0) * ui;
        assert_eq!(state.cursor_icon(x, y), CursorIcon::ColResize);
        state.mouse_down(x, y, Mods::default(), &mut label, &env, &mut clip);
        assert!(state.review_resize.is_some());
        state.mouse_move(x - 40.0 * ui, y, &mut label, &env);
        state.mouse_up(x - 40.0 * ui, y, &mut clip);
        assert!(state.review_resize.is_none());
        paint_editor(&mut state, 900, 700, &mut fonts);
        let after = state.pane.as_ref().unwrap().bounds;
        assert!((after.w - before.w - 40.0).abs() < 0.01);
        assert!(state.main_view().area.w < body_width);
        assert!(state.pane_view().unwrap().area.w > 0.0);
        assert_eq!(state.main_view().doc.main, main);
        assert_eq!(state.main_view().doc.annotation_text(id), "Comment text");
        assert_eq!(state.main_view().doc.is_modified(), modified);
        assert!(state.main_view().doc.undo_label().is_none());
        state.close_pane();
        state.open_pane(quill_core::StoryId::Annotations);
        paint_editor(&mut state, 900, 700, &mut fonts);
        assert_eq!(state.pane.as_ref().unwrap().bounds.w, after.w);
        state.review_resize = Some(0.0);
        state.resize_review(-10000.0);
        assert!(state.revisions_bounds().unwrap().x >= 240.0);
        state.resize_review(10000.0);
        assert_eq!(state.revisions_bounds().unwrap().w, 200.0);
        state.review_resize = None;
    }
}

#[test]
#[ignore = "manual divider redraw timing; run with --release --ignored --nocapture"]
fn review_divider_redraw_timing() {
    let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
        return;
    };
    let mut doc = quill_core::text::from_plain_text(&"Document paragraph.\n".repeat(30));
    for _ in 0..20 {
        let id = doc.insert_annotation(Position::new(0, 0), Default::default(), Default::default());
        let at = doc.annotation_text_start(id).unwrap();
        doc.insert_text(
            quill_core::StoryId::Annotations,
            at,
            &"Review comment with enough text to wrap at several widths. ".repeat(35),
            Default::default(),
        );
    }
    doc.forget_history();
    let mut state = WinState::new(DocView::new(doc, "Resize timing".into()));
    state.document_zoom = 1.0;
    state.review_filter = revisions::Filter::Comments;
    let env = Env {
        font_names: &[],
        font_families: &[],
        recent: &[],
        windows: &[],
    };
    let mut cache = GlyphCache::new();
    let mut pixels = vec![0; 1200 * 800];
    state.paint(
        &mut Canvas::new(&mut pixels, 1200, 800, 1.0),
        &mut fonts,
        &mut cache,
        &env,
        true,
    );
    let panel = state.revisions_bounds().unwrap();
    state.mouse_down(
        panel.x,
        panel.y + 120.0,
        Mods::default(),
        &mut Label {
            fonts: &mut fonts,
            cache: &mut cache,
        },
        &env,
        &mut Clipboard::new(),
    );
    let mut timings = Vec::new();
    for i in 0..24 {
        let start = Instant::now();
        state.mouse_move(
            550.0 + (i * 17 % 200) as f32,
            panel.y + 120.0,
            &mut Label {
                fonts: &mut fonts,
                cache: &mut cache,
            },
            &env,
        );
        state.paint(
            &mut Canvas::new(&mut pixels, 1200, 800, 1.0),
            &mut fonts,
            &mut cache,
            &env,
            true,
        );
        timings.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    timings.sort_by(f64::total_cmp);
    eprintln!(
        "Divider redraw: mean {:.2} ms, p95 {:.2} ms",
        timings.iter().sum::<f64>() / timings.len() as f64,
        timings[22]
    );
}

#[test]
fn divider_moves_live_but_rewraps_comments_only_on_release() {
    let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
        return;
    };
    let mut doc = quill_core::text::from_plain_text("Body text");
    let id = doc.insert_annotation(Position::new(0, 0), Default::default(), Default::default());
    let at = doc.annotation_text_start(id).unwrap();
    let text = "A longer comment to check wrapping during divider movement. ".repeat(60);
    doc.insert_text(
        quill_core::StoryId::Annotations,
        at,
        &text,
        Default::default(),
    );
    doc.forget_history();
    let mut state = WinState::new(DocView::new(doc, "Review".into()));
    state.document_zoom = 1.0;
    state.open_note_in(quill_core::StoryId::Annotations, id);
    paint_editor(&mut state, 900, 700, &mut fonts);
    let panel = state.revisions_bounds().unwrap();
    let old_height = state.pane_view().unwrap().total_height();
    let env = Env {
        font_names: &[],
        font_families: &[],
        recent: &[],
        windows: &[],
    };
    let mut cache = GlyphCache::new();
    let mut clip = Clipboard::new();
    state.mouse_down(
        panel.x,
        panel.y + 120.0,
        Mods::default(),
        &mut Label {
            fonts: &mut fonts,
            cache: &mut cache,
        },
        &env,
        &mut clip,
    );
    for x in [panel.x - 160.0, panel.x + 60.0] {
        state.mouse_move(
            x,
            panel.y + 120.0,
            &mut Label {
                fonts: &mut fonts,
                cache: &mut cache,
            },
            &env,
        );
        paint_editor(&mut state, 900, 700, &mut fonts);
        assert!(
            (state.revisions_bounds().unwrap().x - x).abs() < 0.01,
            "border must follow each pointer position"
        );
        assert_eq!(
            state.pane_view().unwrap().total_height(),
            old_height,
            "dragging must reuse the comment layout"
        );
    }
    // Editing during a held drag must still invalidate and prepare safely.
    state.view.type_text("Added ");
    paint_editor(&mut state, 900, 700, &mut fonts);
    state.mouse_up(panel.x + 60.0, panel.y + 120.0, &mut clip);
    assert!(state.review_resize_text_width.is_none());
    paint_editor(&mut state, 900, 700, &mut fonts);
    assert!(
        state.pane_view().unwrap().total_height() > old_height,
        "release must wrap to the narrower width"
    );
    state.close_pane();
    assert!(state.main_view().doc.annotation_text(id).contains("Added "));
    assert!(
        state
            .main_view()
            .doc
            .annotation_text(id)
            .contains(text.trim())
    );
}

#[test]
fn every_divider_grab_point_is_clear_of_the_native_scrollbar() {
    let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
        return;
    };
    let env = Env {
        font_names: &[],
        font_families: &[],
        recent: &[],
        windows: &[],
    };
    let mut cache = GlyphCache::new();
    let mut clip = Clipboard::new();
    for ui in [1.0, 1.5, 2.0] {
        for native in [false, true] {
            for grab in [-7.0, -4.0, -1.0, 3.0] {
                let mut doc = quill_core::text::from_plain_text("Document body");
                doc.insert_annotation(Position::new(0, 0), Default::default(), Default::default());
                let mut state = WinState::new(DocView::new(doc, "Review".into()));
                state.ui = ui;
                state.native_scrollbars = native;
                paint_editor(&mut state, 1200, 800, &mut fonts);
                let r = state.revisions_bounds().unwrap();
                // mac_widgets positions the native NSScrollView by this
                // right edge, even while its overlay thumb is invisible.
                let native_right = state.vscroll.right() * ui;
                let native_left = state.vscroll.x * ui;
                let grab_left = (r.x - revisions::DIVIDER_WIDTH) * ui;
                let grab_right = (r.x + 4.0) * ui;
                assert!(
                    native_right <= grab_left || native_left >= grab_right,
                    "native scroll view must not cover any of the grab strip"
                );
                let (x, y) = ((r.x + grab) * ui, (r.y + 120.0) * ui);
                assert_eq!(state.cursor_icon(x, y), CursorIcon::ColResize);
                state.mouse_down(
                    x,
                    y,
                    Mods::default(),
                    &mut Label {
                        fonts: &mut fonts,
                        cache: &mut cache,
                    },
                    &env,
                    &mut clip,
                );
                assert!(
                    state.review_resize.is_some(),
                    "the first click must grab the divider"
                );
                state.mouse_move(
                    x - 30.0 * ui,
                    y,
                    &mut Label {
                        fonts: &mut fonts,
                        cache: &mut cache,
                    },
                    &env,
                );
                state.mouse_up(x - 30.0 * ui, y, &mut clip);
                assert!((state.revisions_bounds().unwrap().x - (r.x - 30.0)).abs() < 0.01);
            }
        }
    }
}

#[test]
fn review_resizing_keeps_document_scrollbar_at_the_window_edge() {
    let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
        return;
    };
    let env = Env {
        font_names: &[],
        font_families: &[],
        recent: &[],
        windows: &[],
    };
    let mut cache = GlyphCache::new();
    let mut clip = Clipboard::new();
    for native in [false, true] {
        let mut doc = quill_core::text::from_plain_text(&"Body paragraph\n".repeat(100));
        let id = doc.insert_annotation(Position::new(0, 0), Default::default(), Default::default());
        let at = doc.annotation_text_start(id).unwrap();
        doc.insert_text(
            quill_core::StoryId::Annotations,
            at,
            &"Comment text. ".repeat(200),
            Default::default(),
        );
        let mut state = WinState::new(DocView::new(doc, "Review".into()));
        state.native_scrollbars = native;
        state.document_zoom = 1.0;
        paint_editor(&mut state, 900, 700, &mut fonts);
        let bar = state.vscroll;
        let comments_bar_x = state.vscroll_other.x;
        assert_eq!(bar.right(), 900.0);
        assert_eq!(state.revisions_bounds().unwrap().right(), bar.x);
        let panel = state.revisions_bounds().unwrap();
        state.mouse_down(
            panel.x,
            panel.y + 120.0,
            Mods::default(),
            &mut Label {
                fonts: &mut fonts,
                cache: &mut cache,
            },
            &env,
            &mut clip,
        );
        for x in [panel.x - 100.0, panel.x + 60.0] {
            state.mouse_move(
                x,
                panel.y + 120.0,
                &mut Label {
                    fonts: &mut fonts,
                    cache: &mut cache,
                },
                &env,
            );
            paint_editor(&mut state, 900, 700, &mut fonts);
            assert_eq!(
                state.vscroll, bar,
                "document scrollbar must not follow the divider"
            );
            assert_eq!(state.vscroll_other.x, comments_bar_x);
            assert!((state.revisions_bounds().unwrap().x - x).abs() < 0.01);
        }
        state.mouse_up(panel.x + 60.0, panel.y + 120.0, &mut clip);
        paint_editor(&mut state, 900, 700, &mut fonts);
        // The rightmost control scrolls the document, even if comments
        // previously had keyboard focus. Wheel scrolling in Review stays local.
        let comments = state.pane_view().unwrap().area;
        state.pointer = (comments.x + 20.0, comments.y + 30.0);
        state.wheel(60.0);
        let comment_scroll = state.pane_view().unwrap().scroll_y;
        assert!(comment_scroll > 0.0);
        assert_eq!(state.main_view().scroll_y, 0.0);
        if !native {
            state.focus_pane();
            let (x, y) = (bar.x + BAR / 2.0, bar.bottom() - 3.0);
            state.mouse_down(
                x,
                y,
                Mods::default(),
                &mut Label {
                    fonts: &mut fonts,
                    cache: &mut cache,
                },
                &env,
                &mut clip,
            );
            state.mouse_up(x, y, &mut clip);
            assert!(state.main_view().scroll_y > 0.0);
            assert_eq!(state.pane_view().unwrap().scroll_y, comment_scroll);
        }
        state.close_pane();
        paint_editor(&mut state, 900, 700, &mut fonts);
        assert_eq!(state.vscroll.x, bar.x);
    }
}

#[cfg(target_os = "macos")]
fn assert_visual_baseline(
    name: &str,
    width: u32,
    height: u32,
    scale: f32,
    script: &str,
    expected: &[u8],
) {
    let path = std::env::temp_dir().join(format!("quill-visual-{name}-{}.png", std::process::id()));
    crate::settings::update(|settings| *settings = crate::settings::Settings::default());
    snapshot(&path, width, height, scale, None, script);
    let actual = std::fs::read(&path).expect("read rendered visual snapshot");
    let _ = std::fs::remove_file(path);
    let (ew, eh, expected) = crate::image::decode_rgba(expected, quill_core::ImageFormat::Png)
        .expect("decode visual baseline");
    let (aw, ah, actual) = crate::image::decode_rgba(&actual, quill_core::ImageFormat::Png)
        .expect("decode rendered visual snapshot");
    assert_eq!((aw, ah), (ew, eh), "{name} dimensions changed");

    let mut changed = 0usize;
    let mut total_delta = 0u64;
    let (expected, expected_tail) = expected.as_chunks::<4>();
    let (actual, actual_tail) = actual.as_chunks::<4>();
    assert!(expected_tail.is_empty() && actual_tail.is_empty());
    for (before, after) in expected.iter().zip(actual) {
        let delta = before[..3]
            .iter()
            .zip(&after[..3])
            .map(|(&a, &b)| a.abs_diff(b) as u64)
            .sum::<u64>();
        total_delta += delta;
        changed += usize::from(delta > 24);
    }
    let pixels = (aw as usize) * (ah as usize);
    let changed_fraction = changed as f64 / pixels as f64;
    let mean_channel_delta = total_delta as f64 / (pixels * 3) as f64;
    assert!(
        changed_fraction <= 0.025 && mean_channel_delta <= 1.5,
        "{name} changed visually: {:.2}% pixels, mean channel delta {:.2}",
        changed_fraction * 100.0,
        mean_channel_delta
    );
}

#[cfg(target_os = "macos")]
#[test]
fn representative_window_visuals_match_baselines() {
    let sample = "type:Quill visual baseline\\nThe quick brown fox jumps over the lazy dog.";
    assert_visual_baseline(
        "compact-100",
        640,
        520,
        1.0,
        &format!("zoom:100|{sample}"),
        include_bytes!("../../../../tests/visual/compact-100.png"),
    );
    assert_visual_baseline(
        "comfortable-retina-150",
        900,
        700,
        2.0,
        &format!("zoom:150|{sample}"),
        include_bytes!("../../../../tests/visual/comfortable-retina-150.png"),
    );
    assert_visual_baseline(
        "wide-200",
        1200,
        720,
        1.0,
        &format!("zoom:200|{sample}"),
        include_bytes!("../../../../tests/visual/wide-200.png"),
    );
}

#[test]
fn initial_window_uses_the_display_without_filling_it() {
    let laptop = initial_window_size(1512.0, 982.0);
    assert!((laptop.width - 1285.2).abs() < 0.01);
    assert!((laptop.height - 834.7).abs() < 0.01);

    let large = initial_window_size(2560.0, 1440.0);
    assert_eq!(large, LogicalSize::new(1400.0, 1000.0));

    let small = initial_window_size(640.0, 480.0);
    assert_eq!(small, LogicalSize::new(640.0, 480.0));
}

#[test]
fn document_zoom_and_pointer_coordinates_are_inverse() {
    let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
    state.document_zoom = 1.25;
    assert_eq!(state.document_point(125.0, 250.0), (100.0, 200.0));
}

#[cfg(target_os = "macos")]
#[test]
fn native_scrollbar_actions_move_and_clamp() {
    assert_eq!(native_scroll_value(200.0, 500.0, 100.0, 1, 0.0), 100.0);
    assert_eq!(native_scroll_value(200.0, 500.0, 100.0, 3, 0.0), 300.0);
    assert_eq!(native_scroll_value(200.0, 500.0, 100.0, 4, 0.0), 184.0);
    assert_eq!(native_scroll_value(200.0, 500.0, 100.0, 5, 0.0), 216.0);
    assert_eq!(native_scroll_value(200.0, 500.0, 100.0, 2, 0.75), 375.0);
    assert_eq!(native_scroll_value(10.0, 500.0, 100.0, 1, 0.0), 0.0);
    assert_eq!(native_scroll_value(490.0, 500.0, 100.0, 3, 0.0), 500.0);
}

#[test]
fn choosing_a_font_family_applies_its_normal_variant() {
    let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
    state.view.type_text("x");
    state.view.sel = Selection::new(Position::new(0, 0), Position::new(0, 1));
    state.view.format_chars("Formatting", |c| {
        c.bold = true;
        c.italic = true;
    });
    state.act(
        Action::FontVariant("Arial".into(), false, false),
        &mut Clipboard::new(),
    );
    let chp = state.view.current_chp();
    assert_eq!(state.view.doc.font_name(chp.font), "Arial");
    assert!(!chp.bold);
    assert!(!chp.italic);
}

#[test]
fn application_menu_actions_escape_the_portable_window() {
    let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
    let clip = &mut Clipboard::new();
    assert_eq!(state.act(Action::About, clip), Effect::About);
    assert_eq!(state.act(Action::Recent(2), clip), Effect::OpenRecent(2));
    assert_eq!(
        state.act(Action::RemoveRecent(1), clip),
        Effect::RemoveRecent(1)
    );
    assert_eq!(state.act(Action::ClearRecent, clip), Effect::ClearRecent);
}

#[test]
fn collapsed_replace_cannot_change_document_from_stale_actions() {
    let mut state = WinState::new(DocView::new(Document::new(), "Find".into()));
    state.view.insert_text("cat dog cat");
    let (mut d, mut st) = crate::find_dialog::dialog(&state.view, false);
    d.set_field("find", "cat");
    crate::find_dialog::switch_mode(&mut st, &mut d, &state.view, true);
    d.set_field("with", "bird");
    crate::find_dialog::switch_mode(&mut st, &mut d, &state.view, false);
    assert!(!d.items.iter().any(|i| matches!(
        i.id,
        "with" | "replace" | "replaceall" | "fmt_rep" | "spc_rep"
    )));
    state.dialogs.push((Which::Find(st), d));
    state
        .view
        .set_selection(quill_core::Selection::caret(Position::new(0, 0)));
    let mut clip = Clipboard::without_system();
    for outcome in [
        Outcome::Button("replace"),
        Outcome::Button("replaceall"),
        Outcome::Ok,
    ] {
        state.tool_outcome(&outcome, &mut clip);
        assert_eq!(state.view.doc.main.paras[0].text, "cat dog cat");
        assert_eq!(state.dialogs.len(), 1);
    }
    let (Which::Find(st), _) = &mut state.dialogs[0] else {
        panic!("expected Find")
    };
    assert_eq!(
        crate::find_dialog::replace(st, &mut state.view, &mut clip),
        crate::find_dialog::Hit::NotFound
    );
    assert_eq!(
        crate::find_dialog::replace_all(st, &mut state.view, &mut clip),
        (0, false)
    );
    assert_eq!(state.view.doc.main.paras[0].text, "cat dog cat");
    state.tool_outcome(&Outcome::Button("mode"), &mut clip);
    assert_eq!(state.dialogs[0].1.field_text("with"), "bird");
    state.tool_outcome(&Outcome::Button("replace"), &mut clip);
    assert_eq!(state.view.doc.main.paras[0].text, "bird dog cat");
}

#[test]
fn native_find_releases_document_input_after_a_search() {
    let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
        return;
    };
    let mut state = WinState::new(DocView::new(Document::new(), "Find".into()));
    state.view.insert_text("cat dog");
    let (mut d, mut st) = crate::find_dialog::dialog(&state.view, false);
    d.set_field("find", "cat");
    crate::find_dialog::update(&mut st, &mut d, &state.view);
    let search = st.search.clone();
    state.dialogs.push((Which::Find(st), d));
    state
        .view
        .set_selection(quill_core::Selection::caret(Position::new(0, 0)));
    let hit = crate::find_dialog::find_next(&mut state.view, &search);
    state.after_search(hit, &search);
    assert_eq!(hit, crate::find_dialog::Hit::Match);
    state.native_dialogs = true;
    let env = Env {
        font_names: &[],
        font_families: &[],
        recent: &[],
        windows: &[],
    };
    let mut cache = GlyphCache::new();
    let mut label = Label {
        fonts: &mut fonts,
        cache: &mut cache,
    };
    let mut clip = Clipboard::without_system();
    // This point lies inside the old canvas panel rectangle, but the
    // event came from the document, not the separate native panel.
    assert!(state.dialogs[0].1.contains(200.0, 120.0));
    state.mouse_down(
        200.0 * state.ui,
        120.0 * state.ui,
        Mods::default(),
        &mut label,
        &env,
        &mut clip,
    );
    assert!(!state.dialog_has_keys());
    assert!(state.dialogs[0].1.modeless);
}

#[test]
fn modal_dialogs_block_commands_but_find_remains_modeless() {
    let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
    assert!(!state.modal_dialog_open());

    let mut find = Dialog::new("Find", 100.0, 50.0);
    find.modeless = true;
    state.dialogs.push((Which::GoTo, find));
    assert!(!state.modal_dialog_open());
    assert!(state.dialog_has_keys());

    state.dialogs.last_mut().unwrap().1.inactive = true;
    assert!(!state.dialog_has_keys());

    state
        .dialogs
        .push((Which::GoTo, Dialog::new("Character", 100.0, 50.0)));
    assert!(state.modal_dialog_open());
    assert!(state.dialog_has_keys());
}

#[test]
fn more_colors_opens_the_palette_and_what_it_picks_goes_into_the_popup() {
    let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
    let (character, which) = format_dialogs::character_for_find(&state.view, &[]);
    state.dialogs.push((which, character));
    let mut clip = Clipboard::without_system();

    // More Colors… from the Color popup opens the palette on top.
    let n = state.dialogs[0].1.options("color").len();
    state.dialogs[0].1.set_selected("color", n - 1);
    state.dialog_outcome(Outcome::Changed("color"), &mut clip);
    assert_eq!(state.dialogs.len(), 2);
    assert!(matches!(state.dialogs[1].0, Which::Palette(_)));

    // A color typed as an HTML code, then OK: the popup shows it.
    state.dialogs[1].1.set_field("hex", "#336699");
    state.dialog_outcome(Outcome::Changed("hex"), &mut clip);
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert_eq!(state.dialogs.len(), 1);
    assert_eq!(state.dialogs[0].1.selected_text("color"), "#336699");

    // Cancel in the palette puts back the color the popup had.
    let n = state.dialogs[0].1.options("color").len();
    state.dialogs[0].1.set_selected("color", n - 1);
    state.dialog_outcome(Outcome::Changed("color"), &mut clip);
    state.dialogs[1].1.set_field("hex", "#FF00FF");
    state.dialog_outcome(Outcome::Changed("hex"), &mut clip);
    state.dialog_outcome(Outcome::Cancel, &mut clip);
    assert_eq!(state.dialogs.len(), 1);
    assert_eq!(state.dialogs[0].1.selected_text("color"), "Black");

    // Edit… opens the editor; its OK adds the color to the palette.
    let n = state.dialogs[0].1.options("color").len();
    state.dialogs[0].1.set_selected("color", n - 1);
    state.dialog_outcome(Outcome::Changed("color"), &mut clip);
    state.dialog_outcome(Outcome::Button("edit"), &mut clip);
    assert!(matches!(state.dialogs[2].0, Which::EditColor(_)));
    state.dialogs[2].1.set_field("hex", "#102030");
    state.dialog_outcome(Outcome::Changed("hex"), &mut clip);
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert_eq!(state.dialogs.len(), 2);
    let Which::Palette(p) = &state.dialogs[1].0 else {
        panic!("the palette is back on top");
    };
    assert_eq!(p.custom.first(), Some(&quill_core::props::Rgb(0x102030)));
    assert_eq!(p.current, quill_core::props::Rgb(0x102030));
}

#[test]
fn find_format_character_more_colors_is_handled() {
    let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
    let (character, _) = format_dialogs::character_for_find(&state.view, &[]);
    state.dialogs.push((
        Which::FindFormat {
            replace: false,
            kind: 0,
        },
        character,
    ));
    let mut clip = Clipboard::without_system();

    let n = state.dialogs[0].1.options("color").len();
    state.dialogs[0].1.set_selected("color", n - 1);
    state.dialog_outcome(Outcome::Changed("color"), &mut clip);

    assert_eq!(state.dialogs.len(), 2);
    assert!(matches!(state.dialogs[1].0, Which::Palette(_)));
}

#[test]
fn the_color_button_pops_out_a_palette_and_boxes_choose_and_edit() {
    let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
    let (character, which) = format_dialogs::build_character_with_button(&state.view);
    state.dialogs.push((which, character));
    let mut clip = Clipboard::without_system();
    let rgb = |s: &WinState| s.dialogs[0].1.swatch_rgb("color").unwrap();
    assert_eq!(rgb(&state), 0x000000);
    assert_eq!(state.dialogs[0].1.item_label("color_hex"), "#000000");

    // Pressing the button pops the palette out; pressing it again puts it away.
    state.dialog_outcome(Outcome::Button("color"), &mut clip);
    let pop = state.dialogs[0]
        .1
        .popout
        .clone()
        .expect("the palette is out");
    assert_eq!(pop.anchor, "color");
    assert_eq!(pop.items.iter().filter(|i| i.label.is_empty()).count(), 32);
    assert!(pop.items.iter().any(|i| i.id == "std0" && i.selected));
    state.dialog_outcome(Outcome::Button("color"), &mut clip);
    assert!(state.dialogs[0].1.popout.is_none());

    // A box chooses its color and the palette stays out, showing it marked.
    state.dialog_outcome(Outcome::Button("color"), &mut clip);
    state.dialog_outcome(
        Outcome::Swatch {
            id: "std5",
            double: false,
        },
        &mut clip,
    );
    assert_eq!(rgb(&state), 0xFF0000);
    assert_eq!(state.dialogs[0].1.item_label("color_hex"), "#FF0000");
    let pop = state.dialogs[0].1.popout.clone().unwrap();
    assert!(pop.items.iter().any(|i| i.id == "std5" && i.selected));
    assert!(!pop.items.iter().any(|i| i.id == "std0" && i.selected));

    // A double-click edits it: the editor opens and the palette goes in.
    state.dialog_outcome(
        Outcome::Swatch {
            id: "std9",
            double: true,
        },
        &mut clip,
    );
    assert!(matches!(state.dialogs[1].0, Which::EditColor(_)));
    assert!(state.dialogs[0].1.popout.is_none());
    state.dialogs[1].1.set_field("hex", "#102030");
    state.dialog_outcome(Outcome::Changed("hex"), &mut clip);
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert_eq!(state.dialogs.len(), 1);
    assert_eq!(rgb(&state), 0x102030);
    assert_eq!(state.dialogs[0].1.item_label("color_hex"), "#102030");

    // The next time the palette is out, the edited color is a custom one.
    state.dialog_outcome(Outcome::Button("color"), &mut clip);
    let pop = state.dialogs[0].1.popout.clone().unwrap();
    assert!(
        pop.items
            .iter()
            .any(|i| i.id == "cus0" && i.rgb == Some(0x102030) && i.selected)
    );
    // Edit Color… in the palette edits the one chosen, in its box.
    state.dialog_outcome(Outcome::Button("edit"), &mut clip);
    assert!(matches!(state.dialogs[1].0, Which::EditColor(_)));
    state.dialogs[1].1.set_field("hex", "#445566");
    state.dialog_outcome(Outcome::Changed("hex"), &mut clip);
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert_eq!(rgb(&state), 0x445566);
    state.dialog_outcome(Outcome::Button("color"), &mut clip);
    let pop = state.dialogs[0].1.popout.clone().unwrap();
    assert!(
        pop.items
            .iter()
            .any(|i| i.id == "cus0" && i.rgb == Some(0x445566))
    );
    assert!(!pop.items.iter().any(|i| i.rgb == Some(0x102030)));
}

use quill_core::Rgb;

/// A window on "Blue words, plain", its first word in #336699 and the
/// Character dialog open with its color button.
fn blue_words() -> WinState {
    let mut doc = quill_core::text::from_plain_text("Blue words, plain");
    doc.format_chars(
        quill_core::StoryId::Main,
        Position::new(0, 0),
        Position::new(0, 4),
        |c| c.color = Rgb(0x336699),
    );
    doc.forget_history();
    let mut state = WinState::new(DocView::new(doc, "Doc".into()));
    let (character, which) = format_dialogs::build_character_with_button(&state.view);
    state.dialogs.push((which, character));
    state
}

fn pop_item(state: &WinState, id: &str) -> crate::dialog::PopItem {
    let pop = state.dialogs.last().unwrap().1.popout.clone().expect("out");
    pop.items.into_iter().find(|i| i.id == id).expect(id)
}

#[test]
fn deleting_a_color_the_document_uses_asks_and_makes_its_text_black() {
    let mut state = blue_words();
    let mut clip = Clipboard::without_system();
    let blue = |s: &WinState| s.view.doc.main.paras[0].chp_of_char(0).color;
    state.dialog_outcome(Outcome::Button("color"), &mut clip);
    // Pointing at a box shows its HTML code when it has no name.
    assert_eq!(pop_item(&state, "cus0").rgb, Some(0x336699));
    assert_eq!(pop_item(&state, "cus0").tip.as_deref(), Some("#336699"));
    assert_eq!(pop_item(&state, "std5").tip.as_deref(), Some("#FF0000"));
    // Delete is for the document's own colors: the text's is one.
    assert!(pop_item(&state, "cus0").selected);
    assert!(pop_item(&state, "delete_color").enabled);
    let pick = |state: &mut WinState, id| {
        state.dialog_outcome(
            Outcome::Swatch { id, double: false },
            &mut Clipboard::without_system(),
        )
    };
    pick(&mut state, "std5");
    assert!(!pop_item(&state, "delete_color").enabled);
    pick(&mut state, "cus0");
    assert!(pop_item(&state, "delete_color").enabled);

    state.dialog_outcome(Outcome::Button("delete_color"), &mut clip);
    assert_eq!(state.dialogs.len(), 2);
    assert!(matches!(
        state.dialogs[1].0,
        Which::Alert(format_dialogs::Alert::DeleteColor(Rgb(0x336699)))
    ));
    // Cancel changes nothing.
    state.dialog_outcome(Outcome::Cancel, &mut clip);
    assert_eq!(blue(&state), Rgb(0x336699));
    assert!(!state.view.doc.is_modified());

    state.dialog_outcome(Outcome::Button("color"), &mut clip);
    pick(&mut state, "cus0");
    state.dialog_outcome(Outcome::Button("delete_color"), &mut clip);
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert_eq!(state.dialogs.len(), 1);
    assert_eq!(blue(&state), Rgb::BLACK);
    assert_eq!(state.view.doc.undo_label(), Some("Delete Color"));
    // The palette is back, without the color; the dialog shows black.
    assert_eq!(pop_item(&state, "cus0").rgb, None);
    assert_eq!(state.dialogs[0].1.swatch_rgb("color"), Some(0));
    assert_eq!(state.dialogs[0].1.item_label("color_hex"), "#000000");
    state.view.doc.undo();
    assert_eq!(blue(&state), Rgb(0x336699));
}

#[test]
fn deleting_a_color_can_choose_its_replacement() {
    let mut state = blue_words();
    let mut clip = Clipboard::without_system();
    state.dialog_outcome(Outcome::Button("color"), &mut clip);
    state.dialog_outcome(
        Outcome::Swatch {
            id: "cus0",
            double: false,
        },
        &mut clip,
    );
    state.dialog_outcome(Outcome::Button("delete_color"), &mut clip);
    state.dialog_outcome(Outcome::Button("choose"), &mut clip);
    assert!(matches!(state.dialogs[1].0, Which::EditColor(_)));
    state.dialogs[1].1.set_field("hex", "#AA2200");
    state.dialog_outcome(Outcome::Changed("hex"), &mut clip);
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert_eq!(state.dialogs.len(), 1);
    assert_eq!(
        state.view.doc.main.paras[0].chp_of_char(0).color,
        Rgb(0xAA2200)
    );
    assert_eq!(
        state.view.doc.main.paras[0].chp_of_char(6).color,
        Rgb::BLACK
    );
    assert_eq!(pop_item(&state, "cus0").rgb, Some(0xAA2200));
}

#[test]
fn deleting_an_unused_or_saved_color() {
    let mut state = blue_words();
    let mut clip = Clipboard::without_system();
    crate::settings::update(|s| s.named_colors = vec![(0x336699, "Brand".into())]);
    state.dialog_outcome(Outcome::Button("color"), &mut clip);
    // A color saved in Preferences is for every document: no Delete.
    assert_eq!(pop_item(&state, "cus0").tip.as_deref(), Some("Brand"));
    state.dialog_outcome(
        Outcome::Swatch {
            id: "cus0",
            double: false,
        },
        &mut clip,
    );
    assert!(!pop_item(&state, "delete_color").enabled);
    crate::settings::update(|s| s.named_colors.clear());

    // A color made with Edit Color but not used yet goes at once.
    state.dialog_outcome(
        Outcome::Swatch {
            id: "std0",
            double: false,
        },
        &mut clip,
    );
    state.dialog_outcome(Outcome::Button("edit"), &mut clip);
    state.dialogs[1].1.set_field("hex", "#123456");
    state.dialog_outcome(Outcome::Changed("hex"), &mut clip);
    state.dialog_outcome(Outcome::Ok, &mut clip);
    state.dialog_outcome(Outcome::Button("color"), &mut clip);
    assert_eq!(pop_item(&state, "cus0").rgb, Some(0x123456));
    assert!(pop_item(&state, "cus0").selected);
    state.dialog_outcome(Outcome::Button("delete_color"), &mut clip);
    assert_eq!(state.dialogs.len(), 1);
    assert!(!state.view.doc.is_modified());
    assert_eq!(pop_item(&state, "cus0").rgb, Some(0x336699));
    crate::palette::drop_edits();
}

#[test]
fn font_sources_are_staged_until_preferences_save_and_cancel_discards_edits() {
    let before = crate::settings::get();
    let mut draft = before.clone();
    draft.font_folders.clear();
    let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
    state.dialogs.push((
        Which::Preferences {
            panel: crate::prefs_dialog::DEFAULT_FONT_PANEL,
            families: Vec::new(),
            original: Box::new(before.clone()),
            draft: Box::new(draft.clone()),
        },
        crate::prefs_dialog::preferences_with(crate::prefs_dialog::DEFAULT_FONT_PANEL, &[], &draft),
    ));
    let mut clip = Clipboard::without_system();
    state.dialog_outcome(Outcome::Button("font_sources"), &mut clip);
    assert_eq!(state.dialogs.len(), 2);
    if let Which::FontSources(sources) = &mut state.dialogs.last_mut().unwrap().0 {
        sources.font_folders[0] = crate::settings::FontFolder {
            path: "/Fonts/Test".into(),
            enabled: true,
        };
    } else {
        panic!("expected font sources");
    }
    state
        .dialogs
        .last_mut()
        .unwrap()
        .1
        .set_field("font_levels", "6");
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert_eq!(state.dialogs.len(), 1);
    assert_eq!(crate::settings::get().font_folders, before.font_folders);
    state.dialog_outcome(Outcome::Button("font_sources"), &mut clip);
    state.dialog_outcome(Outcome::Button("fclear0"), &mut clip);
    state.dialog_outcome(Outcome::Cancel, &mut clip);
    if let Which::Preferences { draft, .. } = &state.dialogs[0].0 {
        assert_eq!(draft.font_folders.len(), 1);
        assert_eq!(draft.font_folder_levels, 6);
    } else {
        panic!("expected preferences");
    }
    state.dialog_outcome(Outcome::Button("save"), &mut clip);
    assert_eq!(
        crate::settings::get().font_folders[0].path,
        std::path::Path::new("/Fonts/Test")
    );
    assert_eq!(crate::settings::get().font_folder_levels, 6);
    state.dialog_outcome(Outcome::Button("font_sources"), &mut clip);
    state.dialog_outcome(Outcome::Button("fclear0"), &mut clip);
    state.dialog_outcome(Outcome::Button("source_add"), &mut clip);
    state.dialog_outcome(Outcome::Ok, &mut clip);
    if let Which::Preferences { draft, .. } = &state.dialogs[0].0 {
        assert!(draft.font_folders.is_empty(), "empty rows are discarded");
    }
    state.dialog_outcome(Outcome::Cancel, &mut clip);
    assert_eq!(crate::settings::get().font_folders.len(), 1);
    crate::settings::update(|s| *s = before);
}

#[test]
fn preferences_colors_are_chosen_from_the_palette() {
    let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
    let d = crate::prefs_dialog::preferences(crate::prefs_dialog::COLORS_PANEL, &[]);
    state.dialogs.push((
        Which::Preferences {
            panel: crate::prefs_dialog::COLORS_PANEL,
            families: Vec::new(),
            original: Box::new(crate::settings::get()),
            draft: Box::new(crate::settings::get()),
        },
        d,
    ));
    let mut clip = Clipboard::without_system();
    state.dialog_outcome(Outcome::Button("cbox1"), &mut clip);
    let pop = state.dialogs[0].1.popout.clone().unwrap();
    assert_eq!(pop.anchor, "cbox1");
    assert!(!pop.items.iter().any(|i| i.id == "delete_color"));
    state.dialog_outcome(
        Outcome::Swatch {
            id: "std5",
            double: false,
        },
        &mut clip,
    );
    assert_eq!(state.dialogs[0].1.item_label("color1"), "#FF0000");
    assert_eq!(state.dialogs[0].1.swatch_rgb("cbox1"), Some(0xFF0000));
    // Another box: the pop-out moves to it.
    state.dialog_outcome(Outcome::Button("cbox2"), &mut clip);
    assert_eq!(state.dialogs[0].1.popout.as_ref().unwrap().anchor, "cbox2");
    state.dialog_outcome(Outcome::Button("edit"), &mut clip);
    state.dialogs[1].1.set_field("hex", "#0A0B0C");
    state.dialog_outcome(Outcome::Changed("hex"), &mut clip);
    state.dialog_outcome(Outcome::Ok, &mut clip);
    assert_eq!(state.dialogs[0].1.item_label("color2"), "#0A0B0C");
    state.dialogs[0].1.set_field("cname1", "Red");
    state.dialogs[0].1.set_field("cname2", "Ink");
    state.dialog_outcome(Outcome::Button("cclear2"), &mut clip);
    assert_eq!(state.dialogs[0].1.item_label("color2"), "None");
    state.dialog_outcome(Outcome::Button("save"), &mut clip);
    let saved = crate::settings::with(|s| s.named_colors.clone());
    crate::settings::update(|s| s.named_colors.clear());
    assert_eq!(saved, [(0xFF0000, "Red".to_string())]);
}

#[test]
fn nested_modal_keyboard_actions_close_one_layer_at_a_time() {
    let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
    let (paragraph, which) = format_dialogs::paragraph(&state.view);
    state.dialogs.push((which, paragraph));

    let mut clip = Clipboard::new();
    state.dialog_outcome(Outcome::Button("tabs"), &mut clip);
    assert_eq!(state.dialogs.len(), 2);
    assert!(matches!(
        state.dialogs.last(),
        Some((Which::Tabs { .. }, _))
    ));

    state.dialog_key(DKey::Escape, None, &mut clip);
    assert_eq!(state.dialogs.len(), 1);
    assert!(matches!(
        state.dialogs.last(),
        Some((Which::Paragraph(_), _))
    ));

    state.dialog_key(DKey::Escape, None, &mut clip);
    assert!(state.dialogs.is_empty());
}

#[test]
fn split_panes_keep_independent_scroll_positions() {
    let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
    state.width = 800.0;
    state.height = 600.0;
    state.view.scroll_y = 40.0;
    state.toggle_split();

    let split = state.split.unwrap();
    assert!(!split.upper_active);
    assert_eq!(split.other_scroll, 40.0);

    state.view.scroll_y = 120.0;
    state.switch_pane();
    let split = state.split.unwrap();
    assert!(split.upper_active);
    assert_eq!(state.view.scroll_y, 40.0);
    assert_eq!(split.other_scroll, 120.0);

    state.view.scroll_y = 75.0;
    state.switch_pane();
    let split = state.split.unwrap();
    assert!(!split.upper_active);
    assert_eq!(state.view.scroll_y, 120.0);
    assert_eq!(split.other_scroll, 75.0);
}

#[test]
fn joining_a_split_keeps_the_surviving_panes_scroll_position() {
    let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
    state.width = 800.0;
    state.height = 600.0;
    state.view.scroll_y = 90.0;
    state.toggle_split();
    state.view.scroll_y = 180.0;
    state.switch_pane();
    state.view.scroll_y = 30.0;

    let bottom = state.height - BAR;
    state.finish_split_drag(bottom);
    assert!(state.split.is_none());
    assert_eq!(state.view.scroll_y, 30.0);

    state.toggle_split();
    state.view.scroll_y = 180.0;
    state.switch_pane();
    state.view.scroll_y = 30.0;
    let top = state.text_top();
    state.finish_split_drag(top);
    assert!(state.split.is_none());
    assert_eq!(state.view.scroll_y, 180.0);
}

#[test]
fn a_shared_document_moves_between_windows() {
    let mut doc = Document::new();
    doc.main.paras[0].insert(0, "shared text", Default::default());
    let mut a = WinState::new(DocView::new(doc, "Doc".into()));
    let mut b = WinState::new(DocView::new(Document::new(), "Doc".into()));
    a.view.sel = Selection::caret(Position::new(0, 3));
    a.view.scroll_y = 40.0;
    let (mut ap, mut bp) = (
        Port::default(),
        Port {
            scroll_y: 0.0,
            scroll_x: 0.0,
            sel: Selection::caret(Position::new(0, 7)),
            upstream: false,
        },
    );
    hand_over(&mut a, &mut ap, &mut b, &mut bp);
    assert_eq!(b.view.doc.main.paras[0].text, "shared text");
    assert_eq!(b.view.sel.focus, Position::new(0, 7));
    assert_eq!(b.view.scroll_y, 0.0);
    b.view.insert_text("!");
    // And back: each window finds its own place again.
    hand_over(&mut b, &mut bp, &mut a, &mut ap);
    assert_eq!(a.view.doc.main.paras[0].text, "shared !text");
    assert_eq!(a.view.sel.focus, Position::new(0, 3));
    assert_eq!(a.view.scroll_y, 40.0);
    assert_eq!(bp.sel.focus, Position::new(0, 8));
}
#[test]
fn remembered_window_frames_stay_on_a_connected_screen() {
    let laptop = [0.0, 0.0, 1512.0, 982.0];
    let monitor = [1512.0, -200.0, 2560.0, 1440.0];
    // On the laptop: as remembered.
    assert_eq!(
        restore_frame([100, 50, 900, 700], &[laptop], &[]),
        Some([100.0, 50.0, 900.0, 700.0])
    );
    // Left on the monitor, which is now unplugged: the default place.
    assert_eq!(restore_frame([2000, 100, 900, 700], &[laptop], &[]), None);
    assert!(restore_frame([2000, 100, 900, 700], &[laptop, monitor], &[]).is_some());
    // Too big for the screen now: shrunk to fit.
    assert_eq!(
        restore_frame([0, 0, 3000, 2000], &[laptop], &[]),
        Some([0.0, 0.0, 1512.0, 982.0])
    );
    // A window already there: the next one steps down and right.
    assert_eq!(
        restore_frame(
            [100, 50, 900, 700],
            &[laptop],
            &[(100.0, 50.0), (124.0, 74.0)]
        ),
        Some([148.0, 98.0, 900.0, 700.0])
    );
}
#[test]
fn zooming_far_down_a_long_document_stays_on_the_same_page() {
    let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
        return;
    };
    let paragraph = "A paragraph of ordinary text that runs on for a few lines. ".repeat(8);
    let doc = quill_core::text::from_plain_text(&format!("{paragraph}\n").repeat(1200));
    for page_view in [false, true] {
        let mut state = WinState::new(DocView::new(doc.clone(), "Zoom".into()));
        state.document_zoom = 1.0;
        state.ui = 1.0;
        state.view.page_view = page_view;
        paint_editor(&mut state, 900, 700, &mut fonts);
        state.view.scroll_y = (state.view.max_scroll() * 0.4).floor();
        paint_editor(&mut state, 900, 700, &mut fonts);
        let page = state.view.visible_page();
        assert!(page > 20, "the test document is long enough");
        // The caret is still on page 1; zooming must not go back to it.
        for _ in 0..6 {
            state.zoom_by(1.1, false);
            paint_editor(&mut state, 900, 700, &mut fonts);
            assert!(state.view.visible_page().abs_diff(page) <= 1);
        }
        state.zoom_step(1);
        paint_editor(&mut state, 900, 700, &mut fonts);
        assert!(state.view.visible_page().abs_diff(page) <= 1);
        for _ in 0..6 {
            state.zoom_by(1.0 / 1.1, false);
            paint_editor(&mut state, 900, 700, &mut fonts);
            assert!(state.view.visible_page().abs_diff(page) <= 1);
        }
    }
    crate::settings::update(|s| *s = crate::settings::Settings::default());
}

#[test]
fn zooming_in_on_a_page_keeps_it_centered_sideways() {
    let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
        return;
    };
    let doc = quill_core::text::from_plain_text(&"A paragraph of text.\n".repeat(200));
    let mut state = WinState::new(DocView::new(doc, "Zoom".into()));
    state.document_zoom = 1.0;
    state.ui = 1.0;
    state.view.page_view = true;
    paint_editor(&mut state, 900, 700, &mut fonts);
    assert_eq!(state.view.scroll_x, 0.0);
    state.zoom_by(3.0, false);
    paint_editor(&mut state, 900, 700, &mut fonts);
    // The page is wider than the window now: its middle is the
    // window's middle, not its left edge.
    let (pw, _) = state.view.page_size();
    let want = crate::docview::PAGE_GAP + pw / 2.0 - state.view.area.w / 2.0;
    assert!(state.view.scroll_x > 0.0);
    assert!(
        (state.view.scroll_x - want).abs() < 1.5,
        "{} vs {want}",
        state.view.scroll_x
    );
    state.zoom_by(1.0 / 3.0, false);
    paint_editor(&mut state, 900, 700, &mut fonts);
    assert_eq!(state.view.scroll_x, 0.0);
}

#[test]
fn stationary_selection_drag_keeps_scrolling_until_release() {
    let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
        return;
    };
    let doc = quill_core::text::from_plain_text(&"A paragraph for selection.\n".repeat(150));
    let mut state = WinState::new(DocView::new(doc, "Selection".into()));
    state.document_zoom = 1.0;
    state.ui = 1.0;
    paint_editor(&mut state, 720, 480, &mut fonts);
    let a = state.view.area;
    state.view.mouse_down(a.x + 40.0, a.y + 30.0, false, false);
    let anchor = state.view.sel.anchor;
    state.pointer = (a.x + 40.0, a.bottom() + 40.0);
    for _ in 0..5 {
        assert!(state.selection_scroll(true));
    }
    assert!(state.view.scroll_y >= 190.0);
    assert_eq!(state.view.sel.anchor, anchor);
    assert!(state.view.sel.focus > anchor);
    // Just past the edge, as at the bottom of the screen, it still moves
    // at a useful pace.
    let y = state.view.scroll_y;
    state.pointer = (a.x + 40.0, a.bottom() + 1.0);
    for _ in 0..5 {
        assert!(state.selection_scroll(true));
    }
    assert!(state.view.scroll_y >= y + 50.0);
    state.view.mouse_up();
    let y = state.view.scroll_y;
    assert!(!state.selection_scroll(true));
    assert_eq!(state.view.scroll_y, y);
}

#[test]
fn horizontal_pan_keeps_page_hit_testing_and_shared_ports_aligned() {
    let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
        return;
    };
    let mut state = WinState::new(DocView::new(
        quill_core::text::from_plain_text("Hello world"),
        "Pan".into(),
    ));
    state.document_zoom = 1.0;
    state.view.page_view = true;
    paint_editor(&mut state, 420, 480, &mut fonts);
    let a = state.view.area;
    let x = state.view.text_left_window() + 12.0;
    let y = a.y + crate::docview::PAGE_GAP + 72.0 + 4.0;
    let before = state.view.hit(x, y);
    state.view.scroll_sideways(100.0);
    assert_eq!(state.view.scroll_x, 100.0);
    assert_eq!(state.view.hit(x - 100.0, y), before);
    let mut port = Port::default();
    state.view.swap_port(&mut port);
    assert_eq!(port.scroll_x, 100.0);
    assert_eq!(state.view.scroll_x, 0.0);
    state.view.swap_port(&mut port);
    state.view.area.w = 1200.0;
    state.view.scroll_sideways(0.0);
    assert_eq!(state.view.scroll_x, 0.0);
}
#[test]
fn deleting_a_comment_from_its_focused_editor_preserves_text_for_undo() {
    use quill_core::{StoryId, notes::AnnotationInfo};
    let mut doc = quill_core::text::from_plain_text("Body text");
    let id = doc.insert_annotation(
        Position::new(0, 4),
        Default::default(),
        AnnotationInfo::default(),
    );
    doc.forget_history(); // A loaded document, before editing the comment.
    let mut state = WinState::new(DocView::new(doc, "Comments".into()));
    state.open_note_in(StoryId::Annotations, id);
    state.view.type_text("Edited comment");
    state.delete_annotation();
    assert!(state.pane.is_none());
    assert_eq!(state.view.doc.main.paras[0].text, "Body text");
    assert!(state.view.doc.annotation_ranges().is_empty());
    state.view.doc.undo();
    assert!(state.view.doc.annotation_mark(id).is_some());
    assert!(
        state
            .view
            .doc
            .annotation_text(id)
            .contains("Edited comment")
    );
}
#[test]
fn horizontal_scrollbar_click_drag_and_release_pan_the_document() {
    let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
        return;
    };
    let mut state = WinState::new(DocView::new(Document::new(), "Horizontal".into()));
    state.document_zoom = 1.0;
    state.view.page_view = true;
    paint_editor(&mut state, 420, 480, &mut fonts);
    let bar = state.horizontal_bar();
    let env = Env {
        font_names: &[],
        font_families: &[],
        recent: &[],
        windows: &[],
    };
    let mut cache = GlyphCache::new();
    let mut clip = Clipboard::new();
    let mut label = Label {
        fonts: &mut fonts,
        cache: &mut cache,
    };
    state.mouse_down(
        bar.right() - 4.0,
        bar.y + 8.0,
        Mods::default(),
        &mut label,
        &env,
        &mut clip,
    );
    assert!(state.main_view().scroll_x > 0.0);
    state.mouse_up(bar.right() - 4.0, bar.y + 8.0, &mut clip);
    state.main_view_mut().scroll_x = 0.0;
    state.mouse_down(
        bar.x + BAR + 4.0,
        bar.y + 8.0,
        Mods::default(),
        &mut label,
        &env,
        &mut clip,
    );
    state.mouse_move(bar.right() - BAR - 8.0, bar.y + 8.0, &mut label, &env);
    assert!(state.main_view().scroll_x > 50.0);
    state.mouse_up(bar.right() - BAR - 8.0, bar.y + 8.0, &mut clip);
    assert!(state.horizontal_press.is_none());
}

#[test]
fn a_styles_character_dialog_picks_colors_from_the_palette() {
    let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
    let (d, st) = crate::style_dialog::editor(&state.view, Some(state.view.current_pap().style));
    state.dialogs.push((Which::Style(st), d));
    let mut clip = Clipboard::new();
    state.dialog_outcome(Outcome::Button("character"), &mut clip);
    assert!(matches!(
        state.dialogs.last().unwrap().0,
        Which::StyleCharacter(_)
    ));
    state.dialog_outcome(Outcome::Button("color"), &mut clip);
    assert!(
        state.dialogs.last().unwrap().1.popout.is_some(),
        "the palette opens beside the color box"
    );
}

#[test]
fn apply_in_paragraph_styles_gives_the_style_to_the_paragraph() {
    let mut doc = Document::new();
    let id = doc.add_style(
        "Loud",
        0,
        None,
        doc.styles.get(0).pap.clone(),
        doc.styles.get(0).chp,
    );
    let mut state = WinState::new(DocView::new(doc, "Doc".into()));
    let (mut d, st) = crate::style_dialog::list(&state.view);
    let mut st = st;
    crate::style_dialog::refresh_list(&mut st, &mut d, &state.view, Some(id));
    state.dialogs.push((Which::ParagraphStyles(st), d));
    let mut clip = Clipboard::new();
    state.dialog_outcome(Outcome::Apply, &mut clip);
    assert_eq!(state.view.current_pap().style, id);
}

#[test]
fn the_toolbar_color_button_opens_the_palette_and_a_box_colors_the_text() {
    use quill_core::props::Rgb;
    let mut doc = quill_core::text::from_plain_text("Some text");
    doc.main.paras[0].format_range(0, 9, &mut |c| c.color = Rgb(0x336699));
    let mut state = WinState::new(DocView::new(doc, "Doc".into()));
    state.width = 900.0;
    state.height = 700.0;
    state.view.sel = Selection::new(Position::new(0, 0), Position::new(0, 4));
    let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
        return;
    };
    let mut cache = GlyphCache::new();
    let mut label = Label {
        fonts: &mut fonts,
        cache: &mut cache,
    };
    let env = Env {
        font_names: &[],
        font_families: &[],
        recent: &[],
        windows: &[],
    };
    state.open_popup(Hit::ColorPopup, &mut label, &env);
    assert!(state.menu.is_none(), "the palette is not a menu");
    let t = state.tool_pal.as_ref().expect("the palette opens");
    // The selection's own color is in the custom row, marked.
    assert!(
        t.pop
            .items
            .iter()
            .any(|i| i.rgb == Some(0x336699) && i.selected)
    );
    // Press the red standard box.
    let red = t
        .pop
        .items
        .iter()
        .find(|i| i.rgb == Some(0xFF0000))
        .unwrap();
    let (x, y) = (t.panel.x + red.rect.x + 2.0, t.panel.y + red.rect.y + 2.0);
    state.tool_palette_down(x, y);
    assert!(state.tool_pal.is_none(), "choosing puts it away");
    assert_eq!(
        state.view.doc.main.paras[0].chp_of_char(0).color,
        Rgb(0xFF0000)
    );
    assert_eq!(
        state.view.doc.main.paras[0].chp_of_char(5).color,
        Rgb(0x336699)
    );
    // A press outside only puts it away.
    state.open_popup(Hit::ColorPopup, &mut label, &env);
    assert!(state.tool_palette_down(2.0, 2.0).is_some());
    assert!(state.tool_pal.is_none());
}
