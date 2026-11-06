//! Tools > Spelling, Hyphenation and Thesaurus: three windows, laid out
//! from the original. Each works on the document while
//! it stays open.

use crate::dialog::Dialog;
use crate::docview::{DocView, HyphenSpot};
use quill_core::Position;
use quill_core::thesaurus::Meaning;

/// What the window asks of the app after a button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    /// Close the window, with a message to show if any.
    Close(Option<&'static str>),
    /// Open Preferences at its Spelling panel.
    Options,
}

const DONE: &str = "The entire document has been checked.";
const NO_DICTIONARY: &str = "No spelling service is installed for the current system language.";

// ----- Spelling -----

#[derive(Clone, Debug)]
pub struct SpellState {
    /// The unknown word, where it is and as written.
    at: (Position, Position),
    word: String,
    suggestions: Vec<String>,
    /// Where checking began, and whether it has gone round from the top.
    start: Position,
    wrapped: bool,
}

/// Tools > Spelling (⌘L): the window on the first unknown word from the
/// insertion point, or a message when there is none.
pub fn spelling(view: &mut DocView) -> Result<(Dialog, SpellState), &'static str> {
    if let Err(error) = crate::language::prepare_dictionary() {
        crate::app::alert("Quill could not load its custom dictionary.", &error);
    }
    if !crate::spellcheck::available() {
        return Err(NO_DICTIONARY);
    }
    let mut d = Dialog::new("Spelling", 458.0, 150.0);
    d.modeless = true;
    d.close_box = true;
    d.text("Not in Dictionary:", 14.0, 8.0);
    d.text_as("word", "", 133.0, 8.0);
    d.text("Change To:", 14.0, 35.0);
    d.field("to", "", 109.0, 32.0, 158.0);
    d.text("Suggestions:", 14.0, 56.0);
    d.popup("list", &["No suggestions yet"], 0, 109.0, 52.0, 158.0);
    d.button("ignore", "Ignore", 280.0, 32.0, 75.0, true);
    d.button("ignoreall", "Ignore All", 366.0, 32.0, 78.0, false);
    d.button("change", "Change", 280.0, 58.0, 75.0, false);
    d.button("changeall", "Change All", 366.0, 58.0, 78.0, false);
    d.button("add", "Add", 280.0, 84.0, 75.0, false);
    d.button("cancel", "Close", 366.0, 84.0, 78.0, false);
    d.button("suggest", "Suggest", 280.0, 110.0, 75.0, false);
    d.button("options", "Options…", 366.0, 110.0, 78.0, false);
    d.text("Add Words To:", 14.0, 125.0);
    d.popup(
        "dict",
        &["Custom Dictionary", "System Dictionary"],
        usize::from(crate::settings::with(|s| s.words_to_system)),
        109.0,
        122.0,
        159.0,
    );
    let start = view.sel.range().0;
    let mut st = SpellState {
        at: (start, start),
        word: String::new(),
        suggestions: Vec::new(),
        start,
        wrapped: false,
    };
    if !next_word(&mut st, &mut d, view, start) {
        return Err(DONE);
    }
    Ok((d, st))
}

/// Go on to the next unknown word from `from`, going round from the top
/// once. False when the whole document has been checked.
fn next_word(st: &mut SpellState, d: &mut Dialog, view: &mut DocView, from: Position) -> bool {
    let top = Position::new(0, 0);
    let mut found = view.next_misspelling(from);
    if st.wrapped {
        // Round from the top: stop where checking began.
        found = found.filter(|(s, _)| *s < st.start);
    } else if found.is_none() && st.start != top {
        st.wrapped = true;
        found = view.next_misspelling(top).filter(|(s, _)| *s < st.start);
    }
    let Some((s, e)) = found else {
        return false;
    };
    st.at = (s, e);
    st.word = view.doc.main.paras[s.para].text[s.offset..e.offset].to_string();
    view.set_selection(quill_core::Selection::new(s, e));
    view.scroll_to_caret();
    d.set_label("word", &st.word);
    st.suggestions.clear();
    d.set_options("list", vec!["No suggestions yet".to_string()], 0);
    d.enable("suggest", true);
    if crate::settings::with(|s| s.always_suggest) {
        suggest(st, d);
    } else {
        d.set_field("to", &st.word);
    }
    true
}

fn suggest(st: &mut SpellState, d: &mut Dialog) {
    st.suggestions = crate::spellcheck::suggest(&st.word, 12);
    let choices = if st.suggestions.is_empty() {
        vec!["No suggestions".to_string()]
    } else {
        st.suggestions.clone()
    };
    d.set_options("list", choices, 0);
    d.set_field("to", st.suggestions.first().map_or(&st.word, |s| s));
    d.enable("suggest", false);
}

/// A button in the Spelling window.
pub fn spelling_button(
    st: &mut SpellState,
    d: &mut Dialog,
    view: &mut DocView,
    id: &str,
) -> Action {
    let (s, e) = st.at;
    let to = d.field_text("to");
    let after = match id {
        "ignore" => e,
        "ignoreall" => {
            crate::language::ignore_all(&st.word);
            e
        }
        "add" => {
            let system = d.selected("dict") == 1;
            let result = if system {
                crate::spellcheck::learn(&st.word)
            } else {
                crate::language::add_word(&st.word)
            };
            if let Err(error) = result {
                crate::app::alert(
                    if system {
                        "Quill could not add that word to the system dictionary."
                    } else {
                        "Quill could not add that word to its dictionary."
                    },
                    &error,
                );
                return Action::None;
            }
            e
        }
        "change" => {
            if to != st.word && !to.is_empty() {
                view.replace_word(s, e, &to);
            }
            view.sel.focus
        }
        "changeall" => {
            if to != st.word && !to.is_empty() {
                view.change_all(&st.word, &to);
            }
            Position::new(
                s.para,
                (s.offset + to.len()).min(view.doc.main.paras[s.para].len()),
            )
        }
        "suggest" => {
            suggest(st, d);
            return Action::None;
        }
        "options" => return Action::Options,
        _ => return Action::None,
    };
    if next_word(st, d, view, after) {
        Action::None
    } else {
        Action::Close(Some(DONE))
    }
}

pub fn spelling_changed(st: &SpellState, d: &mut Dialog, id: &str) {
    if id == "dict" {
        crate::settings::update(|s| s.words_to_system = d.selected("dict") == 1);
    }

    if id == "list"
        && let Some(suggestion) = st.suggestions.get(d.selected("list"))
    {
        d.set_field("to", suggestion);
    }
}

// ----- Hyphenation -----

#[derive(Clone, Debug)]
pub struct HyphState {
    spot: Option<HyphenSpot>,
    /// Search again once the layout has caught up, from here.
    pending: Option<Position>,
    end: Option<Position>,
}

/// Tools > Hyphenation: the window on the first line that could take part
/// of the next one's first word.
pub fn hyphenation(view: &mut DocView) -> Result<(Dialog, HyphState), &'static str> {
    if crate::language::hyphenator().is_none() {
        return Err(
            "The hyphenation patterns could not be found. Quill looks for them in the data folder beside the program.",
        );
    }
    let mut d = Dialog::new("Hyphenation", 460.0, 84.0);
    d.modeless = true;
    d.close_box = true;
    d.text("Hyphenate:", 14.0, 9.0);
    d.field("word", "", 104.0, 5.0, 344.0);
    d.check("caps", "Hyphenate Capitalized Words", 16.0, 30.0, false);
    d.button("nochange", "No Change", 13.0, 53.0, 131.0, true);
    d.button("change", "Change", 150.0, 53.0, 81.0, false);
    let all = if view.sel.is_empty() {
        "Hyphenate All"
    } else {
        "Hyphenate Selection"
    };
    d.button("all", all, 241.0, 53.0, 142.0, false);
    d.button("cancel", "Cancel", 392.0, 53.0, 53.0, false);
    let (s, e) = view.sel.range();
    let mut st = HyphState {
        spot: None,
        pending: None,
        end: (!view.sel.is_empty()).then_some(e),
    };
    let from = if view.sel.is_empty() {
        Position::new(0, 0)
    } else {
        s
    };
    show_spot(&mut st, &mut d, view, from);
    Ok((d, st))
}

/// Show the next place from `from`, with its proposed hyphen.
fn show_spot(st: &mut HyphState, d: &mut Dialog, view: &mut DocView, from: Position) {
    st.spot = view.next_hyphenation(from, st.end, d.is_on("caps"));
    match &st.spot {
        Some(spot) => {
            let shown: String = spot
                .word
                .chars()
                .enumerate()
                .flat_map(|(k, c)| (k == spot.proposed).then_some('-').into_iter().chain([c]))
                .collect();
            d.set_field("word", &shown);
            view.set_selection(quill_core::Selection::new(spot.start, spot.end));
            view.scroll_to_caret();
        }
        None => d.set_field("word", ""),
    }
    d.enable("change", st.spot.is_some());
    d.enable("nochange", st.spot.is_some());
}

/// Called each frame: search again once a change has been laid out.
pub fn hyphenation_refresh(st: &mut HyphState, d: &mut Dialog, view: &mut DocView) {
    if let Some(from) = st.pending.take() {
        show_spot(st, d, view, from);
    }
}

pub fn hyphenation_button(
    st: &mut HyphState,
    d: &mut Dialog,
    view: &mut DocView,
    id: &str,
) -> Action {
    match id {
        "nochange" => {
            if let Some(spot) = st.spot.clone() {
                show_spot(st, d, view, Position::new(spot.end.para, spot.end.offset));
            }
        }
        "change" => {
            if let Some(spot) = st.spot.clone() {
                // The hyphen goes where the field shows it.
                let field = d.field_text("word");
                let at = field
                    .chars()
                    .position(|c| c == '-')
                    .filter(|&k| k > 0 && k < spot.word.chars().count())
                    .unwrap_or(spot.proposed);
                view.insert_hyphen(&spot, at);
                st.pending = Some(Position::new(spot.end.para, spot.end.offset + 2));
                st.spot = None;
            }
        }
        "all" => {
            let from = st.spot.as_ref().map_or(Position::new(0, 0), |s| s.start);
            view.hyphenate_all(from, st.end, d.is_on("caps"));
            return Action::Close(None);
        }
        _ => {}
    }
    Action::None
}

pub fn hyphenation_changed(st: &mut HyphState, d: &mut Dialog, view: &mut DocView, id: &str) {
    if id == "caps" {
        let from = st.spot.as_ref().map_or(Position::new(0, 0), |s| s.start);
        show_spot(st, d, view, from);
    }
}

// ----- Thesaurus -----

#[derive(Clone, Debug)]
pub struct ThesState {
    /// The word to replace and where it is.
    original: String,
    range: Option<(Position, Position)>,
    meanings: Vec<Meaning>,
    /// Words near an unknown one, shown instead of meanings.
    nearby: Vec<String>,
    left: usize,
    right: usize,
    history: Vec<String>,
}

/// Tools > Thesaurus: meanings and synonyms of the selected word.
pub fn thesaurus(view: &mut DocView) -> Result<(Dialog, ThesState), &'static str> {
    if crate::language::thesaurus().is_none() {
        return Err(
            "The thesaurus could not be found. Quill looks for it in the data folder beside the program.",
        );
    }
    let range = view.word_at_caret();
    let original = range.map_or(String::new(), |(s, e)| {
        view.doc.main.paras[s.para].text[s.offset..e.offset].to_string()
    });
    if let Some((s, e)) = range {
        view.set_selection(quill_core::Selection::new(s, e));
    }
    let mut d = Dialog::new("Thesaurus", 454.0, 230.0);
    d.modeless = true;
    d.close_box = true;
    d.text("Replace:", 10.0, 10.0);
    d.text_as("original", &original, 87.0, 10.0);
    d.text("With:", 10.0, 37.0);
    d.field("with", &original, 82.0, 32.0, 194.0);
    d.menu_button("history", "", &[""], 282.0, 32.0, 19.0);
    d.button("replace", "Replace", 318.0, 5.0, 61.0, false);
    d.button("original_word", "Original", 386.0, 5.0, 59.0, false);
    d.button("lookup", "Look Up", 318.0, 32.0, 61.0, true);
    d.button("cancel", "Cancel", 386.0, 32.0, 59.0, false);
    d.text_as("left_title", "", 10.0, 61.0);
    d.popup("meanings", &["No meanings"], 0, 6.0, 78.0, 221.0);
    d.text("Synonyms:", 236.0, 61.0);
    d.popup("synonyms", &["No synonyms"], 0, 233.0, 78.0, 214.0);
    d.enable("replace", range.is_some());
    let mut st = ThesState {
        original: original.clone(),
        range,
        meanings: Vec::new(),
        nearby: Vec::new(),
        left: 0,
        right: usize::MAX,
        history: Vec::new(),
    };
    look_up(&mut st, &mut d, &original);
    Ok((d, st))
}

/// The synonyms shown for meaning `k`: its words less the one that names
/// it.
fn synonyms(st: &ThesState) -> Vec<&str> {
    let Some(m) = st.meanings.get(st.left) else {
        return Vec::new();
    };
    let skip = usize::from(m.pos != "antonym" && m.words.len() > 1);
    m.words[skip..].iter().map(String::as_str).collect()
}

fn look_up(st: &mut ThesState, d: &mut Dialog, word: &str) {
    let word = word.trim();
    let t = crate::language::thesaurus();
    match t.and_then(|t| t.look_up(word)) {
        Some((head, meanings)) => {
            st.meanings = meanings;
            st.nearby.clear();
            d.set_label("left_title", &format!("Meanings For: {head}"));
        }
        None => {
            st.meanings.clear();
            st.nearby = t.map(|t| t.nearby(word, 40)).unwrap_or_default();
            d.set_label("left_title", "Alphabetical List:");
        }
    }
    st.left = 0;
    st.right = usize::MAX;
    let left = if st.meanings.is_empty() {
        if st.nearby.is_empty() {
            vec!["No matches".to_string()]
        } else {
            st.nearby.clone()
        }
    } else {
        st.meanings.iter().map(Meaning::label).collect()
    };
    d.set_options("meanings", left, 0);
    let words: Vec<String> = synonyms(st).into_iter().map(str::to_string).collect();
    d.set_options(
        "synonyms",
        if words.is_empty() {
            vec!["No synonyms".to_string()]
        } else {
            words
        },
        0,
    );
    if !word.is_empty() && st.history.last().map(String::as_str) != Some(word) {
        st.history.push(word.to_string());
        let names: Vec<String> = st.history.iter().rev().cloned().collect();
        d.set_options("history", names, 0);
    }
}

/// Give `to` the capitals of `like`.
fn match_case(to: &str, like: &str) -> String {
    let caps = like.chars().filter(|c| c.is_alphabetic()).count();
    let upper = like.chars().filter(|c| c.is_uppercase()).count();
    if caps > 1 && upper == caps {
        to.to_uppercase()
    } else if like.chars().next().is_some_and(|c| c.is_uppercase()) {
        let mut c = to.chars();
        c.next()
            .map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
    } else {
        to.to_string()
    }
}

pub fn thesaurus_button(
    st: &mut ThesState,
    d: &mut Dialog,
    view: &mut DocView,
    id: &str,
) -> Action {
    match id {
        "lookup" => {
            let w = d.field_text("with");
            look_up(st, d, &w);
        }
        "original_word" => {
            let w = st.original.clone();
            d.set_field("with", &w);
            look_up(st, d, &w);
        }
        "replace" => {
            if let Some((s, e)) = st.range {
                let with = match_case(d.field_text("with").trim(), &st.original);
                if !with.is_empty() {
                    view.replace_word(s, e, &with);
                }
            }
            return Action::Close(None);
        }
        _ => {}
    }
    Action::None
}

/// A word chosen from the history menu: look it up.
pub fn thesaurus_changed(st: &mut ThesState, d: &mut Dialog, id: &str) {
    match id {
        "history" => {
            let k = d.selected("history");
            let names: Vec<String> = st.history.iter().rev().cloned().collect();
            if let Some(w) = names.get(k) {
                d.set_field("with", w);
                look_up(st, d, &w.clone());
            }
        }
        "meanings" => {
            let k = d.selected("meanings");
            if let Some(w) = st.nearby.get(k).cloned() {
                d.set_field("with", &w);
                return;
            }
            st.left = k.min(st.meanings.len().saturating_sub(1));
            st.right = usize::MAX;
            let words: Vec<String> = synonyms(st).into_iter().map(str::to_string).collect();
            d.set_options(
                "synonyms",
                if words.is_empty() {
                    vec!["No synonyms".to_string()]
                } else {
                    words
                },
                0,
            );
            if let Some(m) = st.meanings.get(k)
                && m.pos != "antonym"
            {
                let w = m.words.first().cloned().unwrap_or_default();
                d.set_field("with", &w);
            }
        }
        "synonyms" => {
            let k = d.selected("synonyms");
            st.right = k;
            if let Some(w) = synonyms(st).get(k).map(|s| s.to_string()) {
                d.set_field("with", &w);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::Document;

    #[test]
    fn spelling_uses_only_standard_controls() {
        let mut view = DocView::new(Document::new(), "Test".to_string());
        view.type_text("teh");
        let Ok((dialog, _)) = spelling(&mut view) else {
            return;
        };
        assert!(dialog.modeless);
        assert_eq!(dialog.selected("dict"), 0);
        let dictionary = dialog.items.iter().find(|item| item.id == "dict").unwrap();
        assert!(
            matches!(&dictionary.kind, crate::dialog::Kind::Popup { options, .. } if options == &["Custom Dictionary", "System Dictionary"])
        );
        assert!(
            !dialog
                .items
                .iter()
                .any(|item| matches!(item.kind, crate::dialog::Kind::Custom))
        );
    }

    #[test]
    fn thesaurus_uses_only_standard_controls() {
        let mut view = DocView::new(Document::new(), "Test".to_string());
        view.type_text("happy");
        let Ok((dialog, _)) = thesaurus(&mut view) else {
            return;
        };
        assert!(dialog.modeless);
        assert!(
            !dialog
                .items
                .iter()
                .any(|item| matches!(item.kind, crate::dialog::Kind::Custom))
        );
    }
}
