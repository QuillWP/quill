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
    ///
    /// The tops are in order except inside a table row, where each cell's
    /// paragraphs start again at the row's top. So the search by top finds
    /// the place, and the range then takes in the whole row at each end: a
    /// row never splits across pages, so it is never more than a page.
    fn paras_between(&self, y0: f32, y1: f32) -> std::ops::Range<usize> {
        let n = self.layouts.len();
        if self.tops.len() != n + 1 {
            return 0..n;
        }
        // The paragraph before the first top inside the window may reach
        // into it, so it comes too.
        let a = self.tops[..n]
            .partition_point(|&t| t < y0)
            .saturating_sub(1);
        let b = self.tops[..n].partition_point(|&t| t <= y1).min(n);
        let paras = &self.doc.main.paras;
        let a = merged_start(paras, row_start(paras, a.min(n.saturating_sub(1))));
        let b = row_end(paras, b.max(a));
        a..b.min(n).max(a)
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

/// Whether `p` ends a row of the outermost table.
fn ends_outer_row(p: &quill_core::Paragraph) -> bool {
    p.mark == quill_core::Mark::RowEnd && p.pap.nest == 0
}

/// The first paragraph of the table row that paragraph `i` is in, or `i`
/// itself when it is not in a table.
fn row_start(paras: &[quill_core::Paragraph], mut i: usize) -> usize {
    while i > 0
        && paras.get(i).is_some_and(|p| p.pap.in_table)
        && paras[i - 1].pap.in_table
        && !ends_outer_row(&paras[i - 1])
    {
        i -= 1;
    }
    i
}

/// The first paragraph of the row where the cells merged down into the
/// row starting at `i` begin. Their text is in that row, but is drawn as
/// far down as the merge goes, so it is needed wherever the merge shows.
fn merged_start(paras: &[quill_core::Paragraph], mut i: usize) -> usize {
    let continues = |row_end: usize| {
        paras[row_end].row.as_ref().is_some_and(|r| {
            r.cells
                .iter()
                .any(|c| c.vmerge == quill_core::VMerge::Continue)
        })
    };
    while i > 0 && paras[i].pap.in_table && paras[i - 1].pap.in_table {
        let end = row_end(paras, i).saturating_sub(1);
        if !ends_outer_row(&paras[end]) || !continues(end) {
            break;
        }
        i = row_start(paras, i - 1);
    }
    i
}

/// One past the last paragraph of the table row that paragraph `i` starts
/// in or follows, or `i` itself when it is not in a table.
fn row_end(paras: &[quill_core::Paragraph], i: usize) -> usize {
    let mut j = i;
    if !paras.get(j).is_some_and(|p| p.pap.in_table) {
        return i;
    }
    while j < paras.len() && paras[j].pap.in_table && !ends_outer_row(&paras[j]) {
        j += 1;
    }
    (j + 1).min(paras.len())
}

#[cfg(test)]
mod tests {
    use quill_core::{CharProps, Mark, ParaProps, Paragraph};

    fn cell_para(mark: Mark) -> Paragraph {
        let pap = ParaProps {
            in_table: true,
            ..Default::default()
        };
        let mut p = Paragraph::with_text("x", CharProps::default(), pap);
        p.mark = mark;
        p
    }

    #[test]
    fn the_range_takes_in_the_whole_table_row_that_the_window_touches() {
        // 200 ordinary paragraphs, then one row of two cells (the first
        // holds ten paragraphs, the second one), then 200 more.
        let plain = || Paragraph::with_text("x", CharProps::default(), ParaProps::default());
        let mut paras: Vec<Paragraph> = (0..200).map(|_| plain()).collect();
        let row_first = paras.len();
        for _ in 0..9 {
            paras.push(cell_para(Mark::Paragraph));
        }
        paras.push(cell_para(Mark::CellEnd));
        paras.push(cell_para(Mark::CellEnd));
        paras.push(cell_para(Mark::RowEnd));
        let row_last = paras.len() - 1;
        paras.extend((0..200).map(|_| plain()));
        let n = paras.len();
        // Tops: ten points a paragraph, but the second cell starts again
        // at the row's top, and the paragraphs after it carry on from the
        // tall cell.
        let mut tops = Vec::new();
        let mut y = 0.0;
        for i in 0..n {
            if i == row_first + 10 {
                y = tops[row_first];
            }
            if i == row_last {
                y = tops[row_first] + 100.0;
            }
            tops.push(y);
            y += 10.0;
        }
        tops.push(y);
        let mut v = DocView::new(quill_core::Document::new(), "t".into());
        v.doc.main.paras = paras;
        v.layouts = (0..n).map(|_| None).collect();
        v.tops = tops.clone();
        // A window over the middle of the tall cell.
        let (y0, y1) = (tops[row_first] + 40.0, tops[row_first] + 60.0);
        let range = v.paras_between(y0, y1);
        assert!(
            range.start <= row_first && range.end > row_last,
            "{range:?} does not hold the row {row_first}..={row_last}"
        );
        // Away from tables, it is just the paragraphs in view and the one
        // before them.
        let range = v.paras_between(tops[20], tops[25]);
        assert_eq!(range, 19..26);
    }

    #[test]
    fn the_range_takes_in_the_row_a_merged_cell_starts_in() {
        use quill_core::{RowProps, VMerge};
        // Three rows of two cells; the first cell of the first row is
        // merged down through the other two, and its text is long.
        let plain = || Paragraph::with_text("x", CharProps::default(), ParaProps::default());
        let mut paras: Vec<Paragraph> = (0..50).map(|_| plain()).collect();
        let first_row = paras.len();
        for merge in [VMerge::Restart, VMerge::Continue, VMerge::Continue] {
            paras.push(cell_para(Mark::CellEnd));
            paras.push(cell_para(Mark::CellEnd));
            let mut end = cell_para(Mark::RowEnd);
            let mut row = RowProps::even(2, 8640);
            row.cells[0].vmerge = merge;
            end.row = Some(Box::new(row));
            paras.push(end);
        }
        let last_row = paras.len() - 3;
        paras.extend((0..50).map(|_| plain()));
        let n = paras.len();
        let mut tops: Vec<f32> = (0..=n).map(|i| i as f32 * 10.0).collect();
        // The rows sit 100 points apart, the merged text filling them.
        for r in 0..3 {
            for k in 0..3 {
                tops[first_row + 3 * r + k] = 500.0 + 100.0 * r as f32;
            }
        }
        for (i, t) in tops.iter_mut().enumerate().skip(last_row + 3) {
            *t = 800.0 + 10.0 * (i - last_row - 3) as f32;
        }
        let mut v = DocView::new(quill_core::Document::new(), "t".into());
        v.doc.main.paras = paras;
        v.layouts = (0..n).map(|_| None).collect();
        v.tops = tops;
        // A window over the last row only.
        let range = v.paras_between(720.0, 790.0);
        assert!(
            range.start <= first_row,
            "{range:?} misses the merge's first row"
        );
    }

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
