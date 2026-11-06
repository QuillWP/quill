//! Helpers shared by the integration tests.
#![allow(dead_code)]

use std::collections::BTreeMap;

use quill_core::notes::{annotation_ids, note_ids};
use quill_core::para::chars;
use quill_core::{CharProps, Document, Object, ParaProps, Story, StoryId};

/// An RGB PNG of `w` x `h` pixels, colored by `pixel(x, y)`, so pictures
/// in tests are real data the readers have to keep.
pub fn png(w: u32, h: u32, pixel: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
    let crc = quill_formats::zip::crc32;
    let chunk = |kind: &[u8], body: &[u8]| {
        let mut c = (body.len() as u32).to_be_bytes().to_vec();
        c.extend_from_slice(kind);
        c.extend_from_slice(body);
        let mut k = kind.to_vec();
        k.extend_from_slice(body);
        c.extend_from_slice(&crc(&k).to_be_bytes());
        c
    };
    let mut raw = Vec::with_capacity(((w * 3 + 1) * h) as usize);
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            raw.extend_from_slice(&pixel(x, y));
        }
    }
    let mut head = w.to_be_bytes().to_vec();
    head.extend_from_slice(&h.to_be_bytes());
    head.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &head));
    out.extend(chunk(
        b"IDAT",
        &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6),
    ));
    out.extend(chunk(b"IEND", &[]));
    out
}

/// Everything a document keeps, as named lines, with ids that may be
/// renumbered on reading (fonts, styles, notes, pictures) replaced by what
/// they stand for.
pub fn describe(doc: &Document) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let notes: Vec<u32> = note_ids(&doc.main.paras)
        .into_iter()
        .chain(annotation_ids(&doc.main.paras))
        .collect();
    let note = |id: u32| match notes.iter().position(|&n| n == id) {
        Some(k) => format!("note {k}"),
        None => format!("unknown note {id}"),
    };
    let style = |id: u16| doc.styles.get(id).name.clone();
    let chp = |c: &CharProps, ch: Option<char>| {
        let object = match ch {
            Some(chars::FOOTNOTE | chars::ANNOTATION) => note(c.object),
            Some(chars::OBJECT) => match doc.object(c.object) {
                Some(Object::Picture(p)) => format!(
                    "picture {:?} {:?} {:?} {:?} {:?} {} bytes",
                    p.format,
                    p.pixels,
                    p.size,
                    p.crop,
                    p.natural,
                    p.data.len()
                ),
                Some(Object::Vacant) | None => "missing picture".into(),
            },
            _ if c.object != 0 => format!("object {}", c.object),
            _ => String::new(),
        };
        let character_style = doc
            .styles
            .character(c.character_style)
            .map_or("", |s| s.name.as_str());
        let plain = CharProps {
            character_style: 0,
            font: 0,
            object: 0,
            link: 0,
            comment: 0,
            rev: 0,
            ..*c
        };
        let link = quill_core::link::get(c.link)
            .map(|l| format!(" link={l:?}"))
            .unwrap_or_default();
        let comment = if c.comment != 0
            && notes.contains(&c.comment)
            && ch.is_some()
            && ch != Some(chars::ANNOTATION)
        {
            format!(" comment={}", note(c.comment))
        } else {
            String::new()
        };
        let revision = quill_core::revision::get(c.rev)
            .map(|r| format!(" revision={r:?}"))
            .unwrap_or_default();
        format!(
            "{plain:?} character_style={character_style:?} font={} {object}{link}{comment}{revision}",
            doc.font_name(c.font)
        )
    };
    let pap = |p: &ParaProps| {
        let plain = ParaProps {
            style: 0,
            ..p.clone()
        };
        format!("style={} {plain:?}", style(p.style))
    };
    let mut story = |name: &str, s: &Story| {
        out.insert(format!("{name} paragraphs"), s.paras.len().to_string());
        for (i, p) in s.paras.iter().enumerate() {
            let key = |what: &str| format!("{name} {i:02} {what}");
            out.insert(key("text"), format!("{:?}", p.text));
            // Runs by what they look like, not how they happen to be split.
            let mut runs: Vec<(usize, String)> = Vec::new();
            for (k, ch) in p.text.char_indices() {
                let d = chp(&p.chp_of_char(k), Some(ch));
                match runs.last_mut() {
                    Some((n, last)) if *last == d => *n += 1,
                    _ => runs.push((1, d)),
                }
            }
            for (k, (n, d)) in runs.iter().enumerate() {
                out.insert(key(&format!("run {k:02}")), format!("{n} x {d}"));
            }
            out.insert(
                key("mark"),
                format!("{:?} {}", p.mark, chp(&p.mark_chp, None)),
            );
            out.insert(key("pap"), pap(&p.pap));
            if let Some(r) = &p.row {
                out.insert(key("row"), format!("{r:?}"));
            }
            if let Some(s) = &p.section {
                // Without its headers, described below with font names.
                let s = quill_core::SectionProps {
                    headers: None,
                    ..(**s).clone()
                };
                out.insert(key("section"), format!("{s:?}"));
            }
        }
    };
    story("main", &doc.main);
    story("notes", &doc.notes);
    story("annotations", &doc.annotations);
    for id in StoryId::HEADERS {
        story(id.name(), doc.story(id));
    }
    // Later sections' own headers and footers, and which share.
    let mut own = Vec::new();
    for k in 1..doc.sections().len() {
        own.push((
            format!("section {k} has its own headers"),
            doc.has_own_headers(k).to_string(),
        ));
        if doc.has_own_headers(k) {
            for id in StoryId::HEADERS {
                story(
                    &format!("section {k} {}", id.name()),
                    doc.header_story(k, id),
                );
            }
        }
    }
    out.extend(own);
    for s in &doc.styles.styles {
        out.insert(
            format!("style {}", s.name),
            format!(
                "based on {:?}, next {}, builtin {}, {} | {}",
                s.based_on.map(style),
                style(s.next),
                s.builtin,
                pap(&s.pap),
                chp(&s.chp, None)
            ),
        );
    }
    for s in &doc.styles.characters {
        let mut props = s.format.props;
        props.font = 0;
        out.insert(
            format!("character style {}", s.name),
            format!(
                "mask {}, props {props:?}, font {}",
                s.format.mask,
                if s.format.mask & 1 != 0 {
                    doc.font_name(s.format.props.font)
                } else {
                    "<inherited>"
                }
            ),
        );
    }
    for (id, m) in &doc.note_marks {
        out.insert(format!("mark of {}", note(*id)), m.clone());
    }
    for (id, info) in &doc.annotation_info {
        // Deleted notes retain metadata for Undo, but are not saved content.
        if !notes.contains(id) {
            continue;
        }
        let parent = info.parent.map(note);
        let plain = quill_core::notes::AnnotationInfo {
            parent: None,
            ..info.clone()
        };
        out.insert(
            format!("info of {}", note(*id)),
            format!("{plain:?} parent={parent:?}"),
        );
    }
    out.insert("props".into(), format!("{:?}", doc.props));
    let last = quill_core::SectionProps {
        headers: None,
        ..doc.section.clone()
    };
    out.insert("section".into(), format!("{last:?}"));
    out.insert("summary".into(), format!("{:?}", doc.summary));
    out
}

/// Every line that differs between the document and its copy, read back.
pub fn differences(doc: &Document, back: &Document) -> Vec<String> {
    let (a, b) = (describe(doc), describe(back));
    let mut diffs = Vec::new();
    for (k, v) in &a {
        match b.get(k) {
            None => diffs.push(format!("lost      {k}: {v}")),
            Some(w) if w != v => diffs.push(format!("changed   {k}:\n  was {v}\n  now {w}")),
            _ => {}
        }
    }
    for (k, w) in &b {
        if !a.contains_key(k) {
            diffs.push(format!("appeared  {k}: {w}"));
        }
    }
    diffs
}
