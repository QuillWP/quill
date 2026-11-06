//! Pictures in the text, as the original handles them: click one to select it
//! (a frame with three handles), drag the corner handle to scale it, drag
//! a side handle to crop it, or Shift-drag a side handle to scale it
//! within its crop.

use super::*;
use quill_core::Picture;

/// Handles are this many points square, inside the picture's frame.
const HANDLE: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Right,
    Bottom,
    Corner,
}

/// A handle being dragged.
#[derive(Clone, Copy, Debug)]
pub struct PicDrag {
    pub at: Position,
    pub handle: Handle,
    pub shift: bool,
    /// The picture's frame when the drag began, in window points.
    pub start: Rect,
    /// Where the pointer is now.
    pub to: (f32, f32),
}

impl DocView {
    /// Insert a picture at the selection, shrunk to fit the column if it
    /// is wider (at its own proportions).
    /// Paste Special: the Clipboard as `form`.
    pub fn paste_as(&mut self, form: crate::paste_special::Form, clip: &mut super::Clipboard) {
        use crate::paste_special::Form;
        match form {
            Form::Formatted => {
                let _ = self.exec(crate::commands::Cmd::Paste, false, clip);
            }
            Form::Unformatted => {
                let Some(text) = clip.plain_text() else {
                    return;
                };
                if !self.room_for(quill_core::limits::Size::of_text(&text)) {
                    return;
                }
                // Plain text takes the format of the insertion point.
                self.doc.begin("Paste", self.sel);
                let at = self.delete_selection();
                let chp = self.doc.main.paras[at.para].chp_at(at.offset);
                let end = self.doc.insert_plain(super::MAIN, at, &text, chp);
                self.finish_edit(quill_core::Selection::caret(end));
            }
            Form::Picture => {
                if let Some(pic) =
                    crate::paste_special::clipboard_image().and_then(quill_formats::images::picture)
                {
                    self.insert_picture(pic);
                }
            }
        }
    }

    pub fn insert_picture(&mut self, mut pic: Picture) {
        crate::image::ensure_preview(&mut pic);
        let width = self.column_twips(self.sel.focus.para);
        if pic.size.0 > width {
            let h = (pic.size.1 as i64 * width as i64 / pic.size.0.max(1) as i64) as i32;
            pic = pic.scaled_to(width, h);
        }
        self.doc.begin("Insert Picture", self.sel);
        let at = self.delete_selection();
        let chp = self
            .typing_chp
            .unwrap_or_else(|| self.doc.main.paras[at.para].chp_at(at.offset));
        let end = self.doc.insert_picture(MAIN, at, pic, chp);
        self.finish_edit(Selection::caret(end));
    }

    /// The picture whose character starts at `at`.
    fn picture_at(&self, at: Position) -> Option<&Picture> {
        let p = self.doc.main.paras.get(at.para)?;
        if !p.text[at.offset..].starts_with(chars::OBJECT) {
            return None;
        }
        self.doc.picture(p.chp_of_char(at.offset).object)
    }

    /// The picture that is the whole selection, if there is one.
    pub fn selected_picture(&self) -> Option<Position> {
        let (s, e) = self.sel.range();
        (s.para == e.para && e.offset == s.offset + chars::OBJECT.len_utf8())
            .then_some(s)
            .filter(|&s| self.picture_at(s).is_some())
    }

    /// The picture's frame in window points.
    fn picture_rect(&self, at: Position) -> Option<Rect> {
        let pic = self.picture_at(at)?;
        let l = self.layouts.get(at.para)?.as_ref()?;
        let li = l.line_of(at.offset, false);
        let line = &l.lines[li];
        let g = l
            .line_glyphs(line)
            .iter()
            .find(|g| g.offset as usize == at.offset)?;
        let (w, h) = (pic.size.0 as f32 / 20.0, pic.size.1 as f32 / 20.0);
        let base = self.tops[at.para] + line.top + line.baseline;
        let (x, y) = self.to_window(self.xs[at.para] + g.x, base - h);
        Some(Rect::new(x, y, w, h))
    }

    fn handles(r: Rect) -> [(Handle, Rect); 3] {
        let h = HANDLE;
        [
            (
                Handle::Right,
                Rect::new(r.right() - h, r.y + (r.h - h) / 2.0, h, h),
            ),
            (
                Handle::Bottom,
                Rect::new(r.x + (r.w - h) / 2.0, r.bottom() - h, h, h),
            ),
            (
                Handle::Corner,
                Rect::new(r.right() - h, r.bottom() - h, h, h),
            ),
        ]
    }

    /// The handle under a window point on the selected picture, or the one
    /// being dragged, for the pointer.
    pub fn picture_cursor(&self, wx: f32, wy: f32) -> Option<Handle> {
        if let Some(d) = &self.pic_drag {
            return Some(d.handle);
        }
        let r = self.picture_rect(self.selected_picture()?)?;
        Self::handles(r)
            .into_iter()
            .find(|(_, h)| Self::grown(*h).contains(wx, wy))
            .map(|(handle, _)| handle)
    }

    /// A handle with the few points around it that still grab it.
    fn grown(h: Rect) -> Rect {
        Rect::new(h.x - 2.0, h.y - 2.0, h.w + 4.0, h.h + 4.0)
    }

    /// A click on a picture: select it, or start dragging one of its
    /// handles. Returns true if the click was the picture's.
    pub(super) fn picture_mouse_down(&mut self, wx: f32, wy: f32, shift: bool) -> bool {
        if let Some(at) = self.selected_picture()
            && let Some(r) = self.picture_rect(at)
            && let Some((handle, _)) = Self::handles(r)
                .into_iter()
                .find(|(_, h)| Self::grown(*h).contains(wx, wy))
        {
            self.pic_drag = Some(PicDrag {
                at,
                handle,
                shift,
                start: r,
                to: (wx, wy),
            });
            self.drag_status();
            return true;
        }
        let (pos, _) = self.hit(wx, wy);
        let story = &self.doc.main;
        let candidates = [pos, story.prev_char(pos)];
        for at in candidates {
            if at.para != pos.para {
                continue;
            }
            if let Some(r) = self.picture_rect(at)
                && r.contains(wx, wy)
            {
                let end = Position::new(at.para, at.offset + chars::OBJECT.len_utf8());
                self.sel = Selection::new(at, end);
                self.drag = None;
                self.reset_blink();
                return true;
            }
        }
        false
    }

    /// The frame the picture would have if the drag ended here.
    fn dragged_rect(d: &PicDrag) -> Rect {
        let r = d.start;
        let w = (d.to.0 - r.x).max(4.0);
        let h = (d.to.1 - r.y).max(4.0);
        match d.handle {
            Handle::Right => Rect::new(r.x, r.y, w, r.h),
            Handle::Bottom => Rect::new(r.x, r.y, r.w, h),
            Handle::Corner => Rect::new(r.x, r.y, w, h),
        }
    }

    /// What the status bar shows while a handle is dragged: the scale for
    /// the corner, the size for a side.
    fn drag_status(&mut self) {
        let Some(d) = self.pic_drag else {
            return;
        };
        let Some(pic) = self.picture_at(d.at) else {
            return;
        };
        let r = Self::dragged_rect(&d);
        let (sx, sy) = pic.scale();
        self.status = Some(match d.handle {
            Handle::Corner => {
                let px = (sx * r.w / d.start.w).round();
                let py = (sy * r.h / d.start.h).round();
                if px == py {
                    format!("{px}%")
                } else {
                    format!("{px}% \u{00D7} {py}%")
                }
            }
            Handle::Right => crate::dialog::units::length((r.w * 20.0).round() as i32),
            Handle::Bottom => crate::dialog::units::length((r.h * 20.0).round() as i32),
        });
    }

    pub(super) fn picture_drag(&mut self, wx: f32, wy: f32) -> bool {
        let Some(d) = &mut self.pic_drag else {
            return false;
        };
        d.to = (wx, wy);
        self.drag_status();
        true
    }

    /// Let go of a handle: scale or crop the picture.
    pub(super) fn picture_mouse_up(&mut self) -> bool {
        let Some(d) = self.pic_drag.take() else {
            return false;
        };
        self.status = None;
        let Some(pic) = self.picture_at(d.at).cloned() else {
            return true;
        };
        let r = Self::dragged_rect(&d);
        let (w, h) = ((r.w * 20.0).round() as i32, (r.h * 20.0).round() as i32);
        if (w, h) == pic.size {
            return true;
        }
        let new = match (d.handle, d.shift) {
            (Handle::Corner, _) => pic.scaled_to(w, h),
            (_, false) => pic.cropped_to(w, h),
            // Shift with a side handle scales the picture by that side and
            // keeps the other side's size, cropping to it.
            (Handle::Right, true) => {
                let k = w as f32 / pic.size.0.max(1) as f32;
                let big = pic.scaled_to(w, (pic.size.1 as f32 * k).round() as i32);
                big.cropped_to(w, pic.size.1)
            }
            (Handle::Bottom, true) => {
                let k = h as f32 / pic.size.1.max(1) as f32;
                let big = pic.scaled_to((pic.size.0 as f32 * k).round() as i32, h);
                big.cropped_to(pic.size.0, h)
            }
        };
        self.doc.begin("Picture", self.sel);
        self.doc.set_picture(MAIN, d.at, new);
        let sel = self.sel;
        self.finish_edit(sel);
        true
    }

    /// The selected picture's frame and handles, and the outline of a drag.
    pub(super) fn draw_picture_selection(&self, c: &mut Canvas) {
        let Some(at) = self.selected_picture() else {
            return;
        };
        let Some(r) = self.picture_rect(at) else {
            return;
        };
        c.frame(r, color::BLACK);
        for (_, h) in Self::handles(r) {
            c.fill(h, color::BLACK);
        }
        if let Some(d) = &self.pic_drag {
            let o = Self::dragged_rect(d);
            c.dotted_hline(o.x, o.right(), o.y, color::BLACK);
            c.dotted_hline(o.x, o.right(), o.bottom(), color::BLACK);
            let mut y = o.y.floor();
            while y < o.bottom() {
                c.fill(Rect::new(o.x, y, c.hair(), c.hair()), color::BLACK);
                c.fill(Rect::new(o.right(), y, c.hair(), c.hair()), color::BLACK);
                y += 2.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pointer_finds_the_handles_of_the_selected_picture() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let mut v = DocView::new(quill_core::Document::new(), "Pic".into());
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x40\0\0\0\x30\x08\x02\0\0\0".to_vec();
        v.insert_picture(Picture::from_bytes(png).unwrap());
        v.update_layout(&mut fonts);
        let at = Position::new(0, 0);
        let end = Position::new(0, chars::OBJECT.len_utf8());
        // Not selected: no handles.
        v.set_selection(Selection::caret(end));
        let r = v.picture_rect(at).unwrap();
        assert_eq!(v.picture_cursor(r.right() - 3.0, r.bottom() - 3.0), None);
        v.set_selection(Selection::new(at, end));
        let corner = (r.right() - 3.0, r.bottom() - 3.0);
        assert_eq!(v.picture_cursor(corner.0, corner.1), Some(Handle::Corner));
        let right = (r.right() - 3.0, r.y + r.h / 2.0);
        assert_eq!(v.picture_cursor(right.0, right.1), Some(Handle::Right));
        let bottom = (r.x + r.w / 2.0, r.bottom() - 3.0);
        assert_eq!(v.picture_cursor(bottom.0, bottom.1), Some(Handle::Bottom));
        // Just outside the grab margin, and the middle of the picture.
        assert_eq!(v.picture_cursor(r.right() + 4.0, r.bottom() + 4.0), None);
        assert_eq!(v.picture_cursor(r.x + 5.0, r.y + 5.0), None);
        // While a handle is being dragged the pointer keeps its shape.
        assert!(v.picture_mouse_down(corner.0, corner.1, false));
        assert_eq!(v.picture_cursor(0.0, 0.0), Some(Handle::Corner));
    }
}
