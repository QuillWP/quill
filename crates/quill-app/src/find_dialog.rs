//! Edit > Find (⌘F), Replace (⌘H) and Go To (⌘G). Find and Replace are
//! modeless windows, as in the original; what they last looked for is kept
//! for Find Again (⌘⌥A) and the next time they open.

use crate::dialog::{Dialog, Kind, units};
use crate::docview::{Clipboard, DocView};
use quill_core::find::{
    self, CharSpec, Direction, FIND_SPECIALS, Pattern, REPLACE_SPECIALS, Replacement,
};
use quill_core::{Position, Selection};
use std::cell::RefCell;

/// Search popup choices. Selection is offered only with text selected.
pub const DOWN: usize = 0;
pub const UP: usize = 1;
pub const ALL: usize = 2;
pub const SELECTION: usize = 3;

/// Everything Find and Replace remember.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Search {
    pub find: String,
    pub replace: String,
    pub match_case: bool,
    pub whole_word: bool,
    pub dir: usize,
}

impl Search {
    pub fn pattern(&self) -> Pattern {
        Pattern::new(
            &self.find,
            self.match_case,
            self.whole_word && !find::has_specials(&self.find),
        )
    }

    /// The pattern as `view` shows the text: while deleted text is hidden,
    /// Find looks past it, as the reader does.
    pub fn pattern_in(&self, view: &DocView) -> Pattern {
        let mut pat = self.pattern();
        pat.skip_deleted = !view.show_changes;
        pat
    }

    pub fn replacement(&self) -> Replacement {
        Replacement::new(&self.replace)
    }
}

thread_local! {
    static LAST: RefCell<Search> = RefCell::new(Search::default());
}

pub fn last() -> Search {
    LAST.with(|l| l.borrow().clone())
}

fn remember(s: &Search) {
    LAST.with(|l| *l.borrow_mut() = s.clone());
}

/// The Find or Replace window's own state.
#[derive(Clone, Debug)]
pub struct FindState {
    pub replace: bool,
    pub search: Search,
    /// The last match found and selected, which Replace replaces.
    pub found: Option<(Position, Position)>,
}

/// Build the Find (`replace` false) or Replace window.
pub fn dialog(view: &DocView, replace: bool) -> (Dialog, FindState) {
    let mut search = last();
    let has_sel = !view.sel.is_empty();
    // With text selected the search starts on the selection, as in the original.
    if has_sel && view.sel.start().para != view.sel.end().para {
        search.dir = SELECTION;
    } else if search.dir == SELECTION {
        search.dir = DOWN;
    }
    let title = "Find";
    let h = if replace { 238.0 } else { 142.0 };
    let mut d = Dialog::new(title, 465.0, h);
    d.modeless = true;
    d.close_box = true;
    let lx = 8.0;
    d.text("Find What:", lx, 7.0);
    d.field("find", &search.find, 102.0, 5.0, 250.0);
    let specials: Vec<&str> = FIND_SPECIALS.iter().map(|(n, _)| *n).collect();
    d.menu_button("spc_find", "Special", &specials, 23.0, 27.0, 75.0);
    let (checks_y, search_y) = if replace {
        d.text("Replace With:", 8.0, 150.0);
        d.field("with", &search.replace, 102.0, 148.0, 250.0);
        let specials: Vec<&str> = REPLACE_SPECIALS.iter().map(|(n, _)| *n).collect();
        d.menu_button("spc_rep", "Special", &specials, 23.0, 170.0, 75.0);
        d.button("findnext", "Find Next", 369.0, 9.0, 88.0, true);
        d.button("replace", "Replace", 369.0, 152.0, 88.0, false);
        d.button("replaceall", "Replace All", 369.0, 178.0, 88.0, false);
        d.button("cancel", "Cancel", 369.0, 36.0, 88.0, false);
        (84.0, 66.0)
    } else {
        d.button("findnext", "Find Next", 369.0, 9.0, 88.0, true);
        d.button("cancel", "Cancel", 369.0, 36.0, 88.0, false);
        (84.0, 66.0)
    };
    d.check(
        "whole",
        "Match Whole Word Only",
        8.0,
        checks_y,
        search.whole_word,
    );
    d.check("case", "Match Case", 195.0, checks_y, search.match_case);
    d.text("Search:", 376.0, search_y);
    let mut dirs = vec!["Down", "Up", "All"];
    if has_sel {
        dirs.push("Selection");
    }
    d.popup(
        "dir",
        &dirs,
        search.dir.min(dirs.len() - 1),
        367.0,
        search_y + 15.0,
        89.0,
    );
    d.button(
        "mode",
        if replace {
            "Hide Replace"
        } else {
            "Replace…"
        },
        8.0,
        110.0,
        110.0,
        false,
    );
    d.focus("find");
    let mut st = FindState {
        replace,
        search,
        found: None,
    };
    update(&mut st, &mut d, view);
    (d, st)
}

/// Change modes without losing the search, replacement or current match.
pub fn switch_mode(st: &mut FindState, d: &mut Dialog, view: &DocView, replace: bool) {
    update(st, d, view);
    let found = st.found;
    let (mut next, mut state) = dialog(view, replace);
    next.origin = d.origin;
    if let Some(old) = d.items.iter().find(|i| i.id == "find")
        && let Kind::Field { sel, .. } = &old.kind
        && let Some(new) = next.items.iter_mut().find(|i| i.id == "find")
        && let Kind::Field { sel: next_sel, .. } = &mut new.kind
    {
        *next_sel = *sel;
    }
    state.search = st.search.clone();
    next.set_selected("dir", st.search.dir);
    state.found = found;
    *d = next;
    *st = state;
    update(st, d, view);
}

/// Read the window into the search and set what can be pressed.
pub fn update(st: &mut FindState, d: &mut Dialog, view: &DocView) {
    let s = &mut st.search;
    s.find = d.field_text("find");
    if st.replace {
        s.replace = d.field_text("with");
    }
    s.match_case = d.is_on("case");
    s.whole_word = d.is_on("whole");
    s.dir = d.selected("dir");
    let can_find = !s.pattern().is_empty();
    d.enable("findnext", can_find);
    d.enable("replaceall", st.replace && can_find);
    let on_match = st.found.is_some_and(|(a, b)| view.sel.range() == (a, b));
    d.enable("replace", st.replace && can_find && on_match);
    d.enable("whole", !find::has_specials(&s.find));
    remember(s);
}

/// A control in the Find window changed.
pub fn changed(st: &mut FindState, d: &mut Dialog, view: &DocView, id: &str) {
    if !st.replace && matches!(id, "spc_rep" | "with") {
        return;
    }
    match id {
        "spc_find" => {
            if let Some((_, code)) = FIND_SPECIALS.get(d.selected(id)) {
                d.insert_in_field("find", code);
            }
        }
        "spc_rep" => {
            if let Some((_, code)) = REPLACE_SPECIALS.get(d.selected(id)) {
                d.insert_in_field("with", code);
            }
        }
        _ => {}
    }
    update(st, d, view);
}

/// How a search went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Match,
    NotFound,
    /// Reached the end (or start) of the document partway through: ask
    /// whether to go on from the other end.
    ReachedEnd,
}

/// Find the next match from the selection and select it.
pub fn find_next(view: &mut DocView, s: &Search) -> Hit {
    let pat = s.pattern_in(view);
    if pat.is_empty() {
        return Hit::NotFound;
    }
    let story = &view.doc.main;
    let up = s.dir == UP;
    let (from, dir) = if up {
        (view.sel.start(), Direction::Up)
    } else {
        (view.sel.end(), Direction::Down)
    };
    if let Some((a, b)) = find::find(story, &pat, from, dir) {
        view.select_found(a, b);
        return Hit::Match;
    }
    match s.dir {
        ALL => {
            // All goes round to the top without asking.
            match find::find(story, &pat, story.start(), Direction::Down) {
                Some((a, b)) if a < from || view.sel.is_empty() => {
                    view.select_found(a, b);
                    Hit::Match
                }
                _ => Hit::NotFound,
            }
        }
        UP if from == story.end() => Hit::NotFound,
        DOWN if from == story.start() => Hit::NotFound,
        UP | DOWN => Hit::ReachedEnd,
        _ => Hit::NotFound,
    }
}

/// Go on from the other end of the document after `ReachedEnd`.
pub fn find_wrapped(view: &mut DocView, s: &Search) -> bool {
    let pat = s.pattern_in(view);
    let story = &view.doc.main;
    let found = if s.dir == UP {
        find::find(story, &pat, story.end(), Direction::Up)
    } else {
        find::find(story, &pat, story.start(), Direction::Down)
    };
    match found {
        Some((a, b)) => {
            view.select_found(a, b);
            true
        }
        None => false,
    }
}

/// Replace: replace the match that is selected, then find the next one.
pub fn replace(st: &mut FindState, view: &mut DocView, clip: &mut Clipboard) -> Hit {
    if !st.replace {
        return Hit::NotFound;
    }
    let s = st.search.clone();
    if let Some((a, b)) = st.found
        && view.sel.range() == (a, b)
    {
        let frag = if s.replace.contains("^c") {
            clip.fragment()
        } else {
            None
        };
        // Find Next goes on from past the old text, which tracking keeps.
        let past = find::FromEnd::new(&view.doc.main, b);
        let (_, e) = view.replace_found(a, b, &s.replacement(), s.match_case, frag.as_ref());
        let next = e.max(past.get(&view.doc.main));
        view.set_selection(Selection::caret(next));
    }
    let r = find_next(view, &s);
    st.found = (r == Hit::Match).then(|| view.sel.range());
    r
}

/// Replace All. Returns how many were replaced and whether it worked on
/// the selection only.
pub fn replace_all(st: &mut FindState, view: &mut DocView, clip: &mut Clipboard) -> (usize, bool) {
    if !st.replace {
        return (0, false);
    }
    let s = st.search.clone();
    let story = &view.doc.main;
    let (from, to) = match s.dir {
        SELECTION => view.sel.range(),
        DOWN => (view.sel.start(), story.end()),
        UP => (story.start(), view.sel.end()),
        _ => (story.start(), story.end()),
    };
    let frag = if s.replace.contains("^c") {
        clip.fragment()
    } else {
        None
    };
    let n = view.replace_all(
        &s.pattern_in(view),
        &s.replacement(),
        from,
        to,
        frag.as_ref(),
    );
    st.found = None;
    (n, s.dir == SELECTION)
}

// ----- the format dialogs -----

/// The Character dialog for a character style: every control starts blank
/// or gray, meaning the style leaves it to the paragraph.
pub fn character_spec(
    view: &DocView,
    fonts: &[quill_layout::FontFamilyInfo],
    spec: &CharSpec,
) -> Dialog {
    let mut family_choices = vec![quill_layout::FontFamilyInfo {
        name: String::new(),
        draws_name: false,
        variants: Vec::new(),
    }];
    family_choices.extend_from_slice(fonts);
    let (mut d, _) = crate::format_dialogs::character_for_find(view, &family_choices);
    let names = d.options("font");
    // With no font, the blank choice: the dialog may have put the document's
    // own font ahead of it.
    let blank = names.iter().position(|n| n.is_empty()).unwrap_or(0);
    let font = spec
        .font
        .map(|f| view.doc.font_name(f).to_string())
        .and_then(|n| names.iter().position(|x| x.eq_ignore_ascii_case(&n)))
        .unwrap_or(blank);
    d.set_selected("font", font);
    d.set_field(
        "size",
        &spec.size.map(crate::bars::format_size).unwrap_or_default(),
    );
    let mut u = vec![String::new()];
    u.extend(d.options("underline"));
    let ui = spec
        .underline
        .and_then(|x| {
            crate::format_dialogs::UNDERLINE_KINDS
                .iter()
                .position(|k| *k == x)
        })
        .map_or(0, |i| i + 1);
    d.set_options("underline", u, ui);
    // A box with the palette beside it, as in Format > Character. The hex
    // code is blank for "any color", and None puts it back to that.
    d.items.retain(|item| item.id != "color");
    d.swatch_button(
        "color",
        146.0,
        74.0,
        44.0,
        22.0,
        spec.color.map_or(0xFFFFFF, |c| c.0),
    );
    d.text_as(
        "color_hex",
        &spec.color.map_or(String::new(), crate::palette::hex),
        198.0,
        78.0,
    );
    d.button("color_clear", "None", 252.0, 74.0, 44.0, false);
    for (id, v) in [
        ("bold", spec.bold),
        ("italic", spec.italic),
        ("outline", spec.outline),
        ("shadow", spec.shadow),
        ("strike", spec.strike),
        ("smallcaps", spec.small_caps),
        ("allcaps", spec.all_caps),
        ("hidden", spec.hidden),
    ] {
        d.set_check3(id, v);
    }
    for (id, on) in [
        ("pos_normal", spec.script == Some(quill_core::Script::None)),
        ("pos_super", spec.script == Some(quill_core::Script::Super)),
        ("pos_sub", spec.script == Some(quill_core::Script::Sub)),
        ("sp_normal", spec.spacing == Some(0)),
        ("sp_cond", spec.spacing.is_some_and(|p| p < 0)),
        ("sp_exp", spec.spacing.is_some_and(|p| p > 0)),
    ] {
        d.set_on(id, on);
    }
    let by = |v: Option<i32>| match v {
        Some(t) if t != 0 => units::points(t.abs()),
        _ => String::new(),
    };
    d.set_field(
        "pos_by",
        &spec
            .position
            .filter(|&p| p != 0)
            .map(units::points)
            .unwrap_or_default(),
    );
    d.set_field("sp_by", &by(spec.spacing));
    d.enable("apply", false);
    d.focus("size");
    d
}

/// The color the box of a Character format dialog shows, or None for any.
pub fn spec_color(d: &Dialog) -> Option<quill_core::Rgb> {
    if d.item_label("color_hex").is_empty() {
        return None;
    }
    d.swatch_rgb("color").map(quill_core::Rgb)
}

pub fn read_character_spec(d: &Dialog, view: &mut DocView) -> Option<CharSpec> {
    let mut s = CharSpec::default();
    let font = d.selected_text("font");
    if !font.is_empty() {
        s.font = Some(view.doc.font_id(&font));
    }
    // Left empty, the size is not part of the search; typed, it has to be
    // a size the Character dialog would take.
    let size = d.field_text("size");
    if !size.trim().is_empty() {
        s.size = Some(crate::format_dialogs::parse_font_size(&size)?);
    }
    s.underline = d
        .selected("underline")
        .checked_sub(1)
        .and_then(|i| crate::format_dialogs::UNDERLINE_KINDS.get(i).copied());
    s.color = spec_color(d);
    s.bold = d.check3_state("bold");
    s.italic = d.check3_state("italic");
    s.outline = d.check3_state("outline");
    s.shadow = d.check3_state("shadow");
    s.strike = d.check3_state("strike");
    s.small_caps = d.check3_state("smallcaps");
    s.all_caps = d.check3_state("allcaps");
    s.hidden = d.check3_state("hidden");
    let by = |id| units::parse(&d.field_text(id), "pt").map(|t| t.abs());
    s.script = match d.radio_on(1) {
        "pos_normal" => Some(quill_core::Script::None),
        "pos_super" => Some(quill_core::Script::Super),
        "pos_sub" => Some(quill_core::Script::Sub),
        _ => None,
    };
    // A shift by hand matters only when one is typed.
    let raised = d.field_text("pos_by");
    s.position = if raised.trim().is_empty() {
        None
    } else {
        Some(units::parse(&raised, "pt")?)
    };
    s.spacing = match d.radio_on(2) {
        "sp_normal" => Some(0),
        "sp_cond" => Some(-by("sp_by").unwrap_or(30)),
        "sp_exp" => Some(by("sp_by").unwrap_or(60)),
        _ => None,
    };
    Some(s)
}

// ----- Go To -----

pub fn go_to(view: &DocView) -> Dialog {
    let mut d = Dialog::new("", 262.0, 100.0);
    d.native_title = Some("Go To".to_string());
    d.plain = true;
    d.text("Go To", 13.0, 6.0);
    d.text("Page Number:", 13.0, 31.0);
    let page = (view.caret_page() + 1).to_string();
    d.field("page", &page, 107.0, 28.0, 76.0);
    d.action_buttons(&[("cancel", "Cancel", 58.0, false), ("ok", "OK", 58.0, true)]);
    d.focus("page");
    d
}

/// Read Go To's field: "5", "p5", "+2" or "-1". Returns the page index.
pub fn go_to_page(d: &Dialog, view: &DocView) -> Option<usize> {
    let t = d.field_text("page").trim().to_ascii_lowercase();
    let t = t.strip_prefix('p').unwrap_or(&t);
    let cur = view.caret_page() as i64 + 1;
    let n: i64 = if let Some(r) = t.strip_prefix('+') {
        cur.saturating_add(r.trim().parse::<i64>().ok()?)
    } else if let Some(r) = t.strip_prefix('-') {
        cur.saturating_sub(r.trim().parse::<i64>().ok()?)
    } else {
        t.parse().ok()?
    };
    let pages = view.pages.len().max(1) as i64;
    Some((n.clamp(1, pages) - 1) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::Document;

    fn view(text: &str) -> DocView {
        let mut v = DocView::new(Document::new(), "t".into());
        v.insert_text(text);
        v.set_selection(Selection::caret(Position::new(0, 0)));
        v
    }

    #[test]
    fn go_to_large_relative_page_numbers_do_not_overflow() {
        let view = view("page");
        let mut dialog = go_to(&view);
        dialog.set_field("page", "+9223372036854775807");
        assert_eq!(go_to_page(&dialog, &view), Some(0));
        dialog.set_field("page", "-9223372036854775807");
        assert_eq!(go_to_page(&dialog, &view), Some(0));
    }

    fn search(find: &str, dir: usize) -> Search {
        Search {
            find: find.into(),
            dir,
            ..Default::default()
        }
    }

    #[test]
    fn switching_modes_keeps_search_replacement_and_match() {
        let v = view("cat dog");
        let (mut d, mut st) = dialog(&v, true);
        d.set_field("find", "cat");
        d.set_field("with", "bird");
        st.found = Some(v.sel.range());
        let found = st.found;
        d.origin = (230.0, 170.0);
        d.set_selected("dir", ALL);
        switch_mode(&mut st, &mut d, &v, false);
        assert!(!st.replace);
        assert_eq!(d.origin, (230.0, 170.0));
        assert_eq!(st.search.dir, ALL);
        assert_eq!(d.field_text("find"), "cat");
        switch_mode(&mut st, &mut d, &v, true);
        assert!(st.replace);
        assert_eq!(d.field_text("with"), "bird");
        assert_eq!(st.found, found);
        for replace in [false, true] {
            let (d, _) = dialog(&v, replace);
            for item in &d.items {
                assert!(item.rect.bottom() <= d.height - 8.0, "{}", item.id);
            }
        }
    }

    #[test]
    fn finds_down_then_asks_to_wrap() {
        let mut v = view("one two one two");
        let s = search("two", DOWN);
        assert_eq!(find_next(&mut v, &s), Hit::Match);
        assert_eq!(v.sel.range(), (Position::new(0, 4), Position::new(0, 7)));
        assert_eq!(find_next(&mut v, &s), Hit::Match);
        assert_eq!(v.sel.start(), Position::new(0, 12));
        assert_eq!(find_next(&mut v, &s), Hit::ReachedEnd);
        assert!(find_wrapped(&mut v, &s));
        assert_eq!(v.sel.start(), Position::new(0, 4));
        // From the middle, Word asks before going round, even for text
        // that is nowhere.
        let none = search("zzz", DOWN);
        assert_eq!(find_next(&mut v, &none), Hit::ReachedEnd);
        assert!(!find_wrapped(&mut v, &none));
        v.set_selection(Selection::caret(Position::new(0, 0)));
        assert_eq!(find_next(&mut v, &none), Hit::NotFound);
    }

    #[test]
    fn all_wraps_without_asking() {
        let mut v = view("one two one");
        v.set_selection(Selection::caret(Position::new(0, 9)));
        assert_eq!(find_next(&mut v, &search("one", ALL)), Hit::Match);
        assert_eq!(v.sel.start(), Position::new(0, 0));
    }

    #[test]
    fn replace_then_find_next_and_undo_all() {
        let mut v = view("cat cat cat");
        let mut clip = Clipboard::new();
        let mut st = FindState {
            replace: true,
            search: Search {
                replace: "dog".into(),
                ..search("cat", DOWN)
            },
            found: None,
        };
        assert_eq!(replace(&mut st, &mut v, &mut clip), Hit::Match);
        assert_eq!(replace(&mut st, &mut v, &mut clip), Hit::Match);
        assert_eq!(v.doc.main.paras[0].text, "dog cat cat");
        assert_eq!(v.sel.start(), Position::new(0, 4));
        v.set_selection(Selection::caret(Position::new(0, 0)));
        let (n, _) = replace_all(&mut st, &mut v, &mut clip);
        assert_eq!(n, 2);
        assert_eq!(v.doc.main.paras[0].text, "dog dog dog");
        v.exec(crate::commands::Cmd::Undo, false, &mut clip);
        assert_eq!(v.doc.main.paras[0].text, "dog cat cat");
    }
    #[test]
    fn replacing_tracked_deletions_moves_on() {
        // "quietly" is already deleted and shown struck through. With
        // tracking on, Replace All and Replace each replace it once and
        // move on, instead of finding the old text again forever.
        for all in [true, false] {
            let mut v = view("The cat sat quietly.");
            v.doc.props.track_changes = true;
            v.doc.author = "Ada".into();
            v.doc.delete(
                quill_core::StoryId::Main,
                Position::new(0, 12),
                Position::new(0, 19),
            );
            let mut st = FindState {
                replace: true,
                search: Search {
                    find: "quietly".into(),
                    replace: "toward".into(),
                    dir: if all { ALL } else { DOWN },
                    ..Default::default()
                },
                found: None,
            };
            let mut clip = Clipboard::new();
            if all {
                assert_eq!(replace_all(&mut st, &mut v, &mut clip).0, 1);
            } else {
                assert_eq!(find_next(&mut v, &st.search), Hit::Match);
                st.found = Some(v.sel.range());
                assert_eq!(replace(&mut st, &mut v, &mut clip), Hit::ReachedEnd);
            }
            let accepted = quill_core::tracking::accepted(&v.doc.main.paras);
            assert_eq!(accepted[0].text, "The cat sat toward.");
        }
    }
}
