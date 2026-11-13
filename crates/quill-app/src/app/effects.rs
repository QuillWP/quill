//! Carrying out what a window asks the app to do.

use super::*;

impl App {
    pub(super) fn handle_effect(&mut self, el: &ActiveEventLoop, id: WindowId, effect: Effect) {
        if effect != Effect::None {
            crate::ev!("effect: {effect:?}");
        }
        self.effect(el, id, effect);
        // A dialog or command may have asked for printing, or for a new
        // window.
        if let Some(i) = self.index(id)
            && let Some(job) = self.windows[i].1.st.pending_print.take()
        {
            self.print(i, job);
        }
        if let Some(i) = self.index(id)
            && let Some((doc, title)) = self.windows[i].1.st.pending_window.take()
        {
            self.new_window(el, doc, Some(title));
        }
    }

    pub(super) fn effect(&mut self, el: &ActiveEventLoop, id: WindowId, effect: Effect) {
        match effect {
            Effect::None => {}
            Effect::About => about(),
            Effect::Licenses => licenses(),
            Effect::ReplaceFonts => self.replace_missing_fonts(id),
            Effect::OpenRecent(index) => {
                if let Some(path) = self.recent.get(index).cloned() {
                    self.open_files(el, &[path]);
                }
            }
            Effect::RemoveRecent(index) => {
                crate::recent::remove(&mut self.recent, index);
                self.sync_menus();
            }
            Effect::ClearRecent => {
                crate::recent::clear(&mut self.recent);
                self.sync_menus();
            }
            Effect::NewWindow => self.new_window(el, crate::settings::get().new_document(), None),
            Effect::Dialog(d) => {
                #[cfg(windows)]
                if d == DialogId::Print {
                    if let Some(i) = self.index(id) {
                        self.print(
                            i,
                            crate::page_dialogs::PrintJob {
                                copies: 1,
                                pages: None,
                                pdf: false,
                            },
                        );
                    }
                    return;
                }
                #[cfg(target_os = "macos")]
                if matches!(d, DialogId::Print | DialogId::PageSetup) {
                    if let Some(i) = self.index(id) {
                        if d == DialogId::Print {
                            self.print(
                                i,
                                crate::page_dialogs::PrintJob {
                                    copies: 1,
                                    pages: None,
                                    pdf: false,
                                },
                            );
                        } else {
                            self.native_page_setup(i);
                        }
                    }
                    return;
                }
                let list = self.window_list(id);
                if let Some(i) = self.index(id) {
                    let env = Env {
                        font_names: &self.font_names,
                        font_families: &self.font_families,
                        recent: &self.recent,
                        windows: &list,
                    };
                    if d == DialogId::PasteSpecial {
                        let forms = self.clipboard.get_or_insert_with(Clipboard::new).forms();
                        self.windows[i].1.st.open_paste_special(forms);
                    } else {
                        self.windows[i].1.st.open_dialog(d, &env);
                    }
                    #[cfg(target_os = "macos")]
                    if matches!(d, DialogId::Find | DialogId::Replace)
                        && let Some(native) = &self.windows[i].1.dialog
                    {
                        native.activate_find();
                    }
                    if d == DialogId::ShowClipboard {
                        let text = self
                            .clipboard
                            .get_or_insert_with(Clipboard::new)
                            .fragment()
                            .map(|f| f.plain_text())
                            .unwrap_or_default();
                        self.windows[i].1.st.set_clipboard_text(text);
                    }
                    self.windows[i].1.window.request_redraw();
                }
            }
            Effect::Open => self.open_dialog(el, Some(id)),
            Effect::FindAgain => {
                if let Some(i) = self.index(id) {
                    self.windows[i].1.st.find_again();
                    self.windows[i].1.window.request_redraw();
                }
            }
            Effect::Split => {
                if let Some(i) = self.index(id) {
                    self.windows[i].1.st.toggle_split();
                    self.windows[i].1.window.request_redraw();
                }
            }
            Effect::Mirror => self.mirror(el, id),
            Effect::MarkdownSource => self.markdown_view(id, true),
            Effect::MarkdownWysiwyg => self.markdown_view(id, false),
            Effect::FullScreen => {
                if let Some(i) = self.index(id) {
                    let w = &self.windows[i].1.window;
                    // On the Mac, Borderless is the system's own full screen,
                    // in a space of its own, as the green button gives.
                    w.set_fullscreen(match w.fullscreen() {
                        Some(_) => None,
                        None => Some(winit::window::Fullscreen::Borderless(None)),
                    });
                }
            }
            Effect::LookUp =>
            {
                #[cfg(target_os = "macos")]
                if let Some(i) = self.index(id) {
                    let w = &self.windows[i].1;
                    let z = w.st.document_zoom;
                    match w.st.view.look_up_spot() {
                        Some((word, font, size, x, y)) => crate::mac_lookup::show(
                            &w.window,
                            &word,
                            &font,
                            (size * z) as f64,
                            (x * z) as f64,
                            (y * z) as f64,
                        ),
                        None => {
                            let w = &mut self.windows[i].1;
                            w.st.view.status =
                                Some("Put the insertion point in a word to look it up.".into());
                            w.window.request_redraw();
                        }
                    }
                }
            }
            Effect::Zoom(step) => {
                if let Some(i) = self.index(id) {
                    self.windows[i].1.st.zoom_step(step);
                    self.windows[i].1.window.request_redraw();
                }
            }
            Effect::ChooseMergeData => {
                let dialog = crate::file_dialog::FileDialog::new()
                    .set_title("Open Data Document")
                    .add_filter("Documents", quill_formats::READABLE);
                let file = crate::events::timed("Open Data Document dialog", || dialog.pick_file());
                let list = self.window_list(id);
                if let (Some(path), Some(i)) = (file, self.index(id)) {
                    let env = Env {
                        font_names: &self.font_names,
                        font_families: &self.font_families,
                        recent: &self.recent,
                        windows: &list,
                    };
                    let st = &mut self.windows[i].1.st;
                    crate::merge_dialogs::attach_data(&mut st.view, &path);
                    st.open_dialog(DialogId::MergeHelper, &env);
                    self.windows[i].1.window.request_redraw();
                }
            }
            Effect::Pane(story) => {
                if let Some(i) = self.index(id) {
                    self.windows[i].1.st.open_pane(story);
                    self.windows[i].1.window.request_redraw();
                }
            }
            Effect::OpenNote(story, note) => {
                if let Some(i) = self.index(id) {
                    self.windows[i].1.st.open_note_in(story, note);
                    self.windows[i].1.window.request_redraw();
                }
            }
            Effect::ReplyToAnnotation | Effect::ResolveAnnotation => {
                if let Some(i) = self.index(id) {
                    let reply = matches!(effect, Effect::ReplyToAnnotation);
                    self.windows[i].1.st.annotation_command(reply);
                    self.windows[i].1.window.request_redraw();
                }
            }
            Effect::DeleteAnnotation => {
                if let Some(i) = self.index(id) {
                    self.windows[i].1.st.delete_annotation();
                    self.windows[i].1.window.request_redraw();
                }
            }
            Effect::InsertPicture => {
                let mut dialog = crate::file_dialog::FileDialog::new()
                    .set_title("Insert Picture")
                    .add_filter(
                        "Pictures",
                        &["png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "webp"],
                    );
                if let Some(dir) = self
                    .index(id)
                    .and_then(|i| self.windows[i].1.st.main_view().path.clone())
                    .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                {
                    dialog = dialog.set_directory(dir);
                }
                let picked = crate::events::timed("Insert Picture dialog", || dialog.pick_file());
                if let Some(path) = picked {
                    self.insert_picture_file(id, &path);
                }
            }
            Effect::NativePreview => {
                #[cfg(target_os = "macos")]
                self.effect(el, id, Effect::Dialog(DialogId::Print));
                #[cfg(not(target_os = "macos"))]
                if let Some(i) = self.index(id) {
                    self.preview_in_system_viewer(i);
                }
            }
            Effect::Print => {
                // The common dispatcher preserves native Windows/macOS printing
                // and the existing Linux print workflow.
                self.effect(el, id, Effect::Dialog(DialogId::Print));
            }
            Effect::SavePdf => {
                if let Some(i) = self.index(id) {
                    self.windows[i].1.st.pending_print = Some(crate::page_dialogs::PrintJob {
                        copies: 1,
                        pages: None,
                        pdf: true,
                    });
                }
            }
            Effect::Save => {
                self.save(id, false);
            }
            Effect::SaveAs => {
                self.save(id, true);
            }
            #[cfg(target_os = "macos")]
            Effect::Close if crate::mac_licenses::close_if_key() => {}
            Effect::Close => {
                // A Mac app stays open with no windows; elsewhere the last
                // window closing quits.
                let session = self.open_paths();
                if self.close_window(id) && self.windows.is_empty() && !cfg!(target_os = "macos") {
                    crate::settings::update(|s| {
                        s.last_session = session;
                        s.quit_cleanly = true;
                    });
                    el.exit();
                }
            }
            Effect::Quit => {
                let session = self.open_paths();
                let ids: Vec<WindowId> = self.windows.iter().map(|(w, _)| *w).collect();
                for w in ids {
                    if !self.close_window(w) {
                        return;
                    }
                }
                crate::settings::update(|s| {
                    s.last_session = session;
                    s.quit_cleanly = true;
                });
                el.exit();
            }
        }
    }
}
