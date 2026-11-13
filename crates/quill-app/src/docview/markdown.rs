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

    /// Whether the window's file is Markdown, in either view.
    pub fn is_markdown_file(&self) -> bool {
        self.path
            .as_deref()
            .and_then(quill_formats::Format::from_path)
            == Some(quill_formats::Format::Markdown)
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
            Some(s) if self.source_edited() => {
                markdown::losses(&s.original, &crate::settings::with(|s| s.document.clone()))
            }
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

    /// What a crash recovery copy should hold. Outside Source view, the
    /// window's document. In Source view with the text untouched, the
    /// formatted document, since the text adds nothing to it. Edited, the
    /// text as it is and the formatted document it was made from, both as
    /// they are: reading the text as Markdown here would drop what it
    /// cannot carry (a reference definition, say) before the person chose
    /// to go back.
    pub fn recovery_documents(&self) -> (std::borrow::Cow<'_, Document>, Option<&Document>) {
        use std::borrow::Cow;
        match &self.md_source {
            None => (Cow::Borrowed(&self.doc), None),
            Some(s) if !self.source_edited() => (Cow::Owned(s.original.compact_for_save()), None),
            Some(s) => (Cow::Borrowed(&self.doc), Some(&s.original)),
        }
    }

    /// Put a recovered Source view back: the window holds the source text
    /// as it was left, and `original` is the formatted document it was
    /// made from.
    pub fn resume_markdown_source(&mut self, original: Document) {
        let shown = markdown::write(&original);
        self.md_source = Some(Box::new(MarkdownSource { original, shown }));
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

    /// The Markdown fonts in the Preferences changed: give this document
    /// the new ones, as Replace Fonts would. Source view and a Markdown
    /// file are changed; any other document has its own fonts. Code (in a
    /// fixed-width font) keeps its font. The change belongs to the view,
    /// not the document, so it is not an undo step or an edit.
    pub fn restyle_markdown_fonts(&mut self, old: &markdown::Fonts, new: &markdown::Fonts) {
        let source = self.in_markdown_source();
        if !source && !self.is_markdown_file() {
            return;
        }
        let unchanged = if source {
            (&old.source_font, old.source_size) == (&new.source_font, new.source_size)
        } else {
            (&old.display_font, old.display_size) == (&new.display_font, new.display_size)
        };
        if unchanged {
            return;
        }
        let was_modified = self.doc.is_modified();
        self.doc.begin("Markdown Fonts", self.sel);
        if source {
            // Source view is all one font, whatever it was left in.
            let to = self.doc.font_id(&new.source_font);
            self.doc.set_font_everywhere(to, new.source_size);
        } else {
            // Sizes follow the document's own body size, so the change is
            // right even if text came back from an earlier undo.
            let base = u32::from(self.doc.styles.get(0).chp.size);
            let to = self.doc.font_id(&new.display_font);
            self.doc
                .replace_font_scaled(markdown::is_mono, to, u32::from(new.display_size), base);
        }
        self.typing_chp = None;
        let sel = self.sel;
        self.finish_edit(sel);
        self.doc.forget_last_step(was_modified);
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
    fn new_markdown_fonts_restyle_an_open_document_in_place() {
        let old = markdown::Fonts::default();
        let new = markdown::Fonts {
            source_font: "Menlo".into(),
            source_size: 30,
            display_font: "Georgia".into(),
            display_size: 48,
        };
        let face = |v: &DocView, para: usize| {
            let chp = v.doc.main.paras[para].mark_chp;
            (v.doc.font_name(chp.font).to_string(), chp.size)
        };
        // Source view takes the source font and size.
        let mut view = view_of("# Head\n\nSome words.\n");
        view.path = Some("/tmp/Notes.md".into());
        view.show_markdown_source();
        let text = texts(&view);
        view.restyle_markdown_fonts(&old, &new);
        assert_eq!(face(&view, 0), ("Menlo".to_string(), 30));
        assert_eq!(texts(&view), text);
        assert!(!view.doc.is_modified());
        // A formatted Markdown file takes the display font and size.
        let mut view = view_of("# Head\n\nSome words.\n");
        view.path = Some("/tmp/Notes.md".into());
        view.restyle_markdown_fonts(&old, &new);
        let body = view.doc.main.paras[1].chp_of_char(0);
        assert_eq!((view.doc.font_name(body.font), body.size), ("Georgia", 48));
        // The change is one undo step, and it does not count as an edit.
        assert!(!view.doc.is_modified());
        assert_eq!(view.doc.undo_label(), None);
        // A change to the display font leaves Source view alone.
        let mut view = view_of("# Head\n");
        view.show_markdown_source();
        let before = face(&view, 0);
        view.restyle_markdown_fonts(
            &old,
            &markdown::Fonts {
                display_font: "Georgia".into(),
                display_size: 48,
                ..old.clone()
            },
        );
        assert_eq!(face(&view, 0), before);
        // Another kind of document keeps its own fonts.
        let mut view = view_of("Plain\n");
        let before = view.doc.main.paras[0].chp_of_char(0);
        view.restyle_markdown_fonts(&old, &new);
        assert_eq!(view.doc.main.paras[0].chp_of_char(0), before);
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
    fn recovery_saves_the_formatted_document_not_the_source_text() {
        let mut doc = Document::new();
        let mut chp = doc.styles.get(0).chp;
        chp.color = quill_core::Rgb(0xFF0000);
        let pap = doc.styles.get(0).pap.clone();
        doc.main.paras = vec![quill_core::Paragraph::with_text("Red text", chp, pap)];
        doc.forget_history();
        let mut view = DocView::new(doc, "Red".into());
        // Outside Source view the window's own document is what is saved.
        assert_eq!(view.recovery_documents().0.main.paras[0].text, "Red text");
        assert!(view.recovery_documents().1.is_none());
        view.show_markdown_source();
        // Untouched, the saved copy is the formatted original, color and all.
        let (saved, original) = view.recovery_documents();
        assert!(original.is_none());
        assert_eq!(saved.main.paras[0].text, "Red text");
        assert_eq!(
            saved.main.paras[0].chp_at(0).color,
            quill_core::Rgb(0xFF0000)
        );
        drop(saved);
        // Edited, it is the text as typed, beside the formatted original.
        // A reference definition shows nothing once read as Markdown, so
        // reading it here would lose it.
        view.type_text("[draft]: https://example.com/a\n");
        let (saved, original) = view.recovery_documents();
        let text = markdown::source_text(&saved);
        assert!(text.contains("[draft]: https://example.com/a"), "{text}");
        let original = original.unwrap().clone();
        assert_eq!(
            original.main.paras[0].chp_at(0).color,
            quill_core::Rgb(0xFF0000)
        );
        // Recovered, the window is back in Source view, edited.
        let mut back = DocView::new(saved.into_owned(), "Red - Recovered".into());
        back.resume_markdown_source(original);
        assert!(back.in_markdown_source());
        assert!(back.source_edited());
        assert_eq!(markdown::source_text(&back.doc), text);
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

    #[test]
    fn text_pasted_into_the_source_is_in_the_source_font() {
        use crate::commands::Cmd;
        let mut clip = crate::docview::Clipboard::new();
        // Copy a word in another font, at another size, in bold.
        let mut other = Document::new();
        let mut chp = other.styles.get(0).chp;
        chp.font = other.font_id("Georgia");
        chp.size = 48;
        chp.bold = true;
        let pap = other.styles.get(0).pap.clone();
        other.main.paras = vec![quill_core::Paragraph::with_text("Pasted", chp, pap)];
        let end = quill_core::Position::new(0, 6);
        let frag = other.copy(
            quill_core::StoryId::Main,
            quill_core::Position::new(0, 0),
            end,
        );
        clip.put(&other, frag);
        let mut view = view_of("Some text\n");
        view.show_markdown_source();
        let source = view.doc.styles.get(0).chp;
        view.exec(Cmd::Paste, false, &mut clip);
        assert!(markdown::source_text(&view.doc).starts_with("Pasted"));
        let p = &view.doc.main.paras[0];
        for at in 0..p.len() {
            let c = p.chp_of_char(at);
            assert_eq!((c.font, c.size, c.bold), (source.font, source.size, false));
        }
    }

    #[test]
    fn formatting_commands_do_nothing_in_the_source() {
        use crate::commands::Cmd;
        let mut clip = crate::docview::Clipboard::new();
        let mut view = view_of("Some text\n");
        view.show_markdown_source();
        view.exec(Cmd::SelectAll, false, &mut clip);
        let before = view.doc.main.paras.clone();
        for cmd in [
            Cmd::Bold,
            Cmd::LargerFontSize,
            Cmd::AlignCenter,
            Cmd::Nest,
            Cmd::PlainText,
        ] {
            assert_eq!(view.exec(cmd, false, &mut clip), Effect::None, "{cmd:?}");
        }
        // The dialogs that would format open no more than the commands do.
        for cmd in [Cmd::CharacterDialog, Cmd::ParagraphDialog, Cmd::StyleDialog] {
            assert_eq!(view.exec(cmd, false, &mut clip), Effect::None, "{cmd:?}");
        }
        assert_eq!(view.doc.main.paras, before);
        assert!(!view.doc.is_modified());
        // Plain editing still works.
        view.exec(Cmd::DocEnd, false, &mut clip);
        view.type_text("!");
        assert!(view.doc.is_modified());
        // Back in the formatted view, Bold works again.
        let mut view = view_of("Some text\n");
        view.exec(Cmd::SelectAll, false, &mut clip);
        view.exec(Cmd::Bold, false, &mut clip);
        assert!(view.doc.main.paras[0].chp_of_char(0).bold);
    }

    #[test]
    fn text_pasted_into_a_markdown_file_takes_the_format_where_it_goes() {
        use crate::commands::Cmd;
        let mut clip = crate::docview::Clipboard::new();
        let mut other = Document::new();
        let mut chp = other.styles.get(0).chp;
        chp.font = other.font_id("Georgia");
        chp.size = 48;
        chp.bold = true;
        let pap = other.styles.get(0).pap.clone();
        other.main.paras = vec![quill_core::Paragraph::with_text("Pasted", chp, pap)];
        let frag = other.copy(
            quill_core::StoryId::Main,
            quill_core::Position::new(0, 0),
            quill_core::Position::new(0, 6),
        );
        clip.put(&other, frag);
        let mut view = view_of("Some text\n");
        view.path = Some("notes.md".into());
        let here = view.doc.main.paras[0].chp_at(0);
        view.exec(Cmd::Paste, false, &mut clip);
        let p = &view.doc.main.paras[0];
        assert!(p.text.starts_with("Pasted"));
        for at in 0..p.len() {
            let c = p.chp_of_char(at);
            assert_eq!((c.font, c.size, c.bold), (here.font, here.size, here.bold));
        }
    }
}
