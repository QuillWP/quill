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
use objc2_foundation::{NSPoint, NSRange, NSRect, NSSize, NSString};
use objc2_foundation_03::{
    MainThreadMarker as MainThreadMarker03, NSAttributedString, NSDictionary,
    NSString as NSString03,
};
use std::ffi::{CStr, CString, c_char};
use std::sync::{Mutex, OnceLock};
use winit::event_loop::EventLoopProxy;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

// AppKit selections count UTF-16 units; dialog fields use UTF-8 byte offsets.
fn byte_offset(text: &str, units: usize) -> usize {
    let mut used = 0;
    for (byte, ch) in text.char_indices() {
        used += ch.len_utf16();
        if used > units {
            return byte;
        }
    }
    text.len()
}

const SCALE: f64 = 1.25;

/// Let font sources include application bundles without asking the user to
/// reveal their contents in Finder first. Other folder pickers keep treating
/// packages as files.
pub fn pick_font_folder(directory: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    objc2::rc::autoreleasepool(|_| {
        // SAFETY: Preferences calls this on AppKit's main thread. The retained
        // panel and autorelease pool keep its URL and strings alive until the
        // selected filesystem path has been copied.
        unsafe {
            let raw: *mut AnyObject = msg_send![class!(NSOpenPanel), openPanel];
            let panel = Retained::retain(raw)?;
            let panel = &*panel;
            let title = NSString::from_str("Choose a Font Folder or App");
            let _: () = msg_send![panel, setTitle: &*title];
            let _: () = msg_send![panel, setCanChooseDirectories: true];
            let _: () = msg_send![panel, setCanChooseFiles: false];
            let _: () = msg_send![panel, setAllowsMultipleSelection: false];
            let _: () = msg_send![panel, setTreatsFilePackagesAsDirectories: true];
            if let Some(directory) = directory {
                let path = NSString::from_str(&directory.to_string_lossy());
                let url: *mut AnyObject =
                    msg_send![class!(NSURL), fileURLWithPath: &*path isDirectory: true];
                let _: () = msg_send![panel, setDirectoryURL: url];
            }
            let response: isize = msg_send![panel, runModal];
            if response != 1 {
                return None;
            }
            let url: *mut AnyObject = msg_send![panel, URL];
            if url.is_null() {
                return None;
            }
            let path: *const c_char = msg_send![url, fileSystemRepresentation];
            if path.is_null() {
                return None;
            }
            use std::os::unix::ffi::OsStrExt;
            Some(std::path::PathBuf::from(std::ffi::OsStr::from_bytes(
                CStr::from_ptr(path).to_bytes(),
            )))
        }
    })
}

static PROXY: OnceLock<Mutex<EventLoopProxy<UserEvent>>> = OnceLock::new();
static ACTIONS: OnceLock<Mutex<Vec<ActionTarget>>> = OnceLock::new();
static TABLES: OnceLock<Mutex<Vec<TableData>>> = OnceLock::new();

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
        let equivalent: extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) -> bool =
            dialog_key_equivalent;
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
    AnyClass::get("QuillDialogPanel").expect("dialog panel class was registered")
}

/// Command-X, C, V, A and Z in a dialog's text field belong to the field.
/// The menu bar would otherwise claim them for the document behind (or, for
/// Cut, find its item disabled and do nothing).
extern "C" fn dialog_key_equivalent(
    this: *mut AnyObject,
    _cmd: Sel,
    event: *mut AnyObject,
) -> bool {
    use crate::commands::Cmd;
    const COMMAND: usize = 1 << 20;
    const SHIFT_CONTROL_OPTION: usize = (1 << 17) | (1 << 18) | (1 << 19);
    let flags: usize = unsafe { msg_send![event, modifierFlags] };
    if flags & COMMAND != 0 && flags & SHIFT_CONTROL_OPTION == 0 {
        let chars: *mut AnyObject = unsafe { msg_send![event, charactersIgnoringModifiers] };
        let cmd = match unsafe { rust_string(chars) }.as_str() {
            "x" => Some(Cmd::Cut),
            "c" => Some(Cmd::Copy),
            "v" => Some(Cmd::Paste),
            "a" => Some(Cmd::SelectAll),
            "z" => Some(Cmd::Undo),
            _ => None,
        };
        if cmd.is_some_and(edit_in_field) {
            return true;
        }
    }
    unsafe { msg_send![super(this, class!(NSPanel)), performKeyEquivalent: event] }
}

fn dispatch(sender: *mut AnyObject) {
    let Some(target) = actions()
        .lock()
        .ok()
        .and_then(|a| a.iter().find(|a| a.control == sender as usize).copied())
    else {
        return;
    };
    // IDs are shared by table views and ordinary style popups. Only a table
    // answers selectedRow; a popup's selection is read from its dialog value.
    let is_table: bool = unsafe { msg_send![sender, isKindOfClass: class!(NSTableView)] };
    let choice = if is_table {
        let row: isize = unsafe { msg_send![sender, selectedRow] };
        let double = unsafe {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let event: *mut AnyObject = msg_send![app, currentEvent];
            !event.is_null() && {
                let kind: usize = msg_send![event, type];
                // Only a mouse-up can be a double-click; a selection moved
                // by the keyboard is not.
                let clicks: isize = if kind == 2 {
                    msg_send![event, clickCount]
                } else {
                    0
                };
                clicks >= 2
            }
        };
        (row >= 0).then_some((row as usize, double, false))
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
    } else if target.id.starts_with("std") || target.id.starts_with("cus") {
        // A box of the color pop-out: whether this press was the second of
        // a double-click.
        let double = unsafe {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let event: *mut AnyObject = msg_send![app, currentEvent];
            !event.is_null() && {
                let clicks: isize = msg_send![event, clickCount];
                clicks >= 2
            }
        };
        Some((0, double, false))
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

/// Native conversion is deliberately conservative. A custom canvas item may
/// be interactive or a live preview, so it remains in the old renderer.
pub fn supported(dialog: &Dialog) -> bool {
    is_alert(dialog) || !dialog.needs_canvas()
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
    /// A color button; the color its picture was last drawn in.
    Swatch,
}

struct Control {
    object: Retained<AnyObject>,
    _container: Option<Retained<AnyObject>>,
    id: &'static str,
    kind: ControlKind,
    options: Option<Vec<String>>,
    /// For a color button, the color its picture shows now.
    swatch: std::cell::Cell<u32>,
}

pub struct NativeDialog {
    panel: Retained<AnyObject>,
    // An NSAlert owns its window and buttons. Ordinary dialogs do not need
    // a separate owner because their NSPanel owns every control directly.
    _alert: Option<Retained<AnyObject>>,
    parent: *mut AnyObject,
    controls: Vec<Control>,
    /// The panel popped out beside a control, while there is one.
    popover: Option<Popover>,
    depth: usize,
    title: String,
    modeless: bool,
    alert_message: Option<String>,
}

impl NativeDialog {
    pub fn panel_id(&self) -> usize {
        Retained::as_ptr(&self.panel) as usize
    }

    pub fn activate(&self) {
        let panel = Retained::as_ptr(&self.panel) as *mut AnyObject;
        unsafe {
            let _: () = msg_send![panel, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
        }
    }

    pub fn activate_find(&self) {
        if self.title != "Find" {
            return;
        }
        let panel = Retained::as_ptr(&self.panel) as *mut AnyObject;
        unsafe {
            let _: () = msg_send![panel, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
            if let Some(control) = self.controls.iter().find(|c| c.id == "find") {
                let _: bool =
                    msg_send![panel, makeFirstResponder: Retained::as_ptr(&control.object)];
            }
        }
    }

    /// Expand Find in the same native window, keeping its top-left corner.
    pub fn resize_find(&mut self, depth: usize, dialog: &Dialog) -> bool {
        if self.depth != depth
            || !self.modeless
            || self.title != "Find"
            || dialog.title != "Find"
            || !dialog.modeless
            || !self.controls.iter().any(|c| c.id == "mode")
            || !dialog.items.iter().any(|i| i.id == "mode")
        {
            return false;
        }
        let panel = Retained::as_ptr(&self.panel) as *mut AnyObject;
        unsafe {
            let old: NSRect = msg_send![panel, frame];
            let content_rect = NSRect::new(
                NSPoint::ZERO,
                NSSize::new(dialog.width as f64 * SCALE, dialog.height as f64 * SCALE),
            );
            let mut frame: NSRect = msg_send![panel, frameRectForContentRect: content_rect];
            frame.origin = NSPoint::new(
                old.origin.x,
                old.origin.y + old.size.height - frame.size.height,
            );
            let _: bool = msg_send![panel, makeFirstResponder: std::ptr::null::<AnyObject>()];
            if let Ok(mut targets) = actions().lock() {
                targets.retain(|a| a.panel != panel as usize);
            }
            for control in &self.controls {
                let view = control._container.as_ref().unwrap_or(&control.object);
                let _: () = msg_send![&**view, removeFromSuperview];
            }
            self.controls.clear();
            let _: () = msg_send![panel, setFrame: frame display: true];
            let content: *mut AnyObject = msg_send![panel, contentView];
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let delegate: *mut AnyObject = msg_send![app, delegate];
            for item in &dialog.items {
                let rect = native_rect(item.rect, dialog.height as f64 * SCALE);
                let Some((object, container, kind)) =
                    make_control(item, rect, panel, delegate, content)
                else {
                    return false;
                };
                self.controls.push(Control {
                    object,
                    _container: container,
                    id: item.id,
                    kind,
                    options: match &item.kind {
                        Kind::Popup { options, .. } => Some(options.clone()),
                        _ => None,
                    },
                    swatch: std::cell::Cell::new(0),
                });
            }
            let _: () = msg_send![panel, recalculateKeyViewLoop];
            if let Some(control) = self
                .controls
                .iter()
                .find(|c| Some(c.id) == dialog.focused_id())
            {
                let _: bool =
                    msg_send![panel, makeFirstResponder: Retained::as_ptr(&control.object)];
            }
        }
        self.sync_from_model(dialog);
        true
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

    /// Where this window's top-left corner is, when `dialog` is the same
    /// kind of window rebuilt in its place (Preferences changing panels).
    /// The new window opens there instead of back in the middle.
    pub fn top_left_for(&self, dialog: &Dialog) -> Option<(f64, f64)> {
        if self.alert_message.is_some()
            || is_alert(dialog)
            || !self.modeless
            || !dialog.modeless
            || self.title != native_title(dialog)
        {
            return None;
        }
        let panel = Retained::as_ptr(&self.panel) as *mut AnyObject;
        let frame: NSRect = unsafe { msg_send![panel, frame] };
        Some((frame.origin.x, frame.origin.y + frame.size.height))
    }

    pub fn show(
        window: &Window,
        depth: usize,
        dialog: &Dialog,
        top_left: Option<(f64, f64)>,
    ) -> Option<Self> {
        let handle = window.window_handle().ok()?.as_raw();
        let RawWindowHandle::AppKit(handle) = handle else {
            return None;
        };
        let view = handle.ns_view.as_ptr().cast::<AnyObject>();
        let parent: *mut AnyObject = unsafe { msg_send![view, window] };
        if parent.is_null() {
            return None;
        }
        Self::show_with_parent(parent, depth, dialog, top_left)
    }

    /// Application settings do not need a document window as their parent.
    pub fn show_standalone(
        depth: usize,
        dialog: &Dialog,
        top_left: Option<(f64, f64)>,
    ) -> Option<Self> {
        Self::show_with_parent(std::ptr::null_mut(), depth, dialog, top_left)
    }

    fn show_with_parent(
        parent: *mut AnyObject,
        depth: usize,
        dialog: &Dialog,
        top_left: Option<(f64, f64)>,
    ) -> Option<Self> {
        if !supported(dialog) {
            return None;
        }

        if is_alert(dialog) {
            return Self::show_alert(parent, depth, dialog);
        }

        let width = dialog.width as f64 * SCALE;
        let height = dialog.height as f64 * SCALE;
        let rect = NSRect::new(NSPoint::ZERO, NSSize::new(width, height));
        let style: usize = 1 | 2; // titled + closable
        let panel_class = dialog_panel_class();
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
                swatch: std::cell::Cell::new(match &item.kind {
                    Kind::Swatch { rgb, .. } => *rgb,
                    _ => 0,
                }),
            });
        }
        let mut result = Self {
            panel,
            _alert: None,
            parent,
            controls,
            popover: None,
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
            if !parent.is_null()
                && dialog.title == "Find"
                && dialog.items.iter().any(|i| i.id == "mode")
            {
                let parent_frame: NSRect = msg_send![parent, frame];
                let frame: NSRect = msg_send![panel_ptr, frame];
                let origin = NSPoint::new(
                    parent_frame.origin.x + (parent_frame.size.width - frame.size.width) / 2.0,
                    parent_frame.origin.y + (parent_frame.size.height - frame.size.height) / 2.0,
                );
                let _: () = msg_send![panel_ptr, setFrameOrigin: origin];
            } else if let Some((x, y)) = top_left {
                let _: () = msg_send![panel_ptr, setFrameTopLeftPoint: NSPoint::new(x, y)];
            } else {
                let _: () = msg_send![panel_ptr, center];
            }
            if dialog.modeless {
                let _: () =
                    msg_send![panel_ptr, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
                let _: () = msg_send![panel_ptr, makeKeyWindow];
            } else {
                // Keep a modal panel attached to its document while letting
                // winit's event loop continue to drive both windows.
                if !parent.is_null() {
                    let _: () = msg_send![parent, addChildWindow: panel_ptr ordered: 1isize];
                }
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
                swatch: std::cell::Cell::new(0),
            });
        }

        unsafe {
            // NSAlert keeps placeholder controls until it performs its final
            // layout pass. Without this, a manually presented alert can show
            // its normally hidden help, suppression and spare button views.
            let _: () = msg_send![alert_ptr, layout];
            let _: () = msg_send![panel_ptr, setDelegate: delegate];
            let _: () = msg_send![panel_ptr, setReleasedWhenClosed: false];
            if !parent.is_null() {
                let _: () = msg_send![parent, addChildWindow: panel_ptr ordered: 1isize];
            }
            let _: () = msg_send![panel_ptr, center];
            let _: () = msg_send![panel_ptr, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
        }
        Some(Self {
            panel,
            _alert: Some(alert),
            parent,
            controls,
            popover: None,
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
                        let value = rust_string(s);
                        let editor: *mut AnyObject = msg_send![ptr, currentEditor];
                        if let Some(item) = dialog.items.iter_mut().find(|i| i.id == control.id)
                            && let Kind::Field { text, sel } = &mut item.kind
                        {
                            if !editor.is_null() {
                                let range: NSRange = msg_send![editor, selectedRange];
                                *sel = (
                                    byte_offset(&value, range.location),
                                    byte_offset(
                                        &value,
                                        range.location.saturating_add(range.length),
                                    ),
                                );
                            } else if *text != value {
                                *sel = (value.len(), value.len());
                            }
                            *text = value;
                        }
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
                    | ControlKind::Swatch => {}
                }
            }
        }
        if let Some((family, bold, italic)) = choice {
            dialog.set_selected(changed, family);
            dialog.set_font_variant(bold, italic);
        }
    }

    /// Keep the document available for application activation while the
    /// app event handler blocks editing behind this modal panel.
    pub fn raise_modal(&self) -> bool {
        if self.modeless {
            return false;
        }
        unsafe {
            let _: () =
                msg_send![&*self.panel, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
        }
        true
    }

    pub fn sync_from_model(&mut self, dialog: &Dialog) {
        if self.alert_message.is_some() {
            return;
        }
        for (control, item) in self.controls.iter().zip(&dialog.items) {
            let ptr = Retained::as_ptr(&control.object) as *mut AnyObject;
            unsafe {
                if !matches!(control.kind, ControlKind::Group) {
                    let _: () = msg_send![ptr, setEnabled: item.enabled];
                }
                match &item.kind {
                    Kind::Field { text, sel } => {
                        set_field_enabled(ptr, item.enabled);
                        let current: *mut AnyObject = msg_send![ptr, stringValue];
                        if rust_string(current) != *text {
                            let value = NSString::from_str(text);
                            let _: () = msg_send![ptr, setStringValue: &*value];
                        }
                        let editor: *mut AnyObject = msg_send![ptr, currentEditor];
                        if !editor.is_null() {
                            let a = text[..sel.0].encode_utf16().count();
                            let b = text[..sel.1].encode_utf16().count();
                            let range = NSRange::new(a.min(b), a.abs_diff(b));
                            let _: () = msg_send![editor, setSelectedRange: range];
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
                        if *selected != usize::MAX
                            && !matches!(control.kind, ControlKind::Popup(true))
                        {
                            let _: () = msg_send![ptr, selectItemAtIndex: *selected as isize];
                        }
                    }
                    Kind::Text | Kind::Note => {
                        set_static_label(ptr, &item.label, item.struck);
                    }
                    Kind::Button { .. } => {
                        let value = NSString::from_str(&item.label);
                        let _: () = msg_send![ptr, setTitle: &*value];
                    }
                    Kind::Swatch { rgb, button: true } if control.swatch.get() != *rgb => {
                        let frame: NSRect = msg_send![ptr, frame];
                        set_swatch_picture(ptr, *rgb, frame.size);
                        control.swatch.set(*rgb);
                    }
                    Kind::Group => {
                        let value = NSString::from_str(&item.label);
                        let _: () = msg_send![ptr, setTitle: &*value];
                    }
                    Kind::List { rows, state, .. }
                        if matches!(control.kind, ControlKind::Table) =>
                    {
                        let changed = tables().lock().is_ok_and(|mut tables| {
                            let Some(data) =
                                tables.iter_mut().find(|data| data.table == ptr as usize)
                            else {
                                return false;
                            };
                            if data.rows == *rows {
                                return false;
                            }
                            data.rows = rows.clone();
                            true
                        });
                        if changed {
                            let _: () = msg_send![ptr, reloadData];
                        }
                        if !rows.is_empty() {
                            let row = state.selected.min(rows.len() - 1);
                            let selected: isize = msg_send![ptr, selectedRow];
                            if selected != row as isize {
                                let set: *mut AnyObject =
                                    msg_send![class!(NSIndexSet), indexSetWithIndex: row];
                                let _: () = msg_send![ptr, selectRowIndexes: set byExtendingSelection: false];
                                let _: () = msg_send![ptr, scrollRowToVisible: row as isize];
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        self.sync_popover(dialog);
    }

    /// Open, change or close the panel popped out beside a control to match
    /// what the model says.
    fn sync_popover(&mut self, dialog: &Dialog) {
        let Some(spec) = &dialog.popout else {
            self.popover = None;
            return;
        };
        if self.popover.as_ref().is_some_and(|p| p.spec == *spec) {
            return;
        }
        let panel = Retained::as_ptr(&self.panel) as *mut AnyObject;
        let Some(anchor) = self.controls.iter().find(|c| c.id == spec.anchor) else {
            return;
        };
        let anchor = Retained::as_ptr(&anchor.object) as *mut AnyObject;
        unsafe {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let delegate: *mut AnyObject = msg_send![app, delegate];
            let Some((view, controls)) = popover_view(spec, panel, delegate) else {
                return;
            };
            let vc: *mut AnyObject = msg_send![class!(NSViewController), new];
            if vc.is_null() {
                return;
            }
            let _: () = msg_send![vc, setView: Retained::as_ptr(&view) as *mut AnyObject];
            if let Some(p) = &mut self.popover {
                // Already open: swap what is in it, as a box is chosen.
                let _: () = msg_send![&*p.object, setContentViewController: vc];
                if let Ok(mut a) = actions().lock() {
                    a.retain(|a| !p.controls.contains(&a.control));
                }
                p.controls = controls;
                p.spec = spec.clone();
            } else {
                let raw: *mut AnyObject = msg_send![class!(NSPopover), new];
                if let Some(object) = Retained::from_raw(raw) {
                    // Transient: a press outside puts it away.
                    let _: () = msg_send![&*object, setBehavior: 1isize];
                    let _: () = msg_send![&*object, setContentViewController: vc];
                    let bounds: NSRect = msg_send![anchor, bounds];
                    // Below the control. Buttons are flipped views, where
                    // the lower edge is MaxY (3) rather than MinY (1).
                    let flipped: bool = msg_send![anchor, isFlipped];
                    let below: usize = if flipped { 3 } else { 1 };
                    let _: () = msg_send![
                        &*object, showRelativeToRect: bounds ofView: anchor preferredEdge: below
                    ];
                    self.popover = Some(Popover {
                        object,
                        spec: spec.clone(),
                        controls,
                        opened: std::time::Instant::now(),
                    });
                }
            }
            let _: () = msg_send![vc, release];
        }
    }

    /// Whether the system put the popped-out panel away by itself (a press
    /// outside it), so the model should forget it.
    pub fn popover_dismissed(&mut self) -> bool {
        let Some(p) = &self.popover else {
            return false;
        };
        if p.opened.elapsed() < std::time::Duration::from_millis(300) {
            return false;
        }
        let shown: bool = unsafe { msg_send![&*p.object, isShown] };
        if shown {
            return false;
        }
        self.popover = None;
        true
    }

    pub fn outcome(dialog: &Dialog, id: &'static str) -> Outcome {
        match id {
            "ok" => Outcome::Ok,
            "cancel" => Outcome::Cancel,
            "apply" => Outcome::Apply,
            id if dialog.items.iter().any(|item| {
                item.id == id
                    && matches!(
                        item.kind,
                        Kind::Button { .. } | Kind::Swatch { button: true, .. }
                    )
            }) =>
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
            (ControlKind::Table, Kind::List { .. }) => true,
            (ControlKind::Swatch, Kind::Swatch { button: true, .. }) => true,
            (ControlKind::Radio(a), Kind::Radio { group: b, .. }) => a == b,
            (
                ControlKind::Popup(menu_button),
                Kind::Popup {
                    options, selected, ..
                },
            ) => {
                // A menu button keeps its kind whatever the model holds: it
                // records the last item chosen, which must not rebuild the
                // window (and reselect the text being typed) on the next edit.
                (*menu_button || *selected != usize::MAX)
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
        unsafe {
            let _: () = msg_send![panel, setDelegate: std::ptr::null::<AnyObject>()];
            if self.modeless {
                let _: () = msg_send![panel, orderOut: std::ptr::null::<AnyObject>()];
            } else {
                if !self.parent.is_null() {
                    let _: () = msg_send![self.parent, removeChildWindow: panel];
                }
                let _: () = msg_send![panel, orderOut: std::ptr::null::<AnyObject>()];
            }
            if !self.parent.is_null() {
                let _: () =
                    msg_send![self.parent, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
            }
        }
    }
}

/// Give a menu-bar editing command to the text field being edited in a
/// dialog, if there is one. The menu bar's Cut, Copy, Paste, Select All and
/// Undo otherwise act on the document behind a Find or Replace window.
/// Returns whether a field took the command.
pub fn edit_in_field(cmd: crate::commands::Cmd) -> bool {
    use crate::commands::Cmd;
    let action = match cmd {
        Cmd::Cut => sel!(cut:),
        Cmd::Copy => sel!(copy:),
        Cmd::Paste => sel!(paste:),
        Cmd::SelectAll => sel!(selectAll:),
        Cmd::Undo => sel!(undo:),
        _ => return false,
    };
    // SAFETY: the menu callback runs on the main thread; these are plain
    // AppKit queries on live objects, and sendAction walks the responder
    // chain from the key window's first responder.
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let key: *mut AnyObject = msg_send![app, keyWindow];
        if key.is_null() {
            return false;
        }
        let responder: *mut AnyObject = msg_send![key, firstResponder];
        if responder.is_null() {
            return false;
        }
        // A text field being edited has the field editor, an NSText, as its
        // first responder; the document window's is an ordinary view.
        let editing: bool = msg_send![responder, isKindOfClass: class!(NSText)];
        if !editing {
            return false;
        }
        msg_send![
            app,
            sendAction: action,
            to: std::ptr::null::<AnyObject>(),
            from: std::ptr::null::<AnyObject>()
        ]
    }
}

unsafe fn set_field_enabled(field: *mut AnyObject, enabled: bool) {
    unsafe {
        let _: () = msg_send![field, setEnabled: enabled];
        let _: () = msg_send![field, setEditable: enabled];
        let _: () = msg_send![field, setSelectable: enabled];
        let _: () = msg_send![field, setDrawsBackground: true];
        let background: *mut AnyObject = if enabled {
            msg_send![class!(NSColor), textBackgroundColor]
        } else {
            msg_send![class!(NSColor), controlBackgroundColor]
        };
        let foreground: *mut AnyObject = if enabled {
            msg_send![class!(NSColor), controlTextColor]
        } else {
            msg_send![class!(NSColor), disabledControlTextColor]
        };
        let _: () = msg_send![field, setBackgroundColor: background];
        let _: () = msg_send![field, setTextColor: foreground];
    }
}

fn native_rect(r: crate::canvas::Rect, content_height: f64) -> NSRect {
    let x = r.x as f64 * SCALE;
    let h = r.h as f64 * SCALE;
    let y = content_height - (r.y + r.h) as f64 * SCALE;
    NSRect::new(NSPoint::new(x, y), NSSize::new(r.w as f64 * SCALE, h))
}

// Keep overridden formatting legible rather than hiding its saved values.
unsafe fn set_static_label(field: *mut AnyObject, text: &str, struck: bool) {
    unsafe {
        let string = NSString::from_str(text);
        if !struck {
            let _: () = msg_send![field, setStringValue: &*string];
            return;
        }
        let value: *mut AnyObject = msg_send![class!(NSMutableAttributedString), alloc];
        let value: *mut AnyObject = msg_send![value, initWithString: &*string];
        let range = NSRange::new(0, text.encode_utf16().count());
        let font: *mut AnyObject = msg_send![field, font];
        let _: () =
            msg_send![value, addAttribute: &*NSString::from_str("NSFont") value: font range: range];
        let strike: *mut AnyObject = msg_send![class!(NSNumber), numberWithInteger: 1isize];
        let _: () = msg_send![value, addAttribute: &*NSString::from_str("NSStrikethrough") value: strike range: range];
        let _: () = msg_send![field, setAttributedStringValue: value];
        let _: () = msg_send![value, release];
    }
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
        return None;
    }
    if let Kind::List { double, .. } = &item.kind {
        return unsafe {
            make_table(
                item.id,
                frame,
                panel,
                delegate,
                content,
                false,
                double.is_some(),
            )
        };
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
        Kind::Swatch { button: true, .. } => (class!(NSButton), ControlKind::Swatch, Some(0)),
        Kind::Custom | Kind::Swatch { .. } | Kind::Slider { .. } | Kind::List { .. } => {
            unreachable!()
        }
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
                set_field_enabled(ptr, item.enabled);
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
                set_static_label(ptr, &item.label, item.struck);
            }
            Kind::Group => {
                let _: () = msg_send![ptr, setTitle: &*label];
            }
            Kind::Swatch { rgb, .. } => {
                let _: () = msg_send![ptr, setBordered: false];
                set_swatch_picture(ptr, *rgb, frame.size);
            }
            Kind::Custom | Kind::Slider { .. } | Kind::List { .. } => unreachable!(),
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

/// Draw a color button's picture: the color with a thin gray frame, and
/// a blue one inside it when `chosen`.
unsafe fn set_swatch_picture_marked(button: *mut AnyObject, rgb: u32, size: NSSize, chosen: bool) {
    let part = |shift: u32| ((rgb >> shift) & 0xFF) as f64 / 255.0;
    unsafe {
        let image: *mut AnyObject = msg_send![class!(NSImage), alloc];
        let image: *mut AnyObject = msg_send![image, initWithSize: size];
        if image.is_null() {
            return;
        }
        let _: () = msg_send![image, lockFocus];
        let fill = NSRect::new(NSPoint::ZERO, size);
        let color: *mut AnyObject = msg_send![
            class!(NSColor), colorWithSRGBRed: part(16) green: part(8) blue: part(0) alpha: 1.0f64
        ];
        let _: () = msg_send![color, set];
        let _: () = msg_send![class!(NSBezierPath), fillRect: fill];
        let (frame_color, inset): (*mut AnyObject, f64) = if chosen {
            (msg_send![class!(NSColor), systemBlueColor], 1.0)
        } else {
            (msg_send![class!(NSColor), grayColor], 0.5)
        };
        let _: () = msg_send![frame_color, set];
        let line = NSRect::new(
            NSPoint::new(inset, inset),
            NSSize::new(size.width - 2.0 * inset, size.height - 2.0 * inset),
        );
        let _: () = msg_send![class!(NSBezierPath), strokeRect: line];
        let _: () = msg_send![image, unlockFocus];
        let _: () = msg_send![button, setImage: image];
        let _: () = msg_send![button, setImagePosition: 1usize];
        let _: () = msg_send![button, setImageScaling: 1usize];
        let _: () = msg_send![image, release];
    }
}

unsafe fn set_swatch_picture(button: *mut AnyObject, rgb: u32, size: NSSize) {
    unsafe { set_swatch_picture_marked(button, rgb, size, false) }
}

/// A box of a pop-out, or its button, as it was built.
struct Popover {
    object: Retained<AnyObject>,
    /// What it was built from, to see when the model has changed it.
    spec: crate::dialog::Popout,
    /// Its controls, to take their actions away.
    controls: Vec<usize>,
    opened: std::time::Instant,
}

impl Drop for Popover {
    fn drop(&mut self) {
        if let Ok(mut a) = actions().lock() {
            a.retain(|a| !self.controls.contains(&a.control));
        }
        unsafe {
            let _: () = msg_send![&*self.object, close];
        }
    }
}

/// The view holding a pop-out's boxes, its actions going to `panel`.
unsafe fn popover_view(
    spec: &crate::dialog::Popout,
    panel: *mut AnyObject,
    delegate: *mut AnyObject,
) -> Option<(Retained<AnyObject>, Vec<usize>)> {
    let size = NSSize::new(spec.size.0 as f64 * SCALE, spec.size.1 as f64 * SCALE);
    let raw: *mut AnyObject = unsafe { msg_send![class!(NSView), alloc] };
    let raw: *mut AnyObject =
        unsafe { msg_send![raw, initWithFrame: NSRect::new(NSPoint::ZERO, size)] };
    let view = unsafe { Retained::from_raw(raw) }?;
    let view_ptr = Retained::as_ptr(&view) as *mut AnyObject;
    let mut controls = Vec::new();
    for (index, item) in spec.items.iter().enumerate() {
        let frame = NSRect::new(
            NSPoint::new(
                item.rect.x as f64 * SCALE,
                size.height - (item.rect.y + item.rect.h) as f64 * SCALE,
            ),
            NSSize::new(item.rect.w as f64 * SCALE, item.rect.h as f64 * SCALE),
        );
        let raw: *mut AnyObject = unsafe { msg_send![class!(NSButton), alloc] };
        let raw: *mut AnyObject = unsafe { msg_send![raw, initWithFrame: frame] };
        if raw.is_null() {
            continue;
        }
        let button = unsafe { Retained::from_raw(raw) }?;
        let ptr = Retained::as_ptr(&button) as *mut AnyObject;
        unsafe {
            let _: () = msg_send![ptr, setButtonType: 0usize];
            if item.label.is_empty() {
                let _: () = msg_send![ptr, setBordered: false];
                let rgb = item.rgb.unwrap_or(0xFFFFFF);
                if item.rgb.is_some() {
                    set_swatch_picture_marked(ptr, rgb, frame.size, item.selected);
                } else {
                    // An empty custom box: a pale outline.
                    set_swatch_picture_marked(ptr, 0xF4F4F4, frame.size, false);
                }
                if let Some(rgb) = item.rgb {
                    let hex = format!("#{:06X}", rgb & 0xFF_FFFF);
                    let tip = NSString::from_str(item.tip.as_deref().unwrap_or(&hex));
                    let _: () = msg_send![ptr, setToolTip: &*tip];
                }
            } else {
                let title = NSString::from_str(item.label);
                let _: () = msg_send![ptr, setTitle: &*title];
                let _: () = msg_send![ptr, setBezelStyle: 1usize];
                let _: () = msg_send![ptr, setEnabled: item.enabled];
                if let Some(tip) = &item.tip {
                    let tip = NSString::from_str(tip);
                    let _: () = msg_send![ptr, setToolTip: &*tip];
                }
            }
            let _: () = msg_send![ptr, setTarget: delegate];
            let _: () = msg_send![ptr, setAction: sel!(quillDialogAction:)];
            let _: () = msg_send![view_ptr, addSubview: ptr];
        }
        actions().lock().ok()?.push(ActionTarget {
            control: ptr as usize,
            panel: panel as usize,
            id: item.id,
            choice: Some((index, false, false)),
            default: false,
        });
        controls.push(ptr as usize);
    }
    Some((view, controls))
}

/// A one-column table in a scroll view. `double` makes a double-click on a
/// row send the action again, with the click count to tell it apart.
unsafe fn make_table(
    id: &'static str,
    frame: NSRect,
    panel: *mut AnyObject,
    delegate: *mut AnyObject,
    content: *mut AnyObject,
    allow_empty: bool,
    double: bool,
) -> Option<(
    Retained<AnyObject>,
    Option<Retained<AnyObject>>,
    ControlKind,
)> {
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
    let identifier = NSString::from_str(id);
    let column_raw: *mut AnyObject =
        unsafe { msg_send![column_raw, initWithIdentifier: &*identifier] };
    let column = unsafe { Retained::from_raw(column_raw) }?;
    let column_ptr = Retained::as_ptr(&column) as *mut AnyObject;
    unsafe {
        let _: () = msg_send![column_ptr, setWidth: frame.size.width];
        let _: () = msg_send![table_ptr, addTableColumn: column_ptr];
        let _: () = msg_send![table_ptr, setHeaderView: std::ptr::null::<AnyObject>()];
        let _: () = msg_send![table_ptr, setAllowsMultipleSelection: false];
        let _: () = msg_send![table_ptr, setAllowsEmptySelection: allow_empty];
        if double {
            let _: () = msg_send![table_ptr, setTarget: delegate];
            let _: () = msg_send![table_ptr, setDoubleAction: sel!(quillDialogAction:)];
        }
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
        id,
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
    fn list_and_keys_ids_do_not_send_table_messages_to_other_objects() {
        // NSObject is safe to construct off the main thread and, like a popup,
        // does not answer selectedRow. The control ID must not choose that API.
        let object: Retained<AnyObject> =
            unsafe { Retained::from_raw(msg_send![class!(NSObject), new]).unwrap() };
        let sender = Retained::as_ptr(&object) as *mut AnyObject;
        for id in ["list", "keys"] {
            actions().lock().unwrap().push(ActionTarget {
                control: sender as usize,
                panel: 0,
                id,
                choice: None,
                default: false,
            });
            dispatch(sender);
            actions()
                .lock()
                .unwrap()
                .retain(|target| target.control != sender as usize);
        }
    }

    #[test]
    fn special_codes_replace_only_the_native_field_selection() {
        for (start, length, expected) in [(4, 0, "a\u{1f600}b^t"), (1, 2, "a^tb"), (0, 4, "^t")] {
            let mut d = Dialog::new("Find", 300.0, 100.0);
            d.field("find", "a\u{1f600}b", 0.0, 0.0, 100.0);
            if let Kind::Field { sel, .. } = &mut d.items[0].kind {
                *sel = (
                    byte_offset("a\u{1f600}b", start),
                    byte_offset("a\u{1f600}b", start + length),
                );
            }
            d.insert_in_field("find", "^t");
            assert_eq!(d.field_text("find"), expected);
        }
    }

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
    }

    #[test]
    fn the_character_dialog_with_its_color_button_is_native() {
        let view = crate::docview::DocView::new(quill_core::Document::new(), "t".into());
        let (dialog, _) = crate::format_dialogs::build_character_with_button(&view);
        assert!(supported(&dialog));
        let mut swatch = Dialog::new("Colors", 200.0, 100.0);
        swatch.swatch("preview", 10.0, 10.0, 40.0, 40.0, 0);
        assert!(!supported(&swatch));
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
