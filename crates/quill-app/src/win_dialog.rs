//! Windows controls for the platform-neutral dialog model.
use crate::app::UserEvent;
use crate::dialog::{Dialog, Kind, Outcome};
use std::cell::RefCell;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
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
    lists: Vec<bool>,
    rows: Vec<Vec<String>>,
    capturing: bool,
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
            let ptr = GetWindowLongPtrW(panel, GWLP_USERDATA) as *const State;
            if !ptr.is_null()
                && (*ptr).capturing
                && matches!(message.message, WM_KEYDOWN | WM_SYSKEYDOWN)
            {
                let vk = message.wParam.0 as u32;
                if vk == 27 {
                    let _ = (*ptr).proxy.send_event(UserEvent::WindowsShortcut {
                        panel: panel.0 as usize,
                        spec: None,
                    });
                    return true;
                }
                if let Some(spec) = shortcut_spec(vk) {
                    let _ = (*ptr).proxy.send_event(UserEvent::WindowsShortcut {
                        panel: panel.0 as usize,
                        spec: Some(spec),
                    });
                    return true;
                }
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

pub fn supported(dialog: &Dialog) -> bool {
    !dialog.items.iter().any(|item| {
        // Lists are drawn by Quill until Windows has its own.
        matches!(
            item.kind,
            Kind::Swatch { .. } | Kind::Slider { .. } | Kind::List { .. }
        ) || (matches!(item.kind, Kind::Custom)
            && !matches!(item.id, "alert_icon" | "list" | "keys" | "description"))
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
    pub fn panel_id(&self) -> usize {
        self.hwnd.0 as usize
    }
    pub fn matches(&self, depth: usize, d: &Dialog) -> bool {
        self.depth == depth && self.signature == signature(d)
    }
    pub fn show(
        parent: &Window,
        depth: usize,
        d: &Dialog,
        proxy: EventLoopProxy<UserEvent>,
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
                lists: d
                    .items
                    .iter()
                    .map(|i| matches!(i.kind, Kind::Custom) && matches!(i.id, "list" | "keys"))
                    .collect(),
                rows: vec![Vec::new(); d.items.len()],
                capturing: false,
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
            let mut owner_rect = RECT::default();
            GetWindowRect(owner, &mut owner_rect).ok()?;
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
                Some(owner),
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
            };
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
                    Kind::Custom if matches!(item.id, "list" | "keys") => (
                        w!("LISTBOX"),
                        LBS_NOTIFY | WS_VSCROLL.0 as i32 | WS_BORDER.0 as i32,
                    ),
                    _ => (w!("STATIC"), 0),
                };
                let interactive = matches!(
                    item.kind,
                    Kind::Button { .. }
                        | Kind::Check { .. }
                        | Kind::Radio { .. }
                        | Kind::Field { .. }
                        | Kind::Popup { .. }
                        | Kind::Custom
                );
                let title = wide(&item.label);
                let r = item.rect;
                let height = if matches!(item.kind, Kind::Popup { .. }) {
                    r.h + 180.0
                } else {
                    r.h
                };
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
                    (r.w * scale) as i32,
                    (height * scale) as i32,
                    Some(hwnd),
                    Some(HMENU((index + 100) as *mut _)),
                    Some(instance.into()),
                    Some((&mut *result.state as *mut State).cast()),
                ) {
                    Ok(child) => child,
                    Err(_) => return None,
                };
                SendMessageW(
                    child,
                    WM_SETFONT,
                    Some(WPARAM(font.0 as usize)),
                    Some(LPARAM(1)),
                );
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
                result.controls.push(child);
            }
            // A group frame must sit beneath the controls it encloses, or it
            // paints over them. Once every control exists, push the frames to
            // the bottom. They take no tab stop, so the tab order is unchanged.
            for (item, control) in d.items.iter().zip(&result.controls) {
                if matches!(item.kind, Kind::Group) {
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
        self.state.capturing = d.item_label("name") == "Press the new key…";
        unsafe {
            for (item, hwnd) in d.items.iter().zip(&self.controls) {
                let _ = EnableWindow(*hwnd, item.enabled);
                match &item.kind {
                    Kind::Custom if matches!(item.id, "list" | "keys") => {
                        let rows = crate::commands_dialog::native_rows(d, item.id);
                        if self.state.rows[self.controls.iter().position(|h| h == hwnd).unwrap()]
                            != rows
                        {
                            SendMessageW(*hwnd, LB_RESETCONTENT, None, None);
                            for row in &rows {
                                let text = wide(row);
                                SendMessageW(
                                    *hwnd,
                                    LB_ADDSTRING,
                                    None,
                                    Some(LPARAM(text.as_ptr() as isize)),
                                );
                            }
                            let index = self.controls.iter().position(|h| h == hwnd).unwrap();
                            self.state.rows[index] = rows;
                        }
                        if let Some(row) = crate::commands_dialog::native_selection(d, item.id) {
                            SendMessageW(*hwnd, LB_SETCURSEL, Some(WPARAM(row)), None);
                        }
                    }
                    Kind::Custom if item.id == "description" => {
                        let text = wide(&crate::commands_dialog::native_description(d));
                        let _ = SetWindowTextW(*hwnd, PCWSTR(text.as_ptr()));
                    }
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
                    _ => {
                        let text = wide(&item.label);
                        let _ = SetWindowTextW(*hwnd, PCWSTR(text.as_ptr()));
                    }
                }
            }
        }
        self.state.syncing = false;
    }
    pub fn selected_row(&self, id: &str) -> Option<usize> {
        let i = self
            .state
            .ids
            .iter()
            .position(|candidate| *candidate == id)?;
        if !self.state.lists[i] {
            return None;
        }
        let row = unsafe { SendMessageW(self.controls[i], LB_GETCURSEL, None, None).0 };
        (row >= 0).then_some(row as usize)
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
            _ if d
                .items
                .iter()
                .any(|i| i.id == id && matches!(i.kind, Kind::Button { .. })) =>
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

fn shortcut_spec(vk: u32) -> Option<String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, MAPVK_VK_TO_CHAR, MapVirtualKeyW,
    };
    let name = match vk {
        8 => "⌫".into(),
        9 => "⇥".into(),
        13 => "↩".into(),
        32 => "Space".into(),
        33 => "Page Up".into(),
        34 => "Page Down".into(),
        35 => "End".into(),
        36 => "Home".into(),
        37 => "←".into(),
        38 => "↑".into(),
        39 => "→".into(),
        40 => "↓".into(),
        46 => "⌦".into(),
        112..=135 => format!("F{}", vk - 111),
        16..=18 | 91 | 92 => return None,
        _ => {
            let ch = char::from_u32(unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_CHAR) } & 0x7fff_ffff)?;
            if ch.is_control() {
                return None;
            }
            ch.to_uppercase().collect()
        }
    };
    let mut spec = String::new();
    for (vk, marker) in [(17, "⌘"), (16, "⇧"), (18, "⌥")] {
        if unsafe { GetKeyState(vk) } < 0 {
            spec.push_str(marker);
        }
    }
    spec.push_str(&name);
    Some(spec)
}
