//! Application settings remain available without any document windows.

use super::*;

pub(super) struct ApplicationPreferences {
    state: WinState,
    native: Option<crate::mac_dialog::NativeDialog>,
}

impl ApplicationPreferences {
    pub(super) fn new(font_names: &[String], font_families: &[FontFamilyInfo]) -> Self {
        let mut state = WinState::new(DocView::new(Document::new(), String::new()));
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
            self.native = None;
            if let Some(dialog) = top {
                self.native = crate::mac_dialog::NativeDialog::show_standalone(depth, dialog);
            }
        }
        if let (Some(native), Some(dialog)) = (&mut self.native, top) {
            native.sync_from_model(dialog);
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
    fn changing_save_preferences_without_a_document_updates_application_settings() {
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
        assert_eq!(crate::settings::get().foreign_as_quill, !before);
        preferences.state.dialog_outcome(Outcome::Cancel, &mut clip);
        assert!(preferences.is_closed());
        assert_eq!(crate::settings::get().foreign_as_quill, !before);
        assert!(!preferences.state.view.doc.is_modified());
        crate::settings::update(|s| s.foreign_as_quill = before);
    }
}
