//! Format > Paragraph Styles (⌘T): a list of the document's paragraph
//! styles, or of every style, with New, Edit and Delete beside it. New and
//! Edit open the style editor, which shows a style's description, Based On
//! and Next Style; Paragraph and Character there edit the pending
//! formatting, OK saves it to the style, and Apply also puts the style on
//! the selected paragraphs. Deleting a style that is in use asks what its
//! paragraphs become.

use crate::dialog::{Dialog, units};
use crate::docview::DocView;
use quill_core::props::{
    Align, BorderStyle, CharProps, LineSpacing, ParaProps, StyleId, TabAlign, Underline,
    WORD_COLORS,
};
use quill_core::{Document, StoryId};

#[derive(Clone, Debug)]
pub struct StyleState {
    /// The styles the editor can be on: None is a new style.
    pub entries: Vec<Option<StyleId>>,
    pub selected: usize,
    /// Formatting being edited; the document changes only on Define or Apply.
    pub pap: ParaProps,
    pub chp: CharProps,
    pub fonts: Vec<String>,
    /// The character style the paragraph style takes, numbered from one;
    /// zero for none, when `chp` is used as it is.
    pub character: u16,
    /// Styles in the Based On and Next Style popups, after a blank first
    /// choice (no style; for Next Style, the style itself).
    pub choices: Vec<StyleId>,
}

/// Styles sorted by name, the way the list and popups show them.
fn sorted(doc: &Document, ids: impl Iterator<Item = StyleId>) -> Vec<StyleId> {
    let mut v: Vec<StyleId> = ids.collect();
    v.sort_by_key(|&id| doc.styles.get(id).name.to_lowercase());
    v.dedup();
    v
}

/// The document's styles: Normal, styles in use and styles the user made.
fn document_styles(view: &DocView) -> Vec<StyleId> {
    let doc = &view.doc;
    let used = doc.main.paras.iter().map(|p| p.pap.style);
    let made = (0..doc.styles.styles.len() as StyleId).filter(|&i| !doc.styles.get(i).builtin);
    sorted(doc, used.chain(made).chain([0, view.current_pap().style]))
}

/// Every style the editor can be on: a new one, then all the styles.
fn entries(view: &DocView) -> Vec<Option<StyleId>> {
    std::iter::once(None)
        .chain(
            sorted(&view.doc, 0..view.doc.styles.styles.len() as StyleId)
                .into_iter()
                .map(Some),
        )
        .collect()
}

// ----- the Paragraph Styles list -----

/// What the Paragraph Styles list shows: the document's styles or all of
/// them, and the style of each row.
#[derive(Clone, Debug, Default)]
pub struct ListState {
    pub all: bool,
    pub ids: Vec<StyleId>,
}

/// Format > Paragraph Styles: the list, on the current paragraph's style.
pub fn list(view: &DocView) -> (Dialog, ListState) {
    // The same layout as the Character Styles list, with the two radio
    // buttons above the list.
    let mut d = Dialog::new("Paragraph Styles", 460.0, 312.0);
    d.radio("docstyles", "This Document", 12.0, 12.0, 1, true);
    d.radio("allstyles", "All Styles", 140.0, 12.0, 1, false);
    let mut st = ListState {
        all: false,
        ids: document_styles(view),
    };
    let rows = list_names(view, &st.ids);
    d.list("list", rows, 0, 12.0, 36.0, 256.0, 176.0, Some("edit"));
    d.button("new", "+", 12.0, 218.0, 30.0, false);
    d.button("delete", "\u{2212}", 46.0, 218.0, 30.0, false);
    d.button("edit", "Edit\u{2026}", 84.0, 218.0, 84.0, false);
    d.button("duplicate", "Duplicate", 174.0, 218.0, 94.0, false);
    d.button("apply", "Apply", 282.0, 36.0, 166.0, false);
    d.button("remove", "Remove Style", 282.0, 66.0, 166.0, false);
    d.button("update", "Update to Selection", 282.0, 96.0, 166.0, false);
    d.button("clear", "Clear Formatting", 282.0, 136.0, 166.0, false);
    d.button("ok", "Close", 282.0, 218.0, 166.0, true);
    d.note("description", "", 12.0, 252.0, 436.0);
    d.note("description2", "", 12.0, 268.0, 436.0);
    d.note("status", "", 12.0, 288.0, 436.0);
    refresh_list(&mut st, &mut d, view, Some(view.current_pap().style));
    (d, st)
}

fn list_names(view: &DocView, ids: &[StyleId]) -> Vec<String> {
    ids.iter()
        .map(|&id| view.doc.styles.get(id).display_name())
        .collect()
}

/// The style chosen in the list.
pub fn list_selected(st: &ListState, d: &Dialog) -> Option<StyleId> {
    d.list_selected("list")
        .and_then(|row| st.ids.get(row).copied())
}

/// List the styles again, on style `keep` when it is there.
pub fn refresh_list(st: &mut ListState, d: &mut Dialog, view: &DocView, keep: Option<StyleId>) {
    let keep = keep.or_else(|| list_selected(st, d));
    st.ids = if st.all {
        sorted(&view.doc, 0..view.doc.styles.styles.len() as StyleId)
    } else {
        document_styles(view)
    };
    // A style that was deleted from the list leaves the row it had chosen.
    let row = keep
        .and_then(|k| st.ids.iter().position(|&id| id == k))
        .or_else(|| d.list_selected("list"))
        .unwrap_or(0)
        .min(st.ids.len().saturating_sub(1));
    d.set_rows("list", list_names(view, &st.ids), row);
    show_chosen(st, d, view);
}

/// The chosen style's description, and which buttons can work on it.
fn show_chosen(st: &ListState, d: &mut Dialog, view: &DocView) {
    let chosen = list_selected(st, d);
    let (text, uses) = match chosen {
        Some(id) => {
            let s = view.doc.styles.get(id);
            let uses = match view.doc.style_uses(id) {
                0 => "Not used in this document.".to_string(),
                1 => "Used by 1 paragraph.".to_string(),
                n => format!("Used by {n} paragraphs."),
            };
            (describe(&view.doc, s.based_on, &s.pap, &s.own()), uses)
        }
        None => (String::new(), String::new()),
    };
    show_lines(d, &["description", "description2"], &text, false);
    d.set_label("status", &uses);
    for id in ["edit", "duplicate", "apply", "remove", "update"] {
        d.enable(id, chosen.is_some());
    }
    d.enable(
        "delete",
        chosen.is_some_and(|id| id != 0 && !view.doc.styles.get(id).builtin),
    );
}

/// A control in the list changed: the row, or which styles it shows.
pub fn list_changed(st: &mut ListState, d: &mut Dialog, view: &DocView, id: &str) {
    if id == "docstyles" || id == "allstyles" {
        st.all = id == "allstyles";
        refresh_list(st, d, view, None);
    } else {
        show_chosen(st, d, view);
    }
}

/// The list's own buttons: Apply, Remove Style, Update to Selection and
/// Duplicate. The ones that open other windows (+, Edit, Import, Close)
/// are handled by the window.
pub fn list_button(st: &mut ListState, d: &mut Dialog, view: &mut DocView, action: &str) {
    if action == "clear" {
        let keep = list_selected(st, d);
        view.clear_formatting();
        refresh_list(st, d, view, keep);
        d.set_label("status", "Formatting cleared. Changes can be undone.");
        return;
    }
    let Some(id) = list_selected(st, d) else {
        return;
    };
    let saved = "Style saved in this document. Changes can be undone.";
    let (keep, message) = match action {
        "apply" => {
            view.set_style(id);
            (id, "Applied to the selected paragraphs.")
        }
        "remove" => {
            view.set_style(0);
            (id, "The selected paragraphs are back to Normal.")
        }
        "update" => {
            if !update(view, id) {
                d.set_label("status", "No changes: the selection already matches.");
                return;
            }
            view.set_style(id);
            (id, saved)
        }
        "duplicate" => (duplicate(view, id), saved),
        _ => return,
    };
    refresh_list(st, d, view, Some(keep));
    d.set_label("status", message);
}

/// Redefine style `id` from the selection's paragraph and character
/// formatting, as one undoable step. False when the style already looks
/// like the selection.
pub fn update(view: &mut DocView, id: StyleId) -> bool {
    let mut pap = view.current_pap().clone();
    let chp = view.current_chp();
    let s = view.doc.styles.get(id);
    pap.style = id;
    pap.in_table = s.pap.in_table;
    if pap == s.pap && chp == s.chp {
        return false;
    }
    view.doc.begin("Define Style", view.sel);
    view.doc.redefine_style(id, pap, chp);
    view.doc.commit(view.sel);
    view.invalidate_layout();
    true
}

/// Delete style `id` from the list, or None when its paragraphs need a
/// new style first: then the question to ask, and the styles it offers.
pub fn delete_from_list(view: &mut DocView, id: StyleId) -> Option<(Dialog, Vec<StyleId>)> {
    let uses = view.doc.style_uses(id);
    if uses == 0 {
        delete_style(view, id, 0);
        return None;
    }
    let s = view.doc.styles.get(id);
    let name = s.display_name();
    let choices: Vec<StyleId> = sorted(&view.doc, 1..view.doc.styles.styles.len() as StyleId)
        .into_iter()
        .filter(|&k| k != id)
        .collect();
    let mut options = vec!["No Style (Normal)".to_string()];
    options.extend(
        choices
            .iter()
            .map(|&k| view.doc.styles.get(k).display_name()),
    );
    let mut d = Dialog::new("Delete Style", 380.0, 104.0);
    let paragraphs = if uses == 1 {
        "1 paragraph uses".to_string()
    } else {
        format!("{uses} paragraphs use")
    };
    d.text(&format!("{paragraphs} \u{201c}{name}\u{201d}."), 14.0, 12.0);
    d.text("Change them to:", 14.0, 42.0);
    d.popup_owned("replacement", options, 0, 120.0, 39.0, 160.0);
    d.button("ok", "Delete", 296.0, 10.0, 70.0, true);
    d.button("cancel", "Cancel", 296.0, 40.0, 70.0, false);
    Some((d, choices))
}

/// Delete in that question: the style goes and its paragraphs take the
/// style chosen, or Normal.
pub fn delete_question_ok(id: StyleId, choices: &[StyleId], d: &Dialog, view: &mut DocView) {
    let with = d
        .selected("replacement")
        .checked_sub(1)
        .and_then(|k| choices.get(k).copied())
        .unwrap_or(0);
    delete_style(view, id, with);
}

// ----- the style editor -----

/// The style editor on style `id`, or on a new style made from the
/// selection's formatting when `id` is None.
pub fn editor(view: &DocView, id: Option<StyleId>) -> (Dialog, StyleState) {
    let title = match id {
        Some(id) => format!(
            "Paragraph Style: {}",
            view.doc.styles.get(id).display_name()
        ),
        None => "New Paragraph Style".to_string(),
    };
    let mut d = Dialog::new(&title, 484.0, 368.0);
    d.close_box = true;
    d.button("ok", "OK", 350.0, 14.0, 120.0, true);
    d.button("cancel", "Cancel", 350.0, 40.0, 120.0, false);
    d.button("apply", "Apply", 350.0, 66.0, 120.0, false);
    d.button("paragraph", "Paragraph…", 350.0, 92.0, 120.0, false);
    d.button("character", "Character…", 350.0, 118.0, 120.0, false);
    d.text("Style:", 15.0, 12.0);
    d.field("name", "", 14.0, 28.0, 320.0);
    d.button("usedefault", "Use As Default", 350.0, 144.0, 120.0, false);
    d.disable("usedefault");
    d.group("Paragraph Formatting", 14.0, 56.0, 320.0, 92.0);
    for (id, y) in [
        ("desc0", 72.0),
        ("desc1", 90.0),
        ("desc2", 108.0),
        ("desc3", 126.0),
    ] {
        d.note(id, "", 21.0, y, 305.0);
    }
    d.group("Manual Character Formatting", 14.0, 156.0, 320.0, 74.0);
    for (id, y) in [("char0", 172.0), ("char1", 190.0), ("char2", 208.0)] {
        d.note(id, "", 21.0, y, 305.0);
    }
    d.text("Based On:", 15.0, 240.0);
    d.text("Next Style:", 15.0, 266.0);
    let choices = sorted(&view.doc, 0..view.doc.styles.styles.len() as StyleId);
    let names = choice_names(view, &choices);
    d.popup_owned("basedon", names.clone(), 0, 116.0, 237.0, 218.0);
    d.popup_owned("next", names, 0, 116.0, 263.0, 218.0);
    d.text("Character Style:", 15.0, 292.0);
    d.popup_owned(
        "character_style",
        character_choices(view),
        0,
        116.0,
        289.0,
        218.0,
    );
    d.note("character_status", "", 15.0, 319.0, 454.0);
    d.note(
        "actions_help",
        "OK saves the style. Apply also gives it to the selected paragraphs.",
        15.0,
        339.0,
        454.0,
    );
    let mut st = StyleState {
        entries: entries(view),
        selected: 0,
        pap: view.current_pap().clone(),
        chp: view.current_chp(),
        fonts: view.doc.fonts.clone(),
        character: 0,
        choices,
    };
    let i = st.entries.iter().position(|e| *e == id).unwrap_or(0);
    select(&mut st, &mut d, view, i);
    (d, st)
}

/// OK in the editor: save the style. False when there is nothing to save
/// yet (a new style with no name), so the editor stays open.
pub fn save(st: &mut StyleState, d: &mut Dialog, view: &mut DocView) -> bool {
    define(st, d, view).is_some() || st.entries[st.selected].is_some()
}

/// The style the editor is on, once it has one.
pub fn editing(st: &StyleState) -> Option<StyleId> {
    st.entries.get(st.selected).copied().flatten()
}

/// The Character Style popup: No Style, the document's character styles,
/// and a last choice that makes a new one.
fn character_choices(view: &DocView) -> Vec<String> {
    std::iter::once("No Style".to_string())
        .chain(view.doc.styles.characters.iter().map(|s| s.name.clone()))
        .chain(std::iter::once("New Character Style\u{2026}".to_string()))
        .collect()
}

/// A character style was made from the popup's New Character Style: list it
/// and link the style being edited to it.
pub fn character_saved(st: &mut StyleState, d: &mut Dialog, view: &DocView, id: u16) {
    st.character = id;
    d.set_options("character_style", character_choices(view), id as usize);
    update_description(st, d, view);
}

/// Show entry `i` in the fields.
fn select(st: &mut StyleState, d: &mut Dialog, view: &DocView, i: usize) {
    st.selected = i.min(st.entries.len() - 1);
    let at = |id: Option<StyleId>| {
        id.and_then(|id| st.choices.iter().position(|&c| c == id))
            .map_or(0, |k| k + 1)
    };
    match st.entries[st.selected] {
        None => {
            st.pap = view.current_pap().clone();
            st.chp = view.current_chp();
            st.character = 0;
            d.set_field("name", "");
            d.set_selected("basedon", at(Some(view.current_pap().style)));
            d.set_selected("next", 0);
        }
        Some(id) => {
            let s = view.doc.styles.get(id);
            st.pap = s.pap.clone();
            st.chp = s.own();
            st.character = s.character;
            d.set_field("name", &s.display_name());
            d.set_selected("basedon", at(s.based_on));
            d.set_selected("next", if s.next == id { 0 } else { at(Some(s.next)) });
        }
    }
    d.set_selected("character_style", st.character as usize);
    update_description(st, d, view);
    d.focus("name");
}

/// The Based On and Next Style popup items: a blank, then `choices`.
fn choice_names(view: &DocView, choices: &[StyleId]) -> Vec<String> {
    std::iter::once(String::new())
        .chain(
            choices
                .iter()
                .map(|&id| view.doc.styles.get(id).display_name()),
        )
        .collect()
}

fn show_lines(d: &mut Dialog, ids: &[&str], text: &str, struck: bool) {
    let mut lines = vec![String::new()];
    for word in text.split_whitespace() {
        if lines
            .last()
            .is_some_and(|line| !line.is_empty() && line.len() + word.len() + 1 > 58)
            && lines.len() < ids.len()
        {
            lines.push(word.to_string());
        } else {
            let line = lines.last_mut().unwrap();
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
    }
    for (index, id) in ids.iter().enumerate() {
        d.set_label(id, lines.get(index).map_or("", String::as_str));
        d.set_struck(id, struck);
    }
}

pub fn update_description(st: &StyleState, d: &mut Dialog, view: &DocView) {
    let base = st
        .entries
        .get(st.selected)
        .copied()
        .flatten()
        .map_or(Some(view.current_pap().style), |id| {
            view.doc.styles.get(id).based_on
        });
    let mut doc = Document::new();
    doc.fonts = st.fonts.clone();
    doc.styles = view.doc.styles.clone();
    let paragraph = describe_component(&doc, base, &st.pap, &st.chp, false, true);
    let characters = describe_component(&doc, None, &st.pap, &st.chp, true, false);
    show_lines(d, &["desc0", "desc1", "desc2", "desc3"], &paragraph, false);
    let linked = view.doc.styles.character(st.character);
    show_lines(
        d,
        &["char0", "char1", "char2"],
        &characters,
        linked.is_some(),
    );
    let status = match linked {
        Some(c) => {
            let summary = crate::character_style_dialog::summary(&view.doc, c.format);
            let sets = summary.strip_prefix("Sets: ").unwrap_or("nothing yet");
            format!(
                "Linked to {} ({sets}). No Style brings back the manual formatting above.",
                c.name
            )
        }
        None => "No Style uses the manual character formatting.".to_string(),
    };
    d.set_label("character_status", &status);
}

/// A private document holds pending font names without changing the real one.
pub fn character_view(st: &StyleState) -> DocView {
    let mut doc = Document::new();
    doc.fonts = st.fonts.clone();
    doc.styles.styles[0].chp = st.chp;
    doc.main.paras[0].mark_chp = st.chp;
    DocView::new(doc, "Style character formatting".into())
}

/// The style a popup names, if any.
fn chosen(st: &StyleState, d: &Dialog, id: &str) -> Option<StyleId> {
    let k = d.selected(id);
    (k > 0).then(|| st.choices.get(k - 1).copied()).flatten()
}

pub fn changed(st: &mut StyleState, d: &mut Dialog, view: &DocView, id: &str) {
    if id == "character_style" {
        st.character = d.selected("character_style") as u16;
        update_description(st, d, view);
    }
}

/// Define: make a new style from the selection, or change the chosen
/// style's name, formatting, Based On and Next Style. Returns the style, or None if
/// there is nothing to define (New Style with no name).
pub fn define(st: &mut StyleState, d: &mut Dialog, view: &mut DocView) -> Option<StyleId> {
    let name = d.field_text("name").trim().to_string();
    let based = chosen(st, d, "basedon");
    let next = chosen(st, d, "next");
    if st.entries[st.selected].is_none() && name.is_empty() {
        return None;
    }
    if let Some(id) = st.entries[st.selected]
        && view.doc.styles.find(&name).is_some_and(|other| other != id)
    {
        return None;
    }
    let mut chp = st.chp;
    if let Some(font) = st.fonts.get(chp.font as usize) {
        chp.font = view.doc.font_id(font);
    }
    let id = match st.entries[st.selected] {
        None if name.is_empty() => return None,
        None => {
            let pap = st.pap.clone();
            view.doc.begin("Define Style", view.sel);
            let id = match view.doc.styles.find(&name) {
                // Defining an existing name redefines it from the selection.
                Some(id) => id,
                None => view
                    .doc
                    .add_style(&name, based.unwrap_or(0), next, pap.clone(), chp),
            };
            view.doc.redefine_style_linked(id, pap, chp, st.character);
            view.doc.commit(view.sel);
            id
        }
        Some(id) => {
            let s = view.doc.styles.get(id);
            // The field shows the title-case name of an automatic style;
            // leaving it alone must not change the stored name.
            let new_name = if name.is_empty() || name == s.display_name() {
                s.name.clone()
            } else {
                name
            };
            let taken = view
                .doc
                .styles
                .find(&new_name)
                .is_some_and(|other| other != id);
            if taken {
                return Some(id);
            }
            let formatting_changed =
                s.pap != st.pap || s.own() != chp || s.character != st.character;
            view.doc.begin("Define Style", view.sel);
            if formatting_changed {
                view.doc
                    .redefine_style_linked(id, st.pap.clone(), chp, st.character);
            }
            view.doc
                .rename_style(id, &new_name, based, next.unwrap_or(id));
            view.doc.commit(view.sel);
            id
        }
    };
    view.invalidate_layout();
    // The list may have gained the style.
    st.fonts = view.doc.fonts.clone();
    st.choices = sorted(&view.doc, 0..view.doc.styles.styles.len() as StyleId);
    let names = choice_names(view, &st.choices);
    d.set_options("basedon", names.clone(), 0);
    d.set_options("next", names, 0);
    st.entries = entries(view);
    let i = st.entries.iter().position(|e| *e == Some(id)).unwrap_or(0);
    select(st, d, view, i);
    // A new style is now this one, under its own name.
    d.title = format!(
        "Paragraph Style: {}",
        view.doc.styles.get(id).display_name()
    );
    Some(id)
}

/// Apply (and OK): define what is pending, then give the selected
/// paragraphs the style.
pub fn apply(st: &mut StyleState, d: &mut Dialog, view: &mut DocView) -> bool {
    let id = match st.entries[st.selected] {
        None => match define(st, d, view) {
            Some(id) => id,
            None => return true,
        },
        Some(id) => {
            define(st, d, view);
            id
        }
    };
    view.set_style(id);
    true
}

/// A name for a copy of `name` that `taken` says is free, as the Finder
/// names copies: "Emphasis copy", then "Emphasis copy 2", whichever is
/// copied. Paragraph and character styles both name their copies this way.
pub fn copy_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut base = name;
    if let Some((stem, n)) = base.rsplit_once(" copy ")
        && n.parse::<u32>().is_ok()
    {
        base = stem;
    }
    let base = base.strip_suffix(" copy").unwrap_or(base);
    let base: String = base.chars().take(240).collect();
    let mut candidate = format!("{base} copy");
    let mut n = 2;
    while taken(&candidate) {
        candidate = format!("{base} copy {n}");
        n += 1;
    }
    candidate
}

/// Copy style `id` under a "... copy" name, as one undoable step, and
/// return the copy. A style whose next style is itself gives a copy whose
/// next style is the copy.
pub fn duplicate(view: &mut DocView, id: StyleId) -> StyleId {
    let s = view.doc.styles.get(id).clone();
    let name = copy_name(&s.display_name(), |n| view.doc.styles.find(n).is_some());
    let next = (s.next != id).then_some(s.next);
    view.doc.begin("Define Style", view.sel);
    let new = view
        .doc
        .add_style(&name, s.based_on.unwrap_or(0), next, s.pap.clone(), s.own());
    view.doc
        .redefine_style_linked(new, s.pap.clone(), s.own(), s.character);
    view.doc.commit(view.sel);
    view.invalidate_layout();
    new
}

/// Delete style `id` as one undoable step; its paragraphs take style
/// `with` (0 for Normal). Normal and the automatic styles stay. True when
/// it went.
pub fn delete_style(view: &mut DocView, id: StyleId, with: StyleId) -> bool {
    if id == 0 || view.doc.styles.get(id).builtin {
        return false;
    }
    view.doc.begin("Delete Style", view.sel);
    view.doc.delete_style_replacing(id, with);
    view.doc.commit(view.sel);
    view.invalidate_layout();
    true
}

/// The question the original asks when a paragraph's own style is chosen for it
/// again but the paragraph no longer looks like the style. None when it
/// does, or when the style is a different one.
pub fn reapply(view: &DocView, id: StyleId) -> Option<Dialog> {
    let pap = view.current_pap();
    let st = view.doc.styles.get(id);
    if pap.style != id {
        return None;
    }
    let mut same_pap = pap.clone();
    same_pap.in_table = st.pap.in_table;
    if same_pap == st.pap && view.current_chp() == st.chp {
        return None;
    }
    let mut d = Dialog::new("Style", 360.0, 92.0);
    d.text(&format!("Style: {}", st.name), 14.0, 10.0);
    d.radio(
        "redefine",
        "Redefine the style based on selection?",
        14.0,
        34.0,
        1,
        true,
    );
    d.radio(
        "return",
        "Return the selection to the style?",
        14.0,
        54.0,
        1,
        false,
    );
    d.button("ok", "OK", 286.0, 10.0, 60.0, true);
    d.button("cancel", "Cancel", 286.0, 40.0, 60.0, false);
    Some(d)
}

/// OK in that question.
pub fn reapply_ok(id: StyleId, d: &Dialog, view: &mut DocView) -> bool {
    if d.is_on("redefine") {
        let pap = view.current_pap().clone();
        let chp = view.current_chp();
        view.doc.begin("Define Style", view.sel);
        view.doc.redefine_style(id, pap, chp);
        view.doc.commit(view.sel);
        view.invalidate_layout();
    } else {
        view.set_style(id);
        let (first, last) = view.selected_paras();
        view.doc.begin("Style", view.sel);
        view.doc.revert_to_style(StoryId::Main, first, last);
        view.doc.commit(view.sel);
    }
    true
}

/// What a style or the selection looks like, in the original's words: the base
/// style's name, "+", then what differs ("Normal + Font: Helvetica,
/// Bold, Indent: Left 0.5 in").
pub fn describe(doc: &Document, base: Option<StyleId>, pap: &ParaProps, chp: &CharProps) -> String {
    describe_component(doc, base, pap, chp, true, true)
}

fn describe_component(
    doc: &Document,
    base: Option<StyleId>,
    pap: &ParaProps,
    chp: &CharProps,
    characters: bool,
    paragraph: bool,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    let (bp, bc) = match base {
        Some(b) => {
            let s = doc.styles.get(b);
            (s.pap.clone(), s.chp)
        }
        None => (ParaProps::default(), CharProps::default()),
    };
    let size = |hp: u16| {
        let s = format!("{:.1}", hp as f32 / 2.0);
        s.trim_end_matches(".0").to_string()
    };
    if base.is_none() || (chp.font != bc.font && chp.size != bc.size) {
        parts.push(format!(
            "Font: {} {} Point",
            doc.font_name(chp.font),
            size(chp.size)
        ));
    } else if chp.font != bc.font {
        parts.push(format!("Font: {}", doc.font_name(chp.font)));
    } else if chp.size != bc.size {
        parts.push(format!("{} Point", size(chp.size)));
    }
    let flag = |parts: &mut Vec<String>, on: bool, was: bool, name: &str| {
        if on != was {
            parts.push(if on {
                name.to_string()
            } else {
                format!("Not {name}")
            });
        }
    };
    flag(&mut parts, chp.bold, bc.bold, "Bold");
    flag(&mut parts, chp.italic, bc.italic, "Italic");
    if chp.underline != bc.underline {
        parts.push(
            match chp.underline {
                Underline::None => "No Underline",
                Underline::Single => "Underline",
                Underline::Word => "Word Underline",
                Underline::Double => "Double Underline",
                Underline::Dotted => "Dotted Underline",
            }
            .to_string(),
        );
    }
    flag(&mut parts, chp.strike, bc.strike, "Strikethru");
    flag(&mut parts, chp.outline, bc.outline, "Outline");
    flag(&mut parts, chp.shadow, bc.shadow, "Shadow");
    flag(&mut parts, chp.small_caps, bc.small_caps, "Small Caps");
    flag(&mut parts, chp.all_caps, bc.all_caps, "All Caps");
    flag(&mut parts, chp.hidden, bc.hidden, "Hidden");
    if chp.color != bc.color
        && let Some((name, _)) = WORD_COLORS.iter().find(|(_, c)| *c == chp.color)
    {
        parts.push(format!("Color: {name}"));
    }
    if chp.script != bc.script {
        parts.push(
            match chp.script {
                quill_core::Script::None => "Not Superscript/Subscript",
                quill_core::Script::Super => "Superscript",
                quill_core::Script::Sub => "Subscript",
            }
            .into(),
        );
    }
    if chp.position != bc.position {
        parts.push(match chp.position {
            0 => "Normal Position".into(),
            p if p > 0 => format!("Raised {}", units::points(p)),
            p => format!("Lowered {}", units::points(-p)),
        });
    }
    if chp.spacing != bc.spacing {
        parts.push(match chp.spacing {
            0 => "Normal Spacing".into(),
            s if s > 0 => format!("Expanded {}", units::points(s)),
            s => format!("Condensed {}", units::points(-s)),
        });
    }
    let character_count = parts.len();
    if base.is_none() || pap.align != bp.align {
        parts.push(
            match pap.align {
                Align::Left => "Flush Left",
                Align::Center => "Centered",
                Align::Right => "Flush Right",
                Align::Justify => "Justified",
            }
            .to_string(),
        );
    }
    let mut indent = Vec::new();
    if pap.left != bp.left {
        indent.push(format!("Left {}", units::length(pap.left)));
    }
    if pap.first != bp.first {
        indent.push(format!("First {}", units::length(pap.first)));
    }
    if pap.right != bp.right {
        indent.push(format!("Right {}", units::length(pap.right)));
    }
    if !indent.is_empty() {
        parts.push(format!("Indent: {}", indent.join(" ")));
    }
    if pap.line != bp.line {
        parts.push(match pap.line {
            LineSpacing::Auto => "Line Spacing: Auto".into(),
            LineSpacing::AtLeast(t) => format!("Line Spacing: At Least {}", units::points(t)),
            LineSpacing::Exactly(t) => format!("Line Spacing: Exactly {}", units::points(t)),
            LineSpacing::Multiple(m) => format!("Line Spacing: {} lines", m as f32 / 240.0),
        });
    }
    if pap.before != bp.before {
        parts.push(format!("Space Before {}", units::points(pap.before)));
    }
    if pap.after != bp.after {
        parts.push(format!("Space After {}", units::points(pap.after)));
    }
    flag(
        &mut parts,
        pap.page_break_before,
        bp.page_break_before,
        "Page Break Before",
    );
    flag(
        &mut parts,
        pap.keep_with_next,
        bp.keep_with_next,
        "Keep With Next",
    );
    flag(
        &mut parts,
        pap.keep_together,
        bp.keep_together,
        "Keep Lines Together",
    );
    if pap.tabs != bp.tabs && !pap.tabs.is_empty() {
        let tabs: Vec<String> = pap
            .tabs
            .iter()
            .map(|t| {
                let kind = match t.align {
                    TabAlign::Left => "",
                    TabAlign::Center => " Centered",
                    TabAlign::Right => " Flush Right",
                    TabAlign::Decimal => " Decimal",
                    TabAlign::Bar => " Vertical Bar",
                };
                format!("{}{kind}", units::length(t.pos))
            })
            .collect();
        parts.push(format!("Tabs: {}", tabs.join(", ")));
    }
    if pap.borders != bp.borders && !pap.borders.is_none() {
        let b = &pap.borders;
        let boxed = [b.top, b.bottom, b.left, b.right]
            .iter()
            .all(|l| l.style != BorderStyle::None);
        parts.push(if b.shadow {
            "Border: Shadow".into()
        } else if boxed {
            "Border: Box".into()
        } else {
            "Border".into()
        });
    }
    if (pap.shading, pap.shading_color) != (bp.shading, bp.shading_color) {
        parts.push(match pap.shading_color {
            Some(color) => format!("Shading: #{:06X}", color.0),
            None if pap.shading == 0 => "Shading: None".into(),
            None => format!("Shading: {}%", pap.shading as f32 / 10.0),
        });
    }
    if !paragraph {
        parts.truncate(character_count);
    }
    if !characters {
        parts.drain(..character_count);
    }
    let body = parts.join(", ");
    match base {
        Some(b) => {
            let name = doc.styles.get(b).display_name();
            if body.is_empty() {
                name
            } else {
                format!("{name} + {body}")
            }
        }
        None => body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> DocView {
        let mut v = DocView::new(Document::new(), "Untitled1".into());
        v.type_text("Hello");
        v
    }

    /// A document with a "Loud" style (bold, based on Normal) and a "Quiet"
    /// style that is based on Loud and followed by Loud.
    fn styled() -> Document {
        let mut doc = Document::new();
        let loud = doc.add_style(
            "Loud",
            0,
            None,
            doc.styles.get(0).pap.clone(),
            CharProps {
                bold: true,
                ..doc.styles.get(0).chp
            },
        );
        doc.add_style(
            "Quiet",
            loud,
            Some(loud),
            doc.styles.get(0).pap.clone(),
            doc.styles.get(0).chp,
        );
        doc
    }

    #[test]
    fn clear_formatting_goes_back_to_plain_normal_and_undoes_in_one_step() {
        let mut v = DocView::new(styled(), "Untitled1".into());
        v.type_text("Hello");
        let loud = v.doc.styles.find("Loud").unwrap();
        v.set_style(loud);
        v.set_selection(quill_core::Selection {
            anchor: quill_core::Position::new(0, 0),
            focus: quill_core::Position::new(0, 5),
        });
        v.format_chars("Italic", |c| c.italic = true);
        v.doc.begin("t", v.sel);
        v.doc
            .format_paras(quill_core::StoryId::Main, 0, 0, |p| p.left = 720);
        v.doc.commit(v.sel);
        assert!(v.current_chp().bold && v.current_chp().italic);
        v.clear_formatting();
        assert_eq!(v.current_pap().style, 0);
        assert_eq!(v.current_pap().left, 0);
        let c = v.doc.main.paras[0].chp_of_char(0);
        assert!(!c.bold && !c.italic);
        assert_eq!(c.character_style, 0);
        assert_eq!(v.doc.main.paras[0].text, "Hello");
        v.doc.undo();
        assert_eq!(v.doc.main.paras[0].pap.style, loud);
        assert_eq!(v.doc.main.paras[0].pap.left, 720);
        assert!(v.doc.main.paras[0].chp_of_char(0).italic);
    }

    #[test]
    fn the_lists_clear_formatting_buttons_work_on_paragraphs_and_on_text() {
        let mut v = DocView::new(styled(), "Untitled1".into());
        v.type_text("Hello World");
        let loud = v.doc.styles.find("Loud").unwrap();
        v.set_style(loud);
        v.doc.begin("t", v.sel);
        v.doc
            .format_paras(quill_core::StoryId::Main, 0, 0, |p| p.left = 720);
        v.doc.commit(v.sel);
        let select = |v: &mut DocView, end: usize| {
            v.set_selection(quill_core::Selection {
                anchor: quill_core::Position::new(0, 0),
                focus: quill_core::Position::new(0, end),
            })
        };
        select(&mut v, 11);
        v.format_chars("Italic", |c| c.italic = true);
        select(&mut v, 5);
        // Character Styles: only the selected text loses what was set by
        // hand. It goes back to the paragraph's style, which is bold here.
        let mut c = crate::character_style_dialog::dialog(&v);
        crate::character_style_dialog::button(&mut c, &mut v, "clear");
        assert!(!v.doc.main.paras[0].chp_of_char(0).italic);
        assert!(v.doc.main.paras[0].chp_of_char(0).bold);
        assert!(
            v.doc.main.paras[0].chp_of_char(8).italic,
            "other text is left"
        );
        assert_eq!(v.doc.main.paras[0].pap.style, loud);
        assert_eq!(v.doc.main.paras[0].pap.left, 720);
        // Paragraph Styles: the whole paragraph goes back to plain Normal.
        let (mut d, mut st) = list(&v);
        list_button(&mut st, &mut d, &mut v, "clear");
        assert_eq!(v.current_pap().style, 0);
        assert_eq!(v.current_pap().left, 0);
        assert!(!v.doc.main.paras[0].chp_of_char(8).bold);
        assert!(!v.doc.main.paras[0].chp_of_char(8).italic);
    }

    #[test]
    fn the_list_has_the_same_buttons_as_character_styles() {
        let v = view();
        let (d, _) = list(&v);
        let c = crate::character_style_dialog::dialog(&v);
        for id in [
            "list",
            "new",
            "delete",
            "edit",
            "duplicate",
            "apply",
            "remove",
            "update",
            "clear",
            "ok",
        ] {
            let a = d.items.iter().find(|i| i.id == id).expect(id);
            let b = c.items.iter().find(|i| i.id == id).expect(id);
            assert_eq!(a.label, b.label, "{id}");
            assert_eq!(a.rect.x, b.rect.x, "{id}");
            assert_eq!(a.rect.w, b.rect.w, "{id}");
        }
    }

    #[test]
    fn update_to_selection_redefines_only_when_the_selection_differs() {
        let mut v = view();
        let id = v.doc.add_style(
            "Loud",
            0,
            None,
            v.current_pap().clone(),
            v.doc.styles.get(0).chp,
        );
        // The selection already looks like it: nothing to do.
        assert!(!update(&mut v, id));
        v.format_chars("Bold", |c| c.bold = true);
        assert!(update(&mut v, id));
        assert!(v.doc.styles.get(id).chp.bold);
        v.doc.undo();
        assert!(!v.doc.styles.get(id).chp.bold);
    }

    #[test]
    fn list_buttons_duplicate_apply_and_remove() {
        let mut v = DocView::new(styled(), "Untitled1".into());
        v.type_text("Hello");
        let (mut d, mut st) = list(&v);
        let loud = v.doc.styles.find("Loud").unwrap();
        refresh_list(&mut st, &mut d, &v, Some(loud));
        list_button(&mut st, &mut d, &mut v, "apply");
        assert_eq!(v.current_pap().style, loud);
        list_button(&mut st, &mut d, &mut v, "remove");
        assert_eq!(v.current_pap().style, 0);
        list_button(&mut st, &mut d, &mut v, "duplicate");
        let copy = v.doc.styles.find("Loud copy").expect("the copy");
        assert_eq!(list_selected(&st, &d), Some(copy));
    }

    #[test]
    fn the_editor_opens_on_the_style_it_was_given() {
        let v = view();
        let (d, st) = editor(&v, Some(0));
        assert!(
            !d.items
                .iter()
                .any(|item| matches!(item.kind, crate::dialog::Kind::Custom))
        );
        assert_eq!(st.entries[0], None);
        assert_eq!(editing(&st), Some(0));
        assert_eq!(d.field_text("name"), "Normal");
        assert_eq!(d.title, "Paragraph Style: Normal");
        assert!(
            d.items
                .iter()
                .find(|item| item.id == "desc0")
                .is_some_and(|item| !item.label.is_empty())
        );
        // It edits one style: no list of its own and no Delete.
        assert!(!d.items.iter().any(|i| matches!(i.id, "list" | "delete")));
    }

    #[test]
    fn defines_and_applies_a_new_style_from_the_selection() {
        let mut v = view();
        v.format_chars("Bold", |c| c.bold = true);
        let (mut d, mut st) = editor(&v, None);
        assert_eq!(d.title, "New Paragraph Style");
        // A new style needs a name before OK can save it.
        assert!(!save(&mut st, &mut d, &mut v));
        d.set_field("name", "Loud");
        assert!(apply(&mut st, &mut d, &mut v));
        let id = v.doc.styles.find("Loud").unwrap();
        assert_eq!(v.current_pap().style, id);
        assert_eq!(editing(&st), Some(id));
        assert_eq!(d.title, "Paragraph Style: Loud");
        // Undo takes the style away again.
        v.doc.undo();
        v.doc.undo();
        assert!(v.doc.styles.find("Loud").is_none());
    }

    #[test]
    fn the_list_shows_the_documents_styles_or_all_of_them() {
        let v = view();
        let (mut d, mut st) = list(&v);
        assert_eq!(d.title, "Paragraph Styles");
        let few = st.ids.len();
        assert_eq!(list_selected(&st, &d), Some(0), "on Normal");
        // Normal cannot be deleted, but can be edited.
        assert!(d.is_enabled("edit"));
        assert!(!d.is_enabled("delete"));
        d.set_on("allstyles", true);
        d.set_on("docstyles", false);
        list_changed(&mut st, &mut d, &v, "allstyles");
        assert!(st.ids.len() > few + 20);
        assert_eq!(list_selected(&st, &d), Some(0), "still on Normal");
    }

    #[test]
    fn deleting_a_style_in_use_asks_what_its_paragraphs_become() {
        let mut v = view();
        let pap = v.current_pap().clone();
        let chp = v.current_chp();
        let a = v.doc.add_style("Alpha", 0, None, pap.clone(), chp);
        let b = v.doc.add_style("Beta", 0, None, pap, chp);
        // Not in use: it goes at once.
        assert!(delete_from_list(&mut v, b).is_none());
        assert!(v.doc.styles.find("Beta").is_none());
        let b = v
            .doc
            .add_style("Beta", 0, None, v.current_pap().clone(), chp);
        v.set_style(a);
        let (mut d, choices) = delete_from_list(&mut v, a).expect("asks");
        assert!(v.doc.styles.find("Alpha").is_some(), "nothing goes yet");
        assert_eq!(d.options("replacement")[0], "No Style (Normal)");
        assert!(!choices.contains(&a) && !choices.contains(&0));
        let k = choices.iter().position(|&c| c == b).unwrap();
        d.set_selected("replacement", k + 1);
        delete_question_ok(a, &choices, &d, &mut v);
        let beta = v.doc.styles.find("Beta").unwrap();
        assert_eq!(v.current_pap().style, beta);
        assert!(v.doc.styles.find("Alpha").is_none());
        // Undo brings it back, and No Style gives Normal.
        v.doc.undo();
        let a = v.doc.styles.find("Alpha").unwrap();
        assert_eq!(v.current_pap().style, a);
        let (d, choices) = delete_from_list(&mut v, a).expect("asks");
        delete_question_ok(a, &choices, &d, &mut v);
        assert_eq!(v.current_pap().style, 0);
    }

    #[test]
    fn reapplying_asks_only_when_the_paragraph_was_reformatted() {
        let mut v = view();
        assert!(reapply(&v, 0).is_none());
        v.exec(
            crate::commands::Cmd::AlignCenter,
            false,
            &mut crate::docview::Clipboard::new(),
        );
        let mut d = reapply(&v, 0).expect("asks");
        assert!(reapply_ok(0, &d, &mut v));
        assert_eq!(
            v.doc.styles.get(0).pap.align,
            quill_core::props::Align::Center
        );
        // And the other way: put the paragraph back.
        v.exec(
            crate::commands::Cmd::AlignRight,
            false,
            &mut crate::docview::Clipboard::new(),
        );
        d = reapply(&v, 0).expect("asks");
        d.set_on("redefine", false);
        d.set_on("return", true);
        assert!(reapply_ok(0, &d, &mut v));
        assert_eq!(v.current_pap().align, quill_core::props::Align::Center);
    }

    #[test]
    fn describes_differences_from_the_base_style() {
        let d = Document::new();
        let h1 = d.styles.find("heading 1").unwrap();
        let s = d.styles.get(h1);
        let text = describe(&d, s.based_on, &s.pap, &s.chp);
        assert!(text.starts_with("Normal + Font: Helvetica"), "{text}");
        assert!(text.contains("Bold"), "{text}");
        let n = d.styles.get(0);
        assert_eq!(
            describe(&d, None, &n.pap, &n.chp),
            "Font: New York 12 Point, Flush Left"
        );
    }
}
