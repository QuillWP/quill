//! Application settings remain available without any document windows.

use super::*;

pub(super) struct ApplicationPreferences {
    state: WinState,
    native: Option<crate::mac_dialog::NativeDialog>,
}

impl ApplicationPreferences {
    pub(super) fn new(font_names: &[String], font_families: &[FontFamilyInfo]) -> Self {
        let mut state = WinState::new(DocView::new(Document::new(), String::new()));
        // Colors' boxes edit with the system Colors panel, as in a window.
        state.on_screen = true;
        state.open_dialog(
            DialogId::Preferences,
            &Env {
                font_names,
                font_families,
                recent: &[],
                windows: &[],
            },
        );
        Self {
            state,
            native: None,
        }
    }

    pub(super) fn is_closed(&self) -> bool {
        self.state.dialogs.is_empty()
    }

    pub(super) fn is_modal(&self) -> bool {
        self.state.modal_dialog_open()
    }

    pub(super) fn activate(&self) {
        if let Some(native) = &self.native {
            native.activate();
        }
    }

    pub(super) fn owns(&self, panel: usize) -> bool {
        self.native.as_ref().is_some_and(|d| d.panel_id() == panel)
    }

    pub(super) fn sync(&mut self) {
        let top = self.state.dialogs.last().map(|(_, d)| d);
        let depth = self.state.dialogs.len();
        if !matches!((&self.native, top), (Some(n), Some(d)) if n.matches(depth, d)) {
            // Changing the Show panel rebuilds the window; keep it where
            // the person moved it.
            let top_left = match (&self.native, top) {
                (Some(native), Some(dialog)) => native.top_left_for(dialog),
                _ => None,
            };
            self.native = None;
            if let Some(dialog) = top {
                self.native =
                    crate::mac_dialog::NativeDialog::show_standalone(depth, dialog, top_left);
            }
        }
        if let (Some(native), Some(dialog)) = (&mut self.native, top) {
            native.sync_from_model(dialog);
        }
        // A press outside the color pop-out put it away: forget it.
        if let Some(native) = &mut self.native
            && native.popover_dismissed()
        {
            if let Some((_, d)) = self.state.dialogs.last_mut() {
                d.popout = None;
            }
            self.state.color_pop = None;
        }
    }

    pub(super) fn action(
        &mut self,
        id: &'static str,
        choice: Option<(usize, bool, bool)>,
        clip: &mut Clipboard,
    ) {
        let (Some(native), Some((_, dialog))) = (&self.native, self.state.dialogs.last_mut())
        else {
            return;
        };
        dialog.inactive = false;
        native.apply_to_model(dialog, id, choice);
        let outcome = crate::mac_dialog::NativeDialog::outcome(dialog, id);
        self.state.dialog_outcome(outcome, clip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_without_a_document_reuse_the_normal_settings_and_close_path() {
        let mut preferences = ApplicationPreferences::new(&[], &[]);
        assert!(!preferences.is_closed());
        assert!(!preferences.is_modal());
        assert!(matches!(
            preferences.state.dialogs.last(),
            Some((Which::Preferences { panel: 0, .. }, _))
        ));
        assert!(!preferences.state.view.doc.is_modified());
        assert!(preferences.state.view.path.is_none());
        let mut clip = Clipboard::without_system();
        preferences.state.dialog_outcome(Outcome::Cancel, &mut clip);
        assert!(preferences.is_closed());
        assert!(!preferences.state.view.doc.is_modified());
    }

    #[test]
    fn canceling_preferences_discards_the_draft() {
        let before = crate::settings::get().foreign_as_quill;
        let mut preferences = ApplicationPreferences::new(&[], &[]);
        let mut clip = Clipboard::without_system();
        preferences
            .state
            .dialogs
            .last_mut()
            .unwrap()
            .1
            .set_selected("panel", 2);
        preferences
            .state
            .dialog_outcome(Outcome::Changed("panel"), &mut clip);
        assert!(matches!(
            preferences.state.dialogs.last(),
            Some((Which::Preferences { panel: 2, .. }, _))
        ));
        preferences
            .state
            .dialogs
            .last_mut()
            .unwrap()
            .1
            .set_on("foreign_quill", !before);
        preferences
            .state
            .dialog_outcome(Outcome::Changed("foreign_quill"), &mut clip);
        assert_eq!(crate::settings::get().foreign_as_quill, before);
        preferences.state.dialog_outcome(Outcome::Cancel, &mut clip);
        assert!(preferences.is_closed());
        assert_eq!(crate::settings::get().foreign_as_quill, before);
        assert!(!preferences.state.view.doc.is_modified());
        crate::settings::update(|s| s.foreign_as_quill = before);
    }
    fn change(
        preferences: &mut ApplicationPreferences,
        id: &'static str,
        value: &str,
        clip: &mut Clipboard,
    ) {
        let d = &mut preferences.state.dialogs.last_mut().unwrap().1;
        if id == "panel" {
            d.set_selected(id, value.parse().unwrap());
        } else {
            d.set_field(id, value);
        }
        preferences.state.dialog_outcome(Outcome::Changed(id), clip);
    }

    #[test]
    fn save_and_close_commits_edits_across_panels_and_preserves_other_settings() {
        let before = crate::settings::get();
        let mut p = ApplicationPreferences::new(&[], &[]);
        let mut clip = Clipboard::without_system();
        change(&mut p, "name", "Draft Name", &mut clip);
        change(&mut p, "panel", "3", &mut clip);
        change(&mut p, "size", "27", &mut clip);
        change(&mut p, "panel", "0", &mut clip);
        assert_eq!(
            p.state.dialogs.last().unwrap().1.field_text("name"),
            "Draft Name"
        );
        assert_eq!(crate::settings::get().name, before.name);
        assert_eq!(crate::settings::get().size, before.size);
        // Changes made outside Preferences while it is open must survive its save.
        crate::settings::update(|s| s.recent_count = 37);
        p.state.dialog_outcome(Outcome::Ok, &mut clip);
        assert!(p.is_closed());
        let after = crate::settings::get();
        assert_eq!(after.name, "Draft Name");
        assert_eq!(after.size, 54);
        assert_eq!(after.recent_count, 37);
        crate::settings::update(|s| *s = before);
    }

    #[test]
    fn save_keeps_the_window_open_and_cancel_discards_only_later_edits() {
        let before = crate::settings::get();
        let mut p = ApplicationPreferences::new(&[], &[]);
        let mut clip = Clipboard::without_system();
        change(&mut p, "name", "Saved Name", &mut clip);
        p.state.dialog_outcome(Outcome::Button("save"), &mut clip);
        assert!(!p.is_closed());
        assert_eq!(crate::settings::get().name, "Saved Name");
        change(&mut p, "name", "Discard Me", &mut clip);
        change(&mut p, "panel", "3", &mut clip);
        change(&mut p, "size", "31", &mut clip);
        p.state.dialog_outcome(Outcome::Cancel, &mut clip);
        assert!(p.is_closed());
        assert_eq!(crate::settings::get().name, "Saved Name");
        assert_eq!(crate::settings::get().size, before.size);
        crate::settings::update(|s| *s = before);
    }
}
