//! Check that a mouse event during a modal panel does not crash winit.
//!
//! Quill runs Save, Open, Print and alerts with `runModal` from inside
//! winit's event handler. While a panel is up, AppKit passes clicks through
//! `sendEvent:` and can spin the run loop before the handler returns. This
//! does the same from inside the handler and reports whether it got back.
//! Unpatched winit 0.30 aborts with "tried to handle event while another
//! event is currently being handled"; the patched copy in vendor/winit holds
//! the click until the handler is free and delivers it then. See
//! vendor/README.md.
//!
//!     cargo run -p quill-app --example modal_events

#[cfg(target_os = "macos")]
fn main() {
    use winit::application::ApplicationHandler;
    use winit::event::{DeviceEvent, DeviceId, WindowEvent};
    use winit::event_loop::{ActiveEventLoop, EventLoop};
    use winit::window::WindowId;

    #[derive(Default)]
    struct Check {
        ran: bool,
        held_after: usize,
    }

    impl ApplicationHandler for Check {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.ran {
                return;
            }
            self.ran = true;
            // SAFETY: called on the main thread inside the event handler,
            // which is exactly the situation being checked.
            unsafe { click_while_busy() };
            println!("back in the handler without a crash");
            // Let the held click arrive, then stop.
            event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
        }

        fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
            if self.ran && matches!(event, DeviceEvent::Button { .. }) {
                self.held_after += 1;
            }
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            if self.ran && !event_loop.exiting() {
                println!(
                    "held mouse events delivered afterwards: {}",
                    self.held_after
                );
                event_loop.exit();
            }
        }

        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }

    let event_loop = EventLoop::new().expect("event loop");
    event_loop.run_app(&mut Check::default()).expect("run");
}

#[cfg(target_os = "macos")]
unsafe fn click_while_busy() {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::NSPoint;

    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        // What the crash log shows AppKit doing during the Save panel: a
        // click passes through NSApplication's sendEvent: (winit sets the
        // event aside, its handler being busy), and AppKit's gesture
        // handling then spins the run loop with runUntilDate:, which runs
        // what winit set aside while the handler is still busy.
        for kind in [1usize, 2usize] {
            let event: *mut AnyObject = msg_send![class!(NSEvent),
                mouseEventWithType: kind
                location: NSPoint::new(10., 10.)
                modifierFlags: 0usize
                timestamp: 0.0f64
                windowNumber: 0isize
                context: std::ptr::null::<AnyObject>()
                eventNumber: 0isize
                clickCount: 1isize
                pressure: 1.0f32];
            let _: () = msg_send![app, sendEvent: event];
        }
        let until: *mut AnyObject = msg_send![class!(NSDate), dateWithTimeIntervalSinceNow: 0.2f64];
        let run_loop: *mut AnyObject = msg_send![class!(NSRunLoop), currentRunLoop];
        let _: () = msg_send![run_loop, runUntilDate: until];
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("This check is for the Mac.");
}
