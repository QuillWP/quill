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
        let Some(format) = usize::try_from(index)
            .ok()
            .and_then(|i| Format::SAVE_AS.get(i))
        else {
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
        allow_only(panel, *format);
        let _: () = msg_send![panel, setNameFieldStringValue: &*name];
    }
}

/// Let the panel give only names ending in one of `format`'s extensions.
/// With other types refused, macOS settles the name in the panel itself
/// (offering to use the right extension), so the file Quill writes is the
/// one the panel granted, which is all the App Sandbox lets it write.
///
/// SAFETY: `panel` is a live NSSavePanel, used on the main thread.
unsafe fn allow_only(panel: *mut AnyObject, format: Format) {
    // SAFETY: as the caller promises; plain Foundation collection calls.
    unsafe {
        let types: *mut AnyObject = msg_send![class!(NSMutableArray), array];
        for ext in format.extensions() {
            let ext = NSString::from_str(ext);
            let _: () = msg_send![types, addObject: &*ext];
        }
        let _: () = msg_send![panel, setAllowedFileTypes: types];
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

/// A new, empty folder on the same volume as `document` to make its next
/// saved copy in before renaming it over the document: the system's
/// NSItemReplacementDirectory, which the App Sandbox lets Quill write in.
/// Quill removes the folder after the save. Given to quill_formats at start.
pub fn replacement_folder(document: &Path) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    // An existing document is named itself; a new one by its folder.
    let (target, is_folder) = if document.exists() {
        (document, false)
    } else {
        (document.parent()?, true)
    };
    let target = NSString::from_str(target.to_str()?);
    autoreleasepool(|_| {
        // SAFETY: Foundation class and instance methods with their documented
        // argument types (NSSearchPathDirectory and NSSearchPathDomainMask are
        // NSUInteger). Every returned object is checked for nil before use.
        unsafe {
            let url: *mut AnyObject = msg_send![class!(NSURL), fileURLWithPath: &*target isDirectory: Bool::new(is_folder)];
            if url.is_null() {
                return None;
            }
            const ITEM_REPLACEMENT_DIRECTORY: usize = 99;
            const USER_DOMAIN_MASK: usize = 1;
            let manager: *mut AnyObject = msg_send![class!(NSFileManager), defaultManager];
            let mut error: *mut AnyObject = std::ptr::null_mut();
            let folder: *mut AnyObject = msg_send![manager,
                URLForDirectory: ITEM_REPLACEMENT_DIRECTORY
                inDomain: USER_DOMAIN_MASK
                appropriateForURL: url
                create: Bool::YES
                error: &mut error];
            if folder.is_null() {
                return None;
            }
            let path: *mut AnyObject = msg_send![folder, path];
            if path.is_null() {
                return None;
            }
            let utf8: *const c_char = msg_send![path, fileSystemRepresentation];
            if utf8.is_null() {
                return None;
            }
            let bytes = CStr::from_ptr(utf8).to_bytes();
            Some(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
        }
    })
}

/// The document and the related file being written for it, while
/// `copy_related` runs. The presenter's methods read it, from any thread.
static RELATED: Mutex<Option<(PathBuf, PathBuf)>> = Mutex::new(None);

/// An NSURL for one of the paths in RELATED, or nil.
fn related_url(pick: fn(&(PathBuf, PathBuf)) -> &PathBuf) -> *mut AnyObject {
    let related = RELATED.lock().unwrap_or_else(|e| e.into_inner());
    let Some(text) = related.as_ref().and_then(|r| pick(r).to_str()) else {
        return std::ptr::null_mut();
    };
    let text = NSString::from_str(text);
    // SAFETY: a Foundation class method; the URL is autoreleased, as a
    // getter's result should be.
    unsafe { msg_send![class!(NSURL), fileURLWithPath: &*text] }
}

extern "C" fn presented_item_url(_: *mut AnyObject, _: Sel) -> *mut AnyObject {
    related_url(|r| &r.1)
}

extern "C" fn primary_presented_item_url(_: *mut AnyObject, _: Sel) -> *mut AnyObject {
    related_url(|r| &r.0)
}

extern "C" fn presented_item_operation_queue(_: *mut AnyObject, _: Sel) -> *mut AnyObject {
    // SAFETY: a Foundation class method.
    unsafe { msg_send![class!(NSOperationQueue), mainQueue] }
}

/// An NSFilePresenter for a file that belongs with a document: its
/// presentedItemURL is the related file and its primaryPresentedItemURL the
/// document.
fn presenter_class() -> &'static AnyClass {
    static CLASS: OnceLock<&'static AnyClass> = OnceLock::new();
    CLASS.get_or_init(|| {
        let mut builder = ClassBuilder::new("QuillRelatedFilePresenter", class!(NSObject)).unwrap();
        if let Some(protocol) = objc2::runtime::AnyProtocol::get("NSFilePresenter") {
            builder.add_protocol(protocol);
        }
        type Getter = extern "C" fn(*mut AnyObject, Sel) -> *mut AnyObject;
        // SAFETY: each signature is the NSFilePresenter property getter's.
        unsafe {
            builder.add_method(sel!(presentedItemURL), presented_item_url as Getter);
            builder.add_method(
                sel!(primaryPresentedItemURL),
                primary_presented_item_url as Getter,
            );
            builder.add_method(
                sel!(presentedItemOperationQueue),
                presented_item_operation_queue as Getter,
            );
        }
        builder.register()
    })
}

/// Copy `document` to `related`, a file beside it with the same name and
/// another extension that Info.plist declares as a related item (a backup).
/// The App Sandbox allows that file only while a file presenter names the
/// document as its primary item and the write is coordinated. Outside the
/// sandbox the same steps simply copy the file.
pub fn copy_related(document: &Path, related: &Path) -> Result<(), String> {
    use block2::StackBlock;
    use objc2_06::msg_send as send;
    use objc2_06::runtime::AnyObject as Object;
    use std::cell::RefCell;
    use std::os::unix::ffi::OsStrExt;

    let Some(text) = related.to_str() else {
        return quill_formats::copy_atomic(document, related);
    };
    *RELATED.lock().unwrap_or_else(|e| e.into_inner()) =
        Some((document.to_path_buf(), related.to_path_buf()));
    let result: RefCell<Option<Result<(), String>>> = RefCell::new(None);
    let text = NSString::from_str(text);
    autoreleasepool(|_| {
        // SAFETY: Foundation methods with their documented argument types.
        // The coordinator runs the accessor before it returns, so the block
        // and what it borrows outlive it; the presenter is removed after.
        unsafe {
            let presenter: Retained<AnyObject> = msg_send_id![presenter_class(), new];
            let _: () = msg_send![class!(NSFileCoordinator), addFilePresenter: &*presenter];
            let raw: *mut AnyObject = msg_send![class!(NSFileCoordinator), alloc];
            let raw: *mut AnyObject = msg_send![raw, initWithFilePresenter: &*presenter];
            if let Some(coordinator) = Retained::from_raw(raw) {
                let url: *mut AnyObject = msg_send![class!(NSURL), fileURLWithPath: &*text];
                let accessor = StackBlock::new(|given: *mut Object| {
                    let path = if given.is_null() {
                        related.to_path_buf()
                    } else {
                        let path: *mut Object = send![given, path];
                        let utf8: *const c_char = send![path, fileSystemRepresentation];
                        if utf8.is_null() {
                            related.to_path_buf()
                        } else {
                            PathBuf::from(std::ffi::OsStr::from_bytes(
                                CStr::from_ptr(utf8).to_bytes(),
                            ))
                        }
                    };
                    *result.borrow_mut() = Some(quill_formats::copy_atomic(document, &path));
                });
                // NSFileCoordinatorWritingForReplacing.
                const FOR_REPLACING: usize = 1 << 3;
                let mut error: *mut Object = std::ptr::null_mut();
                let coordinator = Retained::as_ptr(&coordinator) as *mut Object;
                let _: () = send![coordinator,
                    coordinateWritingItemAtURL: url.cast::<Object>(),
                    options: FOR_REPLACING,
                    error: &mut error,
                    byAccessor: &*accessor];
            }
            let _: () = msg_send![class!(NSFileCoordinator), removeFilePresenter: &*presenter];
        }
    });
    *RELATED.lock().unwrap_or_else(|e| e.into_inner()) = None;
    result.into_inner().unwrap_or_else(|| {
        Err(format!(
            "The system would not let Quill write \u{201c}{}\u{201d}.",
            related.display()
        ))
    })
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
            for format in Format::SAVE_AS {
                let title =
                    NSString::from_str(&format!("{} (.{})", format.name(), format.extensions()[0]));
                let _: () = msg_send![popup, addItemWithTitle: &*title];
            }
            let initial = Format::from_path(Path::new(name)).unwrap_or(Format::Quill);
            let index = Format::SAVE_AS
                .iter()
                .position(|f| *f == initial)
                .unwrap_or(0);
            let _: () = msg_send![popup, selectItemAtIndex: index as isize];
            allow_only(panel, initial);
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
    fn a_related_copy_is_made_and_replaced_beside_the_document() {
        if !crate::unrestricted_host_services() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("quill-related-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let document = dir.join("Report.qwp");
        let backup = dir.join("Report.qwp-backup");
        for version in ["first", "second"] {
            std::fs::write(&document, version).unwrap();
            copy_related(&document, &backup).unwrap();
            assert_eq!(std::fs::read_to_string(&backup).unwrap(), version);
        }
        let mut names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["Report.qwp", "Report.qwp-backup"]);
        assert!(RELATED.lock().unwrap().is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_replacement_folder_is_new_and_on_the_documents_volume() {
        use std::os::unix::fs::MetadataExt;
        let dir = std::env::temp_dir().join(format!("quill-replacement-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let existing = dir.join("Report.qwp");
        std::fs::write(&existing, b"x").unwrap();
        let volume = std::fs::metadata(&dir).unwrap().dev();
        // An existing document, and a new one not saved yet.
        for document in [existing.clone(), dir.join("New.qwp")] {
            let folder = replacement_folder(&document).expect("a folder");
            let meta = std::fs::metadata(&folder).unwrap();
            assert!(meta.is_dir(), "{}", folder.display());
            assert_eq!(meta.dev(), volume, "{}", folder.display());
            assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
            std::fs::remove_dir(&folder).unwrap();
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

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
