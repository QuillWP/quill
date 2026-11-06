//! Pagination, section geometry, headers, footers, and layout invalidation.

use super::*;

impl DocView {
    pub(super) fn options(&self) -> LayoutOptions {
        let p = &self.doc.props;
        LayoutOptions {
            width: if self.captions {
                (self.area.w - 2.0 * TEXT_LEFT).max(24.0)
            } else {
                p.text_width() as f32 / 20.0
            },
            default_tab: p.default_tab as f32 / 20.0,
            show_hidden: if self.printing {
                self.doc.props.print_hidden
            } else {
                self.show_marks || crate::settings::with(|s| s.show_hidden)
            },
            show_marks: self.show_marks && !self.printing,
            show_changes: self.show_changes,
            note_labels: self.note_labels.clone(),
            annotation_labels: self.annotation_labels.clone(),
            objects: std::sync::Arc::new(
                self.doc
                    .objects
                    .iter()
                    .map(|o| match o {
                        quill_core::Object::Picture(p) => {
                            (p.size.0 as f32 / 20.0, p.size.1 as f32 / 20.0)
                        }
                        quill_core::Object::Vacant => (0.0, 0.0),
                    })
                    .collect(),
            ),
            ..fields(1)
        }
    }

    /// Bring the layout cache up to date with the document.
    pub fn update_layout(&mut self, fonts: &mut Fonts) {
        self.layout_pass(fonts);
        // Hyphenate All reads every paragraph's glyphs.
        if self.hyph_job.is_some() {
            self.restore_all(fonts);
        }
        // Hyphenate All works a line at a time, each against the layout
        // the one before left.
        while self.hyphenate_step() {
            self.layout_pass(fonts);
        }
    }

    pub(super) fn layout_pass(&mut self, fonts: &mut Fonts) {
        let n = self.doc.main.paras.len();
        for ch in self.doc.take_changes() {
            if ch.story == StoryId::Notes {
                self.notes_dirty = true;
                continue;
            }
            if ch.story != MAIN {
                self.hf_dirty = true;
                continue;
            }
            let end = (ch.first + ch.removed).min(self.layouts.len());
            let first = ch.first.min(self.layouts.len());
            self.layouts
                .splice(first..end, std::iter::repeat_n(None, ch.inserted));
            let cend = end.min(self.collapsed.len());
            let cfirst = first.min(cend);
            self.collapsed
                .splice(cfirst..cend, std::iter::repeat_n(false, ch.inserted));
            let wend = end.min(self.wraps.len());
            let wfirst = first.min(wend);
            self.wraps
                .splice(wfirst..wend, std::iter::repeat_n(Vec::new(), ch.inserted));
            // Neighbors may join or leave a border box with the change.
            if first > 0 {
                self.layouts[first - 1] = None;
            }
            if let Some(next) = self.layouts.get_mut(first + ch.inserted) {
                *next = None;
            }
        }
        self.collapsed.resize(n, false);
        if let Some((first, flags)) = self.collapse_after.take() {
            for (k, f) in flags.into_iter().enumerate() {
                if let Some(slot) = self.collapsed.get_mut(first + k) {
                    *slot = f;
                }
            }
        }
        self.update_note_labels();
        let opts = self.options();
        if self.layouts.len() != n
            || (opts.width - self.laid_out_width).abs() > 0.01
            || opts.show_hidden != self.laid_out_hidden
            || opts.show_changes != self.laid_out_changes
        {
            self.layouts.clear();
            self.layouts.resize(n, None);
            self.wraps.clear();
            self.laid_out_width = opts.width;
            self.laid_out_hidden = opts.show_hidden;
            self.laid_out_changes = opts.show_changes;
        }
        let fonts_table = self.doc.fonts.clone();
        let name = |id: u16| fonts_table.get(id as usize).cloned().unwrap_or_default();
        let mut changed = self.tops.len() != n + 1 || std::mem::take(&mut self.outline_dirty);
        let secs = self.section_list();
        if secs != self.secs {
            self.secs = secs;
            changed = true;
        }
        // Paragraphs are as wide as their section's columns, and cells
        // are laid out at their own width.
        let mut want: Vec<f32> = (0..n).map(|i| self.column_twips(i) as f32 / 20.0).collect();
        let paras = &self.doc.main.paras;
        let mut i = 0;
        while i < n {
            if !paras[i].pap.in_table {
                i += 1;
                continue;
            }
            let Some(row) = table::row_at(paras, i) else {
                i += 1;
                continue;
            };
            let props = table::row_props(paras, &row, self.column_twips(i));
            for (k, &(a, b)) in row.cells.iter().enumerate() {
                let w = ((props.cells[k].width - props.gap) as f32 / 20.0).max(12.0);
                want[a..=b].fill(w);
            }
            want[row.end] = 12.0;
            i = row.end + 1;
        }
        // Outline view indents each paragraph by its level.
        if self.outline {
            for (i, w) in want.iter_mut().enumerate() {
                if !paras[i].pap.in_table {
                    *w = (*w - self.outline_x(i).1).max(36.0);
                }
            }
        }
        self.widths.resize(n, 0.0);
        for (i, &w) in want.iter().enumerate() {
            let stale = match &self.layouts[i] {
                Some(_) => (self.widths[i] - w).abs() > 0.01,
                None => true,
            };
            if stale {
                self.widths[i] = w;
                let mut l = self.layout_one(i, fonts, &opts, &name);
                if self.strip_now(i) {
                    l.strip();
                }
                self.layouts[i] = Some(l);
                changed = true;
            }
        }
        self.wraps.resize(n, Vec::new());
        if self.notes_dirty {
            self.layout_notes(fonts, &opts);
            changed = true;
        }
        if changed
            || self.frames_on() != self.placed_frames_on
            || self.page_view != self.placed_page_view
        {
            self.place_and_wrap(fonts, &opts);
            self.repaginate();
            self.toc_stale = true;
            // Frames placed by page need the new pages.
            if self.has_page_frames() {
                self.place_and_wrap(fonts, &opts);
                self.repaginate();
            }
        } else {
            let p = &self.doc.props;
            if self.laid_out_page
                != (
                    p.page_height,
                    p.margin_top,
                    p.margin_bottom,
                    p.widow_control,
                )
            {
                self.repaginate();
                self.toc_stale = true;
            }
        }
        // Header/footer height participates in pagination when a margin is
        // At Least. Page assignments choose first/even/ordinary headers,
        // so settle the small dependency loop until the body rooms stop
        // changing (normally one extra pass).
        self.layout_headers(fonts, &opts);
        for _ in 0..4 {
            let wanted = self.wanted_page_body_extra();
            let same = wanted.len() == self.page_body_extra.len()
                && wanted
                    .iter()
                    .zip(&self.page_body_extra)
                    .all(|(a, b)| (a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01);
            if same {
                break;
            }
            self.page_body_extra = wanted;
            if self.frames_on() {
                self.place_and_wrap(fonts, &opts);
            }
            self.repaginate();
            self.layout_headers(fonts, &opts);
        }
        // A new table of contents now has its pages: number it again.
        if self.toc_refresh.is_some() && self.refresh_toc() {
            self.update_layout(fonts);
            return;
        }
        // A live table of contents follows its headings and their pages. A
        // few rounds settle it; a contents that keeps changing its own
        // page numbers stops there.
        if std::mem::take(&mut self.toc_stale) {
            if self.toc_rounds < 3 && self.keep_toc_live() {
                self.toc_rounds += 1;
                self.update_layout(fonts);
                return;
            }
            self.toc_rounds = 0;
        }
        // Numbers that restart on each page depend on the pages just
        // worked out.
        if self.doc.props.footnote_restart_each_page && self.update_note_labels() {
            self.update_layout(fonts);
        }
    }

    /// The header and footer page `k` (from 0) uses.
    pub fn headers_for_page(&self, k: usize) -> (StoryId, StoryId) {
        if self.first_page_of_section(k) {
            (StoryId::FirstHeader, StoryId::FirstFooter)
        } else if self.doc.props.even_odd_headers && k % 2 == 1 {
            (StoryId::EvenHeader, StoryId::EvenFooter)
        } else {
            (StoryId::Header, StoryId::Footer)
        }
    }

    /// The header or footer `story` as page `k` shows it: its section's,
    /// or the one before's it shares.
    pub(super) fn page_header(&self, story: StoryId, k: usize) -> &quill_core::Story {
        self.doc.header_story(self.page_section_index(k), story)
    }

    /// The layout key for a header or footer on page `k`: which section's
    /// it is, and the page when it shows the page number.
    pub(super) fn hf_key(&self, story: StoryId, k: usize) -> (StoryId, usize, u32) {
        let source = self.doc.header_source(self.page_section_index(k));
        let numbered = self
            .page_header(story, k)
            .paras
            .iter()
            .any(|p| p.text.contains(chars::FIELD_PAGE));
        (story, source, if numbered { k as u32 + 1 } else { 0 })
    }

    /// Lay out the headers and footers the pages need.
    pub(super) fn layout_headers(&mut self, fonts: &mut Fonts, opts: &LayoutOptions) {
        if self.hf_dirty {
            self.hf.clear();
            self.hf_dirty = false;
        }
        let fonts_table = self.doc.fonts.clone();
        let name = |id: u16| fonts_table.get(id as usize).cloned().unwrap_or_default();
        for k in 0..self.pages.len() {
            let (h, f) = self.headers_for_page(k);
            for story in [h, f] {
                let key = self.hf_key(story, k);
                let paras = &self.page_header(story, k).paras;
                let empty = paras.len() == 1 && paras[0].text.is_empty();
                if empty || self.hf.contains_key(&key) {
                    continue;
                }
                let (number, format) = self.page_number(k);
                let o = LayoutOptions {
                    page_number: number,
                    page_format: format,
                    ..opts.clone()
                };
                let ls = self
                    .page_header(story, k)
                    .paras
                    .iter()
                    .map(|p| layout_paragraph_joined(p, &name, fonts, &o, Joins::default()))
                    .collect();
                self.hf.insert(key, ls);
            }
        }
    }

    /// Extra room a page's tall header/footer claims from the text body.
    /// Exact margins deliberately allow overlap, matching the Document
    /// dialog; At Least margins grow just enough to avoid it.
    fn wanted_page_body_extra(&self) -> Vec<(f32, f32)> {
        let p = &self.doc.props;
        let (margin_top, margin_bottom) =
            (p.margin_top as f32 / 20.0, p.margin_bottom as f32 / 20.0);
        (0..self.pages.len())
            .map(|k| {
                let sec = self.page_section(k);
                let (header, footer) = self.headers_for_page(k);
                let height = |story| {
                    self.hf
                        .get(&self.hf_key(story, k))
                        .map(|layouts| layouts.iter().map(|layout| layout.height).sum::<f32>())
                };
                let top = if p.top_exact {
                    0.0
                } else {
                    height(header).map_or(0.0, |h| {
                        (sec.header_from_top as f32 / 20.0 + h - margin_top).max(0.0)
                    })
                };
                let bottom = if p.bottom_exact {
                    0.0
                } else {
                    height(footer).map_or(0.0, |h| {
                        (sec.footer_from_bottom as f32 / 20.0 + h - margin_bottom).max(0.0)
                    })
                };
                (top, bottom)
            })
            .collect()
    }

    pub(super) fn page_body_top(&self, k: usize) -> f32 {
        self.doc.props.margin_top as f32 / 20.0
            + self.page_body_extra.get(k).map_or(0.0, |extra| extra.0)
    }

    pub(super) fn page_body_bottom_margin(&self, k: usize) -> f32 {
        self.doc.props.margin_bottom as f32 / 20.0
            + self.page_body_extra.get(k).map_or(0.0, |extra| extra.1)
    }

    pub(super) fn page_body_height(&self, k: usize) -> f32 {
        (self.doc.props.page_height as f32 / 20.0
            - self.page_body_top(k)
            - self.page_body_bottom_margin(k))
        .max(36.0)
    }

    /// The header or footer under a window point in Page Layout view, and
    /// the section of its page: a click above or below a page's text opens
    /// it, as in the original.
    pub fn header_at(&self, wx: f32, wy: f32) -> Option<(StoryId, usize)> {
        if !self.page_view || self.pages.is_empty() {
            return None;
        }
        let (pw, ph) = self.page_size();
        let left = self.area.x + self.page_left();
        if wx < left || wx > left + pw {
            return None;
        }
        let vy = wy - self.area.y + self.scroll_y;
        let k = ((vy - PAGE_GAP / 2.0) / (ph + PAGE_GAP)).floor().max(0.0) as usize;
        if k >= self.pages.len() {
            return None;
        }
        let local = vy - self.page_top(k);
        let (h, f) = self.headers_for_page(k);
        let section = self.page_section_index(k);
        if (0.0..self.page_body_top(k)).contains(&local) {
            Some((h, section))
        } else if local > ph - self.page_body_bottom_margin(k) && local <= ph {
            Some((f, section))
        } else {
            None
        }
    }

    /// Draw page `k`'s header and footer, with the page's top at `top`.
    pub(super) fn draw_headers(
        &self,
        c: &mut dyn Surface,
        fonts: &Fonts,
        cache: &mut GlyphCache,
        k: usize,
        ox: f32,
        top: f32,
    ) {
        let (_, ph) = self.page_size();
        let (h, f) = self.headers_for_page(k);
        let sec = self.page_section(k);
        for story in [h, f] {
            let Some(ls) = self.hf.get(&self.hf_key(story, k)) else {
                continue;
            };
            let total: f32 = ls.iter().map(|l| l.height).sum();
            let mut y = if story.is_footer() {
                top + ph - sec.footer_from_bottom as f32 / 20.0 - total
            } else {
                top + sec.header_from_top as f32 / 20.0
            };
            for (p, l) in self.page_header(story, k).paras.iter().zip(ls) {
                if let Some(b) = &l.border {
                    draw_border_box(c, b, ox, y);
                }
                paint_paragraph(
                    c,
                    fonts,
                    cache,
                    p,
                    l,
                    ox,
                    y,
                    false,
                    self.laid_out_width,
                    &self.doc.objects,
                );
                y += l.height;
            }
        }
    }

    /// Work out where every paragraph sits: one under another, and table
    /// cells side by side with each row as tall as its tallest cell.
    pub(super) fn place(&mut self) {
        let paras = &self.doc.main.paras;
        let n = paras.len();
        let height = |i: usize| self.layouts[i].as_ref().map_or(0.0, |l| l.height);
        let mut tops = vec![0.0; n + 1];
        let mut xs = vec![0.0; n];
        let mut blocks = Vec::new();
        let frames_on = self.frames_on();
        let mut frames = Vec::new();
        let mut clears: Vec<quill_layout::Clear> = Vec::new();
        let mut want_wrap = vec![Vec::new(); n];
        let end_groups = self.end_note_groups();
        let mut endnotes = Vec::new();
        let mut y = 0.0;
        let mut pads = Vec::new();
        let pad_marks = !self.page_view && !self.outline;
        let mut end_notes = |y: &mut f32, after: usize| {
            for (_, ids) in end_groups.iter().filter(|g| g.0 == after) {
                let height =
                    notes::NOTE_SEP + ids.iter().map(|&id| self.note_height(id)).sum::<f32>();
                endnotes.push(notes::EndNotes {
                    after,
                    y: *y,
                    ids: ids.clone(),
                    height,
                });
                *y += height;
            }
        };
        let mut i = 0;
        while i < n {
            if i > 0 {
                end_notes(&mut y, i - 1);
            }
            let row = paras[i]
                .pap
                .in_table
                .then(|| table::row_at(paras, i))
                .flatten();
            // A frame: its paragraphs go where the frame says, and the text
            // after them flows round it.
            if frames_on
                && row.is_none()
                && let Some(f) = self.place_frame(i, y)
            {
                let mut fy = f.y;
                for p in f.first..=f.last {
                    tops[p] = fy;
                    xs[p] = f.x;
                    fy += height(p);
                }
                clears.push(f.clear());
                i = f.last + 1;
                frames.push(f);
                continue;
            }
            // Outline view: collapsed paragraphs take no room, and body
            // text may show its first line only.
            if self.outline_hidden(i) {
                let last = row.as_ref().map_or(i, |r| r.end);
                tops[i..=last].fill(y);
                i = last + 1;
                continue;
            }
            if self.outline && row.is_none() {
                xs[i] = self.outline_x(i).1;
                let first_only = self.first_line_only && self.level(i) == 10;
                let h = match self.layouts[i].as_ref() {
                    Some(l) if first_only && !l.lines.is_empty() => {
                        l.lines[0].top + l.lines[0].height
                    }
                    _ => height(i),
                };
                tops[i] = y;
                blocks.push(Block {
                    first: i,
                    last: i,
                    top: y,
                    height: h,
                    row: None,
                });
                y += h;
                i += 1;
                continue;
            }
            let Some(row) = row else {
                if !clears.is_empty() {
                    let h = height(i);
                    want_wrap[i] = clears
                        .iter()
                        .filter(|c| c.bottom > y && c.top < y + h.max(1.0))
                        .map(|c| quill_layout::Clear {
                            top: c.top - y,
                            bottom: c.bottom - y,
                            ..*c
                        })
                        .collect();
                }
                tops[i] = y;
                blocks.push(Block {
                    first: i,
                    last: i,
                    top: y,
                    height: height(i),
                    row: None,
                });
                y += height(i);
                // Normal view: room under a section's last paragraph for
                // its mark, so the mark's label covers no text.
                if pad_marks && paras[i].mark == Mark::SectionBreak && i + 1 < n {
                    pads.push(y);
                    y += SECTION_PAD;
                }
                i += 1;
                continue;
            };
            let text_width = self.column_twips(i);
            let props = table::row_props(paras, &row, text_width);
            let lefts = props.lefts(text_width);
            let pad = props.gap as f32 / 40.0;
            let mut tallest: f32 = 0.0;
            let mut cells = Vec::new();
            for (k, &(a, b)) in row.cells.iter().enumerate() {
                let x0 = lefts[k] as f32 / 20.0;
                let x1 = x0 + props.cells[k].width as f32 / 20.0;
                let mut cy = y;
                for p in a..=b {
                    tops[p] = cy;
                    xs[p] = x0 + pad;
                    cy += height(p);
                }
                tallest = tallest.max(cy - y);
                cells.push((a, b, x0, x1));
            }
            let h = match props.height {
                quill_core::RowHeight::Auto => tallest,
                quill_core::RowHeight::AtLeast(t) => tallest.max(t as f32 / 20.0),
                quill_core::RowHeight::Exactly(t) => t as f32 / 20.0,
            };
            tops[row.end] = y;
            xs[row.end] = cells.last().map_or(0.0, |c| c.3) + pad;
            blocks.push(Block {
                first: row.first,
                last: row.end,
                top: y,
                height: h,
                row: Some(RowGeom { cells, props }),
            });
            y += h;
            i = row.end + 1;
        }
        end_notes(&mut y, n - 1);
        tops[n] = y;
        self.endnotes = endnotes;
        self.tops = tops;
        self.xs = xs;
        self.blocks = blocks;
        self.frames = frames;
        self.want_wrap = want_wrap;
        self.placed_frames_on = frames_on;
        self.section_pads = pads;
        self.placed_page_view = self.page_view;
    }

    /// The block holding continuous y `y`.
    pub(super) fn block_at(&self, y: f32) -> usize {
        self.blocks
            .partition_point(|b| b.top <= y)
            .saturating_sub(1)
            .min(self.blocks.len().saturating_sub(1))
    }

    /// Break the laid-out text into pages.
    /// `y` down the text as pages count it: without Normal view's room
    /// for section marks, so every view numbers pages alike.
    pub(super) fn unpad(&self, y: f32) -> f32 {
        let mut out = y;
        for &s in &self.section_pads {
            if y >= s + SECTION_PAD {
                out -= SECTION_PAD;
            } else if y > s {
                out -= y - s;
            }
        }
        out
    }

    /// The other way: where page-counting `y` is down the text as shown.
    pub(super) fn pad(&self, y: f32) -> f32 {
        let mut v = y;
        for &s in &self.section_pads {
            if v >= s {
                v += SECTION_PAD;
            }
        }
        v
    }

    pub(super) fn repaginate(&mut self) {
        let p = &self.doc.props;
        let base_height = (p.page_height - p.margin_top - p.margin_bottom).max(720) as f32 / 20.0;
        let mut page_heights: Vec<f32> = self
            .page_body_extra
            .iter()
            .map(|(top, bottom)| (base_height - top - bottom).max(36.0))
            .collect();
        if page_heights.is_empty() {
            page_heights.push(base_height);
        }
        // Lines of paragraphs, and table rows whole.
        let mut units = Vec::new();
        for b in &self.blocks {
            match &b.row {
                None => units.extend(paginate::line_units(
                    b.first,
                    self.layout(b.first),
                    self.unpad(b.top),
                )),
                Some(_) => units.push(paginate::Unit {
                    para: b.first,
                    line: 0,
                    top: self.unpad(b.top),
                    bottom: self.unpad(b.top) + b.height,
                    first: true,
                    last: true,
                    hard_break: false,
                    notes: 0.0,
                }),
            }
        }
        for u in self.end_note_units() {
            let k = units.partition_point(|x| x.para <= u.para);
            units.insert(k, u);
        }
        self.note_room(&mut units);
        let keeps: Vec<Keeps> = self
            .doc
            .main
            .paras
            .iter()
            .map(|p| Keeps {
                page_break_before: p.pap.page_break_before,
                keep_with_next: p.pap.keep_with_next,
                keep_together: p.pap.keep_together,
            })
            .collect();
        let text_width = p.text_width();
        let sections: Vec<paginate::SectionLayout> = self
            .secs
            .iter()
            .map(|(first, s)| paginate::SectionLayout {
                first_para: *first,
                start: s.start,
                columns: s.columns.max(1) as usize,
                column_width: sections::column_width(text_width, s) as f32 / 20.0,
                spacing: s.column_spacing as f32 / 20.0,
            })
            .collect();
        self.bands = paginate::paginate_sections(
            &units,
            &keeps,
            &sections,
            &page_heights,
            p.widow_control,
            notes::NOTE_SEP,
        );
        self.pages = paginate::page_starts(&self.bands);
        self.laid_out_page = (
            p.page_height,
            p.margin_top,
            p.margin_bottom,
            p.widow_control,
        );
    }
}
