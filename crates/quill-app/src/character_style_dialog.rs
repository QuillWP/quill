//! Character Styles: the document's named character styles in a list, and
//! Edit Character Style, which shows everything a style sets on one page.
//! A blank control there means the style leaves that property to the
//! paragraph, so what a style sets is always in view.
use crate::dialog::Dialog;
use crate::docview::DocView;
use quill_core::character_styles::CharacterFormat;
use quill_core::find::CharSpec;
use quill_core::{CharProps, Document, Underline};

/// The on/off properties, as the Character dialog names its check boxes.
const SWITCHES: [&str; 8] = [
    "bold",
    "italic",
    "outline",
    "shadow",
    "strike",
    "smallcaps",
    "allcaps",
    "hidden",
];

pub fn dialog(view: &DocView) -> Dialog {
    let mut d = Dialog::new("Character Styles", 460.0, 300.0);
    let current = view.current_chp().character_style;
    d.list(
        "list",
        names(view),
        current.saturating_sub(1) as usize,
        12.0,
        12.0,
        256.0,
        200.0,
        Some("edit"),
    );
    d.button("new", "+", 12.0, 218.0, 30.0, false);
    d.button("delete", "−", 46.0, 218.0, 30.0, false);
    d.button("edit", "Edit…", 84.0, 218.0, 84.0, false);
    d.button("duplicate", "Duplicate", 174.0, 218.0, 94.0, false);
    d.button("apply", "Apply", 282.0, 12.0, 166.0, false);
    d.button("remove", "Remove Style", 282.0, 42.0, 166.0, false);
    d.button("update", "Update to Selection", 282.0, 72.0, 166.0, false);
    d.button("clear", "Clear Formatting", 282.0, 112.0, 166.0, false);
    d.button("ok", "Close", 282.0, 218.0, 166.0, true);
    d.note("description", "", 12.0, 252.0, 436.0);
    d.note("status", "", 12.0, 272.0, 436.0);
    changed(&mut d, view);
    d
}

fn names(view: &DocView) -> Vec<String> {
    view.doc
        .styles
        .characters
        .iter()
        .map(|s| s.name.clone())
        .collect()
}

/// The style chosen in the list, numbered from one; zero when there are none.
pub fn selected_style(d: &Dialog) -> u16 {
    d.list_selected("list").map_or(0, |row| row as u16 + 1)
}

/// Show the document's styles again with style `id` chosen.
pub fn refresh(d: &mut Dialog, view: &DocView, id: u16) {
    d.set_rows("list", names(view), id.saturating_sub(1) as usize);
    changed(d, view);
}

pub fn changed(d: &mut Dialog, view: &DocView) {
    let style = view.doc.styles.character(selected_style(d));
    let description = match style {
        Some(s) => summary(&view.doc, s.format),
        None => "No character styles yet. Press + to make one.".into(),
    };
    d.set_label("description", &description);
    for id in ["edit", "delete", "duplicate", "apply", "update"] {
        d.enable(id, style.is_some());
    }
}

/// What a style sets, in plain words: "Sets: Georgia, 12 pt, Bold, Red".
/// Bold and the other on/off properties switch the paragraph's own value,
/// so they are named only when the style turns them over.
pub fn summary(doc: &Document, f: CharacterFormat) -> String {
    let has = |bit: u16| f.mask & (1 << bit) != 0;
    let p = f.props;
    let points = |twips: i32| {
        let s = format!("{:.1}", twips.unsigned_abs() as f32 / 20.0);
        s.trim_end_matches(".0").to_string()
    };
    let mut parts: Vec<String> = Vec::new();
    if has(0) {
        parts.push(doc.font_name(p.font).to_string());
    }
    if has(1) {
        parts.push(format!("{} pt", points(p.size as i32 * 10)));
    }
    for (bit, on, name) in [
        (2, p.bold, "Bold"),
        (3, p.italic, "Italic"),
        (5, p.strike, "Strikethrough"),
        (6, p.outline, "Outline"),
        (7, p.shadow, "Shadow"),
        (8, p.small_caps, "Small Caps"),
        (9, p.all_caps, "All Caps"),
        (10, p.hidden, "Hidden"),
    ] {
        if has(bit) && on {
            parts.push(name.into());
        }
    }
    if has(4) {
        parts.push(if p.underline == Underline::None {
            "No Underline".into()
        } else {
            "Underline".into()
        });
    }
    if has(11) {
        parts.push(format!("Color {}", crate::palette::hex(p.color)));
    }
    if has(14) && p.script != quill_core::Script::None {
        parts.push(
            if p.script == quill_core::Script::Super {
                "Superscript"
            } else {
                "Subscript"
            }
            .to_string(),
        );
    }
    if has(12) && p.position != 0 {
        let way = if p.position > 0 { "Raised" } else { "Lowered" };
        parts.push(format!("{way} {} pt", points(p.position)));
    }
    if has(13) && p.spacing != 0 {
        let way = if p.spacing > 0 {
            "Expanded"
        } else {
            "Condensed"
        };
        parts.push(format!("{way} {} pt", points(p.spacing)));
    }
    if parts.is_empty() {
        "Sets nothing yet: text keeps its paragraph's formatting.".into()
    } else {
        format!("Sets: {}", parts.join(", "))
    }
}

macro_rules! fields {
    ($apply:ident) => {
        $apply!(font, 0);
        $apply!(size, 1);
        $apply!(underline, 4);
        $apply!(color, 11);
        $apply!(position, 12);
        $apply!(spacing, 13);
        $apply!(script, 14);
    };
}

macro_rules! switches {
    ($apply:ident) => {
        $apply!(bold, 2);
        $apply!(italic, 3);
        $apply!(strike, 5);
        $apply!(outline, 6);
        $apply!(shadow, 7);
        $apply!(small_caps, 8);
        $apply!(all_caps, 9);
        $apply!(hidden, 10);
    };
}

/// A style as the Character dialog's "any" settings: what it sets is Some.
pub fn spec_of(f: CharacterFormat) -> CharSpec {
    let mut s = CharSpec::default();
    macro_rules! get {
        ($field:ident, $bit:expr) => {
            if f.mask & (1 << $bit) != 0 {
                s.$field = Some(f.props.$field);
            }
        };
    }
    fields!(get);
    switches!(get);
    s
}

/// The style the dialog's settings describe. An on/off property is set only
/// when it is on: switched off in a style, it would change nothing.
pub fn format_of(s: &CharSpec) -> CharacterFormat {
    let mut props = CharProps::default();
    let mut mask = 0;
    macro_rules! put {
        ($field:ident, $bit:expr) => {
            if let Some(v) = s.$field {
                props.$field = v;
                mask |= 1 << $bit;
            }
        };
    }
    fields!(put);
    macro_rules! on {
        ($field:ident, $bit:expr) => {
            if s.$field == Some(true) {
                props.$field = true;
                mask |= 1 << $bit;
            }
        };
    }
    switches!(on);
    CharacterFormat::new(props, mask)
}

/// Edit Character Style for style `id`, or New Character Style when `id`
/// is None: the Character dialog with every control blank where the style
/// sets nothing, and the name above it.
pub fn editor(view: &DocView, fonts: &[quill_layout::FontFamilyInfo], id: Option<u16>) -> Dialog {
    let style = id.and_then(|id| view.doc.styles.character(id));
    let spec = style.map(|s| spec_of(s.format)).unwrap_or_default();
    // A font the style names stays chosen even when it is not installed.
    let mut families = fonts.to_vec();
    if let Some(font) = spec.font {
        let name = view.doc.font_name(font).to_string();
        if !families.iter().any(|f| f.name.eq_ignore_ascii_case(&name)) {
            families.insert(
                0,
                quill_layout::FontFamilyInfo {
                    name,
                    draws_name: false,
                    variants: vec![quill_layout::FontVariant {
                        bold: false,
                        italic: false,
                    }],
                },
            );
        }
    }
    let mut d = crate::find_dialog::character_spec(view, &families, &spec);
    d.title = if style.is_some() {
        "Edit Character Style".into()
    } else {
        "New Character Style".into()
    };
    d.items.retain(|item| item.id != "apply");
    // Room for the name across the top.
    const NAME: f32 = 36.0;
    for item in &mut d.items {
        item.rect.y += NAME;
    }
    d.height += NAME;
    d.text("Name:", 12.0, 12.0);
    d.field(
        "name",
        style.map_or("New Style", |s| s.name.as_str()),
        66.0,
        8.0,
        214.0,
    );
    for id in SWITCHES {
        let on = d.check3_state(id) == Some(true);
        d.set_two_state(id, on);
    }
    d.focus("name");
    d
}

/// Edit Character Style's OK: save the style, or say why not. `id` is the
/// style edited, None for a new one. Returns the style's number.
pub fn finish_editor(
    editor: &Dialog,
    list: &mut Dialog,
    view: &mut DocView,
    id: Option<u16>,
) -> Result<u16, &'static str> {
    let name = editor.field_text("name").trim().to_string();
    let taken = view
        .doc
        .styles
        .find_character(&name)
        .is_some_and(|other| Some(other) != id);
    if name.is_empty() || name.chars().count() > 255 || taken {
        return Err("Choose a name of 1 to 255 characters that no other style has.");
    }
    let Some(spec) = crate::find_dialog::read_character_spec(editor, view) else {
        return Err("Type a size, position or spacing as a number of points.");
    };
    let format = format_of(&spec);
    view.doc.begin("Character Style", view.sel);
    let saved = match id {
        None => view.doc.add_character_style(&name, format),
        Some(id) => {
            view.doc.redefine_character_style(id, format);
            view.doc.rename_character_style(id, &name);
            Some(id)
        }
    };
    view.doc.commit(view.sel);
    let id = saved.ok_or("Quill could not add another style.")?;
    view.finish_character_style_edit(None);
    refresh(list, view, id);
    list.set_label(
        "status",
        "Style saved in this document. Changes can be undone.",
    );
    Ok(id)
}

/// A name for a copy of character style `name` that no character style has
/// yet. See `style_dialog::copy_name`.
pub fn copy_name(doc: &Document, name: &str) -> String {
    crate::style_dialog::copy_name(name, |n| doc.styles.find_character(n).is_some())
}

/// Copy character style `id` under a "... copy" name, as one undoable step.
/// Returns the copy's number.
pub fn duplicate(view: &mut DocView, id: u16) -> Option<u16> {
    let old = view.doc.styles.character(id).cloned()?;
    let name = copy_name(&view.doc, &old.name);
    view.doc.begin("Character Style", view.sel);
    let made = view.doc.add_character_style(&name, old.format);
    view.doc.commit(view.sel);
    if made.is_some() {
        view.finish_character_style_edit(None);
    }
    made
}

/// Delete character style `id` as one undoable step. The styles after it
/// move down a number, so typing at the caret keeps its look: it loses the
/// style if that was the one deleted, and follows its style down otherwise.
pub fn delete(view: &mut DocView, id: u16) -> bool {
    if id == 0 {
        return false;
    }
    let props = view.current_chp();
    view.doc.begin("Character Style", view.sel);
    let gone = view.doc.delete_character_style(id);
    view.doc.commit(view.sel);
    if !gone {
        return false;
    }
    let caret = view.sel.is_empty().then(|| {
        let mut c = props;
        if c.character_style == id {
            c.character_style = 0;
            c.direct_format = 0;
        } else if c.character_style > id {
            c.character_style -= 1;
        }
        c
    });
    view.finish_character_style_edit(caret);
    true
}

/// Redefine character style `id` from the selection, as one undoable step.
/// The selection's own formatting joins what the style already set.
fn update(view: &mut DocView, id: u16) -> Option<u16> {
    let old = view.doc.styles.character(id).cloned()?;
    let base = view.doc.styles.get(view.current_pap().style).chp;
    let mut format = CharacterFormat::from_selection(view.current_chp(), base);
    format.mask |= old.format.mask;
    view.doc.begin("Character Style", view.sel);
    let changed = view.doc.redefine_character_style(id, format);
    view.doc.commit(view.sel);
    if changed {
        view.finish_character_style_edit(None);
    }
    changed.then_some(id)
}

/// The list window's own buttons: everything but +, Edit…, Import… and
/// Close, which open other windows.
pub fn button(d: &mut Dialog, view: &mut DocView, action: &str) {
    let id = selected_style(d);
    match action {
        "apply" | "remove" => {
            view.set_character_style(if action == "remove" { 0 } else { id });
            d.set_label(
                "status",
                "Applied to the selection or to the next text you type.",
            );
            return;
        }
        "clear" => {
            view.clear_character_formatting();
            d.set_label(
                "status",
                "Formatting cleared from the selection or the next text you type.",
            );
            return;
        }
        "update" | "duplicate" | "delete" if id != 0 => {}
        _ => return,
    }
    let changed = match action {
        "update" => update(view, id),
        "duplicate" => duplicate(view, id),
        _ => delete(view, id).then_some(id.saturating_sub(1).max(1)),
    };
    let Some(chosen) = changed else {
        d.set_label("status", "No changes.");
        return;
    };
    if action == "update" {
        view.set_character_style(id);
    }
    refresh(d, view, chosen);
    d.set_label(
        "status",
        "Style saved in this document. Changes can be undone.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::{Position, Selection};

    fn bold_format() -> CharacterFormat {
        CharacterFormat::new(
            CharProps {
                bold: true,
                size: 36,
                ..Default::default()
            },
            (1 << 1) | (1 << 2),
        )
    }

    #[test]
    fn a_style_round_trips_through_the_dialogs_settings() {
        let f = CharacterFormat::new(
            CharProps {
                font: 1,
                size: 36,
                italic: true,
                underline: Underline::Single,
                color: quill_core::Rgb(0xFF0000),
                position: 60,
                ..Default::default()
            },
            (1 << 0) | (1 << 1) | (1 << 3) | (1 << 4) | (1 << 11) | (1 << 12),
        );
        assert_eq!(format_of(&spec_of(f)), f);
        assert_eq!(format_of(&CharSpec::default()).mask, 0);
        // Bold switched off would change nothing, so it is not kept.
        let off = CharSpec {
            bold: Some(false),
            ..Default::default()
        };
        assert_eq!(format_of(&off).mask, 0);
    }

    #[test]
    fn the_editor_shows_only_what_the_style_sets_and_saves_it_back() {
        let mut view = DocView::new(
            quill_core::text::from_plain_text("Some text"),
            "Test".into(),
        );
        let mut list = dialog(&view);
        assert_eq!(list.list_selected("list"), None);
        assert!(
            list.item_label("description")
                .starts_with("No character styles")
        );

        let mut ed = editor(&view, &[], None);
        assert_eq!(ed.title, "New Character Style");
        assert_eq!(ed.field_text("name"), "New Style");
        assert_eq!(ed.field_text("size"), "", "nothing is set to begin with");
        assert_eq!(ed.check3_state("bold"), Some(false), "plain check boxes");
        ed.set_field("name", "Callout");
        ed.set_field("size", "18");
        ed.set_on("bold", true);
        let id = finish_editor(&ed, &mut list, &mut view, None).unwrap();
        let format = view.doc.styles.character(id).unwrap().format;
        assert_eq!(format, bold_format());
        assert_eq!(list.item_label("description"), "Sets: 18 pt, Bold");
        assert_eq!(selected_style(&list), id);
        assert_eq!(view.doc.main.paras[0].chp_of_char(0).character_style, 0);

        let mut ed = editor(&view, &[], Some(id));
        assert_eq!(ed.title, "Edit Character Style");
        assert_eq!(ed.field_text("size"), "18");
        assert!(ed.is_on("bold"));
        ed.set_field("size", "");
        ed.set_field("name", "Loud");
        finish_editor(&ed, &mut list, &mut view, Some(id)).unwrap();
        let style = view.doc.styles.character(id).unwrap();
        assert_eq!(style.name, "Loud");
        assert_eq!(style.format.mask, 1 << 2, "a blank size is no longer set");
        view.doc.undo();
        assert_eq!(view.doc.styles.character(id).unwrap().format, bold_format());
        assert_eq!(view.doc.styles.character(id).unwrap().name, "Callout");
    }

    #[test]
    fn names_must_be_new_and_sizes_numbers() {
        let mut view = DocView::new(Document::new(), "Test".into());
        view.doc
            .add_character_style("Taken", bold_format())
            .unwrap();
        let mut list = dialog(&view);
        let mut ed = editor(&view, &[], None);
        ed.set_field("name", "Taken");
        assert!(finish_editor(&ed, &mut list, &mut view, None).is_err());
        ed.set_field("name", "  ");
        assert!(finish_editor(&ed, &mut list, &mut view, None).is_err());
        ed.set_field("name", "Fresh");
        ed.set_field("size", "big");
        assert!(finish_editor(&ed, &mut list, &mut view, None).is_err());
        assert_eq!(view.doc.styles.characters.len(), 1);
        // Keeping its own name is fine.
        let ed = editor(&view, &[], Some(1));
        assert!(finish_editor(&ed, &mut list, &mut view, Some(1)).is_ok());
    }

    #[test]
    fn a_style_keeps_a_font_that_is_not_installed() {
        let mut view = DocView::new(Document::new(), "Test".into());
        let font = view.doc.font_id("Nowhere Sans");
        let format = CharacterFormat::new(
            CharProps {
                font,
                ..Default::default()
            },
            1,
        );
        let id = view.doc.add_character_style("Odd", format).unwrap();
        let ed = editor(&view, &[], Some(id));
        assert_eq!(ed.selected_text("font"), "Nowhere Sans");
        let mut list = dialog(&view);
        finish_editor(&ed, &mut list, &mut view, Some(id)).unwrap();
        assert_eq!(view.doc.styles.character(id).unwrap().format, format);
    }

    #[test]
    fn apply_update_duplicate_and_delete_from_the_list() {
        let mut view = DocView::new(quill_core::text::from_plain_text("One two"), "Test".into());
        view.doc
            .add_character_style("Emphasis", bold_format())
            .unwrap();
        let mut d = dialog(&view);
        view.set_selection(Selection::new(Position::new(0, 0), Position::new(0, 3)));
        button(&mut d, &mut view, "apply");
        assert_eq!(view.doc.main.paras[0].chp_of_char(0).character_style, 1);
        assert!(view.doc.main.paras[0].chp_of_char(0).bold);
        view.format_chars("Italic", |c| c.italic = true);
        button(&mut d, &mut view, "update");
        let format = view.doc.styles.character(1).unwrap().format;
        assert_eq!(format.mask, (1 << 1) | (1 << 2) | (1 << 3));
        button(&mut d, &mut view, "duplicate");
        assert_eq!(view.doc.styles.characters[1].name, "Emphasis copy");
        assert_eq!(selected_style(&d), 2);
        button(&mut d, &mut view, "duplicate");
        assert_eq!(view.doc.styles.characters[2].name, "Emphasis copy 2");
        d.select_row("list", 0);
        button(&mut d, &mut view, "delete");
        assert_eq!(view.doc.styles.characters.len(), 2);
        assert_eq!(view.doc.main.paras[0].chp_of_char(0).character_style, 0);
        assert!(
            view.doc.main.paras[0].chp_of_char(0).italic,
            "its look stays"
        );
        button(&mut d, &mut view, "remove");
        assert_eq!(view.doc.main.paras[0].chp_of_char(0).character_style, 0);
    }
}
