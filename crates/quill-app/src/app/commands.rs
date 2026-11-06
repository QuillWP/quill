//! Native menu construction and command dispatch for the application shell.

use super::*;

impl App {
    /// The menus as they should look now, for the window in front.
    #[cfg(target_os = "macos")]
    fn current_menus(&self) -> (Vec<Menu>, Vec<Action>) {
        let front = self
            .front
            .and_then(|id| self.index(id))
            .or(if self.windows.is_empty() {
                None
            } else {
                Some(self.windows.len() - 1)
            });
        let list = match front {
            Some(i) => self.window_list(self.windows[i].0),
            None => Vec::new(),
        };
        let (view, formatting, paragraph, ruler, full_screen) = match front {
            Some(i) => {
                let st = &self.windows[i].1.st;
                (
                    &st.view,
                    st.show_formatting_toolbar,
                    st.show_paragraph_toolbar,
                    st.show_ruler,
                    st.full_screen,
                )
            }
            None => (&self.blank, true, true, true, false),
        };
        let cx = MenuContext {
            view,
            font_families: &self.font_families,
            show_formatting_toolbar: formatting,
            show_paragraph_toolbar: paragraph,
            show_ruler: ruler,
            full_screen,
            recent: &self.recent,
            windows: &list,
        };
        app_menus::build(&cx)
    }

    /// Bring the system menu bar up to date.
    pub(super) fn sync_menus(&mut self) {
        if let Some(i) = self.front.and_then(|id| self.index(id)) {
            self.take_doc(i);
        }
        // Runs after anything that opens, saves or closes a document.
        self.note_session();
        #[cfg(target_os = "macos")]
        {
            self.ensure_fonts();
            if let Some(preferences) = &mut self.preferences {
                preferences.sync();
                if preferences.is_closed() {
                    self.preferences = None;
                }
            }
            let (menus, actions) = self.current_menus();
            match &mut self.native {
                Some(n) => n.sync(&menus, actions),
                None => {
                    let proxy = self.proxy.clone();
                    self.native =
                        crate::native_menu::NativeMenu::install(&menus, actions, move |id| {
                            let _ = proxy.send_event(UserEvent::Menu(id.0));
                        });
                }
            }
        }
    }

    /// A choice from the system menu bar.
    #[cfg(target_os = "macos")]
    pub(super) fn menu_choice(&mut self, el: &ActiveEventLoop, id: &str) {
        #[cfg(target_os = "macos")]
        let action = {
            match crate::native_menu::context_index(id) {
                Some(i) => self.context_actions.get(i).cloned(),
                None => {
                    let index = crate::native_menu::action_index(&muda::MenuId::new(id));
                    index.and_then(|i| self.native.as_ref().and_then(|n| n.actions.get(i).cloned()))
                }
            }
        };
        #[cfg(not(target_os = "macos"))]
        let action: Option<Action> = {
            let _ = id;
            None
        };
        let Some(action) = action else {
            return;
        };
        // While a text field in a dialog is being edited (Find, Replace and
        // the like), Cut, Copy, Paste, Select All and Undo are the field's.
        #[cfg(target_os = "macos")]
        if let Action::Cmd(cmd) = &action
            && crate::mac_dialog::edit_in_field(*cmd)
        {
            return;
        }
        let front = self
            .front
            .filter(|id| self.index(*id).is_some())
            .or(self.windows.last().map(|w| w.0));
        if front
            .and_then(|id| self.index(id))
            .is_some_and(|i| self.windows[i].1.st.modal_dialog_open())
        {
            return;
        }
        if self.preferences.as_ref().is_some_and(|p| p.is_modal()) {
            if let Some(preferences) = &self.preferences {
                preferences.activate();
            }
            return;
        }
        match (action, front) {
            (Action::Cmd(Cmd::Preferences), front)
                if front.is_none() || self.preferences.is_some() =>
            {
                if let Some(preferences) = &self.preferences {
                    preferences.activate();
                } else {
                    self.ensure_fonts();
                    self.preferences = Some(preferences::ApplicationPreferences::new(
                        &self.font_names,
                        &self.font_families,
                    ));
                }
            }
            (Action::About, _) => about(),
            (Action::Licenses, _) => licenses(),
            (Action::Recent(index), _) => {
                if let Some(path) = self.recent.get(index).cloned() {
                    self.open_files(el, &[path]);
                }
            }
            (Action::RemoveRecent(index), _) => {
                crate::recent::remove(&mut self.recent, index);
                self.sync_menus();
            }
            (Action::ClearRecent, _) => {
                crate::recent::clear(&mut self.recent);
                self.sync_menus();
            }
            (Action::Window(n), _) => {
                if let Some((_, w)) = self.windows.get(n) {
                    w.window.focus_window();
                }
            }
            (Action::Cmd(Cmd::New), None) => {
                self.new_window(el, crate::settings::get().new_document(), None)
            }
            (Action::Cmd(Cmd::Open), None) => self.open_dialog(el, None),
            (Action::Cmd(Cmd::Quit), None) => {
                crate::settings::update(|s| {
                    s.last_session.clear();
                    s.quit_cleanly = true;
                });
                el.exit()
            }
            (_, None) => {}
            (action, Some(id)) => {
                let Some(i) = self.index(id) else {
                    return;
                };
                self.take_doc(i);
                self.redraw_group(i);
                let clip = self.clipboard.get_or_insert_with(Clipboard::new);
                let w = &mut self.windows[i].1;
                let action = match action {
                    Action::Cmd(Cmd::Sort) if Mods::from_winit(w.mods).shift => {
                        Action::Cmd(Cmd::SortDescending)
                    }
                    a => a,
                };
                let effect = w.st.act(action, clip);
                if let Some(fonts) = self.fonts.as_mut() {
                    w.st.view.update_layout(fonts);
                }
                w.st.view.scroll_to_caret();
                w.window.request_redraw();
                self.handle_effect(el, id, effect);
            }
        }
    }
}
