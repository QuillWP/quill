//! View > Markdown: switching a window between Markdown source and the
//! formatted document, and the warnings that go with it.

use super::*;

/// The features of `lost` as lines of a warning.
pub(super) fn bullet_list(lost: &[&str]) -> String {
    lost.iter()
        .map(|l| format!("- {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

impl App {
    /// Show the window's document as Markdown source (`source`) or
    /// formatted, asking first when something would be left out.
    pub(super) fn markdown_view(&mut self, id: WindowId, source: bool) {
        let Some(i) = self.index(id) else {
            return;
        };
        let w = &mut self.windows[i].1;
        // A header or footnote pane edits a copy: put its text back first.
        w.st.close_pane();
        let view = w.st.main_view();
        if view.in_markdown_source() == source {
            return;
        }
        if source {
            let lost = quill_formats::markdown::losses(
                &view.doc,
                &crate::settings::with(|s| s.document.clone()),
            );
            if !lost.is_empty()
                && !ask(
                    "Show this document as Markdown?",
                    &format!(
                        "Markdown cannot carry everything in it:\n\n{}\n\nYou can read and edit the \
                         Markdown text. Switching back to WYSIWYG without changing the text gives \
                         the document back exactly; if you change it, the document is rebuilt \
                         from the text and these are left out.",
                        bullet_list(&lost)
                    ),
                    "Show Source",
                )
            {
                return;
            }
        } else {
            let lost = view.losses_on_return();
            if !lost.is_empty()
                && !ask(
                    "Rebuild the document from the Markdown?",
                    &format!(
                        "You changed the Markdown text, so the document is rebuilt from it. \
                         Markdown cannot carry these, so they will be lost:\n\n{}",
                        bullet_list(&lost)
                    ),
                    "Switch to WYSIWYG",
                )
            {
                return;
            }
        }
        let view = w.st.main_view_mut();
        if source {
            view.show_markdown_source();
        } else {
            view.show_markdown_formatted();
        }
        w.window.request_redraw();
        self.retitle();
    }
}
