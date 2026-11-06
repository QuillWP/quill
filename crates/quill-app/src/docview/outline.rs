//! Outline view (View > Outline): the document by its headings. Each
//! heading is indented by its level and body text sits under its heading;
//! a symbol beside each paragraph shows what it is (a hollow plus for a
//! heading with text under it, a bar for one without, a small square for
//! body text). Headings can be promoted, demoted and moved with what is
//! under them, and what is under a heading can be collapsed out of view.
//! Measurements are the original's.

use super::*;

/// Each level of heading is indented this much more, in points.
pub const LEVEL: f32 = 40.0;
/// From a symbol to its paragraph's text.
const SYMBOL_GAP: f32 = 20.0;
/// Body text is level 10: under every heading.
const BODY: u8 = 10;

impl DocView {
    /// The outline level of paragraph `i`: 1-9 for headings, 10 for body
    /// text.
    pub(super) fn level(&self, i: usize) -> u8 {
        let p = &self.doc.main.paras[i].pap;
        self.doc.styles.heading_level(p.style).unwrap_or(BODY)
    }

    /// The paragraphs under paragraph `i`: those after it up to the next
    /// heading of its level or above.
    pub(super) fn subtext(&self, i: usize) -> std::ops::Range<usize> {
        let l = self.level(i);
        let n = self.doc.main.paras.len();
        if l == BODY {
            return i + 1..i + 1;
        }
        let end = (i + 1..n).find(|&j| self.level(j) <= l).unwrap_or(n);
        i + 1..end
    }

    /// Where paragraph `i`'s symbol and text go across, from the left
    /// margin, in Outline view.
    pub(super) fn outline_x(&self, i: usize) -> (f32, f32) {
        let l = self.level(i);
        if l != BODY {
            let sym = LEVEL * (l - 1) as f32;
            return (sym, sym + SYMBOL_GAP);
        }
        // Body text sits under the heading before it.
        let under = (0..i).rev().map(|j| self.level(j)).find(|&l| l != BODY);
        let text = under.map_or(0.0, |h| LEVEL * (h - 1) as f32 + SYMBOL_GAP);
        (text + 2.0, text + SYMBOL_GAP)
    }

    /// Whether paragraph `i` is hidden in Outline view.
    pub(super) fn outline_hidden(&self, i: usize) -> bool {
        self.outline && self.collapsed.get(i).copied().unwrap_or(false)
    }

    /// The paragraphs a command acts on: the selected ones, with what is
    /// hidden under any of them.
    fn outline_targets(&self) -> std::ops::RangeInclusive<usize> {
        let (s, e) = self.sel.range();
        let mut last = e.para;
        if e.para > s.para && e.offset == 0 {
            last -= 1;
        }
        let sub = self.subtext(last);
        let hidden_end = sub
            .take_while(|&j| self.collapsed.get(j) == Some(&true))
            .last();
        s.para..=hidden_end.unwrap_or(last).max(last)
    }

    /// Give paragraphs new heading levels (None makes them body text), as
    /// one undoable step.
    fn relevel(&mut self, label: &'static str, f: impl Fn(u8, Option<u8>) -> Option<u8>) {
        let range = self.outline_targets();
        let sel = self.sel;
        self.doc.begin(label, sel);
        let mut before = (0..*range.start())
            .rev()
            .map(|j| self.level(j))
            .find(|&l| l != BODY);
        for i in range {
            let l = self.level(i);
            let want = f(l, before);
            let style = match want {
                Some(n) => self.doc.styles.find(&format!("heading {}", n.clamp(1, 9))),
                None => Some(0),
            };
            if let Some(style) = style
                && style != self.doc.main.paras[i].pap.style
            {
                self.doc.apply_style(MAIN, i, i, style);
            }
            if let Some(n) = want {
                before = Some(n);
            }
        }
        self.doc.commit(sel);
    }

    /// ← Promote: headings up a level; body text becomes a heading at
    /// the level of the one it is under.
    pub fn promote(&mut self) {
        self.relevel("Promote", |l, before| match l {
            BODY => Some(before.unwrap_or(1)),
            l => Some(l.saturating_sub(1).max(1)),
        });
    }

    /// → Demote: headings down a level; body text stays.
    pub fn demote(&mut self) {
        self.relevel("Demote", |l, _| match l {
            BODY => None,
            l => Some((l + 1).min(9)),
        });
    }

    /// » Demote to Body Text.
    pub fn demote_to_body(&mut self) {
        self.relevel("Demote to Body", |_, _| None);
    }

    /// ↑ and ↓: move the selected paragraphs (with what is collapsed
    /// under them) past the visible paragraph before or after them.
    pub fn move_paragraphs(&mut self, down: bool) {
        let range = self.outline_targets();
        let (a, b) = (*range.start(), *range.end());
        let n = self.doc.main.paras.len();
        // The other block: the next visible paragraph and what is hidden
        // under it, or the previous visible one and its hidden text.
        let other = if down {
            if b + 2 >= n {
                return;
            }
            let first = b + 1;
            let last = (first + 1..n - 1)
                .take_while(|&j| self.outline_hidden(j))
                .last()
                .unwrap_or(first);
            first..=last
        } else {
            let Some(first) = (0..a).rev().find(|&j| !self.outline_hidden(j)) else {
                return;
            };
            first..=a - 1
        };
        let paras = &self.doc.main.paras;
        if paras[*other.start().min(&a)..=*other.end().max(&b)]
            .iter()
            .any(|p| p.pap.in_table || p.mark == Mark::SectionBreak)
        {
            return;
        }
        let (first, last) = if down {
            (a, *other.end())
        } else {
            (*other.start(), b)
        };
        let block: Vec<Paragraph> = paras[a..=b].to_vec();
        let rest: Vec<Paragraph> = paras[other.clone()].to_vec();
        let hidden = |r: std::ops::RangeInclusive<usize>| -> Vec<bool> {
            r.map(|j| self.collapsed.get(j).copied().unwrap_or(false))
                .collect()
        };
        let (hb, hr) = (hidden(a..=b), hidden(other.clone()));
        let new: Vec<Paragraph>;
        let flags: Vec<bool>;
        if down {
            new = rest.into_iter().chain(block).collect();
            flags = hr.into_iter().chain(hb).collect();
        } else {
            new = block.into_iter().chain(rest).collect();
            flags = hb.into_iter().chain(hr).collect();
        }
        let shift = if down {
            (*other.end() - *other.start() + 1) as isize
        } else {
            -((*other.end() - *other.start() + 1) as isize)
        };
        let sel = self.sel;
        self.doc
            .begin(if down { "Move Down" } else { "Move Up" }, sel);
        self.doc.replace_paragraphs(MAIN, first, last, new);
        let moved = |p: Position| Position::new((p.para as isize + shift) as usize, p.offset);
        let after = Selection::new(moved(sel.anchor), moved(sel.focus));
        self.doc.commit(after);
        self.collapse_after = Some((first, flags));
        self.set_selection(after);
    }

    /// + Expand: show the next level hidden under each selected heading.
    pub fn expand(&mut self) {
        self.reveal(false);
    }

    /// − Collapse: hide the lowest level showing under each selected
    /// heading.
    pub fn collapse(&mut self) {
        self.collapsed.resize(self.doc.main.paras.len(), false);
        let (s, e) = self.sel.range();
        for i in s.para..=e.para {
            if self.outline_hidden(i) {
                continue;
            }
            let sub = self.subtext(i);
            let lowest = sub
                .clone()
                .filter(|&j| !self.outline_hidden(j))
                .map(|j| self.level(j))
                .max();
            if let Some(m) = lowest {
                for j in sub {
                    if self.level(j) == m {
                        self.collapsed[j] = true;
                    }
                }
            }
        }
        self.after_outline_change();
    }

    /// Show what is hidden under the selected headings: the next level, or
    /// everything.
    fn reveal(&mut self, all: bool) {
        self.collapsed.resize(self.doc.main.paras.len(), false);
        let (s, e) = self.sel.range();
        for i in s.para..=e.para {
            if self.outline_hidden(i) {
                continue;
            }
            let sub = self.subtext(i);
            let next = sub
                .clone()
                .filter(|&j| self.outline_hidden(j))
                .map(|j| self.level(j))
                .min();
            if let Some(m) = next {
                for j in sub {
                    if all || self.level(j) <= m {
                        self.collapsed[j] = false;
                    }
                }
            }
        }
        self.after_outline_change();
    }

    /// Show headings down to `level` and hide the rest; 0 shows all.
    pub fn show_level(&mut self, level: u8) {
        self.collapsed.resize(self.doc.main.paras.len(), false);
        for i in 0..self.collapsed.len() {
            self.collapsed[i] = level > 0 && self.level(i) > level;
        }
        self.after_outline_change();
    }

    /// A double-click on a heading's symbol: open everything under it, or
    /// close it if it is all showing.
    pub(super) fn toggle_subtext(&mut self, i: usize) {
        self.collapsed.resize(self.doc.main.paras.len(), false);
        let sub = self.subtext(i);
        if sub.is_empty() {
            return;
        }
        let any_hidden = sub.clone().any(|j| self.outline_hidden(j));
        for j in sub {
            self.collapsed[j] = !any_hidden;
        }
        self.after_outline_change();
    }

    /// Keep the insertion point out of hidden text, and lay out again.
    fn after_outline_change(&mut self) {
        let fix = |v: &Self, p: Position| {
            if !v.outline_hidden(p.para) {
                return p;
            }
            let up = (0..p.para)
                .rev()
                .find(|&j| !v.outline_hidden(j))
                .unwrap_or(0);
            Position::new(up, v.doc.main.paras[up].len())
        };
        let a = fix(self, self.sel.anchor);
        let f = fix(self, self.sel.focus);
        self.sel = Selection::new(a, f);
        self.outline_dirty = true;
    }

    /// Switch Outline view on or off.
    pub fn set_outline(&mut self, on: bool) {
        if self.outline == on {
            return;
        }
        self.outline = on;
        if on {
            self.page_view = false;
            // An empty document starts its outline with a heading.
            let paras = &self.doc.main.paras;
            if paras.len() == 1
                && paras[0].is_empty()
                && self.level(0) == BODY
                && let Some(h) = self.doc.styles.find("heading 1")
            {
                let sel = self.sel;
                self.doc.begin("Outline", sel);
                self.doc.apply_style(MAIN, 0, 0, h);
                self.doc.commit(sel);
            }
        }
        self.outline_dirty = true;
        self.invalidate_layout();
    }

    /// The paragraph whose symbol is under document point (x, y).
    pub(super) fn symbol_at(&self, x: f32, y: f32) -> Option<usize> {
        if !self.outline {
            return None;
        }
        let b = &self.blocks[self.block_at(y)];
        if b.row.is_some() {
            return None;
        }
        let i = b.first;
        let (sx, _) = self.outline_x(i);
        let w = if self.level(i) == BODY { 4.0 } else { 8.0 };
        (x >= sx - 3.0 && x <= sx + w + 3.0).then_some(i)
    }

    /// A click on a symbol selects the paragraph and everything under it;
    /// a double-click opens or closes what is under it.
    pub(super) fn symbol_click(&mut self, i: usize, double: bool) {
        if double {
            self.toggle_subtext(i);
            return;
        }
        let end = self.subtext(i).end;
        let n = self.doc.main.paras.len();
        let to = if end < n {
            Position::new(end, 0)
        } else {
            self.doc.main.end()
        };
        self.set_selection(Selection::new(Position::new(i, 0), to));
    }

    /// Draw paragraph `i`'s symbol, and the gray line under a heading
    /// whose text is collapsed, with its first line's baseline at `base`.
    pub(super) fn draw_symbol(
        &self,
        c: &mut dyn Surface,
        i: usize,
        ox: f32,
        base: f32,
        text_end: f32,
    ) {
        let (sx, tx) = self.outline_x(i);
        let x = (ox + sx).round();
        let b = base.round();
        let px = |c: &mut dyn Surface, dx: f32, dy: f32, w: f32, h: f32| {
            c.fill(Rect::new(x + dx, b + dy, w, h), color::BLACK);
        };
        let level = self.level(i);
        let sub = self.subtext(i);
        if level == BODY {
            // A 4 x 4 hollow square ending a point above the baseline.
            px(c, 0.0, -5.0, 4.0, 1.0);
            px(c, 0.0, -2.0, 4.0, 1.0);
            px(c, 0.0, -4.0, 1.0, 2.0);
            px(c, 3.0, -4.0, 1.0, 2.0);
        } else if sub.is_empty() {
            // A hollow bar sitting on the baseline.
            px(c, 0.0, -4.0, 8.0, 1.0);
            px(c, 0.0, -1.0, 8.0, 1.0);
            px(c, 0.0, -3.0, 1.0, 2.0);
            px(c, 7.0, -3.0, 1.0, 2.0);
        } else {
            // A hollow plus, 8 x 8, reaching 2 points below the baseline.
            const PLUS: [&str; 8] = [
                "..####..", "..#..#..", "###..###", "#......#", "#......#", "###..###", "..#..#..",
                "..####..",
            ];
            for (r, row) in PLUS.iter().enumerate() {
                for (k, ch) in row.chars().enumerate() {
                    if ch == '#' {
                        px(c, k as f32, r as f32 - 5.0, 1.0, 1.0);
                    }
                }
            }
            // Collapsed: what comes straight under it is hidden.
            if self.outline_hidden(sub.start) {
                // A 50% gray line, 2 points tall, under the text.
                let mut gx = (ox + tx).round();
                while gx < ox + text_end + 8.0 {
                    c.fill(Rect::new(gx, b + 2.0, 1.0, 1.0), color::BLACK);
                    c.fill(Rect::new(gx + 2.0, b + 3.0, 1.0, 1.0), color::BLACK);
                    gx += 4.0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Cmd;

    /// An outline: Chapter One > Section A > body, Section B, Chapter Two.
    fn outline() -> (DocView, Clipboard) {
        let mut v = DocView::new(quill_core::Document::new(), "T".into());
        let mut clip = Clipboard::new();
        v.set_outline(true);
        let mut step = |v: &mut DocView, cmd: Option<Cmd>, text: &str| {
            if let Some(c) = cmd {
                v.exec(c, false, &mut clip);
            }
            v.type_text(text);
        };
        step(&mut v, None, "Chapter One");
        step(&mut v, Some(Cmd::NewParagraph), "Section A");
        v.demote();
        step(&mut v, Some(Cmd::NewParagraph), "Body");
        v.demote_to_body();
        step(&mut v, Some(Cmd::NewParagraph), "Section B");
        v.promote();
        step(&mut v, Some(Cmd::NewParagraph), "Chapter Two");
        v.promote();
        (v, clip)
    }

    fn levels(v: &DocView) -> Vec<u8> {
        (0..v.doc.main.paras.len()).map(|i| v.level(i)).collect()
    }

    #[test]
    fn levels_come_from_promote_and_demote() {
        let (v, _) = outline();
        assert_eq!(levels(&v), [1, 2, 10, 2, 1]);
        assert_eq!(v.subtext(0), 1..4);
        assert_eq!(v.subtext(1), 2..3);
    }

    #[test]
    fn collapse_hides_the_lowest_level_first() {
        let (mut v, _) = outline();
        v.collapsed.resize(5, false);
        v.set_selection(Selection::caret(Position::new(0, 0)));
        v.collapse();
        assert_eq!(v.collapsed, [false, false, true, false, false]);
        v.collapse();
        assert_eq!(v.collapsed, [false, true, true, true, false]);
        v.expand();
        assert_eq!(v.collapsed, [false, false, true, false, false]);
        v.show_level(0);
        assert!(v.collapsed.iter().all(|c| !c));
    }

    #[test]
    fn moving_a_collapsed_heading_takes_its_text() {
        let (mut v, _) = outline();
        v.collapsed.resize(5, false);
        // Section A with its body hidden moves below Section B.
        v.set_selection(Selection::caret(Position::new(1, 0)));
        v.collapse();
        v.move_paragraphs(true);
        let texts: Vec<&str> = v.doc.main.paras.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "Chapter One",
                "Section B",
                "Section A",
                "Body",
                "Chapter Two"
            ]
        );
        assert_eq!(v.sel.focus.para, 2);
        v.doc.undo();
        let texts: Vec<&str> = v.doc.main.paras.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "Chapter One",
                "Section A",
                "Body",
                "Section B",
                "Chapter Two"
            ]
        );
    }
}
