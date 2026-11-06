//! Plain text in and out, and Word Count.

use crate::document::{Document, StoryId};
use crate::story::Position;

/// A new document holding `text`, one paragraph per line, in Normal.
pub fn from_plain_text(text: &str) -> Document {
    let mut doc = Document::new();
    let chp = doc.styles.get(0).chp;
    // Strip a byte-order mark if the file had one.
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    doc.insert_plain(StoryId::Main, Position::new(0, 0), text, chp);
    doc.commit(Default::default());
    // Loading is not an edit the user can undo.
    doc.forget_history();
    doc
}

/// The document's main text with each paragraph mark written as `eol`.
/// Line breaks become `eol` too; page breaks become form feeds.
///
/// Tracked changes are taken as accepted: deleted text is left out.
pub fn to_plain_text(doc: &Document, eol: &str) -> String {
    let mut out = String::new();
    let accepted;
    let paras = if crate::tracking::any_changes(&doc.main.paras) {
        accepted = crate::tracking::accepted(&doc.main.paras);
        &accepted
    } else {
        &doc.main.paras
    };
    for (i, p) in paras.iter().enumerate() {
        for c in p.text.chars() {
            match c {
                '\u{000B}' => out.push_str(eol),
                '\u{00AD}' | '\u{FFFC}' => {}
                '\u{00A0}' => out.push(' '),
                '\u{2011}' => out.push('-'),
                c => out.push(c),
            }
        }
        if i + 1 < paras.len() {
            out.push_str(eol);
        }
    }
    out
}

/// What Tools > Word Count reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WordCount {
    /// Every character, counting each paragraph, cell and row mark as one.
    pub characters: usize,
    /// Runs of characters between spaces, tabs and marks.
    pub words: usize,
    /// Paragraphs, including table cells, row ends and empty paragraphs.
    pub paragraphs: usize,
}

pub fn word_count(doc: &Document) -> WordCount {
    // Deleted text doesn't count.
    let accepted;
    let story = if crate::tracking::any_changes(&doc.main.paras) {
        accepted = crate::story::Story {
            paras: crate::tracking::accepted(&doc.main.paras),
        };
        &accepted
    } else {
        &doc.main
    };
    let mut words = 0;
    for p in &story.paras {
        let mut in_word = false;
        for c in p.text.chars() {
            let space = c.is_whitespace() || c == '\u{000B}' || c == '\u{000C}';
            if !space && !in_word {
                words += 1;
            }
            in_word = !space;
        }
    }
    WordCount {
        characters: story.char_count(),
        words,
        paragraphs: story.paras.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::para::{Mark, Paragraph};
    use crate::props::{CharProps, ParaProps};

    #[test]
    fn round_trip_lines() {
        let doc = from_plain_text("One\r\nTwo\rThree\nFour");
        let texts: Vec<_> = doc.main.paras.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(texts, ["One", "Two", "Three", "Four"]);
        assert_eq!(to_plain_text(&doc, "\n"), "One\nTwo\nThree\nFour");
        assert!(!doc.is_modified());
        assert!(doc.undo_label().is_none());
    }

    #[test]
    fn trailing_newline_makes_final_empty_paragraph() {
        let doc = from_plain_text("One\n");
        assert_eq!(doc.main.paras.len(), 2);
        assert_eq!(to_plain_text(&doc, "\n"), "One\n");
    }

    /// The same document counted by the original: 100
    /// characters, 17 words, 10 paragraphs.
    #[test]
    fn matches_original_count() {
        let mut doc = from_plain_text(
            "Hello Text. The Quick brown fox!\nSecond paragraph, with 123 numbers.\n\tTabbed text.",
        );
        let cell = |t: &str, mark| {
            let mut p = Paragraph::with_text(t, CharProps::default(), ParaProps::default());
            p.mark = mark;
            p
        };
        // A 2 x 2 table, then the final empty paragraph. The B2 cell held
        // two extra symbol characters when the original counted it.
        let table = [
            cell("A1", Mark::CellEnd),
            cell("B1", Mark::CellEnd),
            cell("", Mark::RowEnd),
            cell("A2", Mark::CellEnd),
            cell("B2\u{2022}\u{2022}", Mark::CellEnd),
            cell("", Mark::RowEnd),
            cell("", Mark::Paragraph),
        ];
        doc.main.paras.extend(table);
        let wc = word_count(&doc);
        assert_eq!(wc.words, 17);
        assert_eq!(wc.paragraphs, 10);
        assert_eq!(wc.characters, 100);
    }
}
