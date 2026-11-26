//! Opening, saving, backup creation, and close confirmation.

use super::*;
use std::path::Path;

/// After going on with missing fonts: where to deal with them later, until
/// the person says not to show this. It needs the Mac's checkbox.
fn font_reminder() {
    #[cfg(target_os = "macos")]
    if crate::settings::with(|s| s.missing_font_reminder)
        && crate::mac_alert::remind(
            "You can deal with missing fonts at any time.",
            "Edit > Missing Fonts > Highlight Missing Fonts marks the text they affect, and \
             Replace Missing Fonts in the same menu chooses installed fonts for them.",
            "Don't show this reminder again",
        )
    {
        crate::settings::update(|s| s.missing_font_reminder = false);
    }
}

/// A Markdown file names pictures outside its own folder. They stay out
/// unless the person says to load them, and Don't Load is the default
/// button so a stray Return loads nothing. Show Paths lists the files and
/// comes back to the question.
fn ask_to_load_outside_pictures(name: &str, pictures: &[PathBuf]) -> bool {
    const SHOWN: usize = 15;
    let count = pictures.len();
    let what = if count == 1 {
        "1 picture outside its folder".to_string()
    } else {
        format!("{count} pictures outside its folder")
    };
    loop {
        let answer = message_box(
            rfd::MessageLevel::Warning,
            "Quill",
            format!(
                "\u{201c}{name}\u{201d} links to {what}.\n\nLoading a picture copies it \
                 into the document, and it is included when you save or share the document. \
                 Load them only if you trust this file."
            ),
            rfd::MessageButtons::YesNoCancelCustom(
                "Don't Load".into(),
                "Show Paths".into(),
                "Load Pictures".into(),
            ),
        );
        match answer {
            rfd::MessageDialogResult::Custom(label) if label == "Load Pictures" => return true,
            rfd::MessageDialogResult::Custom(label) if label == "Show Paths" => {
                let mut list = pictures
                    .iter()
                    .take(SHOWN)
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                if count > SHOWN {
                    list.push_str(&format!("\n{} more.", count - SHOWN));
                }
                message_box(
                    rfd::MessageLevel::Info,
                    "Quill",
                    format!("Pictures outside the folder:\n\n{list}"),
                    rfd::MessageButtons::Ok,
                );
            }
            _ => return false,
        }
    }
}

/// Whether two paths name one file, however each was written.
fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || match (a.canonicalize(), b.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
}

impl App {
    /// Open documents in new windows. An untouched Untitled window is reused,
    /// and a document already open comes to the front instead of opening
    /// again: two windows on one file would overwrite each other's saves.
    pub(super) fn open_files(&mut self, el: &ActiveEventLoop, paths: &[PathBuf]) {
        crate::ev!("open files: {paths:?}");
        for path in paths {
            let open = self.windows.iter().find(|(_, w)| {
                let here = w.st.main_view().path.as_deref();
                here.is_some_and(|here| same_file(here, path))
            });
            if let Some((_, w)) = open {
                w.window.focus_window();
                crate::recent::note(&mut self.recent, path);
                continue;
            }
            let rtf = crate::settings::with(|s| s.interpret_rtf);
            crate::ev!("reading {path:?}");
            let (mut doc, mut report) = match quill_formats::load_reported(path, rtf) {
                Ok(d) => d,
                // A file named as a document that holds plain text can
                // still be read, as text.
                Err(e)
                    if quill_formats::holds_text_not_its_format(path)
                        && crate::app::ask(
                            &format!("Open \u{201c}{}\u{201d} as plain text?", file_name(path)),
                            &e,
                            "Open as Text",
                        ) =>
                {
                    match quill_formats::load_as_text(path) {
                        Ok(doc) => {
                            let mut report = quill_formats::report::ImportReport::default();
                            report.no_page = true;
                            (doc, report)
                        }
                        Err(e) => {
                            alert(
                                &format!(
                                    "Quill could not open \u{201c}{}\u{201d}.",
                                    file_name(path)
                                ),
                                &e,
                            );
                            continue;
                        }
                    }
                }
                Err(e) => {
                    alert(
                        &format!("Quill could not open \u{201c}{}\u{201d}.", file_name(path)),
                        &e,
                    );
                    continue;
                }
            };
            crate::ev!("read with notes: {:?}", report.notes());
            // In the App Sandbox the pictures beside a Markdown file need the
            // person's leave for their folder, asked for once.
            if report
                .notes()
                .contains(&quill_formats::report::MARKDOWN_PICTURES)
                && Format::from_path(path) == Some(Format::Markdown)
                && crate::folder_access::ask_for_folder_of(path, "show the pictures of")
                && let Ok(again) = quill_formats::load_reported(path, rtf)
            {
                (doc, report) = again;
            }
            if !report.outside_pictures.is_empty()
                && ask_to_load_outside_pictures(&file_name(path), &report.outside_pictures)
                && let Ok(trusted) = quill_formats::load_reported_trusting(path, rtf, true)
            {
                (doc, report) = trusted;
            }
            if report.no_page {
                crate::settings::with(|s| doc.props.take_paper(&s.document));
            }
            crate::image::prepare_document_previews(&mut doc);
            self.ensure_fonts();
            crate::font_names::use_family_names(&mut doc);
            let missing = self
                .fonts
                .as_ref()
                .map(|fonts| missing_fonts(&doc, fonts))
                .unwrap_or_default();
            let title = file_name(path);
            #[cfg(debug_assertions)]
            if let Some(log) = std::env::var_os("QUILL_OPEN_LOG") {
                use std::io::Write as _;
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(log)
                {
                    let _ = writeln!(f, "{}", path.display());
                }
            }
            let reuse = self.windows.iter().position(|(_, w)| {
                let v = w.st.main_view();
                w.group == 0
                    && v.path.is_none()
                    && !v.doc.is_modified()
                    && v.doc.main.paras.len() == 1
                    && v.doc.main.paras[0].text.is_empty()
            });
            let i = match reuse {
                Some(i) => {
                    let w = &mut self.windows[i].1;
                    w.st.close_pane();
                    w.st.view = DocView::new(doc, title.clone());
                    w.st.view.open_in_its_view();
                    w.window.set_title(&title);
                    w.window.request_redraw();
                    i
                }
                None => {
                    self.new_window(el, doc, Some(title));
                    self.windows.len() - 1
                }
            };
            // Laying the document out now finds tables too wide for their
            // columns; the window's first drawing reuses the layout.
            let mut wide = Vec::new();
            if let Some((_, w)) = self.windows.get_mut(i) {
                let v = w.st.main_view_mut();
                v.path = Some(path.clone());
                v.disk_stamp = disk_stamp(path);
                if let Some(fonts) = self.fonts.as_mut() {
                    v.update_layout(fonts);
                    wide = v.wide_table_pages();
                }
            }
            crate::recent::note(&mut self.recent, path);
            let accepted = crate::settings::with(|s| {
                let this = s.accepted_fonts.iter().find(|(p, _)| same_file(p, path));
                this.map_or(Vec::new(), |(_, fonts)| fonts.clone())
            });
            let missing: Vec<_> = missing
                .into_iter()
                .filter(|(font, _)| !accepted.iter().any(|a| a.eq_ignore_ascii_case(font)))
                .collect();
            if let Some(detail) = open_warning(&report, &missing, &wide) {
                let message = if report.is_empty() && missing.is_empty() {
                    format!("\u{201c}{}\u{201d} may not look right.", file_name(path))
                } else {
                    format!(
                        "Quill opened \u{201c}{}\u{201d} with some changes.",
                        file_name(path)
                    )
                };
                if missing.is_empty() {
                    warn(&message, &detail);
                } else if let Some(window) = self.windows.get(i).map(|(id, _)| *id) {
                    self.ask_about_fonts(window, path, &message, &detail, &missing);
                }
            }
        }
    }

    /// The warning for a document with missing fonts, which offers to keep
    /// them, so it does not come up each time, or to replace them.
    fn ask_about_fonts(
        &mut self,
        window: WindowId,
        path: &Path,
        message: &str,
        detail: &str,
        missing: &[(String, String)],
    ) {
        let answer =
            crate::replace_fonts::ask_missing_fonts(format!("{message}\n\n{detail}"), "OK");
        match answer {
            crate::replace_fonts::FontChoice::Keep => {
                crate::settings::update(|s| {
                    let fonts = missing.iter().map(|(font, _)| font.clone());
                    match s
                        .accepted_fonts
                        .iter_mut()
                        .find(|(p, _)| same_file(p, path))
                    {
                        Some((_, kept)) => kept.extend(fonts),
                        None => {
                            s.accepted_fonts.push((path.to_path_buf(), fonts.collect()));
                            let over = s
                                .accepted_fonts
                                .len()
                                .saturating_sub(crate::settings::MAX_ACCEPTED_DOCUMENTS);
                            s.accepted_fonts.drain(..over);
                        }
                    }
                });
            }
            crate::replace_fonts::FontChoice::Replace => {
                self.replace_missing_fonts(window);
            }
            crate::replace_fonts::FontChoice::Other => font_reminder(),
        }
    }

    /// Edit > Missing Fonts > Replace Missing Fonts…
    pub(super) fn replace_missing_fonts(&mut self, id: WindowId) {
        self.ensure_fonts();
        let (Some(i), Some(fonts)) = (self.index(id), self.fonts.as_ref()) else {
            return;
        };
        let w = &mut self.windows[i].1;
        let mut missing = missing_fonts(&w.st.main_view().doc, fonts);
        // Fonts a paste just warned about are listed even when the layout's
        // catalog finds a stand-in for them as present.
        let view = w.st.main_view_mut();
        let warned = std::mem::take(&mut view.pasted_missing);
        let stand_in = fonts.family_info().first().map(|f| f.name.clone());
        for name in warned {
            let used = (0..view.doc.fonts.len())
                .filter_map(|id| u16::try_from(id).ok())
                .any(|id| view.doc.font_name(id).eq_ignore_ascii_case(&name));
            if used
                && !missing.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name))
                && let Some(shown) = fonts
                    .substitute(&name)
                    .map(str::to_string)
                    .or(stand_in.clone())
            {
                missing.push((name, shown));
            }
        }
        if missing.is_empty() {
            w.st.main_view_mut().status = Some("No fonts are missing.".into());
            w.window.request_redraw();
            return;
        }
        w.st.focus_main();
        let (d, st) = crate::replace_fonts::dialog(&missing, &fonts.family_info());
        w.st.dialogs.push((Which::ReplaceFonts(st), d));
        w.window.request_redraw();
    }

    pub(super) fn open_dialog(&mut self, el: &ActiveEventLoop, id: Option<WindowId>) {
        let mut dialog = crate::file_dialog::FileDialog::new()
            .set_title("Open")
            .add_filter("Readable Files", quill_formats::READABLE)
            .add_filter("All Files", &["*"]);
        if let Some(dir) = id
            .and_then(|id| self.index(id))
            .and_then(|i| self.windows[i].1.st.main_view().path.as_ref())
            .and_then(|p| p.parent())
        {
            dialog = dialog.set_directory(dir);
        }
        // Modal inside winit's event handler: needs the patched winit in
        // vendor/winit (see vendor/README.md).
        crate::ev!("Open: about to show the file dialog");
        let picked = crate::events::timed("Open file dialog", || dialog.pick_files());
        crate::ev!("Open: the dialog returned {picked:?}");
        if let Some(paths) = picked {
            self.open_files(el, &paths);
        }
    }

    /// Save a window, asking for a destination when needed.
    pub(super) fn save(&mut self, id: WindowId, ask: bool) -> bool {
        let Some(i) = self.index(id) else {
            return false;
        };
        let w = &mut self.windows[i].1;
        w.st.sync_pane();
        let known =
            w.st.main_view()
                .path
                .clone()
                .filter(|path| Format::from_path(path).is_some());
        let (direct, initial) = save_policy(
            known.as_ref().and_then(|p| Format::from_path(p)),
            ask,
            w.st.main_view().path_chosen,
            crate::settings::with(|s| s.foreign_as_quill),
        );
        let mut chosen = false;
        let own_path = w.st.main_view().path.clone();
        let path = match known {
            Some(path) if direct => path,
            _ => {
                // Save As does not offer Markdown (see Format::SAVE_AS): a
                // Markdown file's panel starts on Quill's own format.
                let initial = if Format::SAVE_AS.contains(&initial) {
                    initial
                } else {
                    Format::Quill
                };
                let filename = format!(
                    "{}.{}",
                    strip_ext(&w.st.main_view().title),
                    initial.extensions()[0]
                );
                let directory =
                    w.st.main_view()
                        .path
                        .as_ref()
                        .and_then(|path| path.parent());
                #[cfg(target_os = "macos")]
                let selected = {
                    crate::ev!("Save: about to show the save panel");
                    crate::events::timed("Save panel", || {
                        crate::mac_save::save_file(&w.window, &filename, directory)
                    })
                };
                #[cfg(not(target_os = "macos"))]
                let selected = {
                    let mut dialog = crate::file_dialog::FileDialog::new()
                        .set_title("Save As")
                        .set_file_name(&filename);
                    for format in std::iter::once(initial)
                        .chain(Format::SAVE_AS.into_iter().filter(|f| *f != initial))
                    {
                        dialog = dialog.add_filter(format.name(), format.extensions());
                    }
                    if let Some(dir) = directory {
                        dialog = dialog.set_directory(dir);
                    }
                    {
                        crate::ev!("Save: about to show the file dialog");
                        crate::events::timed("Save file dialog", || dialog.save_file())
                    }
                };
                chosen = true;
                match selected {
                    Some(path) if savable_by_save_as(&path, own_path.as_deref()) => path,
                    Some(path) => {
                        // Quill does not write this kind of file, so the
                        // name changes. Say so, and say when that name is
                        // taken, since the save panel asked about the other.
                        // The Mac's panel allows only the format's own
                        // extensions, so this is for the other systems'
                        // dialogs; the App Sandbox would refuse the new name.
                        let renamed = path.with_extension("qwp");
                        let replaces = if renamed.exists() {
                            " That file already exists and will be replaced."
                        } else {
                            ""
                        };
                        if !super::ask(
                            &format!(
                                "Quill cannot save \u{201c}{}\u{201d} in that format.",
                                file_name(&path)
                            ),
                            &format!(
                                "It will be saved in Quill's own format as \u{201c}{}\u{201d}.{replaces}",
                                file_name(&renamed)
                            ),
                            "Save",
                        ) {
                            return false;
                        }
                        renamed
                    }
                    None => return false,
                }
            }
        };
        let format = Format::from_path(&path).unwrap_or(Format::Quill);
        let confirmed = match confirm_save(w.st.main_view(), &path, format, chosen, &mut Dialogs) {
            Ok(confirmed) => confirmed,
            Err(SaveStop::Cancelled) => return false,
            Err(SaveStop::Failed(error)) => {
                alert(
                    &format!("Quill could not save \u{201c}{}\u{201d}.", file_name(&path)),
                    &error,
                );
                return false;
            }
        };
        if crate::settings::with(|settings| settings.make_backup)
            && path.exists()
            && let Err(error) = make_backup(&path)
        {
            alert(
                &format!(
                    "Quill could not make a backup of \u{201c}{}\u{201d}.",
                    file_name(&path)
                ),
                &format!(
                    "{error}\n\nThe document was not saved, so its previous saved version is unchanged."
                ),
            );
            return false;
        }
        // The next time it opens, it opens in this view.
        let view = w.st.main_view_mut();
        view.doc.props.view = Some(view.current_view());
        // In Markdown source view the text is the file for Markdown, and
        // every other format gets the document the text stands for.
        crate::ev!("writing {path:?} as {format:?}");
        let write =
            |view: &DocView| write_document(view, confirmed.prepared.as_ref(), &path, format);
        let mut saved = write(w.st.main_view());
        // In the App Sandbox a Markdown file's pictures folder needs the
        // person's leave, asked for once. Only a refusal at that folder asks:
        // a failure of the file itself is shown as it is.
        if let Err(failure) = &saved
            && failure.picture_folder_denied
            && crate::folder_access::ask_for_folder_of(&path, "save the pictures of")
        {
            saved = write(w.st.main_view());
        }
        let saved = saved.map_err(|failure| failure.message);
        crate::ev!("written: {:?}", saved.as_ref().map(|_| ()));
        if let Err(error) = saved {
            let check = (format == Format::Quill && w.st.main_view().markdown_text().is_none())
                .then(|| quill_formats::native_check(&w.st.main_view().doc))
                .flatten();
            let name = file_name(&path);
            // Anything else: the file on disk stays as it was.
            let Some((loss, back)) = check else {
                alert(
                    &format!("Quill could not save \u{201c}{name}\u{201d}."),
                    &error,
                );
                return false;
            };
            // A .qwp that doesn't read back exactly: show the paragraph at
            // fault, and offer to save the document as it reads back.
            let view = w.st.main_view_mut();
            if let Some(para) = loss.para {
                select_paragraph(view, para);
                w.window.request_redraw();
            }
            let keeps_undo = view.doc.can_match_saved(&back);
            let backup = quill_formats::backup_path(&path);
            let detail = format!(
                "{error}{}\n\nSave Anyway saves the document as Quill reads it back. The file \
                 will open, and this window then shows what it holds, so you can check it and \
                 fix what changed by hand. {}{}",
                if loss.para.is_some() {
                    "\n\nQuill has selected that paragraph."
                } else {
                    ""
                },
                if keeps_undo {
                    "Undo takes the change back."
                } else {
                    "This clears Undo, because the fonts or pictures changed."
                },
                match &backup {
                    Some(backup) if path.exists() => format!(
                        " The file as it was is kept as \u{201c}{}\u{201d}.",
                        file_name(backup)
                    ),
                    _ => String::new(),
                },
            );
            if !super::ask(
                &format!("Quill could not save \u{201c}{name}\u{201d} exactly."),
                &detail,
                "Save Anyway",
            ) {
                return false;
            }
            // Keep the old file even when backups are off, since this save
            // knowingly changes something.
            let backed_up = crate::settings::with(|settings| settings.make_backup);
            if !backed_up
                && path.exists()
                && let Err(error) = make_backup(&path)
            {
                alert(
                    &format!("Quill could not make a backup of \u{201c}{name}\u{201d}."),
                    &format!("{error}\n\nThe document was not saved, so the file is unchanged."),
                );
                return false;
            }
            let w = &mut self.windows[i].1;
            let view = w.st.main_view_mut();
            let back = match quill_formats::save_anyway(&view.doc, &path) {
                Ok(back) => back,
                Err(error) => {
                    alert(
                        &format!("Quill could not save \u{201c}{name}\u{201d}."),
                        &error,
                    );
                    return false;
                }
            };
            if !view.doc.match_saved(&back, "Save Anyway", view.sel) {
                // Undo cannot take this back, so the window reopens the
                // file as it is now.
                w.st.close_pane();
                w.st.view = DocView::new(back, name.clone());
                w.st.view.open_in_its_view();
            }
            let view = w.st.main_view_mut();
            view.invalidate_layout();
            if let Some(para) = loss.para {
                select_paragraph(view, para.min(view.doc.main.paras.len() - 1));
            } else {
                view.clamp_selection();
            }
        }
        let w = &mut self.windows[i].1;
        record_saved_file(w.st.main_view_mut(), &path, chosen);
        confirmed.remember(w.st.main_view_mut());
        crate::recovery::remove(w.recovery.take());
        crate::recovery::discard(w.recovery_opening.take());
        w.window.request_redraw();
        self.retitle();
        crate::recent::note(&mut self.recent, &path);
        true
    }

    /// Ask whether to save changes before a window closes.
    pub(super) fn confirm_close(&mut self, id: WindowId) -> bool {
        let Some(i) = self.index(id) else {
            return true;
        };
        let w = &self.windows[i].1;
        if !w.st.main_view().doc.is_modified() {
            return true;
        }
        w.window.focus_window();
        let answer = super::message_box(
            rfd::MessageLevel::Warning,
            "Quill",
            format!(
                "Save changes to \u{201c}{}\u{201d} before closing?",
                w.st.main_view().title
            ),
            rfd::MessageButtons::YesNoCancelCustom(
                "Save".into(),
                "Don\u{2019}t Save".into(),
                "Cancel".into(),
            ),
        );
        match answer {
            rfd::MessageDialogResult::Yes => self.save_before_close(id),
            rfd::MessageDialogResult::Custom(choice) if choice == "Save" => {
                self.save_before_close(id)
            }
            rfd::MessageDialogResult::No => true,
            rfd::MessageDialogResult::Custom(choice) if choice.starts_with("Don") => true,
            _ => false,
        }
    }

    fn save_before_close(&mut self, id: WindowId) -> bool {
        self.save(id, false)
            && self
                .index(id)
                .is_some_and(|i| !self.windows[i].1.st.main_view().doc.is_modified())
    }
}

/// Warn before adopting a format that cannot keep the whole document.
/// The questions a save asks before it writes anything. The app shows
/// dialogs; tests answer from a script. Each returns true to go on.
trait SavePrompts {
    fn docx_losses(&mut self, losses: &[&'static str]) -> bool;
    fn markdown_losses(&mut self, lost: &[&'static str]) -> bool;
    fn format_warning(&mut self, warning: String) -> bool;
    fn replace_changed_file(&mut self, name: &str) -> bool;
}

/// The message boxes the app shows.
struct Dialogs;

impl SavePrompts for Dialogs {
    fn docx_losses(&mut self, losses: &[&'static str]) -> bool {
        super::message_box(
            rfd::MessageLevel::Warning,
            "Some Settings May Be Lost",
            format!(
                "Microsoft Word may remove these Quill-only settings when it resaves this file:\n\n{}\n\nSave a .qwp copy to preserve them. Save as a Word document anyway?",
                losses.join("\n")
            ),
            rfd::MessageButtons::YesNo,
        ) == rfd::MessageDialogResult::Yes
    }

    fn markdown_losses(&mut self, lost: &[&'static str]) -> bool {
        super::message_box(
            rfd::MessageLevel::Warning,
            "Some Features Will Be Left Out",
            format!(
                "Markdown cannot carry these, so the saved file will leave them out:\n\n{}\n\nThe document you're editing keeps them, but the next time you open the file they will be gone. Save a .qwp copy to keep them. Save as Markdown anyway?",
                super::markdown::bullet_list(lost)
            ),
            rfd::MessageButtons::YesNo,
        ) == rfd::MessageDialogResult::Yes
    }

    fn format_warning(&mut self, warning: String) -> bool {
        super::message_box(
            rfd::MessageLevel::Warning,
            "Some Features Will Be Left Out",
            warning,
            rfd::MessageButtons::YesNo,
        ) == rfd::MessageDialogResult::Yes
    }

    fn replace_changed_file(&mut self, name: &str) -> bool {
        super::ask(
            &format!(
                "\u{201c}{name}\u{201d} has been changed by something else since you opened or saved it."
            ),
            "Saving now replaces those changes with this window's version. Turn on backups in Preferences to keep a copy of the file you replace.",
            "Replace File",
        )
    }
}

/// Why a save stopped before writing.
enum SaveStop {
    /// The person said no to one of the questions.
    Cancelled,
    /// The document could not be prepared; the reason.
    Failed(String),
}

/// What a save settled before writing.
struct Confirmed {
    /// The formatted document to write, in Markdown Source view.
    prepared: Option<quill_core::Document>,
    /// What Markdown cannot carry that the person has now been told of. Kept
    /// here until the file is written: a save that is cancelled later, or
    /// fails, has told the person nothing that should stop the next warning.
    md_warned: Option<Vec<&'static str>>,
}

impl Confirmed {
    /// The file was written: remember what the person was told.
    fn remember(&self, view: &mut DocView) {
        if let Some(lost) = &self.md_warned {
            view.md_warned = lost.clone();
        }
    }
}

/// Prepare the document a save writes and ask every question it needs
/// answered first: Word settings that may be lost, Markdown's losses, what
/// the format leaves out, and a file changed by something else.
fn confirm_save(
    view: &DocView,
    path: &Path,
    format: Format,
    chosen: bool,
    prompts: &mut dyn SavePrompts,
) -> Result<Confirmed, SaveStop> {
    // In Markdown Source view the window holds only the text; the other
    // formats are written from the formatted document it stands for, so
    // the warnings below and the write all use that one document.
    let prepared = export_document(view, format).map_err(SaveStop::Failed)?;
    let mut md_warned = None;
    if chosen && format == Format::Docx {
        let losses = docx_word_rewrite_losses(prepared.as_ref().unwrap_or(&view.doc));
        if !losses.is_empty() && !prompts.docx_losses(&losses) {
            return Err(SaveStop::Cancelled);
        }
    }
    if format == Format::Markdown && !view.in_markdown_source() {
        let lost = quill_formats::markdown::losses(
            &view.doc,
            &crate::settings::with(|s| s.document.clone()),
        );
        let unwarned = lost.iter().any(|l| !view.md_warned.contains(l));
        if ((chosen && !lost.is_empty()) || unwarned) && !prompts.markdown_losses(&lost) {
            return Err(SaveStop::Cancelled);
        }
        md_warned = Some(lost);
    }
    if let Some(warning) =
        save_format_warning(prepared.as_ref().unwrap_or(&view.doc), format, chosen)
        && !prompts.format_warning(warning)
    {
        return Err(SaveStop::Cancelled);
    }
    if changed_on_disk(view, path) && !prompts.replace_changed_file(&file_name(path)) {
        return Err(SaveStop::Cancelled);
    }
    Ok(Confirmed {
        prepared,
        md_warned,
    })
}

/// Write the document a save settled on. In Markdown Source view the text is
/// the file for Markdown, and every other format is written from the
/// formatted document the text stands for (`prepared`).
fn write_document(
    view: &DocView,
    prepared: Option<&quill_core::Document>,
    path: &Path,
    format: Format,
) -> Result<(), quill_formats::SaveFailure> {
    match (view.markdown_text(), format) {
        (Some(text), Format::Markdown) => Ok(quill_formats::save_markdown_text(&text, path)?),
        (Some(_), _) => match prepared {
            Some(doc) => quill_formats::save_checked(doc, path, format),
            None => Err("The Markdown could not be read back.".to_string().into()),
        },
        (None, _) => quill_formats::save_checked(&view.doc, path, format),
    }
}

/// Whether Save As may write `path`: a format Quill writes, except Markdown
/// (on hold, see Format::SAVE_AS) unless it is the document's own file.
fn savable_by_save_as(path: &Path, own: Option<&Path>) -> bool {
    match Format::from_path(path) {
        None => false,
        Some(Format::Markdown) => own == Some(path),
        Some(_) => true,
    }
}

/// The formatted document a save writes when the window is in Markdown
/// Source view, where the window's own document is only the text: the one
/// the text stands for, prepared once so every warning and the writer see
/// the same thing. None outside Source view. When the text cannot be read
/// back, a save to another format fails with the reason, before anything is
/// replaced; a save to Markdown writes the text as it is and needs no such
/// document.
fn export_document(view: &DocView, format: Format) -> Result<Option<quill_core::Document>, String> {
    match view.markdown_document() {
        None => Ok(None),
        Some(Ok(doc)) => Ok(Some(doc)),
        Some(Err(_)) if format == Format::Markdown => Ok(None),
        Some(Err(error)) => Err(error),
    }
}

fn save_format_warning(doc: &quill_core::Document, format: Format, chosen: bool) -> Option<String> {
    // Only Quill's own format keeps a paragraph style's link to a character
    // style; the others keep the formatting it gives.
    let normal = doc.styles.normal_character();
    let linked = chosen
        && format != Format::Quill
        && doc
            .styles
            .styles
            .iter()
            .any(|s| s.character != 0 && s.character != normal);
    let link = if linked {
        " Paragraph styles linked to a character style keep its formatting, but not the link."
    } else {
        ""
    };
    let (limits, keeps_changes) = match format {
        Format::Text => (
            "Plain text does not keep formatting, pictures, tables or other document features.",
            false,
        ),
        Format::Odt | Format::Html => (
            "This format keeps basic text, formatting, tables and pictures, but leaves out advanced layout such as headers, footers, section columns and positioned frames.",
            false,
        ),
        _ if linked => ("", true),
        _ => return None,
    };
    if !linked && !chosen && !doc.has_changes() {
        return None;
    }
    let revisions = if doc.has_changes() && !keeps_changes {
        " Tracked changes will be accepted in the saved file."
    } else {
        ""
    };
    let limits = format!("{limits}{revisions}{link}");
    let limits = limits.trim_start();
    Some(format!(
        "{limits}\n\nThe saved file will become the document you're editing. Features left out of the file remain on screen until you close it, but will be gone when you reopen it. Save a .qwp copy first to keep them.\n\nSave as {} anyway?",
        format.name()
    ))
}

/// Every successful Save As adopts the destination and its saved point.
/// Select paragraph `para` of the main text and scroll to it.
fn select_paragraph(view: &mut DocView, para: usize) {
    let end = view.doc.main.paras[para].len();
    view.set_selection(Selection::new(
        quill_core::Position::new(para, 0),
        quill_core::Position::new(para, end),
    ));
    view.scroll_to_caret();
}

/// Copy `document` to its backup beside it (Report.qwp-backup) before a
/// save replaces it. A document that is itself a backup gets none.
fn make_backup(document: &Path) -> Result<(), String> {
    let Some(backup) = quill_formats::backup_path(document) else {
        return Ok(());
    };
    // The App Sandbox allows the backup only as a related file of the
    // document, written while the system coordinates the two.
    #[cfg(target_os = "macos")]
    return crate::mac_save::copy_related(document, &backup);
    #[cfg(not(target_os = "macos"))]
    quill_formats::copy_atomic(document, &backup)
}

fn record_saved_file(view: &mut DocView, path: &Path, chosen: bool) {
    view.doc.mark_saved();
    view.title = file_name(path);
    view.path = Some(path.to_path_buf());
    view.path_chosen |= chosen;
    view.disk_stamp = disk_stamp(path);
}

/// A file's modified time and size, or None if it can't be read.
fn disk_stamp(path: &Path) -> Option<(std::time::SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// True when `path` is the document's own file and it has been changed or
/// replaced since Quill opened or last saved it.
fn changed_on_disk(view: &DocView, path: &Path) -> bool {
    match (&view.path, view.disk_stamp, disk_stamp(path)) {
        (Some(own), Some(then), Some(now)) => own == path && then != now,
        _ => false,
    }
}

fn save_policy(
    format: Option<Format>,
    ask: bool,
    chosen: bool,
    native_default: bool,
) -> (bool, Format) {
    let foreign = format.is_some_and(|f| f != Format::Quill);
    let direct = format.is_some() && !ask && (!foreign || chosen || !native_default);
    let initial = if foreign && native_default && !chosen {
        Format::Quill
    } else {
        format.unwrap_or(Format::Quill)
    };
    (direct, initial)
}

/// Settings Quill can keep in DOCX only as private ignorable XML. Current
/// Microsoft Word removes that XML when it rewrites the document.
fn docx_word_rewrite_losses(doc: &quill_core::Document) -> Vec<&'static str> {
    use quill_core::props::{FrameRel, FrameV};
    use quill_core::{Align, StoryId};

    let mut settings = Vec::new();
    let sections = doc.sections().len();
    if (0..sections).any(|section| doc.nth_section(section).margin_page_numbers) {
        settings.push("• Margin page numbers");
    }
    // Every stored story: `doc.story` would show only the headers of the
    // section the header pane is on.
    let stories = [StoryId::Main, StoryId::Notes, StoryId::Annotations]
        .into_iter()
        .map(|story| doc.story(story))
        .chain(doc.stored_header_stories());
    let paragraphs = stories.flat_map(|story| story.paras.iter());
    let (mut margin_frame, mut latent_table_indent) = (false, false);
    for paragraph in paragraphs {
        margin_frame |= paragraph
            .pap
            .frame
            .is_some_and(|frame| frame.v != FrameV::InLine && frame.v_rel == FrameRel::Margin);
        latent_table_indent |= paragraph
            .row
            .as_ref()
            .is_some_and(|row| row.align != Align::Left && row.indent != row.gap / 2);
    }
    if margin_frame {
        settings.push("• Frame positions relative to the vertical margin");
    }
    if latent_table_indent {
        settings.push("• Stored indents on centered or right-aligned table rows");
    }
    settings
}

/// The fonts a document's text uses that are not installed, each with the
/// font shown in its place.
fn missing_fonts(doc: &quill_core::Document, fonts: &quill_layout::Fonts) -> Vec<(String, String)> {
    let mut used: Vec<u16> = Vec::new();
    let stories = std::iter::once(&doc.main)
        .chain(doc.stored_header_stories())
        .chain([&doc.notes, &doc.annotations]);
    for story in stories {
        for p in &story.paras {
            for run in p.runs.iter().filter(|r| r.len > 0) {
                if !used.contains(&run.chp.font) {
                    used.push(run.chp.font);
                }
            }
        }
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for id in used {
        let name = doc.font_name(id);
        if let Some(shown) = fonts.substitute(name)
            && !out.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))
        {
            out.push((name.to_string(), shown.to_string()));
        }
    }
    out
}

/// What to tell the person who opened a file: what the reader could not
/// keep exactly and which fonts are standing in for missing ones. None
/// when there is nothing to say.
fn open_warning(
    report: &quill_formats::report::ImportReport,
    missing: &[(String, String)],
    wide_tables: &[usize],
) -> Option<String> {
    const MOST_FONTS: usize = 12;
    let mut parts = Vec::new();
    if !report.is_empty() {
        let lines: Vec<String> = report.notes().iter().map(|n| format!("• {n}")).collect();
        parts.push(format!(
            "Some of it could not be kept exactly:\n{}",
            lines.join("\n")
        ));
    }
    if !missing.is_empty() {
        let mut lines: Vec<String> = missing
            .iter()
            .take(MOST_FONTS)
            .map(|(font, shown)| format!("• {font}, shown as {shown}"))
            .collect();
        if missing.len() > MOST_FONTS {
            lines.push(format!("• and {} more", missing.len() - MOST_FONTS));
        }
        parts.push(format!(
            "These fonts are not installed, so another font is shown in their place:\n{}",
            lines.join("\n")
        ));
    }
    if !wide_tables.is_empty() {
        parts.push(format!(
            "There may be a formatting error on {}: a table is wider than its column \
             and may run over the text beside it.",
            page_list(wide_tables)
        ));
    }
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// "page 8", "pages 8 and 12", "pages 3, 8 and 12", or the first few and
/// how many more.
fn page_list(pages: &[usize]) -> String {
    const MOST: usize = 8;
    let mut shown: Vec<String> = pages.iter().take(MOST).map(|p| p.to_string()).collect();
    if pages.len() > MOST {
        shown.push(format!("{} more", pages.len() - MOST));
    }
    match shown.len() {
        1 => format!("page {}", shown[0]),
        n => format!("pages {} and {}", shown[..n - 1].join(", "), shown[n - 1]),
    }
}

#[cfg(test)]
mod tests {
    use super::{docx_word_rewrite_losses, export_document, record_saved_file, save_policy};
    use crate::docview::DocView;
    use quill_core::{Document, Position, StoryId};
    use quill_formats::Format;

    #[test]
    fn a_file_changed_by_something_else_is_noticed_before_saving() {
        let dir = std::env::temp_dir().join(format!("quill-stamp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("letter.qwp");
        std::fs::write(&path, b"one").unwrap();
        let mut view = DocView::new(Document::new(), "letter.qwp".into());
        // Not opened or saved from the file yet: nothing to compare.
        assert!(!super::changed_on_disk(&view, &path));
        view.path = Some(path.clone());
        view.disk_stamp = super::disk_stamp(&path);
        assert!(!super::changed_on_disk(&view, &path));
        // Another program changes it (a different size, so the check holds
        // even on a file system with coarse times).
        std::fs::write(&path, b"changed elsewhere").unwrap();
        assert!(super::changed_on_disk(&view, &path));
        // A different file is not this document's own.
        let other = dir.join("other.qwp");
        std::fs::write(&other, b"x").unwrap();
        assert!(!super::changed_on_disk(&view, &other));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_open_warning_lists_losses_then_fonts() {
        use quill_formats::report::{self, ImportReport};
        assert_eq!(
            super::open_warning(&ImportReport::default(), &[], &[]),
            None
        );
        let mut r = ImportReport::default();
        r.note(report::EQUATIONS);
        let fonts: Vec<(String, String)> = (0..14)
            .map(|i| (format!("Font {i}"), "Times".to_string()))
            .collect();
        let text = super::open_warning(&r, &fonts, &[]).unwrap();
        assert!(text.starts_with(&format!(
            "Some of it could not be kept exactly:\n• {}",
            report::EQUATIONS
        )));
        assert!(text.contains("• Font 0, shown as Times"));
        assert!(text.ends_with("• and 2 more"));
    }

    #[test]
    fn the_open_warning_names_pages_with_tables_too_wide_for_their_column() {
        use quill_formats::report::ImportReport;
        let text = super::open_warning(&ImportReport::default(), &[], &[8]).unwrap();
        assert!(text.starts_with("There may be a formatting error on page 8: a table"));
        assert_eq!(super::page_list(&[8, 12]), "pages 8 and 12");
        assert_eq!(super::page_list(&[3, 8, 12]), "pages 3, 8 and 12");
        let many: Vec<usize> = (1..=10).collect();
        assert_eq!(
            super::page_list(&many),
            "pages 1, 2, 3, 4, 5, 6, 7, 8 and 2 more"
        );
    }

    #[test]
    fn missing_fonts_are_the_used_ones_not_installed() {
        let path = std::path::PathBuf::from("unused.ttf");
        let fonts = quill_layout::Fonts::new(quill_layout::Catalog::from_files(&[(
            "Times", path, 0, 400, false,
        )]));
        let mut doc = Document::new();
        let calibri = doc.font_id("Calibri");
        let unused = doc.font_id("Zapfino");
        assert_ne!(calibri, unused);
        let chp = quill_core::CharProps {
            font: calibri,
            ..doc.styles.get(0).chp
        };
        doc.main.paras[0].insert(0, "Hello", chp);
        assert_eq!(
            super::missing_fonts(&doc, &fonts),
            [("Calibri".to_string(), "Times".to_string())]
        );
    }

    #[test]
    fn foreign_save_default_preserves_explicitly_chosen_formats() {
        use super::save_policy;
        use quill_formats::Format;
        assert_eq!(
            save_policy(Some(Format::Docx), false, false, true),
            (false, Format::Quill)
        );
        assert_eq!(
            save_policy(Some(Format::Rtf), false, false, false),
            (true, Format::Rtf)
        );
        assert!(save_policy(Some(Format::Docx), false, true, true).0);
        assert_eq!(
            save_policy(Some(Format::Docx), true, true, false),
            (false, Format::Docx)
        );
        assert_eq!(
            save_policy(None, false, false, true),
            (false, Format::Quill)
        );
    }

    #[test]
    fn every_explicit_save_as_format_becomes_the_save_destination() {
        for format in Format::ALL {
            for native_default in [false, true] {
                assert_eq!(
                    save_policy(Some(format), false, true, native_default),
                    (true, format)
                );
                assert_eq!(
                    save_policy(Some(format), true, true, native_default),
                    (false, format)
                );
            }
        }
    }

    #[test]
    fn lossy_save_as_warns_before_adopting_the_file() {
        let mut doc = Document::new();
        for format in [Format::Text, Format::Odt, Format::Html] {
            let warning = super::save_format_warning(&doc, format, true).unwrap();
            assert!(warning.contains("saved file will become the document"));
            assert!(warning.contains("Save a .qwp copy first"));
            assert!(!warning.contains("Tracked changes"));
            assert!(super::save_format_warning(&doc, format, false).is_none());
        }
        doc.props.track_changes = true;
        doc.insert_text(
            StoryId::Main,
            Position::new(0, 0),
            "Change",
            Default::default(),
        );
        for format in [Format::Text, Format::Odt, Format::Html] {
            let warning = super::save_format_warning(&doc, format, false).unwrap();
            assert!(warning.contains("Tracked changes will be accepted"));
        }
        for format in [Format::Quill, Format::Docx, Format::Rtf, Format::Markdown] {
            assert!(super::save_format_warning(&doc, format, true).is_none());
        }
    }

    #[test]
    fn saving_as_another_format_names_a_lost_character_style_link() {
        let mut doc = quill_core::text::from_plain_text("Body");
        let callout = doc
            .add_character_style(
                "Callout",
                quill_core::character_styles::CharacterFormat::default(),
            )
            .unwrap();
        let (pap, own) = (doc.styles.get(0).pap.clone(), doc.styles.get(0).chp);
        doc.redefine_style_linked(0, pap, own, callout);
        for format in [Format::Docx, Format::Rtf, Format::Odt, Format::Text] {
            let warning = super::save_format_warning(&doc, format, true).unwrap();
            assert!(
                warning.contains("linked to a character style"),
                "{format:?}"
            );
            assert!(!warning.starts_with(' '));
        }
        assert!(super::save_format_warning(&doc, Format::Quill, true).is_none());
        // Saving again in the format already chosen does not ask again.
        assert!(super::save_format_warning(&doc, Format::Docx, false).is_none());
    }

    #[test]
    fn same_file_sees_through_how_a_path_is_written() {
        let root = std::env::temp_dir().join(format!("quill-same-file-{}", std::process::id()));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let (a, b) = (root.join("a.qwp"), root.join("b.qwp"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        assert!(super::same_file(&a, &a));
        assert!(super::same_file(&a, &root.join("sub/../a.qwp")));
        assert!(!super::same_file(&a, &b));
        // A file that has gone only matches its own path.
        assert!(!super::same_file(&a, &root.join("gone.qwp")));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn save_as_adopts_each_former_export_and_keeps_the_original_file_unchanged() {
        let root = std::env::temp_dir().join(format!("quill-save-as-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        for format in [Format::Text, Format::Odt, Format::Html] {
            let original = root.join(format!("Original-{}.qwp", format.extensions()[0]));
            let destination = root.join(format!("Saved.{}", format.extensions()[0]));
            let mut view = DocView::new(Document::new(), "Original.qwp".into());
            view.type_text("Original sentence.");
            view.doc.format_chars(
                StoryId::Main,
                Position::new(0, 0),
                Position::new(0, 8),
                |c| c.bold = true,
            );
            view.doc.commit(view.sel);
            quill_formats::save(&view.doc, &original, Format::Quill).unwrap();
            record_saved_file(&mut view, &original, true);
            let original_bytes = std::fs::read(&original).unwrap();
            view.type_text(" Added sentence.");
            let before = view.doc.main.paras.clone();
            let undo = view.doc.undo_label();
            assert!(view.doc.is_modified());
            quill_formats::save(&view.doc, &destination, format).unwrap();
            record_saved_file(&mut view, &destination, true);
            assert!(
                !view.doc.is_modified(),
                "closing should not ask to save the old file"
            );
            assert_eq!(view.path.as_ref(), Some(&destination));
            assert_eq!(view.title, super::file_name(&destination));
            assert!(view.path_chosen);
            assert_eq!(view.doc.main.paras, before);
            assert_eq!(view.doc.undo_label(), undo);
            let back = quill_formats::load_reported(&destination, true).unwrap().0;
            assert_eq!(
                back.main.paras[0].text,
                "Original sentence. Added sentence."
            );
            view.type_text(" More editing.");
            assert!(
                view.doc.is_modified(),
                "editing the saved file must ask to save again"
            );
            assert!(save_policy(Some(format), false, view.path_chosen, true).0);
            quill_formats::save(&view.doc, view.path.as_ref().unwrap(), format).unwrap();
            record_saved_file(&mut view, &destination, false);
            assert!(!view.doc.is_modified());
            let back = quill_formats::load_reported(&destination, true).unwrap().0;
            assert!(back.main.paras[0].text.ends_with(" More editing."));
            assert_eq!(std::fs::read(&original).unwrap(), original_bytes);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn docx_warning_names_only_word_losses_in_use() {
        let mut doc = Document::new();
        assert!(docx_word_rewrite_losses(&doc).is_empty());

        doc.section.margin_page_numbers = true;
        assert_eq!(docx_word_rewrite_losses(&doc), ["• Margin page numbers"]);

        doc.main.paras[0].pap.frame = Some(quill_core::Frame {
            v: quill_core::FrameV::At(720),
            v_rel: quill_core::FrameRel::Margin,
            ..Default::default()
        });
        let mut row = quill_core::RowProps::even(1, 1_000);
        row.align = quill_core::Align::Center;
        row.indent = 360;
        row.gap = 160;
        doc.main.paras[0].row = Some(Box::new(row));
        let losses = docx_word_rewrite_losses(&doc);
        assert!(losses.contains(&"• Frame positions relative to the vertical margin"));
        assert!(losses.contains(&"• Stored indents on centered or right-aligned table rows"));
    }

    #[test]
    fn docx_warning_and_write_use_one_document_in_markdown_source_view() {
        let mut doc = Document::new();
        doc.section.margin_page_numbers = true;
        let mut view = DocView::new(doc, "Report.md".into());
        // Formatted view: the window's own document is the one written.
        assert!(export_document(&view, Format::Docx).unwrap().is_none());
        let formatted = docx_word_rewrite_losses(&view.doc);
        assert_eq!(formatted, ["• Margin page numbers"]);

        // Source view: the window holds only the text. The warning must
        // inspect the formatted document the writer will use.
        view.show_markdown_source();
        assert!(docx_word_rewrite_losses(&view.doc).is_empty());
        let prepared = export_document(&view, Format::Docx).unwrap().unwrap();
        assert_eq!(docx_word_rewrite_losses(&prepared), formatted);
        // Accepting writes that same document: the setting is in the file.
        let dir = std::env::temp_dir().join(format!("quill-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Report.docx");
        quill_formats::save(&prepared, &path, Format::Docx).unwrap();
        let back = quill_formats::load(&path).unwrap();
        assert!(back.section.margin_page_numbers);
        std::fs::remove_dir_all(&dir).unwrap();
        // A Markdown save still gets the formatted document for its own
        // warning, and the text is what it writes.
        assert!(export_document(&view, Format::Markdown).unwrap().is_some());
    }

    #[test]
    fn source_that_cannot_be_read_back_stops_every_export_but_markdown() {
        let mut view = DocView::new(Document::new(), "Big.md".into());
        view.show_markdown_source();
        // More paragraphs than a document may have.
        let big = "a\n\n".repeat(quill_core::limits::MAX_PARAGRAPHS + 1);
        view.doc = quill_formats::markdown::source_document(&big);
        assert!(view.source_edited());
        for format in [Format::Docx, Format::Quill, Format::Text] {
            let error = export_document(&view, format).unwrap_err();
            assert!(error.contains("too large"), "{error}");
        }
        assert!(export_document(&view, Format::Markdown).unwrap().is_none());
    }

    /// Answers a save's questions from a script and counts each question.
    #[derive(Default)]
    struct Script {
        markdown: std::collections::VecDeque<bool>,
        replace: std::collections::VecDeque<bool>,
        markdown_asked: usize,
        replace_asked: usize,
    }

    impl super::SavePrompts for Script {
        fn docx_losses(&mut self, _: &[&'static str]) -> bool {
            panic!("no Word question expected");
        }
        fn markdown_losses(&mut self, _: &[&'static str]) -> bool {
            self.markdown_asked += 1;
            self.markdown
                .pop_front()
                .expect("an answer for the Markdown question")
        }
        fn format_warning(&mut self, _: String) -> bool {
            panic!("no format question expected");
        }
        fn replace_changed_file(&mut self, _: &str) -> bool {
            self.replace_asked += 1;
            self.replace
                .pop_front()
                .expect("an answer for Replace File")
        }
    }

    #[test]
    fn markdown_losses_are_remembered_only_by_a_save_that_happened() {
        let dir = std::env::temp_dir().join(format!("quill-md-warned-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Notes.md");
        std::fs::write(&path, "one").unwrap();
        let mut view = DocView::new(Document::new(), "Notes.md".into());
        view.path = Some(path.clone());
        view.disk_stamp = super::disk_stamp(&path);
        // A feature Markdown cannot carry, added to a document that had none.
        view.doc.summary.title = "Draft".into();
        let md = Format::Markdown;

        // Accept the Markdown warning, but something else has changed the
        // file and the person cancels Replace File: nothing was saved.
        std::fs::write(&path, "changed elsewhere").unwrap();
        let mut ask = Script::default();
        ask.markdown.push_back(true);
        ask.replace.push_back(false);
        assert!(super::confirm_save(&view, &path, md, false, &mut ask).is_err());
        assert_eq!((ask.markdown_asked, ask.replace_asked), (1, 1));

        // Saving again must warn again: the first attempt wrote nothing.
        let mut ask = Script::default();
        ask.markdown.push_back(true);
        ask.replace.push_back(true);
        let confirmed = super::confirm_save(&view, &path, md, false, &mut ask)
            .ok()
            .expect("accepted both");
        assert_eq!(
            (ask.markdown_asked, ask.replace_asked),
            (1, 1),
            "the warning came back after a cancelled save"
        );

        // The save happened, as save() records it: now the same losses are
        // not asked about again.
        record_saved_file(&mut view, &path, false);
        confirmed.remember(&mut view);
        let mut ask = Script::default();
        assert!(super::confirm_save(&view, &path, md, false, &mut ask).is_ok());
        assert_eq!((ask.markdown_asked, ask.replace_asked), (0, 0));
        // A newly added unsupported feature is a new loss and asks again.
        view.doc.section.columns = 2;
        let mut ask = Script::default();
        ask.markdown.push_back(true);
        assert!(super::confirm_save(&view, &path, md, false, &mut ask).is_ok());
        assert_eq!(ask.markdown_asked, 1);
        // And a person who declines is not asked a second thing.
        let mut ask = Script::default();
        ask.markdown.push_back(false);
        assert!(super::confirm_save(&view, &path, md, false, &mut ask).is_err());
        assert_eq!(ask.replace_asked, 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A two-section document, the first with text in an installed font,
    /// whose second section owns its headers and footers. `fill` changes the
    /// second section's header story.
    fn second_section_owns_its_header(
        fill: impl FnOnce(&mut Document, &mut quill_core::Story),
    ) -> Document {
        let mut doc = Document::new();
        let times = doc.font_id("Times");
        let chp = quill_core::CharProps {
            font: times,
            ..doc.styles.get(0).chp
        };
        doc.main.paras[0].insert(0, "Body text", chp);
        doc.insert_section_break(Position::new(0, 4));
        let mut header = quill_core::text::from_plain_text("").main;
        fill(&mut doc, &mut header);
        let mut set = vec![quill_core::text::from_plain_text("").main; 6];
        set[0] = header;
        doc.section.headers = Some(std::sync::Arc::new(set));
        doc
    }

    #[test]
    fn a_font_only_in_a_section_owned_header_is_reported_missing() {
        let fonts = quill_layout::Fonts::new(quill_layout::Catalog::from_files(&[(
            "Times",
            std::path::PathBuf::from("unused.ttf"),
            0,
            400,
            false,
        )]));
        let doc = second_section_owns_its_header(|doc, header| {
            let zapfino = doc.font_id("Zapfino");
            let chp = quill_core::CharProps {
                font: zapfino,
                ..doc.styles.get(0).chp
            };
            header.paras[0].insert(0, "Chapter two", chp);
        });
        assert_eq!(
            super::missing_fonts(&doc, &fonts),
            [("Zapfino".to_string(), "Times".to_string())]
        );
    }

    #[test]
    fn a_margin_frame_only_in_a_section_owned_header_is_named_by_the_docx_warning() {
        let doc = second_section_owns_its_header(|_, header| {
            header.paras[0].pap.frame = Some(quill_core::Frame {
                v: quill_core::FrameV::At(720),
                v_rel: quill_core::FrameRel::Margin,
                ..Default::default()
            });
        });
        assert_eq!(
            docx_word_rewrite_losses(&doc),
            ["• Frame positions relative to the vertical margin"]
        );
    }

    #[test]
    fn replacing_a_font_found_in_a_section_owned_header_can_be_undone() {
        let fonts = quill_layout::Fonts::new(quill_layout::Catalog::from_files(&[(
            "Times",
            std::path::PathBuf::from("unused.ttf"),
            0,
            400,
            false,
        )]));
        let mut doc = second_section_owns_its_header(|doc, header| {
            let zapfino = doc.font_id("Zapfino");
            let chp = quill_core::CharProps {
                font: zapfino,
                ..doc.styles.get(0).chp
            };
            header.paras[0].insert(0, "Chapter two", chp);
        });
        let missing = super::missing_fonts(&doc, &fonts);
        assert_eq!(missing.len(), 1);
        let (zapfino, times) = (doc.font_id("Zapfino"), doc.font_id("Times"));
        let sel = quill_core::Selection::default();
        doc.begin("Replace Fonts", sel);
        doc.replace_font(&[zapfino], times);
        doc.commit(sel);
        assert!(super::missing_fonts(&doc, &fonts).is_empty());
        doc.undo();
        assert_eq!(super::missing_fonts(&doc, &fonts), missing);
    }

    #[test]
    fn linked_headers_are_reported_once() {
        let fonts = quill_layout::Fonts::new(quill_layout::Catalog::from_files(&[(
            "Times",
            std::path::PathBuf::from("unused.ttf"),
            0,
            400,
            false,
        )]));
        // Three sections; the headers in the first are shown by the others
        // (Same As Previous), so they are stored once.
        let mut doc = Document::new();
        let times = doc.font_id("Times");
        let zapfino = doc.font_id("Zapfino");
        let body = quill_core::CharProps {
            font: times,
            ..doc.styles.get(0).chp
        };
        doc.main.paras[0].insert(0, "Body text", body);
        doc.insert_section_break(Position::new(0, 2));
        doc.insert_section_break(Position::new(1, 1));
        let chp = quill_core::CharProps {
            font: zapfino,
            ..doc.styles.get(0).chp
        };
        doc.headers[0].paras[0].insert(0, "Running head", chp);
        doc.headers[0].paras[0].pap.frame = Some(quill_core::Frame {
            v: quill_core::FrameV::At(720),
            v_rel: quill_core::FrameRel::Margin,
            ..Default::default()
        });
        assert_eq!(doc.sections().len(), 3);
        assert_eq!(
            super::missing_fonts(&doc, &fonts),
            [("Zapfino".to_string(), "Times".to_string())]
        );
        assert_eq!(
            docx_word_rewrite_losses(&doc),
            ["• Frame positions relative to the vertical margin"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_markdown_save_asks_for_the_pictures_folder_only_when_that_was_refused() {
        use std::os::unix::fs::PermissionsExt;
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
        let dir = std::env::temp_dir().join(format!("quill-md-denied-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        if std::fs::write(dir.join("probe"), b"x").is_ok() {
            // Running as a user who can write anywhere: nothing to refuse.
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let path = dir.join("Notes.md");
        let md = Format::Markdown;
        // Formatted view with a picture: the pictures folder is refused.
        let (doc, _) = quill_formats::markdown::read(
            &format!("One ![a][d].\n\n[d]: data:image/png;base64,{png}\n"),
            None,
        );
        let mut view = DocView::new(doc, "Notes.md".into());
        let failure = super::write_document(&view, None, &path, md).unwrap_err();
        assert!(failure.picture_folder_denied, "{}", failure.message);
        // Source view saves only the text: when the file itself is refused,
        // that is the error, and no folder is asked for.
        view.show_markdown_source();
        let failure = super::write_document(&view, None, &path, md).unwrap_err();
        assert!(!failure.picture_folder_denied);
        assert!(failure.message.contains("Notes.md"), "{}", failure.message);
        // A formatted document without pictures has no pictures folder.
        let mut plain = DocView::new(Document::new(), "Notes.md".into());
        plain.type_text("Just text");
        let failure = super::write_document(&plain, None, &path, md).unwrap_err();
        assert!(!failure.picture_folder_denied);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_as_refuses_a_typed_markdown_name_unless_it_is_the_documents_own_file() {
        use super::savable_by_save_as;
        use std::path::Path;
        let md = Path::new("/d/Notes.md");
        // Markdown is on hold: not for a document of another format, nor
        // another Markdown file.
        assert!(!savable_by_save_as(md, None));
        assert!(!savable_by_save_as(md, Some(Path::new("/d/Notes.qwp"))));
        assert!(!savable_by_save_as(md, Some(Path::new("/d/Other.md"))));
        // A Markdown document saving over itself is its own file.
        assert!(savable_by_save_as(md, Some(md)));
        // The other formats, and names Quill does not write.
        for name in [
            "Notes.qwp",
            "Notes.docx",
            "Notes.rtf",
            "Notes.odt",
            "Notes.html",
            "Notes.txt",
        ] {
            assert!(savable_by_save_as(Path::new(name), None), "{name}");
        }
        assert!(!savable_by_save_as(Path::new("Notes.pages"), None));
    }
}
