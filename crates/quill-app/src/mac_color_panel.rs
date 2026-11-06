//! Edit Color as the standard macOS Colors panel. The panel is shown
//! modally with Cancel and OK under it, so it answers like Quill's other
//! dialogs: OK gives the color, Cancel or the close button gives nothing.

use objc2::rc::autoreleasepool;
use objc2::runtime::{AnyObject, Sel};
use objc2::{class, msg_send, sel};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use quill_core::props::Rgb;

/// What runModalForWindow: returns after stopModal and abortModal.
const STOPPED: isize = -1000;

/// A push button sending `action` to the application.
unsafe fn button(
    title: &str,
    frame: NSRect,
    action: Sel,
    app: *mut AnyObject,
    key: &str,
) -> *mut AnyObject {
    unsafe {
        let b: *mut AnyObject = msg_send![class!(NSButton), alloc];
        let b: *mut AnyObject = msg_send![b, initWithFrame: frame];
        // NSBezelStyleRounded and NSButtonTypeMomentaryPushIn.
        let _: () = msg_send![b, setBezelStyle: 1usize];
        let _: () = msg_send![b, setButtonType: 7usize];
        let _: () = msg_send![b, setTitle: &*NSString::from_str(title)];
        let _: () = msg_send![b, setKeyEquivalent: &*NSString::from_str(key)];
        let _: () = msg_send![b, setTarget: app];
        let _: () = msg_send![b, setAction: action];
        b
    }
}

/// Shows the Colors panel on `start` and waits. Runs inside winit's event
/// handler like the other modal panels: clicks on the document while it is
/// up only wait, because of the patched winit in vendor/winit.
pub fn pick(start: Rgb) -> Option<Rgb> {
    // SAFETY: called on the app thread, as all of Quill's AppKit calls are.
    autoreleasepool(|_| unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let panel: *mut AnyObject = msg_send![class!(NSColorPanel), sharedColorPanel];
        if panel.is_null() {
            return None;
        }
        let _: () = msg_send![panel, setShowsAlpha: false];
        let _: () = msg_send![panel, setTitle: &*NSString::from_str("Edit Color")];
        let color: *mut AnyObject = msg_send![
            class!(NSColor),
            colorWithSRGBRed: start.r() as f64 / 255.0,
            green: start.g() as f64 / 255.0,
            blue: start.b() as f64 / 255.0,
            alpha: 1.0f64
        ];
        let _: () = msg_send![panel, setColor: color];

        // Cancel and OK at the bottom right, in the usual order. stopModal
        // and abortModal take no argument; the sender a button passes is
        // ignored.
        let size = NSSize::new(240.0, 44.0);
        let bar: *mut AnyObject = msg_send![class!(NSView), alloc];
        let bar: *mut AnyObject = msg_send![bar, initWithFrame: NSRect::new(NSPoint::ZERO, size)];
        let cancel = button(
            "Cancel",
            NSRect::new(NSPoint::new(36.0, 8.0), NSSize::new(96.0, 28.0)),
            sel!(abortModal),
            app,
            "\u{1b}",
        );
        let ok = button(
            "OK",
            NSRect::new(NSPoint::new(136.0, 8.0), NSSize::new(96.0, 28.0)),
            sel!(stopModal),
            app,
            "\r",
        );
        let _: () = msg_send![bar, addSubview: cancel];
        let _: () = msg_send![bar, addSubview: ok];
        // Keep the buttons at the right as the panel is resized.
        let _: () = msg_send![cancel, setAutoresizingMask: 1usize];
        let _: () = msg_send![ok, setAutoresizingMask: 1usize];
        let _: () = msg_send![panel, setAccessoryView: bar];

        // The close button cancels. Without this the modal loop would go
        // on with no window to answer it.
        let center: *mut AnyObject = msg_send![class!(NSNotificationCenter), defaultCenter];
        let closing = NSString::from_str("NSWindowWillCloseNotification");
        let _: () = msg_send![
            center,
            addObserver: app,
            selector: sel!(abortModal),
            name: &*closing,
            object: panel
        ];

        let _: () = msg_send![app, activateIgnoringOtherApps: true];
        let answer: isize = msg_send![app, runModalForWindow: panel];

        let _: () = msg_send![center, removeObserver: app, name: &*closing, object: panel];
        let chosen: *mut AnyObject = msg_send![panel, color];
        let _: () = msg_send![panel, setAccessoryView: std::ptr::null_mut::<AnyObject>()];
        let _: () = msg_send![panel, orderOut: std::ptr::null_mut::<AnyObject>()];
        let _: () = msg_send![cancel, release];
        let _: () = msg_send![ok, release];
        let _: () = msg_send![bar, release];
        if answer != STOPPED || chosen.is_null() {
            return None;
        }
        let space: *mut AnyObject = msg_send![class!(NSColorSpace), sRGBColorSpace];
        let rgb: *mut AnyObject = msg_send![chosen, colorUsingColorSpace: space];
        if rgb.is_null() {
            return None;
        }
        let channel = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
        let r: f64 = msg_send![rgb, redComponent];
        let g: f64 = msg_send![rgb, greenComponent];
        let b: f64 = msg_send![rgb, blueComponent];
        Some(Rgb(channel(r) << 16 | channel(g) << 8 | channel(b)))
    })
}
