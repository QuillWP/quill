//! Opening, saving, backup creation, and close confirmation.

use super::*;
use std::path::Path;

impl App {
    /// Open documents in new windows. An untouched Untitled window is reused.
    pub(super) fn open_files(&mut self, el: &ActiveEventLoop, paths: &[PathBuf]) {
        for path in paths {
            let rtf = crate::settings::with(|s| s.interpret_rtf);
            let (mut doc, report) = match quill_formats::load_reported(path, rtf) {
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
                        Ok(doc) => (doc, Default::default()),
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
            crate::image::prepare_document_previews(&mut doc);
            self.ensure_fonts();
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
                    w.window.set_title(&title);
                    w.window.request_redraw();
                    i
                }
                None => {
                    self.new_window(el, doc, Some(title));
                    self.windows.len() - 1
                }
            };
            if let Some((_, w)) = self.windows.get_mut(i) {
                w.st.main_view_mut().path = Some(path.clone());
            }
            crate::recent::note(&mut self.recent, path);
            if let Some(detail) = open_warning(&report, &missing) {
                warn(
                    &format!(
                        "Quill opened \u{201c}{}\u{201d} with some changes.",
                        file_name(path)
                    ),
                    &detail,
                );
            }
        }
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
                let selected = crate::mac_save::save_file(&filename, directory);
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
                let answer = rfd::MessageDialog::new()
                    .set_level(rfd::MessageLevel::Warning)
                    .set_title("Some Settings May Be Lost")
                    .set_description(format!(
                        "Microsoft Word may remove these Quill-only settings when it resaves this file:\n\n{}\n\nSave a .qwp copy to preserve them. Save as a Word document anyway?",
                        losses.join("\n")
                    ))
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show();
                if answer != rfd::MessageDialogResult::Yes {
                    return false;
                }
            }
        }
        if chosen
            && matches!(format, Format::Odt | Format::Html | Format::Text)
            && w.st.main_view().doc.has_changes()
        {
            let answer = rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Warning)
                .set_title("Tracked Changes Will Be Accepted")
                .set_description(format!(
                    "{} can't hold tracked changes, so the exported file will have every change accepted. The document you're editing keeps them. Export anyway?",
                    format.name()
                ))
                .set_buttons(rfd::MessageButtons::YesNo)
                .show();
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
        match quill_formats::save(&w.st.main_view().doc, &path, format) {
            Ok(()) => {
                let editable_saved = record_saved_file(w.st.main_view_mut(), &path, format, chosen);
                if editable_saved {
                    crate::recovery::remove(w.recovery.take());
                }
                w.window.request_redraw();
                if editable_saved {
                    self.retitle();
                    crate::recent::note(&mut self.recent, &path);
                }
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
        let answer = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("Quill")
            .set_description(format!(
                "Save changes to \u{201c}{}\u{201d} before closing?",
                w.st.main_view().title
            ))
            .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
                "Save".into(),
                "Don\u{2019}t Save".into(),
                "Cancel".into(),
            ))
            .show();
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

/// Only a complete editable save replaces the document's identity and saved point.
/// Export success leaves its recovery copy, dirty state, and original path intact.
fn record_saved_file(view: &mut DocView, path: &Path, format: Format, chosen: bool) -> bool {
    if format.export_only() {
        view.status = Some(format!(
            "Exported {} - editable original remains open",
            file_name(path)
        ));
        return false;
    }
    view.doc.mark_saved();
    view.title = file_name(path);
    view.path = Some(path.to_path_buf());
    view.path_chosen |= chosen;
    true
}

fn save_policy(
    format: Option<Format>,
    ask: bool,
    chosen: bool,
    native_default: bool,
) -> (bool, Format) {
    // A lossy source needs an editable destination even when the user prefers
    // saving imported files in their original format. Export is always explicit.
    if format.is_some_and(Format::export_only) {
        return (false, Format::Quill);
    }
    let foreign = format.is_some_and(|f| f != Format::Quill);
    let direct = format.is_some() && !ask && (!foreign || chosen || !native_default);
    let initial = if foreign && native_default {
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
    (!parts.is_empty()).then(|| parts.join("\n\n"))
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
        assert_eq!(super::open_warning(&ImportReport::default(), &[]), None);
        let mut r = ImportReport::default();
        r.note(report::EQUATIONS);
        let fonts: Vec<(String, String)> = (0..14)
            .map(|i| (format!("Font {i}"), "Times".to_string()))
            .collect();
        let text = super::open_warning(&r, &fonts).unwrap();
        assert!(text.starts_with(&format!(
            "Some of it could not be kept exactly:\n• {}",
            report::EQUATIONS
        )));
        assert!(text.contains("• Font 0, shown as Times"));
        assert!(text.ends_with("• and 2 more"));
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
    fn exports_never_become_the_implicit_save_destination() {
        for format in [Format::Text, Format::Odt, Format::Html] {
            for ask in [false, true] {
                for chosen in [false, true] {
                    for native_default in [false, true] {
                        assert_eq!(
                            save_policy(Some(format), ask, chosen, native_default),
                            (false, Format::Quill)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn text_export_keeps_original_dirty_and_editable_save_clears_it() {
        let root = std::env::temp_dir().join(format!("quill-text-export-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let original = root.join("Original.qwp");
        let text = root.join("Copy.txt");
        let mut view = DocView::new(Document::new(), "Original.qwp".into());
        view.path = Some(original.clone());
        view.path_chosen = true;
        view.type_text("Keep my formatting");
        view.doc.format_chars(
            StoryId::Main,
            Position::new(0, 0),
            Position::new(0, 4),
            |c| c.bold = true,
        );
        view.doc.commit(view.sel);
        let before = view.doc.main.paras.clone();
        let undo = view.doc.undo_label();
        assert!(view.doc.is_modified());
        quill_formats::save(&view.doc, &text, Format::Text).unwrap();
        assert!(
            std::fs::read_to_string(&text)
                .unwrap()
                .contains("Keep my formatting")
        );
        assert!(!record_saved_file(&mut view, &text, Format::Text, true));
        assert!(
            view.doc.is_modified(),
            "closing must still ask to save the editable original"
        );
        assert_eq!(view.path.as_ref(), Some(&original));
        assert_eq!(view.title, "Original.qwp");
        assert!(view.path_chosen);
        assert_eq!(view.doc.main.paras, before);
        assert_eq!(view.doc.undo_label(), undo);
        assert!(!original.exists());
        quill_formats::save(&view.doc, &original, Format::Quill).unwrap();
        assert!(record_saved_file(&mut view, &original, Format::Quill, true));
        assert!(!view.doc.is_modified());
        let back = quill_formats::load_reported(&original, true).unwrap().0;
        assert!(back.main.paras[0].chp_at(0).bold);
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
