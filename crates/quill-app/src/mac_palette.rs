//! The Style Sheets palette's window on the Mac: a tool window, as other
//! Mac programs' palettes are. It floats above the document windows while
//! Quill is in front and goes away with Quill, and its title bar is the
//! thin one tool windows have, with only a small close button.

use objc2::msg_send;
use objc2::runtime::AnyObject;
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
        for which in [MINIATURIZE, ZOOM] {
            let button: *mut AnyObject = msg_send![win, standardWindowButton: which];
            if !button.is_null() {
                let _: () = msg_send![button, setHidden: true];
            }
        }
    }
}
