//! Insert > Table of Contents, Index, Index Entry and TOC Entry, and line
//! numbers in the margin (Section > Line Numbers). Page numbers come from
//! the current pagination; the table of contents is made again once its
//! own pages are in place, so the numbers after it are right.

use super::*;
use quill_core::LineNumbering;
use quill_core::contents::{IndexOptions, TocOptions};

/// The original's automatic distance of line numbers from the text: 0.25 in.
const LINE_NUMBER_GAP: f32 = 18.0;

impl DocView {
    /// The page `at` is on: its index and its number as printed.
    pub(super) fn page_label(&self, at: Position) -> (usize, String) {
        let Some(Some(l)) = self.layouts.get(at.para) else {
            return (0, "1".into());
        };
        let line = &l.lines[l.line_of(at.offset, false)];
        let y = self.tops[at.para] + line.top;
        let k = self.band_at(y).map_or(0, |b| b.page);
        let (n, format) = self.page_number(k);
        (k, format.text(n))
    }

    fn toc_paragraphs(&self, o: &TocOptions) -> Vec<Paragraph> {
        let entries: Vec<_> = self
            .doc
            .toc_entries(o)
            .into_iter()
            .map(|e| {
                let label = self.page_label(e.at).1;
                (e, label)
            })
            .collect();
        self.doc.toc_paragraphs(&entries, o.page_numbers)
    }

    /// Start in the Table of Contents dialog: the contents go at the start
    /// of the document. Its page numbers are worked out again once the
    /// contents' own pages are laid out.
    pub fn insert_toc(&mut self, o: TocOptions) {
        let paras = self.toc_paragraphs(&o);
        self.doc.begin("Table of Contents", self.sel);
        self.doc.place_toc(paras);
        self.doc.change_document_props(quill_core::DocumentProps {
            live_toc: Some(o),
            ..self.doc.props.clone()
        });
        self.toc_refresh = Some(o);
        self.sel = Selection::caret(Position::new(0, 0));
    }

    /// The second pass of `insert_toc`, from `update_layout`. Returns true
    /// if the contents changed.
    pub(super) fn refresh_toc(&mut self) -> bool {
        let Some(o) = self.toc_refresh.take() else {
            return false;
        };
        let paras = self.toc_paragraphs(&o);
        let old = self.doc.existing_toc().map_or(0, |e| e + 1);
        let same = paras.len() == old
            && paras
                .iter()
                .zip(&self.doc.main.paras)
                .all(|(a, b)| a.text == b.text);
        if !same {
            self.doc.place_toc(paras);
        }
        let sel = Selection::caret(Position::new(0, 0));
        self.doc.commit(sel);
        self.set_selection(sel);
        self.scroll_y = 0.0;
        !same
    }

    /// Bring a live table of contents up to date with its headings and
    /// their pages, as part of the edit that changed them. Typing in it,
    /// or formatting it, is undone the same way. Returns true if it
    /// changed. A contents that is no longer whole (deleted, or its
    /// section break gone) stops being live.
    pub(super) fn keep_toc_live(&mut self) -> bool {
        let Some(o) = self.doc.props.live_toc else {
            return false;
        };
        let Some(end) = self.doc.existing_toc() else {
            self.doc.continue_last("Table of Contents", self.sel);
            self.doc.change_document_props(quill_core::DocumentProps {
                live_toc: None,
                ..self.doc.props.clone()
            });
            self.doc.commit(self.sel);
            return false;
        };
        let paras = self.toc_paragraphs(&o);
        let same = paras.len() == end + 1
            && paras.iter().zip(&self.doc.main.paras).all(|(a, b)| {
                a.text == b.text
                    && a.pap.style == b.pap.style
                    && a.run_bounds()
                        .map(|r| *r.2)
                        .eq(b.run_bounds().map(|r| *r.2))
            });
        if same {
            return false;
        }
        // Typing or formatting in it goes back, with a word why.
        if self.in_live_toc() {
            self.status = Some(
                "The table of contents keeps itself up to date. To change it by hand, right-click it and choose Make Editable."
                    .into(),
            );
        }
        self.doc.continue_last("Table of Contents", self.sel);
        let n = self.doc.place_toc(paras);
        // Keep the selection on the same text after the contents.
        let shift = |p: Position| {
            if p.para > end {
                Position::new(p.para + n - (end + 1), p.offset)
            } else {
                Position::new(0, 0)
            }
        };
        let sel = Selection::new(shift(self.sel.anchor), shift(self.sel.focus));
        self.sel = sel;
        self.doc.commit(sel);
        true
    }

    /// Whether the selection is in a live table of contents.
    pub fn in_live_toc(&self) -> bool {
        self.doc.props.live_toc.is_some()
            && self
                .doc
                .existing_toc()
                .is_some_and(|end| self.sel.start().para <= end)
    }

    /// Update Table of Contents: make it again now, with the settings it
    /// was made with (or the usual ones), and keep it live.
    pub fn update_toc(&mut self) {
        let o = self.doc.props.live_toc.unwrap_or_default();
        if self.doc.existing_toc().is_none() {
            return;
        }
        self.insert_toc(o);
    }

    /// Make Editable: the table of contents becomes ordinary text, kept
    /// as it is, to change by hand. It no longer updates itself.
    pub fn make_toc_editable(&mut self) {
        if self.doc.props.live_toc.is_none() {
            return;
        }
        let props = quill_core::DocumentProps {
            live_toc: None,
            ..self.doc.props.clone()
        };
        self.doc
            .set_document_props("Make Editable", props, self.sel);
        self.status = Some("The table of contents is now ordinary text.".into());
    }

    /// Start in the Index dialog: the index goes at the end of the
    /// document, after a section break.
    pub fn insert_index(&mut self, o: IndexOptions) {
        let entries: Vec<_> = self
            .doc
            .index_entries()
            .into_iter()
            .map(|e| {
                let (k, label) = self.page_label(e.at);
                (e, k, label)
            })
            .collect();
        let paras = self.doc.index_paragraphs(&entries, &o);
        let sel = self.sel;
        self.doc.begin("Index", sel);
        let start = self.doc.place_index(paras);
        self.finish_edit(Selection::caret(Position::new(start, 0)));
    }

    /// Insert > Index Entry (`kind` 'i') and TOC Entry ('c'): hidden
    /// `.i.` before the selection and `;` after it, so the selected text is
    /// the entry. With nothing selected the insertion point goes between
    /// them, in hidden text, to type the entry.
    pub fn insert_entry(&mut self, kind: char) {
        let (s, e) = self.sel.range();
        if s.para != e.para {
            return;
        }
        let mut chp = self
            .typing_chp
            .unwrap_or_else(|| self.doc.main.paras[s.para].chp_at(s.offset));
        chp.hidden = true;
        chp.object = 0;
        let open = format!(".{kind}.");
        self.doc.begin(
            if kind == 'i' {
                "Index Entry"
            } else {
                "TOC Entry"
            },
            self.sel,
        );
        let after = self.doc.insert_text(MAIN, e, ";", chp);
        self.doc.insert_text(MAIN, s, &open, chp);
        let caret = if s == e {
            Position::new(s.para, s.offset + open.len())
        } else {
            Position::new(after.para, after.offset + open.len())
        };
        self.finish_edit(Selection::caret(caret));
        if s == e {
            self.typing_chp = Some(chp);
        }
    }

    /// Page `k`'s line numbers: where each goes (its right edge across
    /// the text area and its baseline down it) and the number.
    pub(super) fn line_numbers(&self, k: usize) -> Vec<(f32, f32, u32)> {
        let mut out = Vec::new();
        if !self
            .secs
            .iter()
            .any(|(_, s)| s.line_numbers != LineNumbering::Off)
        {
            return out;
        }
        let mut n = 0u32;
        let (mut last_page, mut last_sec) = (None, None);
        for b in &self.bands {
            if b.page > k {
                break;
            }
            let Some((_, sec)) = self.secs.get(b.section) else {
                continue;
            };
            let restart = match sec.line_numbers {
                LineNumbering::Off => continue,
                LineNumbering::ByPage => last_page != Some(b.page),
                LineNumbering::BySection => last_sec != Some(b.section),
                LineNumbering::Continuous => false,
            };
            if restart {
                n = 0;
            }
            last_page = Some(b.page);
            last_sec = Some(b.section);
            let gap = sec
                .line_from_text
                .map_or(LINE_NUMBER_GAP, |t| t as f32 / 20.0);
            let every = sec.line_count_by.max(1) as u32;
            for block in &self.blocks[self.block_at(b.y)..] {
                if block.top >= b.end {
                    break;
                }
                if block.row.is_some() {
                    continue;
                }
                let i = block.first;
                if self.doc.main.paras[i].pap.suppress_line_numbers {
                    continue;
                }
                for line in &self.layout(i).lines {
                    let y = self.tops[i] + line.top;
                    if y < b.y || y >= b.end {
                        continue;
                    }
                    n += 1;
                    if b.page == k && n.is_multiple_of(every) {
                        out.push((b.x - gap, b.top + (y - b.y) + line.baseline, n));
                    }
                }
            }
        }
        out
    }

    /// Draw page `k`'s line numbers in the line number style, with the
    /// page's text area at (`ox`, `oy`).
    pub(super) fn draw_line_numbers(
        &self,
        c: &mut dyn Surface,
        fonts: &Fonts,
        cache: &mut GlyphCache,
        k: usize,
        ox: f32,
        oy: f32,
    ) {
        let marks = self.line_numbers(k);
        if marks.is_empty() {
            return;
        }
        let style = self.doc.styles.find("line number").unwrap_or(0);
        let chp = self.doc.styles.get(style).chp;
        let face = fonts.find_resolved(self.doc.font_name(chp.font), chp.bold, chp.italic);
        let size = chp.size as f32 / 2.0;
        for (right, base, n) in marks {
            let text = n.to_string();
            let ids: Vec<_> = text.chars().map(|ch| fonts.glyph_in(face, ch)).collect();
            let w: f32 = ids.iter().map(|&id| fonts.advance(face, id, size)).sum();
            let mut x = ox + right - w;
            for id in ids {
                c.glyph(fonts, cache, face, id, size, x, oy + base, color::BLACK);
                x += fonts.advance(face, id, size);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_of_contents_is_one_undo_step() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let mut doc = quill_core::text::from_plain_text("Chapter One\ntext\nChapter Two");
        let h1 = doc.styles.find("heading 1").unwrap();
        doc.apply_style(MAIN, 0, 0, h1);
        doc.apply_style(MAIN, 2, 2, h1);
        doc.forget_history();
        let mut v = DocView::new(doc, "T".into());
        v.update_layout(&mut fonts);
        v.insert_toc(TocOptions::default());
        v.update_layout(&mut fonts);
        assert_eq!(v.doc.main.paras[0].text, "Chapter One\t1");
        assert_eq!(v.doc.main.paras.len(), 5);
        v.doc.undo();
        assert_eq!(v.doc.main.paras.len(), 3);
        assert_eq!(v.doc.main.paras[0].text, "Chapter One");
    }

    #[test]
    fn room_for_section_marks_leaves_pages_alone() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let text = (0..240)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut doc = quill_core::text::from_plain_text(&text);
        for k in (15..240).step_by(20) {
            let p = &mut doc.main.paras[k];
            p.mark = quill_core::Mark::SectionBreak;
            p.section = Some(Box::new(quill_core::SectionProps {
                start: quill_core::SectionStart::NoBreak,
                ..doc.section.clone()
            }));
        }
        let mut v = DocView::new(doc, "T".into());
        let mut by_view = Vec::new();
        for page_view in [false, true] {
            v.page_view = page_view;
            v.update_layout(&mut fonts);
            let starts: Vec<usize> = v.pages.iter().map(|p| p.para).collect();
            let label = v.page_label(Position::new(239, 0)).1;
            by_view.push((starts, label));
        }
        assert!(by_view[0].0.len() > 2);
        assert_eq!(by_view[0], by_view[1]);
        // Normal view has the room under each section's last line.
        v.page_view = false;
        v.update_layout(&mut fonts);
        let gap = v.tops[16] - v.tops[15] - v.layout(15).height;
        assert!((gap - SECTION_PAD).abs() < 0.01);
    }

    #[test]
    fn a_live_table_of_contents_follows_its_headings() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let mut doc = quill_core::text::from_plain_text("Chapter One\ntext\nChapter Two");
        let h1 = doc.styles.find("heading 1").unwrap();
        doc.apply_style(MAIN, 0, 0, h1);
        doc.apply_style(MAIN, 2, 2, h1);
        doc.forget_history();
        let mut v = DocView::new(doc, "T".into());
        let mut clip = crate::docview::Clipboard::new();
        v.update_layout(&mut fonts);
        v.insert_toc(TocOptions::default());
        v.update_layout(&mut fonts);
        assert!(v.doc.props.live_toc.is_some());
        let toc = |v: &DocView| v.doc.main.paras[0].text.clone();
        assert_eq!(toc(&v), "Chapter One\t1");

        // Editing a heading updates it, and one Undo takes both back.
        v.set_selection(Selection::caret(Position::new(2, 7)));
        v.type_text("s");
        v.update_layout(&mut fonts);
        assert_eq!(toc(&v), "Chapters One\t1");
        v.exec(crate::commands::Cmd::Undo, false, &mut clip);
        v.update_layout(&mut fonts);
        assert_eq!(toc(&v), "Chapter One\t1");
        assert_eq!(v.doc.main.paras[2].text, "Chapter One");

        // Typing in it does not stick while it is live.
        v.set_selection(Selection::caret(Position::new(0, 3)));
        v.type_text("xyz");
        v.update_layout(&mut fonts);
        assert_eq!(toc(&v), "Chapter One\t1");

        // Made editable, it is ordinary text; Update makes it live again.
        v.exec(crate::commands::Cmd::MakeTocEditable, false, &mut clip);
        assert!(v.doc.props.live_toc.is_none());
        v.set_selection(Selection::caret(Position::new(0, 3)));
        v.type_text("xyz");
        v.update_layout(&mut fonts);
        assert_eq!(toc(&v), "Chaxyzpter One\t1");
        v.exec(crate::commands::Cmd::UpdateToc, false, &mut clip);
        v.update_layout(&mut fonts);
        assert_eq!(toc(&v), "Chapter One\t1");
        assert!(v.doc.props.live_toc.is_some());

        // Deleted, it is gone for good: nothing brings it back.
        let end = v.doc.existing_toc().unwrap();
        v.set_selection(Selection::new(
            Position::new(0, 0),
            Position::new(end + 1, 0),
        ));
        v.exec(crate::commands::Cmd::Clear, false, &mut clip);
        v.update_layout(&mut fonts);
        assert!(v.doc.props.live_toc.is_none());
        assert_eq!(v.doc.main.paras[0].text, "Chapter One");
    }

    #[test]
    fn entries_hide_their_markers() {
        let mut v = DocView::new(quill_core::Document::new(), "T".into());
        v.type_text("Cats sleep");
        v.set_selection(Selection::new(Position::new(0, 0), Position::new(0, 4)));
        v.insert_entry('i');
        let p = &v.doc.main.paras[0];
        assert_eq!(p.text, ".i.Cats; sleep");
        assert!(p.chp_of_char(0).hidden && p.chp_of_char(7).hidden);
        assert!(!p.chp_of_char(3).hidden);
        assert_eq!(v.doc.index_entries()[0].main, "Cats");
    }
}
