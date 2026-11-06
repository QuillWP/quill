//! Save a folder's documents in Quill's own formats, for the fuzz corpora:
//! `cargo run -p quill-formats --example seed_exports fuzz/word-fixtures/2026-11-01`.
//! Reads `sources` and `word`, writes .qwp, .rtf, .htm, .docx and .odt to `quill`.

use quill_formats::Format;
use std::path::Path;

fn main() {
    let root = std::env::args().nth(1).expect("a fixture folder");
    let root = Path::new(&root);
    let out = root.join("quill");
    std::fs::create_dir_all(&out).unwrap();
    let mut made = 0;
    for folder in ["sources", "word"] {
        let mut paths: Vec<_> = std::fs::read_dir(root.join(folder))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        paths.sort();
        for path in paths {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let wanted = ["rtf", "docx", "htm", "html", "odt"]
                .contains(&path.extension().map_or("", |e| e.to_str().unwrap_or("")));
            if name.starts_with("._") || name.starts_with("~$") || !wanted {
                continue;
            }
            let doc = match quill_formats::load(&path) {
                Ok(doc) => doc,
                Err(error) => {
                    println!("{name}: {error}");
                    continue;
                }
            };
            let stem = format!("{folder}-{}", name.replace('.', "-"));
            for ext in ["qwp", "rtf", "htm", "docx", "odt"] {
                let target = out.join(format!("{stem}.{ext}"));
                let format = Format::from_path(&target).expect("known extension");
                quill_formats::save(&doc, &target, format).unwrap_or_else(|e| panic!("{e}"));
                made += 1;
            }
        }
    }
    println!("{made} files");
}
