//! Native controls embedded in Quill's otherwise custom-drawn document window.
//!
//! AppKit is supplied by macOS. This module only owns the two `NSScroller`
//! objects a document window may need and relays their target/action messages
//! into winit's user-event queue.

use crate::app::UserEvent;
use crate::canvas::Rect;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{class, msg_send, sel};
use objc2_app_kit::{NSScrollView, NSScroller, NSScrollerKnobStyle, NSScrollerStyle, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use std::cell::Cell;
use std::sync::{Mutex, OnceLock};
use winit::event_loop::EventLoopProxy;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

static PROXY: OnceLock<Mutex<EventLoopProxy<UserEvent>>> = OnceLock::new();

/// `-[WinitApplicationDelegate quillScroll:]`.
extern "C" fn scroll_action(_this: *mut AnyObject, _cmd: Sel, sender: *mut AnyObject) {
    if sender.is_null() {
        return;
    }
    // SAFETY: the only objects using this action are NSScrollers created
    // below, and both selectors return scalar values with these encodings.
    let (part, value): (usize, f64) =
        unsafe { (msg_send![sender, hitPart], msg_send![sender, doubleValue]) };
    if let Some(proxy) = PROXY.get()
        && let Ok(proxy) = proxy.lock()
    {
        let _ = proxy.send_event(UserEvent::NativeScroll {
            scroller: sender as usize,
            part,
            value,
        });
    }
}

/// Add the scrollbar action to winit's application delegate. This is done
/// after the event loop has registered its Objective-C class.
pub fn install(proxy: EventLoopProxy<UserEvent>) {
    let _ = PROXY.set(Mutex::new(proxy));
    let Some(class) = AnyClass::get("WinitApplicationDelegate") else {
        return;
    };
    let f: extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) = scroll_action;
    // SAFETY: `f` is an Objective-C action method: void return, self, command,
    // and one object argument. Adding methods to a registered class is the
    // purpose of `class_addMethod`.
    unsafe {
        let imp: unsafe extern "C" fn() = std::mem::transmute(f);
        objc2::ffi::class_addMethod(
            class as *const AnyClass as *mut _,
            sel!(quillScroll:).as_ptr(),
            Some(imp),
            c"v@:@".as_ptr(),
        );
    }
}

pub struct MacScrollbars {
    primary: MacScrollbar,
    secondary: MacScrollbar,
}

struct MacScrollbar {
    container: Retained<NSScrollView>,
    scroller: Retained<NSScroller>,
    last: Cell<Option<(u32, u32, bool)>>,
}

impl MacScrollbars {
    pub fn new(window: &Window) -> Option<Self> {
        let handle = window.window_handle().ok()?.as_raw();
        let RawWindowHandle::AppKit(handle) = handle else {
            return None;
        };
        // Winit creates and accesses its NSView on this same application
        // thread. Its view is flipped, matching Quill's top-left coordinates.
        let mtm = unsafe { MainThreadMarker::new_unchecked() };
        let parent = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
        let primary = make_scroller(mtm);
        let secondary = make_scroller(mtm);

        // SAFETY: NSApplication's delegate is winit's live application
        // delegate, and NSView retains each subview added to it.
        unsafe {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let delegate: *mut AnyObject = msg_send![app, delegate];
            primary.scroller.setTarget(delegate.as_ref());
            secondary.scroller.setTarget(delegate.as_ref());
            primary.scroller.setAction(Some(sel!(quillScroll:)));
            secondary.scroller.setAction(Some(sel!(quillScroll:)));
            primary.scroller.setContinuous(true);
            secondary.scroller.setContinuous(true);
            parent.addSubview(&primary.container);
            parent.addSubview(&secondary.container);
        }
        secondary.container.setHidden(true);
        Some(Self { primary, secondary })
    }

    pub fn owns(&self, sender: usize) -> Option<usize> {
        if Retained::as_ptr(&self.primary.scroller) as usize == sender {
            Some(0)
        } else if Retained::as_ptr(&self.secondary.scroller) as usize == sender {
            Some(1)
        } else {
            None
        }
    }

    pub fn sync(&self, primary: ScrollbarState, secondary: Option<ScrollbarState>, ui: f32) {
        sync_one(&self.primary, primary, ui);
        match secondary {
            Some(state) => sync_one(&self.secondary, state, ui),
            None => self.secondary.container.setHidden(true),
        }
    }
}

impl Drop for MacScrollbars {
    fn drop(&mut self) {
        // SAFETY: both controls are still alive and are only manipulated on
        // the main application thread.
        unsafe {
            self.primary.container.removeFromSuperview();
            self.secondary.container.removeFromSuperview();
        }
    }
}

#[derive(Clone, Copy)]
pub struct ScrollbarState {
    pub frame: Rect,
    pub value: f32,
    pub proportion: f32,
    pub hidden: bool,
    pub enabled: bool,
}

fn make_scroller(mtm: MainThreadMarker) -> MacScrollbar {
    // SAFETY: allocation, initialization and configuration all happen on the
    // main thread as required by AppKit.
    unsafe {
        let container = NSScrollView::initWithFrame(mtm.alloc(), NSRect::ZERO);
        let scroller = NSScroller::initWithFrame(mtm.alloc(), NSRect::ZERO);
        container.setDrawsBackground(false);
        container.setHasHorizontalScroller(false);
        container.setVerticalScroller(Some(&scroller));
        container.setHasVerticalScroller(true);
        container.setAutohidesScrollers(false);
        container.setScrollerStyle(NSScrollerStyle::Overlay);
        container.setScrollerKnobStyle(NSScrollerKnobStyle::Dark);
        scroller.setKnobStyle(NSScrollerKnobStyle::Dark);
        MacScrollbar {
            container,
            scroller,
            last: Cell::new(None),
        }
    }
}

fn sync_one(scrollbar: &MacScrollbar, state: ScrollbarState, ui: f32) {
    let scroller = &scrollbar.scroller;
    scrollbar.container.setHidden(state.hidden);
    if state.hidden {
        return;
    }
    let r = state.frame;
    // Overlay scrollers float over the document rather than occupying the
    // old full-width scrollbar trough. AppKit draws the thumb itself.
    let right = (r.x + r.w) * ui;
    let width = 16.0_f32.min(r.w * ui);
    let frame = NSRect::new(
        NSPoint::new((right - width) as f64, (r.y * ui) as f64),
        NSSize::new(width as f64, (r.h * ui) as f64),
    );
    // SAFETY: geometry and scalar control properties may be updated on the
    // main thread at any time outside AppKit's drawing callback.
    unsafe {
        scrollbar.container.setFrame(frame);
        scrollbar.container.tile();
        scroller.setDoubleValue(state.value.clamp(0.0, 1.0) as f64);
        scroller.setKnobProportion(state.proportion.clamp(0.0, 1.0) as f64);
        let _: () = msg_send![scroller, setNeedsDisplay: true];
    }
    scroller.setEnabled(state.enabled);
    let now = (
        state.value.clamp(0.0, 1.0).to_bits(),
        state.proportion.clamp(0.0, 1.0).to_bits(),
        state.enabled,
    );
    if scrollbar.last.replace(Some(now)) != Some(now) && state.enabled {
        // NSScrollView owns the overlay's presentation and fade animation.
        // Flash when content size or position changes, including wheel and
        // trackpad scrolling driven by Quill's canvas.
        unsafe { scrollbar.container.flashScrollers() };
    }
}
