//! Format > Style (⌘T): the document's style sheet. Pick a style to see
//! its description, Based On and Next Style; type a name under New Style
//! and Define to make a style from the selection's formatting; Apply puts
//! the chosen style on the selected paragraphs. Layout measured from the
//! original.

use crate::dialog::{Dialog, units};
use crate::docview::DocView;
use quill_core::props::{
    Align, BorderStyle, CharProps, LineSpacing, ParaProps, StyleId, TabAlign, Underline,
    WORD_COLORS,
};
use quill_core::{Document, StoryId};

#[derive(Clone, Debug)]
pub struct StyleState {
    /// Show every style, not just the document's.
    pub all: bool,
    /// The list: None is "New Style".
    pub entries: Vec<Option<StyleId>>,
    pub selected: usize,
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

fn entries(view: &DocView, all: bool) -> Vec<Option<StyleId>> {
    let ids = if all {
        sorted(&view.doc, 0..view.doc.styles.styles.len() as StyleId)
    } else {
        document_styles(view)
    };
    std::iter::once(None)
        .chain(ids.into_iter().map(Some))
        .collect()
}

pub fn style(view: &DocView) -> (Dialog, StyleState) {
    let mut d = Dialog::new(&format!("Style: {}", view.title), 348.0, 286.0);
    d.close_box = true;
    let style_entries = entries(view, false);
    let style_names = entry_names(view, &style_entries);
    d.popup_owned("list", style_names, 0, 14.0, 9.0, 254.0);
    d.button("ok", "OK", 279.0, 14.0, 53.0, true);
    d.button("cancel", "Cancel", 279.0, 40.0, 53.0, false);
    d.button("apply", "Apply", 279.0, 66.0, 53.0, false);
    d.button("define", "Define", 279.0, 92.0, 53.0, false);
    d.button("delete", "Delete", 279.0, 115.0, 53.0, false);
    d.text("Show:", 15.0, 43.0);
    d.radio("docstyles", "Document Styles", 60.0, 42.0, 1, true);
    d.radio("allstyles", "All Styles", 192.0, 42.0, 1, false);
    d.text("Style:", 15.0, 68.0);
    d.field("name", "", 14.0, 84.0, 209.0);
    d.button("usedefault", "Use As Default", 226.0, 85.0, 106.0, false);
    d.disable("usedefault");
    d.group("Description", 14.0, 112.0, 320.0, 109.0);
    for (line, y) in [
        ("desc0", 128.0),
        ("desc1", 146.0),
        ("desc2", 164.0),
        ("desc3", 182.0),
        ("desc4", 200.0),
    ] {
        d.note(line, "", 21.0, y, 305.0);
    }
    d.text("Based On:", 15.0, 230.0);
    d.text("Next Style:", 15.0, 256.0);
    let choices = sorted(&view.doc, 0..view.doc.styles.styles.len() as StyleId);
    let names = choice_names(view, &choices);
    d.popup_owned("basedon", names.clone(), 0, 93.0, 227.0, 241.0);
    d.popup_owned("next", names, 0, 93.0, 253.0, 241.0);
    let mut st = StyleState {
        all: false,
        entries: style_entries,
        selected: 0,
        choices,
    };
    // Start on the current paragraph's style, as the original does.
    let cur = view.current_pap().style;
    if let Some(i) = st.entries.iter().position(|e| *e == Some(cur)) {
        select(&mut st, &mut d, view, i);
    }
    (d, st)
}

/// Show entry `i` in the fields.
fn select(st: &mut StyleState, d: &mut Dialog, view: &DocView, i: usize) {
    st.selected = i.min(st.entries.len() - 1);
    d.set_selected("list", st.selected);
    let at = |id: Option<StyleId>| {
        id.and_then(|id| st.choices.iter().position(|&c| c == id))
            .map_or(0, |k| k + 1)
    };
    match st.entries[st.selected] {
        None => {
            d.set_field("name", "");
            d.set_selected("basedon", at(Some(view.current_pap().style)));
            d.set_selected("next", 0);
            d.enable("delete", false);
        }
        Some(id) => {
            let s = view.doc.styles.get(id);
            d.set_field("name", &s.name);
            d.set_selected("basedon", at(s.based_on));
            d.set_selected("next", if s.next == id { 0 } else { at(Some(s.next)) });
            d.enable("delete", !s.builtin);
        }
    }
    update_description(st, d, view);
    d.focus("name");
}

/// The Based On and Next Style popup items: a blank, then `choices`.
fn choice_names(view: &DocView, choices: &[StyleId]) -> Vec<String> {
    std::iter::once(String::new())
        .chain(
            choices
                .iter()
                .map(|&id| view.doc.styles.get(id).name.clone()),
        )
        .collect()
}

fn entry_names(view: &DocView, entries: &[Option<StyleId>]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| match entry {
            None => "New Style".to_string(),
            Some(id) => view.doc.styles.get(*id).name.clone(),
        })
        .collect()
}

fn update_entry_options(st: &StyleState, d: &mut Dialog, view: &DocView) {
    d.set_options("list", entry_names(view, &st.entries), st.selected);
}

fn update_description(st: &StyleState, d: &mut Dialog, view: &DocView) {
    let text = description(st, view);
    let mut lines = vec![String::new()];
    for word in text.split_whitespace() {
        let should_wrap = lines
            .last()
            .is_some_and(|line| !line.is_empty() && line.len() + word.len() + 1 > 62)
            && lines.len() < 5;
        if should_wrap {
            lines.push(word.to_string());
        } else {
            let line = lines.last_mut().expect("description always has a line");
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
    }
    for index in 0..5 {
        d.set_label(
            match index {
                0 => "desc0",
                1 => "desc1",
                2 => "desc2",
                3 => "desc3",
                _ => "desc4",
            },
            lines.get(index).map_or("", String::as_str),
        );
    }
}

/// The style a popup names, if any.
fn chosen(st: &StyleState, d: &Dialog, id: &str) -> Option<StyleId> {
    let k = d.selected(id);
    (k > 0).then(|| st.choices.get(k - 1).copied()).flatten()
}

pub fn changed(st: &mut StyleState, d: &mut Dialog, view: &DocView, id: &str) {
    if id == "list" {
        select(st, d, view, d.selected("list"));
        return;
    }
    if id == "docstyles" || id == "allstyles" {
        let keep = st.entries[st.selected];
        st.all = id == "allstyles";
        st.entries = entries(view, st.all);
        let i = st.entries.iter().position(|e| *e == keep).unwrap_or(0);
        update_entry_options(st, d, view);
        select(st, d, view, i);
    }
}

/// Define: make a new style from the selection, or change the chosen
/// style's name, Based On and Next Style. Returns the style, or None if
/// there is nothing to define (New Style with no name).
pub fn define(st: &mut StyleState, d: &mut Dialog, view: &mut DocView) -> Option<StyleId> {
    let name = d.field_text("name").trim().to_string();
    let based = chosen(st, d, "basedon");
    let next = chosen(st, d, "next");
    let id = match st.entries[st.selected] {
        None if name.is_empty() => return None,
        None => {
            let pap = view.current_pap().clone();
            let chp = view.current_chp();
            view.doc.begin("Define Style", view.sel);
            let id = match view.doc.styles.find(&name) {
                // Defining an existing name redefines it from the selection.
                Some(id) => {
                    view.doc.redefine_style(StoryId::Main, id, pap, chp);
                    id
                }
                None => view
                    .doc
                    .add_style(&name, based.unwrap_or(0), next, pap, chp),
            };
            view.doc.commit(view.sel);
            id
        }
        Some(id) => {
            let s = view.doc.styles.get(id);
            let new_name = if name.is_empty() {
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
            view.doc.begin("Define Style", view.sel);
            view.doc
                .rename_style(id, &new_name, based, next.unwrap_or(id));
            view.doc.commit(view.sel);
            id
        }
    };
    // The list may have gained the style.
    st.choices = sorted(&view.doc, 0..view.doc.styles.styles.len() as StyleId);
    let names = choice_names(view, &st.choices);
    d.set_options("basedon", names.clone(), 0);
    d.set_options("next", names, 0);
    st.entries = entries(view, st.all);
    let i = st.entries.iter().position(|e| *e == Some(id)).unwrap_or(0);
    update_entry_options(st, d, view);
    select(st, d, view, i);
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

/// Delete the chosen style; its paragraphs return to Normal.
pub fn delete(st: &mut StyleState, d: &mut Dialog, view: &mut DocView) {
    let Some(id) = st.entries[st.selected] else {
        return;
    };
    if view.doc.styles.get(id).builtin {
        return;
    }
    view.doc.begin("Delete Style", view.sel);
    view.doc.delete_style(StoryId::Main, id);
    view.doc.commit(view.sel);
    view.invalidate_layout();
    st.choices = sorted(&view.doc, 0..view.doc.styles.styles.len() as StyleId);
    let names = choice_names(view, &st.choices);
    d.set_options("basedon", names.clone(), 0);
    d.set_options("next", names, 0);
    st.entries = entries(view, st.all);
    update_entry_options(st, d, view);
    select(st, d, view, st.selected.min(st.entries.len() - 1));
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
        view.doc.redefine_style(StoryId::Main, id, pap, chp);
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
    if chp.position != bc.position {
        parts.push(match chp.position {
            0 => "Normal Position".into(),
            p if p > 0 => format!("Superscript {}", units::points(p)),
            p => format!("Subscript {}", units::points(-p)),
        });
    }
    if chp.spacing != bc.spacing {
        parts.push(match chp.spacing {
            0 => "Normal Spacing".into(),
            s if s > 0 => format!("Expanded {}", units::points(s)),
            s => format!("Condensed {}", units::points(-s)),
        });
    }
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
    if pap.shading != bp.shading && pap.shading > 0 {
        parts.push(format!("Shading: {}%", pap.shading as f32 / 10.0));
    }
    let body = parts.join(", ");
    match base {
        Some(b) => {
            let name = &doc.styles.get(b).name;
            if body.is_empty() {
                name.clone()
            } else {
                format!("{name} + {body}")
            }
        }
        None => body,
    }
}

/// The description for the list's current entry.
fn description(st: &StyleState, view: &DocView) -> String {
    let doc = &view.doc;
    match st.entries.get(st.selected).copied().flatten() {
        // New Style: the selection's formatting, against its style.
        None => {
            let pap = view.current_pap();
            describe(doc, Some(pap.style), pap, &view.current_chp())
        }
        Some(id) => {
            let s = doc.styles.get(id);
            describe(doc, s.based_on, &s.pap, &s.chp)
        }
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

    #[test]
    fn opens_on_the_current_style() {
        let v = view();
        let (d, st) = style(&v);
        assert!(
            !d.items
                .iter()
                .any(|item| matches!(item.kind, crate::dialog::Kind::Custom))
        );
        assert_eq!(st.entries[0], None);
        assert_eq!(st.entries[st.selected], Some(0));
        assert_eq!(d.field_text("name"), "Normal");
        assert_eq!(d.title, "Style: Untitled1");
        assert!(
            d.items
                .iter()
                .find(|item| item.id == "desc0")
                .is_some_and(|item| !item.label.is_empty())
        );
    }

    #[test]
    fn defines_and_applies_a_new_style_from_the_selection() {
        let mut v = view();
        v.format_chars("Bold", |c| c.bold = true);
        let (mut d, mut st) = style(&v);
        d.set_selected("list", 0);
        changed(&mut st, &mut d, &v, "list");
        assert_eq!(st.selected, 0);
        d.set_field("name", "Loud");
        assert!(apply(&mut st, &mut d, &mut v));
        let id = v.doc.styles.find("Loud").unwrap();
        assert_eq!(v.current_pap().style, id);
        assert_eq!(st.entries[st.selected], Some(id));
        // Undo takes the style away again.
        v.doc.undo();
        v.doc.undo();
        assert!(v.doc.styles.find("Loud").is_none());
    }

    #[test]
    fn all_styles_lists_the_automatic_ones() {
        let v = view();
        let (mut d, mut st) = style(&v);
        let few = st.entries.len();
        d.set_on("allstyles", true);
        d.set_on("docstyles", false);
        changed(&mut st, &mut d, &v, "allstyles");
        assert!(st.entries.len() > few + 20);
        assert_eq!(st.entries[st.selected], Some(0), "still on Normal");
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
