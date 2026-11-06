//! Message boxes as native alerts. Without a parent window rfd shows its
//! message boxes through CFUserNotification: another process draws the
//! alert while Quill's main thread waits for it, so a click on Quill put the
//! alert behind and left Quill spinning with nothing to answer. An NSAlert
//! is Quill's own window and runs in the same modal loop as the Save panel.

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2::{class, msg_send, msg_send_id};
use objc2_foundation::NSString;
use rfd::{MessageButtons, MessageDialogResult, MessageLevel};

/// What each button says, first (the default) to last, and what it means.
fn buttons(kind: &MessageButtons) -> Vec<(String, MessageDialogResult)> {
    use MessageDialogResult as R;
    let custom = |text: &String| (text.clone(), R::Custom(text.clone()));
    match kind {
        MessageButtons::Ok => vec![("OK".into(), R::Ok)],
        MessageButtons::OkCancel => vec![("OK".into(), R::Ok), ("Cancel".into(), R::Cancel)],
        MessageButtons::YesNo => vec![("Yes".into(), R::Yes), ("No".into(), R::No)],
        MessageButtons::YesNoCancel => vec![
            ("Yes".into(), R::Yes),
            ("No".into(), R::No),
            ("Cancel".into(), R::Cancel),
        ],
        MessageButtons::OkCustom(ok) => vec![custom(ok)],
        MessageButtons::OkCancelCustom(ok, cancel) => vec![custom(ok), custom(cancel)],
        MessageButtons::YesNoCancelCustom(yes, no, cancel) => {
            vec![custom(yes), custom(no), custom(cancel)]
        }
    }
}

/// The bold line and the smaller text under it. A box titled just "Quill"
/// has its first paragraph as the bold line.
fn headline<'a>(title: &'a str, text: &'a str) -> (&'a str, &'a str) {
    if title == "Quill" {
        text.split_once("\n\n").unwrap_or((text, ""))
    } else {
        (title, text)
    }
}

/// Shows the alert and waits for an answer. Runs inside winit's event
/// handler: clicks while it is up only wait, because of the patched winit
/// in vendor/winit (see vendor/README.md).
pub fn show(
    level: MessageLevel,
    title: &str,
    text: &str,
    kind: MessageButtons,
) -> MessageDialogResult {
    let mut choices = buttons(&kind);
    let (message, detail) = headline(title, text);
    let style: usize = match level {
        MessageLevel::Warning => 0,
        MessageLevel::Info => 1,
        MessageLevel::Error => 2,
    };
    // SAFETY: called on the app thread, as all of Quill's AppKit calls are.
    let answer: isize = autoreleasepool(|_| unsafe {
        let alert: Retained<AnyObject> = msg_send_id![class!(NSAlert), new];
        let _: () = msg_send![&*alert, setAlertStyle: style];
        let _: () = msg_send![&*alert, setMessageText: &*NSString::from_str(message)];
        let _: () = msg_send![&*alert, setInformativeText: &*NSString::from_str(detail)];
        for (label, _) in &choices {
            let label = NSString::from_str(label);
            let _: *mut AnyObject = msg_send![&*alert, addButtonWithTitle: &*label];
        }
        // A question at startup comes before any window of Quill's.
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, activateIgnoringOtherApps: true];
        msg_send![&*alert, runModal]
    });
    // NSAlertFirstButtonReturn is 1000, and the rest follow it.
    let index = usize::try_from(answer - 1000).unwrap_or(usize::MAX);
    if index < choices.len() {
        choices.swap_remove(index).1
    } else {
        MessageDialogResult::Cancel
    }
}

/// A note with an OK button and an unticked checkbox. True if the box was
/// ticked.
pub fn remind(message: &str, detail: &str, checkbox: &str) -> bool {
    // SAFETY: called on the app thread, as all of Quill's AppKit calls are.
    autoreleasepool(|_| unsafe {
        let alert: Retained<AnyObject> = msg_send_id![class!(NSAlert), new];
        let _: () = msg_send![&*alert, setAlertStyle: 1usize];
        let _: () = msg_send![&*alert, setMessageText: &*NSString::from_str(message)];
        let _: () = msg_send![&*alert, setInformativeText: &*NSString::from_str(detail)];
        let _: () = msg_send![&*alert, setShowsSuppressionButton: true];
        let suppress: *mut AnyObject = msg_send![&*alert, suppressionButton];
        let _: () = msg_send![suppress, setTitle: &*NSString::from_str(checkbox)];
        let _: isize = msg_send![&*alert, runModal];
        let state: isize = msg_send![suppress, state];
        state == 1
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_titled_quill_leads_with_its_first_paragraph() {
        assert_eq!(headline("Quill", "Save?\n\nDetail"), ("Save?", "Detail"));
        assert_eq!(headline("Quill", "Save?"), ("Save?", ""));
        assert_eq!(
            headline("Be Careful", "Save?\n\nA"),
            ("Be Careful", "Save?\n\nA")
        );
    }

    #[test]
    fn custom_buttons_answer_with_their_own_text() {
        let kind =
            MessageButtons::YesNoCancelCustom("Save".into(), "Don't".into(), "Cancel".into());
        let found = buttons(&kind);
        assert_eq!(found[0].0, "Save");
        assert_eq!(found[1].1, MessageDialogResult::Custom("Don't".into()));
    }
}
