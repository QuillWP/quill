//! Tools > Spelling, Hyphenation and Thesaurus, on the document: finding
//! the next unknown word, changing words, and finding where an optional
//! hyphen would let a line take part of the next line's first word.

use super::*;

/// The original's hot zone: a line ending further than this from the right
/// edge is worth hyphenating.
const HOT_ZONE: f32 = 18.0;

/// A place hyphenation could help: the first word of a line, which could
/// break at `breaks` (character counts), the best that fits being
/// `proposed`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HyphenSpot {
    pub start: Position,
    pub end: Position,
    pub word: String,
    pub breaks: Vec<usize>,
    pub proposed: usize,
}

/// Hyphenate All (or Selection), carried out a line at a time as the
/// layout catches up.
#[derive(Clone, Copy, Debug)]
pub struct HyphenJob {
    pub from: Position,
    pub end: Option<Position>,
    pub capitals: bool,
}

impl DocView {
    /// The next word from `from` that is not in the dictionaries, skipping
    /// hidden text and, as Preferences say, words in capitals or with
    /// numbers.
    pub fn next_misspelling(&self, from: Position) -> Option<(Position, Position)> {
        let (upper, numbers) = crate::settings::with(|s| (s.ignore_uppercase, s.ignore_numbers));
        let paras = &self.doc.main.paras;
        for (i, p) in paras.iter().enumerate().skip(from.para) {
            let start = if i == from.para {
                from.offset.min(p.len())
            } else {
                0
            };
            // Start at a word boundary, not in the middle of one.
            let start = (0..=start)
                .rev()
                .find(|&k| p.text.is_char_boundary(k))
                .unwrap_or(0);
            for (s, e) in quill_core::spell::words(&p.text, start) {
                if s < start || p.chp_of_char(s).hidden {
                    continue;
                }
                let w = &p.text[s..e];
                if numbers && w.chars().any(|c| c.is_numeric()) {
                    continue;
                }
                if upper && w.chars().count() > 1 && w.chars().all(|c| !c.is_lowercase()) {
                    continue;
                }
                if !crate::language::word_ok(w) {
                    return Some((Position::new(i, s), Position::new(i, e)));
                }
            }
        }
        None
    }

    /// Change the word at `s..e` to `with`, as one undoable step, and
    /// leave the insertion point after it.
    pub fn replace_word(&mut self, s: Position, e: Position, with: &str) {
        let chp = self.doc.main.paras[s.para].chp_of_char(s.offset);
        self.doc.begin("Change", Selection::new(s, e));
        self.doc.delete(MAIN, s, e);
        let end = self.doc.insert_text(MAIN, s, with, chp);
        self.finish_edit(Selection::caret(end));
    }

    /// Put plain text in place of the selection, a paragraph for each
    /// line, as one undoable step.
    pub fn insert_plain_text(&mut self, label: &'static str, text: &str) {
        self.doc.begin(label, self.sel);
        let at = self.delete_selection();
        let chp = self
            .typing_chp
            .unwrap_or_else(|| self.doc.main.paras[at.para].chp_at(at.offset));
        let end = self.doc.insert_plain(MAIN, at, text, chp);
        self.finish_edit(Selection::caret(end));
    }

    /// Change All: every whole-word `word` from `from` on becomes `with`.
    /// Returns how many changed.
    pub fn change_all(&mut self, word: &str, with: &str) -> usize {
        let sel = self.sel;
        self.doc.begin("Change All", sel);
        let mut n = 0;
        for i in 0..self.doc.main.paras.len() {
            let spans: Vec<(usize, usize)> =
                quill_core::spell::words(&self.doc.main.paras[i].text, 0)
                    .into_iter()
                    .filter(|&(s, e)| &self.doc.main.paras[i].text[s..e] == word)
                    .collect();
            for &(s, e) in spans.iter().rev() {
                let chp = self.doc.main.paras[i].chp_of_char(s);
                self.doc
                    .delete(MAIN, Position::new(i, s), Position::new(i, e));
                self.doc.insert_text(MAIN, Position::new(i, s), with, chp);
                n += 1;
            }
        }
        self.clamp_selection();
        let sel = self.sel;
        self.finish_edit(sel);
        n
    }

    /// The word the Thesaurus starts from: the selection's first word, or
    /// the word at the insertion point.
    pub fn word_at_caret(&self) -> Option<(Position, Position)> {
        let (s, e) = self.sel.range();
        let p = &self.doc.main.paras[s.para];
        let words = quill_core::spell::words(&p.text, 0);
        if !self.sel.is_empty() {
            let (a, b) = words.into_iter().find(|&(_, b)| b > s.offset)?;
            let b = if e.para == s.para {
                b.min(e.offset.max(a))
            } else {
                b
            };
            return (a < b).then_some((Position::new(s.para, a), Position::new(s.para, b)));
        }
        words
            .into_iter()
            .find(|&(a, b)| a <= s.offset && s.offset <= b)
            .map(|(a, b)| (Position::new(s.para, a), Position::new(s.para, b)))
    }

    /// The next line start from `from` (up to `end`) whose first word could
    /// be partly taken up by the line before, by the original's hot zone.
    pub fn next_hyphenation(
        &self,
        from: Position,
        end: Option<Position>,
        capitals: bool,
    ) -> Option<HyphenSpot> {
        let h = crate::language::hyphenator()?;
        let paras = &self.doc.main.paras;
        for (i, p) in paras.iter().enumerate().skip(from.para) {
            if end.is_some_and(|e| i > e.para) {
                break;
            }
            if p.pap.in_table {
                continue;
            }
            let Some(Some(l)) = self.layouts.get(i) else {
                continue;
            };
            let right = self.widths.get(i).copied().unwrap_or(0.0) - p.pap.right as f32 / 20.0;
            for k in 0..l.lines.len().saturating_sub(1) {
                let (line, next) = (&l.lines[k], &l.lines[k + 1]);
                let ws = next.start;
                if Position::new(i, ws) < from || end.is_some_and(|e| Position::new(i, ws) >= e) {
                    continue;
                }
                let free = right - line.end_x;
                if free < HOT_ZONE {
                    continue;
                }
                // The first word of the next line, letters only.
                let we = p.text[ws..]
                    .char_indices()
                    .find(|(_, c)| !c.is_alphabetic())
                    .map_or(p.text.len(), |(k, _)| ws + k);
                let word = &p.text[ws..we];
                if word.chars().count() < 5
                    || p.chp_of_char(ws).hidden
                    || (!capitals && word.chars().next().is_some_and(|c| c.is_uppercase()))
                    || p.text[we..].starts_with(chars::OPTIONAL_HYPHEN)
                {
                    continue;
                }
                let breaks = h.breaks(word);
                let glyph_x = |off: usize| {
                    l.line_glyphs(next)
                        .iter()
                        .find(|g| g.offset as usize == off)
                        .map(|g| g.x)
                };
                let Some(x0) = glyph_x(ws) else {
                    continue;
                };
                let size = p.chp_of_char(ws).size as f32 / 2.0;
                let dash = size * 0.33;
                let fits = breaks.iter().rev().copied().find(|&b| {
                    let off = ws + word.char_indices().nth(b).map_or(word.len(), |(k, _)| k);
                    glyph_x(off).is_some_and(|x| x - x0 + dash <= free)
                });
                if let Some(proposed) = fits {
                    return Some(HyphenSpot {
                        start: Position::new(i, ws),
                        end: Position::new(i, we),
                        word: word.to_string(),
                        breaks,
                        proposed,
                    });
                }
            }
        }
        None
    }

    /// Put an optional hyphen after the `at`th letter of the word at
    /// `spot`.
    pub fn insert_hyphen(&mut self, spot: &HyphenSpot, at: usize) {
        let off = spot.start.offset
            + spot
                .word
                .char_indices()
                .nth(at)
                .map_or(spot.word.len(), |(k, _)| k);
        let pos = Position::new(spot.start.para, off);
        let chp = self.doc.main.paras[pos.para].chp_of_char(spot.start.offset);
        self.doc.begin("Hyphenation", self.sel);
        self.doc
            .insert_text(MAIN, pos, &chars::OPTIONAL_HYPHEN.to_string(), chp);
        if self.hyph_job.is_none() {
            let sel = self.sel;
            self.doc.commit(sel);
        }
    }

    /// Hyphenate All: hyphenate every line it can from `from`, as one
    /// undoable step, while the layout catches up after each.
    pub fn hyphenate_all(&mut self, from: Position, end: Option<Position>, capitals: bool) {
        self.doc.begin("Hyphenation", self.sel);
        self.hyph_job = Some(HyphenJob {
            from,
            end,
            capitals,
        });
    }

    /// One step of Hyphenate All. True if it changed something, so the
    /// layout must be brought up to date before the next.
    pub(super) fn hyphenate_step(&mut self) -> bool {
        let Some(job) = self.hyph_job else {
            return false;
        };
        match self.next_hyphenation(job.from, job.end, job.capitals) {
            Some(spot) => {
                self.insert_hyphen(&spot, spot.proposed);
                let end = job.end.map(|e| {
                    if e.para == spot.start.para {
                        Position::new(e.para, e.offset + chars::OPTIONAL_HYPHEN.len_utf8())
                    } else {
                        e
                    }
                });
                self.hyph_job = Some(HyphenJob {
                    from: Position::new(spot.start.para, spot.start.offset + 1),
                    end,
                    ..job
                });
                true
            }
            None => {
                self.hyph_job = None;
                let sel = Selection::caret(self.sel.range().0);
                self.doc.commit(sel);
                self.set_selection(sel);
                false
            }
        }
    }
}
