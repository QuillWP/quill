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
use std::ffi::{CStr, c_char};
use std::sync::{Mutex, OnceLock};
use winit::event_loop::EventLoopProxy;

static PROXY: OnceLock<Mutex<EventLoopProxy<UserEvent>>> = OnceLock::new();
/// winit's own `keyDown:` and `insertText:replacementRange:`.
static KEY_DOWN: OnceLock<Imp> = OnceLock::new();
static INSERT_TEXT: OnceLock<Imp> = OnceLock::new();

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
        let _ = p.send_event(UserEvent::InsertText(text));
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

/// Wrap winit's view methods. winit registers its view class with the
/// first window, so this is called as each window opens; it acts once.
pub fn install(proxy: &EventLoopProxy<UserEvent>) {
    if INSERT_TEXT.get().is_some() {
        return;
    }
    let Some(class) = AnyClass::get("WinitView") else {
        return;
    };
    let _ = PROXY.set(Mutex::new(proxy.clone()));
    let key: KeyDownFn = key_down;
    let insert: InsertTextFn = insert_text;
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
