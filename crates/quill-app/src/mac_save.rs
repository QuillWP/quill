//! Native Save As with an explicit format selector. rfd's macOS backend
//! combines extension filters instead of presenting a format popup.

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyObject, Sel};
use objc2::{class, msg_send, sel};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use quill_formats::Format;
use std::ffi::{CStr, c_char};
use std::path::{Path, PathBuf};
use std::sync::Once;

fn format_path(name: &Path, format: Format) -> PathBuf {
    name.with_extension(format.extensions()[0])
}

// The save panel itself is the target; there is no global current-panel
// pointer that could outlive cancellation or accidentally address another panel.
extern "C" fn format_changed(panel: *mut AnyObject, _: Sel, popup: *mut AnyObject) {
    // SAFETY: AppKit invokes this action on the save panel with our popup.
    unsafe {
        let index: isize = msg_send![popup, indexOfSelectedItem];
        let Some(format) = usize::try_from(index).ok().and_then(|i| Format::ALL.get(i)) else {
            return;
        };
        let name: *mut AnyObject = msg_send![panel, nameFieldStringValue];
        let utf8: *const c_char = msg_send![name, UTF8String];
        if utf8.is_null() {
            return;
        }
        let name = CStr::from_ptr(utf8).to_string_lossy();
        let path = format_path(Path::new(name.as_ref()), *format);
        let name = NSString::from_str(&path.to_string_lossy());
        let ext = NSString::from_str(format.extensions()[0]);
        let types: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: &*ext];
        let _: () = msg_send![panel, setAllowedFileTypes: types];
        let _: () = msg_send![panel, setNameFieldStringValue: &*name];
    }
}

/// The height the Save panel is given when macOS opens it shorter: 760
/// points, or less on a small screen. A taller panel is left as it is.
fn panel_height(current: f64, screen: f64) -> f64 {
    current.max(760f64.min(screen * 0.8))
}

// macOS opens the panel at the size it last remembered for Quill, which can
// leave little room for the file list once the Format popup is added.
// Called just after the panel appears; a size set before it opens is
// replaced by the remembered one.
extern "C" fn grow_panel(panel: *mut AnyObject, _: Sel, _: *mut AnyObject) {
    // SAFETY: AppKit invokes this on the save panel from its run loop.
    unsafe {
        let mut frame: NSRect = msg_send![panel, frame];
        let screen: *mut AnyObject = msg_send![panel, screen];
        let room = if screen.is_null() {
            950.0
        } else {
            let visible: NSRect = msg_send![screen, visibleFrame];
            visible.size.height
        };
        let height = panel_height(frame.size.height, room);
        if height > frame.size.height {
            // Grow downward, keeping the title bar where it is.
            frame.origin.y -= height - frame.size.height;
            frame.size.height = height;
            let _: () = msg_send![panel, setFrame: frame display: true];
        }
    }
}

pub fn save_file(name: &str, directory: Option<&Path>) -> Option<PathBuf> {
    autoreleasepool(|_| {
        // SAFETY: called on the app thread. Every alloc/init result below is
        // owned by Retained, and panel retains the accessory during runModal.
        unsafe {
            static INSTALL: Once = Once::new();
            INSTALL.call_once(|| {
                let action: extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) = format_changed;
                let imp: unsafe extern "C" fn() = std::mem::transmute(action);
                objc2::ffi::class_addMethod(
                    class!(NSSavePanel) as *const _ as *mut _,
                    sel!(quillSaveFormatChanged:).as_ptr(),
                    Some(imp),
                    c"v@:@".as_ptr(),
                );
                let grow: extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) = grow_panel;
                let imp: unsafe extern "C" fn() = std::mem::transmute(grow);
                objc2::ffi::class_addMethod(
                    class!(NSSavePanel) as *const _ as *mut _,
                    sel!(quillSaveGrow:).as_ptr(),
                    Some(imp),
                    c"v@:@".as_ptr(),
                );
            });
            let raw: *mut AnyObject = msg_send![class!(NSSavePanel), savePanel];
            let panel = Retained::retain(raw)?;
            let panel = Retained::as_ptr(&panel) as *mut AnyObject;
            let raw: *mut AnyObject = msg_send![class!(NSView), alloc];
            let raw: *mut AnyObject = msg_send![raw, initWithFrame: NSRect::new(NSPoint::new(0., 0.), NSSize::new(380., 40.))];
            let accessory = Retained::from_raw(raw)?;
            let raw: *mut AnyObject = msg_send![class!(NSPopUpButton), alloc];
            let raw: *mut AnyObject = msg_send![raw, initWithFrame: NSRect::new(NSPoint::new(90., 8.), NSSize::new(280., 26.)) pullsDown: false];
            let popup = Retained::from_raw(raw)?;
            let popup = Retained::as_ptr(&popup) as *mut AnyObject;
            for format in Format::ALL {
                let title =
                    NSString::from_str(&format!("{} (.{})", format.name(), format.extensions()[0]));
                let _: () = msg_send![popup, addItemWithTitle: &*title];
            }
            let initial = Format::from_path(Path::new(name)).unwrap_or(Format::Quill);
            let index = Format::ALL.iter().position(|f| *f == initial).unwrap_or(0);
            let _: () = msg_send![popup, selectItemAtIndex: index as isize];
            let _: () = msg_send![popup, setTarget: panel];
            let _: () = msg_send![popup, setAction: sel!(quillSaveFormatChanged:)];
            let caption = NSString::from_str("Format:");
            let raw: *mut AnyObject = msg_send![class!(NSTextField), labelWithString: &*caption];
            let _: () = msg_send![raw, setFrame: NSRect::new(NSPoint::new(12., 12.), NSSize::new(74., 20.))];
            let _: () = msg_send![&*accessory, addSubview: raw];
            let _: () = msg_send![&*accessory, addSubview: popup];
            let _: () = msg_send![panel, setAccessoryView: &*accessory];
            let _: () = msg_send![panel, setCanCreateDirectories: true];
            let _: () = msg_send![panel, setAllowsOtherFileTypes: false];
            let _: () = msg_send![panel, setExtensionHidden: false];
            let title = NSString::from_str("Save As");
            let _: () = msg_send![panel, setTitle: &*title];
            let filename = NSString::from_str(name);
            let _: () = msg_send![panel, setNameFieldStringValue: &*filename];
            if let Some(directory) = directory {
                let path = NSString::from_str(&directory.to_string_lossy());
                let url: *mut AnyObject =
                    msg_send![class!(NSURL), fileURLWithPath: &*path isDirectory: true];
                let _: () = msg_send![panel, setDirectoryURL: url];
            }
            format_changed(panel, sel!(quillSaveFormatChanged:), popup);
            // Make room once the panel is up, in the modal run loop.
            let mode = NSString::from_str("kCFRunLoopCommonModes");
            let modes: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: &*mode];
            let _: () = msg_send![panel,
                performSelector: sel!(quillSaveGrow:)
                withObject: std::ptr::null::<AnyObject>()
                afterDelay: 0.0f64
                inModes: modes];
            // Runs inside winit's event handler. Clicks and drags while the
            // panel is up only wait, instead of crashing, because of the
            // patched winit in vendor/winit (see vendor/README.md).
            let response: isize = msg_send![panel, runModal];
            let _: () = msg_send![class!(NSObject), cancelPreviousPerformRequestsWithTarget: panel];
            if response != 1 {
                return None;
            }
            let url: *mut AnyObject = msg_send![panel, URL];
            if url.is_null() {
                return None;
            }
            let path: *mut AnyObject = msg_send![url, path];
            let utf8: *const c_char = msg_send![path, UTF8String];
            if utf8.is_null() {
                return None;
            }
            Some(PathBuf::from(
                CStr::from_ptr(utf8).to_string_lossy().into_owned(),
            ))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_save_panel_grows_and_a_tall_one_is_left_alone() {
        assert_eq!(panel_height(600.0, 1350.0), 760.0);
        assert_eq!(panel_height(900.0, 1350.0), 900.0);
        // A small screen gets most of its height, not 760.
        assert_eq!(panel_height(500.0, 700.0), 560.0);
    }

    #[test]
    fn changing_format_replaces_extension_without_losing_stem() {
        for format in Format::ALL {
            let path = format_path(Path::new("A letter.2026.qwp"), format);
            assert_eq!(Format::from_path(&path), Some(format));
            assert_eq!(path.file_stem().unwrap(), "A letter.2026");
        }
        assert_eq!(
            format_path(Path::new("Untitled"), Format::Docx),
            Path::new("Untitled.docx")
        );
    }
}
