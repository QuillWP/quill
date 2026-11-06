//! Frames (Format > Frame): paragraphs placed on the page, with the text
//! flowing round them. As in the original they are placed only in Page Layout
//! view, Print Preview and printing; Normal view shows them where they are
//! in the text.

use super::*;
use quill_core::props::{Frame, FrameH, FrameRel, FrameV};
use quill_layout::Clear;

/// A frame placed in the column: paragraphs `first..=last`, at `x`, `y`
/// (points from the left margin and down the column), `w` by `h`.
#[derive(Clone, Copy, Debug)]
pub struct FrameBox {
    pub first: usize,
    pub last: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub from_text: f32,
    /// A drop cap: the text below it starts straight after its lines.
    pub drop: bool,
}

impl FrameBox {
    /// The area the text keeps out of.
    pub fn clear(&self) -> Clear {
        let d = self.from_text;
        let dv = if self.drop { 0.0 } else { d };
        Clear {
            top: self.y - dv,
            bottom: self.y + self.h + dv,
            left: self.x - d,
            right: self.x + self.w + d,
        }
    }
}

/// How far down a frame sits, in points from the top margin, given the
/// height of the text area and the page.
fn frame_top(f: &Frame, h: f32, text_h: f32, page_h: f32, top_margin: f32) -> f32 {
    let page = f.v_rel == FrameRel::Page;
    let local = match f.v {
        FrameV::InLine => 0.0,
        FrameV::Top if page => -top_margin,
        FrameV::Top => 0.0,
        FrameV::Center if page => (page_h - h) / 2.0 - top_margin,
        FrameV::Center => (text_h - h) / 2.0,
        FrameV::Bottom if page => page_h - h - top_margin,
        FrameV::Bottom => text_h - h,
        FrameV::At(t) if page => t as f32 / 20.0 - top_margin,
        FrameV::At(t) => t as f32 / 20.0,
    };
    // Frames stay inside the text area, so they belong to one page.
    local.clamp(0.0, (text_h - h).max(0.0))
}

impl DocView {
    /// Whether frames are placed on the page (not shown in the text).
    pub(super) fn frames_on(&self) -> bool {
        self.page_view || self.headers_wanted || self.printing
    }

    /// Whether any frame is placed relative to the page rather than where
    /// its text is, so placing it needs the pages.
    pub(super) fn has_page_frames(&self) -> bool {
        self.frames_on()
            && self
                .doc
                .main
                .paras
                .iter()
                .any(|p| p.pap.frame.is_some_and(|f| f.v != FrameV::InLine))
    }

    /// Place the frame that starts at paragraph `i`, with the text column
    /// at `y`. None if `i` is not framed. The document's last paragraph is
    /// never taken out of the text, so there is always somewhere to type.
    pub(super) fn place_frame(&self, i: usize, y: f32) -> Option<FrameBox> {
        let paras = &self.doc.main.paras;
        let f = paras[i].pap.frame?;
        if i + 1 >= paras.len() || paras[i].pap.in_table {
            return None;
        }
        let mut last = i;
        while last + 2 < paras.len()
            && paras[last + 1].pap.frame == Some(f)
            && !paras[last + 1].pap.in_table
        {
            last += 1;
        }
        let layout = |p: usize| self.layouts[p].as_ref();
        let h: f32 = (i..=last)
            .map(|p| layout(p).map_or(0.0, |l| l.height))
            .sum();
        // A drop cap spans body lines, not the oversized letter's line
        // box (which also includes font leading and descent).
        let h = if f.drop_lines > 0 {
            layout(last + 1)
                .and_then(|body| {
                    let count = usize::from(f.drop_lines);
                    let used = count.min(body.lines.len());
                    let line = body.lines.get(used.checked_sub(1)?)?;
                    Some(line.top + line.height * (1 + count - used) as f32)
                })
                .unwrap_or(h)
        } else {
            h
        };
        let pt = |t: i32| t as f32 / 20.0;
        let props = &self.doc.props;
        let column = pt(props.text_width());
        // Auto width: as wide as the widest line, so a picture or a drop
        // cap gets a frame its own size.
        let w = match f.width {
            Some(t) => pt(t),
            None => (i..=last)
                .filter_map(layout)
                .flat_map(|l| l.lines.iter().map(|ln| ln.end_x))
                .fold(0.0f32, f32::max)
                .clamp(1.0, column),
        };
        let page = if self.pages.is_empty() {
            0
        } else {
            paginate::page_at(&self.pages, y)
        };
        // Inside and Outside depend on which side of a spread the page is:
        // page 1 is a right-hand page.
        let right_hand = page % 2 == 0;
        let (lo, hi) = match f.h_rel {
            FrameRel::Page => {
                let left = pt(props.margin_left + props.gutter);
                (-left, pt(props.page_width) - left)
            }
            _ => (0.0, column),
        };
        let x = if f.drop_lines > 0 && f.in_margin {
            -w - pt(f.from_text)
        } else {
            match f.h {
                FrameH::Left => lo,
                FrameH::Right => hi - w,
                FrameH::Center => (lo + hi - w) / 2.0,
                FrameH::Inside if right_hand => lo,
                FrameH::Inside => hi - w,
                FrameH::Outside if right_hand => hi - w,
                FrameH::Outside => lo,
                FrameH::At(t) => lo + pt(t),
            }
        };
        let fy = if f.v == FrameV::InLine || self.pages.is_empty() {
            y
        } else {
            let text_h = self.page_body_height(page);
            let local = frame_top(
                &f,
                h,
                text_h,
                pt(props.page_height),
                self.page_body_top(page),
            );
            self.doc_y_on_page(page, local)
        };
        Some(FrameBox {
            first: i,
            last,
            x,
            y: fy,
            w,
            h,
            from_text: pt(f.from_text),
            drop: f.drop_lines > 0,
        })
    }

    /// Place the paragraphs, then lay out again the ones a frame now
    /// pushes aside, until nothing changes.
    pub(super) fn place_and_wrap(&mut self, fonts: &mut Fonts, opts: &LayoutOptions) {
        let fonts_table = self.doc.fonts.clone();
        let name = |id: u16| fonts_table.get(id as usize).cloned().unwrap_or_default();
        for _ in 0..4 {
            self.place();
            let n = self.layouts.len();
            let mut again = false;
            for i in 0..n {
                let want = self.want_wrap.get(i).cloned().unwrap_or_default();
                if self.wraps.get(i) == Some(&want) {
                    continue;
                }
                let paras = &self.doc.main.paras;
                let j = Joins::around(paras, i);
                let o = LayoutOptions {
                    width: self.widths[i],
                    wrap: want.clone(),
                    ..opts.clone()
                };
                self.layouts[i] = Some(layout_paragraph_joined(&paras[i], &name, fonts, &o, j));
                self.wraps[i] = want;
                again = true;
            }
            if !again {
                break;
            }
        }
    }

    /// The frames on page `k`: their index and where they are on the page,
    /// in points from its top left corner.
    pub fn page_frames(&self, k: usize) -> Vec<(usize, Rect)> {
        if k >= self.pages.len() {
            return Vec::new();
        }
        let p = &self.doc.props;
        let left = (p.margin_left + p.gutter) as f32 / 20.0;
        self.frames
            .iter()
            .enumerate()
            .filter_map(|(i, f)| {
                let (page, x, local) = self.on_page(f.y);
                (page == k).then(|| {
                    (
                        i,
                        Rect::new(left + x + f.x, self.page_body_top(k) + local, f.w, f.h),
                    )
                })
            })
            .collect()
    }

    /// Put frame `i` at (`x`, `y`) points from its page's top left corner,
    /// as dragging it in editable Page Layout does.
    pub fn move_frame(&mut self, i: usize, x: f32, y: f32) {
        let Some(f) = self.frames.get(i).copied() else {
            return;
        };
        let t = |v: f32| (v * 20.0).round() as i32;
        let sel = self.sel;
        self.doc.begin("Move Frame", sel);
        self.doc.format_paras(MAIN, f.first, f.last, |p| {
            if let Some(fr) = &mut p.frame {
                fr.h = quill_core::FrameH::At(t(x));
                fr.h_rel = FrameRel::Page;
                fr.v = FrameV::At(t(y));
                fr.v_rel = FrameRel::Page;
                fr.drop_lines = 0;
                if fr.from_text == 0 {
                    fr.from_text = 180;
                }
            }
        });
        self.doc.commit(sel);
    }

    pub fn frame_position_point(&self, page: usize, x: f32, y: f32) -> (f32, f32) {
        (
            x - self.area.x - self.page_left(),
            y - self.area.y + self.scroll_y - self.page_top(page),
        )
    }

    pub fn frame_position_hit(&self, x: f32, y: f32) -> Option<(usize, usize, Rect, f32, f32)> {
        if !self.page_view || !self.area.contains(x, y) {
            return None;
        }
        let page = self.visible_page();
        for page in page..(page + 2).min(self.pages.len()) {
            let (px, py) = self.frame_position_point(page, x, y);
            for (index, rect) in self.page_frames(page) {
                if rect.contains(px, py) {
                    return Some((index, page, rect, px, py));
                }
            }
        }
        None
    }

    pub fn frame_target_rect(&self, page: usize, target: Rect) -> Rect {
        Rect::new(
            self.area.x + self.page_left() + target.x,
            self.area.y + self.page_top(page) - self.scroll_y + target.y,
            target.w,
            target.h,
        )
    }

    /// A click on a placed frame: the frame's paragraph under document
    /// point (x, y).
    pub(super) fn frame_at(&self, x: f32, y: f32) -> Option<usize> {
        let f = self
            .frames
            .iter()
            .find(|f| x >= f.x && x < f.x + f.w && y >= f.y && y < f.y + f.h)?;
        (f.first..=f.last)
            .rev()
            .find(|&p| self.tops[p] <= y)
            .or(Some(f.first))
    }

    /// Draw the frames that fall between document y `top` and `bottom`,
    /// with document (0, 0) at (`ox`, `oy`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_frames(
        &self,
        c: &mut dyn Surface,
        fonts: &Fonts,
        cache: &mut GlyphCache,
        focused: bool,
        ox: f32,
        oy: f32,
        top: f32,
        bottom: f32,
    ) {
        let (s, e) = self.sel.range();
        for f in &self.frames {
            if f.y + f.h < top || f.y > bottom {
                continue;
            }
            for para in f.first..=f.last {
                let l = self.layout(para);
                let (px, py) = (ox + self.xs[para], oy + self.tops[para]);
                if let Some(bx) = &l.border {
                    draw_border_box(c, bx, px, py);
                }
                if c.screen()
                    && !self.sel.is_empty()
                    && self.selected_picture().is_none()
                    && para >= s.para
                    && para <= e.para
                {
                    self.draw_selection(c, para, l, px, py, s, e, focused);
                }
                self.draw_paragraph(c, fonts, cache, para, l, px, py);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positioned_frames_move_in_page_layout_without_print_preview() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let mut view = DocView::new(
            quill_core::text::from_plain_text("Frame content\nBody text"),
            "Frame".into(),
        );
        let frame = Frame {
            h: FrameH::At(1440),
            v: FrameV::At(1440),
            h_rel: FrameRel::Page,
            v_rel: FrameRel::Page,
            width: Some(2880),
            ..Default::default()
        };
        view.doc.main.paras[0].pap.frame = Some(frame);
        view.page_view = true;
        view.area = Rect::new(0.0, 0.0, 900.0, 700.0);
        view.update_layout(&mut fonts);
        let &(index, rect) = view.page_frames(0).first().expect("placed frame");
        let target = view.frame_target_rect(0, rect);
        let hit = view
            .frame_position_hit(target.x + 1.0, target.y + 1.0)
            .unwrap();
        assert_eq!(hit.0, index);
        view.move_frame(index, 90.0, 110.0);
        let moved = view.doc.main.paras[0].pap.frame.unwrap();
        assert_eq!(moved.h, FrameH::At(1800));
        assert_eq!(moved.v, FrameV::At(2200));
        assert_eq!(moved.h_rel, FrameRel::Page);
        assert_eq!(moved.v_rel, FrameRel::Page);
    }

    #[test]
    fn drop_caps_wrap_the_requested_number_of_body_lines() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        for lines in [2, 3, 4] {
            let mut doc = quill_core::text::from_plain_text(&format!(
                "W\n{}",
                "rapped text flows around the cap and returns to the margin. ".repeat(20)
            ));
            doc.main.paras[0].format_range(0, 1, &mut |c| c.size = 96);
            doc.main.paras[0].pap.frame = Some(Frame {
                drop_lines: lines,
                from_text: 40,
                ..Default::default()
            });
            let mut view = DocView::new(doc, "Drop cap".into());
            view.page_view = true;
            view.area = Rect::new(0.0, 0.0, 900.0, 700.0);
            view.update_layout(&mut fonts);
            let body = view.layout(1);
            for line in &body.lines[..usize::from(lines)] {
                assert!(line.left > 0.0, "line beside {lines}-line cap");
            }
            assert_eq!(body.lines[usize::from(lines)].left, 0.0);
            assert_eq!(view.doc.main.paras[0].chp_of_char(0).size, 96);
        }
    }

    #[test]
    fn vertical_positions() {
        let f = |v, rel| Frame {
            v,
            v_rel: rel,
            ..Default::default()
        };
        // A 100 point frame, 648 points of text on an 792 point page with
        // a 72 point top margin.
        let top = |fr: Frame| frame_top(&fr, 100.0, 648.0, 792.0, 72.0);
        assert_eq!(top(f(FrameV::Top, FrameRel::Margin)), 0.0);
        assert_eq!(top(f(FrameV::Bottom, FrameRel::Margin)), 548.0);
        assert_eq!(top(f(FrameV::Center, FrameRel::Page)), 274.0);
        assert_eq!(top(f(FrameV::At(2880), FrameRel::Page)), 72.0);
        // Above the text area is kept to its top.
        assert_eq!(top(f(FrameV::Top, FrameRel::Page)), 0.0);
    }
}
