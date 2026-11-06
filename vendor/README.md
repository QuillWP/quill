# Patched dependencies

Quill builds these crates from the copies here instead of crates.io, through
`[patch.crates-io]` in `Cargo.toml` and `fuzz/Cargo.toml`. Each copy is the
published crate with only the changes listed below. Read this before
upgrading one, and carry the change forward or confirm the new release
makes it unnecessary.

## winit 0.30.13

Source: the crates.io release, unchanged except for one file. The change is
marked `QUILL PATCH` in the code. Quill's own modal calls (mac_save.rs,
mac_print.rs, the message boxes in app.rs and the Open panel in
app/files.rs) carry a comment pointing here.

### What goes wrong without it

Quill opens Save, Open, Print and its alerts with AppKit's `runModal`, from
inside winit's event handler, and waits there until the panel closes. While
the panel is up, AppKit still passes mouse moves, clicks and drags made in
Quill's own process (dragging the Save panel's edge, for one) through winit's
`sendEvent:` override. winit sees its handler is busy and sets the event
aside on the run loop, but the panel's modal loop runs that queue straight
away, and winit then panics rather than enter the handler twice:

    tried to handle event while another event is currently being handled

The app aborts and unsaved work is lost. Both crashes reported on
2026-10-28, while printing and in the Save panel, were this. winit has no
setting to avoid it (device events cannot be turned off on the Mac), and
0.31.0-beta.4 queues events the same way.

### The change

`src/platform_impl/macos/app_state.rs`: an event that was set aside, and
finds the handler still busy when the queue runs, is held in a list instead
of being handed over. The held events are delivered, in order, the next
time the run loop is about to wait and the handler is free, which is just
after the panel closes and Quill's handler returns.

Not covered: a window moving to a screen with a different scale factor
while a panel is open still goes straight to the handler. That needs a
window to change displays during a modal panel.

### Upgrading winit

1. Check whether the new release still hands set-aside events to a busy
   handler (`maybe_queue_event` in the macOS backend, `app_state.rs`). If it
   no longer does, remove `vendor/winit` and both `[patch.crates-io]` entries.
2. Otherwise copy the new release here, apply the same change (the code
   marked `QUILL PATCH` in `app_state.rs`), and update the version above.
3. Run `cargo run -p quill-app --example modal_events`. It sends a click
   through AppKit while winit's handler is busy and spins the run loop, as
   AppKit does during the Save panel. It must print "back in the handler
   without a crash" and report the held clicks delivered afterwards;
   unpatched winit aborts there with the panic above.
4. Then open Save As in the app, drag the panel's edge and move the pointer
   over the document window: Quill must not crash.
