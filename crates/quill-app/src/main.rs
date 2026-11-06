//! Quill: a word processor in the spirit of Lucas's favorite word processor from 1992.
// A Windows release build is a GUI program, so starting it opens no console
// window. Debug builds keep the console for their diagnostic output.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod bars;
mod border_dialog;
mod canvas;
mod chrome;
mod clock;
mod color_glyph;
mod commands;
mod commands_dialog;
mod contents_dialogs;
mod crash_log;
mod customize;
mod dialog;
mod docview;
mod find_dialog;
mod font_names;
mod frame_dialog;
mod glossary;
mod glossary_file;
mod icon;
mod image;
mod lang_dialogs;
mod language;
mod licenses;
mod link_dialog;
mod listbox;
mod local_dictionary;
#[cfg(target_os = "macos")]
#[cfg(target_os = "macos")]
mod mac_dialog;
#[cfg(target_os = "macos")]
mod mac_hyphen;
#[cfg(target_os = "macos")]
mod mac_licenses;
#[cfg(target_os = "macos")]
mod mac_lookup;
#[cfg(target_os = "macos")]
mod mac_open;
#[cfg(target_os = "macos")]
mod mac_print;
#[cfg(target_os = "macos")]
mod mac_save;
#[cfg(target_os = "macos")]
mod mac_text_input;
#[cfg(target_os = "macos")]
mod mac_widgets;
mod menu;
mod merge_dialogs;
#[cfg(target_os = "macos")]
mod native_menu;
mod outline_bar;
mod page_dialogs;
mod palette;
mod pane;
mod paste_special;
mod pdf;
mod pdf_font;
use quill_formats::png;
mod app_menus;
mod format_dialogs;
mod prefs_dialog;
mod print_job;
mod recent;
mod recovery;
mod regional;
mod review_dialog;
mod settings;
mod spellcheck;
mod style_dialog;
mod system_clipboard;
mod table_dialogs;
mod tool_dialogs;
mod toolbar;
#[cfg(windows)]
mod win_dialog;
#[cfg(windows)]
mod win_print;

use quill_layout::Catalog;
use winit::event_loop::EventLoop;

fn main() {
    if std::env::args().any(|a| a == "--version") {
        println!("Quill {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    // Tracked changes are signed with Preferences' Your Name and dated in
    // local time, as Word does.
    quill_core::revision::set_author_source(|| settings::with(|s| s.name.clone()));
    quill_core::revision::set_clock(|| clock::now().change_date());
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--snapshot") {
        snapshot(&args[i + 1..]);
        return;
    }
    settings::load();
    crash_log::install();
    // Finding the installed fonts can take a moment on a cold start, so do
    // it while the window system starts up.
    let fonts = std::thread::spawn(Catalog::scan_system);
    let mut builder = EventLoop::<app::UserEvent>::with_user_event();
    // Quill puts up its own menu bar; skip winit's placeholder one.
    #[cfg(target_os = "macos")]
    winit::platform::macos::EventLoopBuilderExtMacOS::with_default_menu(&mut builder, false);
    #[cfg(windows)]
    winit::platform::windows::EventLoopBuilderExtWindows::with_msg_hook(
        &mut builder,
        win_dialog::message_hook,
    );
    let event_loop = match builder.build() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("quill: cannot start the window system: {e}");
            std::process::exit(1);
        }
    };
    let files = args[1..]
        .iter()
        .filter(|a| !a.starts_with('-'))
        .map(std::path::PathBuf::from)
        .collect();
    let proxy = event_loop.create_proxy();
    #[cfg(target_os = "macos")]
    mac_open::install(event_loop.create_proxy());
    #[cfg(target_os = "macos")]
    mac_dialog::install(event_loop.create_proxy());
    #[cfg(target_os = "macos")]
    mac_widgets::install(event_loop.create_proxy());
    let mut app = app::App::new(fonts, files, proxy);
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("quill: {e}");
        std::process::exit(1);
    }
}

/// `quill --snapshot out.png [--size 720x820] [--scale 2] [--open file]
/// [--script steps] [--settings file]`: draw a window offscreen. See
/// `app::snapshot`. `--settings` draws with a settings file's values
/// without writing to it.
fn snapshot(args: &[String]) {
    let Some(out) = args.first() else {
        eprintln!(
            "usage: quill --snapshot out.png [--size WxH] [--scale N] [--open FILE] [--script STEPS] [--settings FILE]"
        );
        std::process::exit(2);
    };
    let opt = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
    };
    let (w, h) = opt("--size")
        .and_then(|s| s.split_once('x'))
        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
        .unwrap_or((720, 820));
    let scale = opt("--scale").and_then(|s| s.parse().ok()).unwrap_or(2.0);
    let doc = opt("--open").map(|p| {
        quill_formats::load(std::path::Path::new(p)).unwrap_or_else(|e| {
            eprintln!("quill: cannot open {p}: {e}");
            std::process::exit(1);
        })
    });
    if let Some(p) = opt("--settings") {
        let text = std::fs::read_to_string(p).unwrap_or_else(|e| {
            eprintln!("quill: cannot read {p}: {e}");
            std::process::exit(1);
        });
        // Snapshots never save settings, so this only changes the drawing.
        settings::update(|s| *s = settings::Settings::from_text(&text));
    }
    let script = opt("--script").cloned().unwrap_or_default();
    app::snapshot(std::path::Path::new(out), w, h, scale, doc, &script);
}
