//! The comments pane: each comment is its own box. A box holds one comment's
//! text and nothing else: the selection cannot leave it, so a drag or a
//! select-all can never take in a second comment, and its icon is drawn
//! beside the box instead of being a character in the text, so it cannot be
//! selected, copied or deleted. Dotted lines join each box to the text it is
//! about.

use super::*;
use crate::canvas::round_rect;

/// Size of a comment's text in the pane, in points at the window's own
/// scale; the document's zoom does not change it.
pub(super) const COMMENT_SIZE: f32 = 10.0;
/// Room left between one comment's box and the next, in points.
pub(super) const CARD_GAP: f32 = 12.0;
/// How far a box reaches above and below its text.
const CARD_PAD: f32 = 3.0;
/// Space between the pane's edge and a box.
const CARD_MARGIN: f32 = 6.0;
const NOTE: u32 = 0xF6D26B;
const NOTE_EDGE: u32 = 0xC9A227;
const CARD_EDGE: u32 = 0xD4D4D4;
const CARD_HERE: u32 = 0x0A84FF;
pub(super) const PANE_BACKGROUND: u32 = 0xF5F5F5;

/// One comment's box, in window points.
#[derive(Clone, Copy, Debug)]
pub struct Card {
    pub id: u32,
    pub rect: Rect,
}

impl DocView {
    /// Set how large the comments pane's text is drawn, laying it out again
    /// when that changes.
    pub fn set_comment_scale(&mut self, scale: f32) {
        if (self.comment_scale - scale).abs() > 0.001 {
            self.comment_scale = scale;
            self.invalidate_layout();
        }
    }

    /// The first and last paragraph of the comment that paragraph `para` is
    /// in: it starts at a paragraph beginning with the mark and runs to the
    /// paragraph before the next mark.
    fn comment_span(&self, para: usize) -> (usize, usize) {
        let paras = &self.doc.main.paras;
        let para = para.min(paras.len().saturating_sub(1));
        let first = (0..=para)
            .rev()
            .find(|&i| paras[i].text.starts_with(chars::ANNOTATION))
            .unwrap_or(0);
        let last = (para + 1..paras.len())
            .find(|&i| paras[i].text.starts_with(chars::ANNOTATION))
            .map_or(paras.len().saturating_sub(1), |next| next - 1);
        (first, last)
    }

    /// The text of the comment holding paragraph `para`: from just after its
    /// mark to the end of its last paragraph. Nothing outside can be
    /// selected or edited from the pane.
    pub(super) fn comment_body(&self, para: usize) -> (Position, Position) {
        let paras = &self.doc.main.paras;
        let (first, last) = self.comment_span(para);
        // After the mark and the space that follows it.
        let mut start = 0;
        if paras[first].text.starts_with(chars::ANNOTATION) {
            start = chars::ANNOTATION.len_utf8();
            if paras[first].text[start..].starts_with(' ') {
                start += 1;
            }
        }
        (
            Position::new(first, start),
            Position::new(last, paras[last].text.len()),
        )
    }

    /// Keep the selection inside the comment it began in.
    pub(super) fn confine_to_comment(&mut self) {
        if !self.captions || self.doc.main.paras.is_empty() {
            return;
        }
        let (start, end) = self.comment_body(self.sel.anchor.para);
        let clamp = |p: Position| p.max(start).min(end);
        let sel = Selection::new(clamp(self.sel.anchor), clamp(self.sel.focus));
        if sel != self.sel {
            self.sel = sel;
            self.goal_x = None;
        }
    }

    /// What a command does in the comments pane: a delete key from a
    /// collapsed caret deletes up to the edge of the comment and no further,
    /// and does nothing at the edge, so comments never join. None means do
    /// nothing; otherwise the command to run.
    pub(super) fn contain_delete(&mut self, cmd: Cmd) -> Option<Cmd> {
        use Cmd::*;
        if !self.captions
            || !self.sel.is_empty()
            || !matches!(
                cmd,
                Backspace | DeleteForward | DeletePrevWord | DeleteNextWord
            )
        {
            return Some(cmd);
        }
        let at = self.sel.focus;
        let (start, end) = self.comment_body(at.para);
        let story = &self.doc.main;
        let (s, e) = match cmd {
            Backspace => (story.prev_char(at), at),
            DeletePrevWord => (story.prev_word(at), at),
            DeleteForward => (at, story.next_char(at)),
            _ => (at, story.next_word(at)),
        };
        let (s, e) = (s.max(start), e.min(end));
        if s >= e {
            return None;
        }
        self.sel = Selection::new(s, e);
        Some(Backspace)
    }

    /// The comments the pane holds, in order: a paste or anything else that
    /// adds, removes or reorders them has gone wrong.
    pub fn comment_ids(&self) -> Vec<u32> {
        self.doc
            .main
            .paras
            .iter()
            .filter(|p| p.text.starts_with(chars::ANNOTATION))
            .map(|p| p.chp_of_char(0).object)
            .collect()
    }

    /// The comment boxes, in window points, for painting and for the lines
    /// to the text they are about.
    pub fn comment_cards(&self) -> Vec<Card> {
        if !self.captions {
            return Vec::new();
        }
        let paras = &self.doc.main.paras;
        let mut out = Vec::new();
        let mut first = None;
        for i in 0..=paras.len() {
            let starts = paras
                .get(i)
                .is_some_and(|p| p.text.starts_with(chars::ANNOTATION));
            if i == paras.len() || starts {
                if let Some(a) = first.take()
                    && let Some(card) = self.card_rect(a, i - 1)
                {
                    out.push(card);
                }
                if starts {
                    first = Some(i);
                }
            }
        }
        out
    }

    /// A box's top and bottom in document points: its text's first and last
    /// line, and a little round them.
    fn card_extent(&self, first: usize, last: usize) -> Option<(f32, f32)> {
        let a = self.layouts.get(first)?.as_ref()?;
        let b = self.layouts.get(last)?.as_ref()?;
        let top = self.tops.get(first)? + a.lines.first()?.top;
        let end = b.lines.last()?;
        let bottom = self.tops.get(last)? + end.top + end.height;
        let pad = CARD_PAD * self.comment_scale;
        Some((top - pad, bottom + pad))
    }

    fn card_rect(&self, first: usize, last: usize) -> Option<Card> {
        let id = self.doc.main.paras[first].chp_of_char(0).object;
        let (top, bottom) = self.card_extent(first, last)?;
        let (_, y) = self.to_window(0.0, top);
        let (_, y2) = self.to_window(0.0, bottom);
        let x = self.area.x + CARD_MARGIN;
        let w = (self.area.w - 2.0 * CARD_MARGIN).max(0.0);
        Some(Card {
            id,
            rect: Rect::new(x, y, w, y2 - y),
        })
    }

    /// The pane's comments in order, with where each one's icon sits, in
    /// document points.
    pub fn comment_tops(&self) -> Vec<(u32, f32)> {
        if !self.captions {
            return Vec::new();
        }
        let paras = &self.doc.main.paras;
        let mut out = Vec::new();
        for (i, p) in paras.iter().enumerate() {
            if p.text.starts_with(chars::ANNOTATION) {
                let (_, last) = self.comment_span(i);
                if let Some((top, _)) = self.card_extent(i, last) {
                    out.push((p.chp_of_char(0).object, top + self.icon_inset()));
                }
            }
        }
        out
    }

    /// How far below the box's top its icon, and so its dotted line, sits.
    fn icon_inset(&self) -> f32 {
        3.0 * self.comment_scale
    }

    /// Paint each comment's box behind its text, and its icon on the box's
    /// top left corner.
    pub(super) fn draw_comment_cards(&self, c: &mut Canvas, focused: bool) {
        let here = self.annotation_here().filter(|_| focused);
        let s = self.comment_scale;
        for card in self.comment_cards() {
            let r = card.rect;
            if r.bottom() < self.area.y - 8.0 || r.y > self.area.bottom() + 8.0 {
                continue;
            }
            let edge = if here == Some(card.id) {
                CARD_HERE
            } else {
                CARD_EDGE
            };
            round_rect(c, r, 6.0 * s, edge);
            round_rect(c, r.inset(s), 5.0 * s, color::WHITE);
            draw_comment_icon(c, r.x - 5.0 * s, r.y - 1.0 * s, s);
        }
    }

    /// Where the comment marks are in this text: each one's id and place.
    pub fn comment_marks(&self) -> Vec<(u32, Position)> {
        let mut out = Vec::new();
        for (i, p) in self.doc.main.paras.iter().enumerate() {
            if !p.text.contains(chars::ANNOTATION) {
                continue;
            }
            for (at, c) in p.text.char_indices() {
                if c == chars::ANNOTATION {
                    out.push((p.chp_of_char(at).object, Position::new(i, at)));
                }
            }
        }
        out
    }

    /// Where a comment's mark is, the end of the text it is about: its
    /// document y at the middle of the line, and its window point if that
    /// is in view.
    pub fn mark_point(&self, at: Position) -> Option<(f32, (f32, f32))> {
        self.layouts.get(at.para)?.as_ref()?;
        let (x, y, h) = self.caret_geometry(at, false);
        let (wx, wy) = self.to_window(x, y + h / 2.0);
        Some((y + h / 2.0, (wx, wy)))
    }

    /// Whether window point `p` is inside this view's area.
    pub fn shows(&self, p: (f32, f32)) -> bool {
        p.0 >= self.area.x
            && p.0 <= self.area.right()
            && p.1 >= self.area.y
            && p.1 <= self.area.bottom()
    }

    /// Scroll so that document y `y` is at window y `wy`.
    pub fn scroll_to_align(&mut self, y: f32, wy: f32) {
        self.scroll_y = (self.area.y + self.to_view(y) - wy).clamp(0.0, self.max_scroll());
    }

    /// Window y of document y `y`.
    pub fn window_y(&self, y: f32) -> f32 {
        self.to_window(0.0, y).1
    }
}

/// A small speech bubble with its tail at the lower left, 10 x 8 points
/// at scale 1, its top left corner at (`x`, `y`).
pub(super) fn draw_comment_icon(c: &mut Canvas, x: f32, y: f32, s: f32) {
    round_rect(c, Rect::new(x, y, 10.0 * s, 8.0 * s), 2.0 * s, NOTE_EDGE);
    round_rect(
        c,
        Rect::new(x + 0.5 * s, y + 0.5 * s, 9.0 * s, 7.0 * s),
        1.5 * s,
        NOTE,
    );
    c.fill(Rect::new(x + 2.0 * s, y + 8.0 * s, 2.0 * s, s), NOTE_EDGE);
    c.fill(Rect::new(x + 2.0 * s, y + 7.5 * s, 1.5 * s, s), NOTE);
    c.fill(Rect::new(x + 2.0 * s, y + 9.0 * s, s, s), NOTE_EDGE);
}

/// A dotted line from (`x1`, `y1`) to (`x2`, `y2`).
pub fn dotted_line(c: &mut Canvas, x1: f32, y1: f32, x2: f32, y2: f32, color: u32) {
    const STEP: f32 = 3.0;
    let (dx, dy) = (x2 - x1, y2 - y1);
    let len = (dx * dx + dy * dy).sqrt();
    let n = (len / STEP).floor() as usize;
    let dot = c.hair().max(1.0);
    for i in 0..=n {
        let t = if n == 0 { 0.0 } else { i as f32 / n as f32 };
        c.fill(Rect::new(x1 + dx * t, y1 + dy * t, dot, dot), color);
    }
}

/// Color of the dotted lines from comments to their text.
pub const LINK: u32 = NOTE_EDGE;

#[cfg(test)]
mod tests {
    use super::*;

    /// A comments pane holding two comments, the first of two paragraphs.
    fn pane() -> DocView {
        let text = format!(
            "{m} First one\nand more\n{m} Second one",
            m = chars::ANNOTATION
        );
        let mut v = DocView::new(quill_core::text::from_plain_text(&text), "Comments".into());
        v.captions = true;
        v
    }

    fn at(para: usize, offset: usize) -> Position {
        Position::new(para, offset)
    }

    fn texts(v: &DocView) -> Vec<String> {
        v.doc.main.paras.iter().map(|p| p.text.clone()).collect()
    }

    #[test]
    fn a_selection_cannot_leave_its_comment_or_take_the_icon() {
        let mut v = pane();
        let m = chars::ANNOTATION.len_utf8();
        // A drag from the first comment into the second stops at the first's end.
        v.set_selection(Selection::new(at(0, m + 3), at(2, 6)));
        assert_eq!(v.sel, Selection::new(at(0, m + 3), at(1, "and more".len())));
        // A selection reaching back over the mark starts after it and its space.
        v.set_selection(Selection::new(at(0, 0), at(0, m + 5)));
        assert_eq!(v.sel.start(), at(0, m + 1));
        // Select All takes one comment's text, not the pane.
        let mut clip = Clipboard::new();
        v.set_selection(Selection::caret(at(2, m + 4)));
        v.exec(Cmd::SelectAll, false, &mut clip);
        assert_eq!(
            v.sel,
            Selection::new(at(2, m + 1), at(2, texts(&v)[2].len()))
        );
    }

    #[test]
    fn deleting_never_joins_comments_or_removes_a_mark() {
        let mut v = pane();
        let m = chars::ANNOTATION.len_utf8();
        let before = texts(&v);
        let mut clip = Clipboard::new();
        // Backspace at the start of a comment's text, and Delete at the end of
        // its last paragraph, do nothing.
        for (cmd, p) in [
            (Cmd::Backspace, at(0, m + 1)),
            (Cmd::DeletePrevWord, at(0, m + 1)),
            (Cmd::Backspace, at(2, m + 1)),
            (Cmd::DeleteForward, at(1, "and more".len())),
            (Cmd::DeleteNextWord, at(1, "and more".len())),
        ] {
            v.set_selection(Selection::caret(p));
            v.exec(cmd, false, &mut clip);
            assert_eq!(texts(&v), before, "{cmd:?}");
        }
        // Deleting a word stops at the edge, and ordinary keys still work.
        v.set_selection(Selection::caret(at(0, m + 4)));
        v.exec(Cmd::DeletePrevWord, false, &mut clip);
        assert_eq!(texts(&v)[0], format!("{} st one", chars::ANNOTATION));
        v.set_selection(Selection::caret(at(1, 3)));
        v.exec(Cmd::Backspace, false, &mut clip);
        assert_eq!(texts(&v)[1], "an more");
        assert_eq!(v.comment_ids().len(), 2);
    }
}
