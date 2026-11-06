//! Temporary character-style controls while the final style UI is being designed.
use crate::dialog::Dialog;
use crate::docview::DocView;
use quill_core::character_styles::CharacterFormat;

pub fn dialog(view: &DocView) -> Dialog {
    let mut d = Dialog::new("Character Styles", 460.0, 240.0);
    d.text("Style:", 12.0, 14.0);
    d.popup_owned("list", Vec::new(), 0, 66.0, 10.0, 270.0);
    d.text("Name:", 12.0, 48.0);
    d.field("name", "", 66.0, 44.0, 270.0);
    d.note("description", "", 12.0, 82.0, 430.0);
    d.note(
        "help",
        "Format some text, then define or redefine a style from it.",
        12.0,
        105.0,
        430.0,
    );
    d.note("status", "", 12.0, 128.0, 430.0);
    d.button("rename", "Rename", 12.0, 190.0, 94.0, false);
    d.button("define", "Define New", 12.0, 156.0, 94.0, false);
    d.button("redefine", "Redefine", 114.0, 156.0, 94.0, false);
    d.button("delete", "Delete Style", 216.0, 156.0, 100.0, false);
    d.button("apply", "Apply", 350.0, 12.0, 96.0, false);
    d.button("remove", "Remove", 350.0, 44.0, 96.0, false);
    d.button("ok", "Close", 350.0, 156.0, 96.0, true);
    refresh(&mut d, view, view.current_chp().character_style);
    d.focus("name");
    d
}

fn refresh(d: &mut Dialog, view: &DocView, id: u16) {
    let names = std::iter::once("None".into())
        .chain(view.doc.styles.characters.iter().map(|s| s.name.clone()))
        .collect();
    d.set_options("list", names, id as usize);
    changed(d, view);
}

pub fn changed(d: &mut Dialog, view: &DocView) {
    let id = d.selected("list") as u16;
    let style = view.doc.styles.character(id);
    d.set_field("name", style.map_or("", |s| s.name.as_str()));
    d.set_label(
        "description",
        if style.is_some() {
            "Only this style's defined properties replace paragraph defaults."
        } else {
            "None removes the named layer and keeps unrelated formatting."
        },
    );
    d.enable("redefine", style.is_some());
    d.enable("delete", style.is_some());
    d.enable("rename", style.is_some());
}

pub fn button(d: &mut Dialog, view: &mut DocView, action: &str) {
    let id = d.selected("list") as u16;
    if action == "apply" || action == "remove" {
        view.set_character_style(if action == "remove" { 0 } else { id });
        d.set_label(
            "status",
            "Applied to the selection or to the next text you type.",
        );
        return;
    }
    if !matches!(action, "define" | "redefine" | "delete" | "rename") {
        return;
    }
    let props = view.current_chp();
    let base = view.doc.styles.get(view.current_pap().style).chp;
    let mut format = CharacterFormat::from_selection(props, base);
    if action == "redefine"
        && let Some(old) = view.doc.styles.character(id)
    {
        format.mask |= old.format.mask;
    }
    let name = d.field_text("name");
    if action == "define"
        && (name.trim().is_empty()
            || name.chars().count() > 255
            || view.doc.styles.find_character(&name).is_some())
    {
        d.set_label("status", "Choose a new name of 1 to 255 characters.");
        return;
    }
    if action == "rename"
        && (name.trim().is_empty()
            || name.chars().count() > 255
            || view
                .doc
                .styles
                .find_character(&name)
                .is_some_and(|other| other != id))
    {
        d.set_label("status", "Choose a distinct name of 1 to 255 characters.");
        return;
    }
    view.doc.begin("Character Style", view.sel);
    let changed = match action {
        "define" => view.doc.add_character_style(&name, format),
        "redefine" => view.doc.redefine_character_style(id, format).then_some(id),
        "delete" => view.doc.delete_character_style(id).then_some(0),
        "rename" => view.doc.rename_character_style(id, &name).then_some(id),
        _ => None,
    };
    if changed.is_none() {
        view.doc.commit(view.sel);
        d.set_label("status", "No changes.");
        return;
    }
    let caret = if view.sel.is_empty() && matches!(action, "rename" | "delete") {
        let mut c = props;
        if action == "delete" {
            if c.character_style == id {
                c.character_style = 0;
                c.direct_format = 0;
            } else if c.character_style > id {
                c.character_style -= 1;
            }
        }
        Some(c)
    } else {
        None
    };
    view.finish_character_style_edit(caret);
    if let Some(id) = changed {
        if matches!(action, "define" | "redefine") {
            view.set_character_style(id);
        }
        refresh(d, view, id);
        d.set_label(
            "status",
            "Style saved in this document. Changes can be undone.",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::{Document, Position, Selection};

    #[test]
    fn define_apply_redefine_and_remove_from_the_temporary_controls() {
        let mut view = DocView::new(quill_core::text::from_plain_text("One two"), "Test".into());
        view.set_selection(Selection::new(Position::new(0, 0), Position::new(0, 3)));
        view.format_chars("Italic", |c| c.italic = true);
        let mut d = dialog(&view);
        d.set_field("name", "Emphasis");
        button(&mut d, &mut view, "define");
        let id = view.doc.styles.find_character("Emphasis").unwrap();
        assert_eq!(view.doc.main.paras[0].chp_of_char(0).character_style, id);
        view.set_selection(Selection::new(Position::new(0, 4), Position::new(0, 7)));
        button(&mut d, &mut view, "apply");
        assert!(view.doc.main.paras[0].chp_of_char(4).italic);
        view.format_chars("Bold", |c| c.bold = true);
        button(&mut d, &mut view, "redefine");
        assert!(view.doc.main.paras[0].chp_of_char(0).bold);
        button(&mut d, &mut view, "remove");
        assert_eq!(view.doc.main.paras[0].chp_of_char(4).character_style, 0);
        assert!(!view.doc.main.paras[0].chp_of_char(4).italic);
    }

    #[test]
    fn renaming_or_deleting_a_style_keeps_the_pending_typing_appearance() {
        let mut view = DocView::new(Document::new(), "Test".into());
        let mut d = dialog(&view);
        view.format_chars("Italic", |c| c.italic = true);
        d.set_field("name", "Emphasis");
        button(&mut d, &mut view, "define");
        let before = view.current_chp();
        d.set_field("name", "Renamed");
        button(&mut d, &mut view, "rename");
        assert_eq!(view.current_chp(), before);
        button(&mut d, &mut view, "rename");
        assert_eq!(view.current_chp(), before);
        button(&mut d, &mut view, "delete");
        assert!(view.current_chp().italic);
        assert_eq!(view.current_chp().character_style, 0);
    }

    #[test]
    fn caret_style_is_used_for_typing_and_invalid_names_make_no_edit() {
        let mut view = DocView::new(Document::new(), "Test".into());
        let mut d = dialog(&view);
        button(&mut d, &mut view, "define");
        assert!(view.doc.styles.characters.is_empty());
        view.format_chars("Italic", |c| c.italic = true);
        d.set_field("name", "Emphasis");
        button(&mut d, &mut view, "define");
        assert!(view.current_chp().italic);
        let id = view.doc.styles.find_character("Emphasis").unwrap();
        assert_eq!(view.current_chp().character_style, id);
    }
}
