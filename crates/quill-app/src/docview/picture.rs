//! Pictures in the text: click one to select it (a frame with a handle on
//! each side and corner). Drag a handle to scale the picture at its own
//! proportions, Command-drag to stretch it in the direction pulled, and
//! Control-drag, or drag after Crop in Place, to crop it: the picture stays
//! where it is and only its frame moves in.

use super::*;
use quill_core::Picture;

/// Handles are this many points square, inside the picture's frame.
const HANDLE: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Handle {
    const ALL: [Handle; 8] = [
        Handle::TopLeft,
        Handle::Top,
        Handle::TopRight,
        Handle::Left,
        Handle::Right,
        Handle::BottomLeft,
        Handle::Bottom,
        Handle::BottomRight,
    ];

    /// Which way the handle pulls across and down: -1, 0 or 1.
    fn pull(self) -> (f32, f32) {
        match self {
            Handle::Left => (-1.0, 0.0),
            Handle::Right => (1.0, 0.0),
            Handle::Top => (0.0, -1.0),
            Handle::Bottom => (0.0, 1.0),
            Handle::TopLeft => (-1.0, -1.0),
            Handle::TopRight => (1.0, -1.0),
            Handle::BottomLeft => (-1.0, 1.0),
            Handle::BottomRight => (1.0, 1.0),
        }
    }
}

/// A handle being dragged.
#[derive(Clone, Copy, Debug)]
pub struct PicDrag {
    pub at: Position,
    pub handle: Handle,
    /// Control was down, or Crop in Place is on: the frame moves in over
    /// the picture.
    pub crop: bool,
    /// Command was down: stretch in the direction pulled instead of keeping
    /// the picture's proportions.
    pub stretch: bool,
    /// The picture's frame when the drag began, in window points.
    pub start: Rect,
    /// How far each edge can come back out, in points: what is cropped from
    /// the left, top, right and bottom.
    pub room: [f32; 4],
    /// Where the handle was grabbed, and where the pointer is now.
    pub from: (f32, f32),
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

    fn handles(r: Rect) -> [(Handle, Rect); 8] {
        let h = HANDLE;
        Handle::ALL.map(|handle| {
            let (px, py) = handle.pull();
            let at = |pull: f32, start: f32, size: f32| start + (size - h) * (pull + 1.0) / 2.0;
            (handle, Rect::new(at(px, r.x, r.w), at(py, r.y, r.h), h, h))
        })
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

    /// Whether the pointer at this window point is the crop pointer: over a
    /// handle with Control held, or anywhere on the picture in Crop in Place.
    pub fn picture_crop_cursor(&self, wx: f32, wy: f32) -> bool {
        if let Some(d) = &self.pic_drag {
            return d.crop;
        }
        let Some(r) = self.selected_picture().and_then(|at| self.picture_rect(at)) else {
            return false;
        };
        let handle = self.picture_cursor(wx, wy).is_some();
        (self.crop_held && handle) || (self.cropping_in_place() && (handle || r.contains(wx, wy)))
    }

    /// Whether Crop in Place is on for the selected picture.
    pub fn cropping_in_place(&self) -> bool {
        self.pic_crop.is_some() && self.pic_crop == self.selected_picture()
    }

    /// Right-click, Crop in Place or Resize in Place.
    pub(super) fn crop_in_place(&mut self, on: bool) {
        self.pic_crop = self.selected_picture().filter(|_| on);
    }

    /// Select the picture at a window point, if there is one there.
    pub fn select_picture_at(&mut self, wx: f32, wy: f32) -> bool {
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

    /// A handle with the few points around it that still grab it.
    fn grown(h: Rect) -> Rect {
        Rect::new(h.x - 2.0, h.y - 2.0, h.w + 4.0, h.h + 4.0)
    }

    /// A click on a picture: select it, or start dragging one of its
    /// handles. Returns true if the click was the picture's.
    pub(super) fn picture_mouse_down(&mut self, wx: f32, wy: f32, cmd: bool) -> bool {
        if let Some(at) = self.selected_picture()
            && let Some(r) = self.picture_rect(at)
            && let Some((handle, _)) = Self::handles(r)
                .into_iter()
                .find(|(_, h)| Self::grown(*h).contains(wx, wy))
        {
            let cut = self.picture_at(at).map_or([0.0; 4], |p| p.crop_on_page());
            self.pic_drag = Some(PicDrag {
                at,
                handle,
                crop: self.crop_held || self.cropping_in_place(),
                stretch: cmd,
                start: r,
                room: cut.map(|t| t / 20.0),
                from: (wx, wy),
                to: (wx, wy),
            });
            self.drag_status();
            return true;
        }
        self.pic_crop = None;
        self.select_picture_at(wx, wy)
    }

    /// The frame the picture would have if the drag ended here. The edges
    /// the handle doesn't pull stay put.
    fn dragged_rect(d: &PicDrag) -> Rect {
        let r = d.start;
        let (px, py) = d.handle.pull();
        // Measured from the grab point, so a handle grabbed off its center
        // does not jump.
        let mut w = (r.w + (d.to.0 - d.from.0) * px).max(4.0);
        let mut h = (r.h + (d.to.1 - d.from.1) * py).max(4.0);
        if d.crop {
            // Cropping can give back only what was cropped.
            let out = |pull: f32, near: f32, far: f32| if pull < 0.0 { near } else { far };
            w = w.min(r.w + out(px, d.room[0], d.room[2]));
            h = h.min(r.h + out(py, d.room[1], d.room[3]));
        } else if !d.stretch {
            // Keep the proportions, following whichever side the pointer
            // has pulled further.
            let k = match (px != 0.0, py != 0.0) {
                (true, false) => w / r.w.max(1.0),
                (false, true) => h / r.h.max(1.0),
                _ => (w / r.w.max(1.0)).max(h / r.h.max(1.0)),
            };
            (w, h) = ((r.w * k).max(4.0), (r.h * k).max(4.0));
        }
        let x = if px < 0.0 { r.right() - w } else { r.x };
        let y = if py < 0.0 { r.bottom() - h } else { r.y };
        Rect::new(x, y, w, h)
    }

    /// What the status bar shows while a handle is dragged: the scale, or
    /// the size when cropping.
    fn drag_status(&mut self) {
        let Some(d) = self.pic_drag else {
            return;
        };
        let Some(pic) = self.picture_at(d.at) else {
            return;
        };
        let r = Self::dragged_rect(&d);
        let (sx, sy) = pic.scale();
        self.status = Some(if d.crop {
            let length = |points: f32| crate::dialog::units::length((points * 20.0).round() as i32);
            format!("{} \u{00D7} {}", length(r.w), length(r.h))
        } else {
            let px = (sx * r.w / d.start.w).round();
            let py = (sy * r.h / d.start.h).round();
            if px == py {
                format!("{px}%")
            } else {
                format!("{px}% \u{00D7} {py}%")
            }
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
        let new = if d.crop {
            let inward = |near: f32, far: f32, pull: f32| {
                let by = (far - near) * 20.0;
                if pull == 0.0 { 0.0 } else { by }
            };
            let (px, py) = d.handle.pull();
            let s = d.start;
            pic.crop_edges([
                inward(s.x, r.x, f32::from(px < 0.0)),
                inward(s.y, r.y, f32::from(py < 0.0)),
                inward(r.right(), s.right(), f32::from(px > 0.0)),
                inward(r.bottom(), s.bottom(), f32::from(py > 0.0)),
            ])
        } else {
            pic.scaled_to(w, h)
        };
        self.apply_picture(d.at, new);
        true
    }

    /// Change the picture at `at`, as one undo step.
    pub fn apply_picture(&mut self, at: Position, new: Picture) {
        self.doc.begin("Picture", self.sel);
        self.doc.set_picture(MAIN, at, new);
        let sel = self.sel;
        self.finish_edit(sel);
    }

    /// The selected picture, for the Picture dialog.
    pub fn selected_picture_copy(&self) -> Option<(Position, Picture)> {
        let at = self.selected_picture()?;
        Some((at, self.picture_at(at)?.clone()))
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
        assert_eq!(
            v.picture_cursor(corner.0, corner.1),
            Some(Handle::BottomRight)
        );
        let right = (r.right() - 3.0, r.y + r.h / 2.0);
        assert_eq!(v.picture_cursor(right.0, right.1), Some(Handle::Right));
        let bottom = (r.x + r.w / 2.0, r.bottom() - 3.0);
        assert_eq!(v.picture_cursor(bottom.0, bottom.1), Some(Handle::Bottom));
        assert_eq!(
            v.picture_cursor(r.x + 3.0, r.y + 3.0),
            Some(Handle::TopLeft)
        );
        assert_eq!(
            v.picture_cursor(r.x + 3.0, r.y + r.h / 2.0),
            Some(Handle::Left)
        );
        // Just outside the grab margin, and the middle of the picture.
        assert_eq!(v.picture_cursor(r.right() + 4.0, r.bottom() + 4.0), None);
        assert_eq!(v.picture_cursor(r.x + 20.0, r.y + 20.0), None);
        // While a handle is being dragged the pointer keeps its shape.
        assert!(v.picture_mouse_down(corner.0, corner.1, false));
        assert_eq!(v.picture_cursor(0.0, 0.0), Some(Handle::BottomRight));
    }

    /// Insert a 64 x 48 point picture, select it and lay it out.
    fn view_with_picture() -> Option<(DocView, Position, Rect)> {
        let mut fonts = crate::pdf::tests::test_fonts()?;
        let mut v = DocView::new(quill_core::Document::new(), "Pic".into());
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x40\0\0\0\x30\x08\x02\0\0\0".to_vec();
        v.insert_picture(Picture::from_bytes(png).unwrap());
        v.update_layout(&mut fonts);
        let at = Position::new(0, 0);
        let end = Position::new(0, chars::OBJECT.len_utf8());
        v.set_selection(Selection::new(at, end));
        let r = v.picture_rect(at)?;
        Some((v, at, r))
    }

    #[test]
    fn the_corner_keeps_proportions_unless_command_is_down() {
        let Some((mut v, at, r)) = view_with_picture() else {
            return;
        };
        let corner = (r.right() - 3.0, r.bottom() - 3.0);
        // Pull right only: the height follows.
        assert!(v.picture_mouse_down(corner.0, corner.1, false));
        v.picture_drag(corner.0 + r.w, corner.1);
        v.picture_mouse_up();
        let pic = v.picture_at(at).unwrap();
        let (sx, sy) = pic.scale();
        assert!((sx - sy).abs() < 1.0, "{sx} x {sy}");
        assert!(sx > 150.0);
        // With Command it stretches across only.
        let r = v.picture_rect(at).unwrap();
        let corner = (r.right() - 3.0, r.bottom() - 3.0);
        assert!(v.picture_mouse_down(corner.0, corner.1, true));
        v.picture_drag(corner.0 + 40.0, corner.1);
        v.picture_mouse_up();
        let (sx2, sy2) = v.picture_at(at).unwrap().scale();
        assert!(sx2 > sx + 10.0, "{sx} x {sy} then {sx2} x {sy2}");
        assert!((sy2 - sy).abs() < 1.0);
    }

    #[test]
    fn a_side_handle_scales_at_the_pictures_proportions() {
        let Some((mut v, at, r)) = view_with_picture() else {
            return;
        };
        let left = (r.x + 3.0, r.y + r.h / 2.0);
        assert!(v.picture_mouse_down(left.0, left.1, false));
        v.picture_drag(left.0 - r.w, left.1);
        v.picture_mouse_up();
        let (sx, sy) = v.picture_at(at).unwrap().scale();
        assert!(
            (sx - 200.0).abs() < 1.0 && (sy - 200.0).abs() < 1.0,
            "{sx} x {sy}"
        );
        // With Command it stretches across only.
        let r = v.picture_rect(at).unwrap();
        let right = (r.right() - 3.0, r.y + r.h / 2.0);
        assert!(v.picture_mouse_down(right.0, right.1, true));
        v.picture_drag(right.0 + 40.0, right.1);
        v.picture_mouse_up();
        let (sx, sy) = v.picture_at(at).unwrap().scale();
        assert!(sx > 240.0 && (sy - 200.0).abs() < 1.0, "{sx} x {sy}");
    }

    #[test]
    fn control_crops_only_the_edges_the_handle_pulls() {
        let Some((mut v, at, r)) = view_with_picture() else {
            return;
        };
        v.crop_held = true;
        let corner = (r.x + 3.0, r.y + 3.0);
        assert!(v.picture_crop_cursor(corner.0, corner.1));
        assert!(!v.picture_crop_cursor(r.x + 20.0, r.y + 20.0));
        assert!(v.picture_mouse_down(corner.0, corner.1, false));
        v.picture_drag(corner.0 + 32.0, corner.1 + 12.0);
        v.picture_mouse_up();
        let pic = v.picture_at(at).unwrap();
        assert_eq!(pic.size, (32 * 20, 36 * 20));
        assert!(
            pic.crop.left > 40_000 && pic.crop.top > 20_000,
            "{:?}",
            pic.crop
        );
        assert_eq!((pic.crop.right, pic.crop.bottom), (0, 0));
        assert!((pic.scale().0 - 100.0).abs() < 1.0);
        // The edge can come back only as far as the picture goes.
        let r = v.picture_rect(at).unwrap();
        let left = (r.x + 3.0, r.y + r.h / 2.0);
        assert!(v.picture_mouse_down(left.0, left.1, false));
        v.picture_drag(left.0 - 500.0, left.1);
        v.picture_mouse_up();
        let pic = v.picture_at(at).unwrap();
        assert_eq!(pic.crop.left, 0);
        assert_eq!(pic.size.0, 64 * 20);
    }

    #[test]
    fn crop_in_place_needs_no_control_and_ends_on_resize_in_place() {
        let Some((mut v, at, r)) = view_with_picture() else {
            return;
        };
        v.crop_in_place(true);
        assert!(v.picture_crop_cursor(r.x + 20.0, r.y + 20.0));
        let right = (r.right() - 3.0, r.y + r.h / 2.0);
        assert!(v.picture_mouse_down(right.0, right.1, false));
        v.picture_drag(right.0 - 32.0, right.1);
        v.picture_mouse_up();
        assert!(v.picture_at(at).unwrap().crop.right > 40_000);
        v.crop_in_place(false);
        assert!(!v.picture_crop_cursor(r.x + 20.0, r.y + 20.0));
    }
}
