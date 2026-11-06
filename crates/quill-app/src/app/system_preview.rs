//! Windows/Linux preview through the desktop's PDF viewer.
use super::*;

impl App {
    pub(super) fn preview_in_system_viewer(&mut self, i: usize) {
        self.ensure_fonts();
        let Some(catalog) = self.font_catalog.clone() else {
            return;
        };
        self.take_doc(i);
        let w = &mut self.windows[i].1;
        w.st.sync_pane();
        let doc = w.st.main_view().doc.compact_for_save();
        let title = w.st.main_view().title.clone();
        let window = self.windows[i].0;
        let proxy = self.proxy.clone();
        let spawn = std::thread::Builder::new()
            .name("quill-system-preview".into())
            .spawn(move || {
                let mut fonts = Fonts::new(catalog);
                let mut cache = GlyphCache::new();
                let mut view = DocView::new(doc, title).into_print();
                view.update_layout(&mut fonts);
                let pages = view.pages.len();
                let bytes = crate::pdf::write_with_progress(
                    &mut view,
                    &mut fonts,
                    &mut cache,
                    0..pages,
                    |_, _| {},
                );
                let result = crate::print_job::preview_pdf(&bytes);
                let _ = proxy.send_event(UserEvent::PrintFinished { window, result });
            });
        if let Err(e) = spawn {
            alert("Quill could not open system preview.", &e.to_string());
        }
    }
}
