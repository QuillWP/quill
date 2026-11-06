//! Tracked changes in the window: Track Changes on and off, moving from
//! one change to the next, and accepting or rejecting them.

use super::*;
use quill_core::revision;

impl DocView {
    /// Revision starts in window coordinates, after the visible layout is prepared.
    pub fn revision_anchors(&self) -> Vec<(Position, Position, u32, f32)> {
        if !self.show_changes {
            return Vec::new();
        }
        self.doc
            .changes_in(MAIN)
            .into_iter()
            .filter_map(|(a, b, id)| {
                self.layouts.get(a.para)?.as_ref()?;
                if self.outline_hidden(a.para) {
                    return None;
                }
                let (_, y, h) = self.caret_geometry(a, false);
                let (_, wy) = self.to_window(0.0, y);
                (wy + h >= self.area.y && wy < self.area.bottom()).then_some((a, b, id, wy))
            })
            .collect()
    }
    /// Track Changes (⌘⇧E): turn tracking on or off for the document. The
    /// setting is saved with it, so turning it on is an undoable change.
    pub fn toggle_tracking(&mut self) {
        let on = !self.doc.props.track_changes;
        let props = quill_core::DocumentProps {
            track_changes: on,
            ..self.doc.props.clone()
        };
        self.doc
            .set_document_props("Track Changes", props, self.sel);
        self.status = Some(
            if on {
                "Track Changes is on: insertions and deletions are marked."
            } else {
                "Track Changes is off."
            }
            .to_string(),
        );
    }

    /// The tracked change at the insertion point (or just before it), or
    /// the first one the selection overlaps.
    pub fn change_here(&self) -> Option<(Position, Position, u32)> {
        let (s, e) = self.sel.range();
        let changes = self.doc.changes_in(MAIN);
        if s != e {
            return changes.into_iter().find(|c| c.0 < e && s < c.1);
        }
        changes
            .iter()
            .find(|c| c.0 <= s && s < c.1)
            .or_else(|| changes.iter().find(|c| c.1 == s))
            .copied()
    }

    /// Select the next (or previous) tracked change. False when there is
    /// none that way.
    pub fn go_to_change(&mut self, back: bool) -> bool {
        let (s, e) = self.sel.range();
        let changes = self.doc.changes_in(MAIN);
        let hit = if back {
            changes
                .iter()
                .rev()
                .find(|c| c.1 <= s && (c.0, c.1) != (s, e))
        } else {
            changes.iter().find(|c| c.0 >= e && (c.0, c.1) != (s, e))
        };
        match hit {
            Some(&(a, b, _)) => {
                self.select_found(a, b);
                true
            }
            None => false,
        }
    }

    /// Accept (or reject) the changes in the selection, or the change at
    /// the insertion point, then select the next one. False when there was
    /// nothing to accept or reject.
    pub fn resolve_change(&mut self, accept: bool) -> bool {
        let (s, e) = self.sel.range();
        let (s, e) = if s != e {
            (s, e)
        } else {
            match self.change_here() {
                Some((a, b, _)) => (a, b),
                None => return false,
            }
        };
        let label = if accept {
            "Accept Change"
        } else {
            "Reject Change"
        };
        self.doc.begin(label, self.sel);
        if accept {
            self.doc.accept_changes(MAIN, s, e);
        } else {
            self.doc.reject_changes(MAIN, s, e);
        }
        // Nothing before the change moves, so its start is still good.
        self.finish_edit(Selection::caret(s));
        self.go_to_change(false);
        true
    }

    /// Accept All Changes or Reject All Changes, as one undoable step.
    pub fn resolve_all(&mut self, accept: bool) {
        let label = if accept {
            "Accept All Changes"
        } else {
            "Reject All Changes"
        };
        self.doc.begin(label, self.sel);
        self.doc.resolve_all_changes(accept);
        let para = self.sel.start().para.min(self.doc.main.paras.len() - 1);
        self.finish_edit(Selection::caret(Position::new(para, 0)));
    }

    /// What the Review Changes window says about the change here: what it
    /// is, and who made it when. None when there is no change here.
    pub fn describe_change_here(&self) -> Option<(String, String)> {
        let (a, b, rev) = self.change_here()?;
        let r = revision::get(rev)?;
        let mark_only =
            b.para == a.para + 1 && b.offset == 0 && a.offset == self.doc.main.paras[a.para].len();
        let what = if mark_only { " paragraph mark" } else { "" };
        let (kind, by) = match (&r.inserted, &r.deleted, &r.formatted) {
            (Some(_), Some(d), _) => (format!("Inserted{what}, then deleted"), d),
            (None, Some(d), _) => (format!("Deleted{what}"), d),
            (Some(i), None, _) => (format!("Inserted{what}"), i),
            (None, None, Some((by, old))) => {
                let now = if mark_only {
                    self.doc.main.paras[a.para].mark_chp
                } else {
                    self.doc.main.paras[a.para].chp_of_char(a.offset)
                };
                let names = format_differences(old, &now, &self.doc);
                (format!("Formatted: {}", names.join(", ")), by)
            }
            (None, None, None) => return None,
        };
        Some((kind, format!("{}, {}", by.author, readable_date(&by.date))))
    }
}

/// "2026-10-23T14:05:00Z" as "2026-10-23 14:05". Anything else as it is.
pub(super) fn readable_date(date: &str) -> String {
    match date.get(..16) {
        Some(d) if date.as_bytes().get(10) == Some(&b'T') => d.replacen('T', " ", 1),
        _ => date.to_string(),
    }
}

/// The parts of a character format that changed, as the original names them.
fn format_differences(old: &CharProps, new: &CharProps, doc: &quill_core::Document) -> Vec<String> {
    let mut out = Vec::new();
    let mut flag = |on: bool, was: bool, name: &str| {
        if on != was {
            out.push(if on {
                name.to_string()
            } else {
                format!("Not {name}")
            });
        }
    };
    flag(new.bold, old.bold, "Bold");
    flag(new.italic, old.italic, "Italic");
    flag(
        new.underline != quill_core::Underline::None,
        old.underline != quill_core::Underline::None,
        "Underline",
    );
    flag(new.strike, old.strike, "Strikethrough");
    flag(new.outline, old.outline, "Outline");
    flag(new.shadow, old.shadow, "Shadow");
    flag(new.small_caps, old.small_caps, "Small Caps");
    flag(new.all_caps, old.all_caps, "All Caps");
    flag(new.hidden, old.hidden, "Hidden");
    if new.font != old.font {
        out.push(format!("Font: {}", doc.font_name(new.font)));
    }
    if new.size != old.size {
        let pt = new.size as f32 / 2.0;
        out.push(format!("{pt} pt"));
    }
    if new.color != old.color {
        out.push("Font Color".to_string());
    }
    if new.position != old.position {
        out.push(if new.position > 0 {
            "Superscript".to_string()
        } else if new.position < 0 {
            "Subscript".to_string()
        } else {
            "Normal Position".to_string()
        });
    }
    if new.spacing != old.spacing {
        out.push("Spacing".to_string());
    }
    if out.is_empty() {
        out.push("Format".to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracked(text: &str) -> DocView {
        let mut v = DocView::new(quill_core::Document::new(), "t".into());
        v.insert_text(text);
        v.doc.forget_history();
        v.doc.props.track_changes = true;
        v.doc.author = "Ada".into();
        v
    }

    fn at(para: usize, offset: usize) -> Position {
        Position::new(para, offset)
    }

    #[test]
    fn arrows_skip_hidden_deletions_without_phantom_selection_stops() {
        for tracking in [true, false] {
            let mut v = tracked("aOLDnew");
            v.doc.delete(MAIN, at(0, 1), at(0, 4));
            v.doc.props.track_changes = tracking;
            v.show_changes = false;
            let before = v.doc.main.clone();
            let mut clip = Clipboard::new();
            v.set_selection(Selection::caret(at(0, 4)));
            v.exec(Cmd::Left, false, &mut clip);
            assert_eq!(
                v.sel.focus,
                at(0, 0),
                "one press must cross a visible character"
            );
            v.exec(Cmd::Right, false, &mut clip);
            assert_eq!(v.sel.focus, at(0, 4));
            v.exec(Cmd::Right, false, &mut clip);
            assert_eq!(v.sel.focus, at(0, 5));
            v.exec(Cmd::Left, true, &mut clip);
            assert_eq!(v.sel.range(), (at(0, 4), at(0, 5)));
            v.exec(Cmd::Left, true, &mut clip);
            assert_eq!(v.sel.focus, at(0, 0));
            v.exec(Cmd::Right, true, &mut clip);
            v.exec(Cmd::Right, true, &mut clip);
            assert!(
                v.sel.is_empty(),
                "reversing selection must not leave hidden text selected"
            );
            // A mouse hit may report either logical end of a zero-width run.
            v.set_selection(Selection::caret(at(0, 1)));
            v.exec(Cmd::Left, true, &mut clip);
            v.exec(Cmd::Right, true, &mut clip);
            assert!(v.sel.is_empty());
            assert_eq!(v.doc.main, before, "navigation must preserve the revisions");
            v.show_changes = true;
            v.set_selection(Selection::caret(at(0, 4)));
            v.exec(Cmd::Left, false, &mut clip);
            assert_eq!(v.sel.focus, at(0, 3), "shown deletions remain navigable");
        }
    }

    #[test]
    fn hidden_revision_navigation_handles_unicode_edges_and_paragraphs() {
        let mut v = DocView::new(
            quill_core::text::from_plain_text("é旧\u{1f600}z\nnext"),
            "t".into(),
        );
        v.doc.props.track_changes = true;
        v.doc.delete(MAIN, at(0, 2), at(0, 9));
        let other = revision::intern(revision::Revision {
            deleted: Some(revision::Stamp::new("Grace", "2026-10-23")),
            ..Default::default()
        });
        v.doc
            .format_chars(MAIN, at(0, 5), at(0, 9), |c| c.rev = other);
        v.show_changes = false;
        let mut clip = Clipboard::new();
        v.set_selection(Selection::caret(at(0, 9)));
        v.exec(Cmd::Left, false, &mut clip);
        assert_eq!(v.sel.focus, at(0, 0));
        v.exec(Cmd::Right, false, &mut clip);
        assert_eq!(v.sel.focus, at(0, 9));
        v.exec(Cmd::Right, false, &mut clip);
        assert_eq!(v.sel.focus, at(0, 10));
        v.exec(Cmd::Right, false, &mut clip);
        assert_eq!(v.sel.focus, at(1, 0));
        v.exec(Cmd::Left, false, &mut clip);
        assert_eq!(v.sel.focus, at(0, 10));
        for text in ["all gone", "旧\u{1f600}"] {
            let mut v = tracked(text);
            let end = v.doc.main.end();
            v.doc.delete(MAIN, at(0, 0), end);
            v.show_changes = false;
            v.set_selection(Selection::caret(end));
            v.exec(Cmd::Left, false, &mut clip);
            v.exec(Cmd::Right, false, &mut clip);
            assert_eq!(v.sel.focus, end);
        }
    }

    #[test]
    fn hidden_revisions_have_only_visible_caret_stops_in_layout() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let mut v = tracked("aOLDnew");
        v.doc.delete(MAIN, at(0, 1), at(0, 4));
        v.show_changes = false;
        v.update_layout(&mut fonts);
        v.set_selection(Selection::caret(at(0, 4)));
        let mut clip = Clipboard::new();
        let mut x = v.caret_geometry(v.sel.focus, false).0;
        for cmd in [Cmd::Left, Cmd::Right, Cmd::Right, Cmd::Left] {
            v.exec(cmd, false, &mut clip);
            let next = v.caret_geometry(v.sel.focus, false).0;
            assert!(
                (next - x).abs() > 0.1,
                "each press must visibly move the caret"
            );
            x = next;
        }
    }

    #[test]
    fn next_and_previous_step_through_changes_and_accept_moves_on() {
        let mut v = tracked("one two three");
        v.doc.delete(MAIN, at(0, 0), at(0, 3));
        v.doc.delete(MAIN, at(0, 8), at(0, 13));
        v.set_selection(Selection::caret(at(0, 0)));
        assert!(v.go_to_change(false));
        assert_eq!(v.sel.range(), (at(0, 0), at(0, 3)));
        assert!(v.go_to_change(false));
        assert_eq!(v.sel.range(), (at(0, 8), at(0, 13)));
        assert!(!v.go_to_change(false), "no more after the last");
        assert!(v.go_to_change(true));
        assert_eq!(v.sel.range(), (at(0, 0), at(0, 3)));

        let (what, who) = v.describe_change_here().unwrap();
        assert_eq!(what, "Deleted");
        assert!(who.starts_with("Ada, "), "{who}");

        // Accepting the first removes "one" and selects the next change.
        assert!(v.resolve_change(true));
        assert_eq!(v.doc.main.paras[0].text, " two three");
        assert_eq!(v.sel.range(), (at(0, 5), at(0, 10)));
        assert!(v.resolve_change(false));
        assert_eq!(v.doc.main.paras[0].text, " two three");
        assert!(!v.doc.has_changes());
        assert!(!v.resolve_change(true), "nothing left");
    }

    #[test]
    fn format_changes_are_described_by_what_changed() {
        let mut v = tracked("plain");
        v.doc.format_chars(MAIN, at(0, 0), at(0, 5), |c| {
            c.bold = true;
            c.size = 28;
        });
        v.set_selection(Selection::caret(at(0, 2)));
        let (what, _) = v.describe_change_here().unwrap();
        assert_eq!(what, "Formatted: Bold, 14 pt");
    }

    #[test]
    fn track_changes_is_saved_with_the_document_and_undoable() {
        let mut v = DocView::new(quill_core::Document::new(), "t".into());
        v.toggle_tracking();
        assert!(v.doc.props.track_changes && v.doc.is_modified());
        v.doc.undo();
        assert!(!v.doc.props.track_changes);
    }

    #[test]
    fn backspace_and_delete_step_over_text_already_deleted() {
        let mut clip = Clipboard::new();
        let mut v = tracked("abcd");
        v.set_selection(Selection::caret(at(0, 3)));
        v.exec(Cmd::Backspace, false, &mut clip);
        assert_eq!(v.sel.range(), (at(0, 2), at(0, 2)));
        v.exec(Cmd::Backspace, false, &mut clip);
        assert_eq!(v.sel.range(), (at(0, 1), at(0, 1)));
        let deleted: Vec<bool> = (0..4)
            .map(|i| revision::is_deleted(v.doc.main.paras[0].chp_of_char(i).rev))
            .collect();
        assert_eq!(deleted, [false, true, true, false]);

        // Forward Delete moves on past what it deleted.
        v.set_selection(Selection::caret(at(0, 0)));
        v.exec(Cmd::DeleteForward, false, &mut clip);
        assert_eq!(v.sel.range(), (at(0, 1), at(0, 1)));
        v.exec(Cmd::DeleteForward, false, &mut clip);
        assert_eq!(v.sel.range(), (at(0, 4), at(0, 4)));
        assert_eq!(v.doc.main.paras[0].text, "abcd", "nothing removed");
    }

    #[test]
    fn dates_read_plainly() {
        assert_eq!(readable_date("2026-10-23T14:05:00Z"), "2026-10-23 14:05");
        assert_eq!(readable_date("someday"), "someday");
    }
}
