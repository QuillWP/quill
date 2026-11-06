//! Paragraphs and the runs of character formatting inside them.

use crate::document::SectionProps;
use crate::props::{CharProps, ParaProps, RowProps};

/// Special characters that live inside paragraph text.
pub mod chars {
    pub const TAB: char = '\t';
    /// Shift+Return: a line break inside the paragraph ("End of Line Mark").
    /// The original stores it as character 11 too.
    pub const LINE_BREAK: char = '\u{000B}';
    /// Shift+Enter: a manual page break. Character 12, as in the original.
    pub const PAGE_BREAK: char = '\u{000C}';
    pub const NONBREAKING_SPACE: char = '\u{00A0}';
    pub const OPTIONAL_HYPHEN: char = '\u{00AD}';
    pub const NONBREAKING_HYPHEN: char = '\u{2011}';
    /// Stands in for a picture, field or footnote reference; the run's
    /// `CharProps::object` says which.
    pub const OBJECT: char = '\u{FFFC}';
    /// Fields from the header and footer icon bar: the page number, the
    /// date and the time, shown as their current values.
    pub const FIELD_PAGE: char = '\u{E000}';
    pub const FIELD_DATE: char = '\u{E001}';
    pub const FIELD_TIME: char = '\u{E002}';
    /// A footnote reference, in the text and at the start of its note.
    /// The run's `CharProps::object` is the note's id.
    pub const FOOTNOTE: char = '\u{E003}';
    /// An annotation's mark (Insert > Annotation), drawn as a balloon
    /// with the writer's initials. The run's `CharProps::object` is the
    /// annotation's id.
    pub const ANNOTATION: char = '\u{E004}';
}

/// What ends a paragraph.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mark {
    /// An ordinary paragraph mark (¶).
    #[default]
    Paragraph,
    /// Ends the last paragraph of a table cell.
    CellEnd,
    /// Ends a table row. The paragraph is empty and only carries the row.
    RowEnd,
    /// Ends a paragraph and the section it is in (⌘Enter).
    SectionBreak,
}

/// A run of characters sharing one format. `len` is in bytes of UTF-8.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Run {
    pub len: usize,
    pub chp: CharProps,
}

/// One paragraph: text, its character runs, and the mark that ends it.
///
/// Invariants: the run lengths add up to `text.len()`, no run is empty,
/// neighboring runs have different formats, and every run boundary is a
/// char boundary.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Paragraph {
    pub text: String,
    pub runs: Vec<Run>,
    pub pap: ParaProps,
    pub mark: Mark,
    /// Format of the paragraph mark itself. Typing into an empty paragraph
    /// uses it, and the original's character count includes the mark.
    pub mark_chp: CharProps,
    /// On a table row's end mark: the row's cells and settings.
    pub row: Option<Box<RowProps>>,
    /// On a section break: the settings of the section it ends. The last
    /// section's settings are the document's (`Document::section`).
    pub section: Option<Box<SectionProps>>,
}

impl Paragraph {
    pub fn new(pap: ParaProps, mark_chp: CharProps) -> Self {
        Paragraph {
            text: String::new(),
            runs: Vec::new(),
            pap,
            mark: Mark::Paragraph,
            row: None,
            section: None,
            mark_chp,
        }
    }

    pub fn with_text(text: &str, chp: CharProps, pap: ParaProps) -> Self {
        let mut p = Paragraph::new(pap, chp);
        p.insert(0, text, chp);
        p
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Format at byte `offset`, meaning the format new text typed there gets:
    /// the character before the offset, or the first character at the start,
    /// or the mark's format in an empty paragraph. This is the original's rule.
    ///
    /// Text typed at either end of a hyperlink is not part of it, as in
    /// the original: it drops the link and its color and underline, taking the
    /// paragraph mark's instead.
    pub fn chp_at(&self, offset: usize) -> CharProps {
        let mut chp = match self.runs.first() {
            None => self.mark_chp,
            Some(first) if offset == 0 => first.chp,
            // Any byte of the previous character lies in that character's run.
            Some(_) => self.chp_of_char(offset - 1),
        };
        if chp.link != 0 {
            let inside = offset > 0
                && offset < self.len()
                && self.chp_of_char(offset).link == chp.link
                && self.chp_of_char(offset - 1).link == chp.link;
            if !inside {
                chp.link = 0;
                chp.character_style = 0;
                chp.direct_format = 0;
                chp.color = self.mark_chp.color;
                chp.underline = self.mark_chp.underline;
            }
        }
        chp
    }

    /// The byte range of the hyperlink around `offset`: the link of the
    /// character there, or else of the one before it. None when neither
    /// is linked.
    pub fn link_range(&self, offset: usize) -> Option<(usize, usize, u32)> {
        let bounds: Vec<_> = self.run_bounds().map(|(s, e, c)| (s, e, c.link)).collect();
        let hit = bounds
            .iter()
            .position(|&(s, e, l)| l != 0 && s <= offset && offset < e)
            .or_else(|| {
                bounds
                    .iter()
                    .position(|&(_, e, l)| l != 0 && offset > 0 && e == offset)
            })?;
        let link = bounds[hit].2;
        let mut first = hit;
        while first > 0 && bounds[first - 1].2 == link {
            first -= 1;
        }
        let mut last = hit;
        while last + 1 < bounds.len() && bounds[last + 1].2 == link {
            last += 1;
        }
        Some((bounds[first].0, bounds[last].1, link))
    }

    /// Format of the character that starts at `offset`, or the mark's at the
    /// end of the paragraph.
    pub fn chp_of_char(&self, offset: usize) -> CharProps {
        let mut start = 0;
        for run in &self.runs {
            if offset < start + run.len {
                return run.chp;
            }
            start += run.len;
        }
        self.mark_chp
    }

    /// Insert `text` at byte `offset` with format `chp`.
    pub fn insert(&mut self, offset: usize, text: &str, chp: CharProps) {
        if text.is_empty() {
            return;
        }
        assert!(self.text.is_char_boundary(offset), "offset not on a char");
        self.text.insert_str(offset, text);
        let i = self.split_run_at(offset);
        self.runs.insert(
            i,
            Run {
                len: text.len(),
                chp,
            },
        );
        self.normalize();
    }

    /// Remove bytes `start..end`. Returns the removed text.
    pub fn remove(&mut self, start: usize, end: usize) -> String {
        if start >= end {
            return String::new();
        }
        let removed: String = self.text.drain(start..end).collect();
        // The runs still describe the old text, so the old offsets apply.
        let a = self.split_run_at(start);
        let b = self.split_run_at(end);
        self.runs.drain(a..b);
        self.normalize();
        removed
    }

    /// The paragraph's text and runs from `start..end` as a new paragraph
    /// with the same paragraph format.
    pub fn slice(&self, start: usize, end: usize) -> Paragraph {
        let mut out = Paragraph::new(self.pap.clone(), self.mark_chp);
        out.mark = self.mark;
        out.row = self.row.clone();
        out.section = self.section.clone();
        out.text = self.text[start..end].to_string();
        let mut pos = 0;
        for run in &self.runs {
            let (rs, re) = (pos, pos + run.len);
            pos = re;
            let (s, e) = (rs.max(start), re.min(end));
            if s < e {
                out.runs.push(Run {
                    len: e - s,
                    chp: run.chp,
                });
            }
        }
        out.normalize();
        out
    }

    /// Append another paragraph's text and runs to the end of this one.
    pub fn append(&mut self, other: &Paragraph) {
        self.text.push_str(&other.text);
        self.runs.extend_from_slice(&other.runs);
        self.normalize();
    }

    /// Split into two at `offset`. `self` keeps the text before; the new
    /// paragraph gets the text after. Both keep the paragraph format and
    /// mark format, as Return does in the original. The mark kind moves to the
    /// second half (the first half ends in an ordinary ¶).
    pub fn split_off(&mut self, offset: usize) -> Paragraph {
        let tail = self.slice(offset, self.len());
        self.remove(offset, self.len());
        self.mark = Mark::Paragraph;
        self.row = None;
        self.section = None;
        tail
    }

    /// Apply `f` to the format of every character in `start..end`.
    pub fn format_range(&mut self, start: usize, end: usize, f: &mut impl FnMut(&mut CharProps)) {
        if start >= end {
            return;
        }
        let a = self.split_run_at(start);
        let b = self.split_run_at(end);
        for run in &mut self.runs[a..b] {
            f(&mut run.chp);
        }
        self.normalize();
    }

    /// Byte offsets where runs start, plus the end. Handy for layout.
    pub fn run_bounds(&self) -> impl Iterator<Item = (usize, usize, &CharProps)> {
        let mut pos = 0;
        self.runs.iter().map(move |r| {
            let s = pos;
            pos += r.len;
            (s, pos, &r.chp)
        })
    }

    /// Make sure a run boundary exists at `offset` and return the index of
    /// the run that starts there (or `runs.len()` at the end).
    fn split_run_at(&mut self, offset: usize) -> usize {
        let mut pos = 0;
        for i in 0..self.runs.len() {
            if pos == offset {
                return i;
            }
            let len = self.runs[i].len;
            if offset < pos + len {
                let first = offset - pos;
                self.runs[i].len = first;
                let chp = self.runs[i].chp;
                self.runs.insert(
                    i + 1,
                    Run {
                        len: len - first,
                        chp,
                    },
                );
                return i + 1;
            }
            pos += len;
        }
        self.runs.len()
    }

    /// Restore the invariants: drop empty runs and merge equal neighbors.
    fn normalize(&mut self) {
        self.runs.retain(|r| r.len > 0);
        let mut i = 1;
        while i < self.runs.len() {
            if self.runs[i].chp == self.runs[i - 1].chp && self.runs[i].chp.object == 0 {
                self.runs[i - 1].len += self.runs[i].len;
                self.runs.remove(i);
            } else {
                i += 1;
            }
        }
        debug_assert_eq!(
            self.runs.iter().map(|r| r.len).sum::<usize>(),
            self.text.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bold() -> CharProps {
        CharProps {
            bold: true,
            ..Default::default()
        }
    }

    #[test]
    fn insert_merges_equal_runs() {
        let mut p = Paragraph::default();
        p.insert(0, "Hello", CharProps::default());
        p.insert(5, " world", CharProps::default());
        assert_eq!(p.text, "Hello world");
        assert_eq!(p.runs.len(), 1);
    }

    #[test]
    fn insert_in_middle_splits_runs() {
        let mut p = Paragraph::with_text("Hello", CharProps::default(), ParaProps::default());
        p.insert(2, "XX", bold());
        assert_eq!(p.text, "HeXXllo");
        let lens: Vec<_> = p.runs.iter().map(|r| (r.len, r.chp.bold)).collect();
        assert_eq!(lens, [(2, false), (2, true), (3, false)]);
    }

    #[test]
    fn remove_across_runs_rejoins() {
        let mut p = Paragraph::with_text("Hello", CharProps::default(), ParaProps::default());
        p.insert(2, "XX", bold());
        let gone = p.remove(1, 5);
        assert_eq!(gone, "eXXl");
        assert_eq!(p.text, "Hlo");
        assert_eq!(p.runs.len(), 1);
    }

    #[test]
    fn chp_at_uses_previous_character() {
        let mut p = Paragraph::with_text("ab", CharProps::default(), ParaProps::default());
        p.insert(2, "cd", bold());
        assert!(!p.chp_at(2).bold, "typing after 'b' continues plain");
        assert!(p.chp_at(3).bold);
        assert!(p.chp_at(4).bold);
        assert!(!p.chp_at(0).bold, "start of paragraph uses first char");
        let empty = Paragraph::new(ParaProps::default(), bold());
        assert!(empty.chp_at(0).bold, "empty paragraph uses the mark");
    }

    #[test]
    fn chp_at_start_of_bold_paragraph_is_bold() {
        let p = Paragraph::with_text("ab", bold(), ParaProps::default());
        assert!(p.chp_at(0).bold);
    }

    #[test]
    fn split_and_append_round_trip() {
        let mut p = Paragraph::with_text("Hello world", CharProps::default(), ParaProps::default());
        p.format_range(6, 11, &mut |c| c.italic = true);
        let tail = p.split_off(6);
        assert_eq!(p.text, "Hello ");
        assert_eq!(tail.text, "world");
        assert!(tail.runs[0].chp.italic);
        p.append(&tail);
        assert_eq!(p.text, "Hello world");
        assert_eq!(p.runs.len(), 2);
    }

    #[test]
    fn typing_at_the_ends_of_a_link_leaves_it() {
        let plain = CharProps::default();
        let linked = CharProps {
            link: 7,
            color: crate::props::Rgb(0x0000EE),
            underline: crate::props::Underline::Single,
            ..plain
        };
        let mut p = Paragraph::with_text("see ", plain, ParaProps::default());
        p.insert(4, "here", linked);
        p.insert(8, " now", plain);
        assert_eq!(p.chp_at(6).link, 7, "inside the link");
        let after = p.chp_at(8);
        assert_eq!(after.link, 0);
        assert_eq!(after.color, plain.color);
        assert_eq!(after.underline, plain.underline);
        assert_eq!(p.chp_at(4).link, 0, "before the link");
        assert_eq!(p.link_range(5), Some((4, 8, 7)));
        assert_eq!(p.link_range(8), Some((4, 8, 7)), "just after it");
        assert_eq!(p.link_range(2), None);
        let mut q = Paragraph::with_text("here", linked, ParaProps::default());
        q.mark_chp = plain;
        assert_eq!(q.chp_at(0).link, 0, "start of a paragraph");
        assert_eq!(q.chp_at(4).link, 0, "end of a paragraph");
    }

    #[test]
    fn format_range_on_multibyte_text() {
        let mut p = Paragraph::with_text("naïve café", CharProps::default(), ParaProps::default());
        let start = p.text.find("café").unwrap();
        p.format_range(start, p.len(), &mut |c| c.bold = true);
        assert_eq!(p.slice(start, p.len()).text, "café");
        assert!(p.chp_of_char(start).bold);
        assert!(!p.chp_of_char(0).bold);
    }

    #[test]
    fn objects_never_merge() {
        let obj = |n| CharProps {
            object: n,
            ..Default::default()
        };
        let mut p = Paragraph::default();
        p.insert(0, "\u{FFFC}", obj(1));
        p.insert(3, "\u{FFFC}", obj(1));
        assert_eq!(p.runs.len(), 2);
    }
}
