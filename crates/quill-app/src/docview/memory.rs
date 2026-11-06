//! Keeping long documents small: a paragraph's glyphs (one for every
//! character, the bulk of a layout) are kept only near what is drawn and
//! around the selection. Paragraphs elsewhere keep their lines and
//! heights, which is all pagination needs, and are laid out again when
//! they come into view.
//!
//! Drawing cannot lay out (it only reads the fonts), so the window calls
//! `prepare` before each `draw`, and Print Preview `prepare_pages`, to
//! lay out again whatever is about to be drawn.

use quill_core::CharProps;
use quill_layout::{Fonts, Joins, LayoutOptions, ParaLayout, layout_paragraph_joined};

use super::{DocView, TEXT_TOP, comment_cards};

/// Documents with fewer paragraphs keep every glyph: there is little to
/// save and nothing to get wrong.
const STRIP_ABOVE: usize = 300;
/// Paragraphs either side of what was last drawn that keep their glyphs,
/// so scrolling a little does not lay anything out again.
const MARGIN: usize = 80;
/// Drawn ranges remembered: both panes of a split window, and Print
/// Preview's pages.
const RANGES: usize = 3;
/// Paragraphs either side of the insertion point given their glyphs back
/// before each draw, so moving the insertion point (arrows, Page Down,
/// several presses between two draws) finds them laid out.
const AROUND_CARET: usize = 40;

impl DocView {
    /// Whether glyphs may be dropped at all just now.
    fn may_strip(&self) -> bool {
        !self.keep_all && self.hyph_job.is_none() && self.layouts.len() > STRIP_ABOVE
    }

    /// Whether paragraph `i` keeps its glyphs: near what was drawn or the
    /// insertion point, at the selection's other end, or a frame (drawn
    /// away from its place in the text).
    fn keeps(&self, i: usize) -> bool {
        let focus = self.sel.focus.para;
        self.drawn
            .iter()
            .any(|&(a, b)| i + MARGIN >= a && i <= b + MARGIN)
            || (i + MARGIN >= focus && i <= focus + MARGIN)
            || i == self.sel.anchor.para
            || self
                .doc
                .main
                .paras
                .get(i)
                .is_some_and(|p| p.pap.frame.is_some())
    }

    /// Whether a paragraph just laid out should drop its glyphs at once,
    /// so opening a long document never holds them all.
    pub(super) fn strip_now(&self, i: usize) -> bool {
        self.may_strip() && !self.keeps(i)
    }

    /// Lay out paragraph `i` at the width and wrap it was last given.
    pub(super) fn layout_one(
        &self,
        i: usize,
        fonts: &mut Fonts,
        opts: &LayoutOptions,
        name: &dyn Fn(u16) -> String,
    ) -> ParaLayout {
        let paras = &self.doc.main.paras;
        let j = Joins::around(paras, i);
        let o = LayoutOptions {
            width: self.widths[i],
            wrap: self.wraps.get(i).cloned().unwrap_or_default(),
            ..opts.clone()
        };
        if self.captions {
            // The comments pane sets every comment in the system's own
            // interface font at one small size, whatever the document's
            // fonts are, so comments read like the rest of the window. Only
            // the drawing changes; the comments keep their own formatting.
            let mut p = paras[i].clone();
            let len = p.len();
            let size = (comment_cards::COMMENT_SIZE * 2.0 * self.comment_scale).round() as u16;
            p.format_range(0, len, &mut |c| {
                c.size = size;
                c.font = comment_cards::INTERFACE_FONT;
            });
            p.mark_chp.size = size;
            p.mark_chp.font = comment_cards::INTERFACE_FONT;
            p.pap.before = 0;
            p.pap.after = 0;
            let name = |id| {
                if id == comment_cards::INTERFACE_FONT {
                    quill_layout::fonts::UI_FONT.to_string()
                } else {
                    name(id)
                }
            };
            layout_paragraph_joined(&p, &name, fonts, &o, j)
        } else if self.outline && !self.show_formatting {
            // Outline view without formatting: everything in the Normal
            // style's font and size.
            let normal = self.doc.styles.get(0).chp;
            let mut p = paras[i].clone();
            let len = p.len();
            p.format_range(0, len, &mut |c| {
                *c = CharProps {
                    object: c.object,
                    hidden: c.hidden,
                    ..normal
                }
            });
            p.mark_chp = normal;
            p.pap.before = 0;
            p.pap.after = 0;
            layout_paragraph_joined(&p, name, fonts, &o, j)
        } else {
            layout_paragraph_joined(&paras[i], name, fonts, &o, j)
        }
    }

    /// Give back the glyphs of the paragraphs in `range` (and at the
    /// selection) that had them dropped.
    fn restore(&mut self, fonts: &mut Fonts, range: std::ops::Range<usize>) {
        let n = self.layouts.len();
        let sel = [self.sel.anchor.para, self.sel.focus.para];
        let wanted: Vec<usize> = range
            .chain(sel)
            .filter(|&i| i < n && self.layouts[i].as_ref().is_some_and(|l| l.stripped))
            .collect();
        if wanted.is_empty() {
            return;
        }
        let opts = self.options();
        let fonts_table = self.doc.fonts.clone();
        let name = |id: u16| fonts_table.get(id as usize).cloned().unwrap_or_default();
        for i in wanted {
            self.layouts[i] = Some(self.layout_one(i, fonts, &opts, &name));
        }
    }

    /// Drop the glyphs of every paragraph no longer kept.
    fn sweep(&mut self) {
        if !self.may_strip() {
            return;
        }
        for i in 0..self.layouts.len() {
            if !self.keeps(i)
                && let Some(l) = &mut self.layouts[i]
                && !l.stripped
            {
                l.strip();
            }
        }
    }

    /// Paragraphs whose text lies between continuous y `y0` and `y1`.
    fn paras_between(&self, y0: f32, y1: f32) -> std::ops::Range<usize> {
        let n = self.layouts.len();
        if self.tops.len() != n + 1 {
            return 0..n;
        }
        // Table cells side by side can make the tops step back a little;
        // a few paragraphs either way covers it.
        let a = self.tops[..n]
            .partition_point(|&t| t < y0)
            .saturating_sub(4);
        let b = (self.tops[..n].partition_point(|&t| t <= y1) + 4).min(n);
        a..b.max(a)
    }

    /// Lay out again what `draw` is about to draw, and drop the glyphs
    /// of paragraphs now far from it. Call after `update_layout`.
    pub fn prepare(&mut self, fonts: &mut Fonts) {
        let range = if self.page_view {
            let (_, ph) = self.page_size();
            let (y0, y1) = (self.scroll_y, self.scroll_y + self.area.h);
            let pages: Vec<usize> = (0..self.pages.len())
                .filter(|&k| {
                    let top = self.page_top(k);
                    top <= y1 && top + ph >= y0
                })
                .collect();
            self.page_range(&pages)
        } else {
            let top = self.scroll_y - TEXT_TOP;
            self.paras_between(top, top + self.area.h)
        };
        self.prepare_range(fonts, range);
    }

    /// Lay out again what pages `pages` show, for Print Preview.
    pub fn prepare_pages(&mut self, fonts: &mut Fonts, pages: &[usize]) {
        let range = self.page_range(pages);
        self.prepare_range(fonts, range);
    }

    /// The paragraphs on pages `pages`.
    fn page_range(&self, pages: &[usize]) -> std::ops::Range<usize> {
        let mut out: Option<std::ops::Range<usize>> = None;
        for &k in pages {
            for b in self.page_bands(k) {
                let r = self.paras_between(b.y, b.end);
                out = Some(match out {
                    Some(o) => o.start.min(r.start)..o.end.max(r.end),
                    None => r,
                });
            }
        }
        out.unwrap_or(0..0)
    }

    fn prepare_range(&mut self, fonts: &mut Fonts, range: std::ops::Range<usize>) {
        self.restore(fonts, range.clone());
        let focus = self.sel.focus.para;
        let n = self.layouts.len();
        self.restore(
            fonts,
            focus.saturating_sub(AROUND_CARET)..(focus + AROUND_CARET + 1).min(n),
        );
        let drawn = (range.start, range.end);
        if self.drawn.first() != Some(&drawn) {
            // Newest first; a range seen before moves up rather than being
            // added again, so two panes do not push each other out.
            let known = self.drawn.contains(&drawn);
            self.drawn.retain(|&d| d != drawn);
            self.drawn.insert(0, drawn);
            self.drawn.truncate(RANGES);
            if !known {
                self.sweep();
            }
        }
    }

    /// Give every paragraph its glyphs back, for work that reads the whole
    /// document's (Hyphenate All).
    pub fn restore_all(&mut self, fonts: &mut Fonts) {
        let n = self.layouts.len();
        self.restore(fonts, 0..n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Rect;
    use quill_core::{Position, Selection};

    /// A document long enough to drop glyphs: paragraphs of different
    /// lengths, so lines and pages fall in different places.
    fn long_doc() -> quill_core::Document {
        let text: Vec<String> = (0..900)
            .map(|k| {
                let words = 10 + (k * 7) % 60;
                (0..words)
                    .map(|w| ["alpha", "beta", "gamma", "delta", "epsilon"][(k + w) % 5])
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        quill_core::text::from_plain_text(&text.join("\n"))
    }

    fn view(fonts: &mut Fonts, keep_all: bool) -> DocView {
        let mut v = DocView::new(long_doc(), "Long".into());
        v.keep_all = keep_all;
        v.area = Rect::new(0.0, 0.0, 600.0, 400.0);
        v.update_layout(fonts);
        v.prepare(fonts);
        v
    }

    /// Each glyph's offset and position, for comparing layouts.
    fn placed(l: &ParaLayout) -> Vec<(u32, u32)> {
        l.glyphs.iter().map(|g| (g.offset, g.x.to_bits())).collect()
    }

    #[test]
    fn long_documents_keep_only_nearby_glyphs() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let full = view(&mut fonts, true);
        let mut lean = view(&mut fonts, false);
        let kept = |v: &DocView| v.layouts.iter().flatten().filter(|l| !l.stripped).count();
        assert_eq!(kept(&full), 900);
        assert!(kept(&lean) < 200, "{} paragraphs kept", kept(&lean));
        // Pagination and positions do not depend on the glyphs.
        assert_eq!(lean.pages, full.pages);
        assert_eq!(lean.tops, full.tops);

        // Jump to the end: the paragraphs there come back exactly as a
        // full layout has them, and the insertion point is in the same place.
        let last = lean.doc.main.paras.len() - 1;
        let at = Position::new(last, 20);
        lean.sel = Selection::caret(at);
        lean.scroll_y = lean.max_scroll();
        lean.prepare(&mut fonts);
        assert_eq!(
            lean.caret_geometry(at, false),
            full.caret_geometry(at, false)
        );
        let (a, b) = (lean.layouts[last].as_ref(), full.layouts[last].as_ref());
        assert!(!a.unwrap().stripped);
        assert_eq!(placed(a.unwrap()), placed(b.unwrap()));
        // The middle, never near anything drawn, has no glyphs, and what
        // is kept stays bounded.
        assert!(lean.layouts[450].as_ref().unwrap().stripped);
        assert!(kept(&lean) < 400, "{} paragraphs kept", kept(&lean));
    }

    #[test]
    fn print_preview_pages_get_their_glyphs() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let mut v = view(&mut fonts, false);
        let k = v.pages.len() / 2;
        v.prepare_pages(&mut fonts, &[k]);
        let first = v.pages[k].para;
        assert!(!v.layouts[first].as_ref().unwrap().stripped);
    }

    #[test]
    fn short_documents_keep_everything() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let mut v = DocView::new(quill_core::text::from_plain_text("One\nTwo"), "S".into());
        v.update_layout(&mut fonts);
        v.prepare(&mut fonts);
        assert!(v.layouts.iter().flatten().all(|l| !l.stripped));
    }
}
