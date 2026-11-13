//! Keys, text, the mouse and the cursor, as the app receives them.

use super::*;

impl App {
    pub(super) fn key(
        &mut self,
        el: &ActiveEventLoop,
        id: WindowId,
        event: winit::event::KeyEvent,
    ) {
        if event.state != ElementState::Pressed {
            return;
        }
        let clip = self.clipboard.get_or_insert_with(Clipboard::new);
        let Some(i) = self.windows.iter().position(|(w, _)| *w == id) else {
            return;
        };
        let w = &mut self.windows[i].1;
        if w.st.zoom_edit.is_some() {
            let mods = Mods::from_winit(w.mods);
            if let Key::Character(c) = &event.logical_key
                && mods.cmd
            {
                if c.eq_ignore_ascii_case("v") {
                    if let Some(text) = clip.plain_text() {
                        w.st.zoom_text(&text);
                    }
                    w.window.request_redraw();
                    return;
                }
                if c.eq_ignore_ascii_case("c") || c.eq_ignore_ascii_case("x") {
                    if let Some(text) = w.st.zoom_selection() {
                        clip.put_text(&text);
                        if c.eq_ignore_ascii_case("x") {
                            w.st.zoom_key(DKey::Backspace, None);
                        }
                    }
                    w.window.request_redraw();
                    return;
                }
            }
            let key = match &event.logical_key {
                Key::Named(NamedKey::Enter) => DKey::Return,
                Key::Named(NamedKey::Escape) => DKey::Escape,
                Key::Named(NamedKey::Tab) => DKey::Tab { back: mods.shift },
                Key::Named(NamedKey::Backspace) => DKey::Backspace,
                Key::Named(NamedKey::Delete) => DKey::Delete,
                Key::Named(NamedKey::ArrowLeft) => DKey::Left,
                Key::Named(NamedKey::ArrowRight) => DKey::Right,
                Key::Character(c) if mods.cmd && c.eq_ignore_ascii_case("a") => DKey::SelectAll,
                _ => DKey::Char,
            };
            w.st.zoom_key(
                key,
                if mods.cmd {
                    None
                } else {
                    event.text.as_deref()
                },
            );
            w.window.request_redraw();
            return;
        }
        if w.st.dialog_has_keys() {
            let mods = Mods::from_winit(w.mods);
            let key = match &event.logical_key {
                Key::Named(NamedKey::Escape) => Some(DKey::Escape),
                Key::Character(c) if mods.cmd && c.as_str() == "." => Some(DKey::Escape),
                Key::Character(c) if mods.cmd && c.eq_ignore_ascii_case("a") => {
                    Some(DKey::SelectAll)
                }
                _ if mods.cmd => None,
                Key::Named(NamedKey::Enter) => Some(DKey::Return),
                Key::Named(NamedKey::Tab) => Some(DKey::Tab { back: mods.shift }),
                Key::Named(NamedKey::Backspace) => Some(DKey::Backspace),
                Key::Named(NamedKey::Delete) => Some(DKey::Delete),
                Key::Named(NamedKey::ArrowLeft) => Some(DKey::Left),
                Key::Named(NamedKey::ArrowRight) => Some(DKey::Right),
                _ if event.text.is_some() => Some(DKey::Char),
                _ => None,
            };
            if let Some(k) = key {
                w.st.dialog_key(k, event.text.as_deref(), clip);
                w.window.request_redraw();
                return;
            }
            // Menu keys still work while a modeless window has the
            // keyboard.
            if !(mods.cmd && w.st.front_modeless()) {
                return;
            }
        }
        if w.st.tool_pal.is_some() {
            if event.logical_key == Key::Named(NamedKey::Escape) {
                w.st.tool_pal = None;
            }
            w.window.request_redraw();
            return;
        }
        if w.st.menu.is_some() {
            if event.logical_key == Key::Named(NamedKey::Escape) {
                w.st.menu = None;
            } else {
                let fonts = self.fonts.as_mut().unwrap();
                let mut label = Label {
                    fonts,
                    cache: &mut self.cache,
                };
                let text = event.text.as_deref();
                let effect = w.st.menu_key(&event.logical_key, text, &mut label, clip);
                w.window.request_redraw();
                self.handle_effect(el, id, effect);
                return;
            }
            w.window.request_redraw();
            return;
        }
        let mods = Mods::from_winit(w.mods);
        let code = match event.physical_key {
            PhysicalKey::Code(c) => Some(c),
            PhysicalKey::Unidentified(_) => None,
        };
        if code == Some(KeyCode::KeyR) && mods.cmd && !mods.shift {
            // ⌘R toggles the ruler, ⌘⌥R the formatting toolbar.
            if mods.opt {
                w.st.show_formatting_toolbar = !w.st.show_formatting_toolbar;
            } else {
                w.st.show_ruler = !w.st.show_ruler;
            }
            w.window.request_redraw();
            return;
        }
        if code == Some(KeyCode::KeyD) && mods.cmd && mods.ctrl && !mods.shift && !mods.opt {
            self.effect(el, id, Effect::LookUp);
            return;
        }
        if code == Some(KeyCode::KeyF) && mods.cmd && mods.ctrl && !mods.shift && !mods.opt {
            self.effect(el, id, Effect::FullScreen);
            return;
        }
        let cmd = if code == Some(KeyCode::NumpadEnter) && mods.shift && !mods.cmd {
            Some(Cmd::PageBreak)
        } else if code == Some(KeyCode::NumpadEnter) && mods.cmd && !mods.shift {
            Some(Cmd::SectionBreak)
        } else {
            command_map::lookup(&event.logical_key, code.and_then(keycode_char), mods)
        };
        crate::ev!(
            "key {:?} with cmd={} shift={} opt={} ctrl={} -> command {:?}",
            event.logical_key,
            mods.cmd,
            mods.shift,
            mods.opt,
            mods.ctrl,
            cmd
        );
        // After ⌘⌥T (Outline Command Prefix) the next key is an outline
        // command.
        let modifier = matches!(
            event.logical_key,
            Key::Named(
                NamedKey::Shift
                    | NamedKey::Control
                    | NamedKey::Alt
                    | NamedKey::Super
                    | NamedKey::Meta
            )
        );
        let cmd = if w.st.view.outline_prefix && cmd != Some(Cmd::OutlinePrefix) && !modifier {
            w.st.view.outline_prefix = false;
            let mapped = match &event.logical_key {
                Key::Named(NamedKey::ArrowLeft) => Some(Cmd::PromoteHeading),
                Key::Named(NamedKey::ArrowRight) => Some(Cmd::DemoteHeading),
                Key::Named(NamedKey::ArrowUp) => Some(Cmd::MoveHeadingUp),
                Key::Named(NamedKey::ArrowDown) => Some(Cmd::MoveHeadingDown),
                Key::Character(c) => match c.as_str() {
                    "+" | "=" => Some(Cmd::ExpandSubtext),
                    "-" => Some(Cmd::CollapseSubtext),
                    "a" | "A" => Some(Cmd::ShowBodyText),
                    "b" | "B" => Some(Cmd::MakeBodyText),
                    d => d
                        .parse::<u8>()
                        .ok()
                        .filter(|n| (1..=9).contains(n))
                        .and_then(|n| {
                            command_map::ALL
                                .iter()
                                .find(|c| format!("{c:?}") == format!("ShowHeading{n}"))
                                .copied()
                        }),
                },
                _ => None,
            };
            if mapped.is_none() {
                return;
            }
            mapped
        } else {
            cmd
        };

        let effect = if let Some(cmd) = cmd {
            let extend = mods.shift && cmd.is_movement();
            w.st.view.exec(cmd, extend, clip)
        } else {
            if !mods.cmd
                && !mods.ctrl
                && let Some(text) = &event.text
            {
                w.st.view.type_text(text);
            }
            Effect::None
        };
        // A bare Shift, Cmd, Ctrl or Alt press changes nothing in the
        // document, so it must not pull the view back to the caret (it did
        // on the way to a Cmd-Shift-3 screenshot).
        let bare_modifier = cmd.is_none() && modifier;
        if !bare_modifier && !matches!(cmd, Some(Cmd::PageUp | Cmd::PageDown)) {
            if let Some(fonts) = self.fonts.as_mut() {
                w.st.view.update_layout(fonts);
            }
            w.st.view.scroll_to_caret();
        }
        w.window.request_redraw();
        self.handle_effect(el, id, effect);
    }

    /// Text from an input method (`typed`), which is typing like the
    /// keyboard's, or from the Mac's text services such as Emoji & Symbols,
    /// which goes in as its own step to undo.
    pub(super) fn insert_text(
        &mut self,
        el: &ActiveEventLoop,
        id: WindowId,
        text: &str,
        typed: bool,
    ) {
        let Some(i) = self.index(id) else {
            return;
        };
        let clip = self.clipboard.get_or_insert_with(Clipboard::new);
        let w = &mut self.windows[i].1;
        if w.st.zoom_edit.is_some() {
            w.st.zoom_text(text);
            w.window.request_redraw();
            return;
        }
        w.st.view.end_composing();
        if w.st.tool_pal.is_some() {
            return;
        }
        if w.st.dialog_has_keys() {
            w.st.dialog_key(DKey::Char, Some(text), clip);
            w.window.request_redraw();
            return;
        }
        if w.st.menu.is_some() {
            w.window.request_redraw();
            return;
        }
        if typed {
            w.st.view.type_text(text);
        } else {
            w.st.view.insert_text(text);
        }
        if let Some(fonts) = self.fonts.as_mut() {
            w.st.view.update_layout(fonts);
        }
        w.st.view.scroll_to_caret();
        w.window.request_redraw();
        self.handle_effect(el, id, Effect::None);
    }

    pub(super) fn mouse_button(
        &mut self,
        el: &ActiveEventLoop,
        id: WindowId,
        state: ElementState,
        context: bool,
    ) {
        crate::ev!("mouse button {state:?} (context menu: {context})");
        self.ensure_fonts();
        let list = self.window_list(id);
        let Some(i) = self.index(id) else {
            return;
        };
        let env = Env {
            font_names: &self.font_names,
            font_families: &self.font_families,
            recent: &self.recent,
            windows: &list,
        };
        let clip = self.clipboard.get_or_insert_with(Clipboard::new);
        let fonts = self.fonts.as_mut().unwrap();
        let w = &mut self.windows[i].1;
        let (x, y) = w.cursor;
        let mut label = Label {
            fonts,
            cache: &mut self.cache,
        };
        if cfg!(not(target_os = "macos"))
            && !context
            && state == ElementState::Pressed
            && w.st.in_grow_box(x, y)
        {
            let _ = w.window.drag_resize_window(ResizeDirection::SouthEast);
            return;
        }
        if context {
            // On the Mac the menu is the system's own.
            #[cfg(target_os = "macos")]
            if state == ElementState::Pressed {
                if let Some((m, actions)) = w.st.context_menu_at(x, y, clip) {
                    w.window.request_redraw();
                    self.context_actions = actions;
                    let w = &self.windows[i].1;
                    crate::native_menu::show_context(&m, &w.window, None);
                }
                return;
            }
            if state == ElementState::Pressed {
                w.st.open_context(x, y, &mut label, clip);
            }
            w.window.request_redraw();
            return;
        }
        let (effect, raise) = if state == ElementState::Pressed {
            let mods = Mods::from_winit(w.mods);
            (w.st.mouse_down(x, y, mods, &mut label, &env, clip), None)
        } else {
            w.st.mouse_up(x, y, clip)
        };
        w.window.request_redraw();
        // On the Mac every menu is the system's own, so a pop-up Quill
        // would draw opens as a native menu where it would have been.
        #[cfg(target_os = "macos")]
        if let Some((m, actions, at)) = w.st.take_popup_menu() {
            self.context_actions = actions;
            let w = &self.windows[i].1;
            crate::native_menu::show_context(&m, &w.window, Some(at));
        }
        if let Some(n) = raise
            && let Some((_, other)) = self.windows.get(n)
        {
            other.window.focus_window();
        }
        self.handle_effect(el, id, effect);
    }

    pub(super) fn cursor_moved(&mut self, id: WindowId, px: f64, py: f64) {
        self.ensure_fonts();
        let list = self.window_list(id);
        let Some(i) = self.index(id) else {
            return;
        };
        let env = Env {
            font_names: &self.font_names,
            font_families: &self.font_families,
            recent: &self.recent,
            windows: &list,
        };
        let fonts = self.fonts.as_mut().unwrap();
        let w = &mut self.windows[i].1;
        let scale = w.window.scale_factor();
        let (x, y) = ((px / scale) as f32, (py / scale) as f32);
        w.cursor = (x, y);
        let mut label = Label {
            fonts,
            cache: &mut self.cache,
        };
        if w.st.mouse_move(x, y, &mut label, &env) {
            w.window.request_redraw();
        }
        self.update_cursor(i);
    }

    /// Show the pointer for what is under it now. Holding Control over a
    /// picture's handle changes it without the mouse moving.
    pub(super) fn update_cursor(&mut self, i: usize) {
        let w = &mut self.windows[i].1;
        let (x, y) = w.cursor;
        w.st.view.crop_held = w.mods.control_key();
        let (dx, dy) = w.st.document_point(x, y);
        if w.st.view.picture_crop_cursor(dx, dy)
            && let Some(crop) = &self.crop_cursor
        {
            w.window.set_cursor(crop.clone());
        } else {
            w.window.set_cursor(w.st.cursor_icon(x, y));
        }
    }
}
