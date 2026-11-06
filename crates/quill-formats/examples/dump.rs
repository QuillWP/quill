//! Print what Quill reads from a file: `cargo run -p quill-formats --example
//! dump file.docx`.

fn main() {
    let path = std::env::args().nth(1).expect("usage: dump FILE");
    let doc = quill_formats::load(std::path::Path::new(&path)).unwrap_or_else(|e| panic!("{e}"));
    for p in &doc.main.paras {
        let style = &doc.styles.get(p.pap.style).name;
        let runs: Vec<String> = p
            .run_bounds()
            .map(|(s, e, c)| {
                let mut f = String::new();
                if c.bold {
                    f.push('B');
                }
                if c.italic {
                    f.push('I');
                }
                format!(
                    "[{}{} {}pt {}]",
                    f,
                    doc.font_name(c.font),
                    c.size / 2,
                    &p.text[s..e]
                )
            })
            .collect();
        println!(
            "{style:12} {:?} {:?} {}",
            p.pap.align,
            p.mark,
            runs.join("")
        );
    }
}
