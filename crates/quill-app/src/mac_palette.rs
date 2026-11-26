//! The Style Sheets palette's window on the Mac: a tool window, as other
//! Mac programs' palettes are. It floats above the document windows while
//! Quill is in front and goes away with Quill, and its title bar is the
//! thin one tool windows have, with only a small close button.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send, msg_send_id, sel};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

const TITLED: usize = 1 << 0;
const CLOSABLE: usize = 1 << 1;
const RESIZABLE: usize = 1 << 3;
const UTILITY: usize = 1 << 4;
const FLOATING_LEVEL: isize = 3;
/// NSWindowMiniaturizeButton and NSWindowZoomButton.
const MINIATURIZE: isize = 1;
const ZOOM: isize = 2;

pub fn style(window: &Window) {
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // SAFETY: winit owns this NSView and its window for the call.
    unsafe {
        let view = handle.ns_view.as_ptr().cast::<AnyObject>();
        let win: *mut AnyObject = msg_send![view, window];
        if win.is_null() {
            return;
        }
        let _: () = msg_send![win, setStyleMask: TITLED | CLOSABLE | RESIZABLE | UTILITY];
        let _: () = msg_send![win, setLevel: FLOATING_LEVEL];
        let _: () = msg_send![win, setHidesOnDeactivate: true];
        add_dock_button(win);
        for which in [MINIATURIZE, ZOOM] {
            let button: *mut AnyObject = msg_send![win, standardWindowButton: which];
            if !button.is_null() {
                let _: () = msg_send![button, setHidden: true];
            }
        }
    }
}

unsafe fn add_dock_button(window: *mut AnyObject) {
    // SAFETY: all AppKit objects are created and attached on the app thread.
    unsafe {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(28.0, 18.0));
        let button: objc2::rc::Allocated<AnyObject> = msg_send_id![class!(NSButton), alloc];
        let button: Retained<AnyObject> = msg_send_id![button, initWithFrame: frame];
        let _: () = msg_send![&*button, setBordered: false];
        let tip = NSString::from_str("Dock to Document");
        let _: () = msg_send![&*button, setToolTip: &*tip];
        let _: () = msg_send![&*button, setAccessibilityLabel: &*tip];
        let image_class = class!(NSImage);
        let supports_symbols: bool = msg_send![image_class,
            respondsToSelector: sel!(imageWithSystemSymbolName:accessibilityDescription:)];
        let image: *mut AnyObject = if supports_symbols {
            let symbol = NSString::from_str("arrow.down.right.and.arrow.up.left");
            msg_send![image_class, imageWithSystemSymbolName: &*symbol, accessibilityDescription: &*tip]
        } else {
            std::ptr::null_mut()
        };
        if image.is_null() {
            let arrows = NSString::from_str("↘↖");
            let _: () = msg_send![&*button, setTitle: &*arrows];
        } else {
            let _: () = msg_send![&*button, setImage: image];
            let _: () = msg_send![&*button, setImagePosition: 1isize];
        }

        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let delegate: *mut AnyObject = msg_send![app, delegate];
        let _: () = msg_send![&*button, setTarget: delegate];
        let _: () = msg_send![&*button, setAction: sel!(quillDockStyles:)];
        let accessory: Retained<AnyObject> =
            msg_send_id![class!(NSTitlebarAccessoryViewController), new];
        let _: () = msg_send![&*accessory, setView: &*button];
        // NSLayoutAttributeRight places the button beside the window title.
        let _: () = msg_send![&*accessory, setLayoutAttribute: 2isize];
        let _: () = msg_send![window, addTitlebarAccessoryViewController: &*accessory];
    }
}
