//! Combine the newest features with documents that already have the rest, for
//! the fuzz corpora:
//! `cargo run -p quill-formats --example feature_seeds <out> <folder>...`.
//!
//! Every .rtf, .docx, .odt and .htm in the folders gets named character styles
//! in each story, direct overrides on top of them, headings linked to
//! character styles and a contents list, in three variants: linked, the
//! linked styles deleted again, and Normal itself linked. A few documents are
//! also built from nothing. Each is saved to `<out>/quill` in every format.

use quill_core::character_styles::CharacterFormat;
use quill_core::contents::TocOptions;
use quill_core::{CharProps, Document, Position, Rgb, StoryId, Underline};
use quill_formats::Format;
use std::path::Path;

const STORIES: [StoryId; 5] = [
    StoryId::Main,
    StoryId::Notes,
    StoryId::Annotations,
    StoryId::Header,
    StoryId::Footer,
];

/// Callout sets everything; the others set one or two fields, one of them in
/// a font no computer has.
fn add_styles(doc: &mut Document) -> Vec<u16> {
    let georgia = doc.font_id("Georgia");
    let missing = doc.font_id("Quill Missing Mono");
    let wanted = [
        (
            "Callout",
            CharProps {
                font: georgia,
                size: 30,
                bold: true,
                italic: true,
                underline: Underline::Single,
                strike: true,
                outline: true,
                shadow: true,
                small_caps: true,
                all_caps: true,
                color: Rgb(0x1F5FAA),
                position: 6,
                spacing: 40,
                ..Default::default()
            },
            CharacterFormat::ALL & !(1 << 9),
        ),
        (
            "Quiet",
            CharProps {
                italic: true,
                color: Rgb(0x777777),
                ..Default::default()
            },
            (1 << 3) | (1 << 11),
        ),
        (
            "Code",
            CharProps {
                font: missing,
                size: 18,
                ..Default::default()
            },
            1 | (1 << 1),
        ),
        (
            "Raised",
            CharProps {
                position: 12,
                size: 16,
                ..Default::default()
            },
            (1 << 1) | (1 << 12),
        ),
    ];
    let mut ids = Vec::new();
    for (name, props, mask) in wanted {
        // A document read back from an earlier run may have the name already.
        let name = if doc.styles.find_character(name).is_some() {
            format!("{name} 2")
        } else {
            name.to_string()
        };
        if let Some(id) = doc.add_character_style(&name, CharacterFormat::new(props, mask)) {
            ids.push(id);
        }
    }
    ids
}

/// Char offsets on boundaries: the first few characters, and a middle stretch.
fn spans(text: &str) -> Vec<(usize, usize)> {
    let ends: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain([text.len()])
        .collect();
    let n = ends.len() - 1;
    if n < 2 {
        return Vec::new();
    }
    let mut out = vec![(0, ends[n.min(6)])];
    if n > 12 {
        out.push((ends[n / 3], ends[n / 2]));
    }
    out
}

fn apply_everywhere(doc: &mut Document, ids: &[u16]) {
    for (s, story) in STORIES.into_iter().enumerate() {
        let count = doc.story(story).paras.len().min(400);
        for para in 0..count {
            let text = doc.story(story).paras[para].text.clone();
            for (k, (a, b)) in spans(&text).into_iter().enumerate() {
                let id = ids[(para + k + s) % ids.len()];
                let (start, end) = (Position::new(para, a), Position::new(para, b));
                doc.apply_character_style(story, start, end, id);
                // Hand formatting over part of every third styled stretch.
                if para % 3 == 0 && story == StoryId::Main {
                    doc.format_chars(story, start, end, |c| {
                        c.italic = !c.italic;
                        c.color = Rgb(0xAA2200);
                    });
                }
            }
        }
    }
}

/// Link heading 1 to 3 to the styles, and base a new style on a linked one.
fn link_headings(doc: &mut Document, ids: &[u16]) {
    for (k, name) in ["heading 1", "heading 2", "heading 3"].iter().enumerate() {
        let Some(style) = doc.styles.find(name) else {
            continue;
        };
        let s = doc.styles.get(style);
        let (pap, own) = (s.pap.clone(), s.own());
        doc.redefine_style_linked(style, pap, own, ids[k % ids.len()]);
        if k == 0 {
            let derived = doc.add_style(
                "Heading Derived",
                style,
                None,
                doc.styles.get(style).pap.clone(),
                CharProps {
                    underline: Underline::Double,
                    ..doc.styles.get(style).chp
                },
            );
            let last = doc.main.paras.len() - 1;
            doc.apply_style(StoryId::Main, last, last, derived);
        }
    }
}

fn contents(doc: &mut Document) {
    let entries: Vec<_> = doc
        .toc_entries(&TocOptions::default())
        .into_iter()
        .enumerate()
        .map(|(i, e)| (e, (i + 1).to_string()))
        .collect();
    if !entries.is_empty() {
        let paras = doc.toc_paragraphs(&entries, true);
        doc.place_toc(paras);
    }
}

/// Six heading levels, each linked, with lists and links between them.
fn built() -> Vec<(String, Document)> {
    let mut lines = Vec::new();
    for level in 1..=6 {
        lines.push(format!("Heading at level {level}"));
        lines.push(format!(
            "Body {level} with mail to someone{level}@example.com and https://example.com/{level}."
        ));
    }
    let mut doc = quill_core::text::from_plain_text(&lines.join("\n"));
    for level in 1..=6u16 {
        if let Some(style) = doc.styles.find(&format!("heading {level}")) {
            let para = usize::from(level - 1) * 2;
            doc.apply_style(StoryId::Main, para, para, style);
        }
    }
    let ids = add_styles(&mut doc);
    apply_everywhere(&mut doc, &ids);
    for level in 1..=6usize {
        if let Some(style) = doc.styles.find(&format!("heading {level}")) {
            let s = doc.styles.get(style);
            let (pap, own) = (s.pap.clone(), s.own());
            doc.redefine_style_linked(style, pap, own, ids[level % ids.len()]);
        }
    }
    contents(&mut doc);
    let mut redefined = doc.clone();
    // Redefining a character style moves every linked heading with it.
    for &id in &ids {
        let mut format = redefined.styles.character(id).unwrap().format;
        format.props.size += 4;
        format.mask |= 1 << 1;
        redefined.redefine_character_style(id, format);
    }
    let mut deleted = doc.clone();
    for &id in ids.iter().rev() {
        deleted.delete_character_style(id);
    }
    vec![
        ("built-six-levels".into(), doc),
        ("built-redefined".into(), redefined),
        ("built-deleted".into(), deleted),
    ]
}

fn variants(doc: Document) -> Vec<(&'static str, Document)> {
    let mut linked = doc;
    let ids = add_styles(&mut linked);
    if ids.is_empty() {
        return vec![("styled", linked)];
    }
    apply_everywhere(&mut linked, &ids);
    link_headings(&mut linked, &ids);
    contents(&mut linked);
    let mut deleted = linked.clone();
    deleted.delete_character_style(ids[0]);
    deleted.delete_character_style(ids[1].min(deleted.styles.characters.len() as u16));
    let mut normal = linked.clone();
    let s = normal.styles.get(0);
    let (pap, own) = (s.pap.clone(), s.own());
    normal.redefine_style_linked(0, pap, own, ids[1]);
    vec![("linked", linked), ("deleted", deleted), ("normal", normal)]
}

fn save_all(out: &Path, stem: &str, doc: &mut Document) -> usize {
    doc.keep_in_bounds();
    doc.forget_history();
    let mut made = 0;
    for ext in ["qwp", "docx", "rtf", "odt", "htm", "md"] {
        let target = out.join(format!("{stem}.{ext}"));
        let format = Format::from_path(&target).expect("known extension");
        match quill_formats::save(doc, &target, format) {
            Ok(()) => made += 1,
            Err(error) => println!("{stem}.{ext}: {error}"),
        }
    }
    made
}

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("an output folder");
    let out = Path::new(&root).join("quill");
    std::fs::create_dir_all(&out).unwrap();
    let mut made = 0;
    for (stem, mut doc) in built() {
        made += save_all(&out, &stem, &mut doc);
    }
    for folder in args {
        let folder = Path::new(&folder);
        let mut paths: Vec<_> = std::fs::read_dir(folder)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        paths.sort();
        let tag = folder
            .components()
            .rev()
            .take(2)
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("-");
        for path in paths {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let ext = path.extension().map_or("", |e| e.to_str().unwrap_or(""));
            if name.starts_with("._")
                || name.starts_with("~$")
                || !["rtf", "docx", "htm", "html", "odt"].contains(&ext)
            {
                continue;
            }
            let doc = match quill_formats::load(&path) {
                Ok(doc) => doc,
                Err(error) => {
                    println!("{name}: {error}");
                    continue;
                }
            };
            let stem = format!("{tag}-{}", name.replace('.', "-"));
            for (variant, mut doc) in variants(doc) {
                made += save_all(&out, &format!("{stem}-{variant}"), &mut doc);
            }
        }
    }
    println!("{made} files in {}", out.display());
}
