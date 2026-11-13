//! An optional step-by-step log for finding where Quill hangs or goes wrong
//! when no panic is raised. Off unless started with `--with-events`.
//!
//! Every line is written and flushed at once, so after a freeze the end of
//! the file is the last thing the program did. A watchdog thread adds a
//! line each two seconds while the program is busy and has logged nothing,
//! saying how long it has been stuck and after which step. Identical lines
//! in a row are collapsed, so a tight loop leaves a short file that names
//! the loop. See `quill --help`.

use std::fs::File;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

static ENABLED: AtomicBool = AtomicBool::new(false);
static ALL: AtomicBool = AtomicBool::new(false);
static STATE: OnceLock<Mutex<State>> = OnceLock::new();

/// A line stops being repeated in the file after this many in a row, and
/// is summarized when something else is logged.
const REPEAT_SHOWN: u64 = 3;
/// How long the program may be busy without a log line before the
/// watchdog speaks.
const STUCK_AFTER: Duration = Duration::from_secs(2);

struct State {
    file: Option<File>,
    start: Instant,
    /// The last line logged, how many times in a row, and when.
    last: String,
    repeats: u64,
    last_at: Instant,
    /// False while the event loop is waiting for something to happen, so
    /// that quiet is not taken for a freeze.
    busy: bool,
    /// When the watchdog last spoke about the current stall.
    warned_at: Option<Instant>,
}

/// Whether the log is on.
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Whether the very detailed log (`--with-events=all`) is on: mouse moves,
/// redraws and every wake of the event loop.
pub fn detailed() -> bool {
    ALL.load(Ordering::Relaxed)
}

/// Where the log is written: the file given with `--events-file=PATH`, else
/// `QUILL_EVENTS_FILE`, else `quill-events.log` in the folder Quill was
/// started from, so it is easy to find. Where that cannot be written, the
/// settings folder.
fn path(args: &[String]) -> PathBuf {
    if let Some(p) = args.iter().find_map(|a| a.strip_prefix("--events-file=")) {
        return PathBuf::from(p);
    }
    if let Some(p) = std::env::var_os("QUILL_EVENTS_FILE") {
        return PathBuf::from(p);
    }
    let here = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("quill-events.log");
    if File::create(&here).is_ok() {
        return here;
    }
    crate::settings::data_folder()
        .unwrap_or_else(std::env::temp_dir)
        .join("quill-events.log")
}

/// Turn the log on. The file is started afresh each run. Returns where it is.
pub fn start(detailed: bool, args: &[String]) -> PathBuf {
    let path = path(args);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = File::create(&path).ok();
    let now = Instant::now();
    let state = State {
        file,
        start: now,
        last: String::new(),
        repeats: 0,
        last_at: now,
        busy: true,
        warned_at: None,
    };
    if STATE.set(Mutex::new(state)).is_err() {
        return path;
    }
    ALL.store(detailed, Ordering::Relaxed);
    ENABLED.store(true, Ordering::Relaxed);
    log(&format!(
        "event log started: Quill {}{} on {} {}{}",
        env!("CARGO_PKG_VERSION"),
        if cfg!(debug_assertions) {
            " (debug build)"
        } else {
            ""
        },
        std::env::consts::OS,
        std::env::consts::ARCH,
        if detailed { ", detailed" } else { "" },
    ));
    log(&format!(
        "build: {} {}{}",
        option_env!("QUILL_BUILD_BRANCH").unwrap_or("?"),
        option_env!("QUILL_BUILD_COMMIT").unwrap_or("?"),
        if option_env!("QUILL_BUILD_DIRTY") == Some("1") {
            " (with uncommitted changes)"
        } else {
            ""
        },
    ));
    log(&format!("arguments: {args:?}"));
    for key in [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_SESSION_TYPE",
        "QUILL_BACKEND",
    ] {
        if let Some(v) = std::env::var_os(key) {
            log(&format!("environment {key}={}", v.to_string_lossy()));
        }
    }
    let _ = std::thread::Builder::new()
        .name("event-log watchdog".into())
        .spawn(watchdog);
    path
}

fn write(state: &mut State, text: &str) {
    let line = format!(
        "[{:>9.3}s {}] {text}\n",
        state.start.elapsed().as_secs_f64(),
        std::thread::current().name().unwrap_or("?"),
    );
    if let Some(f) = &mut state.file {
        let _ = f.write_all(line.as_bytes());
        let _ = f.flush();
    }
    eprint!("{line}");
}

/// The first part of a long description, for one-line log entries.
pub fn short(text: &str) -> String {
    const MAX: usize = 220;
    if text.chars().count() <= MAX {
        return text.to_string();
    }
    let cut: String = text.chars().take(MAX).collect();
    format!("{cut}...")
}

/// Log a window event. Mouse moves, redraws and the like are left out unless
/// the detailed log is on, since they come by the hundred.
pub fn window_event(event: &winit::event::WindowEvent) {
    use winit::event::WindowEvent as E;
    if !enabled() {
        return;
    }
    let noisy = matches!(
        event,
        E::CursorMoved { .. }
            | E::RedrawRequested
            | E::AxisMotion { .. }
            | E::CursorEntered { .. }
            | E::CursorLeft { .. }
            | E::Moved(_)
    );
    if noisy && !detailed() {
        busy();
        return;
    }
    log(&format!("window event: {}", short(&format!("{event:?}"))));
}

/// Log one line. Use `ev!` so nothing is formatted when the log is off.
pub fn log(text: &str) {
    let Some(state) = STATE.get() else { return };
    let Ok(mut s) = state.lock() else { return };
    s.busy = true;
    s.last_at = Instant::now();
    s.warned_at = None;
    if s.last == text {
        s.repeats += 1;
        if s.repeats <= REPEAT_SHOWN || s.repeats.is_power_of_two() && s.repeats >= 1024 {
            let note = if s.repeats > REPEAT_SHOWN {
                format!("{text}  (x{} in a row so far)", s.repeats)
            } else {
                text.to_string()
            };
            write(&mut s, &note);
        }
        return;
    }
    if s.repeats > REPEAT_SHOWN {
        let (last, n) = (s.last.clone(), s.repeats);
        write(
            &mut s,
            &format!("(the line above, \"{last}\", came {n} times in a row)"),
        );
    }
    s.last = text.to_string();
    s.repeats = 1;
    write(&mut s, text);
}

/// Note that the event loop is about to wait for something to happen.
pub fn idle() {
    if let Some(state) = STATE.get()
        && let Ok(mut s) = state.lock()
    {
        s.busy = false;
        s.warned_at = None;
    }
}

/// Note that the event loop has woken, without logging a line.
pub fn busy() {
    if let Some(state) = STATE.get()
        && let Ok(mut s) = state.lock()
    {
        s.busy = true;
        s.last_at = Instant::now();
        s.warned_at = None;
    }
}

/// Marks the loop idle when dropped, so every way out of a function counts.
pub struct IdleOnDrop;

impl Drop for IdleOnDrop {
    fn drop(&mut self) {
        idle();
    }
}

/// Run `f`, logging when it begins and how long it took. A call that never
/// returns shows as a "begin" with no "end".
pub fn timed<R>(what: &str, f: impl FnOnce() -> R) -> R {
    if !enabled() {
        return f();
    }
    log(&format!("begin: {what}"));
    let at = Instant::now();
    let r = f();
    log(&format!(
        "end: {what} ({:.0} ms)",
        at.elapsed().as_secs_f64() * 1000.0
    ));
    r
}

fn watchdog() {
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let Some(state) = STATE.get() else { return };
        let Ok(mut s) = state.lock() else { return };
        if !s.busy {
            continue;
        }
        let now = Instant::now();
        let quiet = now.duration_since(s.last_at);
        if quiet < STUCK_AFTER
            || s.warned_at
                .is_some_and(|w| now.duration_since(w) < STUCK_AFTER)
        {
            continue;
        }
        s.warned_at = Some(now);
        let last = s.last.clone();
        let text = format!(
            "!! STUCK: busy for {:.0}s without logging anything. The last step was: {last}",
            quiet.as_secs_f64()
        );
        write(&mut s, &text);
    }
}

/// Log a line when the event log is on.
#[macro_export]
macro_rules! ev {
    ($($arg:tt)*) => {
        if $crate::events::enabled() {
            $crate::events::log(&format!($($arg)*));
        }
    };
}

/// Log a line only when the very detailed log is on.
#[macro_export]
macro_rules! ev_all {
    ($($arg:tt)*) => {
        if $crate::events::detailed() {
            $crate::events::log(&format!($($arg)*));
        }
    };
}
