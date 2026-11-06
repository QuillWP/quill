//! The Mac's Licenses window and the credits in its About panel.
//!
//! The standard About panel shows a short summary of Quill's licensing with a
//! link; the link opens the Licenses window, a plain scrollable, searchable
//! text of Quill's own licenses with a link to the notices file for everything
//! else (see `licenses`). Both draw on files in the app bundle, so nothing
//! needs the network.

use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{class, msg_send, sel};
use objc2_foundation::{NSPoint, NSRange, NSRect, NSSize, NSString};
use std::ffi::{CStr, c_char};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The Licenses window, once made (it is hidden, not freed, when closed).
static WINDOW: AtomicUsize = AtomicUsize::new(0);
/// The object that answers a click on the About panel's link.
static LINKS: AtomicUsize = AtomicUsize::new(0);

/// Close the Licenses window if it is the one in front. Command-W comes
/// through the menu as Close, which would otherwise close a document.
pub fn close_if_key() -> bool {
    let window = WINDOW.load(Ordering::Relaxed) as *mut AnyObject;
    if window.is_null() {
        return false;
    }
    // SAFETY: called on the main thread; the window is kept for good.
    unsafe {
        let key: bool = msg_send![window, isKeyWindow];
        if key {
            let _: () = msg_send![window, performClose: std::ptr::null::<AnyObject>()];
        }
        key
    }
}

/// Show the Licenses window, making it the first time.
pub fn show() {
    // SAFETY: called on the main thread; every message is one the AppKit
    // classes named answer, with the argument types they take.
    unsafe {
        let mut window = WINDOW.load(Ordering::Relaxed) as *mut AnyObject;
        if window.is_null() {
            window = make_window();
            WINDOW.store(window as usize, Ordering::Relaxed);
        }
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, activateIgnoringOtherApps: true];
        let _: () = msg_send![window, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
    }
}

fn make_window() -> *mut AnyObject {
    // SAFETY: called on the main thread with live AppKit objects, using
    // messages the classes named answer.
    unsafe {
        let rect = NSRect::new(NSPoint::ZERO, NSSize::new(720.0, 600.0));
        // Titled, closable, miniaturizable and resizable.
        let style: usize = 1 | 2 | 4 | 8;
        let window: *mut AnyObject = msg_send![class!(NSWindow), alloc];
        let window: *mut AnyObject = msg_send![
            window,
            initWithContentRect: rect
            styleMask: style
            backing: 2usize
            defer: false
        ];
        let title = NSString::from_str("Licenses");
        let _: () = msg_send![window, setTitle: &*title];
        let _: () = msg_send![window, setReleasedWhenClosed: false];
        // A scroll view round a text view that is set up to fill it.
        let scroll: *mut AnyObject = msg_send![class!(NSTextView), scrollableTextView];
        let text: *mut AnyObject = msg_send![scroll, documentView];
        let _: () = msg_send![text, setEditable: false];
        let _: () = msg_send![text, setSelectable: true];
        // Rich only so the notices line can carry a link; it can't be edited.
        let _: () = msg_send![text, setRichText: true];
        let _: () = msg_send![text, setUsesFindBar: true];
        let _: () = msg_send![text, setAutomaticLinkDetectionEnabled: false];
        let font: *mut AnyObject = msg_send![class!(NSFont), userFixedPitchFontOfSize: 11.0f64];
        let _: () = msg_send![text, setFont: font];
        let body = crate::licenses::text();
        let string = NSString::from_str(&body);
        let _: () = msg_send![text, setString: &*string];
        link_notices(text, &body);
        link_addresses(text, &body);
        let _: () = msg_send![window, setContentView: scroll];
        let _: () = msg_send![window, center];
        window
    }
}

/// Make the notices line in `body` a link to the file. The text view opens
/// it in the system's program for text.
fn link_notices(text: *mut AnyObject, body: &str) {
    let line = crate::licenses::NOTICES_LINK;
    let (Some(at), Some(path)) = (body.find(line), crate::licenses::notices_path()) else {
        return;
    };
    // SAFETY: called on the main thread with live AppKit objects, using
    // messages the classes named answer. The range lies inside `body`.
    unsafe {
        let path = NSString::from_str(&path.to_string_lossy());
        let url: *mut AnyObject = msg_send![class!(NSURL), fileURLWithPath: &*path];
        let storage: *mut AnyObject = msg_send![text, textStorage];
        let name = NSString::from_str("NSLink");
        let range = NSRange::new(
            body[..at].encode_utf16().count(),
            line.encode_utf16().count(),
        );
        let _: () = msg_send![storage, addAttribute: &*name value: url range: range];
    }
}

/// Make the web addresses in `body` links.
fn link_addresses(text: *mut AnyObject, body: &str) {
    // SAFETY: called on the main thread with live AppKit objects, using
    // messages the classes named answer. Each range lies inside `body`.
    unsafe {
        let storage: *mut AnyObject = msg_send![text, textStorage];
        let name = NSString::from_str("NSLink");
        for (at, end) in crate::licenses::web_addresses(body) {
            let address = NSString::from_str(&body[at..end]);
            let url: *mut AnyObject = msg_send![class!(NSURL), URLWithString: &*address];
            if url.is_null() {
                continue;
            }
            let range = NSRange::new(
                body[..at].encode_utf16().count(),
                body[at..end].encode_utf16().count(),
            );
            let _: () = msg_send![storage, addAttribute: &*name value: url range: range];
        }
    }
}

/// Quill's About panel: the standard one, with the licensing summary and a
/// link to the Licenses window in its credits.
pub fn show_about() {
    // SAFETY: as in `show`.
    unsafe {
        let credits = credits();
        let key = NSString::from_str("Credits");
        let options: *mut AnyObject =
            msg_send![class!(NSDictionary), dictionaryWithObject: credits forKey: &*key];
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, orderFrontStandardAboutPanelWithOptions: options];
        let _: () = msg_send![app, activateIgnoringOtherApps: true];
        hook_link(app);
    }
}

/// The credits: the summary, then the link, centered in the system font.
fn credits() -> *mut AnyObject {
    // SAFETY: called on the main thread with live AppKit objects, using
    // messages the classes named answer.
    unsafe {
        let summary = crate::licenses::SUMMARY;
        let link = crate::licenses::LINK_TEXT;
        let all = format!("{summary}\n\n{link}");
        let text = NSString::from_str(&all);
        let attributed: *mut AnyObject = msg_send![class!(NSMutableAttributedString), alloc];
        let attributed: *mut AnyObject = msg_send![attributed, initWithString: &*text];
        let whole = NSRange::new(0, all.encode_utf16().count());
        let add = |name: &str, value: *mut AnyObject, range: NSRange| {
            let name = NSString::from_str(name);
            let _: () = msg_send![attributed, addAttribute: &*name value: value range: range];
        };
        let font: *mut AnyObject = msg_send![class!(NSFont), systemFontOfSize: 11.0f64];
        add("NSFont", font, whole);
        let color: *mut AnyObject = msg_send![class!(NSColor), labelColor];
        add("NSColor", color, whole);
        let style: *mut AnyObject = msg_send![class!(NSMutableParagraphStyle), new];
        let _: () = msg_send![style, setAlignment: 2usize]; // centered
        add("NSParagraphStyle", style, whole);
        // The link covers the last sentence.
        let target = NSString::from_str("quill://licenses");
        let url: *mut AnyObject = msg_send![class!(NSURL), URLWithString: &*target];
        let at = summary.encode_utf16().count() + 2;
        add("NSLink", url, NSRange::new(at, link.encode_utf16().count()));
        attributed
    }
}

/// Make a click on the credits' link open the Licenses window. The panel's
/// credits are a text view somewhere in its window; find it and give it a
/// delegate that answers the click itself instead of passing the address
/// to the system.
fn hook_link(app: *mut AnyObject) {
    // SAFETY: called on the main thread with live AppKit objects, using
    // messages the classes named answer.
    unsafe {
        let windows: *mut AnyObject = msg_send![app, windows];
        let n: usize = msg_send![windows, count];
        for i in 0..n {
            let window: *mut AnyObject = msg_send![windows, objectAtIndex: i];
            let visible: bool = msg_send![window, isVisible];
            let content: *mut AnyObject = msg_send![window, contentView];
            if !visible {
                continue;
            }
            if let Some(text) = find_credits(content) {
                let delegate = links_delegate();
                let _: () = msg_send![text, setDelegate: delegate];
                return;
            }
        }
    }
}

/// The text view under `view` that holds the credits.
fn find_credits(view: *mut AnyObject) -> Option<*mut AnyObject> {
    // SAFETY: called on the main thread with live AppKit objects, using
    // messages the classes named answer.
    unsafe {
        if view.is_null() {
            return None;
        }
        let is_text: bool = msg_send![view, isKindOfClass: class!(NSTextView)];
        if is_text {
            let string: *mut AnyObject = msg_send![view, string];
            let utf8: *const c_char = msg_send![string, UTF8String];
            if !utf8.is_null()
                && CStr::from_ptr(utf8)
                    .to_string_lossy()
                    .contains(crate::licenses::LINK_TEXT)
            {
                return Some(view);
            }
            return None;
        }
        let subviews: *mut AnyObject = msg_send![view, subviews];
        let n: usize = msg_send![subviews, count];
        for i in 0..n {
            let sub: *mut AnyObject = msg_send![subviews, objectAtIndex: i];
            if let Some(found) = find_credits(sub) {
                return Some(found);
            }
        }
        None
    }
}

/// `-[QuillLicenseLinks textView:clickedOnLink:atIndex:]`.
extern "C" fn clicked(
    _this: *mut AnyObject,
    _cmd: Sel,
    _text: *mut AnyObject,
    _link: *mut AnyObject,
    _at: usize,
) -> bool {
    show();
    true
}

/// The object that answers clicks on the credits' link, made once.
fn links_delegate() -> *mut AnyObject {
    // SAFETY: called on the main thread with live AppKit objects, using
    // messages the classes named answer.
    unsafe {
        let existing = LINKS.load(Ordering::Relaxed) as *mut AnyObject;
        if !existing.is_null() {
            return existing;
        }
        let class = match AnyClass::get("QuillLicenseLinks") {
            Some(class) => class,
            None => {
                let raw = objc2::ffi::objc_allocateClassPair(
                    class!(NSObject) as *const AnyClass as *const _,
                    c"QuillLicenseLinks".as_ptr(),
                    0,
                );
                assert!(!raw.is_null(), "could not create the license link class");
                let method: extern "C" fn(
                    *mut AnyObject,
                    Sel,
                    *mut AnyObject,
                    *mut AnyObject,
                    usize,
                ) -> bool = clicked;
                objc2::ffi::class_addMethod(
                    raw,
                    sel!(textView:clickedOnLink:atIndex:).as_ptr(),
                    Some(std::mem::transmute::<
                        extern "C" fn(
                            *mut AnyObject,
                            Sel,
                            *mut AnyObject,
                            *mut AnyObject,
                            usize,
                        ) -> bool,
                        unsafe extern "C" fn(),
                    >(method)),
                    c"B@:@@Q".as_ptr(),
                );
                objc2::ffi::objc_registerClassPair(raw);
                AnyClass::get("QuillLicenseLinks").expect("license link class was registered")
            }
        };
        let object: *mut AnyObject = msg_send![class, new];
        LINKS.store(object as usize, Ordering::Relaxed);
        object
    }
}
