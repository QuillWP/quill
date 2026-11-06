//! Opening documents from the Finder (double-click, drag onto the icon,
//! Open With): macOS tells the app delegate `application:openURLs:`,
//! which winit does not pass on, so Quill adds that method to winit's
//! delegate class and sends the paths to the app as an event.
//!
//! Quitting from outside Quill (the Dock's Quit, logging out, shutting
//! down, a quit Apple Event) asks the delegate
//! `applicationShouldTerminate:`, which winit leaves to AppKit, so the app
//! ended without Quill's own Quit: unsaved documents were not offered for
//! saving and the next start took it for a crash. Quill adds that method
//! too.

use crate::app::UserEvent;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{msg_send, sel};
use std::ffi::{CStr, c_char};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use winit::event_loop::EventLoopProxy;

static PROXY: OnceLock<Mutex<EventLoopProxy<UserEvent>>> = OnceLock::new();
/// Paths the Finder sent that the app has not taken yet. When Quill is
/// started by opening a document, AppKit sends them before the app's first
/// `resumed`, which takes them from here so it knows not to offer last
/// session's documents.
static WAITING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Some window holds work that is not saved, or a recovered document:
/// kept current by the app each time its event loop goes idle.
static UNSAVED: AtomicBool = AtomicBool::new(false);

pub fn set_unsaved(unsaved: bool) {
    UNSAVED.store(unsaved, Ordering::Relaxed);
}

/// The paths the Finder has sent since last asked.
pub fn take() -> Vec<PathBuf> {
    WAITING
        .lock()
        .map(|mut w| std::mem::take(&mut *w))
        .unwrap_or_default()
}

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
    if paths.is_empty() {
        return;
    }
    if let Ok(mut w) = WAITING.lock() {
        w.extend(paths);
    }
    if let Some(p) = PROXY.get()
        && let Ok(p) = p.lock()
    {
        let _ = p.send_event(UserEvent::OpenFiles);
    }
}

/// `-[WinitApplicationDelegate applicationShouldTerminate:]`. With nothing
/// unsaved Quill can end now, as a clean quit (the settings already hold
/// the open documents). Otherwise it cancels and runs its own Quit, which
/// asks about each unsaved document; a logout waits for that, as it does
/// for any Mac app with unsaved work.
extern "C" fn should_terminate(_this: *mut AnyObject, _cmd: Sel, _app: *mut AnyObject) -> usize {
    const NOW: usize = 1;
    const CANCEL: usize = 0;
    if !UNSAVED.load(Ordering::Relaxed) {
        crate::settings::update(|s| s.quit_cleanly = true);
        return NOW;
    }
    if let Some(p) = PROXY.get()
        && let Ok(p) = p.lock()
    {
        let _ = p.send_event(UserEvent::Quit);
    }
    CANCEL
}

/// Teach winit's application delegate to open documents and to quit. Call after the
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
    let f: extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize = should_terminate;
    // SAFETY: as above; "Q@:@" returns an NSApplicationTerminateReply (an
    // NSUInteger) and takes self, _cmd and the application.
    unsafe {
        let imp: unsafe extern "C" fn() = std::mem::transmute(f);
        objc2::ffi::class_addMethod(
            class as *const AnyClass as *mut _,
            sel!(applicationShouldTerminate:).as_ptr(),
            Some(imp),
            c"Q@:@".as_ptr(),
        );
    }
}
