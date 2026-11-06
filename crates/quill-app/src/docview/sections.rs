//! Sections and columns (Format > Section): which settings each part of
//! the text has, how wide its columns are, and where each column's share
//! of the text sits on its page. Normal view shows a section's text one
//! column wide, as the original does; Page Layout view, Print Preview and
//! printing put the columns side by side.

use super::*;
use quill_core::{PageNumberFormat, SectionProps};
use quill_layout::paginate::Band;

/// Per-page section facts for a whole layout pass.
pub(super) struct PageSections {
    /// The section each page starts in.
    pub section: Vec<usize>,
    /// Whether the page gets its section's first page header.
    pub first_page: Vec<bool>,
    /// The number printed on each page and how it is written.
    pub number: Vec<(u32, PageNumberFormat)>,
}

impl DocView {
    /// Each section's first paragraph and settings.
    pub(super) fn section_list(&self) -> Vec<(usize, SectionProps)> {
        self.doc
            .sections()
            .into_iter()
            .map(|r| (*r.start(), self.doc.section_props_at(*r.start()).clone()))
            .collect()
    }

    /// The section (from 0) paragraph `para` is in, from the last layout.
    pub(super) fn section_index(&self, para: usize) -> usize {
        self.secs
            .partition_point(|(first, _)| *first <= para)
            .saturating_sub(1)
    }

    /// The width of a column in the section holding paragraph `para`.
    pub(super) fn column_twips(&self, para: usize) -> i32 {
        if self.captions {
            return ((self.area.w - 2.0 * TEXT_LEFT).max(24.0) * 20.0) as i32;
        }
        let width = self.doc.props.text_width();
        let Some((_, s)) = self.secs.get(self.section_index(para)) else {
            return width;
        };
        column_width(width, s)
    }

    /// The section page `k` starts in.
    pub(super) fn page_section_index(&self, k: usize) -> usize {
        self.bands
            .iter()
            .find(|b| b.page >= k)
            .map_or(0, |b| b.section)
    }

    /// The settings that apply to page `k`: its header and footer, page
    /// numbers and so on come from the section it starts in.
    pub fn page_section(&self, k: usize) -> &SectionProps {
        self.secs
            .get(self.page_section_index(k))
            .map_or(&self.doc.section, |(_, s)| s)
    }

    /// Whether page `k` is the first page of its section.
    fn starts_section(&self, k: usize) -> bool {
        let s = self.page_section_index(k);
        k == 0
            || self
                .bands
                .iter()
                .find(|b| b.section == s)
                .is_some_and(|b| b.page >= k)
    }

    /// Whether page `k` gets its section's first page header.
    pub(super) fn first_page_of_section(&self, k: usize) -> bool {
        self.page_section(k).different_first_page && self.starts_section(k)
    }

    /// The number printed on page `k` and how it is written. Numbering
    /// starts again at 1 in a section set to restart.
    pub fn page_number(&self, k: usize) -> (u32, PageNumberFormat) {
        let mut n = 0;
        for page in 0..=k {
            n = if self.page_section(page).restart_page_numbers && self.starts_section(page) {
                1
            } else {
                n + 1
            };
        }
        (n, self.page_section(k).page_number_format)
    }

    /// What `page_section_index`, `headers_for_page` and `page_number` say
    /// for every page, worked out in one sweep for loops over all pages.
    pub(super) fn page_sections(&self) -> PageSections {
        let pages = self.pages.len();
        let sections = self.bands.iter().map(|b| b.section + 1).max().unwrap_or(0);
        let mut first_band = vec![usize::MAX; sections];
        for b in &self.bands {
            if let Some(slot) = first_band.get_mut(b.section)
                && *slot == usize::MAX
            {
                *slot = b.page;
            }
        }
        let mut out = PageSections {
            section: Vec::with_capacity(pages),
            first_page: Vec::with_capacity(pages),
            number: Vec::with_capacity(pages),
        };
        let mut band = 0;
        let mut n = 0;
        for k in 0..pages {
            while band < self.bands.len() && self.bands[band].page < k {
                band += 1;
            }
            let s = self.bands.get(band).map_or(0, |b| b.section);
            let props = self.secs.get(s).map_or(&self.doc.section, |(_, p)| p);
            let starts = k == 0
                || first_band
                    .get(s)
                    .is_some_and(|&p| p != usize::MAX && p >= k);
            n = if props.restart_page_numbers && starts {
                1
            } else {
                n + 1
            };
            out.section.push(s);
            out.first_page.push(props.different_first_page && starts);
            out.number.push((n, props.page_number_format));
        }
        out
    }

    /// The width of the column the insertion point is in.
    pub fn caret_column_twips(&self) -> i32 {
        self.column_twips(self.sel.focus.para)
    }

    /// The window x of the left edge of the column the insertion point
    /// is in, for the ruler.
    pub fn column_left_window(&self) -> f32 {
        let ready = self
            .layouts
            .get(self.sel.focus.para)
            .is_some_and(Option::is_some);
        if !self.page_view || !ready {
            return self.text_left_window();
        }
        let (_, y, _) = self.caret_geometry(self.sel.focus, self.upstream);
        self.text_left_window() + self.on_page(y).1
    }

    /// The settings of the section the insertion point is in.
    pub fn current_section(&self) -> &SectionProps {
        self.doc.section_props_at(self.sel.focus.para)
    }

    /// Change the settings of every section the selection touches, as one
    /// undoable step.
    pub fn format_sections(&mut self, f: impl FnMut(&mut SectionProps)) {
        let (s, e) = self.sel.range();
        let sel = self.sel;
        self.doc.begin("Format Section", sel);
        self.doc.format_sections(s.para, e.para, f);
        self.doc.commit(sel);
    }

    /// The column band holding continuous y `y`.
    pub(super) fn band_at(&self, y: f32) -> Option<&Band> {
        let y = self.unpad(y);
        let k = self.bands.partition_point(|b| b.y <= y).checked_sub(1)?;
        self.bands.get(k)
    }

    /// Where continuous y `y` is on its page: the page, and the band's
    /// offset across and down the page's text area.
    pub(super) fn on_page(&self, y: f32) -> (usize, f32, f32) {
        match self.band_at(y) {
            Some(b) => (b.page, b.x, b.top + (y - b.y)),
            None => (0, 0.0, y),
        }
    }

    /// The continuous y of a point `local` down page `k`'s text area, in
    /// its first column.
    pub(super) fn doc_y_on_page(&self, k: usize, local: f32) -> f32 {
        let on = self.bands.iter().filter(|b| b.page == k);
        let b = on
            .clone()
            .rev()
            .find(|b| b.x == 0.0 && b.top <= local)
            .or_else(|| on.clone().next())
            .or_else(|| self.bands.iter().find(|b| b.page > k));
        match b {
            Some(b) => b.y + local - b.top,
            None => local,
        }
    }

    /// The document point under a point (`x` across from the left margin,
    /// `local` down from the top margin) of page `k`'s text area: the
    /// column under it, or the nearest.
    pub(super) fn doc_at_page(&self, k: usize, x: f32, local: f32) -> (f32, f32) {
        let on: Vec<&Band> = self.bands.iter().filter(|b| b.page == k).collect();
        let Some(&first) = on.first() else {
            // A blank page: the start of the next one.
            let y = self.bands.iter().find(|b| b.page > k).map_or(0.0, |b| b.y);
            return (x, y);
        };
        // The row of columns the point is in, then the column across.
        let row_top = on
            .iter()
            .map(|b| b.top)
            .filter(|&t| t <= local)
            .fold(first.top, f32::max);
        let row: Vec<&Band> = on.into_iter().filter(|b| b.top == row_top).collect();
        let b = row
            .iter()
            .rev()
            .find(|b| x >= b.x - 6.0)
            .copied()
            .unwrap_or(row[0]);
        let y = (b.y + local - b.top).clamp(b.y, (b.end - 0.01).max(b.y));
        (x - b.x, y)
    }

    /// The bands on page `k`.
    pub(super) fn page_bands(&self, k: usize) -> impl Iterator<Item = &Band> {
        self.bands.iter().filter(move |b| b.page == k)
    }

    /// Normal view's section mark: a double dotted line across the window
    /// under the paragraph that ends the section.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_section_mark(
        &self,
        c: &mut dyn Surface,
        fonts: &Fonts,
        cache: &mut GlyphCache,
        para: usize,
        x0: f32,
        x1: f32,
        y: f32,
    ) {
        if self.doc.main.paras[para].mark != Mark::SectionBreak || para + 1 >= self.layouts.len() {
            return;
        }
        let label = section_break_label(self.doc.section_props_at(para + 1).start);
        let face = self.layouts[para]
            .as_ref()
            .and_then(|l| l.glyphs.last())
            .map(|g| g.face)
            .unwrap_or_default();
        super::drawing::draw_break_rule(c, fonts, cache, face, x0, x1, y, label, true);
    }
}

fn section_break_label(start: quill_core::SectionStart) -> &'static str {
    use quill_core::SectionStart::*;
    match start {
        NoBreak => "Section Break (Continuous)",
        NewColumn => "Section Break (New Column)",
        NewPage => "Section Break (Next Page)",
        EvenPage => "Section Break (Even Page)",
        OddPage => "Section Break (Odd Page)",
    }
}

/// The width of one of section `s`'s columns, in a text area `width` wide.
pub fn column_width(width: i32, s: &SectionProps) -> i32 {
    let n = s.columns.max(1) as i32;
    ((width - (n - 1) * s.column_spacing) / n).max(144)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Cmd;

    #[test]
    fn section_break_deletes_in_both_directions_and_undo_restores_settings() {
        for backspace in [true, false] {
            let mut v = DocView::new(Document::new(), "T".into());
            let clip = &mut Clipboard::new();
            v.type_text("Head");
            v.exec(Cmd::SectionBreak, false, clip);
            v.type_text("Body");
            v.sel = Selection::caret(Position::new(0, 0));
            v.format_sections(|s| s.columns = 2);
            let before = v.doc.main.paras.clone();
            v.sel = Selection::caret(if backspace {
                Position::new(1, 0)
            } else {
                Position::new(0, 4)
            });
            v.exec(
                if backspace {
                    Cmd::Backspace
                } else {
                    Cmd::DeleteForward
                },
                false,
                clip,
            );
            assert_eq!(v.doc.main.paras.len(), 1);
            assert_eq!(v.doc.main.paras[0].text, "HeadBody");
            assert_eq!(
                v.current_section().columns,
                1,
                "the following section's settings survive"
            );
            v.exec(Cmd::Undo, false, clip);
            assert_eq!(v.doc.main.paras, before);
        }
    }

    #[test]
    fn section_break_after_an_empty_paragraph() {
        let mut v = DocView::new(quill_core::Document::new(), "T".into());
        let clip = &mut Clipboard::new();
        v.type_text("Head");
        v.exec(Cmd::NewParagraph, false, clip);
        v.exec(Cmd::SectionBreak, false, clip);
        v.type_text("Body");
        let p: Vec<_> = v
            .doc
            .main
            .paras
            .iter()
            .map(|p| (p.text.clone(), p.mark))
            .collect();
        assert_eq!(
            p,
            [
                ("Head".to_string(), Mark::Paragraph),
                (String::new(), Mark::SectionBreak),
                ("Body".to_string(), Mark::Paragraph)
            ]
        );
    }
}
