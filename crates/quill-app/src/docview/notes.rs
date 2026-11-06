//! Footnotes on the page. Normal view keeps them out of the text (they
//! are edited in the footnote pane); Page Layout view, Print Preview and
//! printing put them where Document > Footnotes says: at the bottom of
//! the page (or column), beneath the text, or gathered at the end of each
//! section or of the document, under a separator line.

use super::*;
use quill_core::document::FootnotePosition;
use quill_core::notes::note_ids;
use std::sync::Arc;

/// Space for the separator line above the notes, in points.
pub const NOTE_SEP: f32 = 12.0;
/// The original's separator is a short rule, 2 inches long.
const SEP_WIDTH: f32 = 144.0;

/// Notes gathered at the end of a section or of the document: they come
/// after paragraph `after`, at continuous y `y`.
#[derive(Clone, Debug)]
pub struct EndNotes {
    pub after: usize,
    pub y: f32,
    pub ids: Vec<u32>,
    pub height: f32,
}

/// A note placed on a page: its id and where its text starts, in points
/// from the top left corner of the page's text area.
#[derive(Clone, Copy, Debug)]
pub struct NotePlace {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub h: f32,
}

impl DocView {
    pub(super) fn note_position(&self) -> FootnotePosition {
        self.doc.props.footnote_position
    }

    /// Whether notes sit by their pages (rather than at the end).
    fn notes_by_page(&self) -> bool {
        matches!(
            self.note_position(),
            FootnotePosition::BottomOfPage | FootnotePosition::BeneathText
        )
    }

    /// Each reference in the text: note id, paragraph and offset.
    pub(super) fn note_refs(&self) -> Vec<(u32, usize, usize)> {
        let mut out = Vec::new();
        for (i, p) in self.doc.main.paras.iter().enumerate() {
            if !p.text.contains(chars::FOOTNOTE) {
                continue;
            }
            for (off, c) in p.text.char_indices() {
                if c == chars::FOOTNOTE {
                    out.push((p.chp_of_char(off).object, i, off));
                }
            }
        }
        out
    }

    /// Work out what each reference shows. Returns true if that changed,
    /// after marking the paragraphs that show them for laying out again.
    pub(super) fn update_note_labels(&mut self) -> bool {
        let pages = self.bands.clone();
        let tops: Vec<f32> = self.tops.iter().map(|&t| self.unpad(t)).collect();
        let layouts = &self.layouts;
        let refs = self.note_refs();
        let page_of = |id: u32| -> usize {
            let Some(&(_, para, off)) = refs.iter().find(|r| r.0 == id) else {
                return 0;
            };
            let Some(Some(l)) = layouts.get(para) else {
                return 0;
            };
            let line = &l.lines[l.line_of(off, false)];
            let y = tops.get(para).copied().unwrap_or(0.0) + line.top;
            let k = pages.partition_point(|b| b.y <= y).saturating_sub(1);
            pages.get(k).map_or(0, |b| b.page)
        };
        let labels = self.doc.note_labels(Some(&page_of));
        // Annotation marks show their writer's initials.
        let initials: std::collections::HashMap<u32, String> =
            quill_core::notes::annotation_ids(&self.doc.main.paras)
                .into_iter()
                .map(|id| {
                    let i = self.doc.annotation_info.get(&id);
                    let initials = i.map_or(String::new(), |i| i.initials.clone());
                    // The pane says who wrote each one, after a tab.
                    if self.captions {
                        (id, format!("{initials}\t{}", self.caption(id)))
                    } else {
                        (id, initials)
                    }
                })
                .collect();
        if *self.annotation_labels != initials {
            self.annotation_labels = Arc::new(initials);
            for (i, p) in self.doc.main.paras.iter().enumerate() {
                if p.text.contains(chars::ANNOTATION)
                    && let Some(slot) = self.layouts.get_mut(i)
                {
                    *slot = None;
                }
            }
        }
        if *self.note_labels == labels {
            return false;
        }
        self.note_labels = Arc::new(labels);
        for (i, p) in self.doc.main.paras.iter().enumerate() {
            if p.text.contains(chars::FOOTNOTE)
                && let Some(slot) = self.layouts.get_mut(i)
            {
                *slot = None;
            }
        }
        self.notes_dirty = true;
        true
    }

    /// Lay out the notes story, each note as wide as the column its
    /// reference is in.
    pub(super) fn layout_notes(&mut self, fonts: &mut Fonts, opts: &LayoutOptions) {
        if !self.notes_dirty && self.note_layouts.len() == self.doc.notes.paras.len() {
            return;
        }
        self.notes_dirty = false;
        let fonts_table = self.doc.fonts.clone();
        let name = |id: u16| fonts_table.get(id as usize).cloned().unwrap_or_default();
        self.note_ranges = self.doc.note_ranges();
        let refs = self.note_refs();
        let full = self.doc.props.text_width();
        let mut widths = vec![full; self.doc.notes.paras.len()];
        for (id, r) in &self.note_ranges {
            if let Some(&(_, para, _)) = refs.iter().find(|x| x.0 == *id) {
                widths[r.clone()].fill(self.column_twips(para));
            }
        }
        self.note_widths = widths.iter().map(|&w| w as f32 / 20.0).collect();
        self.note_layouts = self
            .doc
            .notes
            .paras
            .iter()
            .zip(widths)
            .map(|(p, w)| {
                let o = LayoutOptions {
                    width: w as f32 / 20.0,
                    ..opts.clone()
                };
                layout_paragraph_joined(p, &name, fonts, &o, Joins::default())
            })
            .collect();
    }

    /// The height of note `id`'s text.
    pub(super) fn note_height(&self, id: u32) -> f32 {
        self.note_ranges.get(&id).map_or(0.0, |r| {
            r.clone()
                .filter_map(|i| self.note_layouts.get(i))
                .map(|l| l.height)
                .sum()
        })
    }

    /// The notes that go at the end of each section (or of the document),
    /// by the paragraph they follow.
    pub(super) fn end_note_groups(&self) -> Vec<(usize, Vec<u32>)> {
        let pos = self.note_position();
        if self.notes_by_page() || !self.frames_on() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut pending = Vec::new();
        let n = self.doc.main.paras.len();
        for (k, (first, s)) in self.secs.iter().enumerate() {
            let last = self.secs.get(k + 1).map_or(n, |x| x.0) - 1;
            pending.extend(note_ids(&self.doc.main.paras[*first..=last]));
            let flush = k + 1 == self.secs.len()
                || (pos == FootnotePosition::EndOfSection && s.include_endnotes);
            if flush && !pending.is_empty() {
                out.push((last, std::mem::take(&mut pending)));
            }
        }
        out
    }

    /// Footnote room for each pagination unit: each line needs the height
    /// of the notes referenced on it.
    pub(super) fn note_room(&self, units: &mut [paginate::Unit]) {
        if !self.notes_by_page() {
            return;
        }
        for (id, para, off) in self.note_refs() {
            let Some(Some(l)) = self.layouts.get(para) else {
                continue;
            };
            let line = l.line_of(off, false);
            let k = units.partition_point(|u| (u.para, u.line) < (para, line));
            let k = if units.get(k).is_some_and(|u| u.para == para) {
                k
            } else {
                // A table row is one unit.
                k.saturating_sub(1)
            };
            if let Some(u) = units.get_mut(k) {
                u.notes += self.note_height(id);
            }
        }
    }

    /// The notes placed on page `k`, by its columns.
    pub fn page_notes(&self, k: usize) -> Vec<NotePlace> {
        let mut out = Vec::new();
        if !self.notes_by_page() || !self.frames_on() {
            return out;
        }
        let text_h = self.page_body_height(k);
        let beneath = self.note_position() == FootnotePosition::BeneathText;
        let refs = self.note_refs();
        // Each column (by its x) gets the notes of its bands, in order.
        let mut by_x: Vec<(f32, f32, Vec<u32>)> = Vec::new();
        for b in self.page_bands(k) {
            let ids: Vec<u32> = refs
                .iter()
                .filter(|(_, para, off)| {
                    let Some(Some(l)) = self.layouts.get(*para) else {
                        return false;
                    };
                    let y = self.tops[*para] + l.lines[l.line_of(*off, false)].top;
                    y >= b.y && y < b.end.max(b.y + 0.01)
                })
                .map(|r| r.0)
                .collect();
            let bottom = b.top + (b.end - b.y);
            match by_x.iter_mut().find(|c| c.0 == b.x) {
                Some(c) => {
                    c.1 = c.1.max(bottom);
                    c.2.extend(ids);
                }
                None => by_x.push((b.x, bottom, ids)),
            }
        }
        for (x, bottom, ids) in by_x {
            if ids.is_empty() {
                continue;
            }
            let total: f32 = ids.iter().map(|&id| self.note_height(id)).sum();
            let mut y = if beneath {
                bottom + NOTE_SEP
            } else {
                text_h - total
            };
            for id in ids {
                let h = self.note_height(id);
                out.push(NotePlace { id, x, y, h });
                y += h;
            }
        }
        out
    }

    /// Draw note `id`'s paragraphs with their top left corner at (`x`,
    /// `y`).
    #[allow(clippy::too_many_arguments)]
    fn draw_note(
        &self,
        c: &mut dyn Surface,
        fonts: &Fonts,
        cache: &mut GlyphCache,
        id: u32,
        x: f32,
        mut y: f32,
    ) {
        let Some(r) = self.note_ranges.get(&id) else {
            return;
        };
        for i in r.clone() {
            let (Some(p), Some(l)) = (self.doc.notes.paras.get(i), self.note_layouts.get(i)) else {
                continue;
            };
            if let Some(b) = &l.border {
                draw_border_box(c, b, x, y);
            }
            let w = self.note_widths.get(i).copied().unwrap_or(0.0);
            paint_paragraph(c, fonts, cache, p, l, x, y, false, w, &self.doc.objects);
            y += l.height;
        }
    }

    /// The separator line above notes whose first one starts at `y`.
    fn draw_separator(c: &mut dyn Surface, x: f32, y: f32, width: f32) {
        let y = (y - NOTE_SEP / 2.0).floor();
        c.fill(
            Rect::new(x, y, SEP_WIDTH.min(width), c.hair()),
            color::BLACK,
        );
    }

    /// Draw page `k`'s footnotes, with the page's text area at (`ox`,
    /// `oy`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_page_notes(
        &self,
        c: &mut dyn Surface,
        fonts: &Fonts,
        cache: &mut GlyphCache,
        k: usize,
        ox: f32,
        oy: f32,
    ) {
        let places = self.page_notes(k);
        let mut last_x = None;
        for n in &places {
            if last_x != Some(n.x) {
                let w = self
                    .note_ranges
                    .get(&n.id)
                    .and_then(|r| self.note_widths.get(r.start).copied())
                    .unwrap_or(SEP_WIDTH);
                Self::draw_separator(c, ox + n.x, oy + n.y, w);
                last_x = Some(n.x);
            }
            self.draw_note(c, fonts, cache, n.id, ox + n.x, oy + n.y);
        }
    }

    /// Draw the notes gathered at the end of sections that fall between
    /// document y `top` and `bottom`, with document (0, 0) at (`ox`,
    /// `oy`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_end_notes(
        &self,
        c: &mut dyn Surface,
        fonts: &Fonts,
        cache: &mut GlyphCache,
        ox: f32,
        oy: f32,
        top: f32,
        bottom: f32,
    ) {
        for e in &self.endnotes {
            if e.y + e.height < top || e.y > bottom {
                continue;
            }
            let mut y = e.y + NOTE_SEP;
            Self::draw_separator(c, ox, oy + y, self.laid_out_width);
            for &id in &e.ids {
                self.draw_note(c, fonts, cache, id, ox, oy + y);
                y += self.note_height(id);
            }
        }
    }

    /// Who is writing an annotation now: Preferences' name and initials.
    fn annotation_writer() -> quill_core::notes::AnnotationInfo {
        let (name, initials) = crate::settings::with(|s| (s.name.clone(), s.initials.clone()));
        let initials = if initials.trim().is_empty() {
            name.split_whitespace()
                .filter_map(|w| w.chars().next())
                .collect()
        } else {
            initials
        };
        quill_core::notes::AnnotationInfo {
            initials,
            author: name,
            date: crate::clock::now().change_date(),
            ..Default::default()
        }
    }

    /// Insert > Annotation: a mark at the insertion point, signed with the
    /// initials from Preferences. With text selected, the annotation is
    /// about that text and its mark goes after it. Returns its id.
    pub fn insert_annotation(&mut self) -> u32 {
        let info = Self::annotation_writer();
        self.doc.begin("Annotation", self.sel);
        let (s, e) = self.sel.range();
        let chp = self
            .typing_chp
            .unwrap_or_else(|| self.doc.main.paras[e.para].chp_at(e.offset));
        let id = if s == e {
            self.doc.insert_annotation(e, chp, info)
        } else {
            self.doc.insert_annotation_on(s, e, chp, info)
        };
        let end = Position::new(e.para, e.offset + chars::ANNOTATION.len_utf8());
        self.finish_edit(Selection::caret(end));
        self.typing_chp = Some(CharProps { comment: 0, ..chp });
        id
    }

    /// The annotation at the insertion point: the one whose mark is just
    /// before or after it, or else the one whose text it is in. In the
    /// annotations pane (`captions`), the one being read.
    pub fn annotation_here(&self) -> Option<u32> {
        let at = self.sel.focus;
        let paras = &self.doc.main.paras;
        if self.captions {
            return paras[..=at.para]
                .iter()
                .rev()
                .find(|p| p.text.starts_with(chars::ANNOTATION))
                .map(|p| p.chp_of_char(0).object);
        }
        let p = &paras[at.para];
        let before = p.text[..at.offset].chars().next_back();
        if before == Some(chars::ANNOTATION) {
            return Some(
                p.chp_of_char(at.offset - chars::ANNOTATION.len_utf8())
                    .object,
            );
        }
        if p.text[at.offset..].starts_with(chars::ANNOTATION) {
            return Some(p.chp_of_char(at.offset).object);
        }
        let c = p.chp_at(at.offset).comment;
        (c != 0 && self.doc.annotation_info.contains_key(&c)).then_some(c)
    }

    /// Reply to Annotation: a reply to `parent` by whoever Preferences
    /// names. Returns the reply's id.
    pub fn reply_to(&mut self, parent: u32) -> Option<u32> {
        self.doc.begin("Reply", self.sel);
        let id = self.doc.insert_reply(parent, Self::annotation_writer());
        let sel = self.sel;
        self.finish_edit(sel);
        id
    }

    /// Delete annotation `id` and its replies, leaving the insertion point
    /// where its mark was.
    pub fn delete_annotation(&mut self, id: u32) {
        let sel = self.sel;
        self.doc.begin("Delete Annotation", sel);
        let at = self.doc.delete_annotation(id).unwrap_or(sel.focus);
        self.finish_edit(Selection::caret(at));
    }

    /// What the annotations pane shows beside a mark: who wrote it and
    /// when, and whether it is a reply or resolved.
    pub(super) fn caption(&self, id: u32) -> String {
        let Some(i) = self.doc.annotation_info.get(&id) else {
            return String::new();
        };
        let mut out = i.author.clone();
        if !i.date.is_empty() {
            if !out.is_empty() {
                out.push_str(", ");
            }
            out.push_str(&super::changes::readable_date(&i.date));
        }
        if i.parent.is_some() {
            out.push_str(" · Reply");
        }
        let top = self.doc.thread_start(id);
        if i.done || self.doc.annotation_info.get(&top).is_some_and(|t| t.done) {
            out.push_str(" · Resolved");
        }
        out
    }

    /// The annotation View > Annotations shows: the next one from the
    /// insertion point, or else the first.
    pub fn next_annotation(&self) -> Option<u32> {
        let at = self.sel.range().0;
        let mut first = None;
        for (i, p) in self.doc.main.paras.iter().enumerate() {
            for (off, c) in p.text.char_indices() {
                if c != chars::ANNOTATION {
                    continue;
                }
                let id = p.chp_of_char(off).object;
                first.get_or_insert(id);
                if Position::new(i, off) >= at {
                    return Some(id);
                }
            }
        }
        first
    }

    /// Insert > Footnote: a reference at the insertion point, with `mark`
    /// instead of a number if given. Returns the note's id.
    pub fn insert_footnote(&mut self, mark: Option<String>) -> u32 {
        self.doc.begin("Footnote", self.sel);
        let at = self.delete_selection();
        let chp = self
            .typing_chp
            .unwrap_or_else(|| self.doc.main.paras[at.para].chp_at(at.offset));
        let id = self.doc.insert_note(at, chp, mark);
        let end = Position::new(at.para, at.offset + chars::FOOTNOTE.len_utf8());
        self.finish_edit(Selection::caret(end));
        // Typing goes on in the text's own format, not the reference's.
        self.typing_chp = Some(chp);
        id
    }

    /// The note under a window point in Page Layout view, if any.
    pub fn note_at(&self, wx: f32, wy: f32) -> Option<u32> {
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
        let lx = wx - self.area.x - self.text_x();
        let ly = vy - self.page_top(k) - self.page_body_top(k);
        for n in self.page_notes(k) {
            let w = self.column_twips(0) as f32 / 20.0;
            if lx >= n.x && lx < n.x + w && ly >= n.y && ly < n.y + n.h {
                return Some(n.id);
            }
        }
        let (_, y) = self.doc_at(lx, vy);
        for e in &self.endnotes {
            let mut ny = e.y + NOTE_SEP;
            for &id in &e.ids {
                let h = self.note_height(id);
                if y >= ny && y < ny + h {
                    return Some(id);
                }
                ny += h;
            }
        }
        None
    }

    /// Pagination units for the notes gathered at section ends.
    pub(super) fn end_note_units(&self) -> Vec<paginate::Unit> {
        let mut out = Vec::new();
        for e in &self.endnotes {
            let mut y = self.unpad(e.y);
            let mut line = 1_000_000;
            let mut push = |top: f32, bottom: f32| {
                out.push(paginate::Unit {
                    para: e.after,
                    line,
                    top,
                    bottom,
                    first: false,
                    last: false,
                    hard_break: false,
                    notes: 0.0,
                });
                line += 1;
            };
            push(y, y + NOTE_SEP);
            y += NOTE_SEP;
            for &id in &e.ids {
                let Some(r) = self.note_ranges.get(&id) else {
                    continue;
                };
                for l in r.clone().filter_map(|i| self.note_layouts.get(i)) {
                    for ln in &l.lines {
                        push(y + ln.top, y + ln.top + ln.height);
                    }
                    y += l.height;
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_captions_get_a_line_and_body_reflows_with_sidebar_width() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let mut doc = quill_core::Document::new();
        let id = doc.insert_annotation(
            Position::new(0, 0),
            Default::default(),
            quill_core::notes::AnnotationInfo {
                author: "Morgan".into(),
                initials: "M".into(),
                date: "2026-10-23T11:00:00Z".into(),
                ..Default::default()
            },
        );
        let at = doc.annotation_text_start(id).unwrap();
        doc.insert_text(
            StoryId::Annotations,
            at,
            &"A readable comment body. ".repeat(12),
            Default::default(),
        );
        let mut v = DocView::new(
            doc.copy_story_for_edit(StoryId::Annotations),
            "Comments".into(),
        );
        v.captions = true;
        v.area = Rect::new(0.0, 0.0, 300.0, 700.0);
        v.update_layout(&mut fonts);
        let layout = v
            .layouts
            .iter()
            .flatten()
            .find(|l| !l.fields.is_empty())
            .unwrap();
        assert_eq!(
            layout.line_glyphs(&layout.lines[0]).len(),
            1,
            "caption is separate from editable text"
        );
        let wide_lines = layout.lines.len();
        v.area.w = 160.0;
        v.update_layout(&mut fonts);
        let layout = v
            .layouts
            .iter()
            .flatten()
            .find(|l| !l.fields.is_empty())
            .unwrap();
        assert!(layout.lines.len() > wide_lines);
        assert_eq!(v.doc.annotation_info[&id].author, "Morgan");
    }

    #[test]
    fn notes_go_to_the_foot_of_the_page() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let mut v = DocView::new(quill_core::Document::new(), "T".into());
        v.type_text("Text with a note");
        let id = v.insert_footnote(None);
        let at = v.doc.note_text_start(id).unwrap();
        v.doc
            .insert_text(StoryId::Notes, at, "The note.", Default::default());
        v.page_view = true;
        v.area = Rect::new(0.0, 0.0, 700.0, 800.0);
        v.update_layout(&mut fonts);
        let places = v.page_notes(0);
        assert_eq!(places.len(), 1, "{:?} {:?}", v.note_ranges, v.bands);
        let text_h = 648.0;
        assert!(places[0].y > text_h - 40.0 && places[0].y < text_h);
    }

    #[test]
    fn an_annotation_on_a_selection_covers_it_and_replies_follow_it() {
        let mut v = DocView::new(quill_core::Document::new(), "t".into());
        v.insert_text("one two three");
        v.set_selection(Selection::new(Position::new(0, 4), Position::new(0, 7)));
        let id = v.insert_annotation();
        assert_eq!(
            v.doc.main.paras[0].text, "one two\u{E004} three",
            "the text stays"
        );
        assert_eq!(v.doc.main.paras[0].chp_of_char(5).comment, id);
        assert_eq!(v.doc.main.paras[0].chp_of_char(8).comment, 0);
        // Inside the text, or by the mark, it is the annotation here.
        v.set_selection(Selection::caret(Position::new(0, 5)));
        assert_eq!(v.annotation_here(), Some(id));
        v.set_selection(Selection::caret(Position::new(0, 10)));
        assert_eq!(v.annotation_here(), Some(id));
        v.set_selection(Selection::caret(Position::new(0, 1)));
        assert_eq!(v.annotation_here(), None);
        v.set_selection(Selection::caret(Position::new(0, 5)));
        let reply = v.reply_to(id).unwrap();
        assert_eq!(v.doc.annotation_info[&reply].parent, Some(id));
        assert_eq!(v.doc.thread_start(reply), id);
        assert!(v.caption(reply).contains("Reply"));
    }
}
