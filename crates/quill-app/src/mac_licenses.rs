//! The Mac's Licenses and About windows.
//!
//! The About window shows the artwork and a short summary of Quill's licensing with a
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
static ABOUT: AtomicUsize = AtomicUsize::new(0);
/// The object that answers a click on the About panel's link.
static LINKS: AtomicUsize = AtomicUsize::new(0);

/// Close an auxiliary window if it is in front. Command-W comes through
/// the menu as Close, which would otherwise close a document.
pub fn close_if_key() -> bool {
    for saved in [&WINDOW, &ABOUT] {
        let window = saved.load(Ordering::Relaxed) as *mut AnyObject;
        if window.is_null() {
            continue;
        }
        // SAFETY: called on the main thread; these windows are kept for good.
        unsafe {
            let key: bool = msg_send![window, isKeyWindow];
            if key {
                let _: () = msg_send![window, performClose: std::ptr::null::<AnyObject>()];
                return true;
            }
        }
    }
    false
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

/// Show the About window with room for the artwork and its links.
pub fn show_about() {
    // SAFETY: called on the main thread with live AppKit objects.
    unsafe {
        let mut window = ABOUT.load(Ordering::Relaxed) as *mut AnyObject;
        if window.is_null() {
            window = make_about();
            ABOUT.store(window as usize, Ordering::Relaxed);
        }
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, activateIgnoringOtherApps: true];
        let _: () = msg_send![window, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
    }
}

fn make_about() -> *mut AnyObject {
    // SAFETY: all objects and selectors are AppKit/Foundation classes, called
    // on the main thread. The window owns its views and stays alive when closed.
    unsafe {
        let rect = NSRect::new(NSPoint::ZERO, NSSize::new(420.0, 600.0));
        let window: *mut AnyObject = msg_send![class!(NSPanel), alloc];
        let window: *mut AnyObject = msg_send![window, initWithContentRect: rect styleMask: 3usize backing: 2usize defer: false];
        let _: () = msg_send![window, setTitle: &*NSString::from_str("About Quill")];
        let _: () = msg_send![window, setReleasedWhenClosed: false];
        let content: *mut AnyObject = msg_send![window, contentView];
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let icon: *mut AnyObject = msg_send![app, applicationIconImage];
        let image: *mut AnyObject = msg_send![class!(NSImageView), alloc];
        let image: *mut AnyObject = msg_send![image, initWithFrame: NSRect::new(NSPoint::new(178.0, 520.0), NSSize::new(64.0, 64.0))];
        let _: () = msg_send![image, setImage: icon];
        let _: () = msg_send![image, setImageScaling: 3usize];
        let _: () = msg_send![content, addSubview: image];
        let _: () = msg_send![image, release];
        for (string, y, size) in [
            (
                if cfg!(debug_assertions) {
                    "Quill Debug".to_owned()
                } else {
                    "Quill".to_owned()
                },
                486.0,
                20.0,
            ),
            (
                format!("Version {}", env!("CARGO_PKG_VERSION")),
                462.0,
                12.0,
            ),
            (
                "Copyright \u{a9} 2026 Lucas Krupinski and Quill contributors".to_owned(),
                438.0,
                11.0,
            ),
        ] {
            let label: *mut AnyObject =
                msg_send![class!(NSTextField), labelWithString: &*NSString::from_str(&string)];
            let _: () = msg_send![label, setFrame: NSRect::new(NSPoint::new(20.0, y), NSSize::new(380.0, 28.0))];
            let font: *mut AnyObject = msg_send![class!(NSFont), systemFontOfSize: size];
            let _: () = msg_send![label, setFont: font];
            let _: () = msg_send![label, setAlignment: 1usize];
            let _: () = msg_send![content, addSubview: label];
        }
        let scroll: *mut AnyObject = msg_send![class!(NSTextView), scrollableTextView];
        let _: () = msg_send![scroll, setFrame: NSRect::new(NSPoint::new(20.0, 20.0), NSSize::new(380.0, 406.0))];
        let _: () = msg_send![scroll, setHasVerticalScroller: true];
        let _: () = msg_send![scroll, setAutohidesScrollers: true];
        let text: *mut AnyObject = msg_send![scroll, documentView];
        let _: () = msg_send![text, setEditable: false];
        let _: () = msg_send![text, setSelectable: true];
        let _: () = msg_send![text, setRichText: true];
        let _: () = msg_send![text, setDrawsBackground: false];
        let _: () = msg_send![scroll, setDrawsBackground: false];
        let _: () = msg_send![text, setDelegate: links_delegate()];
        let storage: *mut AnyObject = msg_send![text, textStorage];
        let body = credits();
        let _: () = msg_send![storage, setAttributedString: body];
        let _: () = msg_send![body, release];
        let _: () = msg_send![content, addSubview: scroll];
        let _: () = msg_send![window, center];
        window
    }
}

/// The credits: the summary, then the link, centered in the system font.
fn credits() -> *mut AnyObject {
    // SAFETY: called on the main thread with live AppKit objects, using
    // messages the classes named answer.
    unsafe {
        let summary = crate::licenses::SUMMARY;
        let link = crate::licenses::LINK_TEXT;
        let all = format!("quillwp.com\n\n{summary}\n\n{link}");
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
        let _: () = msg_send![style, setAlignment: 1usize]; // centered
        add("NSParagraphStyle", style, whole);
        // The link covers the last sentence.
        let target = NSString::from_str("quill://licenses");
        let url: *mut AnyObject = msg_send![class!(NSURL), URLWithString: &*target];
        let at = all[..all.find(link).unwrap()].encode_utf16().count();
        add("NSLink", url, NSRange::new(at, link.encode_utf16().count()));
        let website = NSString::from_str("https://quillwp.com");
        let website: *mut AnyObject = msg_send![class!(NSURL), URLWithString: &*website];
        add("NSLink", website, NSRange::new(0, "quillwp.com".len()));

        let bytes = include_bytes!("../../../assets/quill-about.jpg");
        let data: *mut AnyObject = msg_send![class!(NSData), dataWithBytes: bytes.as_ptr().cast::<std::ffi::c_void>() length: bytes.len()];
        let image: *mut AnyObject = msg_send![class!(NSImage), alloc];
        let image: *mut AnyObject = msg_send![image, initWithData: data];
        if image.is_null() {
            let _: () = msg_send![style, release];
            return attributed;
        }
        let attachment: *mut AnyObject = msg_send![class!(NSTextAttachment), new];
        let _: () = msg_send![attachment, setImage: image];
        let _: () =
            msg_send![attachment, setBounds: NSRect::new(NSPoint::ZERO, NSSize::new(260.0, 260.0))];
        let picture: *mut AnyObject =
            msg_send![class!(NSAttributedString), attributedStringWithAttachment: attachment];
        let result: *mut AnyObject = msg_send![class!(NSMutableAttributedString), new];
        let _: () = msg_send![result, appendAttributedString: picture];
        let newline: *mut AnyObject = msg_send![class!(NSAttributedString), alloc];
        let newline: *mut AnyObject =
            msg_send![newline, initWithString: &*NSString::from_str("\n")];
        let _: () = msg_send![result, appendAttributedString: newline];
        let _: () = msg_send![result, appendAttributedString: attributed];
        let length: usize = msg_send![result, length];
        let _: () = msg_send![result, addAttribute: &*NSString::from_str("NSParagraphStyle") value: style range: NSRange::new(0, length)];
        let _: () = msg_send![newline, release];
        let _: () = msg_send![attachment, release];
        let _: () = msg_send![image, release];
        let _: () = msg_send![attributed, release];
        let _: () = msg_send![style, release];
        result
    }
}

/// `-[QuillLicenseLinks textView:clickedOnLink:atIndex:]`.
extern "C" fn clicked(
    _this: *mut AnyObject,
    _cmd: Sel,
    _text: *mut AnyObject,
    link: *mut AnyObject,
    _at: usize,
) -> bool {
    // Let AppKit open ordinary web links; only the local license link is ours.
    // SAFETY: AppKit supplies an NSURL or NSString as the live link object.
    unsafe {
        let description: *mut AnyObject = msg_send![link, description];
        let value: *const c_char = msg_send![description, UTF8String];
        if !value.is_null() && CStr::from_ptr(value).to_bytes() == b"quill://licenses" {
            show();
            return true;
        }
    }
    false
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
