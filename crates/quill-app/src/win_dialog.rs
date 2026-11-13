//! Windows controls for the platform-neutral dialog model.
use crate::app::UserEvent;
use crate::dialog::{Dialog, Kind, Outcome};
use std::cell::RefCell;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, ODS_FOCUS};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};
use winit::event_loop::EventLoopProxy;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

thread_local! {
    static PANELS: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) };
}
struct State {
    proxy: EventLoopProxy<UserEvent>,
    ids: Vec<&'static str>,
    syncing: bool,
    default_button: Option<usize>,
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Let Windows implement dialog keyboard navigation before winit handles keys.
pub fn message_hook(msg: *const core::ffi::c_void) -> bool {
    PANELS.with(|panels| {
        let list = panels.borrow().clone();
        list.into_iter().rev().any(|panel| unsafe {
            let message = &*msg.cast::<MSG>();
            if message.hwnd != panel && !IsChild(panel, message.hwnd).as_bool() {
                return false;
            }
            IsDialogMessageW(panel, msg.cast()).as_bool()
        })
    })
}
unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_NCCREATE {
            let create = &*(lp.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        if msg == WM_DRAWITEM {
            return draw_box(lp);
        }
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        if !ptr.is_null() {
            let state = &*ptr;
            if msg == DM_GETDEFID
                && let Some(index) = state.default_button
            {
                return LRESULT(((DC_HASDEFID as usize) << 16 | (index + 100)) as isize);
            }
            let id = if msg == WM_CLOSE {
                Some("cancel")
            } else if msg == WM_HSCROLL && !state.syncing {
                // A slider moved: its control id is its index plus 100.
                let number = GetDlgCtrlID(HWND(lp.0 as *mut _)) as usize;
                number
                    .checked_sub(100)
                    .and_then(|i| state.ids.get(i).copied())
            } else if msg == WM_COMMAND && !state.syncing {
                let number = wp.0 & 0xffff;
                let code = (wp.0 >> 16) as u32;
                if number == 2 {
                    Some("cancel")
                } else if number >= 100 && matches!(code, BN_CLICKED | CBN_SELCHANGE | EN_CHANGE) {
                    state.ids.get(number - 100).copied()
                } else if number == 1 {
                    state.ids.iter().copied().find(|id| *id == "ok")
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(id) = id {
                let _ = state.proxy.send_event(UserEvent::WindowsDialog {
                    panel: hwnd.0 as usize,
                    id,
                });
                return LRESULT(0);
            }
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}

// Trackbar messages and style, which the windows crate keeps elsewhere.
const TBM_GETPOS: u32 = WM_USER;
const TBM_SETPOS: u32 = WM_USER + 5;
const TBM_SETRANGE: u32 = WM_USER + 6;
const TBS_NOTICKS: i32 = 0x10;
const SS_OWNERDRAW: i32 = 0x0D;

/// A color box as a number kept in a button's user data: the color in the
/// low 24 bits, then the marks.
const MARK_SELECTED: u32 = 1 << 24;
const MARK_EMPTY: u32 = 1 << 25;

fn box_bits(rgb: u32, selected: bool, empty: bool) -> isize {
    ((rgb & 0xFF_FFFF)
        | if selected { MARK_SELECTED } else { 0 }
        | if empty { MARK_EMPTY } else { 0 }) as isize
}

fn colorref(rgb: u32) -> COLORREF {
    COLORREF(((rgb & 0xFF) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 0xFF))
}

/// Paint an owner-drawn color box (a swatch button, or a box of the palette).
unsafe fn draw_box(lp: LPARAM) -> LRESULT {
    unsafe {
        let item = &*(lp.0 as *const DRAWITEMSTRUCT);
        let bits = GetWindowLongPtrW(item.hwndItem, GWLP_USERDATA) as u32;
        let mut rect = item.rcItem;
        let fill = CreateSolidBrush(colorref(bits));
        FillRect(item.hDC, &rect, fill);
        let _ = DeleteObject(fill.into());
        let edge = CreateSolidBrush(colorref(if bits & MARK_EMPTY != 0 {
            0xC8C8C8
        } else {
            0x808080
        }));
        FrameRect(item.hDC, &rect, edge);
        let _ = DeleteObject(edge.into());
        if bits & MARK_SELECTED != 0 {
            // The chosen box: a dark frame inside a light one.
            for shade in [0x000000, 0xFFFFFF] {
                rect.left += 1;
                rect.top += 1;
                rect.right -= 1;
                rect.bottom -= 1;
                let brush = CreateSolidBrush(colorref(shade));
                FrameRect(item.hDC, &rect, brush);
                let _ = DeleteObject(brush.into());
            }
        }
        if item.itemState.0 & ODS_FOCUS.0 != 0 {
            rect.left += 2;
            rect.top += 2;
            rect.right -= 2;
            rect.bottom -= 2;
            let _ = DrawFocusRect(item.hDC, &rect);
        }
        LRESULT(1)
    }
}

/// What the palette's pop-up window needs to tell the app.
struct PopupState {
    proxy: EventLoopProxy<UserEvent>,
    panel: usize,
    ids: Vec<&'static str>,
    closing: std::cell::Cell<bool>,
}

unsafe extern "system" fn popup_procedure(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_NCCREATE {
            let create = &*(lp.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const PopupState;
        if !ptr.is_null() {
            let state = &*ptr;
            match msg {
                WM_DRAWITEM => return draw_box(lp),
                WM_COMMAND if (wp.0 >> 16) as u32 == BN_CLICKED => {
                    let number = wp.0 & 0xffff;
                    if number >= 100
                        && let Some(id) = state.ids.get(number - 100)
                    {
                        let _ = state.proxy.send_event(UserEvent::WindowsDialog {
                            panel: state.panel,
                            id,
                        });
                    }
                    return LRESULT(0);
                }
                // A press outside puts the palette away, as on the Mac.
                WM_ACTIVATE if (wp.0 & 0xffff) as u32 == WA_INACTIVE && !state.closing.get() => {
                    let _ = state
                        .proxy
                        .send_event(UserEvent::WindowsPopoutClosed { panel: state.panel });
                }
                _ => {}
            }
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}

/// The color palette's window, opened below the box that asked for it.
struct Popup {
    hwnd: HWND,
    state: Box<PopupState>,
    spec: crate::dialog::Popout,
}

impl Drop for Popup {
    fn drop(&mut self) {
        self.state.closing.set(true);
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

pub fn supported(dialog: &Dialog) -> bool {
    !dialog.items.iter().any(|item| {
        // Lists are drawn by Quill until Windows has its own.
        matches!(item.kind, Kind::List { .. })
            || (matches!(item.kind, Kind::Custom) && item.id != "alert_icon")
    })
}

pub struct NativeDialog {
    hwnd: HWND,
    owner: HWND,
    state: Box<State>,
    controls: Vec<HWND>,
    depth: usize,
    signature: Vec<(String, std::mem::Discriminant<Kind>, Vec<String>)>,
    modal: bool,
    font: HFONT,
    scale: f32,
    dpi: u32,
    instance: HINSTANCE,
    popup: Option<Popup>,
}
fn signature(d: &Dialog) -> Vec<(String, std::mem::Discriminant<Kind>, Vec<String>)> {
    d.items
        .iter()
        .map(|i| {
            (
                i.id.to_string(),
                std::mem::discriminant(&i.kind),
                match &i.kind {
                    Kind::Popup { options, .. } => options.clone(),
                    _ => Vec::new(),
                },
            )
        })
        .collect()
}
impl NativeDialog {
    /// Create the dialog's controls inside its window.
    unsafe fn build_controls(&mut self, d: &Dialog, instance: HINSTANCE, scale: f32) -> Option<()> {
        unsafe {
            // Sliders are trackbars, a class that must be set up first.
            let _ = windows::Win32::UI::Controls::InitCommonControlsEx(
                &windows::Win32::UI::Controls::INITCOMMONCONTROLSEX {
                    dwSize: std::mem::size_of::<windows::Win32::UI::Controls::INITCOMMONCONTROLSEX>(
                    ) as u32,
                    dwICC: windows::Win32::UI::Controls::ICC_BAR_CLASSES,
                },
            );
            for (index, item) in d.items.iter().enumerate() {
                let (class, extra) = match item.kind {
                    Kind::Button { default } => (
                        w!("BUTTON"),
                        if default {
                            BS_DEFPUSHBUTTON
                        } else {
                            BS_PUSHBUTTON
                        },
                    ),
                    Kind::Check { tri, .. } => (
                        w!("BUTTON"),
                        if tri { BS_AUTO3STATE } else { BS_AUTOCHECKBOX },
                    ),
                    Kind::Radio { .. } => (w!("BUTTON"), BS_RADIOBUTTON),
                    Kind::Field { .. } => (w!("EDIT"), ES_AUTOHSCROLL | WS_BORDER.0 as i32),
                    Kind::Popup { .. } => (w!("COMBOBOX"), CBS_DROPDOWNLIST | WS_VSCROLL.0 as i32),
                    Kind::Group => (w!("BUTTON"), BS_GROUPBOX),
                    Kind::Swatch { button: true, .. } => (w!("BUTTON"), BS_OWNERDRAW),
                    // A color box that is only shown, drawn like the buttons.
                    Kind::Swatch { .. } => (w!("STATIC"), SS_OWNERDRAW),
                    Kind::Slider { .. } => (w!("msctls_trackbar32"), TBS_NOTICKS),
                    _ => (w!("STATIC"), 0),
                };
                let interactive = matches!(
                    item.kind,
                    Kind::Button { .. }
                        | Kind::Check { .. }
                        | Kind::Radio { .. }
                        | Kind::Field { .. }
                        | Kind::Popup { .. }
                        | Kind::Swatch { button: true, .. }
                        | Kind::Slider { .. }
                        | Kind::Custom
                );
                let title = wide(&item.label);
                let r = item.rect;
                let height = if matches!(item.kind, Kind::Popup { .. }) {
                    r.h + 180.0
                } else {
                    r.h
                };
                // A label's box is a fixed width from the layout, which the
                // Windows font can outgrow, so the text wraps and is cut off.
                // Make it at least as wide as its text.
                let mut width = r.w * scale;
                if matches!(item.kind, Kind::Text | Kind::Note) {
                    let dc = GetDC(Some(self.hwnd));
                    let old = SelectObject(dc, self.font.into());
                    let mut size = SIZE::default();
                    let _ = GetTextExtentPoint32W(dc, &title[..title.len() - 1], &mut size);
                    SelectObject(dc, old);
                    ReleaseDC(Some(self.hwnd), dc);
                    width = width.max(size.cx as f32 + 4.0);
                }
                let child = match CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    class,
                    PCWSTR(title.as_ptr()),
                    // Without clipping, overlapping siblings (a group box
                    // and the fields inside it) paint over one another.
                    WS_CHILD
                        | WS_VISIBLE
                        | WS_CLIPSIBLINGS
                        | WINDOW_STYLE(extra as u32)
                        | if interactive {
                            WS_TABSTOP
                        } else {
                            WINDOW_STYLE(0)
                        },
                    (r.x * scale) as i32,
                    (r.y * scale) as i32,
                    width as i32,
                    (height * scale) as i32,
                    Some(self.hwnd),
                    Some(HMENU((index + 100) as *mut _)),
                    Some(instance),
                    Some((&mut *self.state as *mut State).cast()),
                ) {
                    Ok(child) => child,
                    Err(_) => return None,
                };
                SendMessageW(
                    child,
                    WM_SETFONT,
                    Some(WPARAM(self.font.0 as usize)),
                    Some(LPARAM(1)),
                );
                if let Kind::Slider { max, .. } = &item.kind {
                    // The range is 0 to max, as the model's slider.
                    SendMessageW(
                        child,
                        TBM_SETRANGE,
                        Some(WPARAM(1)),
                        Some(LPARAM((*max as isize) << 16)),
                    );
                }
                if let Kind::Popup { options, .. } = &item.kind {
                    for option in options {
                        let text = wide(option);
                        SendMessageW(
                            child,
                            CB_ADDSTRING,
                            None,
                            Some(LPARAM(text.as_ptr() as isize)),
                        );
                    }
                }
                self.controls.push(child);
            }
            // A label's box is as wide as its layout slot, not its text, and a
            // label made after a field sits above it and paints the panel
            // color over part of it. Push labels to the bottom, then the group
            // frames below them. Neither takes a tab stop, so the tab order is
            // unchanged.
            for kind_is_group in [false, true] {
                for (item, control) in d.items.iter().zip(&self.controls) {
                    let label = matches!(item.kind, Kind::Text | Kind::Note | Kind::Group);
                    if !label || matches!(item.kind, Kind::Group) != kind_is_group {
                        continue;
                    }
                    let _ = SetWindowPos(
                        *control,
                        Some(HWND_BOTTOM),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                }
            }
        }
        self.signature = signature(d);
        Some(())
    }

    /// Show another page of the same dialog in this window: new controls
    /// and size, same window. Rebuilding the window made it vanish and
    /// reappear on every page change.
    pub fn rebuild(&mut self, depth: usize, d: &Dialog) -> bool {
        // A page with controls this panel cannot make (the Colors swatches)
        // is drawn by Quill instead, as when the dialog first opens.
        if !supported(d) {
            return false;
        }
        self.popup = None;
        unsafe {
            for control in self.controls.drain(..) {
                let _ = DestroyWindow(control);
            }
            self.state.ids = d.items.iter().map(|i| i.id).collect();
            self.state.default_button = d
                .items
                .iter()
                .position(|i| matches!(i.kind, Kind::Button { default: true }));
            let mut rect = RECT {
                right: (d.width * self.scale) as i32,
                bottom: (d.height * self.scale) as i32,
                ..Default::default()
            };
            if windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi(
                &mut rect,
                WS_CAPTION | WS_SYSMENU | WS_POPUP,
                false,
                WS_EX_DLGMODALFRAME,
                self.dpi,
            )
            .is_err()
            {
                return false;
            }
            let _ = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            self.state.syncing = true;
            let (instance, scale) = (self.instance, self.scale);
            if self.build_controls(d, instance, scale).is_none() {
                return false;
            }
            self.depth = depth;
            self.sync_from_model(d);
            if let Some(index) = d
                .items
                .iter()
                .position(|i| Some(i.id) == d.focused_id())
                .or_else(|| {
                    d.items
                        .iter()
                        .position(|i| matches!(i.kind, Kind::Field { .. }))
                })
            {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(
                    self.controls[index],
                ));
            }
            true
        }
    }
}
impl NativeDialog {
    pub fn panel_id(&self) -> usize {
        self.hwnd.0 as usize
    }
    pub fn matches(&self, depth: usize, d: &Dialog) -> bool {
        self.depth == depth && self.signature == signature(d)
    }
    /// Whether this window can show `d` by changing its controls: the same
    /// dialog level, as when a Preferences page is chosen.
    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
    pub fn depth(&self) -> usize {
        self.depth
    }
    /// Leave this window showing but out of use, while another is over it.
    pub fn hold(&self) {
        unsafe {
            let _ = EnableWindow(self.hwnd, false);
        }
    }
    /// Make this window the one in use again, with the owner still locked.
    pub fn release(&self) {
        unsafe {
            if self.modal {
                let _ = EnableWindow(self.owner, false);
            }
            if !IsWindowEnabled(self.hwnd).as_bool() {
                let _ = EnableWindow(self.hwnd, true);
                let _ = SetForegroundWindow(self.hwnd);
            }
        }
    }
    pub fn same_level(&self, depth: usize) -> bool {
        self.depth == depth
    }
    pub fn show(
        parent: &Window,
        depth: usize,
        d: &Dialog,
        proxy: EventLoopProxy<UserEvent>,
        over: Option<HWND>,
    ) -> Option<Self> {
        if !supported(d) {
            return None;
        }
        let RawWindowHandle::Win32(handle) = parent.window_handle().ok()?.as_raw() else {
            return None;
        };
        let owner = HWND(handle.hwnd.get() as *mut _);
        unsafe {
            let instance = GetModuleHandleW(None).ok()?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(procedure),
                hInstance: instance.into(),
                lpszClassName: w!("QuillNativeDialog"),
                hCursor: LoadCursorW(None, IDC_ARROW).ok()?,
                hbrBackground: HBRUSH((COLOR_BTNFACE.0 + 1) as *mut _),
                ..Default::default()
            };
            RegisterClassW(&class);
            let mut state = Box::new(State {
                proxy,
                ids: d.items.iter().map(|i| i.id).collect(),
                syncing: true,
                default_button: d
                    .items
                    .iter()
                    .position(|i| matches!(i.kind, Kind::Button { default: true })),
            });
            let scale = parent.scale_factor() as f32 * 1.25;
            let style = WS_CAPTION | WS_SYSMENU | WS_POPUP;
            let mut rect = RECT {
                right: (d.width * scale) as i32,
                bottom: (d.height * scale) as i32,
                ..Default::default()
            };
            windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi(
                &mut rect,
                style,
                false,
                WS_EX_DLGMODALFRAME,
                (parent.scale_factor() * 96.0).round() as u32,
            )
            .ok()?;
            // Over another dialog, this one is owned by it, so Windows keeps
            // it above that dialog and the two come forward together.
            let parent_window = over.unwrap_or(owner);
            let mut owner_rect = RECT::default();
            GetWindowRect(parent_window, &mut owner_rect).ok()?;
            let title = wide(d.native_title.as_deref().unwrap_or(&d.title));
            let hwnd = CreateWindowExW(
                WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT,
                w!("QuillNativeDialog"),
                PCWSTR(title.as_ptr()),
                style,
                owner_rect.left + (owner_rect.right - owner_rect.left - rect.right + rect.left) / 2,
                owner_rect.top + (owner_rect.bottom - owner_rect.top - rect.bottom + rect.top) / 2,
                rect.right - rect.left,
                rect.bottom - rect.top,
                Some(parent_window),
                None,
                Some(instance.into()),
                Some((&mut *state as *mut State).cast()),
            )
            .ok()?;
            let mut metrics = NONCLIENTMETRICSW {
                cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
                ..Default::default()
            };
            let font = if windows::Win32::UI::HiDpi::SystemParametersInfoForDpi(
                SPI_GETNONCLIENTMETRICS.0,
                metrics.cbSize,
                Some((&mut metrics as *mut NONCLIENTMETRICSW).cast()),
                0,
                (parent.scale_factor() * 96.0).round() as u32,
            )
            .is_ok()
            {
                CreateFontIndirectW(&metrics.lfMessageFont)
            } else {
                HFONT::default()
            };
            let mut result = Self {
                hwnd,
                owner,
                state,
                controls: Vec::new(),
                depth,
                signature: signature(d),
                modal: !d.modeless,
                font,
                scale,
                dpi: (parent.scale_factor() * 96.0).round() as u32,
                instance: instance.into(),
                popup: None,
            };
            result.build_controls(d, instance.into(), scale)?;
            result.sync_from_model(d);
            if result.modal {
                let _ = EnableWindow(owner, false);
            }
            PANELS.with(|panels| panels.borrow_mut().push(hwnd));
            let _ = ShowWindow(hwnd, SW_SHOW);
            if let Some(index) = d
                .items
                .iter()
                .position(|i| Some(i.id) == d.focused_id())
                .or_else(|| {
                    d.items
                        .iter()
                        .position(|i| matches!(i.kind, Kind::Field { .. }))
                })
            {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(
                    result.controls[index],
                ));
            }
            Some(result)
        }
    }
    pub fn sync_from_model(&mut self, d: &Dialog) {
        self.state.syncing = true;
        unsafe {
            for (item, hwnd) in d.items.iter().zip(&self.controls) {
                let _ = EnableWindow(*hwnd, item.enabled);
                match &item.kind {
                    Kind::Custom if item.id == "alert_icon" => {
                        let text = wide("ⓘ");
                        let _ = SetWindowTextW(*hwnd, PCWSTR(text.as_ptr()));
                    }
                    Kind::Field { text, .. } => {
                        if read_text(*hwnd) != *text {
                            let text = wide(text);
                            let _ = SetWindowTextW(*hwnd, PCWSTR(text.as_ptr()));
                        }
                    }
                    Kind::Check { on, mixed, .. } => {
                        SendMessageW(
                            *hwnd,
                            BM_SETCHECK,
                            Some(WPARAM(if *mixed { 2 } else { usize::from(*on) })),
                            None,
                        );
                    }
                    Kind::Radio { on, .. } => {
                        SendMessageW(*hwnd, BM_SETCHECK, Some(WPARAM(usize::from(*on))), None);
                    }
                    Kind::Popup { selected, .. } => {
                        SendMessageW(*hwnd, CB_SETCURSEL, Some(WPARAM(*selected)), None);
                    }
                    Kind::Slider { value, .. } => {
                        SendMessageW(
                            *hwnd,
                            TBM_SETPOS,
                            Some(WPARAM(1)),
                            Some(LPARAM(*value as isize)),
                        );
                    }
                    Kind::Swatch { rgb, .. } => {
                        SetWindowLongPtrW(*hwnd, GWLP_USERDATA, box_bits(*rgb, false, false));
                        let _ = InvalidateRect(Some(*hwnd), None, false);
                    }
                    _ => {
                        let text = wide(&item.label);
                        let _ = SetWindowTextW(*hwnd, PCWSTR(text.as_ptr()));
                    }
                }
            }
        }
        self.state.syncing = false;
        self.sync_popup(d);
    }

    /// Open, change or close the palette window to match the model.
    fn sync_popup(&mut self, d: &Dialog) {
        let Some(spec) = &d.popout else {
            self.popup = None;
            return;
        };
        if self.popup.as_ref().is_some_and(|p| p.spec == *spec) {
            return;
        }
        let Some(anchor) = d
            .items
            .iter()
            .position(|i| i.id == spec.anchor)
            .and_then(|i| self.controls.get(i).copied())
        else {
            return;
        };
        unsafe {
            let (width, height) = (
                (spec.size.0 * self.scale) as i32 + 2,
                (spec.size.1 * self.scale) as i32 + 2,
            );
            if self.popup.is_none() {
                let class = WNDCLASSW {
                    lpfnWndProc: Some(popup_procedure),
                    hInstance: self.instance,
                    lpszClassName: w!("QuillPalettePopup"),
                    hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                    hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as *mut _),
                    ..Default::default()
                };
                RegisterClassW(&class);
                let mut below = RECT::default();
                let _ = GetWindowRect(anchor, &mut below);
                let mut state = Box::new(PopupState {
                    proxy: self.state.proxy.clone(),
                    panel: self.hwnd.0 as usize,
                    ids: Vec::new(),
                    closing: std::cell::Cell::new(false),
                });
                let Ok(hwnd) = CreateWindowExW(
                    WS_EX_TOOLWINDOW,
                    w!("QuillPalettePopup"),
                    w!(""),
                    WS_POPUP | WS_BORDER,
                    below.left,
                    below.bottom,
                    width,
                    height,
                    Some(self.hwnd),
                    None,
                    Some(self.instance),
                    Some((&mut *state as *mut PopupState).cast()),
                ) else {
                    return;
                };
                self.popup = Some(Popup {
                    hwnd,
                    state,
                    spec: crate::dialog::Popout {
                        anchor: "",
                        size: (0.0, 0.0),
                        items: Vec::new(),
                    },
                });
            }
            let Some(popup) = &mut self.popup else {
                return;
            };
            // Swap what is in it, as a box is chosen.
            while let Ok(old) = GetWindow(popup.hwnd, GW_CHILD) {
                if old.0.is_null() {
                    break;
                }
                let _ = DestroyWindow(old);
            }
            popup.state.ids = spec.items.iter().map(|i| i.id).collect();
            for (index, item) in spec.items.iter().enumerate() {
                let boxed = item.label.is_empty();
                let title = wide(item.label);
                let mut style = WS_CHILD | WS_VISIBLE | WS_TABSTOP;
                if boxed {
                    style |= WINDOW_STYLE(BS_OWNERDRAW as u32);
                } else if !item.enabled {
                    style |= WS_DISABLED;
                }
                let Ok(child) = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("BUTTON"),
                    PCWSTR(title.as_ptr()),
                    style,
                    (item.rect.x * self.scale) as i32,
                    (item.rect.y * self.scale) as i32,
                    (item.rect.w * self.scale) as i32,
                    (item.rect.h * self.scale) as i32,
                    Some(popup.hwnd),
                    Some(HMENU((index + 100) as *mut _)),
                    Some(self.instance),
                    None,
                ) else {
                    continue;
                };
                if boxed {
                    SetWindowLongPtrW(
                        child,
                        GWLP_USERDATA,
                        box_bits(
                            item.rgb.unwrap_or(0xF4F4F4),
                            item.selected,
                            item.rgb.is_none(),
                        ),
                    );
                } else {
                    SendMessageW(
                        child,
                        WM_SETFONT,
                        Some(WPARAM(self.font.0 as usize)),
                        Some(LPARAM(1)),
                    );
                }
            }
            popup.spec = spec.clone();
            let _ = SetWindowPos(
                popup.hwnd,
                None,
                0,
                0,
                width,
                height,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            let _ = ShowWindow(popup.hwnd, SW_SHOW);
        }
    }

    pub fn apply_to_model(&self, d: &mut Dialog, id: &'static str) {
        unsafe {
            for (item, hwnd) in d.items.iter_mut().zip(&self.controls) {
                match &mut item.kind {
                    Kind::Field { text, sel } => {
                        *text = read_text(*hwnd);
                        *sel = (text.len(), text.len());
                    }
                    Kind::Check { on, mixed, .. } => {
                        let value = SendMessageW(*hwnd, BM_GETCHECK, None, None).0;
                        *on = value == 1;
                        *mixed = value == 2;
                    }
                    Kind::Slider { value, .. } => {
                        *value = SendMessageW(*hwnd, TBM_GETPOS, None, None).0 as i32;
                    }
                    Kind::Popup { selected, .. } => {
                        let value = SendMessageW(*hwnd, CB_GETCURSEL, None, None).0;
                        if value >= 0 {
                            *selected = value as usize;
                        }
                    }
                    _ => {}
                }
            }
        }
        if let Some(group) = d.items.iter().find_map(|i| match i.kind {
            Kind::Radio { group, .. } if i.id == id => Some(group),
            _ => None,
        }) {
            for item in &mut d.items {
                if let Kind::Radio { group: g, on } = &mut item.kind
                    && *g == group
                {
                    *on = item.id == id;
                }
            }
        }
    }
    pub fn outcome(d: &Dialog, id: &'static str) -> Outcome {
        match id {
            "ok" => Outcome::Ok,
            "cancel" => Outcome::Cancel,
            "apply" => Outcome::Apply,
            _ if d.items.iter().any(|i| {
                i.id == id
                    && matches!(
                        i.kind,
                        Kind::Button { .. } | Kind::Swatch { button: true, .. }
                    )
            }) =>
            {
                Outcome::Button(id)
            }
            _ => Outcome::Changed(id),
        }
    }
}
unsafe fn read_text(hwnd: HWND) -> String {
    unsafe {
        let mut text = vec![0u16; GetWindowTextLengthW(hwnd).max(0) as usize + 1];
        let n = GetWindowTextW(hwnd, &mut text);
        String::from_utf16_lossy(&text[..n.max(0) as usize])
    }
}
impl Drop for NativeDialog {
    fn drop(&mut self) {
        PANELS.with(|panels| panels.borrow_mut().retain(|p| *p != self.hwnd));
        unsafe {
            if self.modal {
                let _ = EnableWindow(self.owner, true);
            }
            let _ = DestroyWindow(self.hwnd);
            if !self.font.is_invalid() {
                let _ = DeleteObject(self.font.into());
            }
            let _ = SetForegroundWindow(self.owner);
        }
    }
}

/// Respect the user's Windows menu-font setting instead of a bundled face.
pub fn menu_font() -> &'static str {
    static FONT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    FONT.get_or_init(|| unsafe {
        let mut metrics = NONCLIENTMETRICSW {
            cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
            ..Default::default()
        };
        if SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            metrics.cbSize,
            Some((&mut metrics as *mut NONCLIENTMETRICSW).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
        {
            let face = metrics.lfMenuFont.lfFaceName;
            let n = face.iter().position(|c| *c == 0).unwrap_or(face.len());
            let name = String::from_utf16_lossy(&face[..n]);
            if !name.is_empty() {
                return name;
            }
        }
        "Segoe UI".into()
    })
}
