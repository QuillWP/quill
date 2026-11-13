//! The application: windows, the event loop, and routing input to the
//! menu bar, toolbars, ruler and text.

use crate::app_menus::{self, Action, MenuContext};
use crate::bars::{
    self, FormattingToolbarState, Hit, ParagraphToolbarState, RulerState, SCALE_HEIGHT, ScaleGrab,
};
use crate::canvas::{Canvas, Label, Rect, color};
use crate::chrome::{self, BAR, ScrollPart};
use crate::commands::{self as command_map, Cmd, Mods};
use crate::dialog::{DKey, Dialog, Outcome};
use crate::docview::{BorderKind, Clipboard, DialogId, DocView, Effect, Handle, Port};
use crate::format_dialogs::{self, Which};
#[cfg(target_os = "linux")]
use crate::linux_window::Window;
use crate::menu::{self, MENU_BAR_HEIGHT, Menu, OpenMenu};
use crate::pane::{self, Pane, PaneHit};
use crate::toolbar::{self, ToolState};
use quill_core::props::{LineSpacing, ParaProps, TabAlign};
use quill_core::{Document, Selection};
use quill_formats::Format;
use quill_layout::{Catalog, FontFamilyInfo, Fonts, GlyphCache};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{
    ElementState, Ime, MouseButton, MouseScrollDelta, StartCause, TouchPhase, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};
#[cfg(not(target_os = "linux"))]
use winit::window::Window;
use winit::window::{CursorIcon, CustomCursor, ResizeDirection, WindowId};

mod commands;
mod dialogs;
mod files;
mod markdown;
mod palette_window;
mod panes;
#[cfg(target_os = "macos")]
mod platform;
#[cfg(target_os = "macos")]
mod preferences;
#[cfg(target_os = "macos")]
mod printing;
mod revisions;
mod snapshot;
mod styles;
#[cfg(not(target_os = "macos"))]
mod system_preview;
mod window_ui;
mod zoom;

pub use snapshot::snapshot;

/// Where an open menu came from.
#[derive(Clone, Copy, Debug, PartialEq)]
enum MenuSource {
    Bar(usize),
    Popup(Hit),
    Context,
    Zoom,
}

struct MenuTrack {
    source: MenuSource,
    menus: Vec<Menu>,
    actions: Vec<Action>,
    open: OpenMenu,
    /// The parent item and open box for one nested menu.
    submenu: Option<(usize, OpenMenu)>,
    /// What has been typed to jump to an item, and when.
    typed: (String, Option<Instant>),
    opened: Instant,
    /// The button is still down from the press that opened it.
    held: bool,
}

impl MenuTrack {
    /// Which of `menus` is open: the bar's own index, or the only one.
    fn index(&self) -> usize {
        match self.source {
            MenuSource::Bar(i) => i,
            MenuSource::Popup(_) | MenuSource::Context | MenuSource::Zoom => 0,
        }
    }

    fn menu(&self) -> &Menu {
        &self.menus[self.index()]
    }
}

impl WinState {
    /// A pop-up menu Quill has just opened (Zoom, the toolbar's own menu,
    /// the formatting toolbar's Font, Size and Style), taken away so the
    /// system can show it instead: the menu, its actions and its top-left
    /// corner in window points.
    #[cfg(target_os = "macos")]
    fn take_popup_menu(&mut self) -> Option<(Menu, Vec<Action>, (f64, f64))> {
        if matches!(self.menu.as_ref()?.source, MenuSource::Bar(_)) {
            return None;
        }
        let track = self.menu.take()?;
        let at = (
            (track.open.rect.x * self.ui) as f64,
            (track.open.rect.y * self.ui) as f64,
        );
        let menu = track.menu().clone();
        Some((menu, track.actions, at))
    }
}

/// Whole rows from wheel movement, keeping the remainder in `rest` for the
/// next turn of the wheel.
fn wheel_rows(rest: &mut f32, dy: f32) -> i32 {
    *rest += dy;
    let rows = (*rest / 16.0).trunc();
    *rest -= rows * 16.0;
    rows as i32
}

/// A drag on the ruler's scale.
struct ScaleDrag {
    grab: ScaleGrab,
    start: Vec<ParaProps>,
    /// For a table column: each row's end mark and settings at the start,
    /// and where the pointer started (interface units, so the drag does
    /// not follow the ruler's zero as the cell moves).
    rows: Vec<(usize, quill_core::RowProps)>,
    from: f32,
}

/// Window > Split (⌘⌥S): two panes on the same text, each scrolled on its
/// own. The active pane's scroll position is the view's; the other's
/// waits here.
#[derive(Clone, Copy, Debug)]
struct Split {
    /// Height of the upper pane, in interface units.
    upper: f32,
    other_scroll: f32,
    upper_active: bool,
}

struct FrameDrag {
    index: usize,
    page: usize,
    grab: (f32, f32),
    target: Rect,
}

/// The split box at the top of the scroll bar, and the bar between panes.
const SPLIT_BOX: f32 = 7.0;
const DIVIDER: f32 = 7.0;
/// Panes smaller than this close when the divider is let go.
const MIN_PANE: f32 = 24.0;

/// Everything about one document window that is not the OS window itself.
/// Shared by real windows and offscreen snapshots.
pub struct WinState {
    pub view: DocView,
    pub show_formatting_toolbar: bool,
    pub show_paragraph_toolbar: bool,
    pub show_ruler: bool,
    /// Whether the window fills the screen (View > Enter Full Screen).
    pub full_screen: bool,
    tab_type: TabAlign,
    menu: Option<MenuTrack>,
    scroll_press: Option<(ScrollPart, f32, f32)>,
    horizontal_press: Option<(ScrollPart, f32, f32)>,
    scale_drag: Option<ScaleDrag>,
    /// Open dialogs and what they are for; the last is in front (Tabs can
    /// open over Paragraph, as in the original).
    pub dialogs: Vec<(Which, Dialog)>,
    vscroll: Rect,
    /// Window size in interface units (points divided by `ui`).
    width: f32,
    height: f32,
    /// How much larger than the original's 72 dpi sizes the window furniture is
    /// drawn. Text is never scaled by this.
    pub ui: f32,
    /// Screen-only scale for the document. Layout and saved measurements stay
    /// in document points.
    document_zoom: f32,
    /// Drawing in dark mode (Preferences > View > Appearance, or the
    /// system's when that is Auto): the interface and the page go dark.
    pub dark: bool,
    zoom_edit: Option<zoom::Edit>,
    review_filter: revisions::Filter,
    review_closed: bool,
    review_width: Option<f32>,
    /// The Style Sheets palette's own state; whether it shows, its tab and
    /// whether it is docked are the settings'.
    palette: crate::styles_palette::Palette,
    review_resize: Option<f32>,
    review_resize_text_width: Option<f32>,
    /// Where the document was scrolled to when the comments last followed it,
    /// and which comment the pane's insertion point was last in.
    comment_sync: Option<f32>,
    comment_here: Option<u32>,
    revision_hover: Option<usize>,
    revision_badges: Vec<revisions::Badge>,
    review_detail_scroll: f32,
    review_detail_max: f32,
    review_shown: Option<(quill_core::Position, quill_core::Position, u32)>,
    position_frames: bool,
    frame_drag: Option<FrameDrag>,
    /// Draw the menu bar inside the window (where the system has none).
    pub own_menu_bar: bool,
    /// Real macOS windows use floating AppKit scrollbars. Snapshots and the
    /// portable window keep Quill's drawn controls.
    native_scrollbars: bool,
    /// The current dialog is being presented by AppKit. Snapshots and any
    /// dialog whose native construction failed keep the portable renderer.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    native_dialogs: bool,
    /// This state belongs to a window on screen, not a snapshot, so AppKit
    /// panels such as the Colors panel can be shown from it.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    on_screen: bool,
    /// The color palette popped out beside a dialog's color, while it is.
    /// The color pop-out's state, and the color box it came out of.
    color_pop: Option<(&'static str, crate::palette::PaletteState)>,
    pub show_toolbar: bool,
    toolbar_press: Option<usize>,
    /// The toolbar button under the pointer.
    toolbar_hover: Option<usize>,
    /// The formatting toolbar or ruler control under the pointer.
    bar_hover: Option<Hit>,
    /// The tooltip for the control under the pointer, once it has rested.
    tip: Option<Tip>,
    /// When and where the ruler was last clicked, to spot double-clicks.
    last_ruler_click: Option<(Instant, f32, f32)>,
    /// The header or footer pane, if open. While it has focus its view is
    /// in `view` (so the formatting toolbar, ruler, keys and menus work on
    /// it) and the main text's view waits in the pane.
    pane: Option<Pane>,
    /// A print job the Print dialog asked for, for the application to run.
    pub pending_print: Option<crate::page_dialogs::PrintJob>,
    /// Merged letters to print in place of the window's document.
    pub print_doc: Option<quill_core::Document>,
    /// A document to open in a window of its own (merged letters).
    pub pending_window: Option<(quill_core::Document, String)>,
    /// How many merged documents this window has made, to name them.
    form_letters: u32,
    /// A command for the app to carry out (Do It in Tools > Commands).
    pub pending_cmd: Option<Cmd>,
    /// Open (true) or save (false) a settings file.
    pub pending_settings: Option<bool>,
    /// The front dialog took the last mouse press, so moves and the
    /// release go to it.
    dialog_mouse: bool,
    /// Font names, for dialogs opened from other dialogs.
    font_names: Vec<String>,
    /// Installed family variants, for nested Character dialogs.
    font_families: Vec<FontFamilyInfo>,
    /// Where the pointer is, in interface units, and wheel movement not
    /// yet worth a whole row in a dialog's list.
    pointer: (f32, f32),
    wheel_rest: f32,
    split: Option<Split>,
    /// Dragging the split box or the divider: where it is now.
    split_drag: Option<f32>,
    /// The other pane's scroll bar.
    vscroll_other: Rect,
    last_split_click: Option<Instant>,
}

/// Interface scale. At 100% (`interface_scale`) the 1992 app's 16-point
/// controls are drawn at 1.2375, about 20 points tall: a little under a
/// Mac's own 22 (Pages) and the original's 26, which looked best next to the
/// document. Points, not pixels, so a Retina display shows the same size
/// as any other; icons are vector and text is drawn at size, so the uneven
/// pixel scale does not blur them. Windows and Linux use 1.25, nearer
/// their own menus.
/// Make `window` light, dark or as the system is, for Preferences >
/// View > Appearance. On macOS the whole application takes the setting
/// instead (`App::sync_appearance`), so its alerts and panels match.
fn set_window_appearance(window: &Window, appearance: crate::settings::Appearance) {
    #[cfg(not(target_os = "macos"))]
    window.set_theme(match appearance {
        crate::settings::Appearance::Auto => None,
        crate::settings::Appearance::Light => Some(winit::window::Theme::Light),
        crate::settings::Appearance::Dark => Some(winit::window::Theme::Dark),
    });
    #[cfg(target_os = "macos")]
    let _ = (window, appearance);
}

/// Whether `window` should draw dark: the Appearance setting, or the
/// system's look when that is Auto.
fn appearance_is_dark(window: &Window) -> bool {
    crate::settings::with(|s| s.appearance)
        .is_dark(window.theme() == Some(winit::window::Theme::Dark))
}

pub fn ui_scale(_display_scale: f32) -> f32 {
    let base = if cfg!(target_os = "macos") {
        1.2375
    } else {
        1.25
    };
    base * crate::settings::with(|s| s.interface_scale) as f32 / 100.0
}

/// Use a comfortable share of the display instead of the original's narrow
/// document window.  Caps keep an enormous or ultrawide screen from making
/// the first window comical; the lower bounds only apply when the display has
/// room for them.
fn initial_window_size(screen_width: f64, screen_height: f64) -> LogicalSize<f64> {
    let width = (screen_width * 0.85)
        .min(1400.0)
        .max(screen_width.min(720.0));
    let height = (screen_height * 0.85)
        .min(1000.0)
        .max(screen_height.min(640.0));
    LogicalSize::new(width, height)
}

/// Where to open a window at the remembered `frame` (left, top, width,
/// height in points), given the screens connected now (the same) and the
/// top-left corners of windows already open. None if the frame's title bar
/// is no longer on a screen, as after unplugging a display. A window
/// already there pushes the new one down and right, as macOS does.
fn restore_frame(frame: [i32; 4], screens: &[[f64; 4]], taken: &[(f64, f64)]) -> Option<[f64; 4]> {
    let [x, y, w, h] = frame.map(f64::from);
    // Enough of the title bar to grab: 100 points across, 20 down.
    let screen = screens.iter().find(|[sx, sy, sw, sh]| {
        let across = (x + w).min(sx + sw) - x.max(*sx);
        let down = (y + 20.0).min(sy + sh) - y.max(*sy);
        across >= 100.0 && down >= 20.0
    })?;
    let (w, h) = (w.min(screen[2]), h.min(screen[3]));
    let (mut x, mut y) = (x, y);
    while taken
        .iter()
        .any(|&(tx, ty)| (tx - x).abs() < 4.0 && (ty - y).abs() < 4.0)
    {
        x += 24.0;
        y += 24.0;
    }
    Some([x, y, w, h])
}

/// Things the window needs from the application to build menus.
pub struct Env<'a> {
    pub font_names: &'a [String],
    pub font_families: &'a [FontFamilyInfo],
    pub recent: &'a [PathBuf],
    pub windows: &'a [(String, bool)],
}

impl WinState {
    pub fn new(view: DocView) -> Self {
        let (formatting, paragraph, ruler, zoom) = crate::settings::with(|s| {
            (
                s.open_with_formatting_toolbar,
                s.open_with_paragraph_toolbar,
                s.open_with_ruler,
                s.document_zoom as f32 / 100.0,
            )
        });
        let mut view = view;
        view.open_in_its_view();
        WinState {
            view,
            show_formatting_toolbar: formatting,
            show_paragraph_toolbar: paragraph,
            show_ruler: ruler,
            full_screen: false,
            tab_type: TabAlign::Left,
            menu: None,
            scroll_press: None,
            horizontal_press: None,
            scale_drag: None,
            dialogs: Vec::new(),
            vscroll: Rect::default(),
            width: 0.0,
            height: 0.0,
            ui: 1.0,
            document_zoom: zoom,
            dark: false,
            zoom_edit: None,
            review_filter: revisions::Filter::All,
            review_closed: false,
            review_width: None,
            palette: crate::styles_palette::Palette::new(crate::styles_palette::Tab::All, true),
            review_resize: None,
            review_resize_text_width: None,
            comment_sync: None,
            comment_here: None,
            revision_hover: None,
            revision_badges: Vec::new(),
            review_detail_scroll: 0.0,
            review_detail_max: 0.0,
            review_shown: None,
            position_frames: false,
            frame_drag: None,
            own_menu_bar: !cfg!(target_os = "macos"),
            native_scrollbars: false,
            native_dialogs: false,
            on_screen: false,
            color_pop: None,
            dialog_mouse: false,
            font_names: Vec::new(),
            font_families: Vec::new(),
            pointer: (0.0, 0.0),
            wheel_rest: 0.0,
            split: None,
            split_drag: None,
            vscroll_other: Rect::default(),
            last_split_click: None,
            show_toolbar: true,
            toolbar_press: None,
            toolbar_hover: None,
            bar_hover: None,
            tip: None,
            last_ruler_click: None,
            pane: None,
            pending_print: None,
            print_doc: None,
            pending_window: None,
            form_letters: 0,
            pending_cmd: None,
            pending_settings: None,
        }
    }

    fn split_rects(&self, sp: &Split) -> (Rect, Rect) {
        let top = self.text_top();
        let bottom = self.height - BAR;
        let (x0, narrower) = self.side_toolbar();
        let lane = if self.view.show_changes && self.view.doc.has_changes() {
            revisions::BADGE_LANE
        } else {
            0.0
        };
        let w = self.width - self.right_gutter() - narrower - lane;
        let upper = sp.upper.clamp(0.0, (bottom - top - DIVIDER).max(0.0));
        let low_top = top + upper + DIVIDER;
        (
            Rect::new(x0, top, w, upper),
            Rect::new(x0, low_top, w, (bottom - low_top).max(0.0)),
        )
    }

    /// The active and other pane's rectangles, in interface units.
    fn active_rects(&self) -> Option<(Rect, Rect)> {
        if self.pane.is_some() {
            return None;
        }
        let sp = self.split.as_ref()?;
        let (u, l) = self.split_rects(sp);
        Some(if sp.upper_active { (u, l) } else { (l, u) })
    }

    /// Give the keyboard and the insertion point's scrolling to the other
    /// pane.
    fn switch_pane(&mut self) {
        let Some(sp) = &mut self.split else {
            return;
        };
        sp.upper_active = !sp.upper_active;
        std::mem::swap(&mut sp.other_scroll, &mut self.view.scroll_y);
        if let Some((a, _)) = self.active_rects() {
            let k = self.ui;
            let z = self.view_zoom();
            self.view.area = Rect::new(a.x * k / z, a.y * k / z, a.w * k / z, a.h * k / z);
        }
    }

    /// Split Window (⌘⌥S): split in half, or join the panes again.
    pub fn toggle_split(&mut self) {
        if self.split.take().is_some() || self.pane.is_some() {
            return;
        }
        let top = self.text_top();
        let bottom = self.height - BAR;
        self.split = Some(Split {
            upper: ((bottom - top - DIVIDER) / 2.0).round(),
            other_scroll: self.view.scroll_y,
            upper_active: false,
        });
    }

    /// Let go of the split box or divider at `y`.
    fn finish_split_drag(&mut self, y: f32) {
        let top = self.text_top();
        let bottom = self.height - BAR;
        let upper = y - top;
        if upper < MIN_PANE {
            // Dragged to the top: the lower pane fills the window.
            if self.split.as_ref().is_some_and(|sp| sp.upper_active) {
                self.switch_pane();
            }
            self.split = None;
        } else if y > bottom - MIN_PANE {
            if self.split.as_ref().is_some_and(|sp| !sp.upper_active) {
                self.switch_pane();
            }
            self.split = None;
        } else {
            match &mut self.split {
                Some(sp) => sp.upper = upper.round(),
                None => {
                    self.split = Some(Split {
                        upper: upper.round(),
                        other_scroll: self.view.scroll_y,
                        upper_active: false,
                    })
                }
            }
        }
    }

    /// The scroll wheel: lists in a window under the pointer scroll by
    /// rows, otherwise the text scrolls.
    fn horizontal_bar(&self) -> Rect {
        let end = self.width - self.right_gutter();
        let start = if self.view.status.is_some() {
            end
        } else {
            210.0
        };
        Rect::new(start, self.height - BAR, (end - start).max(0.0), BAR)
    }

    fn horizontal_step(&mut self, part: ScrollPart) {
        let v = self.main_view_mut();
        let page = (v.area.w - 24.0).max(16.0);
        let dx = match part {
            ScrollPart::LineUp => -16.0,
            ScrollPart::LineDown => 16.0,
            ScrollPart::PageUp => -page,
            ScrollPart::PageDown => page,
            ScrollPart::Thumb => 0.0,
        };
        v.scroll_sideways(dx);
    }

    fn wheel_horizontal(&mut self, dx: f32) {
        if !self.dialogs.is_empty() || self.menu.is_some() || dx == 0.0 {
            return;
        }
        let (x, y) = self.pointer;
        let amount = dx / self.view_zoom();
        if self.pane.as_ref().is_some_and(|p| p.bounds.contains(x, y)) {
            if let Some(v) = self.pane_view_mut() {
                v.scroll_sideways(amount);
            }
        } else {
            self.main_view_mut().scroll_sideways(amount);
        }
    }

    pub fn wheel(&mut self, dy: f32) {
        let (cx, cy) = self.pointer;
        if let Some(t) = &mut self.menu {
            let rows = wheel_rows(&mut self.wheel_rest, dy);
            let m = &t.menus[t.index()];
            if let Some((parent, child_open)) = &mut t.submenu
                && let Some(child) = m.items[*parent].submenu.as_ref()
                && child_open.rect.contains(cx, cy)
            {
                if menu::scroll_by(child, child_open, rows) {
                    child_open.hot = menu::item_at(child, child_open, cx, cy);
                }
                return;
            }
            if menu::scroll_by(m, &mut t.open, rows) {
                t.open.hot = menu::item_at(m, &t.open, cx, cy);
            }
            return;
        }
        if let Some((which, d)) = self.dialogs.last_mut()
            && (d.contains(cx, cy) || d.popup_open())
        {
            let rows = wheel_rows(&mut self.wheel_rest, dy);
            if d.wheel(cx, cy, rows) {
                return;
            }
            if let Which::Commands(st) = which {
                crate::commands_dialog::scroll(st, d, cx, cy, rows);
            }
            return;
        }
        if self.palette_wheel(cx, cy, dy) {
            return;
        }
        if self.review_wheel(cx, cy, dy) {
            return;
        }
        // In a split window the pane under the pointer scrolls.
        if let Some((_, other)) = self.active_rects()
            && other.contains(cx, cy)
            && let Some(sp) = &mut self.split
        {
            sp.other_scroll = (sp.other_scroll + dy / self.document_zoom).max(0.0);
            return;
        }
        let amount = dy / self.view_zoom();
        if let Some(p) = &self.pane {
            if p.bounds.contains(cx, cy) {
                if let Some(v) = self.pane_view_mut() {
                    v.scroll_by(amount);
                }
            } else {
                self.main_view_mut().scroll_by(amount);
            }
        } else {
            self.view.scroll_by(amount);
        }
    }

    /// Run a menu action. Returns what the application should do.
    pub fn act(&mut self, action: Action, clip: &mut Clipboard) -> Effect {
        if self.zoom_edit.is_some() {
            match action {
                Action::Cmd(Cmd::SelectAll) => {
                    self.zoom_key(DKey::SelectAll, None);
                    return Effect::None;
                }
                Action::Cmd(Cmd::Copy | Cmd::Cut) => {
                    if let Some(text) = self.zoom_selection() {
                        clip.put_text(&text);
                        if action == Action::Cmd(Cmd::Cut) {
                            self.zoom_key(DKey::Backspace, None);
                        }
                    }
                    return Effect::None;
                }
                Action::Cmd(Cmd::Paste | Cmd::PasteText) => {
                    if let Some(text) = clip.plain_text() {
                        self.zoom_text(&text);
                    }
                    return Effect::None;
                }
                _ if !self.commit_zoom() => return Effect::None,
                _ => {}
            }
        }
        if self.main_view().in_markdown_source() && action.changes_format() {
            return Effect::None;
        }
        match action {
            Action::None | Action::Window(_) => {}
            Action::About => return Effect::About,
            Action::Licenses => return Effect::Licenses,
            Action::Recent(index) => return Effect::OpenRecent(index),
            Action::RemoveRecent(index) => return Effect::RemoveRecent(index),
            Action::ClearRecent => return Effect::ClearRecent,
            Action::Zoom(choice) => self.choose_zoom(choice),
            Action::Cmd(c) => return self.view.exec(c, false, clip),
            Action::FontVariant(name, bold, italic) => {
                let id = self.view.doc.font_id(&name);
                self.view.format_chars("Formatting", move |c| {
                    c.font = id;
                    c.bold = bold;
                    c.italic = italic;
                });
            }
            Action::Size(hp) => self.view.set_size(hp),
            Action::Color(c) => self.view.set_color(c),
            Action::CellAlign(i) => {
                if let Some(&(_, v, h)) = crate::docview::CELL_ALIGNMENTS.get(i) {
                    self.view.align_cells(v, h);
                }
            }
            Action::CharacterStyle(id) => self.view.set_character_style(id),
            Action::Style(id) => {
                // Choosing a paragraph's own style when it has been
                // reformatted asks whether to redefine the style or put the
                // paragraph back, as the original does.
                if let Some(d) = crate::style_dialog::reapply(&self.view, id) {
                    self.dialogs.push((Which::Reapply(id), d));
                } else {
                    self.view.set_style(id);
                }
            }
            Action::Pane(story) => self.open_pane(story),
            Action::ToggleFormattingToolbar => {
                self.show_formatting_toolbar = !self.show_formatting_toolbar
            }
            Action::ToggleParagraphToolbar => {
                self.show_paragraph_toolbar = !self.show_paragraph_toolbar
            }
            Action::ToggleToolbar => {
                crate::settings::update(|s| s.view_toolbar = !s.view_toolbar);
                self.show_toolbar = crate::settings::with(|s| s.view_toolbar);
            }
            Action::ToggleRuler => self.show_ruler = !self.show_ruler,
            Action::ToggleStylesPalette => self.toggle_palette(),
            Action::ToggleMissingFonts => crate::settings::update(|s| {
                s.highlight_missing_fonts = !s.highlight_missing_fonts;
            }),
            Action::ToggleFullScreen => return Effect::FullScreen,
            Action::ToolbarPosition(at) => crate::settings::update(|s| {
                s.toolbar_position = at;
                s.view_toolbar = true;
            }),
            Action::CustomizeToolbar => {
                let (d, st) = crate::customize::dialog();
                self.dialogs.push((Which::Customize(st), d));
            }
        }
        Effect::None
    }

    /// The pointer went down.
    pub fn mouse_down(
        &mut self,
        x: f32,
        y: f32,
        mods: Mods,
        label: &mut Label,
        env: &Env,
        clip: &mut Clipboard,
    ) -> Effect {
        // (x, y) are window points for the text; (cx, cy) are interface
        // units for everything else.
        let (cx, cy) = (x / self.ui, y / self.ui);
        let (dx, dy) = self.document_point(x, y);
        // A click puts the tooltip away until the pointer moves on.
        self.tip = None;
        // With the Tabs dialog open, clicking a tab on the ruler chooses it.
        if self.show_ruler
            && !self.main_view().outline
            && bars::on_ruler(self.ruler_top(), cy)
            && let Some((tabs, _)) = self.dialog_tabs()
        {
            let rs = self.ruler_state();
            let near = tabs
                .iter()
                .find(|t| (rs.text_left + t.pos as f32 / 1440.0 * rs.inch - cx).abs() <= 4.0)
                .copied();
            if let Some(t) = near
                && let Some((Which::Tabs { selected, .. }, d)) = self.dialogs.last_mut()
            {
                *selected = Some(t.pos);
                format_dialogs::choose_tab(d, &t);
            }
            return Effect::None;
        }
        if let Some((_, d)) = self.dialogs.last_mut() {
            // A click outside a modeless window goes to the document, which
            // takes the keyboard; the window stays open, inactive.
            if d.modeless && (self.native_dialogs || (!d.contains(cx, cy) && !d.popup_open())) {
                d.inactive = true;
            } else {
                d.inactive = false;
                self.dialog_mouse = true;
                let o = d.mouse_down(cx, cy, mods.shift, label);
                self.dialog_outcome(o, clip);
                return Effect::None;
            }
        }
        // A click while a menu is open either works the menu or closes it.
        if let Some(t) = &mut self.menu {
            if t.open.rect.contains(cx, cy) {
                t.held = true;
                return Effect::None;
            }
            let was = t.source;
            self.menu = None;
            if let Some(i) = self.title_at(label, env, cx, cy)
                && was != MenuSource::Bar(i)
            {
                self.open_bar_menu(i, label, env);
            }
            return Effect::None;
        }
        if self.zoom_click(cx, cy, label) {
            return Effect::None;
        }
        if self.zoom_edit.is_some() && !self.commit_zoom() {
            return Effect::None;
        }
        if !self.native_scrollbars && self.horizontal_bar().contains(cx, cy) {
            let v = self.main_view();
            let max = v.max_scroll_x();
            let frac = if max > 0.0 { v.scroll_x / max } else { 0.0 };
            if max > 0.0
                && let Some(part) = chrome::scroll_hit(self.horizontal_bar(), frac, false, cx, cy)
            {
                self.horizontal_press = Some((part, cx, v.scroll_x));
                self.horizontal_step(part);
            }
            return Effect::None;
        }
        if let Some(effect) = self.palette_click(cx, cy, env, clip) {
            return effect;
        }
        if self.review_divider_hit(cx, cy) {
            self.review_resize = self.revisions_bounds().map(|r| cx - r.x);
            self.review_resize_text_width = self.pane_view().map(|v| v.area.w).filter(|w| *w > 0.0);
            return Effect::None;
        }
        if let Some(effect) = self.revision_click(cx, cy) {
            return effect;
        }
        if self.own_menu_bar && cy < MENU_BAR_HEIGHT {
            if let Some(i) = self.title_at(label, env, cx, cy) {
                self.open_bar_menu(i, label, env);
            }
            return Effect::None;
        }
        if let Some(press) = self.toolbar_hit(cx, cy) {
            // The Toolbar Menu button drops its menu rather than waiting
            // for the release.
            if let Some(i) = press
                && toolbar::command(i) == Some(Cmd::ToolbarMenu)
            {
                self.open_toolbar_menu(i, label);
                return Effect::None;
            }
            self.toolbar_press = press;
            return Effect::None;
        }

        if self.show_formatting_toolbar && cy < self.paragraph_toolbar_top() {
            if let Some(hit) = bars::formatting_toolbar_hit(self.formatting_toolbar_top(), cx, cy) {
                return self.bar_click(hit, label, env, clip);
            }
            return Effect::None;
        }
        if self.show_paragraph_toolbar && cy < self.ruler_top() {
            if let Some(hit) = bars::paragraph_toolbar_hit(self.paragraph_toolbar_top(), cx, cy) {
                return self.bar_click(hit, label, env, clip);
            }
            return Effect::None;
        }
        if self.main_view().outline && cy < self.text_top() {
            if let Some(cmd) = crate::outline_bar::hit(self.ruler_top(), cx, cy) {
                self.focus_main();
                return self.view.exec(cmd, false, clip);
            }
            return Effect::None;
        }
        if self.show_ruler && cy < self.text_top() {
            // The ruler sets indents, tabs and columns, which Markdown
            // source does not have.
            if bars::on_ruler(self.ruler_top(), cy) && !self.main_view().in_markdown_source() {
                let grab = bars::scale_grab(&self.ruler_state(), self.ruler_top(), cx, cy);
                // A double-click opens Tabs on a tab stop and Paragraph
                // on an indent marker.
                let now = Instant::now();
                let double = self.last_ruler_click.is_some_and(|(t, px, py)| {
                    now - t < Duration::from_millis(450)
                        && (px - cx).abs() < 4.0
                        && (py - cy).abs() < 4.0
                });
                self.last_ruler_click = Some((now, cx, cy));
                if double {
                    self.last_ruler_click = None;
                    let (d, which) = match grab {
                        ScaleGrab::Tab(pos) => format_dialogs::tabs(&self.view, Some(pos)),
                        ScaleGrab::NewTab => return Effect::None,
                        _ => format_dialogs::paragraph(&self.view),
                    };
                    self.dialogs.push((which, d));
                    return Effect::None;
                }
                let from = cx;
                let (start, rows) = if let ScaleGrab::Column(_) = grab {
                    (Vec::new(), self.view.begin_column_drag())
                } else {
                    (self.view.begin_ruler_drag(), Vec::new())
                };
                self.scale_drag = Some(ScaleDrag {
                    grab,
                    start,
                    rows,
                    from,
                });
                self.scale_drag_to(cx, cy);
            }
            return Effect::None;
        }
        if let Some((bounds, story, section)) =
            self.pane.as_ref().map(|p| (p.bounds, p.story, p.section))
            && bounds.contains(cx, cy)
        {
            let same = self.same_as_previous_allowed();
            match pane::hit(bounds, story, same, cx, cy) {
                PaneHit::Close => self.close_pane(),
                PaneHit::Chrome => {}
                PaneHit::SameAsPrevious => {
                    // Back to the main text, link, and show what it shares.
                    self.close_pane();
                    let v = &mut self.view;
                    let sel = v.sel;
                    v.doc.begin("Same As Previous", sel);
                    v.doc.link_headers(section);
                    v.doc.commit(sel);
                    self.open_pane_in(story, section);
                }
                PaneHit::Field(ch) => {
                    self.focus_pane();
                    self.view.type_text(&ch.to_string());
                }
                PaneHit::Text => {
                    self.focus_pane();
                    let max = self.view.max_scroll();
                    let frac = if max > 0.0 {
                        self.view.scroll_y / max
                    } else {
                        0.0
                    };
                    if !self.native_scrollbars
                        && let Some(part) =
                            chrome::scroll_hit(self.vscroll_other, frac, true, cx, cy)
                    {
                        if max > 0.0 {
                            self.scroll_press = Some((part, cy, self.view.scroll_y));
                            scroll_step(&mut self.view, part);
                        }
                    } else if self.view.area.contains(dx, dy) {
                        self.view.mouse_down(dx, dy, mods.shift, mods.cmd);
                    }
                }
            }
            return Effect::None;
        }
        self.focus_main();
        if self.position_frames
            && let Some((index, page, rect, px, py)) = self.view.frame_position_hit(dx, dy)
        {
            self.frame_drag = Some(FrameDrag {
                index,
                page,
                grab: (px - rect.x, py - rect.y),
                target: rect,
            });
            return Effect::None;
        }
        // The split box above the scroll bar, and the bar between panes.
        if self.pane.is_none() {
            let top = self.text_top();
            let on_box = !self.native_scrollbars
                && self.split.is_none()
                && Rect::new(self.width - BAR, top, BAR, SPLIT_BOX).contains(cx, cy);
            let on_divider = self.active_rects().is_some_and(|(a, o)| {
                let y = a.y.min(o.y) + if a.y < o.y { a.h } else { o.h };
                cy >= y && cy < y + DIVIDER
            });
            if on_box || on_divider {
                let double = self
                    .last_split_click
                    .is_some_and(|t| t.elapsed() < Duration::from_millis(450));
                self.last_split_click = Some(Instant::now());
                if double {
                    // Double-clicking splits in half, or joins the panes.
                    self.toggle_split();
                    self.last_split_click = None;
                } else {
                    self.split_drag = Some(cy);
                }
                return Effect::None;
            }
            // A click in the other pane makes it the active one.
            if let Some((_, o)) = self.active_rects()
                && (o.contains(cx, cy) || self.vscroll_other.contains(cx, cy))
            {
                self.switch_pane();
            }
        }
        // In Page Layout view a click in a page's header or footer opens it.
        if cx < self.width - self.right_gutter()
            && let Some((story, section)) = self.view.header_at(dx, dy)
        {
            self.open_pane_in(story, section);
            return Effect::None;
        }
        // ...and a click on a footnote opens the footnote pane at it.
        if cx < self.width - self.right_gutter()
            && let Some(note) = self.view.note_at(dx, dy)
        {
            self.open_note(note);
            return Effect::None;
        }
        let max = self.view.max_scroll();
        let frac = if max > 0.0 {
            self.view.scroll_y / max
        } else {
            0.0
        };
        if !self.native_scrollbars
            && let Some(part) = chrome::scroll_hit(self.vscroll, frac, true, cx, cy)
        {
            if max > 0.0 {
                self.scroll_press = Some((part, cy, self.view.scroll_y));
                scroll_step(&mut self.view, part);
            }
        } else if self.view.area.contains(dx, dy) {
            self.view.mouse_down(dx, dy, mods.shift, mods.cmd);
            if self.view.change_here().is_some() {
                self.review_closed = false;
                if self.review_filter == revisions::Filter::Comments {
                    self.review_filter = revisions::Filter::All;
                }
            }
        }
        Effect::None
    }

    fn bar_click(
        &mut self,
        hit: Hit,
        label: &mut Label,
        env: &Env,
        clip: &mut Clipboard,
    ) -> Effect {
        // The bars only format, and Markdown source takes no formatting.
        if self.main_view().in_markdown_source() {
            return Effect::None;
        }
        let cmd = match hit {
            Hit::FontPopup | Hit::SizePopup | Hit::ColorPopup | Hit::StylePopup => {
                self.open_popup(hit, label, env);
                return Effect::None;
            }
            Hit::Bold => Cmd::Bold,
            Hit::Italic => Cmd::Italic,
            Hit::Underline => Cmd::Underline,
            Hit::Superscript => Cmd::Superscript,
            Hit::Subscript => Cmd::Subscript,
            Hit::Tool(name) => match bars::tool_command(name) {
                Some(cmd) => cmd,
                // Graphs are not built yet.
                None => return Effect::None,
            },
            Hit::Align(quill_core::Align::Left) => Cmd::AlignLeft,
            Hit::Align(quill_core::Align::Center) => Cmd::AlignCenter,
            Hit::Align(quill_core::Align::Right) => Cmd::AlignRight,
            Hit::Align(quill_core::Align::Justify) => Cmd::Justify,
            Hit::Spacing(i) => {
                let line = match i {
                    0 => LineSpacing::Auto,
                    1 => LineSpacing::AtLeast(360),
                    _ => LineSpacing::AtLeast(480),
                };
                self.view
                    .format_paras("Paragraph Formatting", move |p| p.line = line);
                return Effect::None;
            }
            Hit::SpaceBefore(open) => {
                self.view.format_paras("Paragraph Formatting", move |p| {
                    p.before = if open { 240 } else { 0 }
                });
                return Effect::None;
            }
            Hit::TabType(t) => {
                self.tab_type = t;
                return Effect::None;
            }
            Hit::Columns(n) => {
                self.view.format_sections(move |s| s.columns = n as u16);
                return Effect::None;
            }
            // The table and page scales are not built yet.
            _ => return Effect::None,
        };
        self.view.exec(cmd, false, clip)
    }

    fn scale_drag_to(&mut self, x: f32, y: f32) {
        let Some(drag) = &self.scale_drag else {
            return;
        };
        let rs = self.ruler_state();
        let at = bars::snap(rs.text_left, rs.inch, x);
        let scale_top = self.ruler_top();
        let off = y > scale_top + SCALE_HEIGHT + 12.0 || y < scale_top - 12.0;
        let grab = drag.grab;
        if let ScaleGrab::Column(edge) = grab {
            let (rows, from) = (drag.rows.clone(), drag.from);
            let dx = bars::snap(from, rs.inch, x);
            self.view.column_drag(&rows, edge, dx);
            return;
        }
        let kind = self.tab_type;
        let width = self.view.caret_column_twips();
        let start = drag.start.clone();
        self.view.ruler_drag(&start, |p, st| {
            bars::apply_grab(p, st, grab, at, kind, off, width)
        });
    }

    /// The pointer moved. Returns true when the window needs redrawing.
    pub fn mouse_move(&mut self, x: f32, y: f32, label: &mut Label, env: &Env) -> bool {
        let (cx, cy) = (x / self.ui, y / self.ui);
        let (dx, dy) = self.document_point(x, y);
        self.pointer = (cx, cy);
        if self.palette_move(cx, cy) {
            return true;
        }
        if self.review_resize.is_some() {
            self.resize_review(cx);
            return true;
        }
        if let Some(d) = &self.frame_drag {
            let (px, py) = self.main_view().frame_position_point(d.page, dx, dy);
            let (pw, ph) = self.main_view().page_size();
            if let Some(d) = &mut self.frame_drag {
                d.target.x = (px - d.grab.0).clamp(0.0, (pw - d.target.w).max(0.0));
                d.target.y = (py - d.grab.1).clamp(0.0, (ph - d.target.h).max(0.0));
            }
            return true;
        }

        if let Some((_, d)) = self.dialogs.last_mut()
            && (self.dialog_mouse || !d.modeless)
        {
            let o = d.mouse_move(cx, cy, label);
            self.slider_moved(o);
            return true;
        }
        if let Some(t) = &self.menu {
            // Sliding along the bar switches menus.
            if let MenuSource::Bar(cur) = t.source
                && let Some(i) = self.title_at(label, env, cx, cy)
                && i != cur
            {
                let held = t.held;
                self.open_bar_menu(i, label, env);
                if let Some(t) = &mut self.menu {
                    t.held = held;
                }
            }
            if let Some(t) = &mut self.menu {
                let m = &t.menus[t.index()];
                if let Some((parent, child_open)) = &mut t.submenu
                    && let Some(child) = m.items[*parent].submenu.as_ref()
                    && child_open.rect.contains(cx, cy)
                {
                    menu::autoscroll(child, child_open, cy);
                    child_open.hot = menu::item_at(child, child_open, cx, cy);
                    t.open.hot = Some(*parent);
                    return true;
                }
                menu::autoscroll(m, &mut t.open, cy);
                let hot = menu::item_at(m, &t.open, cx, cy);
                t.open.hot = hot;
                if let Some(i) = hot
                    && let Some(child) = m.items[i].submenu.as_ref()
                    && let Some(parent) = menu::item_rect(m, &t.open, i)
                {
                    if t.submenu.as_ref().map(|s| s.0) != Some(i) {
                        let bounds = Rect::new(0.0, 0.0, self.width, self.height);
                        t.submenu = Some((i, menu::place_side(label, child, parent, bounds)));
                    }
                } else {
                    t.submenu = None;
                }
            }
            return true;
        }
        if let Some((ScrollPart::Thumb, start, scroll)) = self.horizontal_press {
            let span = (self.horizontal_bar().w - 3.0 * BAR).max(1.0);
            let max = self.main_view().max_scroll_x();
            self.main_view_mut().scroll_x = (scroll + (cx - start) / span * max).clamp(0.0, max);
            return true;
        }
        // The toolbars and ruler light the control under the pointer.
        let hover = self.toolbar_button_at(cx, cy);
        let bar = self.bar_hit(cx, cy);
        let badge = self
            .revision_badges
            .iter()
            .position(|b| b.rect.contains(cx, cy));
        let badge_tip_changed = badge
            .and_then(|i| self.revision_badges.get(i))
            .is_some_and(|b| self.tip.as_ref().is_none_or(|t| t.text != b.tip));
        let lit = hover != self.toolbar_hover
            || bar != self.bar_hover
            || badge != self.revision_hover
            || badge_tip_changed;
        self.revision_hover = badge;
        self.toolbar_hover = hover;
        self.bar_hover = bar;
        if lit {
            self.retip(cx, cy);
        }
        if self.scale_drag.is_some() {
            self.scale_drag_to(cx, cy);
            return true;
        }
        if self.split_drag.is_some() {
            let top = self.text_top();
            self.split_drag = Some(cy.clamp(top, self.height - BAR));
            return true;
        }
        if let Some((ScrollPart::Thumb, start_y, start_scroll)) = self.scroll_press {
            let bar = if self.pane.as_ref().is_some_and(|p| p.focused) {
                self.vscroll_other
            } else {
                self.vscroll
            };
            let span = (bar.h - 3.0 * BAR).max(1.0);
            let max = self.view.max_scroll();
            self.view.scroll_y = (start_scroll + (cy - start_y) / span * max).clamp(0.0, max);
            return true;
        }
        if self.view.dragging() {
            let area = self.view.area;
            self.view.mouse_drag(
                dx.clamp(area.x, area.right() - 1.0),
                dy.clamp(area.y, area.bottom() - 1.0),
            );
            return true;
        }
        lit
    }

    /// The pointer went up. Returns the effect and, for the Window menu, the
    /// window to bring forward.
    pub fn mouse_up(&mut self, x: f32, y: f32, clip: &mut Clipboard) -> (Effect, Option<usize>) {
        let (x, y) = (x / self.ui, y / self.ui);
        if self.review_resize.is_some() {
            self.resize_review(x);
            self.review_resize = None;
            self.review_resize_text_width = None;
            return (Effect::None, None);
        }
        if let Some(d) = self.frame_drag.take() {
            self.main_view_mut()
                .move_frame(d.index, d.target.x, d.target.y);
            self.position_frames = false;
            self.main_view_mut().status = None;
            return (Effect::None, None);
        }

        if let Some(i) = self.toolbar_press.take() {
            let r = self.toolbar_hit(x, y).flatten();
            if r == Some(i)
                && let Some(cmd) = toolbar::command(i)
            {
                return (self.view.exec(cmd, false, clip), None);
            }
            return (Effect::None, None);
        }
        if let Some((_, d)) = self.dialogs.last_mut()
            && (self.dialog_mouse || !d.modeless)
        {
            self.dialog_mouse = false;
            let o = d.mouse_up(x, y);
            self.dialog_outcome(o, clip);
            return (Effect::None, None);
        }
        if let Some(y) = self.split_drag.take() {
            // A click on the split box without a drag leaves things as they
            // are.
            if self.split.is_some() || y > self.text_top() + SPLIT_BOX {
                self.finish_split_drag(y);
            }
            return (Effect::None, None);
        }
        self.view.mouse_up();
        self.scroll_press = None;
        self.horizontal_press = None;
        if self.scale_drag.take().is_some() {
            self.view.end_ruler_drag();
        }
        let Some(t) = &mut self.menu else {
            return (Effect::None, None);
        };
        if let Some((parent, child_open)) = &t.submenu
            && let Some(child) = t.menu().items[*parent].submenu.as_ref()
            && let Some(i) = menu::item_at(child, child_open, x, y)
        {
            let id = child.items[i].id as usize;
            let action = t.actions.get(id).cloned().unwrap_or(Action::None);
            self.menu = None;
            return (self.act(action, clip), None);
        }
        if let Some(i) = menu::item_at(t.menu(), &t.open, x, y) {
            let id = t.menu().items[i].id as usize;
            let action = t.actions.get(id).cloned().unwrap_or(Action::None);
            self.menu = None;
            if let Action::Window(n) = action {
                return (Effect::None, Some(n));
            }
            return (self.act(action, clip), None);
        }
        // A quick click on the title leaves the menu open (sticky menus);
        // letting go anywhere else after holding the button closes it.
        let quick = t.opened.elapsed() < Duration::from_millis(400);
        let off_menu = !t.open.rect.contains(x, y);
        if t.held && quick && off_menu {
            t.held = false;
        } else if t.held && off_menu {
            self.menu = None;
        } else {
            t.held = false;
        }
        (Effect::None, None)
    }

    /// Whether a window point is on the grow box in the bottom-right corner.
    fn in_grow_box(&self, x: f32, y: f32) -> bool {
        let (cx, cy) = (x / self.ui, y / self.ui);
        cx >= self.width - BAR && cy >= self.height - BAR
    }

    fn cursor_icon(&self, x: f32, y: f32) -> CursorIcon {
        let (cx, cy) = (x / self.ui, y / self.ui);
        if self.review_resize.is_some() || self.review_divider_hit(cx, cy) {
            return CursorIcon::ColResize;
        }
        if self.revision_badges.iter().any(|b| b.rect.contains(cx, cy)) {
            return CursorIcon::Pointer;
        }
        if let Some(r) = self.zoom_box()
            && r.contains(cx, cy)
            && cx < r.right() - 24.0
        {
            return CursorIcon::Text;
        }
        let (dx, dy) = self.document_point(x, y);
        let in_text = self.menu.is_none()
            && self
                .dialogs
                .last()
                .is_none_or(|(_, d)| d.modeless && !d.contains(cx, cy))
            && self.view.area.contains(dx, dy);
        if let Some(handle) = self.view.picture_cursor(dx, dy)
            && (in_text || self.view.dragging())
        {
            return match handle {
                Handle::Left | Handle::Right => CursorIcon::EwResize,
                Handle::Top | Handle::Bottom => CursorIcon::NsResize,
                Handle::TopLeft | Handle::BottomRight => CursorIcon::NwseResize,
                Handle::TopRight | Handle::BottomLeft => CursorIcon::NeswResize,
            };
        }
        if in_text && let Some(kind) = self.view.border_cursor(dx, dy) {
            return match kind {
                BorderKind::Column => CursorIcon::ColResize,
                BorderKind::Row => CursorIcon::RowResize,
            };
        }
        if in_text && dx >= self.view.text_left_window() - 2.0 {
            CursorIcon::Text
        } else {
            CursorIcon::Default
        }
    }

    /// Called on a timer while a scroll arrow or track is held. Returns true when
    /// something moved.
    fn selection_scroll(&mut self, advance: bool) -> bool {
        if !self.view.dragging() || !self.dialogs.is_empty() || self.menu.is_some() {
            return false;
        }
        let (x, y) = self.document_point(self.pointer.0 * self.ui, self.pointer.1 * self.ui);
        let area = self.view.area;
        if area.w <= 1.0 || area.h <= 1.0 {
            return false;
        }
        let outside = |p: f32, a: f32, b: f32| {
            if p < a {
                p - a
            } else if p > b {
                p - b
            } else {
                0.0
            }
        };
        let dx = outside(x, area.x, area.right());
        let dy = outside(y, area.y, area.bottom());
        if dx == 0.0 && dy == 0.0 {
            return false;
        }
        if advance {
            // Faster the further past the edge, but never a crawl: a window
            // that reaches the bottom of the screen leaves the pointer only
            // a point or two below it.
            let step = |d: f32| {
                if d == 0.0 {
                    0.0
                } else {
                    d.signum() * (d.abs() + 10.0).min(80.0)
                }
            };
            self.view.scroll_sideways(step(dx));
            self.view.scroll_by(step(dy));
            self.view.mouse_drag(
                x.clamp(area.x, area.right() - 1.0),
                y.clamp(area.y, area.bottom() - 1.0),
            );
        }
        true
    }

    fn tick(&mut self) -> bool {
        let mut spelling_changed = self.view.check_live_spelling();
        if let Some(pane) = &mut self.pane {
            spelling_changed |= pane.other.check_live_spelling();
        }
        if let Some((part, _, _)) = self.horizontal_press
            && part != ScrollPart::Thumb
        {
            self.horizontal_step(part);
            return true;
        }
        if self.selection_scroll(true) {
            return true;
        }
        if let Some(t) = &mut self.tip
            && !t.shown
            && Instant::now() >= t.due
        {
            t.shown = true;
            return true;
        }
        if let Some((part, _, _)) = self.scroll_press
            && part != ScrollPart::Thumb
        {
            scroll_step(&mut self.view, part);
            return true;
        }
        spelling_changed
    }
}

/// How long the pointer rests on a control before its tooltip shows.
pub(crate) const TIP_DELAY: Duration = Duration::from_millis(600);

/// A tooltip waiting to show, or showing.
struct Tip {
    text: String,
    /// The pointer when it reached the control, in interface units.
    x: f32,
    y: f32,
    due: Instant,
    shown: bool,
}

impl WinState {
    /// The pointer reached a different control (or none): start that
    /// control's tooltip. Moving from one showing tip to the next control
    /// shows the next at once, as system tooltips do.
    fn retip(&mut self, x: f32, y: f32) {
        let text = self
            .revision_hover
            .and_then(|i| self.revision_badges.get(i).map(|b| b.tip.clone()))
            .or_else(|| match (self.toolbar_hover, self.bar_hover) {
                (Some(i), _) => toolbar::tip(i),
                (None, Some(hit)) => bars::tip(hit),
                (None, None) => None,
            });
        let quick = self.tip.as_ref().is_some_and(|t| t.shown);
        let now = Instant::now();
        self.tip = text.map(|text| Tip {
            text,
            x,
            y,
            due: if quick { now } else { now + TIP_DELAY },
            shown: quick,
        });
    }
}

struct DocWindow {
    #[cfg(target_os = "macos")]
    print_settings: crate::mac_print::Settings,
    // Drop a native child panel before its parent winit window.
    #[cfg(target_os = "macos")]
    dialog: Option<crate::mac_dialog::NativeDialog>,
    #[cfg(windows)]
    dialog: Option<crate::win_dialog::NativeDialog>,
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    st: WinState,
    mods: ModifiersState,
    cursor: (f32, f32),
    focused: bool,
    /// Windows made with Window > New Window share a document and a
    /// group number (0 for a window on its own). Only one of them holds
    /// the document at a time; it moves to whichever window is being used
    /// or drawn, and the others keep their scroll position and selection
    /// in `port`.
    group: u32,
    holds: bool,
    port: Port,
    /// Atomic native-format autosave for the document currently held here.
    recovery: Option<PathBuf>,
    #[cfg(target_os = "macos")]
    scrollbars: Option<crate::mac_widgets::MacScrollbars>,
}

/// Move a shared document from window state `a` to `b`. `a` keeps its
/// scroll position and selection in `a_port`; `b` gets back what it kept
/// in `b_port`.
fn hand_over(a: &mut WinState, a_port: &mut Port, b: &mut WinState, b_port: &mut Port) {
    std::mem::swap(a.main_view_mut(), b.main_view_mut());
    let v = b.main_view_mut();
    v.swap_port(a_port);
    v.swap_port(b_port);
}

/// Events the app sends itself.
#[derive(Debug)]
pub enum UserEvent {
    #[cfg(target_os = "linux")]
    LinuxMenu { window: WindowId, action: Action },
    #[cfg(target_os = "linux")]
    LinuxClose(WindowId),
    #[cfg(target_os = "linux")]
    LinuxFocus(WindowId),
    /// A choice from the system menu bar, by item id.
    #[cfg(target_os = "macos")]
    Menu(String),
    /// The Finder sent documents to open, waiting in `mac_open::take`.
    #[cfg(target_os = "macos")]
    OpenFiles,
    /// Quit from outside Quill (the Dock, logging out) with work unsaved.
    #[cfg(target_os = "macos")]
    Quit,
    /// Text the Mac sent without a key press (Emoji & Symbols, dictation),
    /// to type into the window in front.
    #[cfg(target_os = "macos")]
    InsertText(String),
    /// A background PDF job completed another page.
    PrintProgress {
        window: WindowId,
        done: usize,
        total: usize,
    },
    /// A background PDF job finished writing or submitting its output.
    PrintFinished {
        window: WindowId,
        result: Result<(), String>,
        #[cfg(target_os = "macos")]
        native: bool,
    },
    #[cfg(target_os = "macos")]
    NativePrintReady {
        window: WindowId,
        bytes: Vec<u8>,
        title: String,
        props: quill_core::DocumentProps,
    },
    #[cfg(target_os = "macos")]
    PageSetupFinished {
        window: WindowId,
        result: Result<Option<(i32, i32)>, String>,
        document: bool,
    },
    /// An AppKit scrollbar moved. `scroller` is its stable object address.
    #[cfg(target_os = "macos")]
    NativeScroll {
        scroller: usize,
        part: usize,
        value: f64,
    },
    #[cfg(windows)]
    WindowsShortcut { panel: usize, spec: Option<String> },
    #[cfg(windows)]
    WindowsDialog { panel: usize, id: &'static str },
    /// A control in a native AppKit dialog changed or was pressed.
    #[cfg(target_os = "macos")]
    NativeDialog {
        panel: usize,
        id: &'static str,
        choice: Option<(usize, bool, bool)>,
    },
    /// A key captured by Tools > Commands' native Add shortcut mode.
    #[cfg(target_os = "macos")]
    NativeShortcut { panel: usize, spec: String },
    /// Escape canceled Tools > Commands' native Add shortcut mode.
    #[cfg(target_os = "macos")]
    NativeShortcutCancel { panel: usize },
}

pub struct App {
    #[cfg(target_os = "macos")]
    preferences: Option<preferences::ApplicationPreferences>,
    /// AppKit permits one active print operation per thread. Also reserve
    /// this while its PDF is prepared, so repeated commands cannot queue sheets.
    #[cfg(target_os = "macos")]
    native_print_window: Option<WindowId>,
    windows: Vec<(WindowId, DocWindow)>,
    /// GTK polling must not turn editor timers into continuous repainting.
    #[cfg(target_os = "linux")]
    editor_wake: Option<Instant>,
    /// The window in front, which menu commands act on.
    front: Option<WindowId>,
    /// The Appearance setting the windows were last given.
    appearance: Option<crate::settings::Appearance>,
    /// The Style Sheets palette's own window, while it shows undocked, and
    /// where it was when last closed.
    palette_window: Option<palette_window::PaletteWindow>,
    palette_at: Option<winit::dpi::PhysicalPosition<i32>>,
    /// Stands in for a document when no window is open, so the menus can
    /// still be built (File > New and Open must work).
    #[cfg(target_os = "macos")]
    blank: DocView,
    proxy: EventLoopProxy<UserEvent>,
    #[cfg(target_os = "macos")]
    native: Option<crate::native_menu::NativeMenu>,
    /// What each item of the last native context menu does.
    #[cfg(target_os = "macos")]
    context_actions: Vec<Action>,
    fonts_scan: Option<JoinHandle<Catalog>>,
    /// The pointer for cropping a picture, made once.
    crop_cursor: Option<CustomCursor>,
    /// The installed fonts alone, kept so the extra font folders can be
    /// switched on and off without scanning the system again.
    system_catalog: Option<Catalog>,
    /// The font folders and search depth `font_catalog` was built with.
    font_folders_loaded: FontFolders,
    /// Cheaply cloned paths and face metadata for independent worker fonts.
    font_catalog: Option<Catalog>,
    fonts: Option<Fonts>,
    font_names: Vec<String>,
    font_families: Vec<FontFamilyInfo>,
    cache: GlyphCache,
    clipboard: Option<Clipboard>,
    untitled: u32,
    /// The last group number given to windows sharing a document.
    groups: u32,
    /// Files named on the command line, opened once the event loop runs.
    pending: Vec<PathBuf>,
    recent: Vec<PathBuf>,
    last_recovery: Instant,
    recovery_serial: u64,
    recovered: bool,
    /// Whether the last session's documents have been offered at startup.
    reopened: bool,
}

/// The extra font folders that are switched on, and how deep to search them.
#[derive(Clone, Debug, PartialEq)]
struct FontFolders {
    paths: Vec<PathBuf>,
    levels: u8,
}

impl FontFolders {
    fn current() -> Self {
        crate::settings::with(|s| FontFolders {
            paths: s
                .font_folders
                .iter()
                .filter(|f| f.enabled)
                .map(|f| f.path.clone())
                .collect(),
            levels: s.font_folder_levels,
        })
    }

    fn apply(&self, system: &Catalog) -> Catalog {
        system.with_folders(&self.paths, self.levels)
    }
}

impl App {
    pub fn new(
        fonts_scan: JoinHandle<Catalog>,
        files: Vec<PathBuf>,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Self {
        App {
            #[cfg(target_os = "macos")]
            preferences: None,
            #[cfg(target_os = "macos")]
            native_print_window: None,
            front: None,
            appearance: None,
            palette_window: None,
            palette_at: None,
            #[cfg(target_os = "macos")]
            blank: DocView::new(crate::settings::get().new_document(), String::new()),
            proxy,
            #[cfg(target_os = "macos")]
            native: None,
            #[cfg(target_os = "macos")]
            context_actions: Vec::new(),
            pending: files,
            recent: crate::recent::load(),
            windows: Vec::new(),
            #[cfg(target_os = "linux")]
            editor_wake: None,
            fonts_scan: Some(fonts_scan),
            crop_cursor: None,
            system_catalog: None,
            font_folders_loaded: FontFolders::current(),
            font_catalog: None,
            fonts: None,
            font_names: Vec::new(),
            font_families: Vec::new(),
            cache: GlyphCache::new(),
            clipboard: None,
            untitled: 0,
            groups: 0,
            last_recovery: Instant::now(),
            recovery_serial: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64),
            recovered: false,
            reopened: false,
        }
    }

    fn ensure_fonts(&mut self) {
        if self.fonts.is_none() {
            let system = self
                .fonts_scan
                .take()
                .and_then(|h| h.join().ok())
                .unwrap_or_default();
            let folders = FontFolders::current();
            let catalog = folders.apply(&system);
            crate::font_names::set(&catalog);
            self.system_catalog = Some(system);
            self.font_folders_loaded = folders;
            self.font_catalog = Some(catalog.clone());
            let fonts = Fonts::new(catalog);
            self.font_names = fonts.family_names().iter().map(|s| s.to_string()).collect();
            self.font_families = fonts.family_info();
            self.fonts = Some(fonts);
        }
    }

    /// Preferences > Advanced switched a font folder on or off, or changed
    /// how deep to search: rebuild the font list and lay every document out
    /// again with it.
    fn refresh_font_folders(&mut self) {
        let folders = FontFolders::current();
        if folders == self.font_folders_loaded {
            return;
        }
        let Some(system) = &self.system_catalog else {
            // Fonts aren't loaded yet; they will pick the folders up.
            self.font_folders_loaded = folders;
            return;
        };
        let catalog = folders.apply(system);
        crate::font_names::set(&catalog);
        self.font_folders_loaded = folders;
        let fonts = Fonts::new(catalog.clone());
        self.font_names = fonts.family_names().iter().map(|s| s.to_string()).collect();
        self.font_families = fonts.family_info();
        self.font_catalog = Some(catalog);
        self.fonts = Some(fonts);
        self.cache = GlyphCache::new();
        for (_, w) in &mut self.windows {
            w.st.view.invalidate_layout();
            if let Some(pane) = &mut w.st.pane {
                pane.other.invalidate_layout();
            }
            w.window.request_redraw();
        }
    }

    /// When the next recovery save is due. With autosave off it still looks
    /// again every 30 seconds, to notice the setting being turned back on.
    fn next_recovery(&self) -> Instant {
        let secs = crate::settings::with(|s| s.autosave_secs);
        self.last_recovery + Duration::from_secs(if secs == 0 { 30 } else { secs.into() })
    }

    fn index(&self, id: WindowId) -> Option<usize> {
        self.windows.iter().position(|(w, _)| *w == id)
    }

    fn window_list(&self, current: WindowId) -> Vec<(String, bool)> {
        (0..self.windows.len())
            .map(|i| (self.display_title(i), self.windows[i].0 == current))
            .collect()
    }

    /// The window holding the document that window `i` shows.
    fn holder(&self, i: usize) -> usize {
        let g = self.windows[i].1.group;
        if g == 0 || self.windows[i].1.holds {
            return i;
        }
        self.windows
            .iter()
            .position(|(_, w)| w.group == g && w.holds)
            .unwrap_or(i)
    }

    /// A window's title: the document's, numbered ":1", ":2" when more
    /// than one window shows it, as in the original.
    fn display_title(&self, i: usize) -> String {
        let title = self.windows[self.holder(i)].1.st.main_view().title.clone();
        let g = self.windows[i].1.group;
        if g == 0 {
            return title;
        }
        let n = self.windows[..=i]
            .iter()
            .filter(|(_, w)| w.group == g)
            .count();
        format!("{title}:{n}")
    }

    fn retitle(&mut self) {
        for i in 0..self.windows.len() {
            let t = self.display_title(i);
            self.windows[i].1.window.set_title(&t);
        }
    }

    /// Move a shared document to window `i`, trading scroll positions and
    /// selections so each window keeps its own.
    fn take_doc(&mut self, i: usize) {
        let h = self.holder(i);
        if h == i {
            return;
        }
        let (a, b) = if h < i {
            let (x, y) = self.windows.split_at_mut(i);
            (&mut x[h].1, &mut y[0].1)
        } else {
            let (x, y) = self.windows.split_at_mut(h);
            (&mut y[0].1, &mut x[i].1)
        };
        hand_over(&mut a.st, &mut a.port, &mut b.st, &mut b.port);
        std::mem::swap(&mut a.recovery, &mut b.recovery);
        a.holds = false;
        b.holds = true;
    }

    /// Redraw the other windows on a document after it changed.
    fn redraw_group(&self, i: usize) {
        let g = self.windows[i].1.group;
        if g == 0 {
            return;
        }
        for (k, (_, w)) in self.windows.iter().enumerate() {
            if k != i && w.group == g {
                w.window.request_redraw();
            }
        }
    }

    /// Put the picture in file `path` at the window's insertion point.
    fn insert_picture_file(&mut self, id: WindowId, path: &std::path::Path) {
        let Some(i) = self.index(id) else {
            return;
        };
        match quill_formats::read_file_with_limit(path, crate::paste_special::MAX_PICTURE_BYTES) {
            Ok(bytes) => match quill_formats::images::picture(bytes) {
                Some(pic) => {
                    let w = &mut self.windows[i].1;
                    w.st.view.insert_picture(pic);
                    w.st.view.scroll_to_caret();
                    w.window.request_redraw();
                }
                None => alert(
                    &format!(
                        "Quill could not read the picture \u{201c}{}\u{201d}.",
                        file_name(path)
                    ),
                    "The file is not a PNG, JPEG, GIF, BMP, TIFF or WebP picture, or its picture data is damaged.",
                ),
            },
            Err(e) => alert(
                &format!(
                    "Quill could not open the picture \u{201c}{}\u{201d}.",
                    file_name(path)
                ),
                &e.to_string(),
            ),
        }
    }

    /// Window > New Window: another window on the same document.
    fn mirror(&mut self, el: &ActiveEventLoop, id: WindowId) {
        let Some(i) = self.index(id) else {
            return;
        };
        self.take_doc(i);
        let g = match self.windows[i].1.group {
            0 => {
                self.groups += 1;
                self.groups
            }
            g => g,
        };
        let (title, port, formatting, paragraph, ruler) = {
            let w = &mut self.windows[i].1;
            w.group = g;
            w.holds = true;
            let v = w.st.main_view();
            (
                v.title.clone(),
                Port {
                    scroll_y: v.scroll_y,
                    scroll_x: v.scroll_x,
                    sel: v.sel,
                    upstream: v.upstream,
                },
                w.st.show_formatting_toolbar,
                w.st.show_paragraph_toolbar,
                w.st.show_ruler,
            )
        };
        self.new_window(el, Document::new(), Some(title));
        if let Some((_, w)) = self.windows.last_mut() {
            w.group = g;
            w.holds = false;
            w.port = port;
            w.st.show_formatting_toolbar = formatting;
            w.st.show_paragraph_toolbar = paragraph;
            w.st.show_ruler = ruler;
        }
        self.retitle();
    }

    /// Remember where window `i` is and how big, for the next window.
    fn remember_frame(&self, i: usize) {
        let w = &self.windows[i].1.window;
        // A full-screen window's size is the screen's, not the one to reopen at.
        if w.fullscreen().is_some() {
            return;
        }
        let k = w.scale_factor();
        let Ok(at) = w.outer_position() else {
            return;
        };
        let at = at.to_logical::<f64>(k);
        #[cfg(target_os = "linux")]
        let size = w.frame_size().to_logical::<f64>(k);
        #[cfg(not(target_os = "linux"))]
        let size = w.inner_size().to_logical::<f64>(k);
        let frame = [at.x, at.y, size.width, size.height].map(|v| v.round() as i32);
        if crate::settings::get().window_frame != Some(frame) {
            crate::settings::update(|s| s.window_frame = Some(frame));
        }
    }

    /// Close a window. The last window on a document asks about saving
    /// first; returns false if the user cancels.
    /// The saved documents open now, once each, in window order.
    fn open_paths(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = Vec::new();
        for (_, w) in &self.windows {
            if let Some(p) = &w.st.main_view().path
                && w.recovery.is_none()
                && !paths.contains(p)
            {
                paths.push(p.clone());
            }
        }
        paths
    }

    /// If the pointer is on a link, ask whether to open it and do. False
    /// when there is no link there.
    fn ask_to_open_link(&mut self, id: WindowId) -> bool {
        let Some(i) = self.index(id) else {
            return false;
        };
        let w = &self.windows[i].1;
        let (x, y) = w.cursor;
        let Some(url) = w.st.link_at_point(x, y) else {
            return false;
        };
        if ask("Open this link?", &url, "Open")
            && let Some(i) = self.index(id)
        {
            self.windows[i].1.st.view.open_link(&url);
            self.windows[i].1.window.request_redraw();
        }
        true
    }

    /// Keeps the settings file's list of open documents current, so that
    /// after a crash Quill knows what was open. Not before startup has read
    /// the old list, and not after Quit has written the final one (its
    /// windows are gone by then).
    fn note_session(&self) {
        if !self.reopened || crate::settings::with(|s| s.quit_cleanly) {
            return;
        }
        let session = self.open_paths();
        if crate::settings::with(|s| s.last_session != session) {
            crate::settings::update(|s| s.last_session = session);
        }
    }

    fn close_window(&mut self, id: WindowId) -> bool {
        #[cfg(target_os = "macos")]
        if self.native_print_window == Some(id) {
            return false;
        }
        let Some(i) = self.index(id) else {
            return true;
        };
        let g = self.windows[i].1.group;
        let sibling = (g != 0)
            .then(|| {
                self.windows
                    .iter()
                    .position(|(w, x)| *w != id && x.group == g)
            })
            .flatten();
        match sibling {
            Some(k) => {
                // Hand the document to a window that stays open.
                self.take_doc(k);
                self.remember_frame(i);
                self.windows.retain(|(w, _)| *w != id);
                let left = self.windows.iter().filter(|(_, w)| w.group == g).count();
                if left == 1
                    && let Some((_, w)) = self.windows.iter_mut().find(|(_, w)| w.group == g)
                {
                    w.group = 0;
                }
                self.retitle();
                true
            }
            None => {
                if !self.confirm_close(id) {
                    return false;
                }
                crate::recovery::remove(self.windows[i].1.recovery.take());
                self.remember_frame(i);
                self.windows.retain(|(w, _)| *w != id);
                true
            }
        }
    }

    fn new_window(&mut self, el: &ActiveEventLoop, doc: Document, title: Option<String>) {
        let title = title.unwrap_or_else(|| {
            self.untitled += 1;
            format!("Untitled{}", self.untitled)
        });
        let size = el
            .primary_monitor()
            .map(|m| {
                let scale = m.scale_factor();
                let size = m.size().to_logical::<f64>(scale);
                initial_window_size(size.width, size.height)
            })
            .unwrap_or_else(|| LogicalSize::new(1100.0, 850.0));
        let mut attrs = Window::default_attributes()
            .with_title(&title)
            .with_inner_size(size)
            .with_min_inner_size(LogicalSize::new(480.0, 240.0));
        let mut top_left = None;
        if let Some(frame) = crate::settings::get().window_frame {
            let screens: Vec<[f64; 4]> = el
                .available_monitors()
                .map(|m| {
                    let k = m.scale_factor();
                    let at = m.position().to_logical::<f64>(k);
                    let size = m.size().to_logical::<f64>(k);
                    [at.x, at.y, size.width, size.height]
                })
                .collect();
            let taken: Vec<(f64, f64)> = self
                .windows
                .iter()
                .filter_map(|(_, w)| {
                    let at = w.window.outer_position().ok()?;
                    let at = at.to_logical::<f64>(w.window.scale_factor());
                    Some((at.x, at.y))
                })
                .collect();
            if let Some([x, y, w, h]) = restore_frame(frame, &screens, &taken) {
                attrs = attrs
                    .with_inner_size(LogicalSize::new(w, h))
                    .with_position(winit::dpi::LogicalPosition::new(x, y));
                top_left = Some(winit::dpi::LogicalPosition::new(x, y));
            }
        }
        // The Mac takes the icon from the app bundle; elsewhere the window
        // carries it.
        #[cfg(not(target_os = "macos"))]
        let attrs = attrs.with_window_icon(window_icon());
        // Wayland has no window icons: the desktop looks the icon up from
        // the application ID, which names quill.desktop (and its Icon=).
        #[cfg(target_os = "linux")]
        let attrs = {
            use winit::platform::wayland::WindowAttributesExtWayland;
            attrs.with_name("quill", "quill")
        };
        #[cfg(target_os = "linux")]
        let created = Window::create(el, attrs, self.proxy.clone());
        #[cfg(not(target_os = "linux"))]
        let created = el.create_window(attrs);
        let window = match created {
            Ok(w) => Rc::new(w),
            Err(e) => {
                eprintln!("quill: cannot open a window: {e}");
                return;
            }
        };
        // `with_position` places the Mac's content area, under the title bar,
        // but frames are saved and compared by the window's outer corner.
        if let Some(at) = top_left {
            window.set_outer_position(at);
        }
        let context = softbuffer::Context::new(window.clone()).expect("display connection");
        let surface = softbuffer::Surface::new(&context, window.clone()).expect("window surface");
        #[cfg(target_os = "macos")]
        let scrollbars = crate::mac_widgets::MacScrollbars::new(&window);
        // Input methods (Chinese, Japanese, Korean, dead-key accents, the
        // system's emoji panel) send their text to the window.
        window.set_ime_allowed(true);
        #[cfg(target_os = "macos")]
        crate::mac_text_input::install(&self.proxy);
        let id = window.id();
        let mut st = WinState::new(DocView::new(doc, title));
        #[cfg(target_os = "linux")]
        {
            st.own_menu_bar = !window.has_native_menu();
        }
        self.sync_appearance();
        set_window_appearance(&window, crate::settings::with(|s| s.appearance));
        st.dark = appearance_is_dark(&window);
        #[cfg(target_os = "macos")]
        {
            st.native_scrollbars = scrollbars.is_some();
            st.native_dialogs = false;
            st.on_screen = true;
        }
        self.windows.push((
            id,
            DocWindow {
                #[cfg(target_os = "macos")]
                print_settings: crate::mac_print::Settings::default(),
                #[cfg(any(target_os = "macos", windows))]
                dialog: None,
                window,
                surface,
                st,
                mods: ModifiersState::empty(),
                cursor: (0.0, 0.0),
                focused: true,
                group: 0,
                holds: false,
                port: Port::default(),
                recovery: None,
                #[cfg(target_os = "macos")]
                scrollbars,
            },
        ));
    }

    /// Give every window the Appearance setting, after Preferences changed
    /// it. Each window then draws dark or light to match.
    pub(super) fn sync_appearance(&mut self) {
        let want = crate::settings::with(|s| s.appearance);
        if self.appearance == Some(want) {
            return;
        }
        self.appearance = Some(want);
        #[cfg(target_os = "macos")]
        crate::mac_widgets::set_app_appearance(match want {
            crate::settings::Appearance::Auto => None,
            crate::settings::Appearance::Light => Some(false),
            crate::settings::Appearance::Dark => Some(true),
        });
        for (_, w) in &mut self.windows {
            set_window_appearance(&w.window, want);
            w.st.dark = appearance_is_dark(&w.window);
            w.window.request_redraw();
        }
        if let Some(p) = &self.palette_window {
            set_window_appearance(&p.window, want);
        }
        self.refresh_palette_window();
    }

    fn redraw(&mut self, id: WindowId) {
        self.sync_appearance();
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
        let w = &mut self.windows[i].1;
        #[cfg(target_os = "macos")]
        {
            let depth = w.st.dialogs.len();
            let top = w.st.dialogs.last().map(|(_, d)| d);
            let keep = match (&w.dialog, top) {
                (Some(native), Some(dialog)) => native.matches(depth, dialog),
                _ => false,
            };
            let keep = keep
                || match (&mut w.dialog, top) {
                    (Some(native), Some(dialog)) => native.resize_find(depth, dialog),
                    _ => false,
                };
            if !keep {
                // A window rebuilt in place stays where the person put it.
                let top_left = match (&w.dialog, top) {
                    (Some(native), Some(dialog)) => native.top_left_for(dialog),
                    _ => None,
                };
                w.dialog = None;
                if let Some(dialog) = top {
                    w.dialog =
                        crate::mac_dialog::NativeDialog::show(&w.window, depth, dialog, top_left);
                }
            }
            if let (Some(native), Some(dialog)) = (&mut w.dialog, top) {
                native.sync_from_model(dialog);
            }
            // A press outside the color pop-out put it away: forget it.
            if let Some(native) = &mut w.dialog
                && native.popover_dismissed()
            {
                if let Some((_, d)) = w.st.dialogs.last_mut() {
                    d.popout = None;
                }
                w.st.color_pop = None;
            }
            w.st.native_dialogs = w.dialog.is_some();
        }
        #[cfg(windows)]
        {
            let depth = w.st.dialogs.len();
            let top = w.st.dialogs.last().map(|(_, d)| d);
            if !matches!((&w.dialog, top), (Some(native), Some(dialog)) if native.matches(depth, dialog))
            {
                w.dialog = None;
                if let Some(dialog) = top {
                    w.dialog = crate::win_dialog::NativeDialog::show(
                        &w.window,
                        depth,
                        dialog,
                        self.proxy.clone(),
                    );
                }
            }
            if let (Some(native), Some(dialog)) = (&mut w.dialog, top) {
                native.sync_from_model(dialog);
            }
            w.st.native_dialogs = w.dialog.is_some();
        }
        let fonts = self.fonts.as_mut().unwrap();
        let size = w.window.inner_size();
        let (Some(pw), Some(ph)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        if w.surface.resize(pw, ph).is_err() {
            return;
        }
        let Ok(mut buffer) = w.surface.buffer_mut() else {
            return;
        };
        let scale = w.window.scale_factor() as f32;
        w.st.ui = ui_scale(scale);
        let mut c = Canvas::new(&mut buffer, size.width, size.height, scale);
        w.st.paint(&mut c, fonts, &mut self.cache, &env, w.focused);
        let _ = buffer.present();
        // An input method shows its choices by the insertion point.
        if w.focused {
            let (x, y, h) = w.st.view.caret_in_window();
            w.window.set_ime_cursor_area(
                winit::dpi::LogicalPosition::new(x, y),
                winit::dpi::LogicalSize::new(1.0, h),
            );
        }
        #[cfg(target_os = "macos")]
        if let Some(scrollbars) = &w.scrollbars {
            let max = w.st.main_view().max_scroll();
            let state = crate::mac_widgets::ScrollbarState {
                frame: w.st.vscroll,
                value: if max > 0.0 {
                    w.st.main_view().scroll_y / max
                } else {
                    0.0
                },
                proportion: if max > 0.0 {
                    w.st.main_view().area.h / (w.st.main_view().area.h + max)
                } else {
                    1.0
                },
                hidden: false,
                enabled: max > 0.0,
            };
            let other = if let Some(v) = w.st.pane_view() {
                let max = v.max_scroll();
                Some(crate::mac_widgets::ScrollbarState {
                    frame: w.st.vscroll_other,
                    value: if max > 0.0 { v.scroll_y / max } else { 0.0 },
                    proportion: v.area.h / (v.area.h + max).max(1.0),
                    hidden: false,
                    enabled: max > 0.0,
                })
            } else {
                w.st.split
                    .as_ref()
                    .map(|split| crate::mac_widgets::ScrollbarState {
                        frame: w.st.vscroll_other,
                        value: if max > 0.0 {
                            split.other_scroll / max
                        } else {
                            0.0
                        },
                        proportion: state.proportion,
                        hidden: false,
                        enabled: max > 0.0,
                    })
            };
            let v = w.st.main_view();
            let max = v.max_scroll_x();
            let horizontal = crate::mac_widgets::ScrollbarState {
                frame: w.st.horizontal_bar(),
                value: if max > 0.0 { v.scroll_x / max } else { 0.0 },
                proportion: v.area.w / (v.area.w + max).max(1.0),
                hidden: max <= 0.0 || w.st.horizontal_bar().w < 3.0 * BAR,
                enabled: max > 0.0,
            };
            scrollbars.set_dark(w.st.dark);
            scrollbars.sync(state, other, horizontal, w.st.ui);
        }
    }

    fn handle_effect(&mut self, el: &ActiveEventLoop, id: WindowId, effect: Effect) {
        self.effect(el, id, effect);
        // A dialog or command may have asked for printing, or for a new
        // window.
        if let Some(i) = self.index(id)
            && let Some(job) = self.windows[i].1.st.pending_print.take()
        {
            self.print(i, job);
        }
        if let Some(i) = self.index(id)
            && let Some((doc, title)) = self.windows[i].1.st.pending_window.take()
        {
            self.new_window(el, doc, Some(title));
        }
        // Tools > Commands: a settings file to open or save, or Do It.
        if let Some(i) = self.index(id)
            && let Some(open) = self.windows[i].1.st.pending_settings.take()
        {
            let dialog = rfd::FileDialog::new().set_title(if open {
                "Open Settings File"
            } else {
                "Save Settings As"
            });
            if open {
                if let Some(p) = dialog.pick_file() {
                    match crate::settings::read_text(&p) {
                        Ok(text) => crate::settings::update(|s| {
                            *s = crate::settings::Settings::from_text(&text)
                        }),
                        Err(e) => alert(
                            "Quill could not open the settings file.",
                            &format!("Could not read \u{201c}{}\u{201d}: {e}", p.display()),
                        ),
                    }
                }
            } else if let Some(p) = dialog.set_file_name("Quill Settings").save_file()
                && let Err(e) = crate::settings::write_user_file(
                    &p,
                    crate::settings::get().to_text().as_bytes(),
                )
            {
                alert("Quill could not save the settings file.", &e);
            }
            self.windows[i].1.window.request_redraw();
        }
        if let Some(i) = self.index(id)
            && let Some(cmd) = self.windows[i].1.st.pending_cmd.take()
        {
            let clip = self.clipboard.get_or_insert_with(Clipboard::new);
            let effect = self.windows[i].1.st.view.exec(cmd, false, clip);
            self.windows[i].1.window.request_redraw();
            self.handle_effect(el, id, effect);
        }
    }

    /// Print a window's document, or save it as PDF.
    fn print(&mut self, i: usize, job: crate::page_dialogs::PrintJob) {
        #[cfg(target_os = "macos")]
        if !job.pdf && self.native_print_window.is_some() {
            return;
        }
        self.ensure_fonts();
        let Some(catalog) = self.font_catalog.clone() else {
            return;
        };
        let window = self.windows[i].0;
        self.take_doc(i);
        let w = &mut self.windows[i].1;
        w.st.sync_pane();
        let (doc, title) = match w.st.print_doc.take() {
            Some(doc) => (doc, "Form Letters".to_string()),
            None => {
                let v = w.st.main_view();
                (v.doc.compact_for_save(), v.title.clone())
            }
        };
        #[cfg(windows)]
        if !job.pdf {
            let count = w.st.main_view().pages.len();
            let choice = match crate::win_print::Job::choose(&w.window, count) {
                Ok(Some(job)) => job,
                Ok(None) => return,
                Err(e) => {
                    alert("Quill could not open Print.", &e);
                    return;
                }
            };
            w.st.main_view_mut().status = Some("Preparing print job…".into());
            w.window.request_redraw();
            let proxy = self.proxy.clone();
            let spawn = std::thread::Builder::new()
                .name("quill-print".into())
                .spawn(move || {
                    let mut fonts = Fonts::new(catalog);
                    let mut cache = GlyphCache::new();
                    let mut view = DocView::new(doc, title).into_print();
                    view.update_layout(&mut fonts);
                    let progress = proxy.clone();
                    let result =
                        choice.print(&mut view, &mut fonts, &mut cache, move |done, total| {
                            let _ = progress.send_event(UserEvent::PrintProgress {
                                window,
                                done,
                                total,
                            });
                        });
                    let _ = proxy.send_event(UserEvent::PrintFinished { window, result });
                });
            if let Err(e) = spawn {
                w.st.main_view_mut().status = None;
                alert("Quill could not start printing.", &e.to_string());
            }
            return;
        }
        let title = strip_ext(&title).to_string();
        #[cfg(target_os = "macos")]
        let native_props = doc.props.clone();
        let target = if job.pdf {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Save as PDF")
                .set_file_name(format!("{title}.pdf"))
                .add_filter("PDF", &["pdf"]);
            if let Some(dir) = w.st.main_view().path.as_ref().and_then(|p| p.parent()) {
                dialog = dialog.set_directory(dir);
            }
            let Some(path) = dialog.save_file() else {
                return;
            };
            crate::print_job::Target::Save(path)
        } else {
            crate::print_job::Target::Print { copies: job.copies }
        };
        w.st.main_view_mut().status = Some("Preparing PDF\u{2026}".to_string());
        w.window.request_redraw();
        #[cfg(target_os = "macos")]
        if !job.pdf {
            self.native_print_window = Some(window);
        }
        let proxy = self.proxy.clone();
        let spawn = std::thread::Builder::new()
            .name("quill-pdf".to_string())
            .spawn(move || {
                let mut fonts = Fonts::new(catalog);
                let mut cache = GlyphCache::new();
                let mut view = DocView::new(doc, title).into_print();
                view.update_layout(&mut fonts);
                let n = view.pages.len();
                let range = match job.pages {
                    Some((from, to)) => (from - 1).min(n)..to.min(n),
                    None => 0..n,
                };
                let progress = proxy.clone();
                let bytes = crate::pdf::write_with_progress(
                    &mut view,
                    &mut fonts,
                    &mut cache,
                    range,
                    move |done, total| {
                        let _ = progress.send_event(UserEvent::PrintProgress {
                            window,
                            done,
                            total,
                        });
                    },
                );
                #[cfg(target_os = "macos")]
                if !job.pdf {
                    let _ = proxy.send_event(UserEvent::NativePrintReady {
                        window,
                        bytes,
                        title: view.title.clone(),
                        props: native_props,
                    });
                    return;
                }
                let result = crate::print_job::finish(target, &bytes);
                let _ = proxy.send_event(UserEvent::PrintFinished {
                    window,
                    result,
                    #[cfg(target_os = "macos")]
                    native: false,
                });
            });
        if let Err(e) = spawn {
            #[cfg(target_os = "macos")]
            if !job.pdf {
                self.native_print_window = None;
            }
            w.st.main_view_mut().status = None;
            alert("Quill could not start the PDF job.", &e.to_string());
        }
    }

    fn effect(&mut self, el: &ActiveEventLoop, id: WindowId, effect: Effect) {
        match effect {
            Effect::None => {}
            Effect::About => about(),
            Effect::Licenses => licenses(),
            Effect::ReplaceFonts => self.replace_missing_fonts(id),
            Effect::OpenRecent(index) => {
                if let Some(path) = self.recent.get(index).cloned() {
                    self.open_files(el, &[path]);
                }
            }
            Effect::RemoveRecent(index) => {
                crate::recent::remove(&mut self.recent, index);
                self.sync_menus();
            }
            Effect::ClearRecent => {
                crate::recent::clear(&mut self.recent);
                self.sync_menus();
            }
            Effect::NewWindow => self.new_window(el, crate::settings::get().new_document(), None),
            Effect::Dialog(d) => {
                #[cfg(windows)]
                if d == DialogId::Print {
                    if let Some(i) = self.index(id) {
                        self.print(
                            i,
                            crate::page_dialogs::PrintJob {
                                copies: 1,
                                pages: None,
                                pdf: false,
                            },
                        );
                    }
                    return;
                }
                #[cfg(target_os = "macos")]
                if matches!(d, DialogId::Print | DialogId::PageSetup) {
                    if let Some(i) = self.index(id) {
                        if d == DialogId::Print {
                            self.print(
                                i,
                                crate::page_dialogs::PrintJob {
                                    copies: 1,
                                    pages: None,
                                    pdf: false,
                                },
                            );
                        } else {
                            self.native_page_setup(i);
                        }
                    }
                    return;
                }
                let list = self.window_list(id);
                if let Some(i) = self.index(id) {
                    let env = Env {
                        font_names: &self.font_names,
                        font_families: &self.font_families,
                        recent: &self.recent,
                        windows: &list,
                    };
                    if d == DialogId::PasteSpecial {
                        let forms = self.clipboard.get_or_insert_with(Clipboard::new).forms();
                        self.windows[i].1.st.open_paste_special(forms);
                    } else {
                        self.windows[i].1.st.open_dialog(d, &env);
                    }
                    #[cfg(target_os = "macos")]
                    if matches!(d, DialogId::Find | DialogId::Replace)
                        && let Some(native) = &self.windows[i].1.dialog
                    {
                        native.activate_find();
                    }
                    if d == DialogId::ShowClipboard {
                        let text = self
                            .clipboard
                            .get_or_insert_with(Clipboard::new)
                            .fragment()
                            .map(|f| f.plain_text())
                            .unwrap_or_default();
                        self.windows[i].1.st.set_clipboard_text(text);
                    }
                    self.windows[i].1.window.request_redraw();
                }
            }
            Effect::Open => self.open_dialog(el, Some(id)),
            Effect::FindAgain => {
                if let Some(i) = self.index(id) {
                    self.windows[i].1.st.find_again();
                    self.windows[i].1.window.request_redraw();
                }
            }
            Effect::Split => {
                if let Some(i) = self.index(id) {
                    self.windows[i].1.st.toggle_split();
                    self.windows[i].1.window.request_redraw();
                }
            }
            Effect::Mirror => self.mirror(el, id),
            Effect::MarkdownSource => self.markdown_view(id, true),
            Effect::MarkdownWysiwyg => self.markdown_view(id, false),
            Effect::FullScreen => {
                if let Some(i) = self.index(id) {
                    let w = &self.windows[i].1.window;
                    // On the Mac, Borderless is the system's own full screen,
                    // in a space of its own, as the green button gives.
                    w.set_fullscreen(match w.fullscreen() {
                        Some(_) => None,
                        None => Some(winit::window::Fullscreen::Borderless(None)),
                    });
                }
            }
            Effect::LookUp =>
            {
                #[cfg(target_os = "macos")]
                if let Some(i) = self.index(id) {
                    let w = &self.windows[i].1;
                    let z = w.st.document_zoom;
                    match w.st.view.look_up_spot() {
                        Some((word, font, size, x, y)) => crate::mac_lookup::show(
                            &w.window,
                            &word,
                            &font,
                            (size * z) as f64,
                            (x * z) as f64,
                            (y * z) as f64,
                        ),
                        None => {
                            let w = &mut self.windows[i].1;
                            w.st.view.status =
                                Some("Put the insertion point in a word to look it up.".into());
                            w.window.request_redraw();
                        }
                    }
                }
            }
            Effect::Zoom(step) => {
                if let Some(i) = self.index(id) {
                    self.windows[i].1.st.zoom_step(step);
                    self.windows[i].1.window.request_redraw();
                }
            }
            Effect::ChooseMergeData => {
                let file = rfd::FileDialog::new()
                    .set_title("Open Data Document")
                    .add_filter("Documents", quill_formats::READABLE)
                    .pick_file();
                let list = self.window_list(id);
                if let (Some(path), Some(i)) = (file, self.index(id)) {
                    let env = Env {
                        font_names: &self.font_names,
                        font_families: &self.font_families,
                        recent: &self.recent,
                        windows: &list,
                    };
                    let st = &mut self.windows[i].1.st;
                    crate::merge_dialogs::attach_data(&mut st.view, &path);
                    st.open_dialog(DialogId::MergeHelper, &env);
                    self.windows[i].1.window.request_redraw();
                }
            }
            Effect::Pane(story) => {
                if let Some(i) = self.index(id) {
                    self.windows[i].1.st.open_pane(story);
                    self.windows[i].1.window.request_redraw();
                }
            }
            Effect::OpenNote(story, note) => {
                if let Some(i) = self.index(id) {
                    self.windows[i].1.st.open_note_in(story, note);
                    self.windows[i].1.window.request_redraw();
                }
            }
            Effect::ReplyToAnnotation | Effect::ResolveAnnotation => {
                if let Some(i) = self.index(id) {
                    let reply = matches!(effect, Effect::ReplyToAnnotation);
                    self.windows[i].1.st.annotation_command(reply);
                    self.windows[i].1.window.request_redraw();
                }
            }
            Effect::DeleteAnnotation => {
                if let Some(i) = self.index(id) {
                    self.windows[i].1.st.delete_annotation();
                    self.windows[i].1.window.request_redraw();
                }
            }
            Effect::InsertPicture => {
                let mut dialog = rfd::FileDialog::new()
                    .set_title("Insert Picture")
                    .add_filter(
                        "Pictures",
                        &["png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "webp"],
                    );
                if let Some(dir) = self
                    .index(id)
                    .and_then(|i| self.windows[i].1.st.main_view().path.clone())
                    .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                {
                    dialog = dialog.set_directory(dir);
                }
                if let Some(path) = dialog.pick_file() {
                    self.insert_picture_file(id, &path);
                }
            }
            Effect::NativePreview => {
                #[cfg(target_os = "macos")]
                self.effect(el, id, Effect::Dialog(DialogId::Print));
                #[cfg(not(target_os = "macos"))]
                if let Some(i) = self.index(id) {
                    self.preview_in_system_viewer(i);
                }
            }
            Effect::Print => {
                // The common dispatcher preserves native Windows/macOS printing
                // and the existing Linux print workflow.
                self.effect(el, id, Effect::Dialog(DialogId::Print));
            }
            Effect::SavePdf => {
                if let Some(i) = self.index(id) {
                    self.windows[i].1.st.pending_print = Some(crate::page_dialogs::PrintJob {
                        copies: 1,
                        pages: None,
                        pdf: true,
                    });
                }
            }
            Effect::Save => {
                self.save(id, false);
            }
            Effect::SaveAs => {
                self.save(id, true);
            }
            #[cfg(target_os = "macos")]
            Effect::Close if crate::mac_licenses::close_if_key() => {}
            Effect::Close => {
                // A Mac app stays open with no windows; elsewhere the last
                // window closing quits.
                let session = self.open_paths();
                if self.close_window(id) && self.windows.is_empty() && !cfg!(target_os = "macos") {
                    crate::settings::update(|s| {
                        s.last_session = session;
                        s.quit_cleanly = true;
                    });
                    el.exit();
                }
            }
            Effect::Quit => {
                let session = self.open_paths();
                let ids: Vec<WindowId> = self.windows.iter().map(|(w, _)| *w).collect();
                for w in ids {
                    if !self.close_window(w) {
                        return;
                    }
                }
                crate::settings::update(|s| {
                    s.last_session = session;
                    s.quit_cleanly = true;
                });
                el.exit();
            }
        }
    }

    fn key(&mut self, el: &ActiveEventLoop, id: WindowId, event: winit::event::KeyEvent) {
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
        // Tools > Commands' Keys > Add…: this key press is the new key.
        if let Some((Which::Commands(st), d)) = w.st.dialogs.last_mut()
            && st.capturing
        {
            let code = match event.physical_key {
                PhysicalKey::Code(c) => Some(c),
                PhysicalKey::Unidentified(_) => None,
            };
            let mods = Mods::from_winit(w.mods);
            if let Some(spec) =
                command_map::key_spec(&event.logical_key, code.and_then(keycode_char), mods)
            {
                crate::commands_dialog::captured(st, d, spec);
                w.window.request_redraw();
                return;
            }
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
    fn insert_text(&mut self, el: &ActiveEventLoop, id: WindowId, text: &str, typed: bool) {
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

    fn mouse_button(
        &mut self,
        el: &ActiveEventLoop,
        id: WindowId,
        state: ElementState,
        context: bool,
    ) {
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

    fn cursor_moved(&mut self, id: WindowId, px: f64, py: f64) {
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
    fn update_cursor(&mut self, i: usize) {
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

/// Quill's About box.
#[cfg(target_os = "macos")]
fn about() {
    crate::mac_licenses::show_about();
}

/// Help > Licenses…: the window listing every license Quill carries.
#[cfg(target_os = "macos")]
fn licenses() {
    crate::mac_licenses::show();
}

/// A stand-in until these systems have a Licenses window: the licenses, as a
/// text file opened in the system's own program for text.
#[cfg(not(target_os = "macos"))]
fn licenses() {
    if !crate::licenses::open_text_file() {
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Info)
            .set_title("Licenses")
            .set_description(
                "Quill's licenses are in the files that come with it: LICENSE-MIT, \
                 LICENSE-APACHE and THIRD-PARTY-NOTICES.txt.",
            )
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }
}

/// Other systems retain the portable About box until they receive their own
/// native application-shell integration.
#[cfg(not(target_os = "macos"))]
fn about() {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Info)
        .set_title("About Quill")
        .set_description(format!(
            "Quill {}{}\n\nA small word processor written in Rust, inspired by Lucas's favorite word processor from 1992.\n\n{}\nhttps://quillwp.com\nSee Help > Licenses… for the full text.",
            env!("CARGO_PKG_VERSION"),
            if cfg!(debug_assertions) {
                format!("\n{}", crate::build_info::summary())
            } else {
                String::new()
            },
            crate::licenses::SUMMARY
        ))
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".into())
}

/// Added to the title of a window reopened from a recovery file, after the
/// document's name so the name still comes first.
const RECOVERED: &str = " - Recovered";

/// A window title without its extension, for naming a saved or printed
/// copy: "Report.qwp" -> "Report". A recovered window's suffix goes too.
fn strip_ext(title: &str) -> &str {
    let title = title.strip_suffix(RECOVERED).unwrap_or(title);
    match title.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.len() <= 4 => stem,
        _ => title,
    }
}

/// The documents open when Quill last ended that are still there, if
/// Preferences says to open them again. Set to "After a crash" (Prompt), it
/// asks only when Quill did not end through Quit: after a Quit the person
/// closed them on purpose and has them in Open Recent. Unsaved changes come
/// back separately, as Recovered windows.
fn reopen_last_session(quit_cleanly: bool) -> Vec<PathBuf> {
    use crate::settings::Reopen;
    let (reopen, paths) = crate::settings::with(|s| (s.reopen, s.last_session.clone()));
    let paths: Vec<PathBuf> = paths.into_iter().filter(|p| p.is_file()).collect();
    if paths.is_empty() {
        return paths;
    }
    let yes = match reopen {
        Reopen::Always => true,
        Reopen::Never => false,
        Reopen::Prompt if quit_cleanly => false,
        Reopen::Prompt => {
            let names: Vec<String> = paths
                .iter()
                .take(10)
                .filter_map(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .collect();
            let more = paths.len().saturating_sub(names.len());
            let mut list = names.join("\n");
            if more > 0 {
                list.push_str(&format!("\nand {more} more"));
            }
            let answer = message_box(
                rfd::MessageLevel::Info,
                "Quill",
                format!(
                    "Quill did not quit normally last time. Reopen the documents that \
                     were open?\n\n{list}"
                ),
                rfd::MessageButtons::OkCancelCustom("Reopen".into(), "Don't Reopen".into()),
            );
            matches!(answer, rfd::MessageDialogResult::Custom(ref b) if b == "Reopen")
                || matches!(answer, rfd::MessageDialogResult::Ok)
        }
    };
    if yes { paths } else { Vec::new() }
}

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

/// One click's worth of scrolling for an arrow or the gray track.
fn scroll_step(view: &mut DocView, part: ScrollPart) {
    let page = (view.area.h - 24.0).max(16.0);
    match part {
        ScrollPart::LineUp => view.scroll_by(-16.0),
        ScrollPart::LineDown => view.scroll_by(16.0),
        ScrollPart::PageUp => view.scroll_by(-page),
        ScrollPart::PageDown => view.scroll_by(page),
        ScrollPart::Thumb => {}
    }
}

/// Apply one AppKit `NSScrollerPart` action. Keeping the arithmetic here also
/// makes native scrolling testable without creating an Objective-C view.
#[cfg(target_os = "macos")]
fn native_scroll_value(current: f32, max: f32, page: f32, part: usize, value: f32) -> f32 {
    let next = match part {
        1 => current - page,  // NSScrollerDecrementPage
        2 | 6 => value * max, // knob or knob slot
        3 => current + page,  // NSScrollerIncrementPage
        4 => current - 16.0,  // NSScrollerDecrementLine
        5 => current + 16.0,  // NSScrollerIncrementLine
        _ => current,
    };
    next.clamp(0.0, max.max(0.0))
}

/// The unshifted character a key produces on a US layout, used to match
/// shortcuts like ⌘⇧. whatever the shifted character is.
fn keycode_char(code: KeyCode) -> Option<char> {
    use KeyCode::*;
    const LETTERS: [KeyCode; 26] = [
        KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO,
        KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    ];
    const DIGITS: [KeyCode; 10] = [
        Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
    ];
    if let Some(i) = LETTERS.iter().position(|&c| c == code) {
        return Some((b'a' + i as u8) as char);
    }
    if let Some(i) = DIGITS.iter().position(|&c| c == code) {
        return Some((b'0' + i as u8) as char);
    }
    Some(match code {
        Minus => '-',
        Equal => '=',
        BracketLeft => '[',
        BracketRight => ']',
        Backslash => '\\',
        Semicolon => ';',
        Quote => '\'',
        Comma => ',',
        Period => '.',
        Slash => '/',
        Backquote => '`',
        _ => return None,
    })
}

/// The crop pointer: two corner brackets, black with a white edge, 17 pixels
/// square with its hot spot in the middle.
fn crop_cursor(el: &ActiveEventLoop) -> Option<CustomCursor> {
    const SIZE: u16 = 17;
    let ink = |x: i32, y: i32| {
        // The upper left bracket runs right and down from (4, 4), the lower
        // right one left and up from (12, 12), each past the other's corner.
        (y == 4 && (1..=12).contains(&x))
            || (x == 4 && (1..=12).contains(&y))
            || (y == 12 && (4..=15).contains(&x))
            || (x == 12 && (4..=15).contains(&y))
    };
    let mut rgba = Vec::with_capacity(4 * SIZE as usize * SIZE as usize);
    for y in 0..SIZE as i32 {
        for x in 0..SIZE as i32 {
            let edge = (-1..=1).any(|dy| (-1..=1).any(|dx| ink(x + dx, y + dy)));
            rgba.extend_from_slice(match (ink(x, y), edge) {
                (true, _) => &[0, 0, 0, 255],
                (false, true) => &[255, 255, 255, 255],
                _ => &[0, 0, 0, 0],
            });
        }
    }
    let source = CustomCursor::from_rgba(rgba, SIZE, SIZE, SIZE / 2, SIZE / 2).ok()?;
    Some(el.create_custom_cursor(source))
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.crop_cursor.is_none() {
            self.crop_cursor = crop_cursor(el);
        }
        if !self.recovered {
            self.recovered = true;
            for r in crate::recovery::load_all() {
                self.new_window(el, r.doc, Some(format!("{}{RECOVERED}", r.title)));
                if let Some((_, window)) = self.windows.last_mut() {
                    window.recovery = Some(r.path);
                    if let Some(original) = r.original {
                        window.st.main_view_mut().resume_markdown_source(original);
                    }
                }
            }
        }
        let mut files = std::mem::take(&mut self.pending);
        // Documents the Finder started Quill to open arrive before this.
        #[cfg(target_os = "macos")]
        files.extend(crate::mac_open::take());
        if !self.reopened {
            // Until Quit sets it again, a restart is taken as a crash.
            let quit_cleanly = crate::settings::with(|s| s.quit_cleanly);
            crate::settings::update(|s| s.quit_cleanly = false);
            if files.is_empty() {
                files = reopen_last_session(quit_cleanly);
            }
        }
        self.reopened = true;
        self.open_files(el, &files);
        if self.windows.is_empty() {
            self.new_window(el, crate::settings::get().new_document(), None);
        }
        self.sync_menus();
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: UserEvent) {
        #[cfg(not(target_os = "macos"))]
        let _ = el;
        match event {
            #[cfg(target_os = "linux")]
            UserEvent::LinuxMenu { window, action } => {
                if self.index(window).is_some() {
                    self.front = Some(window);
                    self.menu_action(el, action);
                    if let Some(i) = self.index(window) {
                        self.windows[i].1.window.focus_editor();
                    }
                }
            }
            #[cfg(target_os = "linux")]
            UserEvent::LinuxClose(id) => self.window_event(el, id, WindowEvent::CloseRequested),
            #[cfg(target_os = "linux")]
            UserEvent::LinuxFocus(id) => {
                if let Some(i) = self.index(id) {
                    self.windows[i].1.window.focus_editor();
                }
                self.window_event(el, id, WindowEvent::Focused(true));
            }
            #[cfg(target_os = "macos")]
            UserEvent::Menu(id) => self.menu_choice(el, &id),
            #[cfg(target_os = "macos")]
            UserEvent::OpenFiles => self.open_files(el, &crate::mac_open::take()),
            #[cfg(target_os = "macos")]
            UserEvent::Quit => self.menu_action(el, Action::Cmd(Cmd::Quit)),
            #[cfg(target_os = "macos")]
            UserEvent::InsertText(text) => {
                if let Some(id) = self
                    .windows
                    .iter()
                    .find(|(_, w)| w.focused)
                    .map(|(id, _)| *id)
                {
                    self.insert_text(el, id, &text, false);
                }
            }
            UserEvent::PrintProgress {
                window,
                done,
                total,
            } => {
                if let Some(i) = self.index(window) {
                    let w = &mut self.windows[i].1;
                    w.st.main_view_mut().status =
                        Some(format!("Creating PDF\u{2026} {done} of {total}"));
                    w.window.request_redraw();
                }
            }
            UserEvent::PrintFinished {
                window,
                result,
                #[cfg(target_os = "macos")]
                native,
            } => {
                #[cfg(target_os = "macos")]
                if native && self.native_print_window == Some(window) {
                    self.native_print_window = None;
                }
                if let Some(i) = self.index(window) {
                    let w = &mut self.windows[i].1;
                    w.st.main_view_mut().status = None;
                    w.window.request_redraw();
                }
                if let Err(e) = result {
                    alert("The PDF job did not finish.", &e);
                }
            }
            #[cfg(target_os = "macos")]
            UserEvent::NativePrintReady {
                window,
                bytes,
                title,
                props,
            } => {
                let result = if let Some(i) = self.index(window) {
                    let w = &self.windows[i].1;
                    crate::mac_print::print(
                        &w.window,
                        &bytes,
                        &title,
                        &props,
                        &w.print_settings,
                        self.proxy.clone(),
                    )
                } else {
                    Err("The document window closed before printing.".into())
                };
                if let Err(error) = result {
                    self.user_event(
                        el,
                        UserEvent::PrintFinished {
                            window,
                            result: Err(error),
                            native: true,
                        },
                    );
                }
            }
            #[cfg(target_os = "macos")]
            UserEvent::PageSetupFinished {
                window,
                result,
                document,
            } => {
                self.native_print_window = None;
                self.take_doc_for_page_setup(window, result);
                if document {
                    self.effect(el, window, Effect::Dialog(DialogId::Document));
                }
            }
            #[cfg(windows)]
            UserEvent::WindowsShortcut { panel, spec } => {
                if let Some((_, w)) = self
                    .windows
                    .iter_mut()
                    .find(|(_, w)| w.dialog.as_ref().is_some_and(|d| d.panel_id() == panel))
                    && let Some((Which::Commands(state), dialog)) = w.st.dialogs.last_mut()
                {
                    if let Some(spec) = spec {
                        crate::commands_dialog::captured(state, dialog, spec);
                    } else {
                        crate::commands_dialog::cancel_capture(state, dialog);
                    }
                    w.window.request_redraw();
                }
            }
            #[cfg(windows)]
            UserEvent::WindowsDialog { panel, id } => {
                if let Some((_, w)) = self
                    .windows
                    .iter_mut()
                    .find(|(_, w)| w.dialog.as_ref().is_some_and(|d| d.panel_id() == panel))
                    && let (Some(native), Some((which, dialog))) =
                        (&w.dialog, w.st.dialogs.last_mut())
                {
                    if matches!(id, "list" | "keys")
                        && let Some(row) = native.selected_row(id)
                        && let Which::Commands(state) = which
                    {
                        crate::commands_dialog::select_native(state, dialog, id, row);
                    } else {
                        native.apply_to_model(dialog, id);
                        let outcome = crate::win_dialog::NativeDialog::outcome(dialog, id);
                        let clip = self.clipboard.get_or_insert_with(Clipboard::new);
                        w.st.dialog_outcome(outcome, clip);
                    }
                    w.window.request_redraw();
                }
            }
            #[cfg(target_os = "macos")]
            UserEvent::NativeScroll {
                scroller,
                part,
                value,
            } => self.native_scroll(scroller, part, value),
            #[cfg(target_os = "macos")]
            UserEvent::NativeDialog { panel, id, choice } => self.native_dialog(panel, id, choice),
            #[cfg(target_os = "macos")]
            UserEvent::NativeShortcut { panel, spec } => self.native_shortcut(panel, spec),
            #[cfg(target_os = "macos")]
            UserEvent::NativeShortcutCancel { panel } => self.native_shortcut_cancel(panel),
        }
        self.sync_menus();
    }

    fn new_events(&mut self, _el: &ActiveEventLoop, cause: StartCause) {
        self.refresh_font_folders();
        if let StartCause::ResumeTimeReached { .. } = cause {
            let now = Instant::now();
            #[cfg(target_os = "linux")]
            if self.editor_wake.is_some_and(|deadline| now < deadline) {
                return;
            }
            if now >= self.next_recovery() {
                self.last_recovery = now;
                let autosave = crate::settings::with(|s| s.autosave_secs) > 0;
                let mut serial = self.recovery_serial;
                for (_, w) in &mut self.windows {
                    if autosave && (w.group == 0 || w.holds) && w.st.main_view().doc.is_modified() {
                        serial = serial.wrapping_add(1);
                        let (doc, original) = w.st.main_view().recovery_documents();
                        match crate::recovery::save(
                            w.recovery.as_deref(),
                            &doc,
                            original,
                            &w.st.main_view().title,
                            serial,
                        ) {
                            Ok(path) => w.recovery = Some(path),
                            Err(e) => eprintln!("quill: autosave failed: {e}"),
                        }
                    }
                }
                self.recovery_serial = serial;
            }
            for (_, w) in &mut self.windows {
                if w.st.tick() || w.focused {
                    w.window.request_redraw();
                }
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.is_palette_window(id) {
            self.palette_window_event(el, event);
            return;
        }
        let Some(i) = self.index(id) else {
            return;
        };
        #[cfg(target_os = "macos")]
        if let Some(preferences) = &self.preferences
            && preferences.is_modal()
        {
            let input = matches!(
                event,
                WindowEvent::KeyboardInput { .. }
                    | WindowEvent::Ime(_)
                    | WindowEvent::MouseInput { .. }
                    | WindowEvent::MouseWheel { .. }
                    | WindowEvent::Touch(_)
                    | WindowEvent::DroppedFile(_)
            );
            if input || matches!(event, WindowEvent::Focused(true)) {
                preferences.activate();
            }
            if input {
                return;
            }
        }
        #[cfg(target_os = "macos")]
        if self.windows[i].1.st.modal_dialog_open()
            && let Some(dialog) = &self.windows[i].1.dialog
        {
            let input = matches!(
                event,
                WindowEvent::KeyboardInput { .. }
                    | WindowEvent::Ime(_)
                    | WindowEvent::MouseInput { .. }
                    | WindowEvent::MouseWheel { .. }
                    | WindowEvent::Touch(_)
                    | WindowEvent::DroppedFile(_)
            );
            if (input || matches!(event, WindowEvent::Focused(true)))
                && dialog.raise_modal()
                && input
            {
                return;
            }
        }
        // A window shares its document with others: it needs the document
        // to draw or to take input, and the others need redrawing after.
        let shared = self.windows[i].1.group != 0;
        if shared {
            self.take_doc(i);
            if matches!(
                event,
                WindowEvent::KeyboardInput { .. }
                    | WindowEvent::MouseInput { .. }
                    | WindowEvent::Ime(_)
            ) {
                self.redraw_group(i);
            }
        }
        #[cfg(target_os = "linux")]
        if matches!(
            event,
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                ..
            }
        ) {
            self.windows[i].1.window.focus_editor();
        }
        match event {
            WindowEvent::CloseRequested => {
                self.handle_effect(el, id, Effect::Close);
                self.sync_menus();
            }
            WindowEvent::RedrawRequested => {
                self.redraw(id);
                self.refresh_palette_window();
            }
            WindowEvent::ThemeChanged(theme) => {
                let w = &mut self.windows[i].1;
                w.st.dark = crate::settings::with(|s| s.appearance)
                    .is_dark(theme == winit::window::Theme::Dark);
                w.window.request_redraw();
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                // Entering or leaving full screen, by menu or green button,
                // resizes the window.
                let w = &mut self.windows[i].1;
                let full = w.window.fullscreen().is_some();
                w.window.request_redraw();
                if w.st.full_screen != full {
                    w.st.full_screen = full;
                    self.sync_menus();
                }
            }
            WindowEvent::Focused(f) => {
                if f {
                    self.front = Some(id);
                    self.sync_menus();
                }
                let w = &mut self.windows[i].1;
                if f && w.st.native_dialogs
                    && let Some((_, d)) = w.st.dialogs.last_mut()
                    && d.modeless
                {
                    d.inactive = true;
                    w.st.dialog_mouse = false;
                }
                w.focused = f;
                if !f {
                    w.st.menu = None;
                }
                w.window.request_redraw();
            }
            WindowEvent::ModifiersChanged(m) => {
                self.windows[i].1.mods = m.state();
                self.update_cursor(i);
            }
            WindowEvent::DroppedFile(path) => {
                // A picture dropped on a window goes in at the insertion
                // point; a document opens.
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase());
                if matches!(
                    ext.as_deref(),
                    Some("png" | "jpg" | "jpeg" | "gif" | "tif" | "tiff" | "bmp" | "webp")
                ) {
                    self.insert_picture_file(id, &path);
                } else {
                    self.open_files(el, &[path]);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.key(el, id, event);
                self.sync_menus();
            }
            // Input methods: Chinese, Japanese and Korean, and accents typed
            // with dead keys. What's being composed shows at the insertion
            // point; what's committed is typed in.
            WindowEvent::Ime(Ime::Preedit(text, _)) => {
                self.ensure_fonts();
                let w = &mut self.windows[i].1;
                if let Some(fonts) = self.fonts.as_mut() {
                    w.st.view.set_composing(&text, fonts);
                }
                w.window.request_redraw();
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                self.insert_text(el, id, &text, true);
                self.sync_menus();
            }
            WindowEvent::Ime(Ime::Disabled) => {
                let w = &mut self.windows[i].1;
                w.st.view.end_composing();
                w.window.request_redraw();
            }
            WindowEvent::MouseInput {
                state,
                button: button @ (MouseButton::Left | MouseButton::Right),
                ..
            } => {
                // On the Mac, Control-click on a link opens it. Anywhere
                // else it is a right-click, and a real right-click always is.
                let control_click = cfg!(target_os = "macos")
                    && button == MouseButton::Left
                    && self.windows[i].1.mods.control_key();
                // Control on a picture's handle crops, on any system. The
                // release goes with the press, whatever is held by then.
                let w = &mut self.windows[i].1;
                w.st.view.crop_held = w.mods.control_key();
                let (dx, dy) = w.st.document_point(w.cursor.0, w.cursor.1);
                let crop = button == MouseButton::Left
                    && (w.st.view.picture_dragging()
                        || (w.st.view.crop_held && w.st.view.picture_cursor(dx, dy).is_some()));
                if !crop
                    && control_click
                    && state == ElementState::Pressed
                    && self.ask_to_open_link(id)
                {
                    return;
                }
                let was_resizing = self.windows[i].1.st.review_resize.is_some();
                self.mouse_button(
                    el,
                    id,
                    state,
                    button == MouseButton::Right
                        || (!crop
                            && cfg!(target_os = "macos")
                            && self.windows[i].1.mods.control_key()),
                );
                // A divider drag changes no command state. Native menu work
                // here stalls the first frame of the drag and the release.
                if !was_resizing
                    && self
                        .index(id)
                        .is_none_or(|i| self.windows[i].1.st.review_resize.is_none())
                {
                    self.sync_menus();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_moved(id, position.x, position.y)
            }
            WindowEvent::CursorLeft { .. } => {
                // No toolbar button stays lit once the pointer has gone.
                let w = &mut self.windows[i].1;
                let lit = w.st.toolbar_hover.take().is_some()
                    | w.st.bar_hover.take().is_some()
                    | w.st.tip.take().is_some();
                if lit {
                    w.window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, phase, .. } => {
                let w = &mut self.windows[i].1;
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y * 16.0 * 3.0,
                    MouseScrollDelta::PixelDelta(p) => -(p.y / w.window.scale_factor()) as f32,
                };
                if w.mods.super_key() && w.st.dialogs.is_empty() && w.st.menu.is_none() {
                    // ⌘-scroll zooms: a tenth bigger for each click of a
                    // wheel scrolled up.
                    let line = matches!(delta, MouseScrollDelta::LineDelta(..));
                    let save = line || phase == TouchPhase::Ended;
                    w.st.zoom_by(1.1f64.powf(-dy as f64 / 48.0), save);
                } else {
                    let dx = match delta {
                        MouseScrollDelta::LineDelta(x, _) => -x * 48.0,
                        MouseScrollDelta::PixelDelta(p) => -(p.x / w.window.scale_factor()) as f32,
                    };
                    if w.mods.shift_key() && dx.abs() < 0.01 {
                        w.st.wheel_horizontal(dy);
                    } else {
                        w.st.wheel_horizontal(dx);
                        w.st.wheel(dy);
                    }
                }
                w.window.request_redraw();
            }
            WindowEvent::PinchGesture { delta, phase, .. } => {
                let w = &mut self.windows[i].1;
                if w.st.dialogs.is_empty() && w.st.menu.is_none() {
                    let save = matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled);
                    w.st.zoom_by(1.0 + delta, save);
                    w.window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.sync_palette_window(el);
        #[cfg(target_os = "linux")]
        crate::linux_window::poll();
        #[cfg(target_os = "macos")]
        crate::mac_open::set_unsaved(
            self.windows
                .iter()
                .any(|(_, w)| w.recovery.is_some() || w.st.main_view().doc.is_modified()),
        );
        // Holding a scroll arrow or track keeps scrolling; otherwise wake for the
        // caret's blink and sleep the rest of the time.
        let holding = self.windows.iter_mut().any(|(_, w)| {
            w.st.scroll_press.is_some_and(|p| p.0 != ScrollPart::Thumb)
                || w.st
                    .horizontal_press
                    .is_some_and(|p| p.0 != ScrollPart::Thumb)
                || (w.focused && w.st.selection_scroll(false))
        });
        if holding {
            let next = Instant::now() + Duration::from_millis(60);
            #[cfg(target_os = "linux")]
            {
                self.editor_wake = Some(next);
            }
            el.set_control_flow(ControlFlow::WaitUntil(next));
            return;
        }
        let next = self
            .windows
            .iter()
            .filter(|(_, w)| w.focused && w.st.menu.is_none())
            .map(|(_, w)| w.st.view.next_blink())
            .min()
            .map_or(self.next_recovery(), |t| t.min(self.next_recovery()));
        // Wake when a tooltip is due to show.
        let tip = self
            .windows
            .iter()
            .filter_map(|(_, w)| w.st.tip.as_ref().filter(|t| !t.shown).map(|t| t.due))
            .min();
        let next = tip.map_or(next, |t| t.min(next));
        let spelling = self
            .windows
            .iter()
            .flat_map(|(_, w)| {
                [
                    w.st.view.live_spelling_deadline(),
                    w.st.pane
                        .as_ref()
                        .and_then(|p| p.other.live_spelling_deadline()),
                ]
            })
            .flatten()
            .min();
        let next = spelling.map_or(next, |t| t.min(next));
        #[cfg(target_os = "linux")]
        {
            self.editor_wake = Some(next);
        }
        #[cfg(target_os = "linux")]
        let next = if self.windows.iter().any(|(_, w)| w.window.has_native_menu()) {
            next.min(Instant::now() + Duration::from_millis(16))
        } else {
            next
        };
        el.set_control_flow(ControlFlow::WaitUntil(next));
    }
}

/// Quill's icon (assets/quill-64.png) for window title bars and task bars.
#[cfg(not(target_os = "macos"))]
fn window_icon() -> Option<winit::window::Icon> {
    let png = include_bytes!("../../../assets/quill-64.png");
    let (w, h, rgba) = crate::image::decode_rgba(png, quill_core::ImageFormat::Png)?;
    winit::window::Icon::from_rgba(rgba, w, h).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_icon_picture_decodes_to_a_square_with_alpha() {
        // Windows and Linux windows build their icon from this file.
        let png = include_bytes!("../../../assets/quill-64.png");
        let (w, h, rgba) =
            crate::image::decode_rgba(png, quill_core::ImageFormat::Png).expect("icon decodes");
        assert_eq!((w, h), (64, 64));
        assert_eq!(rgba.len(), 64 * 64 * 4);
        assert!(rgba.chunks(4).any(|p| p[3] == 255));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn toolbar_and_formatting_pop_ups_are_handed_to_the_system_menu_on_the_mac() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let families = fonts.family_info();
        let mut cache = GlyphCache::new();
        let mut label = Label {
            fonts: &mut fonts,
            cache: &mut cache,
        };
        let env = Env {
            font_names: &[],
            font_families: &families,
            recent: &[],
            windows: &[],
        };
        let mut state = WinState::new(DocView::new(Document::new(), "Pop-ups".into()));
        state.width = 900.0;
        state.height = 700.0;
        state.ui = 1.5;
        let toolbar_menu = (0..64)
            .find(|i| toolbar::command(*i) == Some(Cmd::ToolbarMenu))
            .expect("the toolbar has a Toolbar Menu button");
        state.open_toolbar_menu(toolbar_menu, &mut label);
        let (menu, actions, _) = state.take_popup_menu().expect("the toolbar's menu");
        assert!(state.menu.is_none(), "Quill draws nothing itself");
        assert!(
            menu.items
                .iter()
                .any(|it| actions.get(it.id as usize) == Some(&Action::ToolbarPosition(1)))
        );
        for hit in [
            Hit::FontPopup,
            Hit::SizePopup,
            Hit::ColorPopup,
            Hit::StylePopup,
        ] {
            state.open_popup(hit, &mut label, &env);
            let rect = state.menu.as_ref().expect("a pop-up opens").open.rect;
            let (menu, _, at) = state.take_popup_menu().expect("the pop-up's menu");
            assert!(state.menu.is_none(), "{hit:?}: Quill draws nothing itself");
            assert!(!menu.items.is_empty(), "{hit:?}");
            if hit == Hit::FontPopup {
                // Font names keep their own faces in the native menu.
                assert!(menu.items.iter().any(|it| it.font.is_some()));
            }
            assert_eq!(at, ((rect.x * 1.5) as f64, (rect.y * 1.5) as f64));
        }
    }
    use quill_core::{Position, Selection};

    #[test]
    fn the_style_dialogs_character_style_menu_makes_a_new_style_and_comes_back_to_it() {
        let doc = quill_core::text::from_plain_text("Body text");
        let mut state = WinState::new(DocView::new(doc, "Doc".into()));
        let (d, st) = crate::style_dialog::style(&state.view);
        state.dialogs.push((Which::Style(st), d));
        let mut clip = Clipboard::new();
        let none = state.view.doc.styles.characters.len();
        let last = none + 1;
        // The menu's last choice is the one that makes a style.
        assert_eq!(
            state.dialogs[0]
                .1
                .options("character_style")
                .last()
                .map(String::as_str),
            Some("New Character Style\u{2026}")
        );
        state.dialogs[0].1.set_selected("character_style", last);
        state.dialog_outcome(Outcome::Changed("character_style"), &mut clip);
        assert!(matches!(
            state.dialogs[1].0,
            Which::CharacterStyleEdit(None)
        ));
        // The menu goes back to what the style had until the new one is saved.
        assert_eq!(state.dialogs[0].1.selected("character_style"), 0);
        state.dialogs[1].1.set_field("name", "Aside");
        state.dialogs[1].1.set_on("bold", true);
        state.dialog_outcome(Outcome::Ok, &mut clip);
        assert_eq!(state.dialogs.len(), 1, "back in the Style dialog");
        let id = state.view.doc.styles.find_character("Aside").unwrap();
        let d = &state.dialogs[0].1;
        assert_eq!(d.selected("character_style"), id as usize);
        assert!(d.options("character_style").iter().any(|o| o == "Aside"));
        let Which::Style(st) = &state.dialogs[0].0 else {
            panic!("the Style dialog");
        };
        assert_eq!(st.character, id);
        // Cancelling the editor leaves everything as it was.
        let count = state.view.doc.styles.characters.len();
        state.dialogs[0]
            .1
            .set_selected("character_style", count + 1);
        state.dialog_outcome(Outcome::Changed("character_style"), &mut clip);
        state.dialog_outcome(Outcome::Cancel, &mut clip);
        assert_eq!(state.dialogs.len(), 1);
        assert_eq!(state.view.doc.styles.characters.len(), count);
        assert_eq!(state.dialogs[0].1.selected("character_style"), id as usize);
    }

    #[test]
    fn character_styles_window_edits_and_adds_styles_through_its_editor() {
        let mut doc = quill_core::text::from_plain_text("Body text");
        doc.add_character_style(
            "Callout",
            quill_core::character_styles::CharacterFormat::default(),
        )
        .unwrap();
        doc.forget_history();
        let original = quill_formats::native::write(&doc, 0);
        let mut state = WinState::new(DocView::new(doc, "Doc".into()));
        let d = crate::character_style_dialog::dialog(&state.view);
        state.dialogs.push((Which::CharacterStyles, d));
        let mut clip = Clipboard::new();

        // Cancel leaves the style and the document alone.
        state.dialog_outcome(Outcome::Button("edit"), &mut clip);
        assert!(matches!(
            state.dialogs[1].0,
            Which::CharacterStyleEdit(Some(1))
        ));
        state.dialogs[1].1.set_field("size", "18");
        state.dialog_outcome(Outcome::Cancel, &mut clip);
        assert_eq!(state.dialogs.len(), 1);
        assert_eq!(quill_formats::native::write(&state.view.doc, 0), original);

        // OK saves what the editor shows into the style; the text is untouched.
        state.dialog_outcome(Outcome::Button("edit"), &mut clip);
        let editor = &mut state.dialogs[1].1;
        editor.set_field("size", "18");
        editor.set_on("bold", true);
        state.dialog_outcome(Outcome::Ok, &mut clip);
        assert_eq!(state.dialogs.len(), 1, "back in Character Styles");
        let format = state.view.doc.styles.character(1).unwrap().format;
        assert_eq!(format.mask, (1 << 1) | (1 << 2));
        assert_eq!(format.props.size, 36);
        assert_eq!(
            state.view.doc.main.paras[0].chp_of_char(0).character_style,
            0
        );
        assert_eq!(
            state.dialogs[0].1.item_label("description"),
            "Sets: 18 pt, Bold"
        );

        // + makes a new style; a name already in use keeps the editor open.
        state.dialog_outcome(Outcome::Button("new"), &mut clip);
        assert!(matches!(
            state.dialogs[1].0,
            Which::CharacterStyleEdit(None)
        ));
        state.dialogs[1].1.set_field("name", "Callout");
        state.dialog_outcome(Outcome::Ok, &mut clip);
        assert!(state.dialogs.len() > 2, "an alert over the editor");
        state.dialogs.truncate(2);
        state.dialogs[1].1.set_field("name", "Aside");
        state.dialog_outcome(Outcome::Ok, &mut clip);
        assert_eq!(state.dialogs.len(), 1);
        assert_eq!(state.view.doc.styles.find_character("Aside"), Some(2));
        assert_eq!(
            crate::character_style_dialog::selected_style(&state.dialogs[0].1),
            2
        );
        state.view.doc.undo();
        state.view.doc.undo();
        assert_eq!(quill_formats::native::write(&state.view.doc, 0), original);
    }

    #[test]
    fn character_style_dialog_edits_stay_pending_and_cancel_does_not_add_fonts() {
        let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
        let original = quill_formats::native::write(&state.view.doc, 0);
        let (d, st) = crate::style_dialog::style(&state.view);
        state.dialogs.push((Which::Style(st), d));
        let mut clip = Clipboard::new();
        state.dialog_outcome(Outcome::Button("character"), &mut clip);
        let editor = &mut state.dialogs.last_mut().unwrap().1;
        editor.set_options("font", vec!["A pending font".into()], 0);
        editor.set_field("size", "18");
        editor.set_on("bold", true);
        state.dialog_outcome(Outcome::Ok, &mut clip);
        assert!(matches!(&state.dialogs[0].0, Which::Style(st)
            if st.chp.bold && st.chp.size == 36 && st.fonts.contains(&"A pending font".into())));
        assert_eq!(quill_formats::native::write(&state.view.doc, 0), original);
        state.dialog_outcome(Outcome::Cancel, &mut clip);
        assert_eq!(quill_formats::native::write(&state.view.doc, 0), original);
        assert!(!state.view.doc.is_modified());

        let (d, st) = crate::style_dialog::style(&state.view);
        state.dialogs.push((Which::Style(st), d));
        state.dialog_outcome(Outcome::Button("character"), &mut clip);
        state.dialogs.last_mut().unwrap().1.set_field("size", "22");
        state.dialog_outcome(Outcome::Cancel, &mut clip);
        state.dialog_outcome(Outcome::Button("define"), &mut clip);
        assert_eq!(quill_formats::native::write(&state.view.doc, 0), original);
        assert!(!state.view.doc.is_modified());
    }

    #[test]
    fn character_editor_defines_manual_defaults_and_ok_applies_the_defined_style() {
        let doc = quill_core::text::from_plain_text("Body text");
        let mut state = WinState::new(DocView::new(doc, "Doc".into()));
        let (mut d, st) = crate::style_dialog::style(&state.view);
        d.set_selected("list", 0);
        state.dialogs.push((Which::Style(st), d));
        let mut clip = Clipboard::new();
        state.dialog_outcome(Outcome::Changed("list"), &mut clip);
        state
            .dialogs
            .last_mut()
            .unwrap()
            .1
            .set_field("name", "Large body");
        state.dialog_outcome(Outcome::Button("character"), &mut clip);
        let editor = &mut state.dialogs.last_mut().unwrap().1;
        editor.set_field("size", "18");
        editor.set_on("bold", true);
        editor.set_options("font", vec!["A new style font".into()], 0);
        state.dialog_outcome(Outcome::Ok, &mut clip);
        assert!(!state.view.doc.fonts.contains(&"A new style font".into()));
        state.dialog_outcome(Outcome::Button("define"), &mut clip);
        let id = state.view.doc.styles.find("Large body").unwrap();
        assert_eq!(state.view.doc.styles.get(id).chp.size, 36);
        assert!(state.view.doc.styles.get(id).chp.bold);
        assert_eq!(
            state
                .view
                .doc
                .font_name(state.view.doc.styles.get(id).chp.font),
            "A new style font"
        );
        assert_eq!(
            state.view.doc.main.paras[0].pap.style, 0,
            "Define does not apply the new style"
        );
        assert_eq!(state.dialogs.len(), 1, "Define keeps the dialog open");
        state.dialog_outcome(Outcome::Ok, &mut clip);
        assert!(state.dialogs.is_empty());
        assert_eq!(state.view.doc.main.paras[0].pap.style, id);
        assert_eq!(state.view.doc.main.paras[0].chp_of_char(0).size, 36);
        assert!(state.view.doc.main.paras[0].chp_of_char(0).bold);
        let bytes = quill_formats::native::write(&state.view.doc, 0);
        let saved = quill_formats::native::read(&bytes).unwrap();
        assert_eq!(saved.styles.get(id).chp, state.view.doc.styles.get(id).chp);
    }

    #[test]
    fn choosing_a_character_style_in_format_style_links_it_and_no_style_unlinks() {
        let mut doc = quill_core::text::from_plain_text("Body text");
        doc.add_character_style(
            "Emphasis",
            quill_core::character_styles::CharacterFormat::new(
                quill_core::CharProps {
                    italic: true,
                    ..Default::default()
                },
                1 << 3,
            ),
        )
        .unwrap();
        doc.forget_history();
        let original = quill_formats::native::write(&doc, 0);
        let mut state = WinState::new(DocView::new(doc, "Doc".into()));
        let (mut d, st) = crate::style_dialog::style(&state.view);
        let manual = st.chp;
        assert_eq!(
            d.options("character_style"),
            &["No Style", "Emphasis", "New Character Style\u{2026}"]
        );
        d.set_selected("character_style", 1);
        state.dialogs.push((Which::Style(st), d));
        let mut clip = Clipboard::new();
        state.dialog_outcome(Outcome::Changed("character_style"), &mut clip);
        let struck = |state: &WinState| {
            state.dialogs[0]
                .1
                .items
                .iter()
                .filter(|item| item.id.starts_with("char") && item.id.len() == 5)
                .all(|item| item.struck)
        };
        assert!(struck(&state), "the manual values show, struck through");
        assert!(
            state.dialogs[0]
                .1
                .items
                .iter()
                .filter(|item| ["ok", "apply", "define"].contains(&item.id))
                .all(|item| item.enabled),
            "saving works with a style chosen"
        );
        assert!(
            state.dialogs[0]
                .1
                .item_label("character_status")
                .starts_with("Linked to Emphasis (Italic).")
        );

        // Define saves the link; the text in that style takes Italic.
        state.dialog_outcome(Outcome::Button("define"), &mut clip);
        let normal = state.view.doc.styles.get(0);
        assert_eq!(normal.character, 1);
        assert_eq!(normal.own(), manual);
        assert!(state.view.doc.main.paras[0].chp_of_char(0).italic);
        // Choosing the style again shows its link.
        let Which::Style(st) = &state.dialogs[0].0 else {
            unreachable!()
        };
        assert_eq!(st.character, 1);
        assert_eq!(state.dialogs[0].1.selected("character_style"), 1);

        // No Style brings the manual formatting back.
        state.dialogs[0].1.set_selected("character_style", 0);
        state.dialog_outcome(Outcome::Changed("character_style"), &mut clip);
        assert!(!struck(&state));
        state.dialog_outcome(Outcome::Ok, &mut clip);
        assert!(state.dialogs.is_empty());
        assert_eq!(state.view.doc.styles.get(0).character, 0);
        assert!(!state.view.doc.main.paras[0].chp_of_char(0).italic);
        state.view.doc.undo();
        state.view.doc.undo();
        assert_eq!(quill_formats::native::write(&state.view.doc, 0), original);
    }

    #[test]
    fn paragraph_style_editor_stages_changes_and_cancel_keeps_the_document_clean() {
        let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
        let (d, st) = crate::style_dialog::style(&state.view);
        state.dialogs.push((Which::Style(st), d));
        let original = state.view.doc.styles.clone();
        let mut clip = Clipboard::new();
        state.dialog_outcome(Outcome::Button("paragraph"), &mut clip);
        state
            .dialogs
            .last_mut()
            .unwrap()
            .1
            .set_field("before", "18 pt");
        state.dialog_outcome(Outcome::Ok, &mut clip);
        assert_eq!(state.dialogs.len(), 1);
        assert_eq!(state.view.doc.styles, original);
        assert!(!state.view.doc.is_modified());
        assert!(matches!(&state.dialogs[0].0, Which::Style(st) if st.pap.before == 360));
        state.dialog_outcome(Outcome::Cancel, &mut clip);
        assert!(state.dialogs.is_empty());
        assert_eq!(state.view.doc.styles, original);
        assert!(!state.view.doc.is_modified());

        let (d, st) = crate::style_dialog::style(&state.view);
        state.dialogs.push((Which::Style(st), d));
        state.dialog_outcome(Outcome::Button("paragraph"), &mut clip);
        state
            .dialogs
            .last_mut()
            .unwrap()
            .1
            .set_field("before", "18 pt");
        state.dialog_outcome(Outcome::Cancel, &mut clip);
        state.dialog_outcome(Outcome::Button("define"), &mut clip);
        assert_eq!(state.view.doc.styles, original);
        assert!(!state.view.doc.is_modified());
        assert!(state.view.doc.undo_label().is_none());
    }

    #[test]
    fn defining_paragraph_format_changes_the_chosen_style_and_undo_restores_it() {
        let mut doc = quill_core::text::from_plain_text("Body\nHeading one\nHeading two");
        let heading = doc.styles.find("heading 1").unwrap();
        doc.apply_style(quill_core::StoryId::Main, 1, 2, heading);
        doc.forget_history();
        let original = quill_formats::native::write(&doc, 0);
        let mut state = WinState::new(DocView::new(doc, "Doc".into()));
        let (mut d, st) = crate::style_dialog::style(&state.view);
        let index = st
            .entries
            .iter()
            .position(|entry| *entry == Some(heading))
            .unwrap();
        d.set_selected("list", index);
        state.dialogs.push((Which::Style(st), d));
        let mut clip = Clipboard::new();
        state.dialog_outcome(Outcome::Changed("list"), &mut clip);
        let body = state.view.doc.main.paras[0].clone();
        let font = state.view.doc.styles.get(heading).chp;
        state.dialog_outcome(Outcome::Button("paragraph"), &mut clip);
        let editor = &mut state.dialogs.last_mut().unwrap().1;
        for (id, value) in [
            ("before", "18 pt"),
            ("after", "6 pt"),
            ("left", "0.5 in"),
            ("right", "0.25 in"),
            ("first", "0.125 in"),
            ("line_by", "18"),
        ] {
            editor.set_field(id, value);
        }
        editor.set_selected("line", 2);
        state.dialog_outcome(Outcome::Ok, &mut clip);
        assert_eq!(state.view.doc.main.paras[0], body);
        assert!(!state.view.doc.is_modified());
        state.dialog_outcome(Outcome::Button("define"), &mut clip);
        let style = state.view.doc.styles.get(heading);
        assert_eq!(style.chp, font);
        assert_eq!(
            (
                style.pap.before,
                style.pap.after,
                style.pap.left,
                style.pap.right,
                style.pap.first
            ),
            (360, 120, 720, 360, 180)
        );
        assert_eq!(style.pap.line, quill_core::LineSpacing::Exactly(360));
        for para in &state.view.doc.main.paras[1..] {
            assert_eq!(para.pap, style.pap);
        }
        assert_eq!(state.view.doc.main.paras[0], body);
        assert!(state.view.doc.is_modified());
        state.view.doc.undo();
        assert_eq!(quill_formats::native::write(&state.view.doc, 0), original);
        assert!(!state.view.doc.is_modified());
    }

    #[test]
    fn global_character_style_changes_refresh_inactive_headers_without_copying_old_text_back() {
        use quill_core::character_styles::CharacterFormat;
        let mut doc = Document::new();
        doc.headers[0] = quill_core::text::from_plain_text("Header").main;
        let format = CharacterFormat::new(
            quill_core::CharProps {
                italic: true,
                ..Default::default()
            },
            1 << 3,
        );
        let id = doc.add_character_style("Emphasis", format).unwrap();
        doc.apply_character_style(
            quill_core::StoryId::Header,
            Position::new(0, 0),
            Position::new(0, 6),
            id,
        );
        let mut state = WinState::new(DocView::new(doc, "Test".into()));
        state.open_pane(quill_core::StoryId::Header);
        state.focus_main();
        let format = CharacterFormat::new(
            quill_core::CharProps {
                italic: true,
                color: quill_core::Rgb(0xCC2200),
                ..Default::default()
            },
            (1 << 3) | (1 << 11),
        );
        state.view.doc.redefine_character_style(id, format);
        state.sync_pane();
        state.focus_pane();
        assert_eq!(
            state.view.doc.main.paras[0].chp_of_char(0).color,
            quill_core::Rgb(0xCC2200)
        );
    }

    #[test]
    fn pasted_character_styles_in_a_header_are_imported_with_their_definitions() {
        use quill_core::character_styles::CharacterFormat;
        let mut doc = Document::new();
        doc.headers[0] = quill_core::text::from_plain_text("Header").main;
        let mut state = WinState::new(DocView::new(doc, "Test".into()));
        state.open_pane(quill_core::StoryId::Header);
        let id = state
            .view
            .doc
            .add_character_style(
                "Pasted",
                CharacterFormat::new(
                    quill_core::CharProps {
                        italic: true,
                        ..Default::default()
                    },
                    1 << 3,
                ),
            )
            .unwrap();
        state.view.doc.apply_character_style(
            quill_core::StoryId::Main,
            Position::new(0, 0),
            Position::new(0, 6),
            id,
        );
        state.sync_pane();
        let main = state.main_view();
        let id = main.doc.styles.find_character("Pasted").unwrap();
        assert_eq!(
            main.doc.headers[0].paras[0].chp_of_char(0).character_style,
            id
        );
        assert!(main.doc.headers[0].paras[0].chp_of_char(0).italic);
    }

    #[test]
    fn a_recovered_window_is_saved_under_its_own_name() {
        let title = format!("Long.qwp{RECOVERED}");
        assert_eq!(title, "Long.qwp - Recovered");
        assert_eq!(strip_ext(&title), "Long");
        assert_eq!(strip_ext(&format!("Untitled1{RECOVERED}")), "Untitled1");
        assert_eq!(strip_ext("Notes.docx"), "Notes");
    }

    fn paint_editor(state: &mut WinState, width: u32, height: u32, fonts: &mut Fonts) {
        let mut cache = GlyphCache::new();
        let env = Env {
            font_names: &[],
            font_families: &[],
            recent: &[],
            windows: &[],
        };
        let mut pixels = vec![0; (width * height) as usize];
        state.paint(
            &mut Canvas::new(&mut pixels, width, height, 1.0),
            fonts,
            &mut cache,
            &env,
            true,
        );
    }

    #[test]
    fn comment_sidebar_resizes_scrolls_independently_and_keeps_edits() {
        use quill_core::{StoryId, notes::AnnotationInfo};
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let mut doc = quill_core::text::from_plain_text(&"Body paragraph\n".repeat(100));
        let id = doc.insert_annotation(
            Position::new(0, 4),
            Default::default(),
            AnnotationInfo {
                author: "Lucas".into(),
                initials: "L".into(),
                ..Default::default()
            },
        );
        let at = doc.annotation_text_start(id).unwrap();
        doc.insert_text(
            StoryId::Annotations,
            at,
            &"Long comment text. ".repeat(150),
            Default::default(),
        );
        let mut state = WinState::new(DocView::new(doc, "Review".into()));
        state.document_zoom = 1.0;
        state.open_note_in(StoryId::Annotations, id);
        paint_editor(&mut state, 900, 700, &mut fonts);
        let panel = state.pane.as_ref().unwrap().bounds;
        assert_eq!(panel.right(), 900.0 - BAR);
        assert_eq!(panel.y, state.text_top());
        assert!(state.main_view().area.right() <= panel.x);
        assert!(state.pane_view().unwrap().max_scroll() > 0.0);
        state.pointer = (20.0, 200.0);
        state.wheel(60.0);
        let body_scroll = state.main_view().scroll_y;
        assert!(body_scroll > 0.0);
        assert_eq!(state.pane_view().unwrap().scroll_y, 0.0);
        state.pointer = (panel.x + 20.0, panel.y + 140.0);
        state.wheel(60.0);
        assert!(state.pane_view().unwrap().scroll_y > 0.0);
        assert_eq!(state.main_view().scroll_y, body_scroll);
        state.view.type_text("Edited comment. ");
        state.focus_main();
        assert!(
            state
                .main_view()
                .doc
                .annotation_text(id)
                .contains("Edited comment.")
        );
        paint_editor(&mut state, 480, 700, &mut fonts);
        let panel = state.pane.as_ref().unwrap().bounds;
        assert_eq!(panel.right(), 480.0 - BAR);
        assert!(state.pane_view().unwrap().area.w < 240.0);
        assert_eq!(
            pane::hit(
                panel,
                StoryId::Annotations,
                false,
                panel.right() - 20.0,
                panel.y + 16.0
            ),
            PaneHit::Close
        );
        state.native_scrollbars = true;
        paint_editor(&mut state, 480, 700, &mut fonts);
        assert!(state.pane_view().unwrap().area.right() <= state.vscroll_other.x);
        assert!(state.main_view().area.right() <= state.vscroll.x);
        state.close_pane();
        paint_editor(&mut state, 480, 700, &mut fonts);
        assert!(state.pane.is_none());
        assert!(
            state
                .view
                .doc
                .annotation_text(id)
                .contains("Edited comment.")
        );
    }

    #[test]
    fn shared_review_filters_preserve_comment_edits_and_badges_reopen_the_pane() {
        use quill_core::notes::AnnotationInfo;
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let mut doc = quill_core::text::from_plain_text("first second");
        let comment = doc.insert_annotation_on(
            Position::new(0, 0),
            Position::new(0, 5),
            Default::default(),
            AnnotationInfo {
                author: "Alex".into(),
                ..Default::default()
            },
        );
        let at = doc.annotation_text_start(comment).unwrap();
        doc.insert_text(
            quill_core::StoryId::Annotations,
            at,
            "Original comment",
            Default::default(),
        );
        for (word, author) in [("first", "Ada"), ("second", "Grace")] {
            let a = doc.main.paras[0].text.find(word).unwrap();
            let b = a + word.len();
            let id = quill_core::revision::intern(quill_core::revision::Revision {
                inserted: Some(quill_core::revision::Stamp::new(author, "2026-10-23")),
                ..Default::default()
            });
            doc.format_chars(
                quill_core::StoryId::Main,
                Position::new(0, a),
                Position::new(0, b),
                |c| c.rev = id,
            );
        }
        let mut state = WinState::new(DocView::new(doc, "Review".into()));
        paint_editor(&mut state, 900, 700, &mut fonts);
        assert_eq!(
            state.pane.as_ref().unwrap().story,
            quill_core::StoryId::Annotations
        );
        let panel = state.pane.as_ref().unwrap().bounds;
        assert!(state.pane_view().unwrap().area.y > panel.y + 100.0);
        state.focus_pane();
        state.view.set_selection(Selection::caret(at));
        state.view.insert_text("Edited ");
        state.revision_click(panel.x + panel.w * 0.85, panel.y + 45.0);
        assert!(state.review_filter == revisions::Filter::Changes);
        paint_editor(&mut state, 900, 700, &mut fonts);
        assert_eq!(state.pane_view().unwrap().area.h, 0.0);
        assert!(
            state
                .main_view()
                .doc
                .annotation_text(comment)
                .contains("Edited Original")
        );
        state.revision_click(panel.x + panel.w * 0.5, panel.y + 45.0);
        paint_editor(&mut state, 900, 700, &mut fonts);
        assert!(state.review_filter == revisions::Filter::Comments);
        assert!(state.pane_view().unwrap().area.h > 0.0);
        assert_eq!(state.revision_badges.len(), 1);
        assert!(state.revision_badges[0].tip.contains("Ada"));
        assert!(state.revision_badges[0].tip.contains("Grace"));
        state.revision_hover = Some(0);
        state.retip(0.0, 0.0);
        assert!(state.tip.as_ref().unwrap().text.contains("Inserted by Ada"));
        assert!(
            state
                .tip
                .as_ref()
                .unwrap()
                .text
                .contains("Inserted by Grace")
        );
        let badge = state.revision_badges[0].rect;
        state.revision_click(badge.x + 5.0, badge.y + 5.0);
        assert!(state.review_filter == revisions::Filter::All);
        let first = state.main_view().change_here().unwrap();
        state.revision_click(badge.x + 5.0, badge.y + 5.0);
        assert_ne!(state.main_view().change_here().unwrap(), first);
        state.close_pane();
        paint_editor(&mut state, 480, 700, &mut fonts);
        assert!(state.pane.is_none());
        let badge = state.revision_badges[0].rect;
        assert!(badge.x >= state.main_view().area.right() * state.document_zoom / state.ui);
        state.revision_click(badge.x + 5.0, badge.y + 5.0);
        paint_editor(&mut state, 480, 700, &mut fonts);
        assert!(state.pane.is_some());
        assert!(
            state
                .main_view()
                .doc
                .annotation_text(comment)
                .contains("Edited Original")
        );
        state.main_view_mut().show_changes = false;
        paint_editor(&mut state, 480, 700, &mut fonts);
        assert!(state.revision_badges.is_empty());
    }

    #[test]
    fn review_accept_reject_keeps_comments_and_is_undoable() {
        let mut view = DocView::new(
            quill_core::text::from_plain_text("Anchor "),
            "Review".into(),
        );
        view.set_selection(Selection::caret(Position::new(0, 7)));
        view.doc.props.track_changes = true;
        view.doc.author = "Ada".into();
        view.insert_text("inserted");
        view.doc.props.track_changes = false;
        let id = view.doc.insert_annotation(
            Position::new(0, 0),
            Default::default(),
            quill_core::notes::AnnotationInfo {
                author: "Grace".into(),
                ..Default::default()
            },
        );
        let at = view.doc.annotation_text_start(id).unwrap();
        view.doc.insert_text(
            quill_core::StoryId::Annotations,
            at,
            "Keep this discussion",
            Default::default(),
        );
        view.doc.forget_history();
        let mut state = WinState::new(view);
        state.width = 900.0;
        state.height = 700.0;
        state.ensure_review();
        let r = state.revisions_bounds().unwrap();
        let buttons = revisions::review_buttons(r);
        let center = |b: Rect| (b.x + b.w / 2.0, b.y + b.h / 2.0);
        let (x, y) = center(buttons[2]);
        state.revision_click(x, y);
        assert!(!state.main_view().doc.has_changes());
        assert_eq!(
            state.main_view().doc.annotation_text(id),
            "Keep this discussion"
        );
        assert!(state.main_view_mut().doc.undo().is_some());
        assert!(state.main_view().doc.has_changes());
        let (x, y) = center(buttons[3]);
        state.revision_click(x, y);
        assert!(!state.main_view().doc.has_changes());
        assert!(
            state.main_view().doc.main.paras[0]
                .text
                .contains(quill_core::para::chars::ANNOTATION)
        );
        assert_eq!(
            state.main_view().doc.annotation_text(id),
            "Keep this discussion"
        );
    }

    #[test]
    fn review_divider_drag_resizes_both_sides_without_editing() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        for ui in [1.0, 1.5] {
            let mut doc = quill_core::text::from_plain_text("Document body");
            let id =
                doc.insert_annotation(Position::new(0, 0), Default::default(), Default::default());
            let at = doc.annotation_text_start(id).unwrap();
            doc.insert_text(
                quill_core::StoryId::Annotations,
                at,
                "Comment text",
                Default::default(),
            );
            doc.forget_history();
            let mut state = WinState::new(DocView::new(doc, "Review".into()));
            state.ui = ui;
            state.document_zoom = 1.25;
            paint_editor(&mut state, 900, 700, &mut fonts);
            let before = state.pane.as_ref().unwrap().bounds;
            let body_width = state.main_view().area.w;
            let main = state.main_view().doc.main.clone();
            let modified = state.main_view().doc.is_modified();
            let env = Env {
                font_names: &[],
                font_families: &[],
                recent: &[],
                windows: &[],
            };
            let mut cache = GlyphCache::new();
            let mut label = Label {
                fonts: &mut fonts,
                cache: &mut cache,
            };
            let mut clip = Clipboard::new();
            let x = before.x * ui;
            let y = (before.y + 120.0) * ui;
            assert_eq!(state.cursor_icon(x, y), CursorIcon::ColResize);
            state.mouse_down(x, y, Mods::default(), &mut label, &env, &mut clip);
            assert!(state.review_resize.is_some());
            state.mouse_move(x - 40.0 * ui, y, &mut label, &env);
            state.mouse_up(x - 40.0 * ui, y, &mut clip);
            assert!(state.review_resize.is_none());
            paint_editor(&mut state, 900, 700, &mut fonts);
            let after = state.pane.as_ref().unwrap().bounds;
            assert!((after.w - before.w - 40.0).abs() < 0.01);
            assert!(state.main_view().area.w < body_width);
            assert!(state.pane_view().unwrap().area.w > 0.0);
            assert_eq!(state.main_view().doc.main, main);
            assert_eq!(state.main_view().doc.annotation_text(id), "Comment text");
            assert_eq!(state.main_view().doc.is_modified(), modified);
            assert!(state.main_view().doc.undo_label().is_none());
            state.close_pane();
            state.open_pane(quill_core::StoryId::Annotations);
            paint_editor(&mut state, 900, 700, &mut fonts);
            assert_eq!(state.pane.as_ref().unwrap().bounds.w, after.w);
            state.review_resize = Some(0.0);
            state.resize_review(-10000.0);
            assert!(state.revisions_bounds().unwrap().x >= 240.0);
            state.resize_review(10000.0);
            assert_eq!(state.revisions_bounds().unwrap().w, 200.0);
            state.review_resize = None;
        }
    }

    #[test]
    #[ignore = "manual divider redraw timing; run with --release --ignored --nocapture"]
    fn review_divider_redraw_timing() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let mut doc = quill_core::text::from_plain_text(&"Document paragraph.\n".repeat(30));
        for _ in 0..20 {
            let id =
                doc.insert_annotation(Position::new(0, 0), Default::default(), Default::default());
            let at = doc.annotation_text_start(id).unwrap();
            doc.insert_text(
                quill_core::StoryId::Annotations,
                at,
                &"Review comment with enough text to wrap at several widths. ".repeat(35),
                Default::default(),
            );
        }
        doc.forget_history();
        let mut state = WinState::new(DocView::new(doc, "Resize timing".into()));
        state.document_zoom = 1.0;
        state.review_filter = revisions::Filter::Comments;
        let env = Env {
            font_names: &[],
            font_families: &[],
            recent: &[],
            windows: &[],
        };
        let mut cache = GlyphCache::new();
        let mut pixels = vec![0; 1200 * 800];
        state.paint(
            &mut Canvas::new(&mut pixels, 1200, 800, 1.0),
            &mut fonts,
            &mut cache,
            &env,
            true,
        );
        let panel = state.revisions_bounds().unwrap();
        state.mouse_down(
            panel.x,
            panel.y + 120.0,
            Mods::default(),
            &mut Label {
                fonts: &mut fonts,
                cache: &mut cache,
            },
            &env,
            &mut Clipboard::new(),
        );
        let mut timings = Vec::new();
        for i in 0..24 {
            let start = Instant::now();
            state.mouse_move(
                550.0 + (i * 17 % 200) as f32,
                panel.y + 120.0,
                &mut Label {
                    fonts: &mut fonts,
                    cache: &mut cache,
                },
                &env,
            );
            state.paint(
                &mut Canvas::new(&mut pixels, 1200, 800, 1.0),
                &mut fonts,
                &mut cache,
                &env,
                true,
            );
            timings.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        timings.sort_by(f64::total_cmp);
        eprintln!(
            "Divider redraw: mean {:.2} ms, p95 {:.2} ms",
            timings.iter().sum::<f64>() / timings.len() as f64,
            timings[22]
        );
    }

    #[test]
    fn divider_moves_live_but_rewraps_comments_only_on_release() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let mut doc = quill_core::text::from_plain_text("Body text");
        let id = doc.insert_annotation(Position::new(0, 0), Default::default(), Default::default());
        let at = doc.annotation_text_start(id).unwrap();
        let text = "A longer comment to check wrapping during divider movement. ".repeat(60);
        doc.insert_text(
            quill_core::StoryId::Annotations,
            at,
            &text,
            Default::default(),
        );
        doc.forget_history();
        let mut state = WinState::new(DocView::new(doc, "Review".into()));
        state.document_zoom = 1.0;
        state.open_note_in(quill_core::StoryId::Annotations, id);
        paint_editor(&mut state, 900, 700, &mut fonts);
        let panel = state.revisions_bounds().unwrap();
        let old_height = state.pane_view().unwrap().total_height();
        let env = Env {
            font_names: &[],
            font_families: &[],
            recent: &[],
            windows: &[],
        };
        let mut cache = GlyphCache::new();
        let mut clip = Clipboard::new();
        state.mouse_down(
            panel.x,
            panel.y + 120.0,
            Mods::default(),
            &mut Label {
                fonts: &mut fonts,
                cache: &mut cache,
            },
            &env,
            &mut clip,
        );
        for x in [panel.x - 160.0, panel.x + 60.0] {
            state.mouse_move(
                x,
                panel.y + 120.0,
                &mut Label {
                    fonts: &mut fonts,
                    cache: &mut cache,
                },
                &env,
            );
            paint_editor(&mut state, 900, 700, &mut fonts);
            assert!(
                (state.revisions_bounds().unwrap().x - x).abs() < 0.01,
                "border must follow each pointer position"
            );
            assert_eq!(
                state.pane_view().unwrap().total_height(),
                old_height,
                "dragging must reuse the comment layout"
            );
        }
        // Editing during a held drag must still invalidate and prepare safely.
        state.view.type_text("Added ");
        paint_editor(&mut state, 900, 700, &mut fonts);
        state.mouse_up(panel.x + 60.0, panel.y + 120.0, &mut clip);
        assert!(state.review_resize_text_width.is_none());
        paint_editor(&mut state, 900, 700, &mut fonts);
        assert!(
            state.pane_view().unwrap().total_height() > old_height,
            "release must wrap to the narrower width"
        );
        state.close_pane();
        assert!(state.main_view().doc.annotation_text(id).contains("Added "));
        assert!(
            state
                .main_view()
                .doc
                .annotation_text(id)
                .contains(text.trim())
        );
    }

    #[test]
    fn every_divider_grab_point_is_clear_of_the_native_scrollbar() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let env = Env {
            font_names: &[],
            font_families: &[],
            recent: &[],
            windows: &[],
        };
        let mut cache = GlyphCache::new();
        let mut clip = Clipboard::new();
        for ui in [1.0, 1.5, 2.0] {
            for native in [false, true] {
                for grab in [-7.0, -4.0, -1.0, 3.0] {
                    let mut doc = quill_core::text::from_plain_text("Document body");
                    doc.insert_annotation(
                        Position::new(0, 0),
                        Default::default(),
                        Default::default(),
                    );
                    let mut state = WinState::new(DocView::new(doc, "Review".into()));
                    state.ui = ui;
                    state.native_scrollbars = native;
                    paint_editor(&mut state, 1200, 800, &mut fonts);
                    let r = state.revisions_bounds().unwrap();
                    // mac_widgets positions the native NSScrollView by this
                    // right edge, even while its overlay thumb is invisible.
                    let native_right = state.vscroll.right() * ui;
                    let native_left = state.vscroll.x * ui;
                    let grab_left = (r.x - revisions::DIVIDER_WIDTH) * ui;
                    let grab_right = (r.x + 4.0) * ui;
                    assert!(
                        native_right <= grab_left || native_left >= grab_right,
                        "native scroll view must not cover any of the grab strip"
                    );
                    let (x, y) = ((r.x + grab) * ui, (r.y + 120.0) * ui);
                    assert_eq!(state.cursor_icon(x, y), CursorIcon::ColResize);
                    state.mouse_down(
                        x,
                        y,
                        Mods::default(),
                        &mut Label {
                            fonts: &mut fonts,
                            cache: &mut cache,
                        },
                        &env,
                        &mut clip,
                    );
                    assert!(
                        state.review_resize.is_some(),
                        "the first click must grab the divider"
                    );
                    state.mouse_move(
                        x - 30.0 * ui,
                        y,
                        &mut Label {
                            fonts: &mut fonts,
                            cache: &mut cache,
                        },
                        &env,
                    );
                    state.mouse_up(x - 30.0 * ui, y, &mut clip);
                    assert!((state.revisions_bounds().unwrap().x - (r.x - 30.0)).abs() < 0.01);
                }
            }
        }
    }

    #[test]
    fn review_resizing_keeps_document_scrollbar_at_the_window_edge() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let env = Env {
            font_names: &[],
            font_families: &[],
            recent: &[],
            windows: &[],
        };
        let mut cache = GlyphCache::new();
        let mut clip = Clipboard::new();
        for native in [false, true] {
            let mut doc = quill_core::text::from_plain_text(&"Body paragraph\n".repeat(100));
            let id =
                doc.insert_annotation(Position::new(0, 0), Default::default(), Default::default());
            let at = doc.annotation_text_start(id).unwrap();
            doc.insert_text(
                quill_core::StoryId::Annotations,
                at,
                &"Comment text. ".repeat(200),
                Default::default(),
            );
            let mut state = WinState::new(DocView::new(doc, "Review".into()));
            state.native_scrollbars = native;
            state.document_zoom = 1.0;
            paint_editor(&mut state, 900, 700, &mut fonts);
            let bar = state.vscroll;
            let comments_bar_x = state.vscroll_other.x;
            assert_eq!(bar.right(), 900.0);
            assert_eq!(state.revisions_bounds().unwrap().right(), bar.x);
            let panel = state.revisions_bounds().unwrap();
            state.mouse_down(
                panel.x,
                panel.y + 120.0,
                Mods::default(),
                &mut Label {
                    fonts: &mut fonts,
                    cache: &mut cache,
                },
                &env,
                &mut clip,
            );
            for x in [panel.x - 100.0, panel.x + 60.0] {
                state.mouse_move(
                    x,
                    panel.y + 120.0,
                    &mut Label {
                        fonts: &mut fonts,
                        cache: &mut cache,
                    },
                    &env,
                );
                paint_editor(&mut state, 900, 700, &mut fonts);
                assert_eq!(
                    state.vscroll, bar,
                    "document scrollbar must not follow the divider"
                );
                assert_eq!(state.vscroll_other.x, comments_bar_x);
                assert!((state.revisions_bounds().unwrap().x - x).abs() < 0.01);
            }
            state.mouse_up(panel.x + 60.0, panel.y + 120.0, &mut clip);
            paint_editor(&mut state, 900, 700, &mut fonts);
            // The rightmost control scrolls the document, even if comments
            // previously had keyboard focus. Wheel scrolling in Review stays local.
            let comments = state.pane_view().unwrap().area;
            state.pointer = (comments.x + 20.0, comments.y + 30.0);
            state.wheel(60.0);
            let comment_scroll = state.pane_view().unwrap().scroll_y;
            assert!(comment_scroll > 0.0);
            assert_eq!(state.main_view().scroll_y, 0.0);
            if !native {
                state.focus_pane();
                let (x, y) = (bar.x + BAR / 2.0, bar.bottom() - 3.0);
                state.mouse_down(
                    x,
                    y,
                    Mods::default(),
                    &mut Label {
                        fonts: &mut fonts,
                        cache: &mut cache,
                    },
                    &env,
                    &mut clip,
                );
                state.mouse_up(x, y, &mut clip);
                assert!(state.main_view().scroll_y > 0.0);
                assert_eq!(state.pane_view().unwrap().scroll_y, comment_scroll);
            }
            state.close_pane();
            paint_editor(&mut state, 900, 700, &mut fonts);
            assert_eq!(state.vscroll.x, bar.x);
        }
    }

    #[cfg(target_os = "macos")]
    fn assert_visual_baseline(
        name: &str,
        width: u32,
        height: u32,
        scale: f32,
        script: &str,
        expected: &[u8],
    ) {
        let path =
            std::env::temp_dir().join(format!("quill-visual-{name}-{}.png", std::process::id()));
        crate::settings::update(|settings| *settings = crate::settings::Settings::default());
        snapshot(&path, width, height, scale, None, script);
        let actual = std::fs::read(&path).expect("read rendered visual snapshot");
        let _ = std::fs::remove_file(path);
        let (ew, eh, expected) = crate::image::decode_rgba(expected, quill_core::ImageFormat::Png)
            .expect("decode visual baseline");
        let (aw, ah, actual) = crate::image::decode_rgba(&actual, quill_core::ImageFormat::Png)
            .expect("decode rendered visual snapshot");
        assert_eq!((aw, ah), (ew, eh), "{name} dimensions changed");

        let mut changed = 0usize;
        let mut total_delta = 0u64;
        let (expected, expected_tail) = expected.as_chunks::<4>();
        let (actual, actual_tail) = actual.as_chunks::<4>();
        assert!(expected_tail.is_empty() && actual_tail.is_empty());
        for (before, after) in expected.iter().zip(actual) {
            let delta = before[..3]
                .iter()
                .zip(&after[..3])
                .map(|(&a, &b)| a.abs_diff(b) as u64)
                .sum::<u64>();
            total_delta += delta;
            changed += usize::from(delta > 24);
        }
        let pixels = (aw as usize) * (ah as usize);
        let changed_fraction = changed as f64 / pixels as f64;
        let mean_channel_delta = total_delta as f64 / (pixels * 3) as f64;
        assert!(
            changed_fraction <= 0.025 && mean_channel_delta <= 1.5,
            "{name} changed visually: {:.2}% pixels, mean channel delta {:.2}",
            changed_fraction * 100.0,
            mean_channel_delta
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn representative_window_visuals_match_baselines() {
        let sample = "type:Quill visual baseline\\nThe quick brown fox jumps over the lazy dog.";
        assert_visual_baseline(
            "compact-100",
            640,
            520,
            1.0,
            &format!("zoom:100|{sample}"),
            include_bytes!("../../../tests/visual/compact-100.png"),
        );
        assert_visual_baseline(
            "comfortable-retina-150",
            900,
            700,
            2.0,
            &format!("zoom:150|{sample}"),
            include_bytes!("../../../tests/visual/comfortable-retina-150.png"),
        );
        assert_visual_baseline(
            "wide-200",
            1200,
            720,
            1.0,
            &format!("zoom:200|{sample}"),
            include_bytes!("../../../tests/visual/wide-200.png"),
        );
    }

    #[test]
    fn initial_window_uses_the_display_without_filling_it() {
        let laptop = initial_window_size(1512.0, 982.0);
        assert!((laptop.width - 1285.2).abs() < 0.01);
        assert!((laptop.height - 834.7).abs() < 0.01);

        let large = initial_window_size(2560.0, 1440.0);
        assert_eq!(large, LogicalSize::new(1400.0, 1000.0));

        let small = initial_window_size(640.0, 480.0);
        assert_eq!(small, LogicalSize::new(640.0, 480.0));
    }

    #[test]
    fn document_zoom_and_pointer_coordinates_are_inverse() {
        let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
        state.document_zoom = 1.25;
        assert_eq!(state.document_point(125.0, 250.0), (100.0, 200.0));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_scrollbar_actions_move_and_clamp() {
        assert_eq!(native_scroll_value(200.0, 500.0, 100.0, 1, 0.0), 100.0);
        assert_eq!(native_scroll_value(200.0, 500.0, 100.0, 3, 0.0), 300.0);
        assert_eq!(native_scroll_value(200.0, 500.0, 100.0, 4, 0.0), 184.0);
        assert_eq!(native_scroll_value(200.0, 500.0, 100.0, 5, 0.0), 216.0);
        assert_eq!(native_scroll_value(200.0, 500.0, 100.0, 2, 0.75), 375.0);
        assert_eq!(native_scroll_value(10.0, 500.0, 100.0, 1, 0.0), 0.0);
        assert_eq!(native_scroll_value(490.0, 500.0, 100.0, 3, 0.0), 500.0);
    }

    #[test]
    fn choosing_a_font_family_applies_its_normal_variant() {
        let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
        state.view.type_text("x");
        state.view.sel = Selection::new(Position::new(0, 0), Position::new(0, 1));
        state.view.format_chars("Formatting", |c| {
            c.bold = true;
            c.italic = true;
        });
        state.act(
            Action::FontVariant("Arial".into(), false, false),
            &mut Clipboard::new(),
        );
        let chp = state.view.current_chp();
        assert_eq!(state.view.doc.font_name(chp.font), "Arial");
        assert!(!chp.bold);
        assert!(!chp.italic);
    }

    #[test]
    fn application_menu_actions_escape_the_portable_window() {
        let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
        let clip = &mut Clipboard::new();
        assert_eq!(state.act(Action::About, clip), Effect::About);
        assert_eq!(state.act(Action::Recent(2), clip), Effect::OpenRecent(2));
        assert_eq!(
            state.act(Action::RemoveRecent(1), clip),
            Effect::RemoveRecent(1)
        );
        assert_eq!(state.act(Action::ClearRecent, clip), Effect::ClearRecent);
    }

    #[test]
    fn collapsed_replace_cannot_change_document_from_stale_actions() {
        let mut state = WinState::new(DocView::new(Document::new(), "Find".into()));
        state.view.insert_text("cat dog cat");
        let (mut d, mut st) = crate::find_dialog::dialog(&state.view, false);
        d.set_field("find", "cat");
        crate::find_dialog::switch_mode(&mut st, &mut d, &state.view, true);
        d.set_field("with", "bird");
        crate::find_dialog::switch_mode(&mut st, &mut d, &state.view, false);
        assert!(!d.items.iter().any(|i| matches!(
            i.id,
            "with" | "replace" | "replaceall" | "fmt_rep" | "spc_rep"
        )));
        state.dialogs.push((Which::Find(st), d));
        state
            .view
            .set_selection(quill_core::Selection::caret(Position::new(0, 0)));
        let mut clip = Clipboard::without_system();
        for outcome in [
            Outcome::Button("replace"),
            Outcome::Button("replaceall"),
            Outcome::Ok,
        ] {
            state.tool_outcome(&outcome, &mut clip);
            assert_eq!(state.view.doc.main.paras[0].text, "cat dog cat");
            assert_eq!(state.dialogs.len(), 1);
        }
        let (Which::Find(st), _) = &mut state.dialogs[0] else {
            panic!("expected Find")
        };
        assert_eq!(
            crate::find_dialog::replace(st, &mut state.view, &mut clip),
            crate::find_dialog::Hit::NotFound
        );
        assert_eq!(
            crate::find_dialog::replace_all(st, &mut state.view, &mut clip),
            (0, false)
        );
        assert_eq!(state.view.doc.main.paras[0].text, "cat dog cat");
        state.tool_outcome(&Outcome::Button("mode"), &mut clip);
        assert_eq!(state.dialogs[0].1.field_text("with"), "bird");
        state.tool_outcome(&Outcome::Button("replace"), &mut clip);
        assert_eq!(state.view.doc.main.paras[0].text, "bird dog cat");
    }

    #[test]
    fn native_find_releases_document_input_after_a_search() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let mut state = WinState::new(DocView::new(Document::new(), "Find".into()));
        state.view.insert_text("cat dog");
        let (mut d, mut st) = crate::find_dialog::dialog(&state.view, false);
        d.set_field("find", "cat");
        crate::find_dialog::update(&mut st, &mut d, &state.view);
        let search = st.search.clone();
        state.dialogs.push((Which::Find(st), d));
        state
            .view
            .set_selection(quill_core::Selection::caret(Position::new(0, 0)));
        let hit = crate::find_dialog::find_next(&mut state.view, &search);
        state.after_search(hit, &search);
        assert_eq!(hit, crate::find_dialog::Hit::Match);
        state.native_dialogs = true;
        let env = Env {
            font_names: &[],
            font_families: &[],
            recent: &[],
            windows: &[],
        };
        let mut cache = GlyphCache::new();
        let mut label = Label {
            fonts: &mut fonts,
            cache: &mut cache,
        };
        let mut clip = Clipboard::without_system();
        // This point lies inside the old canvas panel rectangle, but the
        // event came from the document, not the separate native panel.
        assert!(state.dialogs[0].1.contains(200.0, 120.0));
        state.mouse_down(
            200.0 * state.ui,
            120.0 * state.ui,
            Mods::default(),
            &mut label,
            &env,
            &mut clip,
        );
        assert!(!state.dialog_has_keys());
        assert!(state.dialogs[0].1.modeless);
    }

    #[test]
    fn modal_dialogs_block_commands_but_find_remains_modeless() {
        let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
        assert!(!state.modal_dialog_open());

        let mut find = Dialog::new("Find", 100.0, 50.0);
        find.modeless = true;
        state.dialogs.push((Which::GoTo, find));
        assert!(!state.modal_dialog_open());
        assert!(state.dialog_has_keys());

        state.dialogs.last_mut().unwrap().1.inactive = true;
        assert!(!state.dialog_has_keys());

        state
            .dialogs
            .push((Which::GoTo, Dialog::new("Character", 100.0, 50.0)));
        assert!(state.modal_dialog_open());
        assert!(state.dialog_has_keys());
    }

    #[test]
    fn more_colors_opens_the_palette_and_what_it_picks_goes_into_the_popup() {
        let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
        let (character, which) = format_dialogs::character_for_find(&state.view, &[]);
        state.dialogs.push((which, character));
        let mut clip = Clipboard::without_system();

        // More Colors… from the Color popup opens the palette on top.
        let n = state.dialogs[0].1.options("color").len();
        state.dialogs[0].1.set_selected("color", n - 1);
        state.dialog_outcome(Outcome::Changed("color"), &mut clip);
        assert_eq!(state.dialogs.len(), 2);
        assert!(matches!(state.dialogs[1].0, Which::Palette(_)));

        // A color typed as an HTML code, then OK: the popup shows it.
        state.dialogs[1].1.set_field("hex", "#336699");
        state.dialog_outcome(Outcome::Changed("hex"), &mut clip);
        state.dialog_outcome(Outcome::Ok, &mut clip);
        assert_eq!(state.dialogs.len(), 1);
        assert_eq!(state.dialogs[0].1.selected_text("color"), "#336699");

        // Cancel in the palette puts back the color the popup had.
        let n = state.dialogs[0].1.options("color").len();
        state.dialogs[0].1.set_selected("color", n - 1);
        state.dialog_outcome(Outcome::Changed("color"), &mut clip);
        state.dialogs[1].1.set_field("hex", "#FF00FF");
        state.dialog_outcome(Outcome::Changed("hex"), &mut clip);
        state.dialog_outcome(Outcome::Cancel, &mut clip);
        assert_eq!(state.dialogs.len(), 1);
        assert_eq!(state.dialogs[0].1.selected_text("color"), "Black");

        // Edit… opens the editor; its OK adds the color to the palette.
        let n = state.dialogs[0].1.options("color").len();
        state.dialogs[0].1.set_selected("color", n - 1);
        state.dialog_outcome(Outcome::Changed("color"), &mut clip);
        state.dialog_outcome(Outcome::Button("edit"), &mut clip);
        assert!(matches!(state.dialogs[2].0, Which::EditColor(_)));
        state.dialogs[2].1.set_field("hex", "#102030");
        state.dialog_outcome(Outcome::Changed("hex"), &mut clip);
        state.dialog_outcome(Outcome::Ok, &mut clip);
        assert_eq!(state.dialogs.len(), 2);
        let Which::Palette(p) = &state.dialogs[1].0 else {
            panic!("the palette is back on top");
        };
        assert_eq!(p.custom.first(), Some(&quill_core::props::Rgb(0x102030)));
        assert_eq!(p.current, quill_core::props::Rgb(0x102030));
    }

    #[test]
    fn find_format_character_more_colors_is_handled() {
        let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
        let (character, _) = format_dialogs::character_for_find(&state.view, &[]);
        state.dialogs.push((
            Which::FindFormat {
                replace: false,
                kind: 0,
            },
            character,
        ));
        let mut clip = Clipboard::without_system();

        let n = state.dialogs[0].1.options("color").len();
        state.dialogs[0].1.set_selected("color", n - 1);
        state.dialog_outcome(Outcome::Changed("color"), &mut clip);

        assert_eq!(state.dialogs.len(), 2);
        assert!(matches!(state.dialogs[1].0, Which::Palette(_)));
    }

    #[test]
    fn the_color_button_pops_out_a_palette_and_boxes_choose_and_edit() {
        let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
        let (character, which) = format_dialogs::build_character_with_button(&state.view);
        state.dialogs.push((which, character));
        let mut clip = Clipboard::without_system();
        let rgb = |s: &WinState| s.dialogs[0].1.swatch_rgb("color").unwrap();
        assert_eq!(rgb(&state), 0x000000);
        assert_eq!(state.dialogs[0].1.item_label("color_hex"), "#000000");

        // Pressing the button pops the palette out; pressing it again puts it away.
        state.dialog_outcome(Outcome::Button("color"), &mut clip);
        let pop = state.dialogs[0]
            .1
            .popout
            .clone()
            .expect("the palette is out");
        assert_eq!(pop.anchor, "color");
        assert_eq!(pop.items.iter().filter(|i| i.label.is_empty()).count(), 32);
        assert!(pop.items.iter().any(|i| i.id == "std0" && i.selected));
        state.dialog_outcome(Outcome::Button("color"), &mut clip);
        assert!(state.dialogs[0].1.popout.is_none());

        // A box chooses its color and the palette stays out, showing it marked.
        state.dialog_outcome(Outcome::Button("color"), &mut clip);
        state.dialog_outcome(
            Outcome::Swatch {
                id: "std5",
                double: false,
            },
            &mut clip,
        );
        assert_eq!(rgb(&state), 0xFF0000);
        assert_eq!(state.dialogs[0].1.item_label("color_hex"), "#FF0000");
        let pop = state.dialogs[0].1.popout.clone().unwrap();
        assert!(pop.items.iter().any(|i| i.id == "std5" && i.selected));
        assert!(!pop.items.iter().any(|i| i.id == "std0" && i.selected));

        // A double-click edits it: the editor opens and the palette goes in.
        state.dialog_outcome(
            Outcome::Swatch {
                id: "std9",
                double: true,
            },
            &mut clip,
        );
        assert!(matches!(state.dialogs[1].0, Which::EditColor(_)));
        assert!(state.dialogs[0].1.popout.is_none());
        state.dialogs[1].1.set_field("hex", "#102030");
        state.dialog_outcome(Outcome::Changed("hex"), &mut clip);
        state.dialog_outcome(Outcome::Ok, &mut clip);
        assert_eq!(state.dialogs.len(), 1);
        assert_eq!(rgb(&state), 0x102030);
        assert_eq!(state.dialogs[0].1.item_label("color_hex"), "#102030");

        // The next time the palette is out, the edited color is a custom one.
        state.dialog_outcome(Outcome::Button("color"), &mut clip);
        let pop = state.dialogs[0].1.popout.clone().unwrap();
        assert!(
            pop.items
                .iter()
                .any(|i| i.id == "cus0" && i.rgb == Some(0x102030) && i.selected)
        );
        // Edit Color… in the palette edits the one chosen, in its box.
        state.dialog_outcome(Outcome::Button("edit"), &mut clip);
        assert!(matches!(state.dialogs[1].0, Which::EditColor(_)));
        state.dialogs[1].1.set_field("hex", "#445566");
        state.dialog_outcome(Outcome::Changed("hex"), &mut clip);
        state.dialog_outcome(Outcome::Ok, &mut clip);
        assert_eq!(rgb(&state), 0x445566);
        state.dialog_outcome(Outcome::Button("color"), &mut clip);
        let pop = state.dialogs[0].1.popout.clone().unwrap();
        assert!(
            pop.items
                .iter()
                .any(|i| i.id == "cus0" && i.rgb == Some(0x445566))
        );
        assert!(!pop.items.iter().any(|i| i.rgb == Some(0x102030)));
    }

    use quill_core::Rgb;

    /// A window on "Blue words, plain", its first word in #336699 and the
    /// Character dialog open with its color button.
    fn blue_words() -> WinState {
        let mut doc = quill_core::text::from_plain_text("Blue words, plain");
        doc.format_chars(
            quill_core::StoryId::Main,
            Position::new(0, 0),
            Position::new(0, 4),
            |c| c.color = Rgb(0x336699),
        );
        doc.forget_history();
        let mut state = WinState::new(DocView::new(doc, "Doc".into()));
        let (character, which) = format_dialogs::build_character_with_button(&state.view);
        state.dialogs.push((which, character));
        state
    }

    fn pop_item(state: &WinState, id: &str) -> crate::dialog::PopItem {
        let pop = state.dialogs.last().unwrap().1.popout.clone().expect("out");
        pop.items.into_iter().find(|i| i.id == id).expect(id)
    }

    #[test]
    fn deleting_a_color_the_document_uses_asks_and_makes_its_text_black() {
        let mut state = blue_words();
        let mut clip = Clipboard::without_system();
        let blue = |s: &WinState| s.view.doc.main.paras[0].chp_of_char(0).color;
        state.dialog_outcome(Outcome::Button("color"), &mut clip);
        // Pointing at a box shows its HTML code when it has no name.
        assert_eq!(pop_item(&state, "cus0").rgb, Some(0x336699));
        assert_eq!(pop_item(&state, "cus0").tip.as_deref(), Some("#336699"));
        assert_eq!(pop_item(&state, "std5").tip.as_deref(), Some("#FF0000"));
        // Delete is for the document's own colors: the text's is one.
        assert!(pop_item(&state, "cus0").selected);
        assert!(pop_item(&state, "delete_color").enabled);
        let pick = |state: &mut WinState, id| {
            state.dialog_outcome(
                Outcome::Swatch { id, double: false },
                &mut Clipboard::without_system(),
            )
        };
        pick(&mut state, "std5");
        assert!(!pop_item(&state, "delete_color").enabled);
        pick(&mut state, "cus0");
        assert!(pop_item(&state, "delete_color").enabled);

        state.dialog_outcome(Outcome::Button("delete_color"), &mut clip);
        assert_eq!(state.dialogs.len(), 2);
        assert!(matches!(
            state.dialogs[1].0,
            Which::Alert(format_dialogs::Alert::DeleteColor(Rgb(0x336699)))
        ));
        // Cancel changes nothing.
        state.dialog_outcome(Outcome::Cancel, &mut clip);
        assert_eq!(blue(&state), Rgb(0x336699));
        assert!(!state.view.doc.is_modified());

        state.dialog_outcome(Outcome::Button("color"), &mut clip);
        pick(&mut state, "cus0");
        state.dialog_outcome(Outcome::Button("delete_color"), &mut clip);
        state.dialog_outcome(Outcome::Ok, &mut clip);
        assert_eq!(state.dialogs.len(), 1);
        assert_eq!(blue(&state), Rgb::BLACK);
        assert_eq!(state.view.doc.undo_label(), Some("Delete Color"));
        // The palette is back, without the color; the dialog shows black.
        assert_eq!(pop_item(&state, "cus0").rgb, None);
        assert_eq!(state.dialogs[0].1.swatch_rgb("color"), Some(0));
        assert_eq!(state.dialogs[0].1.item_label("color_hex"), "#000000");
        state.view.doc.undo();
        assert_eq!(blue(&state), Rgb(0x336699));
    }

    #[test]
    fn deleting_a_color_can_choose_its_replacement() {
        let mut state = blue_words();
        let mut clip = Clipboard::without_system();
        state.dialog_outcome(Outcome::Button("color"), &mut clip);
        state.dialog_outcome(
            Outcome::Swatch {
                id: "cus0",
                double: false,
            },
            &mut clip,
        );
        state.dialog_outcome(Outcome::Button("delete_color"), &mut clip);
        state.dialog_outcome(Outcome::Button("choose"), &mut clip);
        assert!(matches!(state.dialogs[1].0, Which::EditColor(_)));
        state.dialogs[1].1.set_field("hex", "#AA2200");
        state.dialog_outcome(Outcome::Changed("hex"), &mut clip);
        state.dialog_outcome(Outcome::Ok, &mut clip);
        assert_eq!(state.dialogs.len(), 1);
        assert_eq!(
            state.view.doc.main.paras[0].chp_of_char(0).color,
            Rgb(0xAA2200)
        );
        assert_eq!(
            state.view.doc.main.paras[0].chp_of_char(6).color,
            Rgb::BLACK
        );
        assert_eq!(pop_item(&state, "cus0").rgb, Some(0xAA2200));
    }

    #[test]
    fn deleting_an_unused_or_saved_color() {
        let mut state = blue_words();
        let mut clip = Clipboard::without_system();
        crate::settings::update(|s| s.named_colors = vec![(0x336699, "Brand".into())]);
        state.dialog_outcome(Outcome::Button("color"), &mut clip);
        // A color saved in Preferences is for every document: no Delete.
        assert_eq!(pop_item(&state, "cus0").tip.as_deref(), Some("Brand"));
        state.dialog_outcome(
            Outcome::Swatch {
                id: "cus0",
                double: false,
            },
            &mut clip,
        );
        assert!(!pop_item(&state, "delete_color").enabled);
        crate::settings::update(|s| s.named_colors.clear());

        // A color made with Edit Color but not used yet goes at once.
        state.dialog_outcome(
            Outcome::Swatch {
                id: "std0",
                double: false,
            },
            &mut clip,
        );
        state.dialog_outcome(Outcome::Button("edit"), &mut clip);
        state.dialogs[1].1.set_field("hex", "#123456");
        state.dialog_outcome(Outcome::Changed("hex"), &mut clip);
        state.dialog_outcome(Outcome::Ok, &mut clip);
        state.dialog_outcome(Outcome::Button("color"), &mut clip);
        assert_eq!(pop_item(&state, "cus0").rgb, Some(0x123456));
        assert!(pop_item(&state, "cus0").selected);
        state.dialog_outcome(Outcome::Button("delete_color"), &mut clip);
        assert_eq!(state.dialogs.len(), 1);
        assert!(!state.view.doc.is_modified());
        assert_eq!(pop_item(&state, "cus0").rgb, Some(0x336699));
        crate::palette::drop_edits();
    }

    #[test]
    fn preferences_colors_are_chosen_from_the_palette() {
        let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
        let d = crate::prefs_dialog::preferences(crate::prefs_dialog::COLORS_PANEL, &[]);
        state.dialogs.push((
            Which::Preferences {
                panel: crate::prefs_dialog::COLORS_PANEL,
                families: Vec::new(),
                original: Box::new(crate::settings::get()),
                draft: Box::new(crate::settings::get()),
            },
            d,
        ));
        let mut clip = Clipboard::without_system();
        state.dialog_outcome(Outcome::Button("cbox1"), &mut clip);
        let pop = state.dialogs[0].1.popout.clone().unwrap();
        assert_eq!(pop.anchor, "cbox1");
        assert!(!pop.items.iter().any(|i| i.id == "delete_color"));
        state.dialog_outcome(
            Outcome::Swatch {
                id: "std5",
                double: false,
            },
            &mut clip,
        );
        assert_eq!(state.dialogs[0].1.item_label("color1"), "#FF0000");
        assert_eq!(state.dialogs[0].1.swatch_rgb("cbox1"), Some(0xFF0000));
        // Another box: the pop-out moves to it.
        state.dialog_outcome(Outcome::Button("cbox2"), &mut clip);
        assert_eq!(state.dialogs[0].1.popout.as_ref().unwrap().anchor, "cbox2");
        state.dialog_outcome(Outcome::Button("edit"), &mut clip);
        state.dialogs[1].1.set_field("hex", "#0A0B0C");
        state.dialog_outcome(Outcome::Changed("hex"), &mut clip);
        state.dialog_outcome(Outcome::Ok, &mut clip);
        assert_eq!(state.dialogs[0].1.item_label("color2"), "#0A0B0C");
        state.dialogs[0].1.set_field("cname1", "Red");
        state.dialogs[0].1.set_field("cname2", "Ink");
        state.dialog_outcome(Outcome::Button("cclear2"), &mut clip);
        assert_eq!(state.dialogs[0].1.item_label("color2"), "None");
        state.dialog_outcome(Outcome::Button("save"), &mut clip);
        let saved = crate::settings::with(|s| s.named_colors.clone());
        crate::settings::update(|s| s.named_colors.clear());
        assert_eq!(saved, [(0xFF0000, "Red".to_string())]);
    }

    #[test]
    fn nested_modal_keyboard_actions_close_one_layer_at_a_time() {
        let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
        let (paragraph, which) = format_dialogs::paragraph(&state.view);
        state.dialogs.push((which, paragraph));

        let mut clip = Clipboard::new();
        state.dialog_outcome(Outcome::Button("tabs"), &mut clip);
        assert_eq!(state.dialogs.len(), 2);
        assert!(matches!(
            state.dialogs.last(),
            Some((Which::Tabs { .. }, _))
        ));

        state.dialog_key(DKey::Escape, None, &mut clip);
        assert_eq!(state.dialogs.len(), 1);
        assert!(matches!(
            state.dialogs.last(),
            Some((Which::Paragraph(_), _))
        ));

        state.dialog_key(DKey::Escape, None, &mut clip);
        assert!(state.dialogs.is_empty());
    }

    #[test]
    fn split_panes_keep_independent_scroll_positions() {
        let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
        state.width = 800.0;
        state.height = 600.0;
        state.view.scroll_y = 40.0;
        state.toggle_split();

        let split = state.split.unwrap();
        assert!(!split.upper_active);
        assert_eq!(split.other_scroll, 40.0);

        state.view.scroll_y = 120.0;
        state.switch_pane();
        let split = state.split.unwrap();
        assert!(split.upper_active);
        assert_eq!(state.view.scroll_y, 40.0);
        assert_eq!(split.other_scroll, 120.0);

        state.view.scroll_y = 75.0;
        state.switch_pane();
        let split = state.split.unwrap();
        assert!(!split.upper_active);
        assert_eq!(state.view.scroll_y, 120.0);
        assert_eq!(split.other_scroll, 75.0);
    }

    #[test]
    fn joining_a_split_keeps_the_surviving_panes_scroll_position() {
        let mut state = WinState::new(DocView::new(Document::new(), "Doc".into()));
        state.width = 800.0;
        state.height = 600.0;
        state.view.scroll_y = 90.0;
        state.toggle_split();
        state.view.scroll_y = 180.0;
        state.switch_pane();
        state.view.scroll_y = 30.0;

        let bottom = state.height - BAR;
        state.finish_split_drag(bottom);
        assert!(state.split.is_none());
        assert_eq!(state.view.scroll_y, 30.0);

        state.toggle_split();
        state.view.scroll_y = 180.0;
        state.switch_pane();
        state.view.scroll_y = 30.0;
        let top = state.text_top();
        state.finish_split_drag(top);
        assert!(state.split.is_none());
        assert_eq!(state.view.scroll_y, 180.0);
    }

    #[test]
    fn a_shared_document_moves_between_windows() {
        let mut doc = Document::new();
        doc.main.paras[0].insert(0, "shared text", Default::default());
        let mut a = WinState::new(DocView::new(doc, "Doc".into()));
        let mut b = WinState::new(DocView::new(Document::new(), "Doc".into()));
        a.view.sel = Selection::caret(Position::new(0, 3));
        a.view.scroll_y = 40.0;
        let (mut ap, mut bp) = (
            Port::default(),
            Port {
                scroll_y: 0.0,
                scroll_x: 0.0,
                sel: Selection::caret(Position::new(0, 7)),
                upstream: false,
            },
        );
        hand_over(&mut a, &mut ap, &mut b, &mut bp);
        assert_eq!(b.view.doc.main.paras[0].text, "shared text");
        assert_eq!(b.view.sel.focus, Position::new(0, 7));
        assert_eq!(b.view.scroll_y, 0.0);
        b.view.insert_text("!");
        // And back: each window finds its own place again.
        hand_over(&mut b, &mut bp, &mut a, &mut ap);
        assert_eq!(a.view.doc.main.paras[0].text, "shared !text");
        assert_eq!(a.view.sel.focus, Position::new(0, 3));
        assert_eq!(a.view.scroll_y, 40.0);
        assert_eq!(bp.sel.focus, Position::new(0, 8));
    }
    #[test]
    fn remembered_window_frames_stay_on_a_connected_screen() {
        let laptop = [0.0, 0.0, 1512.0, 982.0];
        let monitor = [1512.0, -200.0, 2560.0, 1440.0];
        // On the laptop: as remembered.
        assert_eq!(
            restore_frame([100, 50, 900, 700], &[laptop], &[]),
            Some([100.0, 50.0, 900.0, 700.0])
        );
        // Left on the monitor, which is now unplugged: the default place.
        assert_eq!(restore_frame([2000, 100, 900, 700], &[laptop], &[]), None);
        assert!(restore_frame([2000, 100, 900, 700], &[laptop, monitor], &[]).is_some());
        // Too big for the screen now: shrunk to fit.
        assert_eq!(
            restore_frame([0, 0, 3000, 2000], &[laptop], &[]),
            Some([0.0, 0.0, 1512.0, 982.0])
        );
        // A window already there: the next one steps down and right.
        assert_eq!(
            restore_frame(
                [100, 50, 900, 700],
                &[laptop],
                &[(100.0, 50.0), (124.0, 74.0)]
            ),
            Some([148.0, 98.0, 900.0, 700.0])
        );
    }
    #[test]
    fn zooming_far_down_a_long_document_stays_on_the_same_page() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let paragraph = "A paragraph of ordinary text that runs on for a few lines. ".repeat(8);
        let doc = quill_core::text::from_plain_text(&format!("{paragraph}\n").repeat(1200));
        for page_view in [false, true] {
            let mut state = WinState::new(DocView::new(doc.clone(), "Zoom".into()));
            state.document_zoom = 1.0;
            state.ui = 1.0;
            state.view.page_view = page_view;
            paint_editor(&mut state, 900, 700, &mut fonts);
            state.view.scroll_y = (state.view.max_scroll() * 0.4).floor();
            paint_editor(&mut state, 900, 700, &mut fonts);
            let page = state.view.visible_page();
            assert!(page > 20, "the test document is long enough");
            // The caret is still on page 1; zooming must not go back to it.
            for _ in 0..6 {
                state.zoom_by(1.1, false);
                paint_editor(&mut state, 900, 700, &mut fonts);
                assert!(state.view.visible_page().abs_diff(page) <= 1);
            }
            state.zoom_step(1);
            paint_editor(&mut state, 900, 700, &mut fonts);
            assert!(state.view.visible_page().abs_diff(page) <= 1);
            for _ in 0..6 {
                state.zoom_by(1.0 / 1.1, false);
                paint_editor(&mut state, 900, 700, &mut fonts);
                assert!(state.view.visible_page().abs_diff(page) <= 1);
            }
        }
        crate::settings::update(|s| *s = crate::settings::Settings::default());
    }

    #[test]
    fn zooming_in_on_a_page_keeps_it_centered_sideways() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let doc = quill_core::text::from_plain_text(&"A paragraph of text.\n".repeat(200));
        let mut state = WinState::new(DocView::new(doc, "Zoom".into()));
        state.document_zoom = 1.0;
        state.ui = 1.0;
        state.view.page_view = true;
        paint_editor(&mut state, 900, 700, &mut fonts);
        assert_eq!(state.view.scroll_x, 0.0);
        state.zoom_by(3.0, false);
        paint_editor(&mut state, 900, 700, &mut fonts);
        // The page is wider than the window now: its middle is the
        // window's middle, not its left edge.
        let (pw, _) = state.view.page_size();
        let want = crate::docview::PAGE_GAP + pw / 2.0 - state.view.area.w / 2.0;
        assert!(state.view.scroll_x > 0.0);
        assert!(
            (state.view.scroll_x - want).abs() < 1.5,
            "{} vs {want}",
            state.view.scroll_x
        );
        state.zoom_by(1.0 / 3.0, false);
        paint_editor(&mut state, 900, 700, &mut fonts);
        assert_eq!(state.view.scroll_x, 0.0);
    }

    #[test]
    fn stationary_selection_drag_keeps_scrolling_until_release() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let doc = quill_core::text::from_plain_text(&"A paragraph for selection.\n".repeat(150));
        let mut state = WinState::new(DocView::new(doc, "Selection".into()));
        state.document_zoom = 1.0;
        state.ui = 1.0;
        paint_editor(&mut state, 720, 480, &mut fonts);
        let a = state.view.area;
        state.view.mouse_down(a.x + 40.0, a.y + 30.0, false, false);
        let anchor = state.view.sel.anchor;
        state.pointer = (a.x + 40.0, a.bottom() + 40.0);
        for _ in 0..5 {
            assert!(state.selection_scroll(true));
        }
        assert!(state.view.scroll_y >= 190.0);
        assert_eq!(state.view.sel.anchor, anchor);
        assert!(state.view.sel.focus > anchor);
        // Just past the edge, as at the bottom of the screen, it still moves
        // at a useful pace.
        let y = state.view.scroll_y;
        state.pointer = (a.x + 40.0, a.bottom() + 1.0);
        for _ in 0..5 {
            assert!(state.selection_scroll(true));
        }
        assert!(state.view.scroll_y >= y + 50.0);
        state.view.mouse_up();
        let y = state.view.scroll_y;
        assert!(!state.selection_scroll(true));
        assert_eq!(state.view.scroll_y, y);
    }

    #[test]
    fn horizontal_pan_keeps_page_hit_testing_and_shared_ports_aligned() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let mut state = WinState::new(DocView::new(
            quill_core::text::from_plain_text("Hello world"),
            "Pan".into(),
        ));
        state.document_zoom = 1.0;
        state.view.page_view = true;
        paint_editor(&mut state, 420, 480, &mut fonts);
        let a = state.view.area;
        let x = state.view.text_left_window() + 12.0;
        let y = a.y + crate::docview::PAGE_GAP + 72.0 + 4.0;
        let before = state.view.hit(x, y);
        state.view.scroll_sideways(100.0);
        assert_eq!(state.view.scroll_x, 100.0);
        assert_eq!(state.view.hit(x - 100.0, y), before);
        let mut port = Port::default();
        state.view.swap_port(&mut port);
        assert_eq!(port.scroll_x, 100.0);
        assert_eq!(state.view.scroll_x, 0.0);
        state.view.swap_port(&mut port);
        state.view.area.w = 1200.0;
        state.view.scroll_sideways(0.0);
        assert_eq!(state.view.scroll_x, 0.0);
    }
    #[test]
    fn deleting_a_comment_from_its_focused_editor_preserves_text_for_undo() {
        use quill_core::{StoryId, notes::AnnotationInfo};
        let mut doc = quill_core::text::from_plain_text("Body text");
        let id = doc.insert_annotation(
            Position::new(0, 4),
            Default::default(),
            AnnotationInfo::default(),
        );
        doc.forget_history(); // A loaded document, before editing the comment.
        let mut state = WinState::new(DocView::new(doc, "Comments".into()));
        state.open_note_in(StoryId::Annotations, id);
        state.view.type_text("Edited comment");
        state.delete_annotation();
        assert!(state.pane.is_none());
        assert_eq!(state.view.doc.main.paras[0].text, "Body text");
        assert!(state.view.doc.annotation_ranges().is_empty());
        state.view.doc.undo();
        assert!(state.view.doc.annotation_mark(id).is_some());
        assert!(
            state
                .view
                .doc
                .annotation_text(id)
                .contains("Edited comment")
        );
    }
    #[test]
    fn horizontal_scrollbar_click_drag_and_release_pan_the_document() {
        let Some(mut fonts) = crate::pdf::tests::test_fonts() else {
            return;
        };
        let mut state = WinState::new(DocView::new(Document::new(), "Horizontal".into()));
        state.document_zoom = 1.0;
        state.view.page_view = true;
        paint_editor(&mut state, 420, 480, &mut fonts);
        let bar = state.horizontal_bar();
        let env = Env {
            font_names: &[],
            font_families: &[],
            recent: &[],
            windows: &[],
        };
        let mut cache = GlyphCache::new();
        let mut clip = Clipboard::new();
        let mut label = Label {
            fonts: &mut fonts,
            cache: &mut cache,
        };
        state.mouse_down(
            bar.right() - 4.0,
            bar.y + 8.0,
            Mods::default(),
            &mut label,
            &env,
            &mut clip,
        );
        assert!(state.main_view().scroll_x > 0.0);
        state.mouse_up(bar.right() - 4.0, bar.y + 8.0, &mut clip);
        state.main_view_mut().scroll_x = 0.0;
        state.mouse_down(
            bar.x + BAR + 4.0,
            bar.y + 8.0,
            Mods::default(),
            &mut label,
            &env,
            &mut clip,
        );
        state.mouse_move(bar.right() - BAR - 8.0, bar.y + 8.0, &mut label, &env);
        assert!(state.main_view().scroll_x > 50.0);
        state.mouse_up(bar.right() - BAR - 8.0, bar.y + 8.0, &mut clip);
        assert!(state.horizontal_press.is_none());
    }
}
