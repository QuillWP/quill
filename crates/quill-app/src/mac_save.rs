//! Native Save As with an explicit format selector. rfd's macOS backend
//! combines extension filters instead of presenting a format popup.

use objc2::declare::ClassBuilder;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
use objc2::{class, msg_send, msg_send_id, sel};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use quill_formats::Format;
use std::ffi::{CStr, c_char};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once, OnceLock};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

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

/// The height the Save panel is given when macOS opens it shorter: 640
/// points, or less on a small screen. A taller panel is left as it is.
fn panel_height(current: f64, screen: f64) -> f64 {
    current.max(640f64.min(screen * 0.8))
}

/// The size macOS remembers for the expanded panel, as it stores it:
/// "{880, 448}".
fn parse_size(text: &str) -> Option<(f64, f64)> {
    let (w, h) = text
        .trim()
        .strip_prefix('{')?
        .strip_suffix('}')?
        .split_once(',')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

// macOS opens an expanded panel at the size it remembers for Quill, which
// can leave little room for the file list once the Format popup is added.
// A size set on the panel before it opens is replaced by the remembered
// one, so Quill raises the remembered size instead.
fn make_room(screen: f64) {
    const KEY: &str = "NSNavPanelExpandedSizeForSaveMode";
    // SAFETY: called on the app thread; standardUserDefaults is shared.
    unsafe {
        let defaults: *mut AnyObject = msg_send![class!(NSUserDefaults), standardUserDefaults];
        let key = NSString::from_str(KEY);
        let value: *mut AnyObject = msg_send![defaults, stringForKey: &*key];
        let (width, height) = if value.is_null() {
            (880.0, 0.0)
        } else {
            let utf8: *const c_char = msg_send![value, UTF8String];
            if utf8.is_null() {
                return;
            }
            match parse_size(&CStr::from_ptr(utf8).to_string_lossy()) {
                Some(size) => size,
                None => return,
            }
        };
        let wanted = panel_height(height, screen);
        if wanted > height {
            let size = NSString::from_str(&format!("{{{width}, {wanted}}}"));
            let _: () = msg_send![defaults, setObject: &*size forKey: &*key];
        }
    }
}

// macOS does not always open the panel at the remembered size: a panel
// collapsed and expanded again comes back at its own size (874 by 353),
// and the first panel after a relaunch can open at whatever size the
// system's panel service last saw. So once the panel has opened, and
// again after each expand, Quill watches it from its run loop and grows it.
// It waits until the panel is expanded and its size has held for a few
// checks, because macOS expands in two steps (height, then width) and
// growing between them is undone by the second. It then animates the rest
// of the way, grows it again if macOS shrinks it, and stops watching after
// about two seconds.
extern "C" fn grow_panel(panel: *mut AnyObject, _: Sel, _: *mut AnyObject) {
    // SAFETY: AppKit invokes this on the save panel from its run loop.
    unsafe {
        let expanded: bool = msg_send![panel, isExpanded];
        let mut frame: NSRect = msg_send![panel, frame];
        let size = (frame.size.width, frame.size.height);
        let steady = {
            let mut wait = GROW_WAIT.lock().unwrap_or_else(|e| e.into_inner());
            wait.steady = if wait.last == size {
                wait.steady + 1
            } else {
                0
            };
            wait.last = size;
            wait.tries += 1;
            if wait.tries < 60 {
                schedule_grow(panel, 0.03);
            }
            wait.steady >= 3
        };
        if !expanded || !steady {
            return;
        }
        let height = panel_height(frame.size.height, screen_height(Some(panel)));
        if height > frame.size.height {
            // Grow downward, keeping the top where it is.
            frame.origin.y -= height - frame.size.height;
            frame.size.height = height;
            let _: () = msg_send![panel, setFrame: frame display: true animate: true];
        }
    }
}

// What grow_panel has seen since it started watching.
struct GrowWait {
    last: (f64, f64),
    steady: u32,
    tries: u32,
}

static GROW_WAIT: Mutex<GrowWait> = Mutex::new(GrowWait {
    last: (0.0, 0.0),
    steady: 0,
    tries: 0,
});

/// Starts grow_panel watching `panel` afresh, dropping any watch already
/// under way.
unsafe fn watch_size(panel: *mut AnyObject) {
    // SAFETY: the caller passes the live save panel.
    unsafe {
        let _: () = msg_send![class!(NSObject),
            cancelPreviousPerformRequestsWithTarget: panel
            selector: sel!(quillSaveGrow:)
            object: std::ptr::null::<AnyObject>()];
        *GROW_WAIT.lock().unwrap_or_else(|e| e.into_inner()) = GrowWait {
            last: (0.0, 0.0),
            steady: 0,
            tries: 0,
        };
        schedule_grow(panel, 0.0);
    }
}

/// Calls grow_panel on `panel` after `delay` seconds, in every run loop
/// mode, so it also runs while the sheet is modal.
unsafe fn schedule_grow(panel: *mut AnyObject, delay: f64) {
    // SAFETY: the caller passes the live save panel.
    unsafe {
        let mode = NSString::from_str("kCFRunLoopCommonModes");
        let modes: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: &*mode];
        let _: () = msg_send![panel,
            performSelector: sel!(quillSaveGrow:)
            withObject: std::ptr::null::<AnyObject>()
            afterDelay: delay
            inModes: modes];
    }
}

// The panel's delegate: panel:willExpand: is how Quill hears of an expand.
// AppKit sends it more than once per click, for collapses too, which
// watch_size copes with.
extern "C" fn will_expand(_: *mut AnyObject, _: Sel, panel: *mut AnyObject, expanding: Bool) {
    if expanding.as_bool() {
        // SAFETY: AppKit passes the live save panel.
        unsafe { watch_size(panel) };
    }
}

fn delegate_class() -> &'static AnyClass {
    static CLASS: OnceLock<&'static AnyClass> = OnceLock::new();
    CLASS.get_or_init(|| {
        let mut builder = ClassBuilder::new("QuillSavePanelDelegate", class!(NSObject)).unwrap();
        // SAFETY: the signature is NSOpenSavePanelDelegate's panel:willExpand:.
        unsafe {
            builder.add_method(
                sel!(panel:willExpand:),
                will_expand as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, Bool),
            );
        }
        builder.register()
    })
}

/// Visible height of the screen the document is on, or of the main screen.
fn screen_height(parent: Option<*mut AnyObject>) -> f64 {
    // SAFETY: called on the app thread with a live window, if any.
    unsafe {
        let mut screen: *mut AnyObject = match parent {
            Some(window) => msg_send![window, screen],
            None => std::ptr::null_mut(),
        };
        if screen.is_null() {
            screen = msg_send![class!(NSScreen), mainScreen];
        }
        if screen.is_null() {
            return 950.0;
        }
        let visible: NSRect = msg_send![screen, visibleFrame];
        visible.size.height
    }
}

/// The document's NSWindow, to hang the panel from as a sheet. None when
/// it has no window or already shows a sheet; the panel then opens on its
/// own.
fn sheet_parent(window: &Window) -> Option<*mut AnyObject> {
    let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
        return None;
    };
    // SAFETY: winit owns this NSView and its window while `window` lives.
    unsafe {
        let view = handle.ns_view.as_ptr().cast::<AnyObject>();
        let parent: *mut AnyObject = msg_send![view, window];
        if parent.is_null() {
            return None;
        }
        let sheet: *mut AnyObject = msg_send![parent, attachedSheet];
        sheet.is_null().then_some(parent)
    }
}

/// Shows the panel as a sheet on `parent`, as TextEdit does, and waits for
/// it. A panel on its own window lost its title bar when collapsed. The
/// completion block ends the modal loop with the panel's answer.
unsafe fn run_sheet(panel: *mut AnyObject, parent: *mut AnyObject) -> isize {
    use block2::RcBlock;
    use objc2_06::msg_send as send;
    use objc2_06::runtime::AnyObject as Object;
    let done = RcBlock::new(|response: isize| unsafe {
        let app: *mut Object = send![objc2_06::class!(NSApplication), sharedApplication];
        let _: () = send![app, stopModalWithCode: response];
    });
    unsafe {
        let panel = panel.cast::<Object>();
        let _: () = send![panel, beginSheetModalForWindow: parent.cast::<Object>(), completionHandler: &*done];
        let app: *mut Object = send![objc2_06::class!(NSApplication), sharedApplication];
        send![app, runModalForWindow: panel]
    }
}

pub fn save_file(window: &Window, name: &str, directory: Option<&Path>) -> Option<PathBuf> {
    autoreleasepool(|_| {
        // SAFETY: called on the app thread. Every alloc/init result below is
        // owned by Retained, and panel retains the accessory while it runs.
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
            let parent = sheet_parent(window);
            make_room(screen_height(parent));
            let raw: *mut AnyObject = msg_send![class!(NSSavePanel), savePanel];
            let panel = Retained::retain(raw)?;
            let panel = Retained::as_ptr(&panel) as *mut AnyObject;
            // The panel does not retain its delegate; this lives until return.
            let delegate: Retained<AnyObject> = msg_send_id![delegate_class(), new];
            let _: () = msg_send![panel, setDelegate: &*delegate];
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
            let filename = NSString::from_str(name);
            let _: () = msg_send![panel, setNameFieldStringValue: &*filename];
            if let Some(directory) = directory {
                let path = NSString::from_str(&directory.to_string_lossy());
                let url: *mut AnyObject =
                    msg_send![class!(NSURL), fileURLWithPath: &*path isDirectory: true];
                let _: () = msg_send![panel, setDirectoryURL: url];
            }
            format_changed(panel, sel!(quillSaveFormatChanged:), popup);
            // Runs inside winit's event handler. Clicks and drags while the
            // panel is up only wait, instead of crashing, because of the
            // patched winit in vendor/winit (see vendor/README.md).
            watch_size(panel);
            let response: isize = match parent {
                Some(parent) => run_sheet(panel, parent),
                None => msg_send![panel, runModal],
            };
            let _: () = msg_send![class!(NSObject), cancelPreviousPerformRequestsWithTarget: panel];
            let _: () = msg_send![panel, setDelegate: std::ptr::null::<AnyObject>()];
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
    fn the_remembered_panel_size_is_read_as_macos_writes_it() {
        assert_eq!(parse_size("{874, 353}"), Some((874.0, 353.0)));
        assert_eq!(parse_size("{880,640.5}"), Some((880.0, 640.5)));
        assert_eq!(parse_size("880 x 640"), None);
    }

    #[test]
    fn a_short_save_panel_grows_and_a_tall_one_is_left_alone() {
        assert_eq!(panel_height(500.0, 1350.0), 640.0);
        assert_eq!(panel_height(900.0, 1350.0), 900.0);
        // A small screen gets most of its height, not 640.
        assert_eq!(panel_height(400.0, 700.0), 560.0);
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
