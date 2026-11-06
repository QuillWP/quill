//! Opening documents from the Finder (double-click, drag onto the icon,
//! Open With): macOS tells the app delegate `application:openURLs:`,
//! which winit does not pass on, so Quill adds that method to winit's
//! delegate class and sends the paths to the app as an event.

use crate::app::UserEvent;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{msg_send, sel};
use std::ffi::{CStr, c_char};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use winit::event_loop::EventLoopProxy;

static PROXY: OnceLock<Mutex<EventLoopProxy<UserEvent>>> = OnceLock::new();

/// `-[WinitApplicationDelegate application:openURLs:]`.
extern "C" fn open_urls(
    _this: *mut AnyObject,
    _cmd: Sel,
    _app: *mut AnyObject,
    urls: *mut AnyObject,
) {
    if urls.is_null() {
        return;
    }
    let mut paths = Vec::new();
    // SAFETY: AppKit passes an NSArray of NSURLs; each message is one
    // NSArray, NSURL or NSString answers.
    unsafe {
        let n: usize = msg_send![urls, count];
        for i in 0..n {
            let url: *mut AnyObject = msg_send![urls, objectAtIndex: i];
            if url.is_null() {
                continue;
            }
            let path: *mut AnyObject = msg_send![url, path];
            if path.is_null() {
                continue;
            }
            let utf8: *const c_char = msg_send![path, UTF8String];
            if !utf8.is_null() {
                paths.push(PathBuf::from(
                    CStr::from_ptr(utf8).to_string_lossy().into_owned(),
                ));
            }
        }
    }
    if !paths.is_empty()
        && let Some(p) = PROXY.get()
        && let Ok(p) = p.lock()
    {
        let _ = p.send_event(UserEvent::OpenFiles(paths));
    }
}

/// Teach winit's application delegate to open documents. Call after the
/// event loop is built (which registers the delegate class) and before it
/// runs.
pub fn install(proxy: EventLoopProxy<UserEvent>) {
    let _ = PROXY.set(Mutex::new(proxy));
    let Some(class) = AnyClass::get("WinitApplicationDelegate") else {
        return;
    };
    let f: extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject) = open_urls;
    // SAFETY: the function has the method's signature ("v@:@@": returns
    // nothing, takes self, _cmd and two objects); adding a method to a
    // registered class is what the runtime function is for.
    unsafe {
        let imp: unsafe extern "C" fn() = std::mem::transmute(f);
        objc2::ffi::class_addMethod(
            class as *const AnyClass as *mut _,
            sel!(application:openURLs:).as_ptr(),
            Some(imp),
            c"v@:@@".as_ptr(),
        );
    }
}
