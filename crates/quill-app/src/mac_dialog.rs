//! AppKit presentation for Quill's platform-neutral dialog model.
//!
//! The document engine still owns dialog values and actions. This module
//! turns ordinary dialog items into native macOS controls and relays their
//! actions back through winit. Dialogs containing custom-drawn items keep
//! using Quill's canvas until they receive a native replacement of their own.

use crate::app::UserEvent;
use crate::dialog::{Dialog, Kind, Outcome};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{class, msg_send, sel};
use objc2_06::rc::Retained as Retained06;
use objc2_app_kit_03::{NSFont, NSFontAttributeName, NSFontManager, NSFontTraitMask};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use objc2_foundation_03::{
    MainThreadMarker as MainThreadMarker03, NSAttributedString, NSDictionary,
    NSString as NSString03,
};
use std::ffi::{CStr, CString, c_char};
use std::sync::{Mutex, OnceLock};
use winit::event_loop::EventLoopProxy;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

const SCALE: f64 = 1.25;

static PROXY: OnceLock<Mutex<EventLoopProxy<UserEvent>>> = OnceLock::new();
static ACTIONS: OnceLock<Mutex<Vec<ActionTarget>>> = OnceLock::new();
static TABLES: OnceLock<Mutex<Vec<TableData>>> = OnceLock::new();
static CAPTURING: OnceLock<Mutex<Vec<usize>>> = OnceLock::new();

#[derive(Debug)]
struct TableData {
    table: usize,
    rows: Vec<String>,
}

#[derive(Clone, Copy)]
struct ActionTarget {
    control: usize,
    panel: usize,
    id: &'static str,
    choice: Option<(usize, bool, bool)>,
    default: bool,
}

fn actions() -> &'static Mutex<Vec<ActionTarget>> {
    ACTIONS.get_or_init(|| Mutex::new(Vec::new()))
}

fn tables() -> &'static Mutex<Vec<TableData>> {
    TABLES.get_or_init(|| Mutex::new(Vec::new()))
}

fn capturing() -> &'static Mutex<Vec<usize>> {
    CAPTURING.get_or_init(|| Mutex::new(Vec::new()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DialogKeyAction {
    Default,
    Cancel,
}

fn dialog_key_action(chars: &str, flags: usize) -> Option<DialogKeyAction> {
    match chars.chars().next()? {
        // Return, and Enter on the keypad (or Fn-Return), as in any Mac dialog.
        '\r' | '\n' | '\u{3}' => Some(DialogKeyAction::Default),
        '\u{1b}' => Some(DialogKeyAction::Cancel),
        '.' if flags & (1usize << 20) != 0 => Some(DialogKeyAction::Cancel),
        _ => None,
    }
}

fn activate_dialog_key(panel: *mut AnyObject, action: DialogKeyAction) -> bool {
    let target = actions().lock().ok().and_then(|targets| {
        targets.iter().find_map(|target| {
            if target.panel != panel as usize {
                return None;
            }
            match action {
                DialogKeyAction::Cancel => (target.id == "cancel").then_some(target.control),
                DialogKeyAction::Default => target.default.then_some(target.control),
            }
        })
    });
    let Some(target) = target else {
        return false;
    };
    unsafe {
        let _: () =
            msg_send![target as *mut AnyObject, performClick: std::ptr::null::<AnyObject>()];
    }
    true
}

extern "C" fn dialog_send_event(this: *mut AnyObject, _cmd: Sel, event: *mut AnyObject) {
    let event_type: usize = unsafe { msg_send![event, type] };
    if event_type == 10 {
        // NSWindow sees key-down events here before a field editor can
        // consume Return or Escape as text-navigation keys.
        let chars: *mut AnyObject = unsafe { msg_send![event, charactersIgnoringModifiers] };
        let flags: usize = unsafe { msg_send![event, modifierFlags] };
        if let Some(action) = dialog_key_action(&unsafe { rust_string(chars) }, flags)
            && activate_dialog_key(this, action)
        {
            return;
        }
    }
    unsafe {
        let _: () = msg_send![super(this, class!(NSPanel)), sendEvent: event];
    }
}

fn dialog_panel_class() -> &'static AnyClass {
    if let Some(class) = AnyClass::get("QuillDialogPanel") {
        return class;
    }
    let raw = unsafe {
        objc2::ffi::objc_allocateClassPair(
            class!(NSPanel) as *const AnyClass as *const _,
            c"QuillDialogPanel".as_ptr(),
            0,
        )
    };
    assert!(!raw.is_null(), "could not create dialog panel class");
    unsafe {
        let send_event: extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) = dialog_send_event;
        objc2::ffi::class_addMethod(
            raw,
            sel!(sendEvent:).as_ptr(),
            Some(std::mem::transmute::<
                extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
                unsafe extern "C" fn(),
            >(send_event)),
            c"v@:@".as_ptr(),
        );
        objc2::ffi::objc_registerClassPair(raw);
    }
    AnyClass::get("QuillDialogPanel").expect("dialog panel class was registered")
}

fn dispatch(sender: *mut AnyObject) {
    let Some(target) = actions()
        .lock()
        .ok()
        .and_then(|a| a.iter().find(|a| a.control == sender as usize).copied())
    else {
        return;
    };
    let choice = if matches!(target.id, "list" | "keys") {
        let row: isize = unsafe { msg_send![sender, selectedRow] };
        (row >= 0).then_some((row as usize, false, false))
    } else if target.choice.is_none() && target.id == "font" {
        unsafe {
            let selected: *mut AnyObject = msg_send![sender, selectedItem];
            if selected.is_null() {
                None
            } else {
                let tag: isize = msg_send![selected, tag];
                (tag > 0).then(|| {
                    let encoded = (tag - 1) as usize;
                    (encoded / 4, encoded & 1 != 0, encoded & 2 != 0)
                })
            }
        }
    } else {
        target.choice
    };
    if let Some(proxy) = PROXY.get().and_then(|p| p.lock().ok()) {
        let _ = proxy.send_event(UserEvent::NativeDialog {
            panel: target.panel,
            id: target.id,
            choice,
        });
    }
}

extern "C" fn control_action(_this: *mut AnyObject, _cmd: Sel, sender: *mut AnyObject) {
    if !sender.is_null() {
        dispatch(sender);
    }
}

extern "C" fn text_changed(_this: *mut AnyObject, _cmd: Sel, note: *mut AnyObject) {
    if note.is_null() {
        return;
    }
    let sender: *mut AnyObject = unsafe { msg_send![note, object] };
    if !sender.is_null() {
        dispatch(sender);
    }
}

extern "C" fn table_changed(_this: *mut AnyObject, _cmd: Sel, note: *mut AnyObject) {
    if note.is_null() {
        return;
    }
    let table: *mut AnyObject = unsafe { msg_send![note, object] };
    if !table.is_null() {
        dispatch(table);
    }
}

extern "C" fn table_rows(_this: *mut AnyObject, _cmd: Sel, table: *mut AnyObject) -> isize {
    tables()
        .lock()
        .ok()
        .and_then(|tables| {
            tables
                .iter()
                .find(|data| data.table == table as usize)
                .map(|data| data.rows.len() as isize)
        })
        .unwrap_or(0)
}

extern "C" fn table_value(
    _this: *mut AnyObject,
    _cmd: Sel,
    table: *mut AnyObject,
    _column: *mut AnyObject,
    row: isize,
) -> *mut AnyObject {
    let value = tables().lock().ok().and_then(|tables| {
        tables
            .iter()
            .find(|data| data.table == table as usize)
            .and_then(|data| data.rows.get(row.max(0) as usize).cloned())
    });
    let Some(value) = value.and_then(|value| CString::new(value).ok()) else {
        return std::ptr::null_mut();
    };
    unsafe { msg_send![class!(NSString), stringWithUTF8String: value.as_ptr()] }
}

fn command_panel_is_capturing(panel: *mut AnyObject) -> bool {
    capturing()
        .lock()
        .is_ok_and(|panels| panels.contains(&(panel as usize)))
}

fn native_key_spec(event: *mut AnyObject) -> Option<String> {
    if event.is_null() {
        return None;
    }
    let chars: *mut AnyObject = unsafe { msg_send![event, charactersIgnoringModifiers] };
    let chars = unsafe { rust_string(chars) };
    let first = chars.chars().next()?;
    let key = match first {
        '\r' | '\n' => "↩".to_string(),
        '\t' => "⇥".to_string(),
        ' ' => "Space".to_string(),
        '\u{7f}' => "⌫".to_string(),
        '\u{f728}' => "⌦".to_string(),
        '\u{f700}' => "↑".to_string(),
        '\u{f701}' => "↓".to_string(),
        '\u{f702}' => "←".to_string(),
        '\u{f703}' => "→".to_string(),
        '\u{f729}' => "Home".to_string(),
        '\u{f72b}' => "End".to_string(),
        '\u{f72c}' => "Page Up".to_string(),
        '\u{f72d}' => "Page Down".to_string(),
        c if ('\u{f704}'..='\u{f71b}').contains(&c) => {
            format!("F{}", c as u32 - 0xf704 + 1)
        }
        c if !c.is_control() => c.to_uppercase().collect(),
        _ => return None,
    };
    let flags: usize = unsafe { msg_send![event, modifierFlags] };
    let mut spec = String::new();
    for (mask, symbol) in [
        (1usize << 20, "⌘"),
        (1usize << 17, "⇧"),
        (1usize << 19, "⌥"),
        (1usize << 18, "⌃"),
    ] {
        if flags & mask != 0 {
            spec.push_str(symbol);
        }
    }
    spec.push_str(&key);
    Some(spec)
}

fn capture_shortcut(panel: *mut AnyObject, event: *mut AnyObject) -> bool {
    if !command_panel_is_capturing(panel) {
        return false;
    }
    let chars: *mut AnyObject = unsafe { msg_send![event, charactersIgnoringModifiers] };
    if unsafe { rust_string(chars) }.starts_with('\u{1b}') {
        if let Some(proxy) = PROXY.get().and_then(|proxy| proxy.lock().ok()) {
            let _ = proxy.send_event(UserEvent::NativeShortcutCancel {
                panel: panel as usize,
            });
        }
        return true;
    }
    let Some(spec) = native_key_spec(event) else {
        return false;
    };
    if let Some(proxy) = PROXY.get().and_then(|proxy| proxy.lock().ok()) {
        let _ = proxy.send_event(UserEvent::NativeShortcut {
            panel: panel as usize,
            spec,
        });
    }
    true
}

extern "C" fn command_key_down(this: *mut AnyObject, _cmd: Sel, event: *mut AnyObject) {
    if !capture_shortcut(this, event) {
        unsafe {
            let _: () = msg_send![super(this, class!(NSPanel)), keyDown: event];
        }
    }
}

extern "C" fn command_key_equivalent(
    this: *mut AnyObject,
    _cmd: Sel,
    event: *mut AnyObject,
) -> bool {
    if capture_shortcut(this, event) {
        true
    } else {
        unsafe { msg_send![super(this, class!(NSPanel)), performKeyEquivalent: event] }
    }
}

fn commands_panel_class() -> &'static AnyClass {
    if let Some(class) = AnyClass::get("QuillCommandsPanel") {
        return class;
    }
    let superclass = class!(NSPanel);
    let name = c"QuillCommandsPanel";
    let raw = unsafe {
        objc2::ffi::objc_allocateClassPair(
            superclass as *const AnyClass as *const _,
            name.as_ptr(),
            0,
        )
    };
    assert!(!raw.is_null(), "could not create Commands panel class");
    unsafe {
        let key_down: extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) = command_key_down;
        objc2::ffi::class_addMethod(
            raw,
            sel!(keyDown:).as_ptr(),
            Some(std::mem::transmute::<
                extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
                unsafe extern "C" fn(),
            >(key_down)),
            c"v@:@".as_ptr(),
        );
        let equivalent: extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) -> bool =
            command_key_equivalent;
        objc2::ffi::class_addMethod(
            raw,
            sel!(performKeyEquivalent:).as_ptr(),
            Some(std::mem::transmute::<
                extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) -> bool,
                unsafe extern "C" fn(),
            >(equivalent)),
            c"B@:@".as_ptr(),
        );
        objc2::ffi::objc_registerClassPair(raw);
    }
    AnyClass::get("QuillCommandsPanel").expect("Commands panel class was registered")
}

extern "C" fn should_close(_this: *mut AnyObject, _cmd: Sel, panel: *mut AnyObject) -> bool {
    let key = panel as usize;
    if let Some(proxy) = PROXY.get().and_then(|p| p.lock().ok()) {
        let _ = proxy.send_event(UserEvent::NativeDialog {
            panel: key,
            id: "cancel",
            choice: None,
        });
    }
    false
}

/// Install the selectors used by native dialog controls on winit's existing
/// application delegate.
pub fn install(proxy: EventLoopProxy<UserEvent>) {
    let _ = PROXY.set(Mutex::new(proxy));
    let Some(class) = AnyClass::get("WinitApplicationDelegate") else {
        return;
    };
    unsafe {
        let action: extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) = control_action;
        let text: extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) = text_changed;
        let close: extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) -> bool = should_close;
        objc2::ffi::class_addMethod(
            class as *const AnyClass as *mut _,
            sel!(quillDialogAction:).as_ptr(),
            Some(std::mem::transmute::<
                extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
                unsafe extern "C" fn(),
            >(action)),
            c"v@:@".as_ptr(),
        );
        for selector in [
            sel!(quillDialogRadio0:),
            sel!(quillDialogRadio1:),
            sel!(quillDialogRadio2:),
        ] {
            objc2::ffi::class_addMethod(
                class as *const AnyClass as *mut _,
                selector.as_ptr(),
                Some(std::mem::transmute::<
                    extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
                    unsafe extern "C" fn(),
                >(action)),
                c"v@:@".as_ptr(),
            );
        }
        objc2::ffi::class_addMethod(
            class as *const AnyClass as *mut _,
            sel!(controlTextDidChange:).as_ptr(),
            Some(std::mem::transmute::<
                extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
                unsafe extern "C" fn(),
            >(text)),
            c"v@:@".as_ptr(),
        );
        let table_note: extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) = table_changed;
        objc2::ffi::class_addMethod(
            class as *const AnyClass as *mut _,
            sel!(tableViewSelectionDidChange:).as_ptr(),
            Some(std::mem::transmute::<
                extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
                unsafe extern "C" fn(),
            >(table_note)),
            c"v@:@".as_ptr(),
        );
        let rows: extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) -> isize = table_rows;
        objc2::ffi::class_addMethod(
            class as *const AnyClass as *mut _,
            sel!(numberOfRowsInTableView:).as_ptr(),
            Some(std::mem::transmute::<
                extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) -> isize,
                unsafe extern "C" fn(),
            >(rows)),
            c"q@:@".as_ptr(),
        );
        let value: extern "C" fn(
            *mut AnyObject,
            Sel,
            *mut AnyObject,
            *mut AnyObject,
            isize,
        ) -> *mut AnyObject = table_value;
        objc2::ffi::class_addMethod(
            class as *const AnyClass as *mut _,
            sel!(tableView:objectValueForTableColumn:row:).as_ptr(),
            Some(std::mem::transmute::<
                extern "C" fn(
                    *mut AnyObject,
                    Sel,
                    *mut AnyObject,
                    *mut AnyObject,
                    isize,
                ) -> *mut AnyObject,
                unsafe extern "C" fn(),
            >(value)),
            c"@@:@@q".as_ptr(),
        );
        objc2::ffi::class_addMethod(
            class as *const AnyClass as *mut _,
            sel!(windowShouldClose:).as_ptr(),
            Some(std::mem::transmute::<
                extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) -> bool,
                unsafe extern "C" fn(),
            >(close)),
            c"B@:@".as_ptr(),
        );
    }
}

/// Show macOS's standard About panel. AppKit reads the application name,
/// version and icon from Quill's bundle, so the panel automatically matches
/// the current system appearance without another Quill-owned dialog.
pub fn show_about() {
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, orderFrontStandardAboutPanel: std::ptr::null::<AnyObject>()];
        let _: () = msg_send![app, activateIgnoringOtherApps: true];
    }
}

/// Native conversion is deliberately conservative. A custom canvas item may
/// be interactive or a live preview, so it remains in the old renderer.
pub fn supported(dialog: &Dialog) -> bool {
    is_alert(dialog)
        || is_commands(dialog)
        || !dialog
            .items
            .iter()
            .any(|item| matches!(item.kind, Kind::Custom))
}

fn is_commands(dialog: &Dialog) -> bool {
    dialog.title == "Commands"
        && dialog.items.iter().any(|item| item.id == "list")
        && dialog.items.iter().any(|item| item.id == "keys")
}

fn is_alert(dialog: &Dialog) -> bool {
    dialog.plain
        && dialog
            .items
            .iter()
            .any(|item| item.id == "alert_icon" && matches!(item.kind, Kind::Custom))
}

fn alert_message(dialog: &Dialog) -> String {
    dialog
        .items
        .iter()
        .filter(|item| matches!(item.kind, Kind::Text))
        .map(|item| item.label.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn native_title(dialog: &Dialog) -> &str {
    dialog.native_title.as_deref().unwrap_or(&dialog.title)
}

#[derive(Clone, Copy)]
enum ControlKind {
    Button,
    Check,
    Radio(u8),
    Field,
    Popup(bool),
    Static,
    Group,
    Table,
    Description,
}

struct Control {
    object: Retained<AnyObject>,
    _container: Option<Retained<AnyObject>>,
    id: &'static str,
    kind: ControlKind,
    options: Option<Vec<String>>,
}

pub struct NativeDialog {
    panel: Retained<AnyObject>,
    // An NSAlert owns its window and buttons. Ordinary dialogs do not need
    // a separate owner because their NSPanel owns every control directly.
    _alert: Option<Retained<AnyObject>>,
    parent: *mut AnyObject,
    controls: Vec<Control>,
    depth: usize,
    title: String,
    modeless: bool,
    alert_message: Option<String>,
}

impl NativeDialog {
    pub fn panel_id(&self) -> usize {
        Retained::as_ptr(&self.panel) as usize
    }

    pub fn matches(&self, depth: usize, dialog: &Dialog) -> bool {
        if let Some(message) = &self.alert_message {
            return self.depth == depth
                && is_alert(dialog)
                && *message == alert_message(dialog)
                && self.controls.len()
                    == dialog
                        .items
                        .iter()
                        .filter(|item| matches!(item.kind, Kind::Button { .. }))
                        .count()
                && self.controls.iter().all(|control| {
                    dialog.items.iter().any(|item| {
                        item.id == control.id && matches!(item.kind, Kind::Button { .. })
                    })
                });
        }
        self.depth == depth
            && self.title == native_title(dialog)
            && self.modeless == dialog.modeless
            && self.controls.len() == dialog.items.len()
            && self
                .controls
                .iter()
                .zip(&dialog.items)
                .all(|(control, item)| control.matches(item))
    }

    pub fn show(window: &Window, depth: usize, dialog: &Dialog) -> Option<Self> {
        if !supported(dialog) {
            return None;
        }
        let handle = window.window_handle().ok()?.as_raw();
        let RawWindowHandle::AppKit(handle) = handle else {
            return None;
        };
        let view = handle.ns_view.as_ptr().cast::<AnyObject>();
        let parent: *mut AnyObject = unsafe { msg_send![view, window] };
        if parent.is_null() {
            return None;
        }

        if is_alert(dialog) {
            return Self::show_alert(parent, depth, dialog);
        }

        let width = dialog.width as f64 * SCALE;
        let height = dialog.height as f64 * SCALE;
        let rect = NSRect::new(NSPoint::ZERO, NSSize::new(width, height));
        let style: usize = 1 | 2; // titled + closable
        let panel_class = if is_commands(dialog) {
            commands_panel_class()
        } else {
            dialog_panel_class()
        };
        let raw: *mut AnyObject = unsafe { msg_send![panel_class, alloc] };
        let raw: *mut AnyObject = unsafe {
            msg_send![raw, initWithContentRect: rect styleMask: style backing: 2usize defer: false]
        };
        let panel = unsafe { Retained::from_raw(raw) }?;
        let panel_ptr = Retained::as_ptr(&panel) as *mut AnyObject;
        let title = NSString::from_str(native_title(dialog));
        let app: *mut AnyObject = unsafe { msg_send![class!(NSApplication), sharedApplication] };
        let delegate: *mut AnyObject = unsafe { msg_send![app, delegate] };
        unsafe {
            let _: () = msg_send![panel_ptr, setTitle: &*title];
            let _: () = msg_send![panel_ptr, setReleasedWhenClosed: false];
            let _: () = msg_send![panel_ptr, setDelegate: delegate];
            let _: () = msg_send![panel_ptr, setHidesOnDeactivate: false];
            let _: () = msg_send![panel_ptr, setAutorecalculatesKeyViewLoop: true];
            // A child NSPanel may otherwise leave its document window key,
            // sending Return, Escape and Command-period to winit instead of
            // the native controls and first responder.
            let _: () = msg_send![panel_ptr, setBecomesKeyOnlyIfNeeded: false];
            if dialog.modeless {
                let _: () = msg_send![panel_ptr, setFloatingPanel: true];
            }
        }
        let content: *mut AnyObject = unsafe { msg_send![panel_ptr, contentView] };
        if content.is_null() {
            return None;
        }

        let mut controls = Vec::with_capacity(dialog.items.len());
        for item in &dialog.items {
            let frame = native_rect(item.rect, height);
            let (object, container, kind) =
                (unsafe { make_control(item, frame, panel_ptr, delegate, content) })?;
            controls.push(Control {
                object,
                _container: container,
                id: item.id,
                kind,
                options: match &item.kind {
                    Kind::Popup { options, .. } => Some(options.clone()),
                    _ => None,
                },
            });
        }
        let mut result = Self {
            panel,
            _alert: None,
            parent,
            controls,
            depth,
            title: native_title(dialog).to_string(),
            modeless: dialog.modeless,
            alert_message: None,
        };
        result.sync_from_model(dialog);
        unsafe {
            let _: () = msg_send![panel_ptr, recalculateKeyViewLoop];
            if let Some(id) = dialog.focused_id()
                && let Some(control) = result.controls.iter().find(|control| control.id == id)
            {
                let _: () = msg_send![panel_ptr, setInitialFirstResponder: Retained::as_ptr(&control.object)];
                let _: bool = msg_send![
                    panel_ptr,
                    makeFirstResponder: Retained::as_ptr(&control.object)
                ];
            }
            let _: () = msg_send![panel_ptr, center];
            if dialog.modeless {
                let _: () =
                    msg_send![panel_ptr, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
                let _: () = msg_send![panel_ptr, makeKeyWindow];
            } else {
                // Keep a modal panel attached to its document while letting
                // winit's event loop continue to drive both windows.
                let _: () = msg_send![parent, addChildWindow: panel_ptr ordered: 1isize];
                let _: () = msg_send![parent, setIgnoresMouseEvents: true];
                let _: () =
                    msg_send![panel_ptr, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
                let _: () = msg_send![panel_ptr, makeKeyWindow];
            }
        }
        Some(result)
    }

    fn show_alert(parent: *mut AnyObject, depth: usize, dialog: &Dialog) -> Option<Self> {
        let raw: *mut AnyObject = unsafe { msg_send![class!(NSAlert), alloc] };
        let raw: *mut AnyObject = unsafe { msg_send![raw, init] };
        let alert = unsafe { Retained::from_raw(raw) }?;
        let alert_ptr = Retained::as_ptr(&alert) as *mut AnyObject;
        let message_text = alert_message(dialog);
        let message = NSString::from_str(&message_text);
        let app: *mut AnyObject = unsafe { msg_send![class!(NSApplication), sharedApplication] };
        let delegate: *mut AnyObject = unsafe { msg_send![app, delegate] };
        let button_count = dialog
            .items
            .iter()
            .filter(|item| matches!(item.kind, Kind::Button { .. }))
            .count();
        unsafe {
            let _: () = msg_send![alert_ptr, setMessageText: &*message];
            // One-button notices are informational. Questions with a Cancel
            // choice use the standard warning appearance.
            let style: isize = if button_count == 1 { 1 } else { 0 };
            let _: () = msg_send![alert_ptr, setAlertStyle: style];
            let _: () = msg_send![alert_ptr, setShowsHelp: false];
            let _: () = msg_send![alert_ptr, setShowsSuppressionButton: false];
        }

        let panel_ptr: *mut AnyObject = unsafe { msg_send![alert_ptr, window] };
        let panel = unsafe { Retained::retain(panel_ptr) }?;
        let panel_ptr = Retained::as_ptr(&panel) as *mut AnyObject;

        let mut controls = Vec::with_capacity(button_count);
        for item in dialog
            .items
            .iter()
            .filter(|item| matches!(item.kind, Kind::Button { .. }))
        {
            let title = NSString::from_str(&item.label);
            let button_ptr: *mut AnyObject =
                unsafe { msg_send![alert_ptr, addButtonWithTitle: &*title] };
            let button = unsafe { Retained::retain(button_ptr) }?;
            unsafe {
                let _: () = msg_send![button_ptr, setTarget: delegate];
                let _: () = msg_send![button_ptr, setAction: sel!(quillDialogAction:)];
                if item.id == "cancel" {
                    let escape = NSString::from_str("\u{1b}");
                    let _: () = msg_send![button_ptr, setKeyEquivalent: &*escape];
                }
            }
            actions().lock().ok()?.push(ActionTarget {
                control: button_ptr as usize,
                panel: panel_ptr as usize,
                id: item.id,
                choice: None,
                default: matches!(&item.kind, Kind::Button { default: true }),
            });
            controls.push(Control {
                object: button,
                _container: None,
                id: item.id,
                kind: ControlKind::Button,
                options: None,
            });
        }

        unsafe {
            // NSAlert keeps placeholder controls until it performs its final
            // layout pass. Without this, a manually presented alert can show
            // its normally hidden help, suppression and spare button views.
            let _: () = msg_send![alert_ptr, layout];
            let _: () = msg_send![panel_ptr, setDelegate: delegate];
            let _: () = msg_send![panel_ptr, setReleasedWhenClosed: false];
            let _: () = msg_send![parent, addChildWindow: panel_ptr ordered: 1isize];
            let _: () = msg_send![parent, setIgnoresMouseEvents: true];
            let _: () = msg_send![panel_ptr, center];
            let _: () = msg_send![panel_ptr, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
        }
        Some(Self {
            panel,
            _alert: Some(alert),
            parent,
            controls,
            depth,
            title: dialog.title.clone(),
            modeless: false,
            alert_message: Some(message_text),
        })
    }

    pub fn apply_to_model(
        &self,
        dialog: &mut Dialog,
        changed: &'static str,
        choice: Option<(usize, bool, bool)>,
    ) {
        // AppKit handles checkboxes. Quill handles radio groups explicitly so
        // groups in different boxes cannot interfere with one another.
        let radio_group = self.controls.iter().find_map(|c| {
            (c.id == changed).then_some(match c.kind {
                ControlKind::Radio(group) => Some(group),
                _ => None,
            })?
        });
        for control in &self.controls {
            let ptr = Retained::as_ptr(&control.object) as *mut AnyObject;
            unsafe {
                match control.kind {
                    ControlKind::Field => {
                        let s: *mut AnyObject = msg_send![ptr, stringValue];
                        dialog.set_field(control.id, &rust_string(s));
                    }
                    ControlKind::Popup(menu_button) => {
                        let selected: isize = msg_send![ptr, indexOfSelectedItem];
                        let selected = if menu_button {
                            selected.saturating_sub(1)
                        } else {
                            selected
                        };
                        dialog.set_selected(control.id, selected.max(0) as usize);
                    }
                    ControlKind::Check => {
                        let state: isize = msg_send![ptr, state];
                        dialog.set_on(control.id, state == 1);
                    }
                    ControlKind::Radio(group) => {
                        if Some(group) == radio_group {
                            dialog.set_on(control.id, control.id == changed);
                        }
                    }
                    ControlKind::Button
                    | ControlKind::Static
                    | ControlKind::Group
                    | ControlKind::Table
                    | ControlKind::Description => {}
                }
            }
        }
        if let Some((family, bold, italic)) = choice {
            dialog.set_selected(changed, family);
            dialog.set_on("bold", bold);
            dialog.set_on("italic", italic);
        }
    }

    pub fn sync_from_model(&mut self, dialog: &Dialog) {
        if self.alert_message.is_some() {
            return;
        }
        for (control, item) in self.controls.iter().zip(&dialog.items) {
            let ptr = Retained::as_ptr(&control.object) as *mut AnyObject;
            unsafe {
                if !matches!(control.kind, ControlKind::Group | ControlKind::Description) {
                    let _: () = msg_send![ptr, setEnabled: item.enabled];
                }
                match &item.kind {
                    Kind::Field { text, .. } => {
                        let current: *mut AnyObject = msg_send![ptr, stringValue];
                        if rust_string(current) != *text {
                            let value = NSString::from_str(text);
                            let _: () = msg_send![ptr, setStringValue: &*value];
                        }
                    }
                    Kind::Check { on, mixed, .. } => {
                        let state: isize = if *mixed {
                            -1
                        } else if *on {
                            1
                        } else {
                            0
                        };
                        let _: () = msg_send![ptr, setState: state];
                        let value = NSString::from_str(&item.label);
                        let _: () = msg_send![ptr, setTitle: &*value];
                    }
                    Kind::Radio { on, .. } => {
                        let state: isize = if *on { 1 } else { 0 };
                        let _: () = msg_send![ptr, setState: state];
                        let value = NSString::from_str(&item.label);
                        let _: () = msg_send![ptr, setTitle: &*value];
                    }
                    Kind::Popup { selected, .. } => {
                        if *selected != usize::MAX {
                            let _: () = msg_send![ptr, selectItemAtIndex: *selected as isize];
                        }
                    }
                    Kind::Text | Kind::Note => {
                        let value = NSString::from_str(&item.label);
                        let _: () = msg_send![ptr, setStringValue: &*value];
                    }
                    Kind::Button { .. } => {
                        let value = NSString::from_str(&item.label);
                        let _: () = msg_send![ptr, setTitle: &*value];
                    }
                    Kind::Group => {
                        let value = NSString::from_str(&item.label);
                        let _: () = msg_send![ptr, setTitle: &*value];
                    }
                    Kind::Custom if matches!(control.kind, ControlKind::Table) => {
                        let rows = crate::commands_dialog::native_rows(dialog, control.id);
                        let has_rows = !rows.is_empty();
                        let changed = tables().lock().is_ok_and(|mut tables| {
                            let Some(data) =
                                tables.iter_mut().find(|data| data.table == ptr as usize)
                            else {
                                return false;
                            };
                            if data.rows == rows {
                                return false;
                            }
                            data.rows = rows;
                            true
                        });
                        if changed {
                            let _: () = msg_send![ptr, reloadData];
                        }
                        let desired = crate::commands_dialog::native_selection(dialog, control.id)
                            .or_else(|| (control.id == "keys" && changed && has_rows).then_some(0));
                        if let Some(row) = desired {
                            let selected: isize = msg_send![ptr, selectedRow];
                            if selected != row as isize {
                                let set: *mut AnyObject =
                                    msg_send![class!(NSIndexSet), indexSetWithIndex: row];
                                let _: () = msg_send![ptr, selectRowIndexes: set byExtendingSelection: false];
                                let _: () = msg_send![ptr, scrollRowToVisible: row as isize];
                            }
                        }
                    }
                    Kind::Custom if matches!(control.kind, ControlKind::Description) => {
                        let value =
                            NSString::from_str(&crate::commands_dialog::native_description(dialog));
                        let _: () = msg_send![ptr, setStringValue: &*value];
                    }
                    _ => {}
                }
            }
        }
        if is_commands(dialog) {
            let panel = self.panel_id();
            if let Ok(mut panels) = capturing().lock() {
                panels.retain(|candidate| *candidate != panel);
                if dialog.item_label("name") == "Press the new key…" {
                    panels.push(panel);
                }
            }
        }
    }

    pub fn outcome(dialog: &Dialog, id: &'static str) -> Outcome {
        match id {
            "ok" => Outcome::Ok,
            "cancel" => Outcome::Cancel,
            "apply" => Outcome::Apply,
            id if dialog
                .items
                .iter()
                .any(|item| item.id == id && matches!(item.kind, Kind::Button { .. })) =>
            {
                Outcome::Button(id)
            }
            id => Outcome::Changed(id),
        }
    }
}

impl Control {
    fn matches(&self, item: &crate::dialog::Item) -> bool {
        match (&self.kind, &item.kind) {
            (ControlKind::Button, Kind::Button { .. })
            | (ControlKind::Check, Kind::Check { .. })
            | (ControlKind::Field, Kind::Field { .. })
            | (ControlKind::Static, Kind::Text | Kind::Note)
            | (ControlKind::Group, Kind::Group) => true,
            (ControlKind::Table | ControlKind::Description, Kind::Custom) => true,
            (ControlKind::Radio(a), Kind::Radio { group: b, .. }) => a == b,
            (
                ControlKind::Popup(menu_button),
                Kind::Popup {
                    options, selected, ..
                },
            ) => {
                *menu_button == (*selected == usize::MAX)
                    && self.options.as_ref().is_some_and(|old| old == options)
            }
            _ => false,
        }
    }
}

impl Drop for NativeDialog {
    fn drop(&mut self) {
        let panel = Retained::as_ptr(&self.panel) as *mut AnyObject;
        if let Ok(mut a) = actions().lock() {
            a.retain(|a| a.panel != panel as usize);
        }
        if let Ok(mut tables) = tables().lock() {
            tables.retain(|data| {
                !self
                    .controls
                    .iter()
                    .any(|control| Retained::as_ptr(&control.object) as usize == data.table)
            });
        }
        if let Ok(mut panels) = capturing().lock() {
            panels.retain(|candidate| *candidate != panel as usize);
        }
        unsafe {
            let _: () = msg_send![panel, setDelegate: std::ptr::null::<AnyObject>()];
            if self.modeless {
                let _: () = msg_send![panel, orderOut: std::ptr::null::<AnyObject>()];
            } else {
                let _: () = msg_send![self.parent, removeChildWindow: panel];
                let _: () = msg_send![self.parent, setIgnoresMouseEvents: false];
                let _: () = msg_send![panel, orderOut: std::ptr::null::<AnyObject>()];
            }
            let _: () = msg_send![self.parent, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
        }
    }
}

fn native_rect(r: crate::canvas::Rect, content_height: f64) -> NSRect {
    let x = r.x as f64 * SCALE;
    let h = r.h as f64 * SCALE;
    let y = content_height - (r.y + r.h) as f64 * SCALE;
    NSRect::new(NSPoint::new(x, y), NSSize::new(r.w as f64 * SCALE, h))
}

unsafe fn make_control(
    item: &crate::dialog::Item,
    frame: NSRect,
    panel: *mut AnyObject,
    delegate: *mut AnyObject,
    content: *mut AnyObject,
) -> Option<(
    Retained<AnyObject>,
    Option<Retained<AnyObject>>,
    ControlKind,
)> {
    if matches!(item.kind, Kind::Custom) {
        return unsafe { make_commands_control(item, frame, panel, delegate, content) };
    }
    let (class, kind, button_type): (&AnyClass, ControlKind, Option<usize>) = match item.kind {
        Kind::Button { .. } => (class!(NSButton), ControlKind::Button, Some(0)),
        Kind::Check { .. } => (class!(NSButton), ControlKind::Check, Some(3)),
        Kind::Radio { group, .. } => (class!(NSButton), ControlKind::Radio(group), Some(4)),
        Kind::Field { .. } => (class!(NSTextField), ControlKind::Field, None),
        Kind::Popup { selected, .. } => (
            class!(NSPopUpButton),
            ControlKind::Popup(selected == usize::MAX),
            None,
        ),
        Kind::Text | Kind::Note => (class!(NSTextField), ControlKind::Static, None),
        Kind::Group => (class!(NSBox), ControlKind::Group, None),
        Kind::Custom => unreachable!(),
    };
    let raw: *mut AnyObject = unsafe { msg_send![class, alloc] };
    let raw: *mut AnyObject = if let Kind::Popup { selected, .. } = item.kind {
        unsafe { msg_send![raw, initWithFrame: frame pullsDown: selected == usize::MAX] }
    } else {
        unsafe { msg_send![raw, initWithFrame: frame] }
    };
    let object = unsafe { Retained::from_raw(raw) }?;
    let ptr = Retained::as_ptr(&object) as *mut AnyObject;
    let label = NSString::from_str(&item.label);
    unsafe {
        match &item.kind {
            Kind::Button { default } => {
                let _: () = msg_send![ptr, setTitle: &*label];
                if *default {
                    let key = NSString::from_str("\r");
                    let _: () = msg_send![ptr, setKeyEquivalent: &*key];
                } else if item.id == "cancel" {
                    let key = NSString::from_str("\u{1b}");
                    let _: () = msg_send![ptr, setKeyEquivalent: &*key];
                }
            }
            Kind::Check { tri, .. } => {
                let _: () = msg_send![ptr, setTitle: &*label];
                let _: () = msg_send![ptr, setAllowsMixedState: *tri];
            }
            Kind::Radio { .. } => {
                let _: () = msg_send![ptr, setTitle: &*label];
            }
            Kind::Field { text, .. } => {
                let value = NSString::from_str(text);
                let _: () = msg_send![ptr, setStringValue: &*value];
                let _: () = msg_send![ptr, setDelegate: delegate];
            }
            Kind::Popup {
                options,
                selected,
                font_families,
            } => {
                if *selected == usize::MAX {
                    let title = NSString::from_str(&item.label);
                    let _: () = msg_send![ptr, addItemWithTitle: &*title];
                }
                for option in options {
                    let value = NSString::from_str(option);
                    let _: () = msg_send![ptr, addItemWithTitle: &*value];
                }
                if *selected != usize::MAX {
                    let _: () = msg_send![ptr, selectItemAtIndex: *selected as isize];
                }
                if let Some(families) = font_families {
                    decorate_font_popup(ptr, panel, delegate, item.id, families);
                }
            }
            Kind::Text | Kind::Note => {
                let _: () = msg_send![ptr, setStringValue: &*label];
                let _: () = msg_send![ptr, setEditable: false];
                let _: () = msg_send![ptr, setSelectable: false];
                let _: () = msg_send![ptr, setBezeled: false];
                let _: () = msg_send![ptr, setDrawsBackground: false];
            }
            Kind::Group => {
                let _: () = msg_send![ptr, setTitle: &*label];
            }
            Kind::Custom => unreachable!(),
        }
        if let Some(button_type) = button_type {
            let _: () = msg_send![ptr, setButtonType: button_type];
        }
        if !matches!(kind, ControlKind::Static | ControlKind::Group) {
            let _: () = msg_send![ptr, setTarget: delegate];
            let action = match kind {
                ControlKind::Radio(0) => sel!(quillDialogRadio0:),
                ControlKind::Radio(1) => sel!(quillDialogRadio1:),
                ControlKind::Radio(_) => sel!(quillDialogRadio2:),
                _ => sel!(quillDialogAction:),
            };
            let _: () = msg_send![ptr, setAction: action];
            actions().lock().ok()?.push(ActionTarget {
                control: ptr as usize,
                panel: panel as usize,
                id: item.id,
                choice: None,
                default: matches!(&item.kind, Kind::Button { default: true }),
            });
        }
        let _: () = msg_send![content, addSubview: ptr];
    }
    Some((object, None, kind))
}

unsafe fn make_commands_control(
    item: &crate::dialog::Item,
    frame: NSRect,
    panel: *mut AnyObject,
    delegate: *mut AnyObject,
    content: *mut AnyObject,
) -> Option<(
    Retained<AnyObject>,
    Option<Retained<AnyObject>>,
    ControlKind,
)> {
    if item.id == "description" {
        let raw: *mut AnyObject = unsafe { msg_send![class!(NSTextField), alloc] };
        let raw: *mut AnyObject = unsafe { msg_send![raw, initWithFrame: frame] };
        let field = unsafe { Retained::from_raw(raw) }?;
        let ptr = Retained::as_ptr(&field) as *mut AnyObject;
        unsafe {
            let _: () = msg_send![ptr, setEditable: false];
            let _: () = msg_send![ptr, setSelectable: true];
            let _: () = msg_send![ptr, setBezeled: true];
            let _: () = msg_send![ptr, setDrawsBackground: true];
            let cell: *mut AnyObject = msg_send![ptr, cell];
            let _: () = msg_send![cell, setWraps: true];
            let _: () = msg_send![cell, setScrollable: false];
            let _: () = msg_send![content, addSubview: ptr];
        }
        return Some((field, None, ControlKind::Description));
    }
    if !matches!(item.id, "list" | "keys") {
        return None;
    }

    let scroll_raw: *mut AnyObject = unsafe { msg_send![class!(NSScrollView), alloc] };
    let scroll_raw: *mut AnyObject = unsafe { msg_send![scroll_raw, initWithFrame: frame] };
    let scroll = unsafe { Retained::from_raw(scroll_raw) }?;
    let scroll_ptr = Retained::as_ptr(&scroll) as *mut AnyObject;

    let table_raw: *mut AnyObject = unsafe { msg_send![class!(NSTableView), alloc] };
    let table_raw: *mut AnyObject =
        unsafe { msg_send![table_raw, initWithFrame: NSRect::new(NSPoint::ZERO, frame.size)] };
    let table = unsafe { Retained::from_raw(table_raw) }?;
    let table_ptr = Retained::as_ptr(&table) as *mut AnyObject;

    let column_raw: *mut AnyObject = unsafe { msg_send![class!(NSTableColumn), alloc] };
    let identifier = NSString::from_str(item.id);
    let column_raw: *mut AnyObject =
        unsafe { msg_send![column_raw, initWithIdentifier: &*identifier] };
    let column = unsafe { Retained::from_raw(column_raw) }?;
    let column_ptr = Retained::as_ptr(&column) as *mut AnyObject;
    unsafe {
        let _: () = msg_send![column_ptr, setWidth: frame.size.width];
        let _: () = msg_send![table_ptr, addTableColumn: column_ptr];
        let _: () = msg_send![table_ptr, setHeaderView: std::ptr::null::<AnyObject>()];
        let _: () = msg_send![table_ptr, setAllowsMultipleSelection: false];
        let _: () = msg_send![table_ptr, setAllowsEmptySelection: item.id == "keys"];
        let _: () = msg_send![table_ptr, setRowHeight: 20.0f64];
        let _: () = msg_send![table_ptr, setIntercellSpacing: NSSize::new(0.0, 1.0)];
        let _: () = msg_send![table_ptr, setDelegate: delegate];
        let _: () = msg_send![table_ptr, setDataSource: delegate];
        let _: () = msg_send![scroll_ptr, setDocumentView: table_ptr];
        let _: () = msg_send![scroll_ptr, setHasVerticalScroller: true];
        let _: () = msg_send![scroll_ptr, setAutohidesScrollers: true];
        let _: () = msg_send![scroll_ptr, setBorderType: 2usize];
        let _: () = msg_send![content, addSubview: scroll_ptr];
    }
    tables().lock().ok()?.push(TableData {
        table: table_ptr as usize,
        rows: Vec::new(),
    });
    actions().lock().ok()?.push(ActionTarget {
        control: table_ptr as usize,
        panel: panel as usize,
        id: item.id,
        choice: None,
        default: false,
    });
    Some((table, Some(scroll), ControlKind::Table))
}

unsafe fn decorate_font_popup(
    popup: *mut AnyObject,
    panel: *mut AnyObject,
    delegate: *mut AnyObject,
    id: &'static str,
    families: &[quill_layout::FontFamilyInfo],
) {
    for (family_index, family) in families.iter().enumerate() {
        let item: *mut AnyObject = unsafe { msg_send![popup, itemAtIndex: family_index as isize] };
        if item.is_null() {
            continue;
        }
        if let Some(title) = attributed_font_title(&family.name, &family.name, false, false) {
            let value = Retained06::as_ptr(&title).cast::<AnyObject>();
            let _: () = unsafe { msg_send![item, setAttributedTitle: value] };
        }
        if family.variants.len() <= 1 {
            continue;
        }
        let menu_raw: *mut AnyObject = unsafe { msg_send![class!(NSMenu), alloc] };
        let menu_title = NSString::from_str(&family.name);
        let menu_raw: *mut AnyObject = unsafe { msg_send![menu_raw, initWithTitle: &*menu_title] };
        let Some(menu) = (unsafe { Retained::from_raw(menu_raw) }) else {
            continue;
        };
        for variant in &family.variants {
            let label_text = match (variant.bold, variant.italic) {
                (false, false) => "Normal",
                (true, false) => "Bold",
                (false, true) => "Italic",
                (true, true) => "Bold Italic",
            };
            let label = NSString::from_str(label_text);
            let empty = NSString::from_str("");
            let child_raw: *mut AnyObject = unsafe { msg_send![class!(NSMenuItem), alloc] };
            let child_raw: *mut AnyObject = unsafe {
                msg_send![child_raw, initWithTitle: &*label action: sel!(quillDialogAction:) keyEquivalent: &*empty]
            };
            let Some(child) = (unsafe { Retained::from_raw(child_raw) }) else {
                continue;
            };
            let child_ptr = Retained::as_ptr(&child) as *mut AnyObject;
            unsafe {
                let _: () = msg_send![child_ptr, setTarget: delegate];
                let flags = usize::from(variant.bold) | (usize::from(variant.italic) << 1);
                let tag = 1 + family_index * 4 + flags;
                let _: () = msg_send![child_ptr, setTag: tag as isize];
                if let Some(title) =
                    attributed_font_title(label_text, &family.name, variant.bold, variant.italic)
                {
                    let value = Retained06::as_ptr(&title).cast::<AnyObject>();
                    let _: () = msg_send![child_ptr, setAttributedTitle: value];
                }
                let _: () = msg_send![Retained::as_ptr(&menu), addItem: child_ptr];
            }
            if let Ok(mut entries) = actions().lock() {
                entries.push(ActionTarget {
                    control: child_ptr as usize,
                    panel: panel as usize,
                    id,
                    choice: Some((family_index, variant.bold, variant.italic)),
                    default: false,
                });
            }
        }
        unsafe {
            let _: () = msg_send![item, setSubmenu: Retained::as_ptr(&menu)];
        }
    }
}

fn attributed_font_title(
    title: &str,
    family: &str,
    bold: bool,
    italic: bool,
) -> Option<Retained06<NSAttributedString>> {
    let mut font = NSFont::fontWithName_size(&NSString03::from_str(family), 14.0)?;
    let mut traits = NSFontTraitMask::empty();
    if bold {
        traits |= NSFontTraitMask::BoldFontMask;
    }
    if italic {
        traits |= NSFontTraitMask::ItalicFontMask;
    }
    if !traits.is_empty()
        && let Some(mtm) = MainThreadMarker03::new()
    {
        font = NSFontManager::sharedFontManager(mtm).convertFont_toHaveTrait(&font, traits);
    }
    let values = [Retained06::into_super(Retained06::into_super(font))];
    let attrs = NSDictionary::from_retained_objects(&[unsafe { NSFontAttributeName }], &values);
    Some(unsafe { NSAttributedString::new_with_attributes(&NSString03::from_str(title), &attrs) })
}

unsafe fn rust_string(value: *mut AnyObject) -> String {
    if value.is_null() {
        return String::new();
    }
    let utf8: *const c_char = unsafe { msg_send![value, UTF8String] };
    if utf8.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(utf8) }
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_dialogs_are_native_but_custom_surfaces_fall_back() {
        let mut dialog = Dialog::new("Ordinary", 200.0, 100.0);
        dialog.field("name", "", 10.0, 10.0, 100.0);
        dialog.button("go", "Go", 120.0, 10.0, 60.0, true);
        assert!(supported(&dialog));
        dialog.custom("preview", 10.0, 40.0, 170.0, 40.0);
        assert!(!supported(&dialog));

        let alert = Dialog::alert("Something happened.", &[("ok", "OK")]);
        assert!(supported(&alert));
        assert!(is_alert(&alert));
        assert_eq!(alert_message(&alert), "Something happened.");

        let (commands, _) = crate::commands_dialog::dialog(None);
        assert!(supported(&commands));
        assert!(is_commands(&commands));
    }

    #[test]
    fn native_buttons_keep_dialog_outcome_semantics() {
        let mut dialog = Dialog::new("Actions", 200.0, 100.0);
        dialog.button("ok", "OK", 10.0, 10.0, 50.0, true);
        dialog.button("tabs", "Tabs", 70.0, 10.0, 50.0, false);
        dialog.check("keep", "Keep", 10.0, 40.0, false);
        assert_eq!(NativeDialog::outcome(&dialog, "ok"), Outcome::Ok);
        assert_eq!(
            NativeDialog::outcome(&dialog, "tabs"),
            Outcome::Button("tabs")
        );
        assert_eq!(
            NativeDialog::outcome(&dialog, "keep"),
            Outcome::Changed("keep")
        );
    }

    #[test]
    fn native_dialog_keyboard_routes_default_and_cancel_actions() {
        assert_eq!(dialog_key_action("\r", 0), Some(DialogKeyAction::Default));
        assert_eq!(
            dialog_key_action("\u{3}", 0),
            Some(DialogKeyAction::Default)
        );
        assert_eq!(
            dialog_key_action("\u{1b}", 0),
            Some(DialogKeyAction::Cancel)
        );
        assert_eq!(
            dialog_key_action(".", 1usize << 20),
            Some(DialogKeyAction::Cancel)
        );
        assert_eq!(dialog_key_action(".", 0), None);
        assert_eq!(dialog_key_action("x", 1usize << 20), None);
    }

    #[test]
    fn a_classic_plain_dialog_can_supply_a_native_window_title() {
        let mut dialog = Dialog::new("", 200.0, 100.0);
        dialog.plain = true;
        dialog.native_title = Some("Go To".to_string());
        assert_eq!(native_title(&dialog), "Go To");
    }
}
