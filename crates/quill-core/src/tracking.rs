//! Track Changes while editing: what tracking does to insertions,
//! deletions and format changes, and accepting and rejecting them. The
//! changes themselves live in `crate::revision`.

use crate::document::{Document, StoryId};
use crate::para::{Mark, Paragraph};
use crate::props::CharProps;
use crate::revision::{self, Revision, Stamp};
use crate::story::Position;

/// A format without its tracked change or comment range, for comparing
/// formats.
fn plain(c: &CharProps) -> CharProps {
    CharProps {
        rev: 0,
        comment: 0,
        ..*c
    }
}

/// Paragraphs as they would be with every tracked change accepted: what
/// plain text, word counts and other programs see.
pub fn accepted(paras: &[Paragraph]) -> Vec<Paragraph> {
    Document::fresh_paste(paras, 0)
}

/// Whether any paragraph holds a tracked change.
pub fn any_changes(paras: &[Paragraph]) -> bool {
    paras
        .iter()
        .any(|p| p.mark_chp.rev != 0 || p.runs.iter().any(|r| r.chp.rev != 0))
}

impl Document {
    /// The stamp new changes get, or None when Track Changes is off.
    pub(crate) fn tracking(&self) -> Option<Stamp> {
        self.props.track_changes.then(|| {
            let mut name = self.author.trim().to_string();
            if name.is_empty() {
                name = revision::default_author().trim().to_string();
            }
            if name.is_empty() {
                name = "Unknown".into();
            }
            Stamp::new(&name, &revision::now())
        })
    }

    /// The revision new text gets: an insertion while tracking, else none.
    /// Text typed next to a change never inherits it.
    pub(crate) fn new_text_rev(&self) -> u32 {
        self.tracking().map_or(0, |by| {
            revision::intern(Revision {
                inserted: Some(by),
                ..Revision::default()
            })
        })
    }

    /// Rebuild `start..end` of a story, deciding for each run, and each
    /// paragraph mark inside the range, whether it stays (and with what
    /// format) or goes. A mark that goes joins its paragraph to the next,
    /// which keeps its format, as Delete does; table marks always stay.
    /// Returns the new end of the range.
    fn rework(
        &mut self,
        story: StoryId,
        start: Position,
        end: Position,
        decide: impl Fn(&CharProps) -> Option<CharProps>,
    ) -> Position {
        let (start, end) = (start.min(end), start.max(end));
        if start == end {
            return start;
        }
        let old = &self.story(story).paras[start.para..=end.para];
        let tail = old[old.len() - 1].len() - end.offset;
        let mut out: Vec<Paragraph> = Vec::with_capacity(old.len());
        let mut join = false;
        for (k, p) in old.iter().enumerate() {
            let i = start.para + k;
            let s = if k == 0 { start.offset } else { 0 };
            let e = if i == end.para { end.offset } else { p.len() };
            let mut p = p.clone();
            let runs: Vec<(usize, usize, CharProps)> = p
                .run_bounds()
                .filter_map(|(a, b, c)| {
                    let (a, b) = (a.max(s), b.min(e));
                    (a < b).then_some((a, b, *c))
                })
                .collect();
            // Back to front, so removing one run leaves the others' offsets.
            for &(a, b, c) in runs.iter().rev() {
                match decide(&c) {
                    None => {
                        p.remove(a, b);
                    }
                    Some(n) if n != c => p.format_range(a, b, &mut |x| *x = n),
                    Some(_) => {}
                }
            }
            let mut mark_goes = false;
            if i < end.para {
                match decide(&p.mark_chp) {
                    Some(n) => p.mark_chp = n,
                    None if matches!(p.mark, Mark::CellEnd | Mark::RowEnd) => p.mark_chp.rev = 0,
                    None => mark_goes = true,
                }
            }
            if join {
                let mut prev = out.pop().expect("a paragraph to join");
                prev.append(&p);
                prev.pap = p.pap.clone();
                prev.mark = p.mark;
                prev.mark_chp = p.mark_chp;
                prev.row = p.row.clone();
                prev.section = p.section.clone();
                p = prev;
            }
            out.push(p);
            join = mark_goes;
        }
        let last = &out[out.len() - 1];
        let new_end = Position::new(start.para + out.len() - 1, last.len() - tail);
        if out[..] != self.story(story).paras[start.para..=end.para] {
            self.splice(story, start.para, end.para - start.para + 1, out);
        }
        new_end
    }

    /// Delete with Track Changes on: text stays, marked deleted, except
    /// text the same author inserted, which goes at once. Returns the new
    /// end of the range; the start does not move.
    pub(crate) fn delete_tracked(
        &mut self,
        story: StoryId,
        start: Position,
        end: Position,
        by: &Stamp,
    ) -> Position {
        self.rework(story, start, end, |c| {
            let Some(r) = revision::get(c.rev) else {
                return Some(CharProps {
                    rev: revision::intern(Revision {
                        deleted: Some(by.clone()),
                        ..Revision::default()
                    }),
                    ..*c
                });
            };
            if r.deleted.is_some() {
                Some(*c)
            } else if r.inserted.as_ref().is_some_and(|i| i.author == by.author) {
                None
            } else {
                Some(CharProps {
                    rev: revision::update(c.rev, |r| r.deleted = Some(by.clone())),
                    ..*c
                })
            }
        })
    }

    /// Accept the tracked changes in `start..end`: insertions and format
    /// changes stay as they are, deleted text goes. Returns the new end.
    pub fn accept_changes(&mut self, story: StoryId, start: Position, end: Position) -> Position {
        self.rework(story, start, end, |c| match revision::get(c.rev) {
            None => Some(*c),
            Some(r) if r.deleted.is_some() => None,
            Some(_) => Some(CharProps { rev: 0, ..*c }),
        })
    }

    /// Reject the tracked changes in `start..end`: inserted text goes,
    /// deleted text comes back, and changed formats return to what they
    /// were. Returns the new end.
    pub fn reject_changes(&mut self, story: StoryId, start: Position, end: Position) -> Position {
        self.rework(story, start, end, |c| match revision::get(c.rev) {
            None => Some(*c),
            Some(r) if r.inserted.is_some() => None,
            Some(r) => Some(match &r.formatted {
                // What the text is (a picture, a note's mark), its link and
                // its comment are not part of the format change.
                Some((_, old)) => CharProps {
                    object: c.object,
                    link: c.link,
                    comment: c.comment,
                    ..plain(old)
                },
                None => CharProps { rev: 0, ..*c },
            }),
        })
    }

    /// Give runs and marks the format change Track Changes records when
    /// `f` changes them. Text inserted while tracking just changes: its
    /// insertion covers its format.
    pub(crate) fn track_format(before: CharProps, c: &mut CharProps, by: &Stamp) {
        if plain(&before) == plain(c) {
            return;
        }
        let r = revision::get(before.rev);
        if r.as_ref().is_some_and(|r| r.inserted.is_some()) {
            return;
        }
        // The format before the first change is the one kept.
        let old = r
            .as_ref()
            .and_then(|r| r.formatted.as_ref().map(|f| f.1))
            .unwrap_or(plain(&before));
        let now = plain(c);
        c.rev = revision::update(before.rev, |r| {
            r.formatted = (now != old).then(|| (by.clone(), old));
        });
    }

    /// Paste's view of copied paragraphs: text already deleted is left
    /// out, a deleted paragraph mark joins its paragraph to the next, and
    /// every run and mark gets `rev` (an insertion while tracking, else
    /// none).
    pub(crate) fn fresh_paste(paras: &[Paragraph], rev: u32) -> Vec<Paragraph> {
        let mut out: Vec<Paragraph> = Vec::with_capacity(paras.len());
        let mut join = false;
        for (k, p) in paras.iter().enumerate() {
            let mut p = p.clone();
            let runs: Vec<(usize, usize, CharProps)> =
                p.run_bounds().map(|(a, b, c)| (a, b, *c)).collect();
            for &(a, b, c) in runs.iter().rev() {
                if revision::is_deleted(c.rev) {
                    p.remove(a, b);
                } else if c.rev != rev || c.comment != 0 {
                    // A comment's range stays with its comment.
                    p.format_range(a, b, &mut |x| {
                        x.rev = rev;
                        x.comment = 0;
                    });
                }
            }
            let mark_goes = revision::is_deleted(p.mark_chp.rev)
                && k + 1 < paras.len()
                && !matches!(p.mark, Mark::CellEnd | Mark::RowEnd);
            p.mark_chp.rev = rev;
            if join {
                let mut prev = out.pop().expect("a paragraph to join");
                prev.append(&p);
                prev.pap = p.pap.clone();
                prev.mark = p.mark;
                prev.mark_chp = p.mark_chp;
                prev.row = p.row.clone();
                prev.section = p.section.clone();
                p = prev;
            }
            out.push(p);
            join = mark_goes;
        }
        out
    }

    /// The tracked changes in a story, in order: each a stretch of text
    /// (paragraph marks included) with one revision.
    pub fn changes_in(&self, story: StoryId) -> Vec<(Position, Position, u32)> {
        let paras = &self.story(story).paras;
        let mut out: Vec<(Position, Position, u32)> = Vec::new();
        let mut add = |s: Position, e: Position, rev: u32| {
            if rev == 0 {
                return;
            }
            if let Some(last) = out.last_mut()
                && last.2 == rev
                && last.1 == s
            {
                last.1 = e;
            } else {
                out.push((s, e, rev));
            }
        };
        for (i, p) in paras.iter().enumerate() {
            for (a, b, c) in p.run_bounds() {
                add(Position::new(i, a), Position::new(i, b), c.rev);
            }
            if i + 1 < paras.len() {
                add(
                    Position::new(i, p.len()),
                    Position::new(i + 1, 0),
                    p.mark_chp.rev,
                );
            }
        }
        out
    }

    /// Accept (or reject) every tracked change in the document: the text,
    /// notes, annotations, and every section's headers and footers.
    pub fn resolve_all_changes(&mut self, accept: bool) {
        let resolve = |doc: &mut Document, id: StoryId| {
            let story = doc.story(id);
            let (s, e) = (story.start(), story.end());
            if accept {
                doc.accept_changes(id, s, e);
            } else {
                doc.reject_changes(id, s, e);
            }
        };
        for id in [StoryId::Main, StoryId::Notes, StoryId::Annotations] {
            resolve(self, id);
        }
        let shown = self.header_section;
        for k in 0..self.sections().len() {
            // A section using the one before's headers has none to change.
            if k > 0 && self.nth_section(k).headers.is_none() {
                continue;
            }
            self.header_section = k;
            for id in StoryId::HEADERS {
                resolve(self, id);
            }
        }
        self.header_section = shown;
    }

    /// Whether any story holds a tracked change.
    pub fn has_changes(&self) -> bool {
        let any = any_changes;
        any(&self.main.paras)
            || any(&self.notes.paras)
            || any(&self.annotations.paras)
            || self.headers.iter().any(|h| any(&h.paras))
            || self.main.paras.iter().any(|p| {
                p.section
                    .as_ref()
                    .and_then(|s| s.headers.as_ref())
                    .is_some_and(|hs| hs.iter().any(|h| any(&h.paras)))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::story::Selection;

    const MAIN: StoryId = StoryId::Main;

    fn p(para: usize, offset: usize) -> Position {
        Position::new(para, offset)
    }

    /// A document holding `lines`, written before Track Changes was on,
    /// then turned on for Ada.
    fn tracked(lines: &str) -> Document {
        let mut doc = Document::new();
        doc.insert_plain(MAIN, p(0, 0), lines, CharProps::default());
        doc.commit(Selection::default());
        doc.forget_history();
        doc.props.track_changes = true;
        doc.author = "Ada".into();
        doc
    }

    /// The text as marked up: [+inserted+], [-deleted-], [~reformatted~],
    /// with ¶ for a paragraph mark that is itself a change.
    fn markup(doc: &Document) -> String {
        let show = |rev: u32, s: &str| match revision::get(rev) {
            None => s.to_string(),
            Some(r) if r.deleted.is_some() => format!("[-{s}-]"),
            Some(r) if r.inserted.is_some() => format!("[+{s}+]"),
            Some(_) => format!("[~{s}~]"),
        };
        let paras = &doc.main.paras;
        let mut out = String::new();
        for (i, para) in paras.iter().enumerate() {
            for (a, b, c) in para.run_bounds() {
                out += &show(c.rev, &para.text[a..b]);
            }
            if i + 1 < paras.len() {
                out += &if para.mark_chp.rev == 0 {
                    "\n".to_string()
                } else {
                    show(para.mark_chp.rev, "¶") + "\n"
                };
            }
        }
        out
    }

    fn all(doc: &Document) -> (Position, Position) {
        (doc.main.start(), doc.main.end())
    }

    #[test]
    fn typing_and_deleting_are_marked_and_can_be_accepted_or_rejected() {
        let mut doc = tracked("Hello world");
        doc.insert_text(MAIN, p(0, 5), " big", CharProps::default());
        doc.delete(MAIN, p(0, 10), p(0, 15));
        assert_eq!(markup(&doc), "Hello[+ big+] [-world-]");

        let mut rejected = doc.clone();
        let (s, e) = all(&rejected);
        rejected.reject_changes(MAIN, s, e);
        assert_eq!(markup(&rejected), "Hello world");

        let (s, e) = all(&doc);
        doc.accept_changes(MAIN, s, e);
        assert_eq!(markup(&doc), "Hello big ");
        assert!(!doc.has_changes());
    }

    #[test]
    fn deleting_your_own_insertion_removes_it_but_not_someone_elses() {
        let mut doc = tracked("ab");
        doc.insert_text(MAIN, p(0, 1), "XY", CharProps::default());
        doc.delete(MAIN, p(0, 1), p(0, 2));
        assert_eq!(markup(&doc), "a[+Y+]b");

        doc.author = "Bo".into();
        doc.delete(MAIN, p(0, 1), p(0, 2));
        assert_eq!(markup(&doc), "a[-Y-]b");
        let rev = doc.main.paras[0].chp_of_char(1).rev;
        assert!(revision::is_inserted(rev) && revision::is_deleted(rev));
        // Rejecting removes Ada's insertion, so Bo's deletion of it goes too.
        let (s, e) = all(&doc);
        doc.reject_changes(MAIN, s, e);
        assert_eq!(markup(&doc), "ab");
    }

    #[test]
    fn deleting_already_deleted_text_changes_nothing_and_the_end_stays_put() {
        let mut doc = tracked("abc");
        let end = doc.delete_and_end(MAIN, p(0, 1), p(0, 2));
        assert_eq!(end, p(0, 2), "the deleted text is still there");
        let before = doc.main.clone();
        doc.delete(MAIN, p(0, 1), p(0, 2));
        assert_eq!(doc.main, before);
    }

    #[test]
    fn return_and_deleted_paragraph_marks_are_changes_too() {
        let mut doc = tracked("onetwo");
        let at = doc.split_paragraph(MAIN, p(0, 3));
        assert_eq!(at, p(1, 0));
        assert_eq!(markup(&doc), "one[+¶+]\ntwo");
        let mut rejected = doc.clone();
        let (s, e) = all(&rejected);
        rejected.reject_changes(MAIN, s, e);
        assert_eq!(markup(&rejected), "onetwo");

        // Someone else's mark is only marked deleted; accepting joins.
        let mut doc = tracked("one\ntwo");
        doc.delete(MAIN, p(0, 3), p(1, 0));
        assert_eq!(markup(&doc), "one[-¶-]\ntwo");
        assert_eq!(doc.main.paras.len(), 2);
        let (s, e) = all(&doc);
        doc.accept_changes(MAIN, s, e);
        assert_eq!(markup(&doc), "onetwo");
    }

    #[test]
    fn a_format_change_remembers_the_first_format_and_rejecting_restores_it() {
        let mut doc = tracked("plain text");
        doc.format_chars(MAIN, p(0, 0), p(0, 5), |c| c.bold = true);
        doc.format_chars(MAIN, p(0, 0), p(0, 5), |c| c.italic = true);
        assert_eq!(markup(&doc), "[~plain~] text");
        let rev = doc.main.paras[0].chp_of_char(0).rev;
        let old = revision::get(rev).unwrap().formatted.as_ref().unwrap().1;
        assert!(
            !old.bold && !old.italic,
            "the format before the first change"
        );

        let mut rejected = doc.clone();
        let (s, e) = all(&rejected);
        rejected.reject_changes(MAIN, s, e);
        let c = rejected.main.paras[0].chp_of_char(0);
        assert!(!c.bold && !c.italic && c.rev == 0);

        // Changing it back by hand is no change at all.
        doc.format_chars(MAIN, p(0, 0), p(0, 5), |c| {
            c.bold = false;
            c.italic = false;
        });
        assert_eq!(markup(&doc), "plain text");

        // Formatting text inserted while tracking is part of the insertion.
        doc.insert_text(MAIN, p(0, 0), "new ", CharProps::default());
        doc.format_chars(MAIN, p(0, 0), p(0, 3), |c| c.bold = true);
        assert_eq!(markup(&doc), "[+new+][+ +]plain text");
        let c = doc.main.paras[0].chp_of_char(0);
        assert!(c.bold && revision::get(c.rev).unwrap().formatted.is_none());
    }

    #[test]
    fn paste_leaves_deleted_text_behind_and_is_new_text() {
        let mut doc = tracked("keep gone\nnext");
        doc.delete(MAIN, p(0, 4), p(0, 9));
        doc.delete(MAIN, p(0, 9), p(1, 0));
        let frag = doc.copy(MAIN, p(0, 0), p(1, 4));
        let mut other = tracked("");
        other.author = "Bo".into();
        other.insert_fragment(MAIN, p(0, 0), &frag);
        assert_eq!(markup(&other), "[+keepnext+]");

        let mut untracked = Document::new();
        untracked.insert_fragment(MAIN, p(0, 0), &frag);
        assert_eq!(markup(&untracked), "keepnext");
    }

    #[test]
    fn typing_beside_a_change_does_not_join_it() {
        let mut doc = tracked("abc");
        doc.delete(MAIN, p(0, 1), p(0, 2));
        doc.props.track_changes = false;
        let chp = doc.main.paras[0].chp_at(2);
        doc.insert_text(MAIN, p(0, 2), "Z", chp);
        assert_eq!(markup(&doc), "a[-b-]Zc");
    }

    #[test]
    fn undo_puts_a_tracked_edit_back_exactly() {
        let mut doc = tracked("one\ntwo");
        let before = doc.main.clone();
        doc.begin("Clear", Selection::default());
        doc.delete(MAIN, p(0, 1), p(1, 2));
        doc.commit(Selection::default());
        assert_ne!(doc.main, before);
        doc.undo();
        assert_eq!(doc.main, before);
    }

    #[test]
    fn accept_all_and_reject_all_reach_notes_and_headers() {
        let mut doc = tracked("text");
        doc.insert_text(MAIN, p(0, 4), " more", CharProps::default());
        doc.insert_text(StoryId::Header, p(0, 0), "Head", CharProps::default());
        doc.insert_text(StoryId::Annotations, p(0, 0), "Note", CharProps::default());
        assert!(doc.has_changes());
        let mut rejected = doc.clone();
        rejected.resolve_all_changes(false);
        assert!(!rejected.has_changes());
        assert_eq!(rejected.main.paras[0].text, "text");
        assert_eq!(rejected.story(StoryId::Header).paras[0].text, "");
        doc.resolve_all_changes(true);
        assert!(!doc.has_changes());
        assert_eq!(doc.main.paras[0].text, "text more");
        assert_eq!(doc.story(StoryId::Header).paras[0].text, "Head");
    }

    #[test]
    fn comments_are_not_changes_and_keep_their_ranges_through_review() {
        let mut doc = tracked("look at this");
        let info = crate::notes::AnnotationInfo::default();
        let id = doc.insert_annotation_on(p(0, 5), p(0, 7), CharProps::default(), info);
        assert_eq!(
            markup(&doc),
            "look at\u{E004} this",
            "no change for the comment"
        );
        assert_eq!(doc.main.paras[0].chp_of_char(5).comment, id);
        doc.format_chars(MAIN, p(0, 5), p(0, 7), |c| c.bold = true);
        let (s, e) = all(&doc);
        doc.reject_changes(MAIN, s, e);
        let c = doc.main.paras[0].chp_of_char(5);
        assert!(!c.bold && c.comment == id);

        // A reply's mark follows the parent's, and earlier replies.
        let a = doc.insert_reply(id, Default::default()).unwrap();
        let b = doc.insert_reply(id, Default::default()).unwrap();
        let ids = crate::notes::annotation_ids(&doc.main.paras);
        assert_eq!(ids, [id, a, b]);
        assert_eq!(doc.annotation_info[&b].parent, Some(id));
    }

    #[test]
    fn changes_are_listed_in_order_with_their_marks() {
        let mut doc = tracked("ab\ncd");
        doc.delete(MAIN, p(0, 1), p(1, 1));
        let changes = doc.changes_in(MAIN);
        assert_eq!(changes.len(), 1, "one deletion: b, the mark and c");
        assert_eq!((changes[0].0, changes[0].1), (p(0, 1), p(1, 1)));
        doc.insert_text(MAIN, p(0, 0), "X", CharProps::default());
        let changes = doc.changes_in(MAIN);
        assert_eq!(changes.len(), 2);
        assert_eq!((changes[0].0, changes[0].1), (p(0, 0), p(0, 1)));
    }
}
