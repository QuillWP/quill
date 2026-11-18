//! Text the Mac sends without a key press: a character chosen in Emoji &
//! Symbols, dictation, and the like. AppKit hands it to the view's
//! `insertText:replacementRange:`, which winit only passes on while an
//! input method is composing, so everything else is lost. Quill wraps that
//! method: text arriving during a key press goes to winit as before, and
//! any other text comes to the app as an event and is typed in.

use crate::app::UserEvent;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{class, msg_send, sel};
use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::{CStr, c_char};
use std::sync::{Mutex, OnceLock};
use winit::event_loop::EventLoopProxy;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowId};

static PROXY: OnceLock<Mutex<EventLoopProxy<UserEvent>>> = OnceLock::new();
/// The WinitView that received text and the Quill window it belongs to.
/// Dictation can finish after focus has moved, so guessing from the window
/// in front can put private speech into the wrong document.
static VIEWS: OnceLock<Mutex<HashMap<usize, WindowId>>> = OnceLock::new();
/// winit's own `keyDown:` and `insertText:replacementRange:`.
static KEY_DOWN: OnceLock<Imp> = OnceLock::new();
static INSERT_TEXT: OnceLock<Imp> = OnceLock::new();
static SET_MARKED: OnceLock<Imp> = OnceLock::new();
static SELECTED: OnceLock<Imp> = OnceLock::new();
static SUBSTRING: OnceLock<Imp> = OnceLock::new();

thread_local! {
    /// A key press is being handled, so inserted text is its typing.
    static IN_KEY_DOWN: Cell<bool> = const { Cell::new(false) };
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NSRange {
    location: usize,
    length: usize,
}

type KeyDownFn = extern "C" fn(*mut AnyObject, Sel, *mut AnyObject);
type InsertTextFn = extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, NSRange);
type SetMarkedFn = extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, NSRange, NSRange);
type SelectedFn = extern "C" fn(*mut AnyObject, Sel) -> NSRange;
type SubstringFn = extern "C" fn(*mut AnyObject, Sel, NSRange, *mut NSRange) -> *mut AnyObject;

/// Dictation asks where the insertion point is before it sends any text.
/// winit answers "no selection", which Dictation takes to mean the view
/// cannot take text, so it listens and then drops what it hears. The text
/// itself comes through `insertText:` and the marked text, so the insertion
/// point is reported as the start, with nothing around it.
extern "C" fn selected_range(_this: *mut AnyObject, _cmd: Sel) -> NSRange {
    NSRange {
        location: 0,
        length: 0,
    }
}

extern "C" fn attributed_substring(
    _this: *mut AnyObject,
    _cmd: Sel,
    _range: NSRange,
    actual: *mut NSRange,
) -> *mut AnyObject {
    // SAFETY: `actual` is null or AppKit's own out-parameter, and the empty
    // string is handed back autoreleased as the method requires.
    unsafe {
        if !actual.is_null() {
            *actual = NSRange {
                location: 0,
                length: 0,
            };
        }
        let empty: *mut AnyObject = msg_send![class!(NSAttributedString), new];
        msg_send![empty, autorelease]
    }
}

extern "C" fn set_marked_text(
    this: *mut AnyObject,
    cmd: Sel,
    string: *mut AnyObject,
    selected: NSRange,
    replacement: NSRange,
) {
    crate::ev!(
        "text service: marked text {:?}",
        unsafe { plain_text(string) }.unwrap_or_default()
    );
    if let Some(&imp) = SET_MARKED.get() {
        // SAFETY: `imp` is winit's setMarkedText:selectedRange:replacementRange:.
        let original: SetMarkedFn = unsafe { std::mem::transmute(imp) };
        original(this, cmd, string, selected, replacement);
    }
}

extern "C" fn key_down(this: *mut AnyObject, cmd: Sel, event: *mut AnyObject) {
    let Some(&imp) = KEY_DOWN.get() else {
        return;
    };
    // SAFETY: `imp` is winit's keyDown:, which has this signature.
    let original: KeyDownFn = unsafe { std::mem::transmute(imp) };
    let was = IN_KEY_DOWN.replace(true);
    original(this, cmd, event);
    IN_KEY_DOWN.set(was);
}

extern "C" fn insert_text(this: *mut AnyObject, cmd: Sel, string: *mut AnyObject, range: NSRange) {
    if IN_KEY_DOWN.get() {
        if let Some(&imp) = INSERT_TEXT.get() {
            // SAFETY: `imp` is winit's insertText:replacementRange:.
            let original: InsertTextFn = unsafe { std::mem::transmute(imp) };
            original(this, cmd, string, range);
        }
        return;
    }
    let Some(text) = (unsafe { plain_text(string) }) else {
        return;
    };
    crate::ev!("text service: insert {text:?}");
    // SAFETY: `this` is winit's view, which answers both.
    unsafe {
        let marked: bool = msg_send![this, hasMarkedText];
        if marked {
            let _: () = msg_send![this, unmarkText];
        }
    }
    if !text.is_empty()
        && let Some(p) = PROXY.get()
        && let Ok(p) = p.lock()
    {
        let window = VIEWS
            .get()
            .and_then(|views| views.lock().ok())
            .and_then(|views| views.get(&(this as usize)).copied());
        if let Some(window) = window {
            let _ = p.send_event(UserEvent::InsertText { window, text });
        }
    }
}

/// The text of an NSString or NSAttributedString.
unsafe fn plain_text(string: *mut AnyObject) -> Option<String> {
    if string.is_null() {
        return None;
    }
    unsafe {
        let attributed: bool = msg_send![string, isKindOfClass: class!(NSAttributedString)];
        let s: *mut AnyObject = if attributed {
            msg_send![string, string]
        } else {
            string
        };
        if s.is_null() {
            return None;
        }
        let utf8: *const c_char = msg_send![s, UTF8String];
        (!utf8.is_null()).then(|| CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }
}

/// Register `window` and wrap winit's view methods. Winit registers its view
/// class with the first window, so the method replacement itself acts once.
pub fn install(proxy: &EventLoopProxy<UserEvent>, window: &Window) {
    if let Ok(handle) = window.window_handle()
        && let RawWindowHandle::AppKit(handle) = handle.as_raw()
    {
        let views = VIEWS.get_or_init(|| Mutex::new(HashMap::new()));
        if let Ok(mut views) = views.lock() {
            views.insert(handle.ns_view.as_ptr() as usize, window.id());
        }
    }
    if INSERT_TEXT.get().is_some() {
        return;
    }
    let Some(class) = AnyClass::get("WinitView") else {
        return;
    };
    let _ = PROXY.set(Mutex::new(proxy.clone()));
    let key: KeyDownFn = key_down;
    let insert: InsertTextFn = insert_text;
    let marked: SetMarkedFn = set_marked_text;
    let selected: SelectedFn = selected_range;
    let substring: SubstringFn = attributed_substring;
    // SAFETY: each replacement has the signature of the method it replaces,
    // and the originals are kept to be called from them.
    unsafe {
        let cls = class as *const AnyClass as *mut objc2::ffi::objc_class;
        for (sel, imp, original) in [
            (
                sel!(keyDown:),
                std::mem::transmute::<KeyDownFn, Imp>(key),
                &KEY_DOWN,
            ),
            (
                sel!(insertText:replacementRange:),
                std::mem::transmute::<InsertTextFn, Imp>(insert),
                &INSERT_TEXT,
            ),
            (
                sel!(setMarkedText:selectedRange:replacementRange:),
                std::mem::transmute::<SetMarkedFn, Imp>(marked),
                &SET_MARKED,
            ),
            (
                sel!(selectedRange),
                std::mem::transmute::<SelectedFn, Imp>(selected),
                &SELECTED,
            ),
            (
                sel!(attributedSubstringForProposedRange:actualRange:),
                std::mem::transmute::<SubstringFn, Imp>(substring),
                &SUBSTRING,
            ),
        ] {
            let method = objc2::ffi::class_getInstanceMethod(cls, sel.as_ptr());
            if method.is_null() {
                continue;
            }
            if let Some(old) = objc2::ffi::method_setImplementation(method as *mut _, Some(imp)) {
                let _ = original.set(old);
            }
        }
    }
}

/// Forget a window after it closes, including any pointer AppKit may reuse.
pub fn unregister(window: WindowId) {
    if let Some(views) = VIEWS.get()
        && let Ok(mut views) = views.lock()
    {
        views.retain(|_, id| *id != window);
    }
}
