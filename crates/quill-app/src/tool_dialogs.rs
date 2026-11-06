//! Tools > Word Count and Renumber, Format > Change Case, and Window >
//! Show Clipboard. Layouts from the original.

use crate::dialog::{Dialog, wrap};
use crate::docview::DocView;
use quill_core::text::WordCount;
use quill_core::tools::{Case, Numbers, Renumber};

// ----- Word Count -----

/// The counts shown: characters, words, lines, paragraphs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub wc: WordCount,
    pub lines: usize,
    /// The same for the footnotes.
    pub notes: WordCount,
    pub note_lines: usize,
}

const COUNT_ROWS: [(&str, &str); 4] = [
    ("chars", "Characters"),
    ("words", "Words"),
    ("lines", "Lines"),
    ("paras", "Paragraphs"),
];

pub fn word_count(view: &DocView) -> (Dialog, Counts) {
    let (wc, lines) = view.word_count();
    let (notes, note_lines) = view.note_count();
    let counts = Counts {
        wc,
        lines,
        notes,
        note_lines,
    };
    let mut d = Dialog::new("Word Count", 378.0, 146.0);
    d.text("Main Text", 117.0, 8.0);
    d.text("Footnotes", 200.0, 8.0);
    d.text("Total", 322.0, 8.0);
    for (k, (id, label)) in COUNT_ROWS.iter().enumerate() {
        // The original leaves Lines off: counting them needs the layout.
        d.check(id, label, 15.0, 33.0 + 17.0 * k as f32, *id != "lines");
    }
    for (k, (id, _)) in COUNT_ROWS.iter().enumerate() {
        let y = 33.0 + 17.0 * k as f32;
        d.text_as(count_id("main", id), "", 126.0, y);
        d.text_as(count_id("notes", id), "", 213.0, y);
        d.text_as(count_id("total", id), "", 326.0, y);
    }
    d.button("count", "Count", 18.0, 116.0, 54.0, true);
    d.button("cancel", "Cancel", 81.0, 116.0, 52.0, false);
    // Counting is quick, so it is done as the dialog opens.
    d.enable("count", false);
    let checks = count_checks(&d);
    update_count_labels(&mut d, &counts, checks);
    (d, counts)
}

fn count_id(column: &str, row: &str) -> &'static str {
    match (column, row) {
        ("main", "chars") => "main_chars",
        ("main", "words") => "main_words",
        ("main", "lines") => "main_lines",
        ("main", "paras") => "main_paras",
        ("notes", "chars") => "notes_chars",
        ("notes", "words") => "notes_words",
        ("notes", "lines") => "notes_lines",
        ("notes", "paras") => "notes_paras",
        ("total", "chars") => "total_chars",
        ("total", "words") => "total_words",
        ("total", "lines") => "total_lines",
        ("total", "paras") => "total_paras",
        _ => unreachable!("word-count column and row are fixed"),
    }
}

pub fn update_count_labels(d: &mut Dialog, counts: &Counts, checks: [bool; 4]) {
    let values = [
        counts.wc.characters,
        counts.wc.words,
        counts.lines,
        counts.wc.paragraphs,
    ];
    let notes = [
        counts.notes.characters,
        counts.notes.words,
        counts.note_lines,
        counts.notes.paragraphs,
    ];
    for (k, ((id, _), (value, note))) in COUNT_ROWS
        .iter()
        .zip(values.into_iter().zip(notes))
        .enumerate()
    {
        let shown = |n: usize| {
            if checks[k] {
                n.to_string()
            } else {
                String::new()
            }
        };
        d.set_label(count_id("main", id), &shown(value));
        d.set_label(count_id("notes", id), &shown(note));
        d.set_label(count_id("total", id), &shown(value + note));
    }
}

pub fn count_checks(d: &Dialog) -> [bool; 4] {
    COUNT_ROWS.map(|(id, _)| d.is_on(id))
}

// ----- Change Case -----

const CASES: [(&str, &str, Case); 5] = [
    ("upper", "UPPERCASE", Case::Upper),
    ("lower", "lowercase", Case::Lower),
    ("title", "Title Case", Case::Title),
    ("sentence", "Sentence case.", Case::Sentence),
    ("toggle", "tOGGLE cASE", Case::Toggle),
];

pub fn change_case() -> Dialog {
    let mut d = Dialog::new("Change Case", 217.0, 102.0);
    for (k, (id, label, _)) in CASES.iter().enumerate() {
        d.radio(id, label, 11.0, 9.0 + 17.0 * k as f32, 1, k == 0);
    }
    d.button("ok", "OK", 147.0, 13.0, 54.0, true);
    d.button("cancel", "Cancel", 148.0, 40.0, 52.0, false);
    d
}

pub fn chosen_case(d: &Dialog) -> Case {
    let on = d.radio_on(1);
    CASES
        .iter()
        .find(|(id, _, _)| *id == on)
        .map_or(Case::Upper, |c| c.2)
}

// ----- Footnote -----

/// Insert > Footnote (from memory of the original: a mark of your own or an
/// automatic number, and the separator buttons).
pub fn footnote() -> Dialog {
    let mut d = Dialog::new("Footnote", 384.0, 128.0);
    d.text("Footnote Reference Mark:", 12.0, 12.0);
    d.field("mark", "", 180.0, 9.0, 70.0);
    d.check("auto", "Auto-numbered Reference", 12.0, 38.0, true);
    d.group("Footnote Separators", 10.0, 64.0, 364.0, 54.0);
    d.button("sep", "Separator…", 18.0, 86.0, 104.0, false);
    d.button("cont_sep", "Cont. Separator…", 130.0, 86.0, 124.0, false);
    d.button("notice", "Cont. Notice…", 262.0, 86.0, 104.0, false);
    for id in ["sep", "cont_sep", "notice"] {
        d.enable(id, false);
    }
    d.button("ok", "OK", 300.0, 8.0, 74.0, true);
    d.button("cancel", "Cancel", 300.0, 34.0, 74.0, false);
    d.focus("mark");
    d
}

/// Typing a mark turns automatic numbering off, as in the original.
pub fn footnote_changed(d: &mut Dialog, id: &str) {
    if id == "mark" && !d.field_text("mark").is_empty() {
        d.set_on("auto", false);
    }
}

/// The mark the Footnote dialog asks for: None for a number.
pub fn footnote_mark(d: &Dialog) -> Option<String> {
    let m: String = d.field_text("mark").trim().chars().take(10).collect();
    (!d.is_on("auto") && !m.is_empty()).then_some(m)
}

// ----- Renumber -----

pub fn renumber() -> Dialog {
    let mut d = Dialog::new("Renumber", 352.0, 106.0);
    d.text("Paragraphs:", 14.0, 8.0);
    d.radio("all", "All", 104.0, 8.0, 1, true);
    d.radio("numbered", "Only If Already Numbered", 142.0, 8.0, 1, false);
    d.text("Start at:", 14.0, 31.0);
    d.field("start", "1", 72.0, 28.0, 86.0);
    d.text("Format:", 170.0, 31.0);
    d.field("format", "", 224.0, 28.0, 86.0);
    d.text("Numbers:", 14.0, 54.0);
    d.radio("n1", "1", 84.0, 54.0, 2, true);
    d.radio("n11", "1.1…", 116.0, 54.0, 2, false);
    d.radio("example", "By Example", 174.0, 54.0, 2, false);
    d.radio("remove", "Remove", 273.0, 54.0, 2, false);
    d.button("ok", "OK", 19.0, 80.0, 52.0, true);
    d.button("cancel", "Cancel", 87.0, 80.0, 55.0, false);
    d.focus("start");
    d
}

/// The dialog's settings, or None if Start At is not a number.
pub fn read_renumber(d: &Dialog) -> Option<Renumber> {
    let start: u32 = d
        .field_text("start")
        .trim()
        .parse()
        .ok()
        .filter(|n| *n > 0)?;
    Some(Renumber {
        only_numbered: d.radio_on(1) == "numbered",
        start,
        format: d.field_text("format"),
        numbers: match d.radio_on(2) {
            "n11" => Numbers::Outline,
            "example" => Numbers::ByExample,
            "remove" => Numbers::Remove,
            _ => Numbers::Plain,
        },
    })
}

// ----- Show Clipboard -----

pub fn clipboard() -> Dialog {
    let mut d = Dialog::new("Clipboard", 320.0, 180.0);
    d.modeless = true;
    d.close_box = true;
    for index in 0..12 {
        d.note(
            match index {
                0 => "clip0",
                1 => "clip1",
                2 => "clip2",
                3 => "clip3",
                4 => "clip4",
                5 => "clip5",
                6 => "clip6",
                7 => "clip7",
                8 => "clip8",
                9 => "clip9",
                10 => "clip10",
                _ => "clip11",
            },
            "",
            8.0,
            8.0 + index as f32 * 14.0,
            304.0,
        );
    }
    d
}

pub fn update_clipboard(d: &mut Dialog, text: &str) {
    let lines: Vec<String> = text.lines().flat_map(|line| wrap(line, 58)).collect();
    for index in 0..12 {
        d.set_label(
            match index {
                0 => "clip0",
                1 => "clip1",
                2 => "clip2",
                3 => "clip3",
                4 => "clip4",
                5 => "clip5",
                6 => "clip6",
                7 => "clip7",
                8 => "clip8",
                9 => "clip9",
                10 => "clip10",
                _ => "clip11",
            },
            lines.get(index).map_or("", String::as_str),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialog::Kind;
    use quill_core::Document;

    #[test]
    fn word_count_is_standard_controls_and_updates_each_column() {
        let view = DocView::new(Document::new(), "Test".to_string());
        let (mut dialog, counts) = word_count(&view);
        assert!(
            !dialog
                .items
                .iter()
                .any(|item| matches!(item.kind, Kind::Custom))
        );

        dialog.set_on("lines", true);
        let checks = count_checks(&dialog);
        update_count_labels(&mut dialog, &counts, checks);
        let label = |id| {
            dialog
                .items
                .iter()
                .find(|item| item.id == id)
                .map(|item| item.label.as_str())
        };
        assert_eq!(label("main_lines"), Some(counts.lines.to_string().as_str()));
        assert_eq!(
            label("total_lines"),
            Some((counts.lines + counts.note_lines).to_string().as_str())
        );
    }

    #[test]
    fn clipboard_is_native_and_updates_its_read_only_lines() {
        let mut dialog = clipboard();
        update_clipboard(
            &mut dialog,
            "A clipboard paragraph long enough to demonstrate native wrapping.",
        );
        assert!(dialog.modeless);
        assert!(
            !dialog
                .items
                .iter()
                .any(|item| matches!(item.kind, Kind::Custom))
        );
        assert!(
            dialog
                .items
                .iter()
                .find(|item| item.id == "clip0")
                .is_some_and(|item| !item.label.is_empty())
        );
    }
}
