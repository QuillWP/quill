//! The editing tools of milestone 8 as the document window runs them:
//! Change Case, Sort, Calculate, Renumber, Bullet, Go Back, and the
//! selection and counting that Find and Word Count need.

use super::*;
use quill_core::find::{Pattern, Replacement};
use quill_core::text::WordCount;
use quill_core::tools::{self, Case, Renumber};

/// How many edit locations Go Back remembers, as in the original.
const BACK: usize = 3;

impl DocView {
    /// The first and last paragraph the selection covers. A selection that
    /// ends at the very start of a paragraph does not include it.
    pub fn selected_paras(&self) -> (usize, usize) {
        let (s, e) = self.sel.range();
        let last = if e.para > s.para && e.offset == 0 {
            e.para - 1
        } else {
            e.para
        };
        (s.para, last)
    }

    /// Select a range found by Find, and bring it into view.
    pub fn select_found(&mut self, start: Position, end: Position) {
        self.set_selection(Selection::new(start, end));
        self.doc.break_typing();
        self.scroll_to_caret();
    }

    /// Replace one match, as an undoable step. Returns where the
    /// replacement is.
    pub fn replace_found(
        &mut self,
        start: Position,
        end: Position,
        rep: &Replacement,
        match_case: bool,
        clip: Option<&Fragment>,
    ) -> (Position, Position) {
        self.doc.begin("Replace", self.sel);
        let r = quill_core::find::replace(&mut self.doc, MAIN, start, end, rep, match_case, clip);
        self.finish_edit(Selection::new(r.0, r.1));
        r
    }

    /// Replace All in `from..to`, as one undoable step. Returns the count
    /// and shows it in the status bar.
    pub fn replace_all(
        &mut self,
        pat: &Pattern,
        rep: &Replacement,
        from: Position,
        to: Position,
        clip: Option<&Fragment>,
    ) -> usize {
        self.doc.begin("Replace All", self.sel);
        let n = quill_core::find::replace_all(&mut self.doc, MAIN, pat, rep, from, to, clip);
        self.clamp_selection();
        let sel = self.sel;
        self.finish_edit(sel);
        self.status = Some(match n {
            1 => "1 Change".to_string(),
            n => format!("{n} Changes"),
        });
        n
    }

    /// Go To: put the insertion point at the top of page `page` (from 0).
    pub fn go_to_page(&mut self, page: usize) {
        let Some(ps) = self.pages.get(page).copied() else {
            return;
        };
        let offset = self
            .layouts
            .get(ps.para)
            .and_then(|l| l.as_ref())
            .and_then(|l| l.lines.get(ps.line))
            .map_or(0, |l| l.start);
        self.set_selection(Selection::caret(Position::new(ps.para, offset)));
        self.doc.break_typing();
        self.scroll_to_caret();
    }

    /// Remember where an edit happened, for Go Back.
    pub(super) fn note_edit(&mut self, at: Position) {
        if let Some(last) = self.back.last_mut()
            && last.para == at.para
        {
            *last = at;
            return;
        }
        self.back.push(at);
        if self.back.len() > BACK {
            self.back.remove(0);
        }
    }

    /// Go Back (⌘⌥Z): to the last place an edit was made, then the one
    /// before, round the last three.
    pub fn go_back(&mut self) {
        if self.back.is_empty() {
            return;
        }
        // The newest place that is not where we already are.
        let here = self.sel.focus;
        let k = self
            .back
            .iter()
            .rposition(|p| *p != here)
            .unwrap_or(self.back.len() - 1);
        let to = self.back.remove(k);
        self.back.insert(0, to);
        self.set_selection(Selection::caret(to));
        self.scroll_to_caret();
    }

    /// Format > Change Case, and the case commands.
    pub fn change_case(&mut self, case: Case) {
        if self.sel.is_empty() {
            return;
        }
        let (s, e) = self.sel.range();
        self.doc.begin("Change Case", self.sel);
        let end = tools::change_case(&mut self.doc, MAIN, s, e, case);
        let sel = if self.sel.anchor <= self.sel.focus {
            Selection::new(s, end)
        } else {
            Selection::new(end, s)
        };
        self.finish_edit(sel);
    }

    /// The table around paragraph `p`: its first and last paragraphs.
    pub fn table_around(&self, p: usize) -> Option<(usize, usize)> {
        let paras = &self.doc.main.paras;
        if !paras.get(p)?.pap.in_table {
            return None;
        }
        let mut first = p;
        while first > 0 && paras[first - 1].pap.in_table {
            first -= 1;
        }
        let mut last = p;
        while last + 1 < paras.len() && paras[last + 1].pap.in_table {
            last += 1;
        }
        Some((first, last))
    }

    /// Select Table: the whole table the insertion point is in, ready to
    /// cut, copy or move.
    pub fn select_table(&mut self) {
        let Some((first, last)) = self.table_around(self.sel.focus.para) else {
            return;
        };
        let end = if last + 1 < self.doc.main.paras.len() {
            Position::new(last + 1, 0)
        } else {
            Position::new(last, self.doc.main.paras[last].len())
        };
        self.set_selection(Selection::new(Position::new(first, 0), end));
        self.doc.break_typing();
    }

    /// Shift-F3: lowercase, then UPPERCASE, then Title Case, then round
    /// again. With nothing selected it works on the word at the insertion
    /// point and leaves the insertion point where it was.
    pub fn cycle_case(&mut self) {
        let caret = self.sel.is_empty().then_some(self.sel);
        let (s, e) = if caret.is_some() {
            match self.word_at_caret() {
                Some(w) => w,
                None => return,
            }
        } else {
            self.sel.range()
        };
        let text = self.doc.main.text_of(s, e);
        let case = if !text.chars().any(char::is_alphabetic) {
            return;
        } else if !text.chars().any(char::is_uppercase) {
            Case::Upper
        } else if !text.chars().any(char::is_lowercase) {
            Case::Title
        } else {
            Case::Lower
        };
        let sel = self.sel;
        self.sel = Selection::new(s, e);
        self.change_case(case);
        if let Some(caret) = caret {
            self.sel = caret;
            self.clamp_selection();
        } else if sel.anchor > sel.focus {
            self.sel = Selection::new(self.sel.end(), self.sel.start());
        }
    }

    /// Tools > Sort (Shift for descending). In a table, rows are ordered by
    /// the column the selection starts in.
    pub fn sort(&mut self, descending: bool) {
        if self.sel.is_empty() {
            return;
        }
        let (first, last) = self.selected_paras();
        let column = table::row_at(&self.doc.main.paras, first)
            .and_then(|r| r.cell_of(first))
            .unwrap_or(0);
        self.doc.begin("Sort", self.sel);
        if tools::sort(&mut self.doc, MAIN, first, last, column, descending) {
            // The sorted paragraphs stay selected.
            let end_para = last.min(self.doc.main.paras.len() - 1);
            let end = Position::new(end_para, self.doc.main.paras[end_para].len());
            let sel = Selection::new(Position::new(first, 0), end);
            self.finish_edit(sel);
        } else {
            let sel = self.sel;
            self.doc.commit(sel);
        }
    }

    /// Tools > Calculate (⌘=): work out the selection, show the answer in
    /// the status bar and put it on the Clipboard.
    pub fn calculate(&mut self, clip: &mut Clipboard) {
        let (s, e) = self.sel.range();
        let text = self.doc.main.text_of(s, e);
        match tools::calculate(&text) {
            Some(answer) => {
                clip.put_text(&answer);
                self.status = Some(answer);
            }
            None => self.status = Some("Error".to_string()),
        }
    }

    /// Run an edit that rewrites the start of paragraphs `first..=last`,
    /// keeping the selection on the same text as the starts grow or
    /// shrink.
    fn edit_paragraph_starts(
        &mut self,
        label: &'static str,
        f: impl FnOnce(&mut Document, usize, usize),
    ) {
        let (first, last) = self.selected_paras();
        let before: Vec<usize> = self.doc.main.paras[first..=last]
            .iter()
            .map(|p| p.len())
            .collect();
        self.doc.begin(label, self.sel);
        f(&mut self.doc, first, last);
        let paras = &self.doc.main.paras;
        let shift = |p: Position| {
            if p.para < first || p.para > last || p.para >= paras.len() {
                return p;
            }
            let delta = paras[p.para].len() as isize - before[p.para - first] as isize;
            let mut off = (p.offset as isize + delta).max(0) as usize;
            off = off.min(paras[p.para].len());
            while !paras[p.para].text.is_char_boundary(off) {
                off -= 1;
            }
            Position::new(p.para, off)
        };
        let sel = Selection::new(shift(self.sel.anchor), shift(self.sel.focus));
        self.finish_edit(sel);
    }

    /// Insert > Bullet and the Toolbar's Bullet button.
    pub fn bullet(&mut self) {
        self.edit_paragraph_starts("Bullet", |doc, a, b| tools::bullet(doc, MAIN, a, b));
    }

    /// Tools > Renumber. Without a selection it numbers the whole
    /// document.
    pub fn renumber(&mut self, opts: &Renumber) {
        if self.sel.is_empty() {
            let end = self.doc.main.end();
            self.sel = Selection::new(self.doc.main.start(), end);
        }
        self.edit_paragraph_starts("Renumber", |doc, a, b| {
            tools::renumber(doc, MAIN, a, b, opts)
        });
    }

    /// Insert > Drop Cap. Shows Page Layout view, where drop caps are
    /// placed, as the original does. Err with the original's message if it cannot.
    pub fn drop_cap(&mut self, dc: &crate::frame_dialog::DropCap) -> Result<(), &'static str> {
        let at = self.sel.start();
        let p = &self.doc.main.paras[at.para];
        if p.pap.frame.is_some() {
            return Err("Cannot Insert Drop Cap to a framed paragraph.");
        }
        if p.pap.in_table || p.text.is_empty() {
            return Err("Cannot Insert Drop Cap here.");
        }
        let len = match dc.apply_to {
            2 if self.sel.end().para == at.para && at.offset == 0 => self.sel.end().offset,
            1 => p.text.find(char::is_whitespace).unwrap_or(p.text.len()),
            _ => p.text.chars().next().map_or(0, char::len_utf8),
        };
        // The paragraph's line height, from its layout.
        let line = self
            .layouts
            .get(at.para)
            .and_then(|l| l.as_ref())
            .and_then(|l| l.lines.first().map(|ln| ln.height))
            .unwrap_or(p.mark_chp.size as f32 / 2.0 * 4.0 / 3.0);
        let font = self.doc.font_id(&dc.font);
        self.doc.begin("Drop Cap", self.sel);
        let ok = tools::drop_cap(
            &mut self.doc,
            MAIN,
            at.para,
            len,
            dc.lines,
            (line * 20.0).round() as i32,
            font,
            dc.in_margin,
            dc.small_caps,
        );
        if !ok {
            let sel = self.sel;
            self.doc.commit(sel);
            return Err("Cannot Insert Drop Cap here.");
        }
        let cap = Position::new(at.para, 0);
        self.finish_edit(Selection::new(cap, Position::new(at.para, len)));
        self.page_view = true;
        Ok(())
    }

    /// Type text where the selection is, as one undoable step, with the
    /// insertion point's format.
    pub fn insert_text(&mut self, text: &str) {
        self.doc.break_typing();
        self.type_text_raw(text);
    }

    /// Insert a fragment (a glossary entry) at the selection.
    pub fn insert_fragment(&mut self, label: &'static str, frag: &Fragment) {
        self.doc.begin(label, self.sel);
        let at = self.delete_selection();
        let end = self.doc.insert_fragment(MAIN, at, frag);
        self.finish_edit(Selection::caret(end));
    }

    /// Tools > Word Count over the selection, or the whole text without
    /// one: characters, words, lines and paragraphs.
    pub fn word_count(&self) -> (WordCount, usize) {
        let whole = (self.doc.main.start(), self.doc.main.end());
        // Select All counts as the whole document, marks and all.
        let all = self.sel.is_empty() || self.sel.range() == whole;
        let (s, e) = if all { whole } else { self.sel.range() };
        let mut counted = Document::new();
        let frag = self.doc.copy(MAIN, s, e);
        counted.main.paras = frag.paras.clone();
        if !all && !frag.ends_with_mark {
            // A piece of a paragraph has no mark to count.
            let wc = quill_core::text::word_count(&counted);
            let lines = self.lines_in(s.para, e.para);
            return (
                WordCount {
                    characters: wc.characters - 1,
                    ..wc
                },
                lines,
            );
        }
        let wc = quill_core::text::word_count(&counted);
        (wc, self.lines_in(s.para, e.para))
    }

    /// Word Count's footnote column: the notes of the references in the
    /// selection (or all of them), and their lines.
    pub fn note_count(&self) -> (WordCount, usize) {
        let whole = (self.doc.main.start(), self.doc.main.end());
        let all = self.sel.is_empty() || self.sel.range() == whole;
        let (s, e) = if all { whole } else { self.sel.range() };
        let frag = self.doc.copy(MAIN, s, e);
        let ids = quill_core::notes::note_ids(&frag.paras);
        let mut counted = Document::new();
        counted.main.paras.clear();
        let mut lines = 0;
        for id in ids {
            if let Some(r) = self.note_ranges.get(&id) {
                counted
                    .main
                    .paras
                    .extend(self.doc.notes.paras[r.clone()].iter().cloned());
                lines += r
                    .clone()
                    .filter_map(|i| self.note_layouts.get(i))
                    .map(|l| l.lines.len())
                    .sum::<usize>();
            }
        }
        if counted.main.paras.is_empty() {
            return (WordCount::default(), 0);
        }
        (quill_core::text::word_count(&counted), lines)
    }

    /// Lines laid out in paragraphs `first..=last` (row end marks have
    /// none of their own).
    fn lines_in(&self, first: usize, last: usize) -> usize {
        (first..=last)
            .filter(|&i| self.doc.main.paras[i].mark != Mark::RowEnd)
            .map(|i| {
                self.layouts
                    .get(i)
                    .and_then(|l| l.as_ref())
                    .map_or(1, |l| l.lines.len().max(1))
            })
            .sum()
    }
}
