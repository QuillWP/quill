//! The Style Sheets palette in a window of its own, for when it is not
//! docked. It is an ordinary small window with the system's title bar, so it
//! can be dragged anywhere on the screen, outside the document window too.
//! What it shows and does belongs to the document window in front.

use super::*;
use crate::styles_palette::{self as sp, Palette};

pub(super) struct PaletteWindow {
    pub window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    /// The palette's own hover, scroll and chosen row. It is swapped into
    /// the front document window's state while it draws or takes a click, so
    /// the window and the docked palette share their code.
    palette: Palette,
    /// The pointer, in interface units.
    cursor: (f32, f32),
    /// The document window it last showed. Its chosen row and scroll
    /// belong to that document, so they start over when another comes in
    /// front.
    shown: Option<WindowId>,
    /// Since when no Quill window has had the keyboard, and whether the
    /// palette is hidden for it.
    #[cfg(target_os = "linux")]
    inactive_since: Option<Instant>,
    #[cfg(target_os = "linux")]
    hidden: bool,
}

impl App {
    /// Open or close the palette's window to match the settings: it is there
    /// while the palette shows undocked, and there is a document window.
    pub(super) fn sync_palette_window(&mut self, el: &ActiveEventLoop) {
        let want = crate::settings::with(|s| s.show_styles_palette && !s.styles_palette_docked)
            && !self.windows.is_empty();
        match (&self.palette_window, want) {
            (None, true) => self.open_palette_window(el),
            (Some(w), false) => {
                self.palette_at = w.window.outer_position().ok();
                self.palette_window = None;
            }
            _ => {}
        }
    }

    fn open_palette_window(&mut self, el: &ActiveEventLoop) {
        let scale = self
            .front_index()
            .map_or(1.0, |i| self.windows[i].1.window.scale_factor());
        let ui = ui_scale(scale as f32) as f64;
        let (w, h) = sp::WINDOW_SIZE;
        let mut attrs = Window::default_attributes()
            .with_title("Style Sheets")
            .with_inner_size(LogicalSize::new(w as f64 * ui, h as f64 * ui))
            .with_min_inner_size(LogicalSize::new(
                sp::WINDOW_MIN.0 as f64 * ui,
                sp::WINDOW_MIN.1 as f64 * ui,
            ));
        // Where it was last, or beside the front window's right edge.
        let at = self.palette_at.or_else(|| {
            let front = &self.windows[self.front_index()?].1.window;
            let pos = front.outer_position().ok()?;
            let size = front.outer_size();
            let width = (w as f64 * ui * scale) as i32;
            Some(winit::dpi::PhysicalPosition::new(
                pos.x + size.width as i32 - width - (24.0 * scale) as i32,
                pos.y + (96.0 * scale) as i32,
            ))
        });
        if let Some(at) = at {
            attrs = attrs.with_position(at);
        }
        #[cfg(not(target_os = "macos"))]
        {
            attrs = attrs
                .with_window_icon(window_icon())
                .with_window_level(winit::window::WindowLevel::AlwaysOnTop);
        }
        #[cfg(target_os = "linux")]
        let created = Window::create(el, attrs, self.proxy.clone());
        #[cfg(not(target_os = "linux"))]
        let created = el.create_window(attrs);
        let Ok(window) = created.map(Rc::new) else {
            return;
        };
        let context = softbuffer::Context::new(window.clone()).expect("display connection");
        let surface = softbuffer::Surface::new(&context, window.clone()).expect("window surface");
        #[cfg(target_os = "macos")]
        crate::mac_palette::style(&window);
        set_window_appearance(&window, crate::settings::with(|s| s.appearance));
        #[cfg(target_os = "linux")]
        {
            window.make_palette();
            // Opening it put the keyboard in it; give that back.
            if let Some(i) = self.front_index() {
                self.windows[i].1.window.focus_window();
            }
        }
        let mut palette = Palette::new(sp::Tab::All, false);
        palette.window = Some((w, h));
        self.palette_window = Some(PaletteWindow {
            window,
            surface,
            palette,
            cursor: (0.0, 0.0),
            shown: None,
            #[cfg(target_os = "linux")]
            inactive_since: None,
            #[cfg(target_os = "linux")]
            hidden: false,
        });
    }

    /// Hide the palette while another program is in front, and show it
    /// again when a Quill window has the keyboard, as the Mac's palette does.
    /// Moving between Quill's windows leaves none active for a moment, so
    /// it waits a little before hiding. Returns when to look again.
    #[cfg(target_os = "linux")]
    pub(super) fn sync_palette_shown(&mut self) -> Option<Instant> {
        let active = self.windows.iter().any(|(_, w)| w.window.is_active());
        let pw = self.palette_window.as_mut()?;
        if active || pw.window.is_active() {
            pw.inactive_since = None;
            if pw.hidden {
                pw.hidden = false;
                pw.window.set_visible(true);
            }
            return None;
        }
        if pw.hidden {
            return None;
        }
        let now = Instant::now();
        let due = *pw.inactive_since.get_or_insert(now) + Duration::from_millis(250);
        if now < due {
            return Some(due);
        }
        pw.hidden = true;
        pw.window.set_visible(false);
        None
    }

    /// The document window in front, which the palette works on.
    fn front_index(&self) -> Option<usize> {
        self.front
            .and_then(|id| self.index(id))
            .or(if self.windows.is_empty() {
                None
            } else {
                Some(0)
            })
    }

    pub(super) fn is_palette_window(&self, id: WindowId) -> bool {
        self.palette_window
            .as_ref()
            .is_some_and(|p| p.window.id() == id)
    }

    /// Ask the palette's window to draw again, after a document changed.
    pub(super) fn refresh_palette_window(&self) {
        if let Some(p) = &self.palette_window {
            p.window.request_redraw();
        }
    }

    pub(super) fn palette_window_event(&mut self, el: &ActiveEventLoop, event: WindowEvent) {
        let Some(front) = self.front_index() else {
            return;
        };
        let front_id = self.windows[front].0;
        let Some(pw) = &mut self.palette_window else {
            return;
        };
        if pw.shown != Some(front_id) {
            pw.shown = Some(front_id);
            pw.palette.chosen = None;
            pw.palette.hover = None;
            pw.palette.scroll = 0.0;
        }
        let scale = pw.window.scale_factor();
        let ui = ui_scale(scale as f32);
        let to_ui = |x: f64, y: f64| ((x / scale) as f32 / ui, (y / scale) as f32 / ui);
        match event {
            WindowEvent::CloseRequested => {
                crate::settings::update(|s| s.show_styles_palette = false);
                self.sync_palette_window(el);
                self.sync_menus();
                self.windows[front].1.window.request_redraw();
            }
            WindowEvent::RedrawRequested => self.draw_palette_window(front),
            WindowEvent::Resized(size) => {
                let (w, h) = to_ui(size.width as f64, size.height as f64);
                pw.palette.window = Some((w, h));
                pw.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => pw.window.request_redraw(),
            WindowEvent::CursorLeft { .. } => {
                pw.palette.hover = None;
                pw.window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = to_ui(position.x, position.y);
                self.take_doc(front);
                let (pw, w) = (
                    self.palette_window.as_mut().unwrap(),
                    &mut self.windows[front].1,
                );
                std::mem::swap(&mut pw.palette, &mut w.st.palette);
                let redraw = w.st.palette_move(x, y);
                std::mem::swap(&mut pw.palette, &mut w.st.palette);
                if redraw {
                    pw.window.request_redraw();
                }
                pw.cursor = (x, y);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y * 16.0 * 3.0,
                    MouseScrollDelta::PixelDelta(p) => -(p.y / scale) as f32,
                };
                let (x, y) = pw.cursor;
                self.take_doc(front);
                let (pw, w) = (
                    self.palette_window.as_mut().unwrap(),
                    &mut self.windows[front].1,
                );
                std::mem::swap(&mut pw.palette, &mut w.st.palette);
                // The wheel's amount is in window points; the palette scrolls
                // in interface units, as the docked one does.
                let moved = w.st.palette_wheel(x, y, dy * ui);
                std::mem::swap(&mut pw.palette, &mut w.st.palette);
                if moved {
                    pw.window.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let (x, y) = pw.cursor;
                self.palette_press(el, front, front_id, x, y);
            }
            _ => {}
        }
    }

    fn palette_press(
        &mut self,
        el: &ActiveEventLoop,
        front: usize,
        front_id: WindowId,
        x: f32,
        y: f32,
    ) {
        self.ensure_fonts();
        self.take_doc(front);
        let list = self.window_list(front_id);
        let env = Env {
            font_names: &self.font_names,
            font_families: &self.font_families,
            recent: &self.recent,
            windows: &list,
        };
        let clip = self.clipboard.get_or_insert_with(Clipboard::new);
        let (pw, w) = (
            self.palette_window.as_mut().unwrap(),
            &mut self.windows[front].1,
        );
        std::mem::swap(&mut pw.palette, &mut w.st.palette);
        let effect = w.st.palette_click(x, y, &env, clip);
        std::mem::swap(&mut pw.palette, &mut w.st.palette);
        let Some(effect) = effect else {
            return;
        };
        pw.window.request_redraw();
        // The person goes on typing in the document, not in the palette.
        w.window.focus_window();
        w.window.request_redraw();
        self.handle_effect(el, front_id, effect);
        self.sync_palette_window(el);
        self.sync_menus();
    }

    fn draw_palette_window(&mut self, front: usize) {
        self.ensure_fonts();
        self.take_doc(front);
        let Some(fonts) = self.fonts.as_mut() else {
            return;
        };
        let (pw, w) = (
            self.palette_window.as_mut().unwrap(),
            &mut self.windows[front].1,
        );
        let size = pw.window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        if pw.surface.resize(width, height).is_err() {
            return;
        }
        let Ok(mut buffer) = pw.surface.buffer_mut() else {
            return;
        };
        let scale = pw.window.scale_factor() as f32;
        let mut c = Canvas::new(&mut buffer, size.width, size.height, scale);
        c.dark = w.st.dark;
        let back = c.zoom(ui_scale(scale));
        let mut label = Label {
            fonts,
            cache: &mut self.cache,
        };
        std::mem::swap(&mut pw.palette, &mut w.st.palette);
        w.st.draw_palette(&mut c, &mut label);
        std::mem::swap(&mut pw.palette, &mut w.st.palette);
        c.set_scale(back);
        let _ = buffer.present();
    }
}
