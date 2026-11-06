//! Insert > Table of Contents, Insert > Index and Section > Line Numbers.
//! Their controls are the original's; the layouts are
//! Quill's own, as these dialogs were not measured.

use crate::dialog::{Dialog, units};
use crate::docview::DocView;
use quill_core::LineNumbering;
use quill_core::contents::{IndexOptions, TocOptions};

/// Insert > Table of Contents.
pub fn toc() -> Dialog {
    let mut d = Dialog::new("Table of Contents", 360.0, 118.0);
    d.text("Collect:", 10.0, 10.0);
    d.radio("outline", "Outline", 70.0, 10.0, 1, true);
    d.radio("cparas", ".C. Paragraphs", 150.0, 10.0, 1, false);
    d.check("numbers", "Show Page Numbers", 10.0, 36.0, true);
    d.group("Level", 10.0, 60.0, 250.0, 48.0);
    d.radio("all", "All", 18.0, 80.0, 2, true);
    d.radio("range", "From:", 70.0, 80.0, 2, false);
    d.field("from", "", 128.0, 77.0, 36.0);
    d.text("To:", 174.0, 80.0);
    d.field("to", "", 204.0, 77.0, 36.0);
    d.button("ok", "Start", 276.0, 8.0, 74.0, true);
    d.button("cancel", "Cancel", 276.0, 34.0, 74.0, false);
    d
}

/// Typing a level picks From/To.
pub fn level_changed(d: &mut Dialog, id: &str) {
    if matches!(id, "from" | "to") && !d.field_text(id).is_empty() {
        d.set_on("range", true);
        d.set_on("all", false);
    }
}

pub fn read_toc(d: &Dialog) -> Option<TocOptions> {
    let mut o = TocOptions {
        outline: d.radio_on(1) == "outline",
        page_numbers: d.is_on("numbers"),
        ..TocOptions::default()
    };
    if d.radio_on(2) == "range" {
        let n = |id| {
            d.field_text(id)
                .trim()
                .parse::<u8>()
                .ok()
                .filter(|n| (1..=9).contains(n))
        };
        o.from = n("from")?;
        o.to = n("to").unwrap_or(9).max(o.from);
    }
    Some(o)
}

/// Insert > Index.
pub fn index() -> Dialog {
    let mut d = Dialog::new("Index", 360.0, 118.0);
    d.text("Format:", 10.0, 10.0);
    d.radio("nested", "Nested", 70.0, 10.0, 1, true);
    d.radio("runin", "Run-in", 150.0, 10.0, 1, false);
    d.group("Index Characters", 10.0, 40.0, 250.0, 48.0);
    d.radio("all", "All", 18.0, 60.0, 2, true);
    d.radio("range", "From:", 70.0, 60.0, 2, false);
    d.field("from", "", 128.0, 57.0, 36.0);
    d.text("To:", 174.0, 60.0);
    d.field("to", "", 204.0, 57.0, 36.0);
    d.button("ok", "Start", 276.0, 8.0, 74.0, true);
    d.button("cancel", "Cancel", 276.0, 34.0, 74.0, false);
    d
}

pub fn read_index(d: &Dialog) -> Option<IndexOptions> {
    let mut o = IndexOptions {
        nested: d.radio_on(1) == "nested",
        letters: None,
    };
    if d.radio_on(2) == "range" {
        let c = |id| {
            d.field_text(id)
                .trim()
                .chars()
                .next()
                .filter(char::is_ascii_alphabetic)
        };
        let a = c("from")?;
        o.letters = Some((a, c("to").unwrap_or('z')));
    }
    Some(o)
}

const NUMBERING: [(&str, LineNumbering); 4] = [
    ("Off", LineNumbering::Off),
    ("By Page", LineNumbering::ByPage),
    ("By Section", LineNumbering::BySection),
    ("Continuous", LineNumbering::Continuous),
];

/// Section > Line Numbers….
pub fn line_numbers(view: &DocView) -> Dialog {
    let s = view.current_section();
    let mut d = Dialog::new("Line Numbers", 290.0, 104.0);
    let names: Vec<&str> = NUMBERING.iter().map(|x| x.0).collect();
    let at = NUMBERING
        .iter()
        .position(|x| x.1 == s.line_numbers)
        .unwrap_or(0);
    d.popup("numbering", &names, at, 10.0, 8.0, 124.0);
    d.text("Count By:", 10.0, 44.0);
    d.field("countby", &s.line_count_by.to_string(), 96.0, 41.0, 60.0);
    d.text("From Text:", 10.0, 72.0);
    let from = s.line_from_text.map_or("Auto".to_string(), units::length);
    d.field("from", &from, 96.0, 69.0, 60.0);
    d.button("ok", "OK", 206.0, 8.0, 74.0, true);
    d.button("cancel", "Cancel", 206.0, 34.0, 74.0, false);
    d
}

/// OK in Line Numbers: the settings go to the selected sections.
pub fn apply_line_numbers(d: &Dialog, view: &mut DocView) -> bool {
    let numbering = NUMBERING[d.selected("numbering").min(3)].1;
    let Some(count) = d
        .field_text("countby")
        .trim()
        .parse::<u16>()
        .ok()
        .filter(|n| *n >= 1)
    else {
        return false;
    };
    let from = match d.field_text("from").trim() {
        f if f.is_empty() || f.eq_ignore_ascii_case("auto") => None,
        f => match units::parse_length(f) {
            Some(t) => Some(t.max(0)),
            None => return false,
        },
    };
    view.format_sections(|s| {
        s.line_numbers = numbering;
        s.line_count_by = count;
        s.line_from_text = from;
    });
    true
}
