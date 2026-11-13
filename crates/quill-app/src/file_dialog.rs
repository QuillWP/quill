//! File dialogs and message boxes for Windows and Linux (the Mac has its own
//! panels, and uses `rfd` for Open).
//!
//! Off Linux these are `rfd`'s. On Linux Quill draws them itself with GTK, on
//! the main thread: `rfd`'s GTK backend runs every dialog on a thread of its
//! own and waits for it, and when the program already uses GTK on the main
//! thread (as it does, to host its window) the dialog never appears and the
//! program freezes with a CPU at 100%. The Open dialog of a build on Ubuntu
//! 24.04 showed exactly that in the `--with-events` log: "begin: Open file
//! dialog" with no "end".

use std::path::{Path, PathBuf};

/// The part of `rfd::FileDialog` that Quill uses.
#[derive(Clone, Debug, Default)]
pub struct FileDialog {
    title: String,
    /// Each filter's name and its extensions, without dots. "*" means any.
    filters: Vec<(String, Vec<String>)>,
    directory: Option<PathBuf>,
    file_name: Option<String>,
}

impl FileDialog {
    pub fn new() -> FileDialog {
        FileDialog::default()
    }

    pub fn set_title(mut self, title: impl Into<String>) -> FileDialog {
        self.title = title.into();
        self
    }

    pub fn add_filter(mut self, name: impl Into<String>, extensions: &[&str]) -> FileDialog {
        self.filters.push((
            name.into(),
            extensions.iter().map(|e| (*e).to_string()).collect(),
        ));
        self
    }

    pub fn set_directory(mut self, directory: impl AsRef<Path>) -> FileDialog {
        self.directory = Some(directory.as_ref().to_path_buf());
        self
    }

    pub fn set_file_name(mut self, name: impl Into<String>) -> FileDialog {
        self.file_name = Some(name.into());
        self
    }

    pub fn pick_file(self) -> Option<PathBuf> {
        #[cfg(target_os = "linux")]
        {
            gtk_dialogs::choose(&self, gtk_dialogs::Kind::Open, false)
                .into_iter()
                .next()
        }
        #[cfg(not(target_os = "linux"))]
        {
            native(move || self.rfd().pick_file())
        }
    }

    pub fn pick_files(self) -> Option<Vec<PathBuf>> {
        #[cfg(target_os = "linux")]
        {
            let files = gtk_dialogs::choose(&self, gtk_dialogs::Kind::Open, true);
            (!files.is_empty()).then_some(files)
        }
        #[cfg(not(target_os = "linux"))]
        {
            native(move || self.rfd().pick_files())
        }
    }

    pub fn save_file(self) -> Option<PathBuf> {
        #[cfg(target_os = "linux")]
        {
            gtk_dialogs::choose(&self, gtk_dialogs::Kind::Save, false)
                .into_iter()
                .next()
        }
        #[cfg(not(target_os = "linux"))]
        {
            native(move || self.rfd().save_file())
        }
    }

    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn pick_folder(self) -> Option<PathBuf> {
        #[cfg(target_os = "linux")]
        {
            gtk_dialogs::choose(&self, gtk_dialogs::Kind::Folder, false)
                .into_iter()
                .next()
        }
        #[cfg(not(target_os = "linux"))]
        {
            native(move || self.rfd().pick_folder())
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn rfd(&self) -> rfd::FileDialog {
        let mut d = rfd::FileDialog::new().set_title(&self.title);
        for (name, extensions) in &self.filters {
            d = d.add_filter(name, extensions);
        }
        if let Some(directory) = &self.directory {
            d = d.set_directory(directory);
        }
        if let Some(name) = &self.file_name {
            d = d.set_file_name(name);
        }
        d
    }
}

/// Run a native dialog. On Windows that is on a thread of its own while this
/// one waits: a dialog run on the main thread, inside winit's event handler,
/// worked the first time and then hung (no window, one core at full speed) on
/// the next one. The Mac needs its dialogs on the main thread.
#[cfg(not(target_os = "linux"))]
fn native<T: Send>(show: impl FnOnce() -> T + Send) -> T {
    #[cfg(windows)]
    {
        crate::ev!("file dialog: showing on its own thread");
        std::thread::scope(|scope| {
            scope
                .spawn(show)
                .join()
                .unwrap_or_else(|_| panic!("the file dialog thread panicked"))
        })
    }
    #[cfg(not(windows))]
    {
        show()
    }
}

/// Show a message box and wait for the answer, as `rfd::MessageDialog` does.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn message(
    level: rfd::MessageLevel,
    title: &str,
    text: &str,
    buttons: rfd::MessageButtons,
) -> rfd::MessageDialogResult {
    #[cfg(target_os = "linux")]
    {
        gtk_dialogs::message(level, title, text, buttons)
    }
    #[cfg(windows)]
    {
        win_message::show(level, title, text, buttons)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        rfd::MessageDialog::new()
            .set_level(level)
            .set_title(title)
            .set_description(text)
            .set_buttons(buttons)
            .show()
    }
}

/// Windows message boxes. rfd's build here has no custom button text and no
/// owner window, so the box could open behind Quill's window while Quill sat
/// waiting for it. This keeps the button labels.
#[cfg(windows)]
mod win_message {
    use rfd::{MessageButtons as B, MessageDialogResult as R, MessageLevel};
    use windows::Win32::UI::Controls::{
        TASKDIALOG_BUTTON, TASKDIALOGCONFIG, TD_ERROR_ICON, TD_INFORMATION_ICON, TD_WARNING_ICON,
        TDF_ALLOW_DIALOG_CANCELLATION, TaskDialogIndirect,
    };
    use windows::core::{HSTRING, PCWSTR};

    /// Show the box on a thread of its own while this one waits. Run on this
    /// thread, inside winit's event handler, the box never appeared and the
    /// message loop spun at full speed. The box has no owner window, because
    /// an owner on a waiting thread cannot answer the dialog's messages.
    pub fn show(level: MessageLevel, title: &str, text: &str, buttons: B) -> R {
        let (title, text) = (title.to_string(), text.to_string());
        std::thread::scope(|scope| {
            scope
                .spawn(move || run(level, &title, &text, buttons))
                .join()
                .unwrap_or(R::Cancel)
        })
    }

    fn run(level: MessageLevel, title: &str, text: &str, buttons: B) -> R {
        crate::ev!("Windows message box: showing on its own thread");
        // Button labels left to right with what each one answers.
        let choices: Vec<(String, R)> = match buttons {
            B::Ok => vec![("OK".into(), R::Ok)],
            B::OkCancel => vec![("OK".into(), R::Ok), ("Cancel".into(), R::Cancel)],
            B::YesNo => vec![("Yes".into(), R::Yes), ("No".into(), R::No)],
            B::YesNoCancel => vec![
                ("Yes".into(), R::Yes),
                ("No".into(), R::No),
                ("Cancel".into(), R::Cancel),
            ],
            B::OkCustom(ok) => vec![(ok.clone(), R::Custom(ok))],
            B::OkCancelCustom(ok, cancel) => vec![
                (ok.clone(), R::Custom(ok)),
                (cancel.clone(), R::Custom(cancel)),
            ],
            B::YesNoCancelCustom(yes, no, cancel) => vec![
                (yes.clone(), R::Custom(yes)),
                (no.clone(), R::Custom(no)),
                (cancel.clone(), R::Custom(cancel)),
            ],
        };
        let labels: Vec<HSTRING> = choices
            .iter()
            .map(|(l, _)| HSTRING::from(l.as_str()))
            .collect();
        // Ids start at 100 so they never collide with the stock IDOK/IDCANCEL.
        let list: Vec<TASKDIALOG_BUTTON> = labels
            .iter()
            .enumerate()
            .map(|(i, l)| TASKDIALOG_BUTTON {
                nButtonID: 100 + i as i32,
                pszButtonText: PCWSTR(l.as_ptr()),
            })
            .collect();
        let title = HSTRING::from(title);
        let text = HSTRING::from(text);
        let mut config: TASKDIALOGCONFIG = unsafe { std::mem::zeroed() };
        config.cbSize = std::mem::size_of::<TASKDIALOGCONFIG>() as u32;
        config.dwFlags = TDF_ALLOW_DIALOG_CANCELLATION;
        config.pszWindowTitle = PCWSTR(title.as_ptr());
        config.pszContent = PCWSTR(text.as_ptr());
        config.Anonymous1.pszMainIcon = match level {
            MessageLevel::Info => TD_INFORMATION_ICON,
            MessageLevel::Warning => TD_WARNING_ICON,
            MessageLevel::Error => TD_ERROR_ICON,
        };
        config.cButtons = list.len() as u32;
        config.pButtons = list.as_ptr();
        let mut chosen = 0i32;
        let done = unsafe { TaskDialogIndirect(&config, Some(&mut chosen), None, None) };
        crate::ev!("Windows message box returned {done:?}, button {chosen}");
        if done.is_err() {
            return R::Cancel;
        }
        match usize::try_from(chosen - 100)
            .ok()
            .and_then(|i| choices.get(i))
        {
            Some((_, answer)) => answer.clone(),
            // Closed with the X or Escape: the same as Cancel.
            None => R::Cancel,
        }
    }
}

#[cfg(target_os = "linux")]
mod gtk_dialogs {
    use super::FileDialog;
    use gtk::prelude::*;
    use rfd::{MessageButtons as B, MessageDialogResult as R, MessageLevel};
    use std::path::PathBuf;

    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum Kind {
        Open,
        Save,
        Folder,
    }

    /// Run a GTK file chooser on this (the main) thread. Empty when canceled.
    pub fn choose(d: &FileDialog, kind: Kind, multiple: bool) -> Vec<PathBuf> {
        let (action, accept) = match kind {
            Kind::Open => (gtk::FileChooserAction::Open, "_Open"),
            Kind::Save => (gtk::FileChooserAction::Save, "_Save"),
            Kind::Folder => (gtk::FileChooserAction::SelectFolder, "_Select"),
        };
        let dialog = gtk::FileChooserDialog::new(Some(&d.title), None::<&gtk::Window>, action);
        dialog.add_button("_Cancel", gtk::ResponseType::Cancel);
        dialog.add_button(accept, gtk::ResponseType::Accept);
        dialog.set_default_response(gtk::ResponseType::Accept);
        dialog.set_select_multiple(multiple);
        if kind == Kind::Save {
            dialog.set_do_overwrite_confirmation(true);
        }
        // Each filter with the extension it stands for, for naming a saved file.
        let mut filters: Vec<(gtk::FileFilter, Option<String>)> = Vec::new();
        for (name, extensions) in &d.filters {
            let filter = gtk::FileFilter::new();
            filter.set_name(Some(name));
            for e in extensions {
                if e == "*" {
                    filter.add_pattern("*");
                } else {
                    filter.add_pattern(&format!("*.{}", e.to_lowercase()));
                    filter.add_pattern(&format!("*.{}", e.to_uppercase()));
                }
            }
            dialog.add_filter(filter.clone());
            let first = extensions
                .iter()
                .find(|e| *e != "*")
                .map(|e| e.to_lowercase());
            filters.push((filter, first));
        }
        if let Some((first, _)) = filters.first() {
            dialog.set_filter(first);
        }
        // In Save As the filter is the file type: choosing one changes the
        // name's extension to match, as the Mac's save panel does.
        if kind == Kind::Save {
            let filters = filters.clone();
            dialog.connect_notify_local(Some("filter"), move |dialog, _| {
                let Some(current) = dialog.filter() else {
                    return;
                };
                let Some((_, Some(ext))) = filters.iter().find(|(f, _)| *f == current) else {
                    return;
                };
                if let Some(name) = dialog.current_name() {
                    let name = name.to_string();
                    let stem = match name.rsplit_once('.') {
                        Some((stem, _)) if !stem.is_empty() => stem,
                        _ => name.as_str(),
                    };
                    dialog.set_current_name(&format!("{stem}.{ext}"));
                }
            });
        }
        if let Some(directory) = &d.directory {
            let _ = dialog.set_current_folder(directory);
        }
        if let Some(name) = &d.file_name {
            dialog.set_current_name(name);
        }
        let answer = dialog.run();
        let mut files = if answer == gtk::ResponseType::Accept {
            dialog.filenames()
        } else {
            Vec::new()
        };
        // A name typed with no extension takes the chosen file type's.
        if kind == Kind::Save
            && let Some(current) = dialog.filter()
            && let Some((_, Some(ext))) = filters.iter().find(|(f, _)| *f == current)
        {
            for path in &mut files {
                if path.extension().is_none() {
                    path.set_extension(ext);
                }
            }
        }
        dismiss(&dialog);
        files
    }

    /// Take a finished dialog off the screen for good. Closing a window only
    /// asks GTK to; until the main loop runs it stays up and keeps its grab
    /// on the keyboard and mouse, so the document window would get no input
    /// while Quill carries on with the answer. Destroying it releases the
    /// grab, and GTK's pending events are run so the window really goes.
    fn dismiss(dialog: &(impl IsA<gtk::Window> + IsA<gtk::Widget>)) {
        dialog.hide();
        // SAFETY: nothing uses the dialog after this; it is dropped by the
        // caller straight away.
        unsafe { dialog.destroy() };
        let mut handled = 0;
        for _ in 0..200 {
            if !gtk::events_pending() {
                break;
            }
            gtk::main_iteration_do(false);
            handled += 1;
        }
        crate::ev!("GTK dialog destroyed, {handled} pending events run");
    }

    /// Run a GTK message box on this thread and say which button was used.
    pub fn message(level: MessageLevel, title: &str, text: &str, buttons: B) -> R {
        let kind = match level {
            MessageLevel::Info => gtk::MessageType::Info,
            MessageLevel::Warning => gtk::MessageType::Warning,
            MessageLevel::Error => gtk::MessageType::Error,
        };
        // Buttons left to right as GTK wants them (the default last), each with
        // what rfd would answer for it.
        let choices: Vec<(String, R)> = match buttons {
            B::Ok => vec![("_OK".into(), R::Ok)],
            B::OkCancel => vec![("_Cancel".into(), R::Cancel), ("_OK".into(), R::Ok)],
            B::YesNo => vec![("_No".into(), R::No), ("_Yes".into(), R::Yes)],
            B::YesNoCancel => vec![
                ("_Cancel".into(), R::Cancel),
                ("_No".into(), R::No),
                ("_Yes".into(), R::Yes),
            ],
            B::OkCustom(ok) => vec![(ok.clone(), R::Custom(ok))],
            B::OkCancelCustom(ok, cancel) => {
                vec![
                    (cancel.clone(), R::Custom(cancel)),
                    (ok.clone(), R::Custom(ok)),
                ]
            }
            B::YesNoCancelCustom(yes, no, cancel) => vec![
                (cancel.clone(), R::Custom(cancel)),
                (no.clone(), R::Custom(no)),
                (yes.clone(), R::Custom(yes)),
            ],
        };
        let dialog = gtk::MessageDialog::new(
            None::<&gtk::Window>,
            gtk::DialogFlags::MODAL,
            kind,
            gtk::ButtonsType::None,
            text,
        );
        dialog.set_title(title);
        for (i, (label, _)) in choices.iter().enumerate() {
            dialog.add_button(label, gtk::ResponseType::Other(i as u16));
        }
        dialog.set_default_response(gtk::ResponseType::Other(choices.len() as u16 - 1));
        let answer = dialog.run();
        dismiss(&dialog);
        match answer {
            gtk::ResponseType::Other(i) => choices
                .into_iter()
                .nth(i as usize)
                .map_or(R::Cancel, |(_, result)| result),
            _ if choices.len() == 1 => choices.into_iter().next().map_or(R::Cancel, |(_, r)| r),
            _ => R::Cancel,
        }
    }
}
