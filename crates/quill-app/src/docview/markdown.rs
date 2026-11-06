//! View > Markdown: a document shown as its Markdown source, to read and
//! edit as text, or as formatted text again.
//!
//! In Source view the window holds a plain document of the Markdown text
//! in a fixed-width font, and the formatted document it stands for is kept
//! to one side. Switching back with the text untouched gives that document
//! back exactly; switching back after editing reads the text as Markdown,
//! so whatever Markdown cannot carry is gone.

use super::*;
use quill_formats::markdown;

/// What a window in Source view keeps of the formatted document.
pub struct MarkdownSource {
    original: Document,
    /// The source as it was when shown, to tell whether it was edited.
    shown: String,
}

impl DocView {
    pub fn in_markdown_source(&self) -> bool {
        self.md_source.is_some()
    }

    /// The Markdown text being edited, in Source view.
    pub fn markdown_text(&self) -> Option<String> {
        self.md_source
            .as_ref()
            .map(|_| markdown::source_text(&self.doc))
    }

    /// Whether the source has been changed since it was shown.
    pub fn source_edited(&self) -> bool {
        self.md_source
            .as_ref()
            .is_some_and(|s| markdown::source_text(&self.doc) != s.shown)
    }

    /// What Markdown could not keep of the formatted document the source
    /// will turn back into, if it has been edited.
    pub fn losses_on_return(&self) -> Vec<&'static str> {
        match &self.md_source {
            Some(s) if self.source_edited() => markdown::losses(&s.original),
            _ => Vec::new(),
        }
    }

    /// The formatted document the source stands for: the one it was made
    /// from if the text is as it was, else the text read as Markdown.
    pub fn markdown_document(&self) -> Option<Document> {
        let s = self.md_source.as_ref()?;
        if !self.source_edited() {
            return Some(s.original.compact_for_save());
        }
        Some(self.read_source().0)
    }

    fn read_source(&self) -> (Document, quill_formats::report::ImportReport) {
        let text = markdown::source_text(&self.doc);
        let base = self.path.as_deref().and_then(std::path::Path::parent);
        let (mut doc, report) = markdown::read(&text, base);
        doc.keep_in_bounds();
        crate::image::prepare_document_previews(&mut doc);
        (doc, report)
    }

    /// Put `doc` in this window in place of its document, keeping the
    /// window's file and view settings, and return the document it had.
    fn replace_document(&mut self, doc: Document) -> Document {
        let mut fresh = DocView::new(doc, std::mem::take(&mut self.title));
        fresh.path = self.path.take();
        fresh.path_chosen = self.path_chosen;
        fresh.show_marks = self.show_marks;
        fresh.show_changes = self.show_changes;
        fresh.show_formatting = self.show_formatting;
        fresh.page_view = self.page_view;
        fresh.area = self.area;
        fresh.md_source = self.md_source.take();
        fresh.md_warned = std::mem::take(&mut self.md_warned);
        std::mem::replace(self, fresh).doc
    }

    /// View > Markdown > Source.
    pub fn show_markdown_source(&mut self) {
        if self.md_source.is_some() {
            return;
        }
        let text = markdown::write(&self.doc);
        let mut source = markdown::source_document(&text);
        let modified = self.doc.is_modified();
        if modified {
            source.touch();
        }
        let original = self.replace_document(source);
        self.md_source = Some(Box::new(MarkdownSource {
            original,
            shown: text,
        }));
        self.status =
            Some("Markdown source: edit the text, then choose View > Markdown > WYSIWYG".into());
    }

    /// View > Markdown > WYSIWYG.
    pub fn show_markdown_formatted(&mut self) {
        let Some(source) = self.md_source.take() else {
            return;
        };
        let edited = markdown::source_text(&self.doc) != source.shown;
        let doc = if edited {
            let (mut doc, _) = self.read_source();
            doc.touch();
            doc
        } else {
            source.original
        };
        self.replace_document(doc);
        self.md_source = None;
        self.status = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view_of(md: &str) -> DocView {
        let (doc, _) = markdown::read(md, None);
        DocView::new(doc, "Notes.md".into())
    }

    fn texts(view: &DocView) -> Vec<String> {
        view.doc.main.paras.iter().map(|p| p.text.clone()).collect()
    }

    #[test]
    fn source_shows_the_markdown_and_back_restores_the_document() {
        let mut view = view_of("# Head\n\nSome *words* here.\n");
        let before = texts(&view);
        view.path = Some("/tmp/Notes.md".into());
        view.show_markdown_source();
        assert!(view.in_markdown_source());
        assert_eq!(texts(&view), ["# Head", "", "Some *words* here.", ""]);
        assert_eq!(
            view.path.as_deref(),
            Some(std::path::Path::new("/tmp/Notes.md"))
        );
        assert_eq!(view.title, "Notes.md");
        assert!(!view.doc.is_modified());
        // Untouched text gives the same document back, with nothing lost.
        assert!(view.losses_on_return().is_empty());
        view.show_markdown_formatted();
        assert!(!view.in_markdown_source());
        assert_eq!(texts(&view), before);
        assert_eq!(view.doc.main.paras[0].text, "Head");
        assert!(!view.doc.is_modified());
    }

    #[test]
    fn editing_the_source_rebuilds_the_document() {
        let mut view = view_of("plain *text*\n");
        view.show_markdown_source();
        view.type_text("**Bold** ");
        assert!(view.source_edited());
        assert!(view.doc.is_modified());
        view.show_markdown_formatted();
        assert_eq!(view.doc.main.paras[0].text, "Bold plain text");
        assert!(view.doc.main.paras[0].chp_at(1).bold);
        assert!(view.doc.is_modified());
    }

    #[test]
    fn a_formatted_document_keeps_its_features_if_the_text_is_left_alone() {
        let mut doc = Document::new();
        let mut chp = doc.styles.get(0).chp;
        chp.color = quill_core::Rgb(0xFF0000);
        let pap = doc.styles.get(0).pap.clone();
        doc.main.paras = vec![quill_core::Paragraph::with_text("Red text", chp, pap)];
        doc.forget_history();
        let mut view = DocView::new(doc, "Red".into());
        view.show_markdown_source();
        assert_eq!(markdown::source_text(&view.doc), "Red text\n");
        // Left alone, the color survives the round trip.
        assert!(view.losses_on_return().is_empty());
        let back = view.markdown_document().unwrap();
        assert_eq!(
            back.main.paras[0].chp_at(0).color,
            quill_core::Rgb(0xFF0000)
        );
        // Edited, the document is read from the text and the color is
        // named as lost.
        view.type_text("x");
        assert!(view.losses_on_return().contains(&"Text colors"));
        view.show_markdown_formatted();
        assert_eq!(
            view.doc.main.paras[0].chp_at(0).color,
            quill_core::Rgb::BLACK
        );
    }

    #[test]
    fn the_view_menu_has_a_markdown_submenu_that_follows_the_mode() {
        use crate::app_menus::{Action, MenuContext, build};
        let mut view = view_of("x\n");
        let find = |view: &DocView| {
            let cx = MenuContext {
                view,
                font_families: &[],
                show_formatting_toolbar: true,
                show_paragraph_toolbar: true,
                show_ruler: true,
                full_screen: false,
                recent: &[],
                windows: &[],
            };
            let (menus, actions) = build(&cx);
            let view_menu = menus.into_iter().find(|m| m.title == "View").unwrap();
            let sub = view_menu
                .items
                .into_iter()
                .find(|i| i.label == "Markdown")
                .and_then(|i| i.submenu)
                .unwrap();
            sub.items
                .iter()
                .map(|i| (i.label.clone(), i.checked, actions[i.id as usize].clone()))
                .collect::<Vec<_>>()
        };
        use crate::commands::Cmd;
        assert_eq!(
            find(&view),
            [
                ("Source".into(), false, Action::Cmd(Cmd::MarkdownSource)),
                ("WYSIWYG".into(), true, Action::Cmd(Cmd::MarkdownWysiwyg)),
            ]
        );
        view.show_markdown_source();
        let now = find(&view);
        assert!(now[0].1 && !now[1].1);
    }
}
