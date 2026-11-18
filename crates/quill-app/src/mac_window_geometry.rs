//! Window > Size To and Move To on the Mac.

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2::{class, msg_send, msg_send_id};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::ffi::{CStr, c_char};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

#[derive(Clone, Copy)]
enum Operation {
    Size,
    Move,
}

/// The smallest window Size To makes, in pixels.
const MIN_SIZE: (i32, i32) = (320, 160);
/// How much of a window stays on screen when Move To puts part of it off
/// the left or right edge, in points, as when dragging one there.
const KEEP_VISIBLE: f64 = 100.0;
/// How much of the title bar must stay above the bottom of the screen.
const TITLE_BAR: f64 = 28.0;

fn parse_pair(first: &str, second: &str, operation: Operation) -> Option<(i32, i32)> {
    let first = first.trim().parse::<i32>().ok()?;
    let second = second.trim().parse::<i32>().ok()?;
    let widest = 100_000;
    // A window can sit partly off the left edge, so X may be negative.
    let first_ok = match operation {
        Operation::Size => (0..=widest).contains(&first),
        Operation::Move => (-widest..=widest).contains(&first),
    };
    ((first_ok) && (0..=widest).contains(&second)).then_some((first, second))
}

/// Where Move To puts a window's top-left corner, in points from the
/// screen's left edge and the area below the menu bar. `left` and `right`
/// are the usable area's edges on the same scale and `height` is its height.
/// Part of the window may hang off either side, but `KEEP_VISIBLE` of it
/// stays on screen, and the title bar never goes under the bottom.
fn clamp_top_left(
    (x, y): (f64, f64),
    (width, height): (f64, f64),
    (left, right, usable_height): (f64, f64, f64),
) -> (f64, f64) {
    let keep = width.min(KEEP_VISIBLE);
    let low = left - (width - keep);
    let high = (right - keep).max(low);
    let lowest = (usable_height - height.min(TITLE_BAR)).max(0.0);
    (x.clamp(low, high), y.clamp(0.0, lowest))
}

unsafe fn rust_string(string: *mut AnyObject) -> String {
    // SAFETY: `string` is an NSString returned by a live NSTextField.
    let bytes: *const c_char = unsafe { msg_send![string, UTF8String] };
    if bytes.is_null() {
        String::new()
    } else {
        // SAFETY: NSString promises a nul-terminated UTF-8 view here.
        unsafe { CStr::from_ptr(bytes) }
            .to_string_lossy()
            .into_owned()
    }
}

fn native_window(window: &Window) -> Option<*mut AnyObject> {
    let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
        return None;
    };
    // SAFETY: winit owns this NSView and its NSWindow while `window` lives.
    unsafe {
        let view = handle.ns_view.as_ptr().cast::<AnyObject>();
        let native: *mut AnyObject = msg_send![view, window];
        (!native.is_null()).then_some(native)
    }
}

unsafe fn screen_frames(native: *mut AnyObject) -> Option<(NSRect, NSRect)> {
    // SAFETY: `native` is a live NSWindow on the application thread.
    let screen: *mut AnyObject = unsafe { msg_send![native, screen] };
    if screen.is_null() {
        return None;
    }
    // SAFETY: NSScreen answers both selectors with NSRect values.
    let frame: NSRect = unsafe { msg_send![screen, frame] };
    let visible: NSRect = unsafe { msg_send![screen, visibleFrame] };
    Some((frame, visible))
}

/// Set the window's frame, moved or shrunk by AppKit as needed so the title
/// bar stays reachable on the screen the window is on.
unsafe fn set_frame_on_screen(native: *mut AnyObject, wanted: NSRect) {
    // SAFETY: `native` is a live NSWindow on the application thread.
    unsafe {
        let screen: *mut AnyObject = msg_send![native, screen];
        let frame: NSRect = if screen.is_null() {
            wanted
        } else {
            msg_send![native, constrainFrameRect: wanted toScreen: screen]
        };
        let _: () = msg_send![native, setFrame: frame display: true animate: false];
    }
}

fn current_values(window: &Window, operation: Operation) -> Option<(i32, i32)> {
    let native = native_window(window)?;
    let scale = window.scale_factor();
    // SAFETY: the pointer came from this live winit window.
    let frame: NSRect = unsafe { msg_send![native, frame] };
    match operation {
        Operation::Size => Some((
            (frame.size.width * scale).round() as i32,
            (frame.size.height * scale).round() as i32,
        )),
        Operation::Move => {
            // Coordinates are relative to this screen's left edge and usable
            // top edge. AppKit's y axis points up from the bottom.
            let (screen, visible) = unsafe { screen_frames(native) }?;
            Some((
                ((frame.origin.x - screen.origin.x) * scale).round() as i32,
                ((visible.origin.y + visible.size.height - (frame.origin.y + frame.size.height))
                    * scale)
                    .round() as i32,
            ))
        }
    }
}

fn prompt(operation: Operation, current: (i32, i32)) -> Result<Option<(i32, i32)>, ()> {
    let (title, detail, first_label, second_label) = match operation {
        Operation::Size => (
            "Size To",
            "Set the complete window size in pixels.",
            "Width:",
            "Height:",
        ),
        Operation::Move => (
            "Move To",
            "Set the top-left position in pixels. Zero is the screen's left edge and the area below the menu bar. X may be negative to slide the window partly off the side; 100 points always stay in view.",
            "X:",
            "Y:",
        ),
    };
    autoreleasepool(|_| {
        // SAFETY: all AppKit objects are created and used on the application thread.
        unsafe {
            let alert: Retained<AnyObject> = msg_send_id![class!(NSAlert), new];
            let _: () = msg_send![&*alert, setMessageText: &*NSString::from_str(title)];
            let _: () = msg_send![&*alert, setInformativeText: &*NSString::from_str(detail)];
            let _: *mut AnyObject =
                msg_send![&*alert, addButtonWithTitle: &*NSString::from_str("OK")];
            let _: *mut AnyObject =
                msg_send![&*alert, addButtonWithTitle: &*NSString::from_str("Cancel")];

            let accessory: *mut AnyObject = msg_send![class!(NSView), alloc];
            let accessory: *mut AnyObject = msg_send![
                accessory,
                initWithFrame: NSRect::new(NSPoint::ZERO, NSSize::new(260.0, 54.0))
            ];
            let first_caption: *mut AnyObject = msg_send![
                class!(NSTextField),
                labelWithString: &*NSString::from_str(first_label)
            ];
            let second_caption: *mut AnyObject = msg_send![
                class!(NSTextField),
                labelWithString: &*NSString::from_str(second_label)
            ];
            let first: *mut AnyObject = msg_send![class!(NSTextField), alloc];
            let first: *mut AnyObject = msg_send![
                first,
                initWithFrame: NSRect::new(NSPoint::new(58.0, 30.0), NSSize::new(190.0, 24.0))
            ];
            let second: *mut AnyObject = msg_send![class!(NSTextField), alloc];
            let second: *mut AnyObject = msg_send![
                second,
                initWithFrame: NSRect::new(NSPoint::new(58.0, 0.0), NSSize::new(190.0, 24.0))
            ];
            let _: () = msg_send![first_caption, setFrame: NSRect::new(NSPoint::new(0.0, 32.0), NSSize::new(54.0, 20.0))];
            let _: () = msg_send![second_caption, setFrame: NSRect::new(NSPoint::new(0.0, 2.0), NSSize::new(54.0, 20.0))];
            let _: () =
                msg_send![first, setStringValue: &*NSString::from_str(&current.0.to_string())];
            let _: () =
                msg_send![second, setStringValue: &*NSString::from_str(&current.1.to_string())];
            for view in [first_caption, second_caption, first, second] {
                let _: () = msg_send![accessory, addSubview: view];
            }
            let _: () = msg_send![&*alert, setAccessoryView: accessory];
            let panel: *mut AnyObject = msg_send![&*alert, window];
            let _: bool = msg_send![panel, makeFirstResponder: first];
            let answer: isize = msg_send![&*alert, runModal];
            let result = if answer == 1000 {
                let first_value: *mut AnyObject = msg_send![first, stringValue];
                let second_value: *mut AnyObject = msg_send![second, stringValue];
                parse_pair(
                    &rust_string(first_value),
                    &rust_string(second_value),
                    operation,
                )
                .map(Some)
                .ok_or(())
            } else {
                Ok(None)
            };
            let _: () = msg_send![first, release];
            let _: () = msg_send![second, release];
            let _: () = msg_send![accessory, release];
            result
        }
    })
}

fn choose(window: &Window, operation: Operation) -> Option<(i32, i32)> {
    let current = current_values(window, operation)?;
    loop {
        match prompt(operation, current) {
            Ok(None) => return None,
            Ok(Some(chosen)) => return Some(chosen),
            Err(()) => {
                crate::mac_alert::show(
                    rfd::MessageLevel::Warning,
                    "Quill",
                    match operation {
                        Operation::Size => "Enter whole numbers from 0 through 100000.",
                        Operation::Move => {
                            "Enter whole numbers from -100000 through 100000 for X, and 0 \
                             through 100000 for Y."
                        }
                    },
                    rfd::MessageButtons::Ok,
                );
            }
        }
    }
}

pub fn size_to(window: &Window) {
    let Some((width, height)) = choose(window, Operation::Size) else {
        return;
    };
    // Too small makes the smallest window, as too big makes the largest.
    let (width, height) = (width.max(MIN_SIZE.0), height.max(MIN_SIZE.1));
    let Some(native) = native_window(window) else {
        return;
    };
    let scale = window.scale_factor();
    // Preserve the top-left corner while changing the complete outer frame,
    // then keep it on the screen the window is on.
    unsafe {
        let old: NSRect = msg_send![native, frame];
        let mut size = NSSize::new(width as f64 / scale, height as f64 / scale);
        if let Some((_, visible)) = screen_frames(native) {
            size.width = size.width.min(visible.size.width);
            size.height = size.height.min(visible.size.height);
        }
        let origin = NSPoint::new(old.origin.x, old.origin.y + old.size.height - size.height);
        set_frame_on_screen(native, NSRect::new(origin, size));
    }
}

pub fn move_to(window: &Window) {
    let Some((x, y)) = choose(window, Operation::Move) else {
        return;
    };
    let Some(native) = native_window(window) else {
        return;
    };
    let scale = window.scale_factor();
    // SAFETY: the pointer came from this live winit window.
    unsafe {
        let Some((screen, visible)) = screen_frames(native) else {
            return;
        };
        let frame: NSRect = msg_send![native, frame];
        let (x, y) = clamp_top_left(
            (x as f64 / scale, y as f64 / scale),
            (frame.size.width, frame.size.height),
            (
                visible.origin.x - screen.origin.x,
                visible.origin.x + visible.size.width - screen.origin.x,
                visible.size.height,
            ),
        );
        let origin = NSPoint::new(
            screen.origin.x + x,
            visible.origin.y + visible.size.height - y - frame.size.height,
        );
        // Not constrained to the screen: part of the window may be off it.
        let _: () = msg_send![native, setFrameOrigin: origin];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_is_nonnegative_and_move_may_start_left_of_the_screen() {
        assert_eq!(
            parse_pair(" 1024 ", "768", Operation::Size),
            Some((1024, 768))
        );
        assert_eq!(parse_pair("-1", "2", Operation::Size), None);
        assert_eq!(parse_pair("3.5", "2", Operation::Size), None);
        assert_eq!(parse_pair("three", "2", Operation::Size), None);
        assert_eq!(parse_pair("100001", "2", Operation::Size), None);
        assert_eq!(parse_pair("-40", "20", Operation::Move), Some((-40, 20)));
        assert_eq!(parse_pair("-100001", "20", Operation::Move), None);
        assert_eq!(parse_pair("10", "-1", Operation::Move), None);
    }

    #[test]
    fn move_to_leaves_a_hundred_points_and_the_title_bar_on_screen() {
        // A 800 x 600 window on a 1440 x 800 usable area.
        let size = (800.0, 600.0);
        let area = (0.0, 1440.0, 800.0);
        assert_eq!(clamp_top_left((100.0, 50.0), size, area), (100.0, 50.0));
        // Slid slightly off the left edge stays where asked.
        assert_eq!(clamp_top_left((-40.0, 0.0), size, area), (-40.0, 0.0));
        // Too far left leaves 100 points showing.
        assert_eq!(clamp_top_left((-5000.0, 0.0), size, area).0, -700.0);
        // Too far right leaves 100 points showing.
        assert_eq!(clamp_top_left((9000.0, 0.0), size, area).0, 1340.0);
        // The top stays at the menu bar, and the title bar above the bottom.
        assert_eq!(clamp_top_left((0.0, 9000.0), size, area).1, 772.0);
        // A window narrower than the margin stays wholly on screen.
        assert_eq!(clamp_top_left((-500.0, 0.0), (80.0, 100.0), area).0, 0.0);
    }
}
