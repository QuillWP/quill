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

#[cfg_attr(test, allow(unused_imports))]
pub(crate) use crate::alerts::{alert, ask, ask_include, message_box, warn};
mod commands;
mod dialogs;
mod effects;
mod files;
mod fonts;
mod input;
mod markdown;
mod menu_track;
mod palette_window;
mod panes;
#[cfg(target_os = "macos")]
mod platform;
#[cfg(target_os = "macos")]
mod preferences;
mod print;
#[cfg(target_os = "macos")]
mod printing;
mod revisions;
mod snapshot;
mod styles;
#[cfg(not(target_os = "macos"))]
mod system_preview;
mod tips;
mod window_ui;
pub use fonts::start_font_load;
use fonts::{FontFolders, FontLoad};
use menu_track::{MenuSource, MenuTrack};
#[cfg(target_os = "macos")]
pub(crate) use tips::TIP_DELAY;
use tips::Tip;
mod zoom;

pub use snapshot::snapshot;

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
    #[cfg(target_os = "linux")]
    let system = {
        let _ = window;
        crate::linux_window::system_dark()
    };
    #[cfg(not(target_os = "linux"))]
    let system = window.theme() == Some(winit::window::Theme::Dark);
    crate::settings::with(|s| s.appearance).is_dark(system)
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
    /// A search of the font sources finished.
    FontsFound,
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
    fonts_scan: Option<FontLoad>,
    /// A search of the font sources running after they changed, and the
    /// sources it is for. The old fonts stay in use until it is done.
    font_rescan: Option<(FontFolders, JoinHandle<Catalog>)>,
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

impl App {
    pub fn new(
        fonts_scan: FontLoad,
        files: Vec<PathBuf>,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Self {
        let loaded = fonts_scan.folders.clone();
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
            font_folders_loaded: loaded,
            font_rescan: None,
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
            let (system, catalog) = self
                .fonts_scan
                .take()
                .and_then(|load| load.found.join().ok())
                .unwrap_or_default();
            self.system_catalog = Some(system);
            self.use_catalog(catalog);
        }
    }

    /// Make `catalog` the fonts every document draws with.
    fn use_catalog(&mut self, catalog: Catalog) {
        crate::font_names::set(&catalog);
        crate::prefs_dialog::set_sources_cut_short(catalog.sources_cut_short());
        let fonts = Fonts::new(catalog.clone());
        self.font_names = fonts.family_names().iter().map(|s| s.to_string()).collect();
        self.font_families = fonts.family_info();
        self.font_catalog = Some(catalog);
        self.fonts = Some(fonts);
    }

    /// Preferences > Fonts switched a font source on or off, or changed
    /// how deep to search: search the sources again on a thread of its
    /// own, so a big source cannot stop the window, and when it is done
    /// rebuild the font list and lay every document out again with it.
    fn refresh_font_folders(&mut self) {
        if self
            .font_rescan
            .as_ref()
            .is_some_and(|(_, found)| found.is_finished())
        {
            let (folders, found) = self.font_rescan.take().unwrap();
            // Taken as done even if the search failed, so it is not
            // started again and again.
            self.font_folders_loaded = folders;
            if let Ok(catalog) = found.join() {
                self.use_new_fonts(catalog);
            }
        }
        if self.font_rescan.is_some() {
            return;
        }
        crate::settings::drop_missing_font_folders();
        let folders = FontFolders::current();
        if folders == self.font_folders_loaded {
            return;
        }
        // Before the fonts are first in use, this runs again once they are.
        let Some(system) = self.system_catalog.clone() else {
            return;
        };
        let wanted = folders.clone();
        let proxy = self.proxy.clone();
        let found = std::thread::spawn(move || {
            let catalog = wanted.apply(&system);
            // Wake the event loop to put the fonts in.
            let _ = proxy.send_event(UserEvent::FontsFound);
            catalog
        });
        self.font_rescan = Some((folders, found));
    }

    fn use_new_fonts(&mut self, catalog: Catalog) {
        self.use_catalog(catalog);
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

    /// Show the system's unsaved-changes dot in each window's close button
    /// while its document has changes that are not saved.
    #[cfg(target_os = "macos")]
    fn sync_edited_marks(&mut self) {
        use winit::platform::macos::WindowExtMacOS;
        for i in 0..self.windows.len() {
            let edited = self.windows[self.holder(i)]
                .1
                .st
                .main_view()
                .doc
                .is_modified();
            let window = &self.windows[i].1.window;
            if window.is_document_edited() != edited {
                window.set_document_edited(edited);
            }
        }
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
        // The debug build's ID is quill-debug, so it is a separate program.
        #[cfg(target_os = "linux")]
        let attrs = {
            use winit::platform::wayland::WindowAttributesExtWayland;
            attrs.with_name(crate::linux_window::PROGRAM, crate::linux_window::PROGRAM)
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
            let interrupted = crate::recovery::interrupted();
            if !interrupted.is_empty() {
                let answer = message_box(
                    rfd::MessageLevel::Warning,
                    "Quill",
                    format!(
                        "Quill did not finish recovering {} document{} last time. The recovery \
                         files may make Quill stop again.\n\nSkip them unless you are ready to try once more.",
                        interrupted.len(),
                        if interrupted.len() == 1 { "" } else { "s" }
                    ),
                    rfd::MessageButtons::YesNoCancelCustom(
                        "Skip Recovery".into(),
                        "Try Again".into(),
                        "Cancel".into(),
                    ),
                );
                if matches!(
                    answer,
                    rfd::MessageDialogResult::Custom(ref label) if label == "Try Again"
                ) {
                    crate::recovery::retry_interrupted(&interrupted);
                }
            }
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
            UserEvent::FontsFound => {}
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
        #[cfg(target_os = "macos")]
        self.sync_edited_marks();
        // New Markdown fonts from the Preferences reach the open windows.
        if let Some((old, new)) = crate::settings::take_markdown_font_change() {
            for (_, w) in &mut self.windows {
                w.st.main_view_mut().restyle_markdown_fonts(&old, &new);
                w.window.request_redraw();
            }
        }
        #[cfg(target_os = "linux")]
        crate::linux_window::poll();
        // winit sends no ThemeChanged on Linux, so watch the setting here.
        #[cfg(target_os = "linux")]
        if let Some(dark) = crate::linux_window::system_dark_changed() {
            let appearance = crate::settings::with(|s| s.appearance);
            for (_, w) in &mut self.windows {
                w.st.dark = appearance.is_dark(dark);
                w.window.request_redraw();
            }
            self.refresh_palette_window();
        }
        #[cfg(target_os = "linux")]
        let palette_wake = self.sync_palette_shown();
        #[cfg(not(target_os = "linux"))]
        let palette_wake: Option<Instant> = None;
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
        let next = palette_wake.map_or(next, |t| t.min(next));
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
mod tests;
