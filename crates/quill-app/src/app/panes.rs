//! Document editing panes for a window.

use super::*;

impl WinState {
    pub(super) fn view_zoom(&self) -> f32 {
        self.document_zoom
    }

    pub(super) fn document_point(&self, x: f32, y: f32) -> (f32, f32) {
        let z = self.view_zoom();
        (x / z, y / z)
    }

    // ----- the header and footer pane -----

    /// The main text's view, wherever focus is.
    pub fn main_view(&self) -> &DocView {
        match &self.pane {
            Some(p) if p.focused => &p.other,
            _ => &self.view,
        }
    }

    pub fn main_view_mut(&mut self) -> &mut DocView {
        match &mut self.pane {
            Some(p) if p.focused => &mut p.other,
            _ => &mut self.view,
        }
    }

    pub(super) fn pane_view_mut(&mut self) -> Option<&mut DocView> {
        match &mut self.pane {
            Some(p) if p.focused => Some(&mut self.view),
            Some(p) => Some(&mut p.other),
            None => None,
        }
    }

    pub(super) fn pane_view(&self) -> Option<&DocView> {
        match &self.pane {
            Some(p) if p.focused => Some(&self.view),
            Some(p) => Some(&p.other),
            None => None,
        }
    }

    /// Open the footnote pane with the insertion point at the start of
    /// note `id`'s text.
    pub fn open_note(&mut self, id: u32) {
        self.open_note_in(quill_core::StoryId::Notes, id);
    }

    /// Open the footnote or annotation pane at note `id`.
    pub fn open_note_in(&mut self, story: quill_core::StoryId, id: u32) {
        use quill_core::notes::NoteKind;
        let kind = if story == quill_core::StoryId::Annotations {
            NoteKind::Annotation
        } else {
            NoteKind::Foot
        };
        self.close_pane();
        self.open_pane(story);
        let at = quill_core::notes::text_start_in(&self.view.doc.main.paras, id, kind);
        if let Some(at) = at {
            self.view.set_selection(Selection::caret(at));
            self.view.scroll_to_caret();
        }
    }

    /// Open a header or footer in the pane (View > Header, or a click in a
    /// page's header in Page Layout view).
    /// Open a pane on `story`; a header or footer is the insertion point's
    /// section's.
    pub fn open_pane(&mut self, story: quill_core::StoryId) {
        let main = self.main_view();
        let section = main.doc.section_at(main.sel.focus.para);
        self.open_pane_in(story, section);
    }

    /// Open a pane on header or footer `story` of `section`.
    pub fn open_pane_in(&mut self, story: quill_core::StoryId, section: usize) {
        if story == quill_core::StoryId::Annotations {
            self.review_closed = false;
            if self.review_filter == revisions::Filter::Changes {
                self.review_filter = revisions::Filter::All;
            }
        }
        if self
            .pane
            .as_ref()
            .is_some_and(|p| p.story == story && p.section == section)
        {
            self.focus_pane();
            return;
        }
        self.close_pane();
        if story == quill_core::StoryId::Annotations {
            self.review_closed = false;
        }
        self.view.doc.header_section = section;
        let main = &self.view;
        let doc = main.doc.copy_story_for_edit(story);
        // With more than one section, say whose header it is.
        let name = if story.header_index_checked().is_some() && main.doc.sections().len() > 1 {
            format!("{} (Section {})", story.name(), section + 1)
        } else {
            story.name().to_string()
        };
        let mut v = DocView::new(doc, format!("{}:{name}", main.title));
        v.show_marks = main.show_marks;
        v.show_changes = main.show_changes;
        v.captions = story == quill_core::StoryId::Annotations;
        self.pane = Some(Pane {
            story,
            section,
            other: v,
            focused: false,
            bounds: Rect::default(),
        });
        self.focus_pane();
    }

    /// Reply to Annotation (`reply`) or Resolve Annotation, from the text
    /// or from the annotations pane. A reply opens in the pane to be
    /// typed.
    pub fn annotation_command(&mut self, reply: bool) {
        let Some(id) = self.view.annotation_here() else {
            self.view.status = Some(
                "Put the insertion point by an annotation's mark or in its text, or in the \
                 annotations pane, first."
                    .into(),
            );
            return;
        };
        let in_pane = self
            .pane
            .as_ref()
            .is_some_and(|p| p.story == quill_core::StoryId::Annotations);
        // The pane edits a copy: put its text back before changing the
        // document.
        self.close_pane();
        if reply {
            if let Some(new) = self.view.reply_to(id) {
                self.open_note_in(quill_core::StoryId::Annotations, new);
            }
            return;
        }
        // Resolving any part of a thread resolves the thread.
        let top = self.view.doc.thread_start(id);
        let done = !self
            .view
            .doc
            .annotation_info
            .get(&top)
            .is_some_and(|i| i.done);
        self.view.doc.set_annotation_done(top, done);
        if in_pane {
            self.open_note_in(quill_core::StoryId::Annotations, id);
        }
        self.view.status = Some(if done { "Resolved." } else { "Reopened." }.into());
    }

    /// Delete Annotation: the annotation at the insertion point, with its
    /// replies.
    pub fn delete_annotation(&mut self) {
        let Some(id) = self.view.annotation_here() else {
            self.view.status = Some(
                "Put the insertion point by an annotation's mark or in its text, or in the \
                 annotations pane, first."
                    .into(),
            );
            return;
        };
        // The pane edits a copy: put its text back before changing the
        // document.
        self.close_pane();
        self.view.delete_annotation(id);
        self.view.status = Some("Annotation deleted.".into());
    }

    pub fn close_pane(&mut self) {
        if self
            .pane
            .as_ref()
            .is_some_and(|p| p.story == quill_core::StoryId::Annotations)
        {
            self.review_closed = true;
        }
        self.focus_main();
        self.sync_pane();
        self.pane = None;
    }

    /// Give the pane the keyboard: swap its view into `view`.
    pub(super) fn focus_pane(&mut self) {
        let Some(p) = &mut self.pane else {
            return;
        };
        if p.focused {
            return;
        }
        // The main text may have gained fonts or changed styles and
        // settings meanwhile; bring the pane's copy up to date.
        self.view.doc.header_section = p.section;
        let main = &self.view.doc;
        if p.other.doc.fonts != main.fonts || p.other.doc.styles != main.styles {
            let pd = &mut p.other.doc;
            pd.fonts = main.fonts.clone();
            pd.main = main.story(p.story).clone();
            pd.forget_history();
            p.other.invalidate_layout();
            p.other.clamp_selection();
        }
        let pd = &mut p.other.doc;
        pd.styles = main.styles.clone();
        pd.props = main.props.clone();
        pd.section = main.section.clone();
        std::mem::swap(&mut self.view, &mut p.other);
        p.focused = true;
    }

    /// Give the main text the keyboard back.
    pub(super) fn focus_main(&mut self) {
        if !self.pane.as_ref().is_some_and(|p| p.focused) {
            return;
        }
        self.sync_pane();
        if let Some(p) = &mut self.pane {
            std::mem::swap(&mut self.view, &mut p.other);
            p.focused = false;
        }
    }

    /// Copy the pane's text into the document's header or footer, and any
    /// settings changed while it had focus.
    pub fn sync_pane(&mut self) {
        let WinState { view, pane, .. } = self;
        let Some(p) = pane else {
            return;
        };
        let story = p.story;
        let section = p.section;
        let focused = p.focused;
        // An inactive pane follows the main document, including global style
        // changes to header text. It must not copy an older version back.
        if !focused {
            view.doc.header_section = section;
            let story = view.doc.story(story);
            if p.other.doc.main != *story
                || p.other.doc.styles != view.doc.styles
                || p.other.doc.fonts != view.doc.fonts
                || p.other.doc.objects != view.doc.objects
            {
                p.other.doc.main = story.clone();
                p.other.doc.fonts = view.doc.fonts.clone();
                p.other.doc.styles = view.doc.styles.clone();
                p.other.doc.objects = view.doc.objects.clone();
                p.other.doc.note_marks = view.doc.note_marks.clone();
                p.other.doc.annotation_info = view.doc.annotation_info.clone();
                p.other.doc.forget_history();
                p.other.invalidate_layout();
                p.other.clamp_selection();
            }
            return;
        }
        let (pane_v, main_v): (&DocView, &mut DocView) = if focused {
            (view, &mut p.other)
        } else {
            (&p.other, view)
        };
        main_v.doc.header_section = section;
        let identical = main_v.doc.fonts == pane_v.doc.fonts
            && main_v.doc.objects == pane_v.doc.objects
            && main_v.doc.story(story) == &pane_v.doc.main;
        if !identical {
            main_v.doc.import_styled_story(
                story,
                &pane_v.doc.main,
                &pane_v.doc.styles,
                &pane_v.doc.fonts,
                &pane_v.doc.objects,
            );
        }
        if focused {
            if main_v.doc.props != pane_v.doc.props {
                main_v.set_document_props("Document", pane_v.doc.props.clone());
            }
            if main_v.doc.section != pane_v.doc.section {
                main_v.set_last_section("Section", pane_v.doc.section.clone());
            }
        }
    }
}
