//! Flat fallback scroll controls and status bar. Native host controls take precedence.

use crate::canvas::{Canvas, Label, Rect, color, round_rect};

/// Width of a scroll bar and height of the status bar, in points.
pub const BAR: f32 = 16.0;

/// Parts of a scroll bar a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollPart {
    LineUp,
    LineDown,
    PageUp,
    PageDown,
    Thumb,
}

/// The thumb hit target of a shared scroll control. `frac` is 0 at the top.
pub fn thumb_rect(r: Rect, frac: f32, vertical: bool) -> Rect {
    if vertical {
        let span = (r.h - 3.0 * BAR).max(0.0);
        Rect::new(r.x, r.y + BAR + span * frac.clamp(0.0, 1.0), BAR, BAR)
    } else {
        let span = (r.w - 3.0 * BAR).max(0.0);
        Rect::new(r.x + BAR + span * frac.clamp(0.0, 1.0), r.y, BAR, BAR)
    }
}

pub fn scroll_hit(r: Rect, frac: f32, vertical: bool, x: f32, y: f32) -> Option<ScrollPart> {
    if !r.contains(x, y) {
        return None;
    }
    let t = thumb_rect(r, frac, vertical);
    let (pos, start, end, thumb) = if vertical {
        (y, r.y, r.bottom(), t.y)
    } else {
        (x, r.x, r.right(), t.x)
    };
    Some(if pos < start + BAR {
        ScrollPart::LineUp
    } else if pos >= end - BAR {
        ScrollPart::LineDown
    } else if t.contains(x, y) {
        ScrollPart::Thumb
    } else if pos < thumb {
        ScrollPart::PageUp
    } else {
        ScrollPart::PageDown
    })
}

/// A small solid triangle pointing up, down, left or right.
fn arrow(c: &mut Canvas, cx: f32, cy: f32, dir: u8, col: u32) {
    for i in 0..4 {
        let i = i as f32;
        match dir {
            0 => c.fill(Rect::new(cx - i, cy - 2.0 + i, 2.0 * i + 1.0, 1.0), col),
            1 => c.fill(Rect::new(cx - i, cy + 2.0 - i, 2.0 * i + 1.0, 1.0), col),
            2 => c.fill(Rect::new(cx - 2.0 + i, cy - i, 1.0, 2.0 * i + 1.0), col),
            _ => c.fill(Rect::new(cx + 2.0 - i, cy - i, 1.0, 2.0 * i + 1.0), col),
        }
    }
}

pub fn draw_scroll(
    c: &mut Canvas,
    r: Rect,
    frac: f32,
    vertical: bool,
    enabled: bool,
    pressed: Option<ScrollPart>,
) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    c.fill(r, TRACK);
    let (a, b) = if vertical {
        (
            Rect::new(r.x, r.y, BAR, BAR),
            Rect::new(r.x, r.bottom() - BAR, BAR, BAR),
        )
    } else {
        (
            Rect::new(r.x, r.y, BAR, BAR),
            Rect::new(r.right() - BAR, r.y, BAR, BAR),
        )
    };
    let arrow_col = if enabled {
        color::BLACK
    } else {
        color::GRAY_DARK
    };
    for (box_, part, dir) in [
        (a, ScrollPart::LineUp, if vertical { 0 } else { 2 }),
        (b, ScrollPart::LineDown, if vertical { 1 } else { 3 }),
    ] {
        if pressed == Some(part) {
            c.fill(box_, 0xDDDDDD);
        }
        arrow(
            c,
            box_.x + BAR / 2.0 - 0.5,
            box_.y + BAR / 2.0 - 0.5,
            dir,
            arrow_col,
        );
    }
    if enabled {
        let t = thumb_rect(r, frac, vertical);
        let thumb = if vertical {
            Rect::new(t.x + 4.0, t.y + 1.0, t.w - 8.0, t.h - 2.0)
        } else {
            Rect::new(t.x + 1.0, t.y + 4.0, t.w - 2.0, t.h - 8.0)
        };
        round_rect(
            c,
            thumb,
            4.0,
            if pressed == Some(ScrollPart::Thumb) {
                0x666666
            } else {
                0x929292
            },
        );
    }
}

/// The flat look of the status bar, the corner between the scroll bars
/// and the split furniture: a light fill and a hairline edge.
pub const TRACK: u32 = 0xF2F2F2;
pub const EDGE: u32 = 0xD0D0D0;

/// The bar under the text: page number box, style name box, then the
/// horizontal scroll bar, and the grow box in the corner. A `message` takes
/// the place of the boxes and scroll bar, so its explanation stays visible.
pub fn draw_status(
    c: &mut Canvas,
    label: &mut Label,
    r: Rect,
    page: &str,
    style: &str,
    message: Option<&str>,
) -> Rect {
    let page_w = 90.0;
    let style_w = 120.0;
    c.fill(r, TRACK);
    let h = c.hair();
    c.fill(Rect::new(r.x, r.y, r.w, h), EDGE);
    let base = r.y + 12.0;
    let ink = crate::icon::INK;
    label.draw(
        c,
        quill_layout::fonts::UI_FONT,
        10.0,
        false,
        r.x + 6.0,
        base,
        message.unwrap_or(page),
        ink,
    );
    if message.is_none() {
        c.fill(Rect::new(r.x + page_w, r.y + 3.0, h, r.h - 6.0), EDGE);
        label.draw(
            c,
            quill_layout::fonts::UI_FONT,
            10.0,
            false,
            r.x + page_w + 6.0,
            base,
            style,
            ink,
        );
        c.fill(
            Rect::new(r.x + page_w + style_w, r.y + 3.0, h, r.h - 6.0),
            EDGE,
        );
    }
    if message.is_some() {
        Rect::new(r.right(), r.y, 0.0, BAR)
    } else {
        Rect::new(r.x + page_w + style_w, r.y, r.w - page_w - style_w, BAR)
    }
}

/// A tooltip with `text`, below and right of the pointer at (`x`, `y`),
/// kept inside a `w` x `h` window.
pub fn draw_tip(c: &mut Canvas, label: &mut Label, text: &str, x: f32, y: f32, w: f32, h: f32) {
    let size = 11.0;
    let font = quill_layout::fonts::UI_FONT;
    let max_width = (w - 14.0).clamp(1.0, 410.0);
    let mut lines = Vec::new();
    let mut line = String::new();
    for ch in text.chars() {
        if ch == '\n' {
            lines.push(std::mem::take(&mut line));
        } else {
            line.push(ch);
            if label.width(font, size, false, &line) > max_width && line.chars().count() > 1 {
                line.pop();
                lines.push(std::mem::take(&mut line));
                line.push(ch);
            }
        }
    }
    lines.push(line);
    let max_lines = (((h - 12.0) / 14.0) as usize).max(1);
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        lines.last_mut().unwrap().push('…');
    }
    let tw = lines
        .iter()
        .map(|line| label.width(font, size, false, line))
        .fold(0.0, f32::max);
    let (bw, bh) = ((tw + 10.0).min(w - 4.0), 4.0 + 14.0 * lines.len() as f32);
    let bx = x.min(w - bw - 2.0).max(2.0);
    // Below the pointer, or above it near the bottom of the window.
    let by = if y + 20.0 + bh < h {
        y + 20.0
    } else {
        y - bh - 4.0
    }
    .clamp(2.0, (h - bh - 2.0).max(2.0));
    let r = Rect::new(bx, by, bw, bh);
    round_rect(c, r, 4.0, 0xB8B8B8);
    let hair = c.hair();
    round_rect(c, r.inset(hair), 3.0, 0xF5F5F5);
    for (i, line) in lines.iter().enumerate() {
        label.draw(
            c,
            quill_layout::fonts::UI_FONT,
            size,
            false,
            bx + 5.0,
            by + 13.0 + i as f32 * 14.0,
            line,
            color::BLACK,
        );
    }
}

/// Neutral corner surface; resizing is handled by the system.
pub fn draw_grow_box(c: &mut Canvas, r: Rect) {
    c.fill(r, TRACK);
    let h = c.hair();
    c.fill(Rect::new(r.x, r.y, h, r.h), EDGE);
    c.fill(Rect::new(r.x, r.y, r.w, h), EDGE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_thumb_moves_between_arrow_buttons_and_clamps() {
        let bar = Rect::new(10.0, 20.0, BAR, 100.0);
        assert_eq!(thumb_rect(bar, -1.0, true).y, 36.0);
        assert_eq!(thumb_rect(bar, 0.0, true).y, 36.0);
        assert_eq!(thumb_rect(bar, 1.0, true).y, 88.0);
        assert_eq!(thumb_rect(bar, 2.0, true).y, 88.0);
    }

    #[test]
    fn horizontal_thumb_moves_between_arrow_buttons_and_clamps() {
        let bar = Rect::new(10.0, 20.0, 100.0, BAR);
        assert_eq!(thumb_rect(bar, -1.0, false).x, 26.0);
        assert_eq!(thumb_rect(bar, 0.0, false).x, 26.0);
        assert_eq!(thumb_rect(bar, 1.0, false).x, 78.0);
        assert_eq!(thumb_rect(bar, 2.0, false).x, 78.0);
    }

    #[test]
    fn vertical_scroll_hit_distinguishes_every_part() {
        let bar = Rect::new(10.0, 20.0, BAR, 100.0);
        let x = bar.x + BAR / 2.0;
        assert_eq!(
            scroll_hit(bar, 0.5, true, x, 25.0),
            Some(ScrollPart::LineUp)
        );
        assert_eq!(
            scroll_hit(bar, 0.5, true, x, 45.0),
            Some(ScrollPart::PageUp)
        );
        assert_eq!(scroll_hit(bar, 0.5, true, x, 68.0), Some(ScrollPart::Thumb));
        assert_eq!(
            scroll_hit(bar, 0.5, true, x, 90.0),
            Some(ScrollPart::PageDown)
        );
        assert_eq!(
            scroll_hit(bar, 0.5, true, x, 110.0),
            Some(ScrollPart::LineDown)
        );
        assert_eq!(scroll_hit(bar, 0.5, true, 9.0, 68.0), None);
    }

    #[test]
    fn horizontal_scroll_hit_distinguishes_every_part() {
        let bar = Rect::new(10.0, 20.0, 100.0, BAR);
        let y = bar.y + BAR / 2.0;
        assert_eq!(
            scroll_hit(bar, 0.5, false, 15.0, y),
            Some(ScrollPart::LineUp)
        );
        assert_eq!(
            scroll_hit(bar, 0.5, false, 35.0, y),
            Some(ScrollPart::PageUp)
        );
        assert_eq!(
            scroll_hit(bar, 0.5, false, 62.0, y),
            Some(ScrollPart::Thumb)
        );
        assert_eq!(
            scroll_hit(bar, 0.5, false, 90.0, y),
            Some(ScrollPart::PageDown)
        );
        assert_eq!(
            scroll_hit(bar, 0.5, false, 105.0, y),
            Some(ScrollPart::LineDown)
        );
        assert_eq!(scroll_hit(bar, 0.5, false, 62.0, 19.0), None);
    }
}
