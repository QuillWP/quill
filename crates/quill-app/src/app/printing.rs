//! Main-thread routing for macOS's titled print and page setup panels.

use super::*;

impl App {
    pub(super) fn native_page_setup(&mut self, i: usize) {
        if self.native_print_window.is_some() {
            return;
        }
        self.take_doc(i);
        self.windows[i].1.st.sync_pane();
        let w = &self.windows[i].1;
        let result = crate::mac_print::page_setup(
            &w.window,
            &w.st.main_view().doc.props,
            &w.print_settings,
            self.proxy.clone(),
        );
        match result {
            Ok(()) => self.native_print_window = Some(self.windows[i].0),
            Err(e) => alert("Quill could not open Page Setup.", &e),
        }
    }

    pub(super) fn take_doc_for_page_setup(
        &mut self,
        window: WindowId,
        result: Result<Option<(i32, i32)>, String>,
    ) {
        let size = match result {
            Ok(Some(size)) => size,
            Ok(None) => return,
            Err(e) => {
                alert("Quill could not apply Page Setup.", &e);
                return;
            }
        };
        if let Some(i) = self.index(window) {
            self.take_doc(i);
            let w = &mut self.windows[i].1;
            let view = w.st.main_view_mut();
            view.edit_document_props("Page Setup", |p| {
                p.page_width = size.0;
                p.page_height = size.1;
            });
            w.window.request_redraw();
        }
    }
}
