//! The Grep, Regex Search and Sed windows of Edit > Advanced. The matching itself is in
//! `advanced.rs`; this file builds the windows and reads them back.

use crate::advanced::{self, GrepHit, GrepOptions, Sed};
use crate::dialog::Dialog;
use crate::docview::DocView;
use quill_core::Position;
use std::cell::RefCell;

/// Rows Grep lists. More matches than this are counted but not listed.
const MAX_ROWS: usize = 2000;

/// How much of a paragraph a list row shows.
const ROW_CHARS: usize = 90;

/// Status lines under the entry field.
const STATUS_LINES: [&str; 3] = ["status1", "status2", "status3"];

/// What the two windows showed last, so they open as they were left.
#[derive(Clone, Default)]
struct Last {
    pattern: String,
    ignore_case: bool,
    whole_word: bool,
    invert: bool,
    script: String,
    rx_pattern: String,
    rx_with: String,
    rx_ignore: bool,
    rx_word: bool,
}

thread_local! {
    static LAST: RefCell<Last> = RefCell::new(Last::default());
}

fn last() -> Last {
    LAST.with(|l| l.borrow().clone())
}

fn remember(f: impl FnOnce(&mut Last)) {
    LAST.with(|l| f(&mut l.borrow_mut()));
}

/// Put `text` in the status lines under the field, wrapped.
fn set_status(d: &mut Dialog, text: &str) {
    let lines = crate::dialog::wrap(text, 62);
    for (i, id) in STATUS_LINES.iter().enumerate() {
        d.set_label(id, lines.get(i).map_or("", String::as_str));
    }
}

fn add_status(d: &mut Dialog, x: f32, y: f32) {
    for (i, id) in STATUS_LINES.iter().enumerate() {
        d.note(id, "", x, y + 13.0 * i as f32, 350.0);
    }
}

// ----- Grep -----

/// The paragraphs Grep last listed.
#[derive(Clone, Debug)]
pub struct GrepState {
    pub hits: Vec<GrepHit>,
}

pub fn grep(_view: &DocView) -> (Dialog, GrepState) {
    let last = last();
    let mut d = Dialog::new("Grep", 465.0, 300.0);
    d.modeless = true;
    d.close_box = true;
    d.text("Pattern:", 8.0, 7.0);
    d.field("pattern", &last.pattern, 62.0, 5.0, 298.0);
    d.check("ignore", "Ignore Case", 8.0, 32.0, last.ignore_case);
    d.check("word", "Whole Word", 118.0, 32.0, last.whole_word);
    d.check("invert", "Invert Match", 228.0, 32.0, last.invert);
    add_status(&mut d, 8.0, 54.0);
    d.list("hits", Vec::new(), 0, 8.0, 96.0, 350.0, 194.0, Some("go"));
    d.button("search", "Find All", 369.0, 9.0, 88.0, true);
    d.button("go", "Go To", 369.0, 36.0, 88.0, false);
    d.button("cancel", "Close", 369.0, 63.0, 88.0, false);
    d.enable("go", false);
    d.focus("pattern");
    set_status(
        &mut d,
        "Lists the paragraphs that match. Groups are written (...), as in grep -E.",
    );
    (d, GrepState { hits: Vec::new() })
}

fn options(d: &Dialog) -> GrepOptions {
    GrepOptions {
        ignore_case: d.is_on("ignore"),
        whole_word: d.is_on("word"),
        invert: d.is_on("invert"),
    }
}

/// A paragraph for a list row: one line, cut to a sensible length.
fn row(hit: &GrepHit) -> String {
    let flat: String = hit
        .text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(ROW_CHARS + 1)
        .collect();
    let text = if flat.chars().count() > ROW_CHARS {
        let cut: String = flat.chars().take(ROW_CHARS).collect();
        format!("{cut}\u{2026}")
    } else {
        flat
    };
    format!("{:>5}  {}", hit.para + 1, text)
}

/// Search the document and list the paragraphs.
pub fn grep_search(st: &mut GrepState, d: &mut Dialog, view: &DocView) {
    let pattern = d.field_text("pattern");
    let opts = options(d);
    remember(|l| {
        l.pattern = pattern.clone();
        l.ignore_case = opts.ignore_case;
        l.whole_word = opts.whole_word;
        l.invert = opts.invert;
    });
    match advanced::grep(&pattern, &opts, &view.search_lines()) {
        Ok(hits) => {
            let total = hits.len();
            let shown: Vec<String> = hits.iter().take(MAX_ROWS).map(row).collect();
            d.set_rows("hits", shown, 0);
            let noun = if total == 1 {
                "paragraph"
            } else {
                "paragraphs"
            };
            let verb = if opts.invert { "do not match" } else { "match" };
            let message = match total {
                0 if opts.invert => "Every paragraph matches.".to_string(),
                0 => "No paragraph matches.".to_string(),
                n if n > MAX_ROWS => format!("{n} {noun} {verb}; the first {MAX_ROWS} are listed."),
                n => format!("{n} {noun} {verb}."),
            };
            set_status(d, &message);
            d.enable("go", total > 0);
            st.hits = hits;
        }
        Err(message) => {
            d.set_rows("hits", Vec::new(), 0);
            d.enable("go", false);
            set_status(d, &message);
            st.hits.clear();
        }
    }
}

/// `at` or, when it is past the end or inside a character, the nearest
/// character start before it.
fn char_floor(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// Select the match in the chosen row and show it. If the document was
/// edited since the search, so that the row's paragraph no longer holds
/// the text it listed, the search runs again instead and nothing moves:
/// the row's place could now be unrelated text.
pub fn grep_go(st: &mut GrepState, d: &mut Dialog, view: &mut DocView) {
    let Some(hit) = d.list_selected("hits").and_then(|i| st.hits.get(i)) else {
        return;
    };
    let current = view.doc.main.paras.get(hit.para).map(|p| p.text.as_str());
    if current != Some(hit.text.as_str()) {
        grep_search(st, d, view);
        set_status(
            d,
            "The document changed since the search, so Grep searched again.",
        );
        return;
    }
    // The match is brought inside the text and onto whole characters, so a
    // position that is somehow off can never split one.
    let para = hit.para;
    let text = &view.doc.main.paras[para].text;
    let start = char_floor(text, hit.start);
    let end = char_floor(text, hit.end).max(start);
    view.select_found(Position::new(para, start), Position::new(para, end));
}

// ----- Sed -----

pub const SED_DOCUMENT: usize = 0;
pub const SED_SELECTION: usize = 1;

pub fn sed(view: &DocView) -> Dialog {
    let last = last();
    let has_sel = !view.sel.is_empty();
    let mut d = Dialog::new("Sed", 465.0, 190.0);
    d.modeless = true;
    d.close_box = true;
    d.text("Script:", 8.0, 7.0);
    d.field("script", &last.script, 62.0, 5.0, 298.0);
    d.text("Apply to:", 8.0, 34.0);
    let mut scopes = vec!["Document"];
    if has_sel {
        scopes.push("Selection");
    }
    let multi = has_sel && view.sel.start().para != view.sel.end().para;
    d.popup(
        "scope",
        &scopes,
        if multi { SED_SELECTION } else { SED_DOCUMENT },
        62.0,
        32.0,
        120.0,
    );
    add_status(&mut d, 8.0, 62.0);
    d.button("apply", "Apply", 369.0, 9.0, 88.0, true);
    d.button("cancel", "Close", 369.0, 36.0, 88.0, false);
    d.focus("script");
    sed_check(&mut d);
    d
}

/// Read the script as it stands and say whether it will run.
pub fn sed_check(d: &mut Dialog) {
    let script = d.field_text("script");
    remember(|l| l.script = script.clone());
    if script.trim().is_empty() {
        set_status(
            d,
            "Example: s/colour/color/g. Separate commands with ; or put them on new lines.",
        );
        d.enable("apply", false);
        return;
    }
    match Sed::parse(&script) {
        Ok(sed) => {
            let n = sed.command_count();
            set_status(
                d,
                &format!(
                    "{n} command{}. Each paragraph is a line; groups are (...) and \\1.",
                    if n == 1 { "" } else { "s" }
                ),
            );
            d.enable("apply", true);
        }
        Err(message) => {
            set_status(d, &message);
            d.enable("apply", false);
        }
    }
}

/// Run the script on the document or the selection. Returns the number
/// of changes, or why it could not run.
pub fn sed_apply(d: &mut Dialog, view: &mut DocView) -> Result<usize, String> {
    let sed = Sed::parse(&d.field_text("script"))?;
    let in_selection = d.selected("scope") == SED_SELECTION && !view.sel.is_empty();
    let n = match view.sed(&sed, in_selection) {
        Ok(n) => n,
        Err(message) => {
            set_status(d, &message);
            return Err(message);
        }
    };
    set_status(
        d,
        &match n {
            0 => "No changes.".to_string(),
            1 => "1 change.".to_string(),
            n => format!("{n} changes."),
        },
    );
    Ok(n)
}

// ----- Regex Search -----

/// Build the Regex Search window. It is laid out like Find (⌘F): Find What
/// on top, and a button that opens the Replace part below it (`replace`).
pub fn regex_search(view: &DocView, replace: bool) -> Dialog {
    let last = last();
    let has_sel = !view.sel.is_empty();
    let h = if replace { 238.0 } else { 142.0 };
    let mut d = Dialog::new("Regex Search", 465.0, h);
    d.modeless = true;
    d.close_box = true;
    d.text("Find What:", 8.0, 7.0);
    d.field("pattern", &last.rx_pattern, 102.0, 5.0, 250.0);
    add_status(&mut d, 104.0, 31.0);
    d.check("word", "Match Whole Word Only", 8.0, 84.0, last.rx_word);
    d.check("ignore", "Ignore Case", 195.0, 84.0, last.rx_ignore);
    d.button("findnext", "Find Next", 369.0, 9.0, 88.0, true);
    d.button("cancel", "Cancel", 369.0, 36.0, 88.0, false);
    if replace {
        d.text("Replace With:", 8.0, 150.0);
        d.field("with", &last.rx_with, 102.0, 148.0, 250.0);
        d.button("replace", "Replace", 369.0, 152.0, 88.0, false);
        d.button("replaceall", "Replace All", 369.0, 178.0, 88.0, false);
        d.text("Replace All in:", 376.0, 66.0);
        let mut scopes = vec!["Document"];
        if has_sel {
            scopes.push("Selection");
        }
        let multi = has_sel && view.sel.start().para != view.sel.end().para;
        d.popup(
            "scope",
            &scopes,
            if multi { SED_SELECTION } else { SED_DOCUMENT },
            367.0,
            81.0,
            89.0,
        );
    }
    d.button(
        "mode",
        if replace {
            "Hide Replace"
        } else {
            "Replace\u{2026}"
        },
        8.0,
        110.0,
        110.0,
        false,
    );
    d.focus("pattern");
    rx_check(&mut d, replace);
    d
}

/// Show or hide the Replace part, keeping what was typed and the window's
/// place on the screen.
pub fn rx_switch(d: &mut Dialog, view: &DocView, replace: bool) {
    // Remember the fields as they stand, then rebuild.
    let _ = rx_read(d, !replace);
    let mut next = regex_search(view, replace);
    next.origin = d.origin;
    *d = next;
}

/// Compile what the fields hold; remember it for the next time.
fn rx_read(d: &Dialog, replace: bool) -> Result<advanced::Search, String> {
    let pattern = d.field_text("pattern");
    let kept = last().rx_with;
    let with = if replace { d.field_text("with") } else { kept };
    let (ignore, word) = (d.is_on("ignore"), d.is_on("word"));
    remember(|l| {
        l.rx_pattern = pattern.clone();
        l.rx_with = with.clone();
        l.rx_ignore = ignore;
        l.rx_word = word;
    });
    // With the Replace part hidden only the pattern is in use, so a
    // replacement left there that cannot be read must not stop a search.
    let used = if replace { with.as_str() } else { "" };
    advanced::Search::new(&pattern, used, ignore, word)
}

/// Say whether the pattern and replacement will run, and enable the buttons.
pub fn rx_check(d: &mut Dialog, replace: bool) {
    let ok = match rx_read(d, replace) {
        Ok(_) => {
            set_status(
                d,
                if replace {
                    "Groups are (...); write \\1 or & in the replacement."
                } else {
                    "Regular expression. Groups are (...)."
                },
            );
            true
        }
        Err(message) => {
            let text = if d.field_text("pattern").is_empty() {
                "Type a pattern to look for. Groups are (...).".to_string()
            } else {
                message
            };
            set_status(d, &text);
            false
        }
    };
    d.enable("findnext", ok);
    if replace {
        d.enable("replace", ok);
        d.enable("replaceall", ok);
    }
}

/// Select the next match after the selection, wrapping to the top.
/// Returns whether there was one.
pub fn rx_find_next(d: &mut Dialog, view: &mut DocView, replace: bool) -> bool {
    let search = match rx_read(d, replace) {
        Ok(s) => s,
        Err(m) => {
            set_status(d, &m);
            return false;
        }
    };
    let end = view.sel.end();
    let found = search.find(&view.search_lines(), (end.para, end.offset));
    match found {
        Some(((para, start, stop), wrapped)) => {
            view.select_found(Position::new(para, start), Position::new(para, stop));
            set_status(
                d,
                if wrapped {
                    "Reached the end; continued from the top."
                } else {
                    ""
                },
            );
            true
        }
        None => {
            set_status(d, "No match.");
            false
        }
    }
}

/// Replace the selected match if it is one, then select the next.
pub fn rx_replace(d: &mut Dialog, view: &mut DocView) {
    let Ok(search) = rx_read(d, true) else {
        return;
    };
    let (a, b) = view.sel.range();
    if a.para == b.para && a.offset < b.offset {
        let edit = search.edit_at(&view.search_lines()[a.para], a.para, a.offset, b.offset);
        if let Some(edit) = edit
            && let Err(message) = view.replace_edit(&edit)
        {
            set_status(d, &message);
            return;
        }
    }
    rx_find_next(d, view, true);
}

/// Replace every match in the document or the selection.
pub fn rx_replace_all(d: &mut Dialog, view: &mut DocView) {
    let search = match rx_read(d, true) {
        Ok(s) => s,
        Err(m) => {
            set_status(d, &m);
            return;
        }
    };
    let in_selection = d.selected("scope") == SED_SELECTION && !view.sel.is_empty();
    let n = match view.run_script("Replace All", &search.replace_all_script(), in_selection) {
        Ok(n) => n,
        Err(message) => {
            set_status(d, &message);
            return;
        }
    };
    set_status(
        d,
        &match n {
            0 => "No changes.".to_string(),
            1 => "1 change.".to_string(),
            n => format!("{n} changes."),
        },
    );
}
