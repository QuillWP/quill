//! A record of every panic, kept beside the settings. A Mac app opened from
//! the Finder has nowhere to print, and a release build stops at once on a
//! panic, so without this file the message and its source line are lost.

use std::io::Write;

/// Add each panic to the crash log, then report it as before.
pub fn install() {
    let Some(path) = crate::settings::sibling("Crash Log", "Quill Crash Log.txt", "crash-log")
    else {
        return;
    };
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let now = crate::clock::now();
        let entry = format!(
            "{} {} - Quill {}{}, thread {}\n{info}\n{}\n\n",
            now.iso_date(),
            now.time_with_seconds(),
            env!("CARGO_PKG_VERSION"),
            if cfg!(debug_assertions) {
                " (debug build)"
            } else {
                ""
            },
            std::thread::current().name().unwrap_or("unnamed"),
            std::backtrace::Backtrace::force_capture(),
        );
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = file.write_all(entry.as_bytes());
        }
        crate::ev!("PANIC: {info}");
        previous(info);
    }));
}
