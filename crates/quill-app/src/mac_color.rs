//! The Other… color in Character: a small alert holding a color well,
//! which opens the system Colors panel to pick any color.

use objc2::rc::autoreleasepool;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use quill_core::props::Rgb;

/// Ask for a color, starting from `start`. None if canceled.
pub fn choose(title: &str, start: Rgb) -> Option<Rgb> {
    autoreleasepool(|_| unsafe {
        let alert: *mut AnyObject = msg_send![class!(NSAlert), new];
        let text = NSString::from_str(title);
        let _: () = msg_send![alert, setMessageText: &*text];
        let info = NSString::from_str("Click the color to open the Colors window.");
        let _: () = msg_send![alert, setInformativeText: &*info];
        for label in ["OK", "Cancel"] {
            let label = NSString::from_str(label);
            let _: *mut AnyObject = msg_send![alert, addButtonWithTitle: &*label];
        }

        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(120.0, 32.0));
        let well: *mut AnyObject = msg_send![class!(NSColorWell), alloc];
        let well: *mut AnyObject = msg_send![well, initWithFrame: frame];
        let (r, g, b) = (
            ((start.0 >> 16) & 0xFF) as f64 / 255.0,
            ((start.0 >> 8) & 0xFF) as f64 / 255.0,
            (start.0 & 0xFF) as f64 / 255.0,
        );
        let color: *mut AnyObject =
            msg_send![class!(NSColor), colorWithSRGBRed: r green: g blue: b alpha: 1.0f64];
        let _: () = msg_send![well, setColor: color];
        let _: () = msg_send![alert, setAccessoryView: well];

        let panel: *mut AnyObject = msg_send![class!(NSColorPanel), sharedColorPanel];
        let _: () = msg_send![panel, setShowsAlpha: false];

        let response: isize = msg_send![alert, runModal];
        // Put the Colors window away with the alert, and let go of it so
        // the next color well does not inherit this one.
        let _: () = msg_send![well, deactivate];
        let _: () = msg_send![panel, orderOut: std::ptr::null::<AnyObject>()];
        let picked: *mut AnyObject = msg_send![well, color];
        let space: *mut AnyObject = msg_send![class!(NSColorSpace), sRGBColorSpace];
        let picked: *mut AnyObject = msg_send![picked, colorUsingColorSpace: space];
        let _: () = msg_send![well, release];
        let _: () = msg_send![alert, release];
        // NSAlertFirstButtonReturn is OK.
        if response != 1000 || picked.is_null() {
            return None;
        }
        let part = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
        let r: f64 = msg_send![picked, redComponent];
        let g: f64 = msg_send![picked, greenComponent];
        let b: f64 = msg_send![picked, blueComponent];
        Some(Rgb(part(r) << 16 | part(g) << 8 | part(b)))
    })
}
