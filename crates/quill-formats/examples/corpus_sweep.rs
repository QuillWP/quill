//! Load every DOCX, RTF and ODT file (and flat ODT) under a folder and report what
//! happened to each: `cargo run -p quill-formats --example corpus_sweep DIR`.
//!
//! Each line is the outcome, the extension, the time taken, the paragraph
//! count and, for ODT packages, how many of the body's letters and digits
//! the document kept (as a percentage), the import notes Quill would show,
//! then the path.

use quill_formats::xml::{Element, Node};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if let Some(ext) = p.extension().and_then(|e| e.to_str())
            && [
                "docx", "docm", "dotx", "dotm", "rtf", "odt", "fodt", "ott", "odm", "html", "htm",
                "xhtml",
            ]
            .contains(&ext.to_lowercase().as_str())
        {
            out.push(p);
        }
    }
}

/// Letters and digits in an ODT package's body, skipping what is never
/// shown: deleted text and index templates.
fn body_chars(path: &Path) -> Option<usize> {
    fn count(e: &Element) -> usize {
        // Comment authors and dates, note numbers and fields Quill keeps as
        // live fields are not text to keep.
        let skip = [
            "tracked-changes",
            "creator",
            "creator-initials",
            "sender-initials",
            "date",
            "time",
            "note-citation",
            "page-number",
            "title",
            "desc",
        ];
        if skip.contains(&e.name.as_str()) || e.name.ends_with("-source") {
            return 0;
        }
        e.children
            .iter()
            .map(|n| match n {
                Node::Text(t) => t.chars().filter(|c| c.is_alphanumeric()).count(),
                Node::Element(c) => count(c),
            })
            .sum()
    }
    let bytes = std::fs::read(path).ok()?;
    let zip = quill_formats::zip::Archive::open(&bytes).ok()?;
    if let Ok(Some(word)) = zip.read("word/document.xml") {
        return Some(docx_chars(&quill_formats::xml::parse(&word).ok()?));
    }
    let content = quill_formats::xml::parse(&zip.read("content.xml").ok()??).ok()?;
    Some(count(content.child("body")?))
}

/// Letters and digits in a Word document's text runs, skipping deleted
/// text, field codes and the old-style copy of each drawing.
fn docx_chars(e: &Element) -> usize {
    if matches!(
        e.name.as_str(),
        "del" | "moveFrom" | "instrText" | "Fallback"
    ) {
        return 0;
    }
    e.children
        .iter()
        .map(|n| match n {
            Node::Text(t) if e.name == "t" => t.chars().filter(|c| c.is_alphanumeric()).count(),
            Node::Element(c) => docx_chars(c),
            Node::Text(_) => 0,
        })
        .sum()
}

/// Letters and digits Quill kept, in the main text, notes and annotations.
fn kept_chars(doc: &quill_core::Document) -> usize {
    [&doc.main, &doc.notes, &doc.annotations]
        .iter()
        .flat_map(|s| s.paras.iter())
        .map(|p| p.text.chars().filter(|c| c.is_alphanumeric()).count())
        .sum()
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: corpus_sweep DIR");
    let mut files = Vec::new();
    walk(Path::new(&dir), &mut files);
    files.sort();
    std::panic::set_hook(Box::new(|_| {}));
    for f in files {
        let (tx, rx) = mpsc::channel();
        let path = f.clone();
        std::thread::spawn(move || {
            let start = Instant::now();
            let r = std::panic::catch_unwind(|| {
                quill_formats::load_reported(&path, true).map(|(doc, report)| {
                    let quill = quill_formats::native::write(&doc, 0);
                    let back = quill_formats::native::read(&quill);
                    let notes = report.notes().join(" | ");
                    (doc.main.paras.len(), back.is_ok(), kept_chars(&doc), notes)
                })
            });
            let _ = tx.send((r, start.elapsed()));
        });
        let ext = f.extension().unwrap().to_string_lossy().to_lowercase();
        let line = match rx.recv_timeout(Duration::from_secs(20)) {
            Ok((Ok(Ok((paras, true, kept, notes))), t)) => {
                let cover = match (ext.as_str(), body_chars(&f)) {
                    ("odt" | "ott" | "docx" | "docm" | "dotx" | "dotm", Some(n)) if n > 0 => {
                        format!("{}%", kept * 100 / n)
                    }
                    _ => "-".into(),
                };
                format!("ok\t{ext}\t{}ms\t{paras}\t{cover}\t{notes}", t.as_millis())
            }
            Ok((Ok(Ok((_, false, _, _))), _)) => format!("quill-roundtrip-fail\t{ext}"),
            Ok((Ok(Err(e)), _)) => format!("error\t{ext}\t{}", e.replace('\n', " ")),
            Ok((Err(_), _)) => format!("PANIC\t{ext}"),
            Err(_) => format!("TIMEOUT\t{ext}"),
        };
        println!("{line}\t{}", f.display());
    }
}
