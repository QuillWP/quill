//! Printing a document from its window.

use super::*;

impl App {
    /// Print a window's document, or save it as PDF.
    pub(super) fn print(&mut self, i: usize, job: crate::page_dialogs::PrintJob) {
        #[cfg(target_os = "macos")]
        if !job.pdf && self.native_print_window.is_some() {
            return;
        }
        self.ensure_fonts();
        let Some(catalog) = self.font_catalog.clone() else {
            return;
        };
        let window = self.windows[i].0;
        self.take_doc(i);
        let w = &mut self.windows[i].1;
        w.st.sync_pane();
        let (doc, title) = match w.st.print_doc.take() {
            Some(doc) => (doc, "Form Letters".to_string()),
            None => {
                let v = w.st.main_view();
                (v.doc.compact_for_save(), v.title.clone())
            }
        };
        #[cfg(windows)]
        if !job.pdf {
            let count = w.st.main_view().pages.len();
            let choice = match crate::win_print::Job::choose(&w.window, count) {
                Ok(Some(job)) => job,
                Ok(None) => return,
                Err(e) => {
                    alert("Quill could not open Print.", &e);
                    return;
                }
            };
            w.st.main_view_mut().status = Some("Preparing print job…".into());
            w.window.request_redraw();
            let proxy = self.proxy.clone();
            let spawn = std::thread::Builder::new()
                .name("quill-print".into())
                .spawn(move || {
                    let mut fonts = Fonts::new(catalog);
                    let mut cache = GlyphCache::new();
                    let mut view = DocView::new(doc, title).into_print();
                    view.update_layout(&mut fonts);
                    let progress = proxy.clone();
                    let result =
                        choice.print(&mut view, &mut fonts, &mut cache, move |done, total| {
                            let _ = progress.send_event(UserEvent::PrintProgress {
                                window,
                                done,
                                total,
                            });
                        });
                    let _ = proxy.send_event(UserEvent::PrintFinished { window, result });
                });
            if let Err(e) = spawn {
                w.st.main_view_mut().status = None;
                alert("Quill could not start printing.", &e.to_string());
            }
            return;
        }
        let title = strip_ext(&title).to_string();
        #[cfg(target_os = "macos")]
        let native_props = doc.props.clone();
        let target = if job.pdf {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Save as PDF")
                .set_file_name(format!("{title}.pdf"))
                .add_filter("PDF", &["pdf"]);
            if let Some(dir) = w.st.main_view().path.as_ref().and_then(|p| p.parent()) {
                dialog = dialog.set_directory(dir);
            }
            let Some(path) = dialog.save_file() else {
                return;
            };
            crate::print_job::Target::Save(path)
        } else {
            crate::print_job::Target::Print { copies: job.copies }
        };
        w.st.main_view_mut().status = Some("Preparing PDF\u{2026}".to_string());
        w.window.request_redraw();
        #[cfg(target_os = "macos")]
        if !job.pdf {
            self.native_print_window = Some(window);
        }
        let proxy = self.proxy.clone();
        let spawn = std::thread::Builder::new()
            .name("quill-pdf".to_string())
            .spawn(move || {
                let mut fonts = Fonts::new(catalog);
                let mut cache = GlyphCache::new();
                let mut view = DocView::new(doc, title).into_print();
                view.update_layout(&mut fonts);
                let n = view.pages.len();
                let range = match job.pages {
                    Some((from, to)) => (from - 1).min(n)..to.min(n),
                    None => 0..n,
                };
                let progress = proxy.clone();
                let bytes = crate::pdf::write_with_progress(
                    &mut view,
                    &mut fonts,
                    &mut cache,
                    range,
                    move |done, total| {
                        let _ = progress.send_event(UserEvent::PrintProgress {
                            window,
                            done,
                            total,
                        });
                    },
                );
                #[cfg(target_os = "macos")]
                if !job.pdf {
                    let _ = proxy.send_event(UserEvent::NativePrintReady {
                        window,
                        bytes,
                        title: view.title.clone(),
                        props: native_props,
                    });
                    return;
                }
                let result = crate::print_job::finish(target, &bytes);
                let _ = proxy.send_event(UserEvent::PrintFinished {
                    window,
                    result,
                    #[cfg(target_os = "macos")]
                    native: false,
                });
            });
        if let Err(e) = spawn {
            #[cfg(target_os = "macos")]
            if !job.pdf {
                self.native_print_window = None;
            }
            w.st.main_view_mut().status = None;
            alert("Quill could not start the PDF job.", &e.to_string());
        }
    }
}
