//! A scrolling list in a dialog: rows in the platform's dialog face, the
//! chosen row highlighted, and a scroll bar down the right.

use crate::canvas::{Canvas, Label, Rect, color, round_rect};

pub const ROW: f32 = 16.0;
const BAR: f32 = 15.0;

/// One row as drawn.
pub struct Row<'a> {
    pub text: &'a str,
    /// A mark drawn before the text (the original's bullet for standard entries).
    pub mark: &'a str,
    pub bold: bool,
    pub underline: bool,
}

/// Which row is chosen and how far the list is scrolled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListBox {
    pub selected: usize,
    pub scroll: usize,
    pub len: usize,
}

/// What a click in the list did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    None,
    Scrolled,
    Chose(usize),
}

impl ListBox {
    fn visible(r: Rect) -> usize {
        ((r.h - 2.0) / ROW).floor().max(1.0) as usize
    }

    /// Choose row `i` and scroll it into view.
    pub fn select(&mut self, i: usize, r: Rect) {
        self.selected = i.min(self.len.saturating_sub(1));
        let rows = Self::visible(r);
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + rows {
            self.scroll = self.selected + 1 - rows;
        }
    }

    /// A click at `(x, y)` relative to the list's top left.
    pub fn click(&mut self, x: f32, y: f32, r: Rect) -> Click {
        let rows = Self::visible(r);
        if x >= r.w - BAR && self.len > rows {
            let max = self.len.saturating_sub(rows);
            let fraction = (y / r.h).clamp(0.0, 1.0);
            self.scroll = (fraction * max as f32).round() as usize;
            return Click::Scrolled;
        }
        let k = self.scroll + ((y - 1.0).max(0.0) / ROW) as usize;
        if k < self.len {
            self.selected = k;
            Click::Chose(k)
        } else {
            Click::None
        }
    }

    pub fn draw(&self, c: &mut Canvas, label: &mut Label, r: Rect, rows: &[Row]) {
        let (font, size, bold) = (quill_layout::fonts::UI_FONT, 12.0, false);
        round_rect(c, r, 5.0, 0xA8A8A8);
        let hair = c.hair();
        round_rect(c, r.inset(hair), 4.5, color::WHITE);
        let area = Rect::new(r.x, r.y, r.w - BAR, r.h);
        let clip = c.set_clip(area.inset(1.0));
        for (k, row) in rows.iter().enumerate().skip(self.scroll) {
            let y = area.y + 1.0 + ROW * (k - self.scroll) as f32;
            if y > area.bottom() {
                break;
            }
            if k == self.selected {
                c.fill(
                    Rect::new(area.x + 1.0, y, area.w - 2.0, ROW),
                    color::HIGHLIGHT,
                );
            }
            let base = y + 12.5;
            let mut x = area.x + 6.0;
            if !row.mark.is_empty() {
                label.draw(c, font, size, bold, x, base, row.mark, color::BLACK);
                x += label.width(font, size, bold, row.mark);
            }
            // Bold rows are drawn indented, like the original's "New".
            if row.bold {
                x = area.x + 20.0;
            }
            label.draw(c, font, size, bold, x, base, row.text, color::BLACK);
            if row.underline {
                let w = label.width(font, size, bold, row.text);
                c.fill(Rect::new(x, base + 1.5, w, 1.0), color::BLACK);
            }
        }
        c.restore_clip(clip);
        // Flat proportional thumb; track clicks use the same full-height range.
        let visible = Self::visible(r);
        if self.len > visible {
            let track = (r.h - 8.0).max(1.0);
            let thumb_h = (track * visible as f32 / self.len as f32)
                .clamp(1.0, track)
                .max(24.0_f32.min(track));
            let max_scroll = self.len.saturating_sub(visible).max(1) as f32;
            let thumb_y = r.y + 4.0 + (track - thumb_h) * self.scroll as f32 / max_scroll;
            round_rect(
                c,
                Rect::new(r.right() - 9.0, thumb_y, 5.0, thumb_h),
                2.5,
                0x8A8A8A,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_choose_rows_and_scroll() {
        let r = Rect::new(0.0, 0.0, 242.0, 114.0);
        let mut l = ListBox {
            len: 20,
            ..Default::default()
        };
        assert_eq!(l.click(50.0, 20.0, r), Click::Chose(1));
        assert_eq!(l.click(235.0, 110.0, r), Click::Scrolled);
        assert!(l.scroll > 0);
        l.select(19, r);
        assert_eq!(l.scroll, 13);
        assert_eq!(l.click(50.0, 2.0, r), Click::Chose(13));
    }
}
