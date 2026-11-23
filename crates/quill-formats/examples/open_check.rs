//! Open documents the way the app does and say what happened to each, for
//! tools/overnight.py: `cargo run --release -p quill-formats --example
//! open_check [FILE...]`. With no files named, it reads one path per line
//! from standard input and answers each before reading the next, so the
//! runner can time each file and start a new process after a hang.
//!
//! For each file it prints `start` and the path, then one tab-separated
//! result line: `done`, the outcome (ok, refused or panic), milliseconds,
//! paragraphs, whether a .qwp save of it opens again (yes, no or -), the
//! import notes, the error or panic message, and the path.

use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

/// The message of the last panic, kept by the panic hook.
static PANIC: Mutex<String> = Mutex::new(String::new());

/// Tabs and line breaks would split a result line, so they become spaces.
fn field(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    flat.chars().take(300).collect()
}

fn check(path: &Path) -> String {
    let start = Instant::now();
    let result = std::panic::catch_unwind(|| {
        quill_formats::load_reported(path, true).map(|(doc, report)| {
            let saved = quill_formats::native::write(&doc, 0);
            let reopens = quill_formats::native::read(&saved).is_ok();
            (doc.main.paras.len(), reopens, report.notes().join(" | "))
        })
    });
    let ms = start.elapsed().as_millis();
    let (outcome, paras, reopens, notes, detail) = match result {
        Ok(Ok((paras, reopens, notes))) => (
            "ok",
            paras.to_string(),
            if reopens { "yes" } else { "no" },
            notes,
            String::new(),
        ),
        Ok(Err(error)) => ("refused", "-".into(), "-", String::new(), error),
        Err(_) => {
            let message = PANIC.lock().map(|m| m.clone()).unwrap_or_default();
            ("panic", "-".into(), "-", String::new(), message)
        }
    };
    format!(
        "done\t{outcome}\t{ms}\t{paras}\t{reopens}\t{}\t{}\t{}",
        field(&notes),
        field(&detail),
        path.display()
    )
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let place = info
            .location()
            .map(|l| format!(" at {}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        if let Ok(mut kept) = PANIC.lock() {
            *kept = format!("{message}{place}");
        }
    }));
    let out = std::io::stdout();
    let answer = |path: &str| {
        let mut out = out.lock();
        let _ = writeln!(out, "start\t{path}");
        let _ = out.flush();
        let line = check(Path::new(path));
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    };
    let named: Vec<String> = std::env::args().skip(1).collect();
    if named.is_empty() {
        for line in std::io::stdin().lock().lines() {
            let Ok(path) = line else { break };
            if !path.is_empty() {
                answer(&path);
            }
        }
    } else {
        for path in &named {
            answer(path);
        }
    }
}
