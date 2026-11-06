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
    /// What was typed into one made editable goes to its headings first,
    /// in the same undo step.
    pub fn update_toc(&mut self) {
        let Some(end) = self.doc.existing_toc() else {
            return;
        };
        self.doc.begin("Table of Contents", self.sel);
        let (o, told) = match self.doc.props.live_toc {
            Some(o) => (o, None),
            None => {
                let o = self.options_of_toc(end);
                (o, self.headings_from_toc(&o, end))
            }
        };
        self.insert_toc(o);
        self.status = told;
    }

    /// The settings an editable table of contents was made with, which it
    /// no longer holds, as its lines show them: the levels of its styles,
    /// and page numbers if a line has a tab.
    fn options_of_toc(&self, end: usize) -> TocOptions {
        let paras = &self.doc.main.paras[..=end];
        let level = |p: &Paragraph| self.doc.styles.toc_level(p.pap.style);
        let levels = || paras.iter().filter_map(level);
        TocOptions {
            page_numbers: paras.iter().any(|p| p.text.contains(chars::TAB)),
            from: levels().min().unwrap_or(1),
            to: levels().max().unwrap_or(9),
            ..TocOptions::default()
        }
    }

    /// Give each heading the text of its line in an editable table of
    /// contents, changing only the words that differ, so a footnote mark or
    /// hidden text in a heading stays. Lines and headings are matched in
    /// order, so with lines added or removed nothing can be matched and
    /// the headings stay. Returns what to tell the person, if anything.
    fn headings_from_toc(&mut self, o: &TocOptions, end: usize) -> Option<String> {
        let entries = self.doc.toc_entries(o);
        if entries.len() != end + 1 {
            return Some("Lines were added or removed, so the headings were left alone.".into());
        }
        let lines: Vec<String> = self.doc.main.paras[..=end]
            .iter()
            .map(|p| match p.text.rsplit_once(chars::TAB) {
                Some((text, _)) if o.page_numbers => text.trim().to_string(),
                _ => p.text.trim().to_string(),
            })
            .collect();
        let mut changed = 0;
        for (entry, line) in entries.iter().zip(&lines) {
            if line.is_empty() || *line == entry.text {
                continue;
            }
            let para = entry.at.para;
            let chars = quill_core::contents::visible_chars(&self.doc.main.paras[para]);
            let new: Vec<char> = line.chars().collect();
            let Some(&(_, first)) = chars.first() else {
                continue;
            };
            let same = |a: &(char, usize), b: &char| a.0 == *b;
            let front = chars
                .iter()
                .zip(&new)
                .take_while(|(a, b)| same(a, b))
                .count();
            let room = chars.len().min(new.len()) - front;
            let back = chars
                .iter()
                .rev()
                .zip(new.iter().rev())
                .take(room)
                .take_while(|(a, b)| same(a, b))
                .count();
            let text = &self.doc.main.paras[para].text;
            let after = |k: usize| {
                let (_, at) = chars[k];
                at + text[at..].chars().next().map_or(0, char::len_utf8)
            };
            let start = if front > 0 { after(front - 1) } else { first };
            let stop = if chars.len() > front + back {
                after(chars.len() - back - 1)
            } else {
                start
            };
            let chp = self.doc.main.paras[para].chp_at(start);
            let at = Position::new(para, start);
            if stop > start {
                self.doc.delete(MAIN, at, Position::new(para, stop));
            }
            let words: String = new[front..new.len() - back].iter().collect();
            self.doc.insert_text(MAIN, at, &words, chp);
            changed += 1;
        }
        (changed > 0).then(|| match changed {
            1 => "1 heading changed to match.".to_string(),
            n => format!("{n} headings changed to match."),
        })
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
    fn editable_contents_updates_six_heading_levels_and_undoes_together() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        for (title_case, page_numbers) in
            [(false, true), (true, true), (false, false), (true, false)]
        {
            let mut doc =
                quill_core::text::from_plain_text("First\nSecond\nThird\nFourth\nFifth\nSixth");
            for level in 1..=6 {
                let id = doc.styles.find(&format!("heading {level}")).unwrap();
                doc.apply_style(MAIN, level - 1, level - 1, id);
                if title_case {
                    doc.styles.styles[id as usize].name = format!("Heading {level}");
                    let id = doc.styles.find(&format!("toc {level}")).unwrap();
                    doc.styles.styles[id as usize].name = format!("TOC {level}");
                }
            }
            doc.forget_history();
            let mut v = DocView::new(doc, "Six levels".into());
            v.update_layout(&mut fonts);
            v.insert_toc(TocOptions {
                to: 6,
                page_numbers,
                ..TocOptions::default()
            });
            v.update_layout(&mut fonts);
            assert_eq!(v.doc.existing_toc(), Some(5), "title_case={title_case}");
            v.make_toc_editable();
            for level in 1..=6 {
                let para = level - 1;
                let tab = v.doc.main.paras[para]
                    .text
                    .find(chars::TAB)
                    .unwrap_or(v.doc.main.paras[para].len());
                let chp = v.doc.main.paras[para].chp_at(0);
                v.doc.begin("Edit contents", v.sel);
                v.doc
                    .delete(MAIN, Position::new(para, 0), Position::new(para, tab));
                v.doc.insert_text(
                    MAIN,
                    Position::new(para, 0),
                    &format!("Renamed level {level}"),
                    chp,
                );
                v.doc.commit(v.sel);
            }
            if title_case {
                let bytes = quill_formats::native::write(&v.doc, 0);
                v = DocView::new(
                    quill_formats::native::read(&bytes).unwrap(),
                    "Reopened contents".into(),
                );
                v.update_layout(&mut fonts);
            }
            v.update_toc();
            v.update_layout(&mut fonts);
            let options = v.doc.props.live_toc.unwrap();
            assert_eq!(options.to, 6);
            assert_eq!(options.page_numbers, page_numbers);
            for level in 1..=6 {
                assert_eq!(
                    v.doc.main.paras[5 + level].text,
                    format!("Renamed level {level}")
                );
                let line = &v.doc.main.paras[level - 1].text;
                let title = line
                    .split_once(chars::TAB)
                    .map_or(line.as_str(), |(title, _)| title);
                assert_eq!(title, format!("Renamed level {level}"));
            }
            assert_eq!(v.status.as_deref(), Some("6 headings changed to match."));
            v.doc.undo();
            assert!(v.doc.props.live_toc.is_none());
            assert_eq!(v.doc.main.paras[11].text, "Sixth");
            assert!(v.doc.main.paras[5].text.starts_with("Renamed level 6"));
            v.doc.redo();
            v.update_layout(&mut fonts);
            assert_eq!(v.doc.main.paras[11].text, "Renamed level 6");
            v.sel = Selection::caret(Position::new(11, 0));
            v.type_text("Live ");
            v.update_layout(&mut fonts);
            assert!(v.doc.main.paras[5].text.starts_with("Live Renamed level 6"));
        }
    }

    #[test]
    fn every_level_of_an_editable_contents_reaches_its_heading() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let mut doc = quill_core::text::from_plain_text("One\ntext\nTwo\tParts");
        let (h1, h2) = (
            doc.styles.find("heading 1").unwrap(),
            doc.styles.find("heading 2").unwrap(),
        );
        doc.apply_style(MAIN, 0, 0, h1);
        doc.apply_style(MAIN, 2, 2, h2);
        doc.forget_history();
        let mut v = DocView::new(doc, "T".into());
        v.update_layout(&mut fonts);
        // Only the first two levels, which Make Editable forgets.
        v.insert_toc(TocOptions {
            to: 2,
            ..TocOptions::default()
        });
        v.update_layout(&mut fonts);
        v.make_toc_editable();
        // The second line, the heading with a tab in it.
        assert!(v.doc.main.paras[1].text.starts_with("Two Parts\t"));
        let chp = v.doc.main.paras[1].chp_at(4);
        v.doc.begin("t", v.sel);
        v.doc.delete(MAIN, Position::new(1, 4), Position::new(1, 9));
        v.doc.insert_text(MAIN, Position::new(1, 4), "Pieces", chp);
        let sel = v.sel;
        v.doc.commit(sel);
        v.update_toc();
        v.update_layout(&mut fonts);
        let texts: Vec<&str> = v.doc.main.paras.iter().map(|p| p.text.as_str()).collect();
        assert!(texts.contains(&"Two\tPieces"), "{texts:?}");
        assert_eq!(v.doc.props.live_toc.unwrap().to, 2);
        assert_eq!(v.status.as_deref(), Some("1 heading changed to match."));
    }

    #[test]
    fn text_typed_in_an_editable_contents_reaches_its_heading() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let mut doc = quill_core::text::from_plain_text("First\nBody\nSecond");
        let h1 = doc.styles.find("heading 1").unwrap();
        doc.apply_style(MAIN, 0, 0, h1);
        doc.apply_style(MAIN, 2, 2, h1);
        doc.forget_history();
        let mut v = DocView::new(doc, "T".into());
        v.update_layout(&mut fonts);
        v.insert_toc(TocOptions::default());
        v.update_layout(&mut fonts);
        v.make_toc_editable();
        let end = v.doc.existing_toc().unwrap();
        assert_eq!(end, 1);
        // Change the first line, not its page number.
        let tab = v.doc.main.paras[0].text.find(chars::TAB).unwrap();
        v.doc.begin("t", v.sel);
        v.doc
            .delete(MAIN, Position::new(0, 0), Position::new(0, tab));
        let chp = v.doc.main.paras[0].chp_at(0);
        v.doc.insert_text(MAIN, Position::new(0, 0), "Opening", chp);
        let sel = v.sel;
        v.doc.commit(sel);

        let texts = |v: &DocView| -> Vec<String> {
            v.doc.main.paras.iter().map(|p| p.text.clone()).collect()
        };
        v.update_toc();
        v.update_layout(&mut fonts);
        assert!(texts(&v).contains(&"Opening".to_string()));
        assert!(!texts(&v).contains(&"First".to_string()));
        assert!(texts(&v).contains(&"Second".to_string()));
        assert!(v.doc.props.live_toc.is_some());
        assert_eq!(v.status.as_deref(), Some("1 heading changed to match."));
        // One Undo puts the heading back.
        v.doc.undo();
        assert!(texts(&v).contains(&"First".to_string()));
    }

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
        assert_eq!(toc(&v), "Chaxyzpter One\t1");
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
        assert_eq!(v.doc.main.paras[0].text, "Chaxyzpter One");
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
