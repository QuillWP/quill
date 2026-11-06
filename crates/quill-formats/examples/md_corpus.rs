//! Write every Word, RTF, ODT and HTML file under a folder as Markdown, read it back,
//! and check that writing again changes nothing: `cargo run --release -p quill-formats
//! --example md_corpus -- <folder>`.
use std::path::Path;
fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out)
            } else {
                out.push(p)
            }
        }
    }
}
fn main() {
    let root = std::env::args().nth(1).unwrap();
    let mut files = Vec::new();
    walk(Path::new(&root), &mut files);
    files.retain(|p| {
        matches!(
            p.extension().and_then(|e| e.to_str()),
            Some("docx" | "rtf" | "odt" | "html")
        )
    });
    let (mut n, mut unstable, mut lossy, mut bad) = (0, 0, 0, 0);
    for p in files {
        let Ok(doc) = quill_formats::load(&p) else {
            continue;
        };
        n += 1;
        let r = std::panic::catch_unwind(|| {
            let lost = quill_formats::markdown::losses(&doc);
            let a = quill_formats::markdown::write(&doc);
            let (d2, _) = quill_formats::markdown::read(&a, None);
            let b = quill_formats::markdown::write(&d2);
            let (d3, _) = quill_formats::markdown::read(&b, None);
            let c = quill_formats::markdown::write(&d3);
            (
                lost.len(),
                b == c,
                d2.size().too_large().is_some(),
                a == b,
                a,
                b,
            )
        });
        match r {
            Ok((l, stable, big, same, a, b)) => {
                if l > 0 {
                    lossy += 1
                }
                if !stable || big {
                    unstable += 1;
                    if unstable <= 6 {
                        let c = quill_formats::markdown::write(
                            &quill_formats::markdown::read(&b, None).0,
                        );
                        let (la, lb): (Vec<_>, Vec<_>) = (b.lines().collect(), c.lines().collect());
                        let k = la.iter().zip(&lb).position(|(x, y)| x != y).unwrap_or(0);
                        println!(
                            "UNSTABLE {} line {k}\n b: {}\n c: {}",
                            p.display(),
                            la.get(k).map(|l| &l[..l.len().min(300)]).unwrap_or(""),
                            lb.get(k).map(|l| &l[..l.len().min(300)]).unwrap_or("")
                        );
                    }
                }
                let _ = (same, &a);
            }
            Err(_) => {
                bad += 1;
                println!("PANIC {}", p.display());
            }
        }
    }
    println!("files {n} lossy {lossy} unstable {unstable} panics {bad}");
}
