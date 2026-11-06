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
            "View > Highlight Missing Fonts marks the text they affect, and Format > Replace \
             Missing Fonts chooses installed fonts for them.",
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
        let answer = super::message_box(
            rfd::MessageLevel::Warning,
            "Quill",
            format!("{message}\n\n{detail}"),
            rfd::MessageButtons::YesNoCancelCustom(
                "OK".into(),
                "Keep These Fonts".into(),
                "Replace Fonts…".into(),
            ),
        );
        match answer {
            rfd::MessageDialogResult::Custom(b) if b == "Keep These Fonts" => {
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
            rfd::MessageDialogResult::Custom(b) if b == "Replace Fonts…" => {
                self.replace_missing_fonts(window);
            }
            _ => font_reminder(),
        }
    }

    /// Format > Replace Missing Fonts…
    pub(super) fn replace_missing_fonts(&mut self, id: WindowId) {
        self.ensure_fonts();
        let (Some(i), Some(fonts)) = (self.index(id), self.fonts.as_ref()) else {
            return;
        };
        let w = &mut self.windows[i].1;
        let missing = missing_fonts(&w.st.main_view().doc, fonts);
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
        let mut dialog = rfd::FileDialog::new()
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
        if let Some(paths) = dialog.pick_files() {
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
        let path = match known {
            Some(path) if direct => path,
            _ => {
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
                let selected = crate::mac_save::save_file(&w.window, &filename, directory);
                #[cfg(not(target_os = "macos"))]
                let selected = {
                    let mut dialog = rfd::FileDialog::new()
                        .set_title("Save As")
                        .set_file_name(&filename);
                    for format in std::iter::once(initial)
                        .chain(Format::ALL.into_iter().filter(|f| *f != initial))
                    {
                        dialog = dialog.add_filter(format.name(), format.extensions());
                    }
                    if let Some(dir) = directory {
                        dialog = dialog.set_directory(dir);
                    }
                    dialog.save_file()
                };
                chosen = true;
                match selected {
                    Some(path) if Format::from_path(&path).is_some() => path,
                    Some(path) => path.with_extension("qwp"),
                    None => return false,
                }
            }
        };
        let format = Format::from_path(&path).unwrap_or(Format::Quill);
        if chosen && format == Format::Docx {
            let losses = docx_word_rewrite_losses(&w.st.main_view().doc);
            if !losses.is_empty() {
                let answer = super::message_box(
                    rfd::MessageLevel::Warning,
                    "Some Settings May Be Lost",
                    format!(
                        "Microsoft Word may remove these Quill-only settings when it resaves this file:\n\n{}\n\nSave a .qwp copy to preserve them. Save as a Word document anyway?",
                        losses.join("\n")
                    ),
                    rfd::MessageButtons::YesNo,
                );
                if answer != rfd::MessageDialogResult::Yes {
                    return false;
                }
            }
        }
        if format == Format::Markdown && !w.st.main_view().in_markdown_source() {
            let view = w.st.main_view_mut();
            let lost = quill_formats::markdown::losses(
                &view.doc,
                &crate::settings::with(|s| s.document.clone()),
            );
            let unwarned = lost.iter().any(|l| !view.md_warned.contains(l));
            if (chosen && !lost.is_empty()) || unwarned {
                let answer = super::message_box(
                    rfd::MessageLevel::Warning,
                    "Some Features Will Be Left Out",
                    format!(
                        "Markdown cannot carry these, so the saved file will leave them out:\n\n{}\n\nThe document you're editing keeps them, but the next time you open the file they will be gone. Save a .qwp copy to keep them. Save as Markdown anyway?",
                        super::markdown::bullet_list(&lost)
                    ),
                    rfd::MessageButtons::YesNo,
                );
                if answer != rfd::MessageDialogResult::Yes {
                    return false;
                }
            }
            view.md_warned = lost;
        }
        let warning = {
            let view = w.st.main_view();
            let formatted = view.markdown_document();
            save_format_warning(formatted.as_ref().unwrap_or(&view.doc), format, chosen)
        };
        if let Some(warning) = warning {
            let answer = super::message_box(
                rfd::MessageLevel::Warning,
                "Some Features Will Be Left Out",
                warning,
                rfd::MessageButtons::YesNo,
            );
            if answer != rfd::MessageDialogResult::Yes {
                return false;
            }
        }
        if crate::settings::with(|settings| settings.make_backup) && path.exists() {
            let backup = path.with_file_name(format!("Backup of {}", file_name(&path)));
            if let Err(error) = quill_formats::copy_atomic(&path, &backup) {
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
        }
        // The next time it opens, it opens in this view.
        let view = w.st.main_view_mut();
        view.doc.props.view = Some(view.current_view());
        // In Markdown source view the text is the file for Markdown, and
        // every other format gets the document the text stands for.
        let saved = {
            let view = w.st.main_view();
            match (view.markdown_text(), format) {
                (Some(text), Format::Markdown) => quill_formats::save_markdown_text(&text, &path),
                (Some(_), _) => match view.markdown_document() {
                    Some(doc) => quill_formats::save(&doc, &path, format),
                    None => Err("The Markdown could not be read back.".into()),
                },
                (None, _) => quill_formats::save(&view.doc, &path, format),
            }
        };
        match saved {
            Ok(()) => {
                record_saved_file(w.st.main_view_mut(), &path, chosen);
                crate::recovery::remove(w.recovery.take());
                w.window.request_redraw();
                self.retitle();
                crate::recent::note(&mut self.recent, &path);
                true
            }
            Err(error) => {
                alert(
                    &format!("Quill could not save \u{201c}{}\u{201d}.", file_name(&path)),
                    &error,
                );
                false
            }
        }
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
fn save_format_warning(doc: &quill_core::Document, format: Format, chosen: bool) -> Option<String> {
    // Only Quill's own format keeps a paragraph style's link to a character
    // style; the others keep the formatting it gives.
    let linked =
        chosen && format != Format::Quill && doc.styles.styles.iter().any(|s| s.character != 0);
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
fn record_saved_file(view: &mut DocView, path: &Path, chosen: bool) {
    view.doc.mark_saved();
    view.title = file_name(path);
    view.path = Some(path.to_path_buf());
    view.path_chosen |= chosen;
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
    if (0..sections).any(|section| doc.nth_section(section).include_endnotes) {
        settings.push("• Include Endnotes");
    }
    if doc.props.print_hidden {
        settings.push("• Print Hidden Text");
    }
    let stories = [
        StoryId::Main,
        StoryId::Header,
        StoryId::Footer,
        StoryId::FirstHeader,
        StoryId::FirstFooter,
        StoryId::EvenHeader,
        StoryId::EvenFooter,
        StoryId::Notes,
        StoryId::Annotations,
    ];
    let paragraphs = stories
        .into_iter()
        .flat_map(|story| doc.story(story).paras.iter());
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
        .chain(doc.headers.iter())
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
    use super::{docx_word_rewrite_losses, record_saved_file, save_policy};
    use crate::docview::DocView;
    use quill_core::{Document, Position, StoryId};
    use quill_formats::Format;

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
        doc.redefine_style_linked(StoryId::Main, 0, pap, own, callout);
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
        doc.section.include_endnotes = true;
        doc.props.print_hidden = true;
        assert_eq!(
            docx_word_rewrite_losses(&doc),
            [
                "• Margin page numbers",
                "• Include Endnotes",
                "• Print Hidden Text"
            ]
        );

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
}
