//! Message boxes: errors, warnings and questions.

// These message boxes, and every rfd file dialog, run modally inside winit's
// event handler. On the Mac that only works because of the patched winit in
// vendor/winit: unpatched, a click while one is up crashes Quill. See
// vendor/README.md before upgrading winit.

/// Show a message box and wait for the answer.
pub(crate) fn message_box(
    level: rfd::MessageLevel,
    title: &str,
    text: impl AsRef<str>,
    buttons: rfd::MessageButtons,
) -> rfd::MessageDialogResult {
    #[cfg(target_os = "macos")]
    {
        crate::mac_alert::show(level, title, text.as_ref(), buttons)
    }
    #[cfg(not(target_os = "macos"))]
    {
        rfd::MessageDialog::new()
            .set_level(level)
            .set_title(title)
            .set_description(text.as_ref())
            .set_buttons(buttons)
            .show()
    }
}

/// Ask a question with `yes` and Cancel; true for `yes`.
pub(crate) fn ask(message: &str, detail: &str, yes: &str) -> bool {
    let answer = message_box(
        rfd::MessageLevel::Warning,
        "Quill",
        format!("{message}\n\n{detail}"),
        rfd::MessageButtons::OkCancelCustom(yes.into(), "Cancel".into()),
    );
    matches!(answer, rfd::MessageDialogResult::Custom(ref b) if b == yes)
        || matches!(answer, rfd::MessageDialogResult::Ok)
}

#[cfg_attr(test, allow(dead_code))]
/// Ask whether to include more than was selected: Some(true) for yes,
/// Some(false) for the selection only, None for Cancel.
pub(crate) fn ask_include(message: &str, detail: &str, yes: &str, no: &str) -> Option<bool> {
    let answer = message_box(
        rfd::MessageLevel::Info,
        "Quill",
        format!("{message}\n\n{detail}"),
        rfd::MessageButtons::YesNoCancelCustom(yes.into(), no.into(), "Cancel".into()),
    );
    match answer {
        rfd::MessageDialogResult::Yes => Some(true),
        rfd::MessageDialogResult::No => Some(false),
        rfd::MessageDialogResult::Custom(choice) if choice == yes => Some(true),
        rfd::MessageDialogResult::Custom(choice) if choice == no => Some(false),
        _ => None,
    }
}

pub(crate) fn alert(message: &str, detail: &str) {
    message_box(
        rfd::MessageLevel::Error,
        "Quill",
        format!("{message}\n\n{detail}"),
        rfd::MessageButtons::Ok,
    );
}

/// A warning the person should read but that does not stop anything.
pub(crate) fn warn(message: &str, detail: &str) {
    message_box(
        rfd::MessageLevel::Warning,
        "Quill",
        format!("{message}\n\n{detail}"),
        rfd::MessageButtons::Ok,
    );
}
