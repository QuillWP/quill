//! PDFKit-backed system printing. Only PDF generation runs off the app thread;
//! AppKit presents titled application-modal panels on the main thread.

use crate::app::UserEvent;
use objc2::declare::ClassBuilder;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
use objc2::{class, msg_send, sel};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use quill_core::DocumentProps;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};
use winit::event_loop::EventLoopProxy;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowId};

#[link(name = "PDFKit", kind = "framework")]
unsafe extern "C" {}

unsafe extern "C" {
    static NSPrintSpoolJob: *mut AnyObject;
    static NSPrintJobSavingURL: *mut AnyObject;
    static NSPrintCopies: *mut AnyObject;
    static NSPrintAllPages: *mut AnyObject;
    static NSPrintFirstPage: *mut AnyObject;
    static NSPrintLastPage: *mut AnyObject;
    static NSWindowDidBecomeKeyNotification: *mut AnyObject;
}

/// Printer choices belong to this document window, never sharedPrintInfo.
#[derive(Clone, Default)]
pub struct Settings(Rc<RefCell<Option<Retained<AnyObject>>>>);

struct Completion {
    title: &'static str,
    titled: Cell<bool>,
    document_requested: Cell<bool>,
    proxy: EventLoopProxy<UserEvent>,
    window: WindowId,
    settings: Settings,
    _delegate: Retained<AnyObject>,
    _owner: Retained<AnyObject>,
    _info: Retained<AnyObject>,
    _pdf: Option<Retained<AnyObject>>,
    _parent: Retained<AnyObject>,
}

impl Drop for Completion {
    fn drop(&mut self) {
        // Remove the main-thread observer before its context is released.
        unsafe {
            let center: *mut AnyObject = msg_send![class!(NSNotificationCenter), defaultCenter];
            let _: () = msg_send![center, removeObserver: &*self._delegate];
        }
    }
}

unsafe fn observe_panel(context: *mut Completion) {
    unsafe {
        let delegate = &*(*context)._delegate;
        let ivar = delegate.class().instance_variable("quillContext").unwrap();
        ivar.load_ptr::<*mut c_void>(delegate).write(context.cast());
        let center: *mut AnyObject = msg_send![class!(NSNotificationCenter), defaultCenter];
        let _: () = msg_send![center, addObserver: delegate
            selector: sel!(quillPrintPanelBecameKey:)
            name: NSWindowDidBecomeKeyNotification object: std::ptr::null_mut::<AnyObject>()];
    }
}

extern "C" fn panel_became_key(this: *mut AnyObject, _: Sel, notification: *mut AnyObject) {
    unsafe {
        let ivar = (*this).class().instance_variable("quillContext").unwrap();
        let context = *ivar.load::<*mut c_void>(&*this);
        let context = &*context.cast::<Completion>();
        if context.titled.get() {
            return;
        }
        let panel: *mut AnyObject = msg_send![notification, object];
        if std::ptr::eq(panel, Retained::as_ptr(&context._parent)) {
            return;
        }
        context.titled.set(true);
        title_panel(&*panel, &*this, context.title);
        if context.title == "Print" {
            // Kept invisible until its size settles, so the change is not
            // seen.
            let _: () = msg_send![panel, setAlphaValue: 0.0f64];
            *SHORTEN_WAIT.lock().unwrap_or_else(|e| e.into_inner()) = (NSSize::new(0., 0.), 0, 0);
            schedule_shorten(this, panel);
        }
    }
}

/// Tallest the Print panel opens, in points: the height of the Save panel.
const PRINT_PANEL_HEIGHT: f64 = 640.0;

// The size shorten_panel saw last, how many checks it has held, and how
// many checks there have been.
static SHORTEN_WAIT: Mutex<(NSSize, u32, u32)> = Mutex::new((NSSize::new(0., 0.), 0, 0));

/// Calls shorten_panel through `delegate` shortly, in every run loop mode,
/// so it also runs while the panel is modal.
unsafe fn schedule_shorten(delegate: *mut AnyObject, panel: *mut AnyObject) {
    unsafe {
        let mode = NSString::from_str("kCFRunLoopCommonModes");
        let modes: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: &*mode];
        let _: () = msg_send![delegate,
            performSelector: sel!(quillShortenPrintPanel:)
            withObject: panel
            afterDelay: 0.05f64
            inModes: modes];
    }
}

/// macOS opens the Print panel about 900 points tall, taller than other
/// apps' print panels. Shorter, its options column scrolls, so Quill brings
/// it down to PRINT_PANEL_HEIGHT, keeping its top where it is. macOS is
/// still sizing the panel when it first becomes key, so this waits until
/// its size has held for a few checks, and gives up after about two
/// seconds. Either way it then shows the panel, which was made invisible
/// while it waited.
extern "C" fn shorten_panel(this: *mut AnyObject, _: Sel, panel: *mut AnyObject) {
    // SAFETY: called through performSelector with the live print panel,
    // which the print operation keeps open while it runs.
    unsafe {
        let visible: bool = msg_send![panel, isVisible];
        if !visible {
            return;
        }
        let mut frame: NSRect = msg_send![panel, frame];
        let (steady, waiting) = {
            let mut wait = SHORTEN_WAIT.lock().unwrap_or_else(|e| e.into_inner());
            wait.1 = if wait.0 == frame.size { wait.1 + 1 } else { 0 };
            wait.0 = frame.size;
            wait.2 += 1;
            let waiting = wait.1 < 3 && wait.2 < 40;
            if waiting {
                schedule_shorten(this, panel);
            }
            (wait.1 >= 3, waiting)
        };
        if steady && frame.size.height > PRINT_PANEL_HEIGHT {
            frame.origin.y += frame.size.height - PRINT_PANEL_HEIGHT;
            frame.size.height = PRINT_PANEL_HEIGHT;
            let _: () = msg_send![panel, setFrame: frame display: true];
        }
        if !waiting {
            let _: () = msg_send![panel, setAlphaValue: 1.0f64];
        }
    }
}

fn paper_twips(size: NSSize) -> Result<(i32, i32), String> {
    let convert = |points: f64| {
        let twips = (points * 20.).round();
        if points.is_finite() && (720.0..=288_000.0).contains(&twips) {
            Ok(twips as i32)
        } else {
            Err("The selected paper size is outside Quill's supported range.".to_string())
        }
    };
    Ok((convert(size.width)?, convert(size.height)?))
}

fn parent(window: &Window) -> Result<Retained<AnyObject>, String> {
    let handle = window.window_handle().map_err(|e| e.to_string())?.as_raw();
    let RawWindowHandle::AppKit(handle) = handle else {
        return Err("The document has no AppKit window.".into());
    };
    // SAFETY: winit owns this NSView. Retain its window for the sheet lifetime.
    unsafe {
        let view = handle.ns_view.as_ptr().cast::<AnyObject>();
        let raw: *mut AnyObject = msg_send![view, window];
        let parent = Retained::retain(raw).ok_or("The document window has closed.")?;
        let sheet: *mut AnyObject = msg_send![&*parent, attachedSheet];
        if !sheet.is_null() {
            return Err("Close the current sheet before printing or changing page setup.".into());
        }
        Ok(parent)
    }
}

fn print_info(settings: &Settings, props: &DocumentProps) -> Result<Retained<AnyObject>, String> {
    // SAFETY: invoked only on the app thread; copying isolates cancellation.
    unsafe {
        let saved = settings.0.borrow();
        let source: *mut AnyObject = saved.as_ref().map_or_else(
            || msg_send![class!(NSPrintInfo), sharedPrintInfo],
            |info| Retained::as_ptr(info) as *mut _,
        );
        let raw: *mut AnyObject = msg_send![source, copy];
        let info = Retained::from_raw(raw).ok_or("Could not create printer settings.")?;
        let landscape = props.page_width > props.page_height;
        let size = NSSize::new(
            props.page_width.min(props.page_height) as f64 / 20.,
            props.page_width.max(props.page_height) as f64 / 20.,
        );
        // NSPrintInfo's paperSize includes orientation. Set portrait first,
        // then the portrait dimensions, then the desired orientation.
        let _: () = msg_send![&*info, setOrientation: 0isize];
        let _: () = msg_send![&*info, setPaperSize: size];
        let _: () = msg_send![&*info, setOrientation: landscape as isize];
        // The PDF already contains document margins. Printer margins must
        // not inset it a second time or silently shrink the page.
        let _: () = msg_send![&*info, setLeftMargin: 0f64];
        let _: () = msg_send![&*info, setRightMargin: 0f64];
        let _: () = msg_send![&*info, setTopMargin: 0f64];
        let _: () = msg_send![&*info, setBottomMargin: 0f64];
        let _: () = msg_send![&*info, setHorizontallyCentered: false];
        let _: () = msg_send![&*info, setVerticallyCentered: false];
        // Never reuse an earlier PDF destination or a stale page range.
        let _: () = msg_send![&*info, setJobDisposition: NSPrintSpoolJob];
        let dict: *mut AnyObject = msg_send![&*info, dictionary];
        for key in [NSPrintJobSavingURL, NSPrintFirstPage, NSPrintLastPage] {
            let _: () = msg_send![dict, removeObjectForKey: key];
        }
        let one: *mut AnyObject = msg_send![class!(NSNumber), numberWithInteger: 1isize];
        let all: *mut AnyObject = msg_send![class!(NSNumber), numberWithBool: true];
        let _: () = msg_send![dict, setObject: one forKey: NSPrintCopies];
        let _: () = msg_send![dict, setObject: all forKey: NSPrintAllPages];
        Ok(info)
    }
}

extern "C" fn printed(
    _: *mut AnyObject,
    _: Sel,
    operation: *mut AnyObject,
    success: Bool,
    context: *mut c_void,
) {
    // SAFETY: one callback per operation; context is its unique Box. AppKit
    // presents printing errors itself; cancellation is not an application error.
    unsafe {
        let completion = Box::from_raw(context.cast::<Completion>());
        if success.as_bool() {
            let info: *mut AnyObject = msg_send![operation, printInfo];
            let raw: *mut AnyObject = msg_send![info, copy];
            *completion.settings.0.borrow_mut() = Retained::from_raw(raw);
        }
        let _ = completion.proxy.send_event(UserEvent::PrintFinished {
            window: completion.window,
            result: Ok(()),
            native: true,
        });
    }
}

extern "C" fn page_setup_finished(
    _: *mut AnyObject,
    _: Sel,
    _: *mut AnyObject,
    response: isize,
    context: *mut c_void,
) {
    // SAFETY: called once after the modal panel returns; info is our isolated copy.
    unsafe {
        let completion = Box::from_raw(context.cast::<Completion>());
        let result = if response == 1 {
            let size: NSSize = msg_send![&*completion._info, paperSize];
            paper_twips(size).map(Some)
        } else {
            Ok(None)
        };
        if matches!(result, Ok(Some(_))) {
            let raw: *mut AnyObject = msg_send![&*completion._info, copy];
            *completion.settings.0.borrow_mut() = Retained::from_raw(raw);
        }
        let _ = completion.proxy.send_event(UserEvent::PageSetupFinished {
            window: completion.window,
            result,
            document: completion.document_requested.get(),
        });
    }
}

extern "C" fn document_clicked(_: *mut AnyObject, _: Sel, sender: *mut AnyObject) {
    // SAFETY: the button belongs to the active Page Setup panel. Canceling it
    // opens Quill's Document dialog after completion; retain that requested action.
    unsafe {
        let tag: isize = msg_send![sender, tag];
        let context = &*(tag as *const Completion);
        context.document_requested.set(true);
        close_panel(std::ptr::null_mut(), sel!(quillClosePrintPanel:), sender);
    }
}

extern "C" fn close_panel(_: *mut AnyObject, _: Sel, sender: *mut AnyObject) {
    // Route the title-bar close button through the same completion path as Cancel.
    unsafe {
        let panel: *mut AnyObject = msg_send![sender, window];
        let content: *mut AnyObject = msg_send![panel, contentView];
        if !click_cancel(content) {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![app, stopModalWithCode: 0isize];
            let _: () = msg_send![panel, orderOut: std::ptr::null_mut::<AnyObject>()];
        }
    }
}

unsafe fn click_cancel(view: *mut AnyObject) -> bool {
    unsafe {
        let button: bool = msg_send![view, isKindOfClass: class!(NSButton)];
        if button {
            let key: *mut AnyObject = msg_send![view, keyEquivalent];
            let escape = NSString::from_str("\u{1b}");
            let is_cancel: bool = msg_send![key, isEqualToString: &*escape];
            let title: *mut AnyObject = msg_send![view, title];
            let cancel = NSString::from_str("Cancel");
            let is_cancel_title: bool = msg_send![title, isEqualToString: &*cancel];
            if is_cancel || is_cancel_title {
                let _: () = msg_send![view, performClick: std::ptr::null_mut::<AnyObject>()];
                return true;
            }
        }
        let children: *mut AnyObject = msg_send![view, subviews];
        let count: usize = msg_send![children, count];
        for i in 0..count {
            let child: *mut AnyObject = msg_send![children, objectAtIndex: i];
            if click_cancel(child) {
                return true;
            }
        }
        false
    }
}

fn title_panel(panel: &AnyObject, delegate: &AnyObject, title: &str) {
    // AppKit owns the panel's content. Keep its native controls and delegate.
    unsafe {
        let style: usize = msg_send![panel, styleMask];
        let _: () = msg_send![panel, setStyleMask: style | 3usize];
        let title = NSString::from_str(title);
        let _: () = msg_send![panel, setTitle: &*title];
        let _: () = msg_send![panel, setTitleVisibility: 0isize];
        let close: *mut AnyObject = msg_send![panel, standardWindowButton: 0usize];
        if !close.is_null() {
            let _: () = msg_send![close, setEnabled: true];
            let _: () = msg_send![close, setTarget: delegate];
            let _: () = msg_send![close, setAction: sel!(quillClosePrintPanel:)];
        }
    }
}

fn delegate() -> Result<Retained<AnyObject>, String> {
    static CLASS: OnceLock<&'static AnyClass> = OnceLock::new();
    let class = CLASS.get_or_init(|| {
        let mut builder = ClassBuilder::new("QuillPrintCompletion", class!(NSObject)).unwrap();
        builder.add_ivar::<*mut c_void>("quillContext");
        // SAFETY: callbacks use exactly AppKit's documented delegate signatures.
        unsafe {
            builder.add_method(
                sel!(printOperationDidRun:success:contextInfo:),
                printed as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, Bool, *mut c_void),
            );
            builder.add_method(
                sel!(pageLayoutDidEnd:returnCode:contextInfo:),
                page_setup_finished
                    as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, isize, *mut c_void),
            );
            builder.add_method(
                sel!(quillDocument:),
                document_clicked as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
            );
            builder.add_method(
                sel!(quillClosePrintPanel:),
                close_panel as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
            );
            builder.add_method(
                sel!(quillShortenPrintPanel:),
                shorten_panel as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
            );
            builder.add_method(
                sel!(quillPrintPanelBecameKey:),
                panel_became_key as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
            );
        }
        builder.register()
    });
    // SAFETY: NSObject's new returns an owned instance.
    unsafe {
        let raw: *mut AnyObject = msg_send![*class, new];
        Retained::from_raw(raw).ok_or("Could not create the print completion handler.".into())
    }
}

pub fn page_setup(
    window: &Window,
    props: &DocumentProps,
    settings: &Settings,
    proxy: EventLoopProxy<UserEvent>,
) -> Result<(), String> {
    autoreleasepool(|_| unsafe {
        let parent = parent(window)?;
        let info = print_info(settings, props)?;
        let raw: *mut AnyObject = msg_send![class!(NSPageLayout), pageLayout];
        let panel = Retained::retain(raw).ok_or("Could not create Page Setup.")?;
        let delegate = delegate()?;
        // Preserve Page Setup's existing Document… route for margins and
        // layout options, without replacing system paper-size controls.
        let raw: *mut AnyObject = msg_send![class!(NSViewController), new];
        let accessory = Retained::from_raw(raw).ok_or("Could not create Page Setup options.")?;
        let raw: *mut AnyObject = msg_send![class!(NSView), alloc];
        let raw: *mut AnyObject = msg_send![raw,
            initWithFrame: NSRect::new(NSPoint::ZERO, NSSize::new(280., 44.))];
        let view = Retained::from_raw(raw).ok_or("Could not create Page Setup options.")?;
        let title = NSString::from_str("Document Setup…");
        let button: *mut AnyObject = msg_send![class!(NSButton),
            buttonWithTitle: &*title target: &*delegate action: sel!(quillDocument:)];
        let _: () = msg_send![button,
            setFrame: NSRect::new(NSPoint::new(12., 8.), NSSize::new(140., 28.))];
        let _: () = msg_send![&*view, addSubview: button];
        let _: () = msg_send![&*accessory, setView: &*view];
        let title = NSString::from_str("Quill");
        let _: () = msg_send![&*accessory, setTitle: &*title];
        let _: () = msg_send![&*panel, addAccessoryController: &*accessory];
        let context = Box::new(Completion {
            title: "Page Setup",
            titled: Cell::new(false),
            document_requested: Cell::new(false),
            proxy,
            window: window.id(),
            settings: settings.clone(),
            _delegate: delegate.clone(),
            _owner: panel.clone(),
            _info: info.clone(),
            _pdf: None,
            _parent: parent.clone(),
        });
        let context = Box::into_raw(context);
        let _: () = msg_send![button, setTag: context as isize];
        observe_panel(context);
        // Modal inside winit's event handler: needs the patched winit in
        // vendor/winit (see vendor/README.md).
        let response: isize = msg_send![&*panel, runModalWithPrintInfo: &*info];
        page_setup_finished(
            std::ptr::null_mut(),
            sel!(pageLayoutDidEnd:returnCode:contextInfo:),
            Retained::as_ptr(&panel) as *mut _,
            response,
            context.cast(),
        );
        Ok(())
    })
}

pub fn print(
    window: &Window,
    bytes: &[u8],
    title: &str,
    props: &DocumentProps,
    settings: &Settings,
    proxy: EventLoopProxy<UserEvent>,
) -> Result<(), String> {
    autoreleasepool(|_| unsafe {
        let parent = parent(window)?;
        let info = print_info(settings, props)?;
        let data: *mut AnyObject = msg_send![class!(NSData),
            dataWithBytes: bytes.as_ptr().cast::<c_void>() length: bytes.len()];
        let raw: *mut AnyObject = msg_send![class!(PDFDocument), alloc];
        let raw: *mut AnyObject = msg_send![raw, initWithData: data];
        let pdf = Retained::from_raw(raw).ok_or("Could not read the document's print PDF.")?;
        // No auto-rotation: respect the chosen orientation. PDFKit implements
        // page ranges, preview, printer settings, scaling and the system PDF menu.
        let raw: *mut AnyObject = msg_send![&*pdf,
            printOperationForPrintInfo: &*info scalingMode: 0isize autoRotate: false];
        let operation = Retained::retain(raw).ok_or("Could not create a print operation.")?;
        let title = NSString::from_str(title);
        let _: () = msg_send![&*operation, setJobTitle: &*title];
        let _: () = msg_send![&*operation, setShowsPrintPanel: true];
        let _: () = msg_send![&*operation, setShowsProgressPanel: true];
        let panel: *mut AnyObject = msg_send![&*operation, printPanel];
        let options: usize = msg_send![panel, options];
        // Copies, page range, paper, orientation, scaling, built-in preview.
        let _: () = msg_send![panel, setOptions: options | 0x1fusize | (1usize << 17)];
        let delegate = delegate()?;
        let context = Box::new(Completion {
            title: "Print",
            titled: Cell::new(false),
            document_requested: Cell::new(false),
            proxy,
            window: window.id(),
            settings: settings.clone(),
            _delegate: delegate.clone(),
            _owner: operation.clone(),
            _info: info,
            _pdf: Some(pdf),
            _parent: parent.clone(),
        });
        let context = Box::into_raw(context);
        observe_panel(context);
        // Runs the Print panel modally inside winit's event handler: needs
        // the patched winit in vendor/winit (see vendor/README.md).
        let success: Bool = msg_send![&*operation, runOperation];
        printed(
            std::ptr::null_mut(),
            sel!(printOperationDidRun:success:contextInfo:),
            Retained::as_ptr(&operation) as *mut _,
            success,
            context.cast(),
        );
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_paper_sizes_keep_orientation_and_custom_measurements() {
        assert_eq!(
            paper_twips(NSSize::new(612., 792.)).unwrap(),
            (12240, 15840)
        );
        assert_eq!(
            paper_twips(NSSize::new(792., 612.)).unwrap(),
            (15840, 12240)
        );
        let size = NSSize::new(11906. / 20., 16838. / 20.);
        assert_eq!(paper_twips(size).unwrap(), (11906, 16838));
    }

    #[test]
    fn native_paper_sizes_reject_nonfinite_and_unreasonable_dimensions() {
        for width in [f64::NAN, f64::INFINITY, -1., 0., 10., 1e20] {
            assert!(paper_twips(NSSize::new(width, 792.)).is_err());
        }
    }

    #[test]
    fn printer_settings_copy_document_paper_and_isolate_canceled_edits() {
        autoreleasepool(|_| unsafe {
            let settings = Settings::default();
            let mut props = DocumentProps {
                page_width: 16838,
                page_height: 11906,
                ..DocumentProps::default()
            };
            let original = print_info(&settings, &props).unwrap();
            let size: NSSize = msg_send![&*original, paperSize];
            assert_eq!(paper_twips(size).unwrap(), (16838, 11906));
            let dict: *mut AnyObject = msg_send![&*original, dictionary];
            let url = NSString::from_str("file:///tmp/previous-print.pdf");
            let _: () = msg_send![dict, setObject: &*url forKey: NSPrintJobSavingURL];
            let two: *mut AnyObject = msg_send![class!(NSNumber), numberWithInteger: 2isize];
            let _: () = msg_send![dict, setObject: two forKey: NSPrintFirstPage];
            *settings.0.borrow_mut() = Some(original);

            let canceled = print_info(&settings, &props).unwrap();
            let _: () = msg_send![&*canceled, setPaperSize: NSSize::new(400., 500.)];
            let saved = settings.0.borrow();
            let size: NSSize = msg_send![&**saved.as_ref().unwrap(), paperSize];
            assert_eq!(paper_twips(size).unwrap(), (16838, 11906));
            drop(saved);

            // Reopening after undo uses the document's dimensions, even if
            // its remembered printer settings came from a landscape job.
            props.page_width = 12240;
            props.page_height = 15840;
            let portrait = print_info(&settings, &props).unwrap();
            let size: NSSize = msg_send![&*portrait, paperSize];
            assert_eq!(paper_twips(size).unwrap(), (12240, 15840));
            let margin: f64 = msg_send![&*portrait, leftMargin];
            assert_eq!(margin, 0.);
            let dict: *mut AnyObject = msg_send![&*portrait, dictionary];
            let destination: *mut AnyObject = msg_send![dict, objectForKey: NSPrintJobSavingURL];
            let first: *mut AnyObject = msg_send![dict, objectForKey: NSPrintFirstPage];
            assert!(destination.is_null());
            // NSPrintInfo may repopulate a removed range key with page 1.
            if !first.is_null() {
                let page: isize = msg_send![first, integerValue];
                assert_eq!(page, 1);
            }
            let all: *mut AnyObject = msg_send![dict, objectForKey: NSPrintAllPages];
            let all: bool = msg_send![all, boolValue];
            assert!(all);
        });
    }
}
