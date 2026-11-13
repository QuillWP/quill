//! GTK owns the X11 window and menu bar; winit renders into its GtkSocket.
//! Native Wayland windows keep the shared renderer's menu bar.

use crate::app::UserEvent;
use crate::app_menus::Action;
use crate::menu::{Item, Menu};
use gtk::prelude::*;
use std::cell::Cell;
use std::ops::Deref;
use std::rc::Rc;
use winit::dpi::{PhysicalPosition, Position};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::platform::x11::{ActiveEventLoopExtX11, WindowAttributesExtX11};
use winit::raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle,
};
use winit::window::{Fullscreen, ResizeDirection, Theme, WindowAttributes};

/// The program's name, which desktops use to match a window to its .desktop
/// entry. The debug build is quill-debug, a separate program like the Mac's
/// Quill Debug app, so it never borrows the installed Quill's entry or icon.
pub const PROGRAM: &str = if cfg!(debug_assertions) {
    "quill-debug"
} else {
    "quill"
};

pub struct Window {
    // Destroy the embedded window before destroying its GTK parent.
    inner: winit::window::Window,
    host: Option<Host>,
}

struct Host {
    window: gtk::Window,
    bar: gtk::MenuBar,
    socket: gtk::Socket,
    nodes: std::cell::RefCell<Vec<gtk::MenuItem>>,
    shown: std::cell::RefCell<Vec<Menu>>,
    actions: std::cell::RefCell<Vec<Action>>,
    accelerators: std::cell::RefCell<gtk::AccelGroup>,
    proxy: EventLoopProxy<UserEvent>,
    syncing: Rc<Cell<bool>>,
}

impl Drop for Host {
    fn drop(&mut self) {
        // All GTK objects are created and destroyed on winit's main thread.
        unsafe { self.window.destroy() };
    }
}

/// Quill's icon for GTK's window. The winit window inside the GTK socket
/// carries its own copy, but a window manager reads the icon from the
/// top-level window, which is GTK's.
fn icons() -> Vec<gtk::gdk_pixbuf::Pixbuf> {
    let png = include_bytes!("../../../assets/quill-64.png");
    let Some((w, h, rgba)) = crate::image::decode_rgba(png, quill_core::ImageFormat::Png) else {
        return Vec::new();
    };
    let bytes = gtk::glib::Bytes::from_owned(rgba);
    vec![gtk::gdk_pixbuf::Pixbuf::from_bytes(
        &bytes,
        gtk::gdk_pixbuf::Colorspace::Rgb,
        true,
        8,
        w as i32,
        h as i32,
        w as i32 * 4,
    )]
}

impl Window {
    pub fn default_attributes() -> WindowAttributes {
        winit::window::Window::default_attributes()
    }

    pub fn create(
        el: &ActiveEventLoop,
        attrs: WindowAttributes,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Result<Self, winit::error::OsError> {
        let host = if el.is_x11() {
            if !gtk::is_initialized() {
                gtk::gdk::set_allowed_backends("x11");
                // The program name becomes the window class, which is how
                // a desktop matches the window to quill.desktop.
                gtk::glib::set_prgname(Some(PROGRAM));
                gtk::glib::set_application_name(if cfg!(debug_assertions) {
                    "Quill Debug"
                } else {
                    "Quill"
                });
            }
            match gtk::init() {
                Ok(()) => Some(Host::new(&attrs, proxy)),
                Err(error) => {
                    eprintln!("quill: GTK menu bar unavailable: {error}");
                    None
                }
            }
        } else {
            None
        };
        let attrs = match &host {
            Some(host) => attrs.with_embed_parent_window(host.socket.id() as u32),
            None => attrs,
        };
        let inner = el.create_window(attrs)?;
        if let Some(host) = &host {
            let id = inner.id();
            let proxy = host.proxy.clone();
            host.window.connect_delete_event(move |_, _| {
                let _ = proxy.send_event(UserEvent::LinuxClose(id));
                gtk::glib::Propagation::Stop
            });
            let proxy = host.proxy.clone();
            host.window.connect_focus_in_event(move |_, _| {
                let _ = proxy.send_event(UserEvent::LinuxFocus(id));
                gtk::glib::Propagation::Proceed
            });
            // GtkSocket drives ConfigureNotify for its embedded winit child.
            host.window.show_all();
            host.socket.grab_focus();
            focus_embedded(&inner);
        }
        Ok(Self { inner, host })
    }

    pub fn focus_editor(&self) {
        if self.host.is_some() {
            focus_embedded(&self.inner);
        } else {
            self.inner.focus_window();
        }
    }

    pub fn has_native_menu(&self) -> bool {
        self.host.is_some()
    }

    pub fn sync_menus(&self, menus: &[Menu], actions: &[Action], blocked: bool) {
        let Some(host) = &self.host else { return };
        let changed = !same_menus(&host.shown.borrow(), menus) || *host.actions.borrow() != actions;
        if changed {
            for child in host.bar.children() {
                // GTK accelerator groups hold references to their menu items.
                host.bar.remove(&child);
                unsafe { child.destroy() };
            }
            host.window.remove_accel_group(&*host.accelerators.borrow());
            let accelerators = gtk::AccelGroup::new();
            host.window.add_accel_group(&accelerators);
            *host.accelerators.borrow_mut() = accelerators;
            let mut nodes = Vec::new();
            for menu in menus {
                let top = gtk::MenuItem::with_label(&menu.title);
                let sub = host.build_menu(menu, actions, self.id(), &mut nodes);
                top.set_submenu(Some(&sub));
                host.bar.append(&top);
            }
            host.bar.show_all();
            *host.nodes.borrow_mut() = nodes;
            *host.shown.borrow_mut() = menus.to_vec();
            *host.actions.borrow_mut() = actions.to_vec();
        }
        host.syncing.set(true);
        let nodes = host.nodes.borrow();
        let mut index = 0;
        for menu in menus {
            sync_items(menu, &nodes, &mut index);
        }
        host.bar.set_sensitive(!blocked);
        host.syncing.set(false);
    }

    /// Light or dark, or the system's look for `None`. GTK draws the menu bar
    /// and, under X11, tells the window manager which title bar to draw, so
    /// a Dark setting turns those dark too.
    pub fn set_theme(&self, theme: Option<Theme>) {
        self.inner.set_theme(theme);
        if self.host.is_some()
            && let Some(settings) = gtk::Settings::default()
        {
            settings.set_gtk_application_prefer_dark_theme(theme == Some(Theme::Dark));
        }
    }

    /// Whether this window has the keyboard. A GTK-hosted window counts as
    /// active while its embedded editor has the focus.
    pub fn is_active(&self) -> bool {
        self.host.as_ref().is_some_and(|h| h.window.is_active()) || self.inner.has_focus()
    }

    /// Show or hide the window. A hosted window is GTK's top-level window,
    /// so hide that rather than the embedded one inside it.
    pub fn set_visible(&self, visible: bool) {
        match &self.host {
            Some(host) if visible => host.window.show(),
            Some(host) => host.window.hide(),
            None => self.inner.set_visible(visible),
        }
    }

    /// Make this window a floating palette: kept above the document
    /// windows, out of the task bar and window switcher, and never given the
    /// keyboard, so typing stays in the document. It has no menu bar.
    pub fn make_palette(&self) {
        let Some(host) = &self.host else { return };
        host.bar.hide();
        host.bar.set_no_show_all(true);
        host.window.set_type_hint(gtk::gdk::WindowTypeHint::Utility);
        host.window.set_keep_above(true);
        host.window.set_skip_taskbar_hint(true);
        host.window.set_skip_pager_hint(true);
        host.window.set_accept_focus(false);
        host.window.set_focus_on_map(false);
    }

    pub fn set_title(&self, title: &str) {
        self.inner.set_title(title);
        if let Some(host) = &self.host {
            host.window.set_title(title);
        }
    }

    pub fn focus_window(&self) {
        if let Some(host) = &self.host {
            host.window.present();
            host.socket.grab_focus();
        }
        self.focus_editor();
    }

    pub fn drag_resize_window(
        &self,
        direction: ResizeDirection,
    ) -> Result<(), winit::error::ExternalError> {
        if let Some(host) = &self.host {
            let edge = match direction {
                ResizeDirection::North => gtk::gdk::WindowEdge::North,
                ResizeDirection::NorthEast => gtk::gdk::WindowEdge::NorthEast,
                ResizeDirection::East => gtk::gdk::WindowEdge::East,
                ResizeDirection::SouthEast => gtk::gdk::WindowEdge::SouthEast,
                ResizeDirection::South => gtk::gdk::WindowEdge::South,
                ResizeDirection::SouthWest => gtk::gdk::WindowEdge::SouthWest,
                ResizeDirection::West => gtk::gdk::WindowEdge::West,
                ResizeDirection::NorthWest => gtk::gdk::WindowEdge::NorthWest,
            };
            if let Some(pointer) = host
                .window
                .display()
                .default_seat()
                .and_then(|s| s.pointer())
            {
                let (_, x, y) = pointer.position();
                host.window
                    .begin_resize_drag(edge, 1, x, y, gtk::current_event_time());
            }
            Ok(())
        } else {
            self.inner.drag_resize_window(direction)
        }
    }

    pub fn set_fullscreen(&self, fullscreen: Option<Fullscreen>) {
        if let Some(host) = &self.host {
            if fullscreen.is_some() {
                host.window.fullscreen();
            } else {
                host.window.unfullscreen();
            }
        } else {
            self.inner.set_fullscreen(fullscreen);
        }
    }

    pub fn fullscreen(&self) -> Option<Fullscreen> {
        if let Some(host) = &self.host {
            return host
                .window
                .window()
                .filter(|w| w.state().contains(gtk::gdk::WindowState::FULLSCREEN))
                .map(|_| Fullscreen::Borderless(None));
        }
        self.inner.fullscreen()
    }

    /// The saved frame includes GTK's menu bar, unlike the render surface.
    pub fn frame_size(&self) -> winit::dpi::PhysicalSize<u32> {
        if let Some(host) = &self.host {
            let (width, height) = host.window.size();
            winit::dpi::LogicalSize::new(width.max(1), height.max(1))
                .to_physical(self.scale_factor())
        } else {
            self.inner.inner_size()
        }
    }

    pub fn outer_position(&self) -> Result<PhysicalPosition<i32>, winit::error::NotSupportedError> {
        if let Some(host) = &self.host {
            let (x, y) = host.window.position();
            let scale = self.scale_factor();
            return Ok(winit::dpi::LogicalPosition::new(x, y).to_physical(scale));
        }
        self.inner.outer_position()
    }

    pub fn set_outer_position(&self, position: impl Into<Position>) {
        if let Some(host) = &self.host {
            let p = position.into().to_logical::<i32>(self.scale_factor());
            host.window.move_(p.x, p.y);
        } else {
            self.inner.set_outer_position(position);
        }
    }
}

impl Deref for Window {
    type Target = winit::window::Window;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl HasWindowHandle for Window {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.inner.window_handle()
    }
}
impl HasDisplayHandle for Window {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        self.inner.display_handle()
    }
}

impl Host {
    fn new(attrs: &WindowAttributes, proxy: EventLoopProxy<UserEvent>) -> Self {
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title(&attrs.title);
        // The default list also reaches dialogs that GTK opens.
        let icons = icons();
        if !icons.is_empty() {
            gtk::Window::set_default_icon_list(&icons);
            window.set_icon_list(&icons);
        }
        let scale = f64::from(window.scale_factor());
        if let Some(size) = attrs.inner_size {
            let size = size.to_logical::<i32>(scale);
            window.set_default_size(size.width, size.height);
        }
        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let bar = gtk::MenuBar::new();
        let socket = gtk::Socket::new();
        socket.set_can_focus(true);
        if let Some(size) = attrs.min_inner_size {
            let size = size.to_logical::<i32>(scale);
            socket.set_size_request(size.width, size.height);
        }
        column.pack_start(&bar, false, false, 0);
        column.pack_start(&socket, true, true, 0);
        let accelerators = gtk::AccelGroup::new();
        window.add_accel_group(&accelerators);
        window.add(&column);
        window.show_all();
        Self {
            window,
            bar,
            socket,
            nodes: Default::default(),
            shown: Default::default(),
            actions: Default::default(),
            accelerators: std::cell::RefCell::new(accelerators),
            proxy,
            syncing: Rc::new(Cell::new(false)),
        }
    }

    fn build_menu(
        &self,
        menu: &Menu,
        actions: &[Action],
        window: winit::window::WindowId,
        nodes: &mut Vec<gtk::MenuItem>,
    ) -> gtk::Menu {
        let native = gtk::Menu::new();
        for item in &menu.items {
            if item.separator {
                native.append(&gtk::SeparatorMenuItem::new());
                continue;
            }
            let node: gtk::MenuItem = if item.checkable {
                gtk::CheckMenuItem::with_label(&item.label).upcast()
            } else {
                gtk::MenuItem::with_label(&item.label)
            };
            if let Some(label) = node.child().and_then(|w| w.downcast::<gtk::Label>().ok())
                && (item.font.is_some() || item.style != Default::default())
            {
                let mut font = gtk::pango::FontDescription::new();
                if let Some(family) = &item.font {
                    font.set_family(family);
                }
                if item.style.bold {
                    font.set_weight(gtk::pango::Weight::Bold);
                }
                if item.style.italic {
                    font.set_style(gtk::pango::Style::Italic);
                }
                let attrs = gtk::pango::AttrList::new();
                attrs.insert(gtk::pango::AttrFontDesc::new(&font));
                if item.style.underline {
                    attrs.insert(gtk::pango::AttrInt::new_underline(
                        gtk::pango::Underline::Single,
                    ));
                }
                label.set_attributes(Some(&attrs));
            }
            nodes.push(node.clone());
            if let Some(spec) = accelerator_spec(&item.key) {
                let (key, mods) = gtk::accelerator_parse(&spec);
                if key != 0 {
                    node.add_accelerator(
                        "activate",
                        &*self.accelerators.borrow(),
                        key,
                        mods,
                        gtk::AccelFlags::VISIBLE,
                    );
                }
            }
            if let Some(sub) = &item.submenu {
                node.set_submenu(Some(&self.build_menu(sub, actions, window, nodes)));
            } else if let Some(action) = actions.get(item.id as usize) {
                let action = action.clone();
                let proxy = self.proxy.clone();
                let syncing = self.syncing.clone();
                node.connect_activate(move |_| {
                    if syncing.get() {
                        return;
                    }
                    let _ = proxy.send_event(UserEvent::LinuxMenu {
                        window,
                        action: action.clone(),
                    });
                });
            }
            native.append(&node);
        }
        native
    }
}

fn same_menus(a: &[Menu], b: &[Menu]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(a, b)| a.title == b.title && same_items(&a.items, &b.items))
}
fn same_items(a: &[Item], b: &[Item]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.label == b.label
                && a.font == b.font
                && a.style == b.style
                && a.key == b.key
                && a.id == b.id
                && a.checkable == b.checkable
                && a.separator == b.separator
                && match (&a.submenu, &b.submenu) {
                    (None, None) => true,
                    (Some(a), Some(b)) => {
                        same_menus(std::slice::from_ref(a), std::slice::from_ref(b))
                    }
                    _ => false,
                }
        })
}
fn sync_items(menu: &Menu, nodes: &[gtk::MenuItem], index: &mut usize) {
    for item in &menu.items {
        if item.separator {
            continue;
        }
        let node = &nodes[*index];
        *index += 1;
        node.set_sensitive(item.enabled);
        if let Some(check) = node.downcast_ref::<gtk::CheckMenuItem>()
            && check.is_active() != item.checked
        {
            check.set_active(item.checked);
        }
        if let Some(menu) = &item.submenu {
            sync_items(menu, nodes, index);
        }
    }
}

/// Whether the desktop is set to dark. winit cannot say on Linux: X11 has no
/// theme, and Wayland reports only what Quill asked for. Ubuntu's Appearance
/// settings and GNOME's write `color-scheme` (and a "-dark" GTK theme on
/// older releases), and GTK_THEME overrides both, as it does for GTK.
pub fn system_dark() -> bool {
    use gtk::gio::prelude::SettingsExt;
    if let Ok(theme) = std::env::var("GTK_THEME") {
        return theme.to_ascii_lowercase().contains("dark");
    }
    thread_local! {
        static INTERFACE: Option<(gtk::gio::Settings, bool)> = gnome_interface();
    }
    INTERFACE.with(|interface| {
        let Some((settings, has_scheme)) = interface else {
            return false;
        };
        if *has_scheme {
            match settings.string("color-scheme").as_str() {
                "prefer-dark" => return true,
                "prefer-light" => return false,
                _ => {}
            }
        }
        settings
            .string("gtk-theme")
            .to_ascii_lowercase()
            .contains("dark")
    })
}

/// GNOME's interface settings, when the desktop has them. Opening a schema
/// or key that is not installed would abort, so look both up first.
fn gnome_interface() -> Option<(gtk::gio::Settings, bool)> {
    let schema =
        gtk::gio::SettingsSchemaSource::default()?.lookup("org.gnome.desktop.interface", true)?;
    if !schema.has_key("gtk-theme") {
        return None;
    }
    let has_scheme = schema.has_key("color-scheme");
    let settings = gtk::gio::Settings::new_full(&schema, None::<&gtk::gio::SettingsBackend>, None);
    Some((settings, has_scheme))
}

/// The system's dark setting when it has changed since the last call, at
/// most every half second. Reading a setting is cheap but not free, and
/// this is asked each time the event loop goes idle.
pub fn system_dark_changed() -> Option<bool> {
    use std::time::{Duration, Instant};
    thread_local! {
        static LAST: Cell<Option<(bool, Instant)>> = const { Cell::new(None) };
    }
    let now = Instant::now();
    let last = LAST.get();
    if last.is_some_and(|(_, at)| now < at + Duration::from_millis(500)) {
        return None;
    }
    let dark = system_dark();
    LAST.set(Some((dark, now)));
    (last.is_some_and(|(was, _)| was != dark)).then_some(dark)
}

/// Service GTK on the same thread as winit. A bounded drain avoids starving
/// document events while a GTK source remains ready.
pub fn poll() {
    if !gtk::is_initialized_main_thread() {
        return;
    }
    for _ in 0..32 {
        if !gtk::events_pending() {
            break;
        }
        gtk::main_iteration_do(false);
    }
}

/// Translate the shared shortcut notation to GTK's accelerator notation.
fn accelerator_spec(label: &str) -> Option<String> {
    let mut modifiers = String::new();
    let mut key = None;
    for c in label.chars() {
        match c {
            '⌘' => modifiers.push_str("<Control>"),
            // The shared Linux keymap has no separate macOS Control modifier.
            '⌃' => return None,
            '⇧' => modifiers.push_str("<Shift>"),
            '⌥' => modifiers.push_str("<Alt>"),
            c => key = Some(c),
        }
    }
    let key = match key? {
        '[' => "bracketleft".into(),
        ']' => "bracketright".into(),
        '.' | '>' => "period".into(),
        ',' | '<' => "comma".into(),
        '=' => "equal".into(),
        '-' => "minus".into(),
        '/' => "slash".into(),
        '\\' => "backslash".into(),
        '`' => "grave".into(),
        '␣' => "space".into(),
        '⌅' => "KP_Enter".into(),
        c if c.is_ascii_alphanumeric() => c.to_ascii_lowercase().to_string(),
        _ => return None,
    };
    Some(format!("{modifiers}{key}"))
}

/// winit asks the window manager to activate top-level X11 windows. The
/// embedded editor is not a managed top-level window, so give it focus directly.
fn focus_embedded(window: &winit::window::Window) {
    static XLIB: std::sync::OnceLock<Option<x11_dl::xlib::Xlib>> = std::sync::OnceLock::new();
    let Some(xlib) = XLIB.get_or_init(|| x11_dl::xlib::Xlib::open().ok()) else {
        return;
    };
    let (Ok(display), Ok(handle)) = (window.display_handle(), window.window_handle()) else {
        return;
    };
    let (RawDisplayHandle::Xlib(display), RawWindowHandle::Xlib(handle)) =
        (display.as_raw(), handle.as_raw())
    else {
        return;
    };
    let Some(display) = display.display else {
        return;
    };
    // SAFETY: winit owns this live X11 display and child window. Calls stay on
    // the main thread, and the request is flushed before returning to winit.
    unsafe {
        (xlib.XSetInputFocus)(
            display.as_ptr().cast(),
            handle.window,
            x11_dl::xlib::RevertToParent,
            x11_dl::xlib::CurrentTime,
        );
        (xlib.XFlush)(display.as_ptr().cast());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_use_control_and_keep_shifted_punctuation() {
        assert_eq!(accelerator_spec("⌘B").as_deref(), Some("<Control>b"));
        assert_eq!(
            accelerator_spec("⌘⇧>").as_deref(),
            Some("<Control><Shift>period")
        );
        assert_eq!(accelerator_spec("⇧⌅").as_deref(), Some("<Shift>KP_Enter"));
        assert_eq!(accelerator_spec(""), None);
        assert_eq!(accelerator_spec("⌃⌘F"), None);
    }

    #[test]
    fn menu_structure_tracks_nested_labels_but_not_checkmarks() {
        let nested = Item::new("Remove from List", 1).submenu(Menu {
            title: "Remove from List".into(),
            items: vec![Item::new("First.qwp", 2)],
        });
        let a = vec![Menu {
            title: "File".into(),
            items: vec![nested],
        }];
        let mut b = a.clone();
        b[0].items[0].enabled = false;
        assert!(same_menus(&a, &b));
        b[0].items[0].submenu.as_mut().unwrap().items[0].label = "Second.qwp".into();
        assert!(!same_menus(&a, &b));
    }
}
