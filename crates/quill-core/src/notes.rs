//! Footnotes and annotations, as the original keeps them. A reference in the text
//! is a FOOTNOTE (or ANNOTATION) character whose `CharProps::object` is
//! the note's id. The notes of each kind live in one story, in the order
//! of their references; each note starts with a paragraph whose first
//! character is its own mark (same id). After any edit that adds or
//! removes references, the story is rebuilt in the references' order, so
//! cut, paste, undo and sorting keep every note with its reference.

use crate::document::{Document, StoryId};
use crate::para::{Paragraph, chars};
use crate::props::CharProps;
use crate::story::{Position, Story};
use std::collections::HashMap;

/// Which kind of note.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NoteKind {
    Foot,
    Annotation,
}

impl NoteKind {
    pub const ALL: [NoteKind; 2] = [NoteKind::Foot, NoteKind::Annotation];

    /// The character that marks a reference to this kind of note.
    pub fn mark(self) -> char {
        match self {
            NoteKind::Foot => chars::FOOTNOTE,
            NoteKind::Annotation => chars::ANNOTATION,
        }
    }

    pub fn story(self) -> StoryId {
        match self {
            NoteKind::Foot => StoryId::Notes,
            NoteKind::Annotation => StoryId::Annotations,
        }
    }

    /// The style the note's text is in.
    fn style(self) -> &'static str {
        match self {
            NoteKind::Foot => "footnote text",
            NoteKind::Annotation => "Normal",
        }
    }
}

/// Who wrote an annotation and when, as the original shows them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnnotationInfo {
    pub initials: String,
    pub author: String,
    /// "2026-10-14T14:05:00Z", or just the day from older files.
    pub date: String,
    /// The annotation this one is a reply to.
    pub parent: Option<u32>,
    /// Marked resolved.
    pub done: bool,
}

/// A copied note, carried on the clipboard with its reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteCopy {
    pub kind: NoteKind,
    pub id: u32,
    pub mark: Option<String>,
    pub info: Option<AnnotationInfo>,
    pub paras: Vec<Paragraph>,
}

/// The ids of references to notes of `kind` in paragraphs, in order.
pub fn ref_ids<'a>(paras: impl IntoIterator<Item = &'a Paragraph>, kind: NoteKind) -> Vec<u32> {
    let ch = kind.mark();
    let mut out = Vec::new();
    for p in paras {
        if !p.text.contains(ch) {
            continue;
        }
        for (s, e, chp) in p.run_bounds() {
            out.extend(
                p.text[s..e]
                    .chars()
                    .filter(|&c| c == ch)
                    .map(|_| chp.object),
            );
        }
    }
    out
}

/// The footnote ids referenced in paragraphs, in order.
pub fn note_ids<'a>(paras: impl IntoIterator<Item = &'a Paragraph>) -> Vec<u32> {
    ref_ids(paras, NoteKind::Foot)
}

/// The annotation ids referenced in paragraphs, in order.
pub fn annotation_ids<'a>(paras: impl IntoIterator<Item = &'a Paragraph>) -> Vec<u32> {
    ref_ids(paras, NoteKind::Annotation)
}

/// The id of the note a notes-story paragraph starts, if it starts one.
fn starts(p: &Paragraph, kind: NoteKind) -> Option<u32> {
    p.text
        .starts_with(kind.mark())
        .then(|| p.chp_of_char(0).object)
}

/// The id of the footnote a notes-story paragraph starts, if any.
pub fn note_start(p: &Paragraph) -> Option<u32> {
    starts(p, NoteKind::Foot)
}

/// Where note `id`'s text starts in a story's paragraphs: after its mark
/// and the space after it.
pub fn text_start_in(paras: &[Paragraph], id: u32, kind: NoteKind) -> Option<Position> {
    let i = paras.iter().position(|p| starts(p, kind) == Some(id))?;
    let mut off = kind.mark().len_utf8();
    if paras[i].text[off..].starts_with(' ') {
        off += 1;
    }
    Some(Position::new(i, off))
}

impl Document {
    /// The paragraphs of each note of `kind` in its story, by id.
    pub fn ranges(&self, kind: NoteKind) -> HashMap<u32, std::ops::Range<usize>> {
        let paras = &self.story(kind.story()).paras;
        let mut out = HashMap::new();
        let mut cur: Option<(u32, usize)> = None;
        for (i, p) in paras.iter().enumerate() {
            if let Some(id) = starts(p, kind) {
                if let Some((prev, first)) = cur {
                    out.entry(prev).or_insert(first..i);
                }
                cur = Some((id, i));
            }
        }
        if let Some((prev, first)) = cur {
            out.entry(prev).or_insert(first..paras.len());
        }
        out
    }

    /// The paragraphs of each footnote, by id.
    pub fn note_ranges(&self) -> HashMap<u32, std::ops::Range<usize>> {
        self.ranges(NoteKind::Foot)
    }

    /// The paragraphs of each annotation, by id.
    pub fn annotation_ranges(&self) -> HashMap<u32, std::ops::Range<usize>> {
        self.ranges(NoteKind::Annotation)
    }

    fn text_para(&self, kind: NoteKind) -> Paragraph {
        let style = self.styles.find(kind.style()).unwrap_or(0);
        let st = self.styles.get(style);
        let mut p = Paragraph::new(st.pap.clone(), st.chp);
        p.pap.style = style;
        p
    }

    /// A note's first paragraph: its mark for note `id`, and a space.
    fn blank(&self, kind: NoteKind, id: u32) -> Paragraph {
        let mut p = self.text_para(kind);
        let base = p.mark_chp;
        let mut mark = match kind {
            NoteKind::Foot => self.note_mark_chp(base),
            NoteKind::Annotation => base,
        };
        mark.object = id;
        p.insert(0, &kind.mark().to_string(), mark);
        p.insert(kind.mark().len_utf8(), " ", base);
        p
    }

    /// `base` formatted as a footnote reference.
    pub fn note_mark_chp(&self, base: CharProps) -> CharProps {
        let mut c = base;
        if let Some(r) = self.styles.find("footnote reference") {
            let st = self.styles.get(r).chp;
            c.size = st.size;
            c.position = st.position;
        }
        c.object = 0;
        c
    }

    /// A note id not used yet, by either kind.
    pub fn fresh_note_id(&mut self) -> u32 {
        let used = NoteKind::ALL
            .iter()
            .flat_map(|&k| {
                ref_ids(&self.main.paras, k)
                    .into_iter()
                    .chain(ref_ids(&self.story(k.story()).paras, k))
            })
            .chain(self.note_bank.keys().copied())
            .chain(self.note_marks.keys().copied())
            .chain(self.annotation_info.keys().copied())
            .max()
            .unwrap_or(0);
        self.next_note = self.next_note.max(used + 1);
        self.next_note += 1;
        self.next_note - 1
    }

    /// Rebuild the story of `kind` so it holds one note per reference, in
    /// the references' order. Notes whose reference went are kept aside,
    /// so pasting the reference back brings them along.
    pub fn reconcile(&mut self, kind: NoteKind) {
        let refs = ref_ids(&self.main.paras, kind);
        let ranges = self.ranges(kind);
        let story = kind.story();
        let mut taken: Vec<Paragraph> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for &id in &refs {
            if !seen.insert(id) {
                continue;
            }
            match ranges.get(&id) {
                Some(r) => taken.extend(self.story(story).paras[r.clone()].iter().cloned()),
                None => match self.note_bank.remove(&id) {
                    Some(paras) => taken.extend(paras),
                    None => taken.push(self.blank(kind, id)),
                },
            }
        }
        for (id, r) in &ranges {
            if !seen.contains(id) {
                let paras = self.story(story).paras[r.clone()].to_vec();
                self.note_bank.insert(*id, paras);
            }
        }
        if taken.is_empty() {
            taken.push(self.text_para(kind));
        }
        if taken != self.story(story).paras {
            let n = self.story(story).paras.len();
            self.splice(story, 0, n, taken);
        }
    }

    /// `reconcile` for footnotes.
    pub fn reconcile_notes(&mut self) {
        self.reconcile(NoteKind::Foot);
    }

    /// Insert > Footnote: a reference at `at`, formatted from `chp`, with
    /// `mark` instead of a number if given. Returns the note's id; its
    /// text starts at `note_text_start(id)`.
    pub fn insert_note(&mut self, at: Position, chp: CharProps, mark: Option<String>) -> u32 {
        let id = self.fresh_note_id();
        if let Some(m) = mark.filter(|m| !m.is_empty()) {
            self.note_marks.insert(id, m);
        }
        let mut c = self.note_mark_chp(chp);
        c.object = id;
        self.insert_text(StoryId::Main, at, &chars::FOOTNOTE.to_string(), c);
        id
    }

    /// Insert > Annotation: a mark at `at` for a note by `info`. Returns
    /// its id.
    pub fn insert_annotation(&mut self, at: Position, chp: CharProps, info: AnnotationInfo) -> u32 {
        let id = self.fresh_note_id();
        self.annotation_info.insert(id, info);
        let c = CharProps {
            object: id,
            comment: 0,
            ..chp
        };
        // Comments are not tracked changes, in the original either.
        let tracking = std::mem::replace(&mut self.props.track_changes, false);
        self.insert_text(StoryId::Main, at, &chars::ANNOTATION.to_string(), c);
        self.props.track_changes = tracking;
        id
    }

    /// Insert > Annotation with text selected: an annotation about
    /// `start..end`, its mark after the text. Returns its id.
    pub fn insert_annotation_on(
        &mut self,
        start: Position,
        end: Position,
        chp: CharProps,
        info: AnnotationInfo,
    ) -> u32 {
        let (start, end) = (start.min(end), start.max(end));
        let id = self.insert_annotation(end, chp, info);
        self.set_comment_range(start, end, id);
        id
    }

    /// Make `start..end` of the main text the text annotation `id` is
    /// about. Not a tracked change.
    pub fn set_comment_range(&mut self, start: Position, end: Position, id: u32) {
        let paras = &self.main.paras;
        let mut new = Vec::with_capacity(end.para - start.para + 1);
        for (i, p) in paras.iter().enumerate().take(end.para + 1).skip(start.para) {
            let mut p = p.clone();
            let s = if i == start.para { start.offset } else { 0 };
            let e = if i == end.para { end.offset } else { p.len() };
            p.format_range(s, e, &mut |c| c.comment = id);
            new.push(p);
        }
        if new[..] != self.main.paras[start.para..=end.para] {
            self.splice(StoryId::Main, start.para, new.len(), new);
        }
    }

    /// Where annotation `id`'s mark is in the main text.
    pub fn annotation_mark(&self, id: u32) -> Option<Position> {
        self.main.paras.iter().enumerate().find_map(|(i, p)| {
            p.text
                .char_indices()
                .find(|&(at, c)| c == chars::ANNOTATION && p.chp_of_char(at).object == id)
                .map(|(at, _)| Position::new(i, at))
        })
    }

    /// A reply to annotation `parent`, by `info`: its mark goes after the
    /// parent's and any replies already there. Returns its id, or None
    /// when the parent's mark is gone.
    pub fn insert_reply(&mut self, parent: u32, mut info: AnnotationInfo) -> Option<u32> {
        let at = self.annotation_mark(parent)?;
        let p = &self.main.paras[at.para];
        let chp = p.chp_of_char(at.offset);
        // After the parent's mark and the replies that follow it.
        let mut after = at.offset + chars::ANNOTATION.len_utf8();
        while p.text[after..].starts_with(chars::ANNOTATION)
            && self
                .annotation_info
                .get(&p.chp_of_char(after).object)
                .is_some_and(|i| i.parent == Some(parent))
        {
            after += chars::ANNOTATION.len_utf8();
        }
        info.parent = Some(parent);
        Some(self.insert_annotation(Position::new(at.para, after), chp, info))
    }

    /// Delete annotation `id` and its replies: their marks go from the text
    /// and the text they were about is no longer marked. Returns where the
    /// first mark was.
    pub fn delete_annotation(&mut self, id: u32) -> Option<Position> {
        let at = self.annotation_mark(id)?;
        // The annotation, and every reply under it however deep.
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        for (&child, info) in &self.annotation_info {
            if let Some(parent) = info.parent {
                children.entry(parent).or_default().push(child);
            }
        }
        let mut ids = std::collections::HashSet::new();
        let mut pending = vec![id];
        while let Some(next) = pending.pop() {
            if ids.insert(next) {
                pending.extend(children.get(&next).into_iter().flatten().copied());
            }
        }
        for i in 0..self.main.paras.len() {
            let p = &self.main.paras[i];
            if !p.runs.iter().any(|r| ids.contains(&r.chp.comment)) {
                continue;
            }
            let mut p = p.clone();
            let n = p.len();
            p.format_range(0, n, &mut |c| {
                if ids.contains(&c.comment) {
                    c.comment = 0;
                }
            });
            self.splice(StoryId::Main, i, 1, vec![p]);
        }
        // Marks last to first, so earlier positions stay put.
        let mut marks: Vec<Position> = Vec::new();
        for (i, p) in self.main.paras.iter().enumerate() {
            for (off, c) in p.text.char_indices() {
                if c == chars::ANNOTATION && ids.contains(&p.chp_of_char(off).object) {
                    marks.push(Position::new(i, off));
                }
            }
        }
        for m in marks.into_iter().rev() {
            let end = Position::new(m.para, m.offset + chars::ANNOTATION.len_utf8());
            // Comments are review metadata: removing one must also work
            // while body text is being tracked. Do not leave a deleted mark.
            let mut p = self.main.paras[m.para].clone();
            p.remove(m.offset, end.offset);
            self.splice(StoryId::Main, m.para, 1, vec![p]);
        }
        Some(at)
    }

    /// Remove the marks of the replies to annotations `gone`, whose marks
    /// an edit just took from the text (deleted by hand, say), and the
    /// replies under those. A reply cannot stand without what it answers;
    /// Delete Annotation takes them together too. Called from that edit,
    /// so one Undo brings the whole thread back. Replies a file arrived
    /// with no annotation for are left alone.
    pub(crate) fn drop_orphan_replies(&mut self, mut gone: std::collections::HashSet<u32>) {
        loop {
            let orphan = |id: u32| {
                self.annotation_info
                    .get(&id)
                    .and_then(|i| i.parent)
                    .is_some_and(|p| gone.contains(&p))
            };
            let mut found = None;
            'paras: for (i, p) in self.main.paras.iter().enumerate() {
                if !p.text.contains(chars::ANNOTATION) {
                    continue;
                }
                for (off, c) in p.text.char_indices() {
                    if c == chars::ANNOTATION && orphan(p.chp_of_char(off).object) {
                        found = Some(Position::new(i, off));
                        break 'paras;
                    }
                }
            }
            let Some(m) = found else {
                break;
            };
            gone.insert(self.main.paras[m.para].chp_of_char(m.offset).object);
            let mut p = self.main.paras[m.para].clone();
            p.remove(m.offset, m.offset + chars::ANNOTATION.len_utf8());
            self.splice(StoryId::Main, m.para, 1, vec![p]);
        }
    }

    /// The annotation a thread of replies starts with.
    pub fn thread_start(&self, mut id: u32) -> u32 {
        for _ in 0..64 {
            match self.annotation_info.get(&id).and_then(|i| i.parent) {
                Some(p) if self.annotation_info.contains_key(&p) => id = p,
                _ => break,
            }
        }
        id
    }

    /// Mark annotation `id` resolved, or open again.
    pub fn set_annotation_done(&mut self, id: u32, done: bool) {
        if let Some(info) = self.annotation_info.get_mut(&id)
            && info.done != done
        {
            info.done = done;
            self.touch();
        }
    }

    /// Where footnote `id`'s text starts in the notes story.
    pub fn note_text_start(&self, id: u32) -> Option<Position> {
        text_start_in(&self.notes.paras, id, NoteKind::Foot)
    }

    /// Where annotation `id`'s text starts in the annotations story.
    pub fn annotation_text_start(&self, id: u32) -> Option<Position> {
        text_start_in(&self.annotations.paras, id, NoteKind::Annotation)
    }

    /// An annotation's text, without its mark.
    pub fn annotation_text(&self, id: u32) -> String {
        let Some(r) = self.annotation_ranges().get(&id).cloned() else {
            return String::new();
        };
        let mut lines: Vec<String> = self.annotations.paras[r]
            .iter()
            .map(|p| p.text.clone())
            .collect();
        if let Some(first) = lines.first_mut() {
            let rest = first
                .strip_prefix(chars::ANNOTATION)
                .unwrap_or(first)
                .trim_start()
                .to_string();
            *first = rest;
        }
        lines.join("\n")
    }

    /// What each annotation mark shows: its initials and its number, as
    /// "LK1".
    pub fn annotation_labels(&self) -> HashMap<u32, String> {
        let mut out = HashMap::new();
        for (n, id) in annotation_ids(&self.main.paras).into_iter().enumerate() {
            let initials = self
                .annotation_info
                .get(&id)
                .map_or("", |i| i.initials.as_str());
            out.insert(id, format!("{initials}{}", n + 1));
        }
        out
    }

    /// What each reference shows: its own mark, or its number. Numbers
    /// count the automatic notes from Number From; `page_of` restarts
    /// them on each page when Restart Each Page is on.
    pub fn note_labels(&self, page_of: Option<&dyn Fn(u32) -> usize>) -> HashMap<u32, String> {
        let mut out = HashMap::new();
        let mut n = self.props.footnote_start.max(1) as u32;
        let mut page = None;
        for id in note_ids(&self.main.paras) {
            if let Some(m) = self.note_marks.get(&id) {
                out.insert(id, m.clone());
                continue;
            }
            if let Some(f) = page_of.filter(|_| self.props.footnote_restart_each_page) {
                let p = f(id);
                if page != Some(p) {
                    n = 1;
                    page = Some(p);
                }
            }
            out.entry(id).or_insert_with(|| {
                n += 1;
                (n - 1).to_string()
            });
        }
        out
    }

    /// Copies of the notes the references in `paras` name, for the
    /// clipboard.
    pub(crate) fn notes_for(&self, paras: &[Paragraph]) -> Vec<NoteCopy> {
        let mut out = Vec::new();
        for kind in NoteKind::ALL {
            let ranges = self.ranges(kind);
            let story = &self.story(kind.story()).paras;
            for id in ref_ids(paras, kind) {
                let Some(r) = ranges.get(&id) else {
                    continue;
                };
                out.push(NoteCopy {
                    kind,
                    id,
                    mark: self.note_marks.get(&id).cloned(),
                    info: self.annotation_info.get(&id).cloned(),
                    paras: story[r.clone()].to_vec(),
                });
            }
        }
        out
    }

    /// Give pasted notes new ids so they are copies, not the originals,
    /// and put their text where the next rebuild finds it.
    pub(crate) fn adopt_notes(&mut self, paras: &mut [Paragraph], notes: &[NoteCopy]) {
        for n in notes {
            let new = self.fresh_note_id();
            let swap = |p: &mut Paragraph| set_note_id(p, n.id, new, n.kind.mark());
            paras.iter_mut().for_each(swap);
            let mut text = n.paras.clone();
            text.iter_mut().for_each(swap);
            if let Some(m) = &n.mark {
                self.note_marks.insert(new, m.clone());
            }
            if let Some(i) = &n.info {
                self.annotation_info.insert(new, i.clone());
            }
            self.note_bank.insert(new, text);
        }
    }

    /// The notes story as a whole, for a reader building a document.
    pub fn set_notes(&mut self, story: Story) {
        self.notes = story;
    }

    /// The annotations story as a whole, for a reader.
    pub fn set_annotations(&mut self, story: Story) {
        self.annotations = story;
    }
}

/// Change the id on `mark` characters naming `old` to `new`.
fn set_note_id(p: &mut Paragraph, old: u32, new: u32, mark: char) {
    let spans: Vec<(usize, usize)> = p
        .run_bounds()
        .filter(|(s, e, c)| c.object == old && p.text[*s..*e].contains(mark))
        .map(|(s, e, _)| (s, e))
        .collect();
    for (s, e) in spans {
        p.format_range(s, e, &mut |c| c.object = new);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::StoryId::Main;

    fn doc() -> Document {
        let mut d = Document::new();
        d.insert_text(
            Main,
            Position::new(0, 0),
            "One two three",
            CharProps::default(),
        );
        d
    }

    fn note_text(d: &Document, id: u32) -> String {
        let r = d.note_ranges()[&id].clone();
        d.notes.paras[r].iter().map(|p| p.text.clone()).collect()
    }

    #[test]
    fn notes_follow_their_references() {
        let mut d = doc();
        let sel = crate::Selection::default();
        let a = d.insert_note(Position::new(0, 13), CharProps::default(), None);
        let at = d.note_text_start(a).unwrap();
        d.insert_text(StoryId::Notes, at, "Last.", CharProps::default());
        let b = d.insert_note(Position::new(0, 3), CharProps::default(), None);
        d.commit(sel);
        // The new note goes first, as its reference does.
        assert_eq!(note_ids(&d.notes.paras), [b, a]);
        let labels = d.note_labels(None);
        assert_eq!((labels[&b].as_str(), labels[&a].as_str()), ("1", "2"));
        // Cutting the second reference and pasting it before the first
        // brings its text along.
        let end = d.main.paras[0].len();
        let start = Position::new(0, end - chars::FOOTNOTE.len_utf8());
        let frag = d.copy(Main, start, Position::new(0, end));
        d.begin("Cut", sel);
        d.delete(Main, start, Position::new(0, end));
        d.commit(sel);
        assert_eq!(note_ids(&d.notes.paras), [b]);
        d.begin("Paste", sel);
        d.insert_fragment(Main, Position::new(0, 0), &frag);
        d.commit(sel);
        let first = note_ids(&d.main.paras)[0];
        assert_ne!(first, a, "a pasted note is a copy");
        assert!(note_text(&d, first).ends_with("Last."));
        d.undo();
        d.undo();
        assert_eq!(note_ids(&d.notes.paras), [b, a]);
    }

    #[test]
    fn custom_marks_do_not_count() {
        let mut d = doc();
        let a = d.insert_note(Position::new(0, 3), CharProps::default(), Some("*".into()));
        let b = d.insert_note(Position::new(0, 7), CharProps::default(), None);
        let labels = d.note_labels(None);
        assert_eq!((labels[&a].as_str(), labels[&b].as_str()), ("*", "1"));
    }

    #[test]
    fn deleting_an_annotation_takes_its_replies_and_undo_brings_them_back() {
        let mut d = doc();
        let sel = crate::Selection::default();
        d.begin("Annotation", sel);
        let a = d.insert_annotation_on(
            Position::new(0, 4),
            Position::new(0, 7),
            CharProps::default(),
            AnnotationInfo::default(),
        );
        let reply = d.insert_reply(a, AnnotationInfo::default()).unwrap();
        let other = d.insert_annotation(
            Position::new(0, 0),
            CharProps::default(),
            AnnotationInfo::default(),
        );
        d.commit(sel);
        let before = d.main.paras[0].clone();
        assert_eq!(annotation_ids(&d.annotations.paras), [other, a, reply]);
        d.begin("Delete Annotation", sel);
        d.delete_annotation(a);
        d.commit(sel);
        let p = &d.main.paras[0];
        assert_eq!(p.text.replace(chars::ANNOTATION, "#"), "#One two three");
        assert!(p.runs.iter().all(|r| r.chp.comment == 0));
        assert_eq!(annotation_ids(&d.annotations.paras), [other]);
        d.undo();
        assert_eq!(d.main.paras[0], before);
        assert_eq!(annotation_ids(&d.annotations.paras), [other, a, reply]);
    }
    #[test]
    fn deleting_an_annotation_s_mark_by_hand_takes_its_replies() {
        let mut d = doc();
        let sel = crate::Selection::default();
        d.begin("Annotation", sel);
        let a = d.insert_annotation(
            Position::new(0, 4),
            CharProps::default(),
            AnnotationInfo::default(),
        );
        let reply = d.insert_reply(a, AnnotationInfo::default()).unwrap();
        d.insert_reply(reply, AnnotationInfo::default()).unwrap();
        let other = d.insert_annotation(
            Position::new(0, 0),
            CharProps::default(),
            AnnotationInfo::default(),
        );
        d.commit(sel);
        let before = d.main.clone();
        let notes = d.annotations.clone();
        // Delete only the first annotation's own mark, as the Delete key does.
        let at = d.annotation_mark(a).unwrap();
        d.begin("Clear", sel);
        let mut p = d.main.paras[at.para].clone();
        p.remove(at.offset, at.offset + chars::ANNOTATION.len_utf8());
        d.splice(StoryId::Main, at.para, 1, vec![p]);
        d.commit(sel);
        assert_eq!(annotation_ids(&d.main.paras), [other]);
        assert_eq!(annotation_ids(&d.annotations.paras), [other]);
        d.undo();
        assert_eq!(d.main, before);
        assert_eq!(d.annotations, notes);
        // A reply that arrived without its annotation stays when some other
        // annotation is deleted.
        d.annotation_info.get_mut(&reply).unwrap().parent = Some(999);
        d.begin("Delete Annotation", sel);
        d.delete_annotation(other);
        d.commit(sel);
        assert!(annotation_ids(&d.main.paras).contains(&reply));
    }

    #[test]
    fn delete_annotation_removes_deep_threads_even_with_tracking_on() {
        let mut d = doc();
        let sel = crate::Selection::default();
        d.begin("Comments", sel);
        let root = d.insert_annotation_on(
            Position::new(0, 0),
            Position::new(0, 3),
            CharProps::default(),
            AnnotationInfo::default(),
        );
        let mut parent = root;
        for _ in 0..80 {
            parent = d.insert_reply(parent, AnnotationInfo::default()).unwrap();
        }
        d.commit(sel);
        let before = d.main.clone();
        let notes = d.annotations.clone();
        d.props.track_changes = true;
        d.begin("Delete Annotation", sel);
        d.delete_annotation(root);
        d.commit(sel);
        assert_eq!(d.main.paras[0].text, "One two three");
        assert!(annotation_ids(&d.annotations.paras).is_empty());
        assert!(d.annotation_ranges().is_empty());
        assert!(!d.has_changes());
        d.undo();
        assert_eq!(d.main, before);
        assert_eq!(d.annotations, notes);
        d.redo();
        assert!(d.annotation_ranges().is_empty());
    }
}
