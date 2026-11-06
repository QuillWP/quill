//! Look Up: the system's own panel for a word (definitions, the Oxford
//! thesaurus, Wikipedia), as Force Click or Control-Command-D shows it in
//! other Mac apps. It only shows; nothing in the document changes.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_foundation::{NSPoint, NSRect, NSString};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

unsafe extern "C" {
    static NSFontAttributeName: *mut AnyObject;
}

/// Show the panel for `word`, written in `font` at `size` points, with its
/// baseline starting at (`x`, `y`) in the window's points from the top left.
pub fn show(window: &Window, word: &str, font: &str, size: f64, x: f64, y: f64) {
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // SAFETY: winit owns this NSView and it outlives the call; every
    // object made here is retained only for the call.
    unsafe {
        let view = handle.ns_view.as_ptr().cast::<AnyObject>();
        let name = NSString::from_str(font);
        let mut f: *mut AnyObject = msg_send![class!(NSFont), fontWithName: &*name, size: size];
        if f.is_null() {
            f = msg_send![class!(NSFont), systemFontOfSize: size];
        }
        let attrs: *mut AnyObject = msg_send![
            class!(NSDictionary),
            dictionaryWithObject: f,
            forKey: NSFontAttributeName
        ];
        let text = NSString::from_str(word);
        let alloc: *mut AnyObject = msg_send![class!(NSAttributedString), alloc];
        let raw: *mut AnyObject = msg_send![alloc, initWithString: &*text, attributes: attrs];
        let Some(string) = Retained::from_raw(raw) else {
            return;
        };
        // AppKit wants the baseline in the view's own coordinates, which
        // count up from the bottom unless the view is flipped.
        let flipped: bool = msg_send![view, isFlipped];
        let bounds: NSRect = msg_send![view, bounds];
        let y = if flipped { y } else { bounds.size.height - y };
        let _: () = msg_send![
            view,
            showDefinitionForAttributedString: &*string,
            atPoint: NSPoint::new(x, y)
        ];
    }
}
