//! Tools > Review Changes: a modeless window that steps through the
//! document's tracked changes, says what each one is and who made it, and
//! accepts or rejects it.

use crate::dialog::Dialog;
use crate::docview::DocView;

pub fn dialog(view: &DocView) -> Dialog {
    let mut d = Dialog::new("Review Changes", 364.0, 120.0);
    d.text_as("what", "", 13.0, 10.0);
    d.text_as("who", "", 13.0, 30.0);
    d.button("previous", "Previous", 13.0, 58.0, 78.0, false);
    d.button("next", "Next", 99.0, 58.0, 78.0, true);
    d.button("accept", "Accept", 191.0, 58.0, 78.0, false);
    d.button("reject", "Reject", 277.0, 58.0, 78.0, false);
    d.button("cancel", "Close", 13.0, 88.0, 78.0, false);
    d.button("acceptall", "Accept All", 191.0, 88.0, 78.0, false);
    d.button("rejectall", "Reject All", 277.0, 88.0, 78.0, false);
    update(&mut d, view, None);
    d
}

/// Show the change at the selection, or `note` (such as "No more
/// changes") when there is none there.
pub fn update(d: &mut Dialog, view: &DocView, note: Option<&str>) {
    let any = view.doc.has_changes();
    let (what, who) = match view.describe_change_here() {
        Some(c) => c,
        None if !any => ("The document has no tracked changes.".into(), String::new()),
        None => (
            note.unwrap_or("Click Next to find a tracked change.")
                .to_string(),
            String::new(),
        ),
    };
    d.set_label("what", &what);
    d.set_label("who", &who);
    let here = view.change_here().is_some();
    d.enable("accept", here);
    d.enable("reject", here);
    for id in ["previous", "next", "acceptall", "rejectall"] {
        d.enable(id, any);
    }
}
