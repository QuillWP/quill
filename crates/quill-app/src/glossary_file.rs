//! The bounded on-disk codec for user glossary entries.

use quill_core::Document;
use quill_core::document::Fragment;
use quill_core::para::Paragraph;

/// A user entry: the text and the font and style names its formats use.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub frag: Fragment,
    pub fonts: Vec<String>,
    pub styles: Vec<String>,
    pub characters: Vec<quill_core::character_styles::CharacterStyle>,
}

const HEADER: &str = "Quill Glossary 1";

/// Serialize glossary entries, with each formatted fragment stored as RTF.
pub fn serialize(list: &[Entry]) -> Vec<u8> {
    let mut out = format!("{HEADER}\n").into_bytes();
    for e in list {
        let mut doc = Document::new();
        doc.fonts = e.fonts.clone();
        doc.styles.characters = e.characters.clone();
        if doc.fonts.is_empty() {
            doc.fonts.push("New York".into());
        }
        doc.main.paras = e.frag.paras.clone();
        if !e.frag.objects.is_empty() {
            let f = Fragment {
                paras: doc.main.paras.clone(),
                ends_with_mark: true,
                objects: e.frag.objects.clone(),
                notes: Vec::new(),
            };
            let mut tmp = Document::new();
            tmp.main.paras = vec![Paragraph::default()];
            tmp.insert_fragment(
                quill_core::StoryId::Main,
                quill_core::Position::new(0, 0),
                &f,
            );
            doc.objects = tmp.objects.clone();
            doc.main.paras = tmp.main.paras[..e.frag.paras.len()].to_vec();
        }
        for p in &mut doc.main.paras {
            p.pap.style = 0;
        }
        let rtf = quill_formats::rtf::write(&doc);
        let name = e.name.replace(['\t', '\n'], " ");
        out.extend_from_slice(
            format!(
                "{name}\t{}\t{}\t{}\n",
                e.frag.ends_with_mark as u8,
                e.frag.paras.len(),
                rtf.len()
            )
            .as_bytes(),
        );
        out.extend_from_slice(&rtf);
        out.push(b'\n');
    }
    out
}

/// Parse as many complete, valid entries as are present. Truncated or damaged
/// tails are ignored so a partially written file cannot crash Quill.
pub fn parse(data: &[u8]) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut at = match data.iter().position(|&b| b == b'\n') {
        Some(n) if &data[..n] == HEADER.as_bytes() => n + 1,
        _ => return out,
    };
    while at < data.len() {
        let Some(n) = data[at..].iter().position(|&b| b == b'\n') else {
            break;
        };
        let line = String::from_utf8_lossy(&data[at..at + n]).to_string();
        at += n + 1;
        let f: Vec<&str> = line.split('\t').collect();
        let [name, mark, count, len] = f[..] else {
            break;
        };
        let (Ok(count), Ok(len)) = (count.parse::<usize>(), len.parse::<usize>()) else {
            break;
        };
        let Some(end) = at.checked_add(len) else {
            break;
        };
        let Some(rtf) = data.get(at..end) else {
            break;
        };
        at = end.saturating_add(1);
        let Ok(mut doc) = quill_formats::rtf::read(rtf) else {
            continue;
        };
        doc.keep_in_bounds();
        let mut paras = doc.main.paras.clone();
        paras.truncate(count.max(1));
        let objects = (1..=doc.objects.len() as u32)
            .filter_map(|id| Some((id, doc.object(id)?.clone())))
            .collect();
        out.push(Entry {
            name: name.to_string(),
            frag: Fragment {
                paras,
                ends_with_mark: mark == "1",
                objects,
                notes: Vec::new(),
            },
            fonts: doc.fonts.clone(),
            styles: doc.styles.styles.iter().map(|s| s.name.clone()).collect(),
            characters: doc.styles.characters.clone(),
        });
    }
    out
}
