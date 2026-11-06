//! macOS control callbacks translated into platform-neutral application state.

use super::*;

impl App {
    pub(super) fn native_scroll(&mut self, sender: usize, part: usize, value: f64) {
        let Some((i, pane)) = self
            .windows
            .iter()
            .enumerate()
            .find_map(|(i, (_, w))| w.scrollbars.as_ref()?.owns(sender).map(|p| (i, p)))
        else {
            return;
        };
        if self.windows[i].1.group != 0 {
            self.take_doc(i);
            self.redraw_group(i);
        }
        let w = &mut self.windows[i].1;
        if pane == 2 {
            let v = w.st.main_view_mut();
            v.scroll_x = native_scroll_value(
                v.scroll_x,
                v.max_scroll_x(),
                (v.area.w - 24.0).max(16.0),
                part,
                value as f32,
            );
            w.window.request_redraw();
            return;
        }
        if pane != 0 && w.st.pane.is_some() {
            if let Some(v) = w.st.pane_view_mut() {
                let max = v.max_scroll();
                let page = (v.area.h - 24.0).max(16.0);
                v.scroll_y = native_scroll_value(v.scroll_y, max, page, part, value as f32);
            }
            w.window.request_redraw();
            return;
        }
        let max = w.st.main_view().max_scroll();
        let page = (w.st.main_view().area.h - 24.0).max(16.0);
        let current = if pane == 0 {
            w.st.main_view().scroll_y
        } else {
            w.st.split.as_ref().map_or(0.0, |split| split.other_scroll)
        };
        let next = native_scroll_value(current, max, page, part, value as f32);
        if pane == 0 {
            w.st.main_view_mut().scroll_y = next;
        } else if let Some(split) = &mut w.st.split {
            split.other_scroll = next;
        }
        w.window.request_redraw();
    }

    pub(super) fn native_dialog(
        &mut self,
        panel: usize,
        id: &'static str,
        choice: Option<(usize, bool, bool)>,
    ) {
        let Some(i) = self.windows.iter().position(|(_, w)| {
            w.dialog
                .as_ref()
                .is_some_and(|dialog| dialog.panel_id() == panel)
        }) else {
            return;
        };
        let clip = self.clipboard.get_or_insert_with(Clipboard::new);
        let w = &mut self.windows[i].1;
        let Some(native) = w.dialog.as_ref() else {
            return;
        };
        let Some((which, dialog)) = w.st.dialogs.last_mut() else {
            return;
        };
        if matches!(id, "list" | "keys")
            && let Some((row, _, _)) = choice
            && let Which::Commands(state) = which
        {
            crate::commands_dialog::select_native(state, dialog, id, row);
            w.window.request_redraw();
            return;
        }
        native.apply_to_model(dialog, id, choice);
        let outcome = crate::mac_dialog::NativeDialog::outcome(dialog, id);
        w.st.dialog_outcome(outcome, clip);
        w.window.request_redraw();
    }

    pub(super) fn native_shortcut(&mut self, panel: usize, spec: String) {
        let Some((_, w)) = self.windows.iter_mut().find(|(_, w)| {
            w.dialog
                .as_ref()
                .is_some_and(|dialog| dialog.panel_id() == panel)
        }) else {
            return;
        };
        if let Some((Which::Commands(state), dialog)) = w.st.dialogs.last_mut()
            && state.capturing
        {
            crate::commands_dialog::captured(state, dialog, spec);
            w.window.request_redraw();
        }
    }

    pub(super) fn native_shortcut_cancel(&mut self, panel: usize) {
        let Some((_, w)) = self.windows.iter_mut().find(|(_, w)| {
            w.dialog
                .as_ref()
                .is_some_and(|dialog| dialog.panel_id() == panel)
        }) else {
            return;
        };
        if let Some((Which::Commands(state), dialog)) = w.st.dialogs.last_mut()
            && state.capturing
        {
            crate::commands_dialog::cancel_capture(state, dialog);
            w.window.request_redraw();
        }
    }
}
