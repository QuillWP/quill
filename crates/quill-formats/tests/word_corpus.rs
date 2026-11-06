//! Quill against Word: every .docx Word for Mac saved from LibreOffice's
//! Writer test files (sw/qa/extras), each beside the text Word wrote of it.
//! For each file the test checks that Quill reads it, how much of Word's
//! text Quill's reading has, and how much changes when Quill saves it as
//! .docx, .rtf and .qwp and reads that back.
//!
//! The corpus lives outside the repository, so the test only runs when
//! QUILL_WORD_OUTPUT names its "Word output" folder:
//!
//!   QUILL_WORD_OUTPUT=".../Word output" cargo test --release -p quill-formats --test word_corpus
//!
//! word_corpus.tsv beside this file records each file's results. A file
//! doing worse than it did fails the test; one doing better is reported.
//! QUILL_BLESS=1 writes the results as the new record.
//!
//! QUILL_MATCH=text checks only the files whose path has that text in it,
//! and lists what each of their round trips changed.

use std::collections::{BTreeMap, HashMap};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use quill_core::Document;
use quill_core::para::chars;
use quill_formats::{docx, native, rtf};

mod common;

const COLUMNS: [&str; 5] = ["load", "kept", "docx", "rtf", "quill"];

/// The text of everything in `doc` that Word's own text file holds, laid
/// out the way Word writes it: the main text, then the notes, the
/// annotations and every section's headers and footers, one paragraph a
/// line, with note marks as their numbers.
fn quill_text(doc: &Document) -> String {
    let labels = doc.note_labels(None);
    let mut out = String::new();
    // Word writes every section's headers and footers, even ones the
    // section takes from the one before. Later sections' own sit on their
    // section breaks, and the last section's on the document.
    let mut sections: Vec<&[quill_core::Story]> = vec![&doc.headers];
    let breaks = doc
        .main
        .paras
        .iter()
        .filter_map(|p| p.section.as_deref())
        .chain([&doc.section])
        .skip(1);
    for s in breaks {
        let last = *sections.last().unwrap_or(&&doc.headers[..]);
        sections.push(s.headers.as_ref().map_or(last, |h| &h[..]));
    }
    let stories = std::iter::once(&doc.main)
        .chain([&doc.notes, &doc.annotations])
        .chain(sections.into_iter().flatten());
    for story in stories {
        // Word's text is the document with its tracked changes accepted.
        for p in &quill_core::tracking::accepted(&story.paras) {
            for (s, e, chp) in p.run_bounds() {
                let mut run = String::new();
                for c in p.text[s..e].chars() {
                    match c {
                        chars::FOOTNOTE => {
                            run.push_str(labels.get(&chp.object).map_or("", |l| l.as_str()))
                        }
                        // Quill's other marks (fields, pictures) are not
                        // text Word would write.
                        '\u{E000}'..='\u{E0FF}' | chars::OBJECT => {}
                        c => run.push(c),
                    }
                }
                // Word writes All Caps and Small Caps text in capitals.
                if chp.all_caps || chp.small_caps {
                    out.push_str(&run.to_uppercase());
                } else {
                    out.push_str(&run);
                }
            }
            out.push('\n');
        }
    }
    out
}

/// Bytes 0x80 to 0xFF in Mac Roman.
const MAC_ROMAN: &str = "ÄÅÇÉÑÖÜáàâäãåçéèêëíìîïñóòôöõúùûü†°¢£§•¶ß®©™´¨≠ÆØ∞±≤≥¥µ∂∑∏π∫ªºΩæø¿¡¬√ƒ≈∆«»…\u{00A0}ÀÃÕŒœ\u{2013}\u{2014}“”‘’÷◊ÿŸ⁄€‹›ﬁﬂ‡·‚„‰ÂÊÁËÈÍÎÏÌÓÔ\u{F8FF}ÒÚÛÙıˆ˜¯˘˙˚¸˝˛ˇ";

fn utf16(data: &[u8], big_endian: bool) -> String {
    let units: Vec<u16> = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| {
            if big_endian {
                u16::from_be_bytes([b[0], b[1]])
            } else {
                u16::from_le_bytes([b[0], b[1]])
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}

/// Word's text file. It's UTF-16 when it starts with a byte-order mark, and
/// also without one when Word couldn't put the text in UTF-8 (then every
/// other byte of plain letters is a zero). Otherwise it's UTF-8, or Mac
/// Roman for the odd old file.
fn read_word_text(data: &[u8]) -> String {
    if let Some(rest) = data.strip_prefix(b"\xff\xfe") {
        return utf16(rest, false);
    }
    if let Some(rest) = data.strip_prefix(b"\xfe\xff") {
        return utf16(rest, true);
    }
    let zeros = |skip| {
        data.iter()
            .skip(skip)
            .step_by(2)
            .filter(|&&b| b == 0)
            .count()
    };
    if data.len() >= 2 && zeros(1) > data.len() / 4 {
        return utf16(data, false);
    }
    if data.len() >= 2 && zeros(0) > data.len() / 4 {
        return utf16(data, true);
    }
    let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
    match std::str::from_utf8(data) {
        Ok(s) => s.to_string(),
        Err(_) => {
            let high: Vec<char> = MAC_ROMAN.chars().collect();
            data.iter()
                .map(|&b| {
                    if b < 0x80 {
                        b as char
                    } else {
                        high[b as usize - 0x80]
                    }
                })
                .collect()
        }
    }
}

/// Word's text file marks each comment as [initials number], like
/// "[Unknown A1]". Those letters aren't the document's text.
fn strip_comment_marks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('[') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let inner = after
            .find(['[', ']', '\r', '\n'])
            .filter(|&end| after[end..].starts_with(']'))
            .map(|end| &after[..end]);
        let is_mark = inner.is_some_and(|s| {
            s.chars().count() <= 120
                && s.ends_with(|c: char| c.is_ascii_digit())
                && s.chars().take(41).any(|c| c.is_ascii_alphabetic())
        });
        match inner {
            Some(s) if is_mark => rest = &after[s.len() + 1..],
            _ => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Each letter and digit, counted. Spacing, punctuation and line ends are
/// left out because Word and Quill write those differently.
fn letters(text: &str) -> HashMap<char, usize> {
    let mut out = HashMap::new();
    for c in text.chars().filter(|c| c.is_alphanumeric()) {
        *out.entry(c).or_default() += 1;
    }
    out
}

/// How much of Word's text Quill also has, as a percentage, or None when
/// Word's has no letters to compare.
fn kept_share(word: &str, quill: &str) -> Option<f64> {
    let want = letters(&strip_comment_marks(word));
    let have = letters(quill);
    let total: usize = want.values().sum();
    if total == 0 {
        return None;
    }
    let kept: usize = want
        .iter()
        .map(|(c, &n)| n.min(have.get(c).copied().unwrap_or(0)))
        .sum();
    Some(100.0 * kept as f64 / total as f64)
}

/// Run `f`, turning a panic into an error.
fn guarded<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, &'static str> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(_)) => Err("error"),
        Err(_) => Err("panic"),
    }
}

type Writer = fn(&Document) -> Vec<u8>;
type Reader = fn(&[u8]) -> Result<Document, String>;

/// The formats each file is saved in and read back from, in `COLUMNS`
/// order.
const FORMATS: [(&str, Writer, Reader); 3] = [
    ("docx", |d| docx::write(d, 0), docx::read),
    ("rtf", rtf::write, rtf::read),
    ("quill", |d| native::write(d, 0), native::read),
];

/// Save `doc` with `write`, read it back with `read`, and list what
/// changed: each paragraph's text, runs and settings, the styles and the
/// document's own settings, as round_trip.rs compares them.
fn round_trip(doc: &Document, write: Writer, read: Reader) -> Result<Vec<String>, &'static str> {
    guarded(|| read(&write(doc)).map(|back| common::differences(doc, &back)))
}

/// One file's results, in `COLUMNS` order.
fn check(docx_path: &Path) -> Vec<String> {
    let word = std::fs::read(docx_path.with_extension("txt"))
        .ok()
        .map(|b| read_word_text(&b));
    let share = |quill: &str| {
        word.as_deref()
            .and_then(|w| kept_share(w, quill))
            .map_or("-".to_string(), |s| format!("{s:.1}"))
    };
    let loaded = guarded(|| {
        let bytes = std::fs::read(docx_path).map_err(|e| e.to_string())?;
        docx::read(&bytes)
    });
    let doc = match loaded {
        Ok(doc) => doc,
        Err(e) => return vec![e.into(), share(""), "-".into(), "-".into(), "-".into()],
    };
    let Ok(text) = guarded(|| Ok(quill_text(&doc))) else {
        return vec![
            "panic".into(),
            share(""),
            "-".into(),
            "-".into(),
            "-".into(),
        ];
    };
    let mut row = vec!["ok".into(), share(&text)];
    for (_, write, read) in FORMATS {
        row.push(match round_trip(&doc, write, read) {
            Ok(changes) => changes.len().to_string(),
            Err(e) => e.into(),
        });
    }
    row
}

/// List what saving the file in each format and reading it back changed.
fn show_changes(path: &Path) {
    let Ok(doc) = guarded(|| docx::read(&std::fs::read(path).map_err(|e| e.to_string())?)) else {
        return;
    };
    for (name, write, read) in FORMATS {
        let Ok(changes) = round_trip(&doc, write, read) else {
            continue;
        };
        if !changes.is_empty() {
            eprintln!(
                "{} through .{name}:\n{}\n",
                path.display(),
                changes.join("\n")
            );
        }
    }
}

/// How good a result is, higher being better, so a lower one is a
/// regression. A round trip's count of changes is better the lower it is,
/// and any count beats an error.
fn rank(column: &str, value: &str) -> f64 {
    match (column, value.parse::<f64>()) {
        ("kept", Ok(v)) => v,
        ("kept", Err(_)) => f64::NAN,
        (_, Ok(n)) => -n,
        _ => match value {
            "ok" => 0.0,
            "error" => -1e9,
            _ => -2e9,
        },
    }
}

/// Every .docx under `dir`, skipping the "._" files macOS leaves on
/// non-Mac disks.
fn docx_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("._") {
            continue;
        }
        if path.is_dir() {
            docx_files(&path, out);
        } else if name.ends_with(".docx") {
            out.push(path);
        }
    }
}

fn record_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/word_corpus.tsv")
}

fn read_record() -> BTreeMap<String, Vec<String>> {
    let text = std::fs::read_to_string(record_path()).unwrap_or_default();
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let file = fields.next()?.to_string();
            Some((file, fields.map(String::from).collect()))
        })
        .collect()
}

fn write_record(results: &BTreeMap<String, Vec<String>>) {
    let mut out = format!("file\t{}\n", COLUMNS.join("\t"));
    for (file, row) in results {
        out.push_str(&format!("{file}\t{}\n", row.join("\t")));
    }
    std::fs::write(record_path(), out).expect("write word_corpus.tsv");
}

/// Check every file, several at once, and say which ones are taking a long
/// time so a hang can be found.
fn check_all(root: &Path, files: &[PathBuf]) -> BTreeMap<String, Vec<String>> {
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let results = Mutex::new(BTreeMap::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    let busy: Vec<Mutex<Option<(usize, Instant)>>> =
        (0..workers).map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for slot in &busy {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = files.get(i) else { break };
                    *slot.lock().unwrap() = Some((i, Instant::now()));
                    let row = check(path);
                    *slot.lock().unwrap() = None;
                    let rel = path.strip_prefix(root).unwrap_or(path);
                    let rel = rel.to_string_lossy().replace('\\', "/");
                    results.lock().unwrap().insert(rel, row);
                    done.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
        let mut reported = Vec::new();
        while done.load(Ordering::Relaxed) < files.len() {
            std::thread::sleep(Duration::from_millis(200));
            for slot in &busy {
                if let Some((i, since)) = *slot.lock().unwrap()
                    && since.elapsed() > Duration::from_secs(30)
                    && !reported.contains(&i)
                {
                    eprintln!("still working on {} after 30 s", files[i].display());
                    reported.push(i);
                }
            }
        }
    });
    results.into_inner().unwrap()
}

#[test]
fn word_saved_files() {
    let Some(root) = std::env::var_os("QUILL_WORD_OUTPUT").map(PathBuf::from) else {
        eprintln!("QUILL_WORD_OUTPUT is not set, so the Word corpus was not checked");
        return;
    };
    let mut files = Vec::new();
    docx_files(&root, &mut files);
    let only = std::env::var("QUILL_MATCH").ok();
    if let Some(only) = &only {
        files.retain(|f| {
            f.strip_prefix(&root)
                .unwrap_or(f)
                .to_string_lossy()
                .contains(only)
        });
    }
    assert!(!files.is_empty(), "no .docx files under {}", root.display());

    // Panics are expected and counted, so keep their messages short.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        let at = info.location().map(|l| l.to_string()).unwrap_or_default();
        eprintln!("panic at {at}");
    }));
    let started = Instant::now();
    let results = check_all(&root, &files);
    std::panic::set_hook(hook);

    // A summary of every column.
    eprintln!("{} files in {:.0?}", results.len(), started.elapsed());
    for (c, column) in COLUMNS.iter().enumerate() {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for row in results.values() {
            let value = row[c].as_str();
            let bucket = match (*column, value.parse::<f64>()) {
                ("kept", Ok(v)) if v >= 98.0 => "98% or more",
                ("kept", Ok(_)) => "below 98%",
                ("kept", Err(_)) => "no Word text",
                ("load", _) => value,
                (_, Ok(0.0)) => "unchanged",
                (_, Ok(_)) => "changed",
                _ => value,
            };
            *counts.entry(bucket).or_default() += 1;
        }
        let counts: Vec<String> = counts.iter().map(|(k, n)| format!("{k} {n}")).collect();
        eprintln!("  {column}: {}", counts.join(", "));
    }

    if only.is_some() {
        for path in &files {
            show_changes(path);
        }
    }

    let record = read_record();
    if std::env::var_os("QUILL_BLESS").is_some() {
        // A run of some of the files keeps the record of the others.
        let mut new = if only.is_some() {
            record
        } else {
            BTreeMap::new()
        };
        new.extend(results);
        write_record(&new);
        eprintln!("recorded in {}", record_path().display());
        return;
    }

    let mut worse = Vec::new();
    let mut better = 0;
    for (file, row) in &results {
        let Some(before) = record.get(file) else {
            worse.push(format!("{file}: not in word_corpus.tsv"));
            continue;
        };
        for (c, column) in COLUMNS.iter().enumerate() {
            let (was, now) = (before.get(c).map_or("", |s| s), row[c].as_str());
            let (a, b) = (rank(column, was), rank(column, now));
            if b < a {
                worse.push(format!("{file}: {column} {was} -> {now}"));
            } else if b > a {
                better += 1;
            }
        }
    }
    let gone = match only {
        Some(_) => 0,
        None => record.keys().filter(|f| !results.contains_key(*f)).count(),
    };
    if better > 0 || gone > 0 {
        eprintln!(
            "{better} results better and {gone} files gone since word_corpus.tsv; \
             QUILL_BLESS=1 records them"
        );
    }
    assert!(
        worse.is_empty(),
        "{} results worse than word_corpus.tsv:\n{}",
        worse.len(),
        worse.join("\n")
    );
}

#[test]
fn word_text_files_are_read_in_their_own_encoding() {
    assert_eq!(read_word_text(b"\xef\xbb\xbfcaf\xc3\xa9"), "café");
    assert_eq!(read_word_text(b"\xff\xfeh\0i\0"), "hi");
    assert_eq!(read_word_text(b"h\0e\0l\0l\0o\0"), "hello");
    assert_eq!(read_word_text(b"\0h\0e\0l\0l\0o"), "hello");
    assert_eq!(read_word_text(b"caf\x8e"), "café");
    assert_eq!(MAC_ROMAN.chars().count(), 128);
}

#[test]
fn comment_marks_and_spacing_do_not_count() {
    assert_eq!(
        strip_comment_marks("See [Unknown A1]this[1] and [x] [AB12]."),
        "See this[1] and [x] ."
    );
    assert_eq!(kept_share("Hello, world [JS3]", "Hello\nwor"), Some(80.0));
    assert_eq!(kept_share("  ...  ", "anything"), None);
}
