//! Quill's own file format, `.qwp`: the document exactly as Quill holds it,
//! so saving never loses anything. .docx, RTF and text are for exchanging
//! documents with other programs.
//!
//! A `.qwp` file is a ZIP archive:
//!
//! ```text
//! mimetype        "application/x-quillwp", stored first and uncompressed
//! meta.txt        a note for people: the format version and who wrote it
//! document.bin    the document (below)
//! objects/1.png   pictures, each file once however many entries share it
//! ```
//!
//! `document.bin` starts with the magic `QUILLDOC` and the format's major
//! and minor version (u16 each, little-endian). Then come fields, and
//! every value is made of fields too: a field is a tag, a length and that
//! many bytes, the tag and length as unsigned LEB128. Numbers are LEB128
//! (zigzag for signed ones), text is UTF-8, and a list is the same tag
//! repeated. A reader skips tags it does not know, so a newer minor
//! version can add fields; a new major version is refused. Fields that are
//! missing take their default.
//!
//! The ZIP's checksums are checked on reading, and what is read is
//! repaired rather than trusted: run lengths that do not fit the text,
//! and font, style and picture numbers that point nowhere.

use std::collections::HashMap;
use std::sync::Arc;

use quill_core::document::{
    DocumentProps, FootnotePosition, LineNumbering, PageNumberFormat, SectionProps, SectionStart,
    SummaryInfo,
};
use quill_core::notes::AnnotationInfo;
use quill_core::props::{
    BorderLine, BorderStyle, Borders, CellProps, Frame, FrameH, FrameRel, FrameV, RowHeight,
    RowProps, TabLeader,
};
use quill_core::{
    Align, CharProps, Crop, Document, ImageFormat, LineSpacing, Mark, Object, ParaProps, Paragraph,
    Picture, Rgb, Run, Story, StoryId, Style, TabAlign, TabStop, Underline,
};

use crate::zip;

pub const MIMETYPE: &str = "application/x-quillwp";
const MAGIC: &[u8; 8] = b"QUILLDOC";
/// A file with a higher major version may hold things this Quill cannot
/// show, so it is refused; minor versions only add fields.
const MAJOR: u16 = 1;
/// 1.1: sections' own headers and footers. 1.2: bounded picture previews.
/// 1.3: hyperlinks. 1.4: tracked changes, and comments' ranges, replies and
/// resolved state. 1.5: live tables of contents.
const MINOR: u16 = 5;

/// Whether `bytes` is a `.qwp` file rather than some other ZIP (.docx).
pub fn is_quill(bytes: &[u8]) -> bool {
    zip::Archive::open(bytes)
        .ok()
        .and_then(|z| z.read("mimetype").ok().flatten())
        .is_some_and(|m| m == MIMETYPE.as_bytes())
}

// ----- writing -----

/// Fields being written.
#[derive(Default)]
struct Out(Vec<u8>);

fn leb(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

impl Out {
    fn bytes(&mut self, tag: u32, data: &[u8]) {
        leb(&mut self.0, tag as u64);
        leb(&mut self.0, data.len() as u64);
        self.0.extend_from_slice(data);
    }

    fn uint(&mut self, tag: u32, v: u64) {
        let mut b = Vec::with_capacity(10);
        leb(&mut b, v);
        self.bytes(tag, &b);
    }

    fn int(&mut self, tag: u32, v: i64) {
        self.uint(tag, ((v << 1) ^ (v >> 63)) as u64);
    }

    fn flag(&mut self, tag: u32, v: bool) {
        self.uint(tag, v as u64);
    }

    fn text(&mut self, tag: u32, s: &str) {
        self.bytes(tag, s.as_bytes());
    }

    fn nest(&mut self, tag: u32, f: impl FnOnce(&mut Out)) {
        let mut inner = Out::default();
        f(&mut inner);
        self.bytes(tag, &inner.0);
    }
}

/// Write the document as a `.qwp` file, its entries dated `secs`.
pub fn write(doc: &Document, secs: u64) -> Vec<u8> {
    let compact = doc.compact_for_save();
    let doc = &compact;
    // Pictures: each file once, entries sharing data sharing the file.
    let mut files: Vec<(String, Arc<[u8]>)> = Vec::new();
    let mut file_of: Vec<(String, Option<String>)> = Vec::new();
    for o in &doc.objects {
        let Object::Picture(p) = o else {
            unreachable!("compact documents contain no vacant objects")
        };
        let name = match files
            .iter()
            .find(|(_, d)| Arc::ptr_eq(d, &p.data) || *d == p.data)
        {
            Some((n, _)) => n.clone(),
            None => {
                let n = format!("objects/{}.{}", files.len() + 1, p.format.extension());
                files.push((n.clone(), p.data.clone()));
                n
            }
        };
        let preview = p.preview.as_ref().map(|data| {
            match files
                .iter()
                .find(|(_, d)| Arc::ptr_eq(d, data) || **d == **data)
            {
                Some((n, _)) => n.clone(),
                None => {
                    let n = format!("previews/{}.png", files.len() + 1);
                    files.push((n.clone(), data.clone()));
                    n
                }
            }
        });
        file_of.push((name, preview));
    }

    let mut o = Out(Vec::new());
    for f in &doc.fonts {
        o.text(1, f);
    }
    for s in &doc.styles.styles {
        o.nest(2, |o| style(o, s));
    }
    o.nest(3, |o| document_props(o, &doc.props));
    o.nest(4, |o| section(o, &doc.section));
    o.nest(5, |o| summary(o, &doc.summary));
    o.nest(6, |o| story(o, &doc.main));
    o.nest(7, |o| story(o, &doc.notes));
    o.nest(8, |o| story(o, &doc.annotations));
    for (k, id) in StoryId::HEADERS.into_iter().enumerate() {
        o.nest(9, |o| {
            o.uint(1, k as u64);
            o.nest(2, |o| story(o, doc.story(id)));
        });
    }
    for (obj, (file, preview)) in doc.objects.iter().zip(&file_of) {
        let Object::Picture(p) = obj else {
            unreachable!("compact documents contain no vacant objects")
        };
        o.nest(10, |o| picture(o, p, file, preview.as_deref()));
    }
    let mut marks: Vec<_> = doc.note_marks.iter().collect();
    marks.sort();
    for (id, m) in marks {
        o.nest(11, |o| {
            o.uint(1, *id as u64);
            o.text(2, m);
        });
    }
    let mut infos: Vec<_> = doc.annotation_info.iter().collect();
    infos.sort_by_key(|(id, _)| **id);
    for (id, info) in infos {
        let AnnotationInfo {
            initials,
            author,
            date,
            parent,
            done,
        } = info;
        o.nest(12, |o| {
            o.uint(1, *id as u64);
            o.text(2, initials);
            o.text(3, author);
            o.text(4, date);
            if let Some(parent) = parent {
                o.uint(5, *parent as u64);
            }
            o.flag(6, *done);
        });
    }

    let mut bin = MAGIC.to_vec();
    bin.extend_from_slice(&MAJOR.to_le_bytes());
    bin.extend_from_slice(&MINOR.to_le_bytes());
    bin.extend_from_slice(&o.0);

    let mut z = zip::Writer::new(secs);
    z.add("mimetype", MIMETYPE.as_bytes());
    z.add(
        "meta.txt",
        format!(
            "Quill document, format {MAJOR}.{MINOR}\nWritten by Quill {}\n",
            env!("CARGO_PKG_VERSION")
        )
        .as_bytes(),
    );
    z.add("document.bin", &bin);
    for (name, data) in &files {
        z.add(name, data);
    }
    z.finish()
}

// Every writer takes its struct apart field by field, so a field added to
// the model and not saved here stops the build.

fn chp(o: &mut Out, c: &CharProps) {
    let CharProps {
        font,
        size,
        bold,
        italic,
        underline,
        strike,
        outline,
        shadow,
        small_caps,
        all_caps,
        hidden,
        color,
        position,
        spacing,
        object,
        link,
        rev,
        comment,
    } = *c;
    o.uint(1, font as u64);
    o.uint(2, size as u64);
    o.flag(3, bold);
    o.flag(4, italic);
    o.uint(
        5,
        match underline {
            Underline::None => 0,
            Underline::Single => 1,
            Underline::Word => 2,
            Underline::Double => 3,
            Underline::Dotted => 4,
        },
    );
    o.flag(6, strike);
    o.flag(7, outline);
    o.flag(8, shadow);
    o.flag(9, small_caps);
    o.flag(10, all_caps);
    o.flag(11, hidden);
    o.uint(12, color.0 as u64);
    o.int(13, position as i64);
    o.int(14, spacing as i64);
    o.uint(15, object as u64);
    // The link itself, not its number: numbers only last while Quill runs.
    if let Some(l) = quill_core::link::get(link) {
        o.nest(16, |o| {
            o.text(1, &l.url);
            o.flag(2, l.new_window);
            o.uint(3, l.hover.0 as u64);
            o.uint(4, l.visited.0 as u64);
        });
    }
    if comment != 0 {
        o.uint(18, comment as u64);
    }
    // The same goes for tracked changes.
    if let Some(r) = quill_core::revision::get(rev) {
        o.nest(17, |o| {
            if let Some(s) = &r.inserted {
                o.nest(1, |o| stamp(o, s));
            }
            if let Some(s) = &r.deleted {
                o.nest(2, |o| stamp(o, s));
            }
            if let Some((s, old)) = &r.formatted {
                o.nest(3, |o| stamp(o, s));
                o.nest(4, |o| chp(o, &CharProps { rev: 0, ..*old }));
            }
        });
    }
}

fn stamp(o: &mut Out, s: &quill_core::revision::Stamp) {
    o.text(1, &s.author);
    o.text(2, &s.date);
}

fn border_line(o: &mut Out, b: &BorderLine) {
    let BorderLine { style, space } = *b;
    o.uint(
        1,
        match style {
            BorderStyle::None => 0,
            BorderStyle::Dotted => 1,
            BorderStyle::Hairline => 2,
            BorderStyle::Single => 3,
            BorderStyle::Thick => 4,
            BorderStyle::Double => 5,
        },
    );
    o.int(2, space as i64);
}

fn borders(o: &mut Out, b: &Borders) {
    let Borders {
        top,
        bottom,
        left,
        right,
        between,
        shadow,
    } = b;
    o.nest(1, |o| border_line(o, top));
    o.nest(2, |o| border_line(o, bottom));
    o.nest(3, |o| border_line(o, left));
    o.nest(4, |o| border_line(o, right));
    o.nest(5, |o| border_line(o, between));
    o.flag(6, *shadow);
}

/// An enum that may carry a number: its kind, then the number.
fn kind(o: &mut Out, k: u64, v: Option<i32>) {
    o.uint(1, k);
    if let Some(v) = v {
        o.int(2, v as i64);
    }
}

fn frame(o: &mut Out, f: &Frame) {
    let Frame {
        h,
        h_rel,
        v,
        v_rel,
        width,
        from_text,
        drop_lines,
        in_margin,
    } = *f;
    o.nest(1, |o| match h {
        FrameH::Left => kind(o, 0, None),
        FrameH::Center => kind(o, 1, None),
        FrameH::Right => kind(o, 2, None),
        FrameH::Inside => kind(o, 3, None),
        FrameH::Outside => kind(o, 4, None),
        FrameH::At(t) => kind(o, 5, Some(t)),
    });
    o.uint(2, rel(h_rel));
    o.nest(3, |o| match v {
        FrameV::InLine => kind(o, 0, None),
        FrameV::Top => kind(o, 1, None),
        FrameV::Center => kind(o, 2, None),
        FrameV::Bottom => kind(o, 3, None),
        FrameV::At(t) => kind(o, 4, Some(t)),
    });
    o.uint(4, rel(v_rel));
    if let Some(w) = width {
        o.int(5, w as i64);
    }
    o.int(6, from_text as i64);
    o.uint(7, drop_lines as u64);
    o.flag(8, in_margin);
}

fn rel(r: FrameRel) -> u64 {
    match r {
        FrameRel::Column => 0,
        FrameRel::Margin => 1,
        FrameRel::Page => 2,
    }
}

fn align(a: Align) -> u64 {
    match a {
        Align::Left => 0,
        Align::Center => 1,
        Align::Right => 2,
        Align::Justify => 3,
    }
}

fn pap(o: &mut Out, p: &ParaProps) {
    let ParaProps {
        style,
        align: al,
        left,
        right,
        first,
        before,
        after,
        line,
        tabs,
        page_break_before,
        keep_with_next,
        keep_together,
        suppress_line_numbers,
        borders: b,
        shading,
        in_table,
        frame: fr,
    } = p;
    o.uint(1, *style as u64);
    o.uint(2, align(*al));
    o.int(3, *left as i64);
    o.int(4, *right as i64);
    o.int(5, *first as i64);
    o.int(6, *before as i64);
    o.int(7, *after as i64);
    o.nest(8, |o| match *line {
        LineSpacing::Auto => kind(o, 0, None),
        LineSpacing::AtLeast(t) => kind(o, 1, Some(t)),
        LineSpacing::Exactly(t) => kind(o, 2, Some(t)),
        LineSpacing::Multiple(m) => kind(o, 3, Some(m as i32)),
    });
    for t in tabs {
        let TabStop {
            pos,
            align: ta,
            leader,
        } = *t;
        o.nest(9, |o| {
            o.int(1, pos as i64);
            o.uint(
                2,
                match ta {
                    TabAlign::Left => 0,
                    TabAlign::Center => 1,
                    TabAlign::Right => 2,
                    TabAlign::Decimal => 3,
                    TabAlign::Bar => 4,
                },
            );
            o.uint(
                3,
                match leader {
                    TabLeader::None => 0,
                    TabLeader::Dots => 1,
                    TabLeader::Dashes => 2,
                    TabLeader::Underline => 3,
                },
            );
        });
    }
    o.flag(10, *page_break_before);
    o.flag(11, *keep_with_next);
    o.flag(12, *keep_together);
    o.flag(13, *suppress_line_numbers);
    o.nest(14, |o| borders(o, b));
    o.uint(15, *shading as u64);
    o.flag(16, *in_table);
    if let Some(f) = fr {
        o.nest(17, |o| frame(o, f));
    }
}

fn row(o: &mut Out, r: &RowProps) {
    let RowProps {
        cells,
        gap,
        indent,
        height,
        align: al,
    } = r;
    for c in cells {
        let CellProps {
            width,
            borders: b,
            shading,
        } = c;
        o.nest(1, |o| {
            o.int(1, *width as i64);
            o.nest(2, |o| borders(o, b));
            o.uint(3, *shading as u64);
        });
    }
    o.int(2, *gap as i64);
    o.int(3, *indent as i64);
    o.nest(4, |o| match *height {
        RowHeight::Auto => kind(o, 0, None),
        RowHeight::AtLeast(t) => kind(o, 1, Some(t)),
        RowHeight::Exactly(t) => kind(o, 2, Some(t)),
    });
    o.uint(5, align(*al));
}

fn section(o: &mut Out, s: &SectionProps) {
    let SectionProps {
        start,
        columns,
        column_spacing,
        header_from_top,
        footer_from_bottom,
        different_first_page,
        page_number_format,
        restart_page_numbers,
        margin_page_numbers,
        page_number_from_top,
        page_number_from_right,
        include_endnotes,
        line_numbers,
        line_count_by,
        line_from_text,
        headers,
    } = s;
    o.uint(
        1,
        match start {
            SectionStart::NoBreak => 0,
            SectionStart::NewColumn => 1,
            SectionStart::NewPage => 2,
            SectionStart::EvenPage => 3,
            SectionStart::OddPage => 4,
        },
    );
    o.uint(2, *columns as u64);
    o.int(3, *column_spacing as i64);
    o.int(4, *header_from_top as i64);
    o.int(5, *footer_from_bottom as i64);
    o.flag(6, *different_first_page);
    o.uint(
        7,
        match page_number_format {
            PageNumberFormat::Arabic => 0,
            PageNumberFormat::UpperRoman => 1,
            PageNumberFormat::LowerRoman => 2,
            PageNumberFormat::UpperLetter => 3,
            PageNumberFormat::LowerLetter => 4,
        },
    );
    o.flag(8, *restart_page_numbers);
    o.flag(9, *margin_page_numbers);
    o.int(10, *page_number_from_top as i64);
    o.int(11, *page_number_from_right as i64);
    o.flag(12, *include_endnotes);
    o.uint(
        13,
        match line_numbers {
            LineNumbering::Off => 0,
            LineNumbering::ByPage => 1,
            LineNumbering::BySection => 2,
            LineNumbering::Continuous => 3,
        },
    );
    o.uint(14, *line_count_by as u64);
    if let Some(t) = line_from_text {
        o.int(15, *t as i64);
    }
    // The section's own headers and footers; none means Same As Previous.
    if let Some(set) = headers {
        o.nest(16, |o| {
            for (k, st) in set.iter().enumerate() {
                o.nest(1, |o| {
                    o.uint(1, k as u64);
                    o.nest(2, |o| story(o, st));
                });
            }
        });
    }
}

fn document_props(o: &mut Out, p: &DocumentProps) {
    let DocumentProps {
        page_width,
        page_height,
        margin_left,
        margin_right,
        margin_top,
        margin_bottom,
        top_exact,
        bottom_exact,
        gutter,
        mirror_margins,
        default_tab,
        widow_control,
        print_hidden,
        even_odd_headers,
        footnote_position,
        footnote_restart_each_page,
        footnote_start,
        track_changes,
        live_toc,
    } = p;
    o.int(1, *page_width as i64);
    o.int(2, *page_height as i64);
    o.int(3, *margin_left as i64);
    o.int(4, *margin_right as i64);
    o.int(5, *margin_top as i64);
    o.int(6, *margin_bottom as i64);
    o.flag(7, *top_exact);
    o.flag(8, *bottom_exact);
    o.int(9, *gutter as i64);
    o.flag(10, *mirror_margins);
    o.int(11, *default_tab as i64);
    o.flag(12, *widow_control);
    o.flag(13, *print_hidden);
    o.flag(14, *even_odd_headers);
    o.uint(
        15,
        match footnote_position {
            FootnotePosition::BottomOfPage => 0,
            FootnotePosition::BeneathText => 1,
            FootnotePosition::EndOfSection => 2,
            FootnotePosition::EndOfDocument => 3,
        },
    );
    o.flag(16, *footnote_restart_each_page);
    o.uint(17, *footnote_start as u64);
    o.flag(18, *track_changes);
    if let Some(t) = live_toc {
        o.nest(19, |o| {
            o.flag(1, t.outline);
            o.flag(2, t.page_numbers);
            o.uint(3, t.from as u64);
            o.uint(4, t.to as u64);
        });
    }
}

fn summary(o: &mut Out, s: &SummaryInfo) {
    let SummaryInfo {
        title,
        subject,
        author,
        version,
        keywords,
    } = s;
    o.text(1, title);
    o.text(2, subject);
    o.text(3, author);
    o.text(4, version);
    o.text(5, keywords);
}

fn style(o: &mut Out, s: &Style) {
    let Style {
        name,
        based_on,
        next,
        pap: p,
        chp: c,
        builtin,
    } = s;
    o.text(1, name);
    if let Some(b) = based_on {
        o.uint(2, *b as u64);
    }
    o.uint(3, *next as u64);
    o.nest(4, |o| pap(o, p));
    o.nest(5, |o| chp(o, c));
    o.flag(6, *builtin);
}

fn paragraph(o: &mut Out, p: &Paragraph) {
    let Paragraph {
        text,
        runs,
        pap: pp,
        mark,
        mark_chp,
        row: r,
        section: s,
    } = p;
    o.text(1, text);
    for run in runs {
        let Run { len, chp: c } = run;
        o.nest(2, |o| {
            o.uint(1, *len as u64);
            o.nest(2, |o| chp(o, c));
        });
    }
    o.nest(3, |o| pap(o, pp));
    o.uint(
        4,
        match mark {
            Mark::Paragraph => 0,
            Mark::CellEnd => 1,
            Mark::RowEnd => 2,
            Mark::SectionBreak => 3,
        },
    );
    o.nest(5, |o| chp(o, mark_chp));
    if let Some(r) = r {
        o.nest(6, |o| row(o, r));
    }
    if let Some(s) = s {
        o.nest(7, |o| section(o, s));
    }
}

fn story(o: &mut Out, s: &Story) {
    let Story { paras } = s;
    for p in paras {
        o.nest(1, |o| paragraph(o, p));
    }
}

fn picture(o: &mut Out, p: &Picture, file: &str, preview_file: Option<&str>) {
    let Picture {
        data: _,
        preview: _,
        preview_pixels,
        format,
        pixels,
        natural,
        size,
        crop,
    } = p;
    o.uint(
        1,
        match format {
            ImageFormat::Png => 0,
            ImageFormat::Jpeg => 1,
        },
    );
    o.uint(2, pixels.0 as u64);
    o.uint(3, pixels.1 as u64);
    o.int(4, natural.0 as i64);
    o.int(5, natural.1 as i64);
    o.int(6, size.0 as i64);
    o.int(7, size.1 as i64);
    let Crop {
        left,
        top,
        right,
        bottom,
    } = *crop;
    o.int(8, left as i64);
    o.int(9, top as i64);
    o.int(10, right as i64);
    o.int(11, bottom as i64);
    o.text(12, file);
    if let (Some(file), Some((w, h))) = (preview_file, preview_pixels) {
        o.text(13, file);
        o.uint(14, *w as u64);
        o.uint(15, *h as u64);
    }
}

// ----- reading -----

/// Something in the file did not parse: it is damaged, or not Quill's.
struct Damaged;

fn unleb(b: &[u8], at: &mut usize) -> Result<u64, Damaged> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *b.get(*at).ok_or(Damaged)?;
        *at += 1;
        v |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err(Damaged)
}

/// The fields of a value, in order.
fn fields(b: &[u8]) -> Result<Vec<(u32, &[u8])>, Damaged> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < b.len() {
        let tag = unleb(b, &mut at)?;
        let len = unleb(b, &mut at)? as usize;
        let end = at.checked_add(len).ok_or(Damaged)?;
        let body = b.get(at..end).ok_or(Damaged)?;
        out.push((tag.min(u32::MAX as u64) as u32, body));
        at = end;
    }
    Ok(out)
}

fn uint(b: &[u8]) -> Result<u64, Damaged> {
    unleb(b, &mut 0)
}

fn int(b: &[u8]) -> Result<i64, Damaged> {
    let v = uint(b)?;
    Ok(((v >> 1) as i64) ^ -((v & 1) as i64))
}

/// Narrowing that clamps rather than wraps, so a bad number stays in range.
fn i32_of(b: &[u8]) -> Result<i32, Damaged> {
    Ok(int(b)?.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
}

fn u16_of(b: &[u8]) -> Result<u16, Damaged> {
    Ok(uint(b)?.min(u16::MAX as u64) as u16)
}

fn u32_of(b: &[u8]) -> Result<u32, Damaged> {
    Ok(uint(b)?.min(u32::MAX as u64) as u32)
}

fn flag(b: &[u8]) -> Result<bool, Damaged> {
    Ok(uint(b)? != 0)
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// An enum's kind and number, written by `kind`.
fn read_kind(b: &[u8]) -> Result<(u64, i32), Damaged> {
    let (mut k, mut v) = (0, 0);
    for (tag, f) in fields(b)? {
        match tag {
            1 => k = uint(f)?,
            2 => v = i32_of(f)?,
            _ => {}
        }
    }
    Ok((k, v))
}

fn read_chp(b: &[u8]) -> Result<CharProps, Damaged> {
    read_chp_in(b, true)
}

/// A character format, with its tracked change only when `tracked`.
fn read_chp_in(b: &[u8], tracked: bool) -> Result<CharProps, Damaged> {
    let mut c = CharProps::default();
    for (tag, f) in fields(b)? {
        match tag {
            1 => c.font = u16_of(f)?,
            2 => c.size = u16_of(f)?,
            3 => c.bold = flag(f)?,
            4 => c.italic = flag(f)?,
            5 => {
                c.underline = match uint(f)? {
                    1 => Underline::Single,
                    2 => Underline::Word,
                    3 => Underline::Double,
                    4 => Underline::Dotted,
                    _ => Underline::None,
                }
            }
            6 => c.strike = flag(f)?,
            7 => c.outline = flag(f)?,
            8 => c.shadow = flag(f)?,
            9 => c.small_caps = flag(f)?,
            10 => c.all_caps = flag(f)?,
            11 => c.hidden = flag(f)?,
            12 => c.color = Rgb(u32_of(f)? & 0xFF_FFFF),
            13 => c.position = i32_of(f)?,
            14 => c.spacing = i32_of(f)?,
            15 => c.object = u32_of(f)?,
            16 => c.link = read_link(f)?,
            17 if tracked => c.rev = read_revision(f)?,
            18 => c.comment = u32_of(f)?,
            _ => {}
        }
    }
    Ok(c)
}

fn read_revision(b: &[u8]) -> Result<u32, Damaged> {
    use quill_core::revision::{Revision, Stamp};
    let read_stamp = |b: &[u8]| -> Result<Stamp, Damaged> {
        let (mut author, mut date) = (String::new(), String::new());
        for (tag, f) in fields(b)? {
            match tag {
                1 => author = text(f),
                2 => date = text(f),
                _ => {}
            }
        }
        Ok(Stamp::new(&author, &date))
    };
    let mut r = Revision::default();
    let (mut by, mut old) = (None, None);
    for (tag, f) in fields(b)? {
        match tag {
            1 => r.inserted = Some(read_stamp(f)?),
            2 => r.deleted = Some(read_stamp(f)?),
            3 => by = Some(read_stamp(f)?),
            // A format change's old format is read without its own
            // revision, so a damaged file can't nest them.
            4 => old = Some(read_chp_in(f, false)?),
            _ => {}
        }
    }
    if let (Some(by), Some(old)) = (by, old) {
        r.formatted = Some((by, old));
    }
    Ok(quill_core::revision::intern(r))
}

fn read_link(b: &[u8]) -> Result<u32, Damaged> {
    let mut l = quill_core::Link::new("");
    for (tag, f) in fields(b)? {
        match tag {
            1 => l.url = text(f),
            2 => l.new_window = flag(f)?,
            3 => l.hover = Rgb(u32_of(f)? & 0xFF_FFFF),
            4 => l.visited = Rgb(u32_of(f)? & 0xFF_FFFF),
            _ => {}
        }
    }
    Ok(if l.url.is_empty() {
        0
    } else {
        quill_core::link::intern(l)
    })
}

fn read_border_line(b: &[u8]) -> Result<BorderLine, Damaged> {
    let mut l = BorderLine::default();
    for (tag, f) in fields(b)? {
        match tag {
            1 => {
                l.style = match uint(f)? {
                    1 => BorderStyle::Dotted,
                    2 => BorderStyle::Hairline,
                    3 => BorderStyle::Single,
                    4 => BorderStyle::Thick,
                    5 => BorderStyle::Double,
                    _ => BorderStyle::None,
                }
            }
            2 => l.space = i32_of(f)?,
            _ => {}
        }
    }
    Ok(l)
}

fn read_borders(b: &[u8]) -> Result<Borders, Damaged> {
    let mut x = Borders::default();
    for (tag, f) in fields(b)? {
        match tag {
            1 => x.top = read_border_line(f)?,
            2 => x.bottom = read_border_line(f)?,
            3 => x.left = read_border_line(f)?,
            4 => x.right = read_border_line(f)?,
            5 => x.between = read_border_line(f)?,
            6 => x.shadow = flag(f)?,
            _ => {}
        }
    }
    Ok(x)
}

fn read_rel(b: &[u8]) -> Result<FrameRel, Damaged> {
    Ok(match uint(b)? {
        1 => FrameRel::Margin,
        2 => FrameRel::Page,
        _ => FrameRel::Column,
    })
}

fn read_align(b: &[u8]) -> Result<Align, Damaged> {
    Ok(match uint(b)? {
        1 => Align::Center,
        2 => Align::Right,
        3 => Align::Justify,
        _ => Align::Left,
    })
}

fn read_frame(b: &[u8]) -> Result<Frame, Damaged> {
    let mut fr = Frame::default();
    for (tag, f) in fields(b)? {
        match tag {
            1 => {
                fr.h = match read_kind(f)? {
                    (1, _) => FrameH::Center,
                    (2, _) => FrameH::Right,
                    (3, _) => FrameH::Inside,
                    (4, _) => FrameH::Outside,
                    (5, t) => FrameH::At(t),
                    _ => FrameH::Left,
                }
            }
            2 => fr.h_rel = read_rel(f)?,
            3 => {
                fr.v = match read_kind(f)? {
                    (1, _) => FrameV::Top,
                    (2, _) => FrameV::Center,
                    (3, _) => FrameV::Bottom,
                    (4, t) => FrameV::At(t),
                    _ => FrameV::InLine,
                }
            }
            4 => fr.v_rel = read_rel(f)?,
            5 => fr.width = Some(i32_of(f)?),
            6 => fr.from_text = i32_of(f)?,
            7 => fr.drop_lines = uint(f)?.min(u8::MAX as u64) as u8,
            8 => fr.in_margin = flag(f)?,
            _ => {}
        }
    }
    Ok(fr)
}

fn read_pap(b: &[u8]) -> Result<ParaProps, Damaged> {
    let mut p = ParaProps::default();
    for (tag, f) in fields(b)? {
        match tag {
            1 => p.style = u16_of(f)?,
            2 => p.align = read_align(f)?,
            3 => p.left = i32_of(f)?,
            4 => p.right = i32_of(f)?,
            5 => p.first = i32_of(f)?,
            6 => p.before = i32_of(f)?,
            7 => p.after = i32_of(f)?,
            8 => {
                p.line = match read_kind(f)? {
                    (1, t) => LineSpacing::AtLeast(t),
                    (2, t) => LineSpacing::Exactly(t),
                    (3, m) => LineSpacing::Multiple(m.clamp(0, u16::MAX as i32) as u16),
                    _ => LineSpacing::Auto,
                }
            }
            9 => {
                let mut t = TabStop::default();
                for (tag, f) in fields(f)? {
                    match tag {
                        1 => t.pos = i32_of(f)?,
                        2 => {
                            t.align = match uint(f)? {
                                1 => TabAlign::Center,
                                2 => TabAlign::Right,
                                3 => TabAlign::Decimal,
                                4 => TabAlign::Bar,
                                _ => TabAlign::Left,
                            }
                        }
                        3 => {
                            t.leader = match uint(f)? {
                                1 => TabLeader::Dots,
                                2 => TabLeader::Dashes,
                                3 => TabLeader::Underline,
                                _ => TabLeader::None,
                            }
                        }
                        _ => {}
                    }
                }
                p.tabs.push(t);
            }
            10 => p.page_break_before = flag(f)?,
            11 => p.keep_with_next = flag(f)?,
            12 => p.keep_together = flag(f)?,
            13 => p.suppress_line_numbers = flag(f)?,
            14 => p.borders = read_borders(f)?,
            15 => p.shading = u16_of(f)?,
            16 => p.in_table = flag(f)?,
            17 => p.frame = Some(read_frame(f)?),
            _ => {}
        }
    }
    Ok(p)
}

fn read_row(b: &[u8]) -> Result<RowProps, Damaged> {
    let mut r = RowProps {
        cells: Vec::new(),
        gap: 0,
        indent: 0,
        height: RowHeight::Auto,
        align: Align::Left,
    };
    for (tag, f) in fields(b)? {
        match tag {
            1 => {
                let mut c = CellProps::default();
                for (tag, f) in fields(f)? {
                    match tag {
                        1 => c.width = i32_of(f)?,
                        2 => c.borders = read_borders(f)?,
                        3 => c.shading = u16_of(f)?,
                        _ => {}
                    }
                }
                r.cells.push(c);
            }
            2 => r.gap = i32_of(f)?,
            3 => r.indent = i32_of(f)?,
            4 => {
                r.height = match read_kind(f)? {
                    (1, t) => RowHeight::AtLeast(t),
                    (2, t) => RowHeight::Exactly(t),
                    _ => RowHeight::Auto,
                }
            }
            5 => r.align = read_align(f)?,
            _ => {}
        }
    }
    Ok(r)
}

fn read_section(b: &[u8]) -> Result<SectionProps, Damaged> {
    // A missing distance means none, not the default.
    let mut s = SectionProps {
        line_from_text: None,
        ..SectionProps::default()
    };
    for (tag, f) in fields(b)? {
        match tag {
            1 => {
                s.start = match uint(f)? {
                    0 => SectionStart::NoBreak,
                    1 => SectionStart::NewColumn,
                    3 => SectionStart::EvenPage,
                    4 => SectionStart::OddPage,
                    _ => SectionStart::NewPage,
                }
            }
            2 => s.columns = u16_of(f)?.max(1),
            3 => s.column_spacing = i32_of(f)?,
            4 => s.header_from_top = i32_of(f)?,
            5 => s.footer_from_bottom = i32_of(f)?,
            6 => s.different_first_page = flag(f)?,
            7 => {
                s.page_number_format = match uint(f)? {
                    1 => PageNumberFormat::UpperRoman,
                    2 => PageNumberFormat::LowerRoman,
                    3 => PageNumberFormat::UpperLetter,
                    4 => PageNumberFormat::LowerLetter,
                    _ => PageNumberFormat::Arabic,
                }
            }
            8 => s.restart_page_numbers = flag(f)?,
            9 => s.margin_page_numbers = flag(f)?,
            10 => s.page_number_from_top = i32_of(f)?,
            11 => s.page_number_from_right = i32_of(f)?,
            12 => s.include_endnotes = flag(f)?,
            13 => {
                s.line_numbers = match uint(f)? {
                    1 => LineNumbering::ByPage,
                    2 => LineNumbering::BySection,
                    3 => LineNumbering::Continuous,
                    _ => LineNumbering::Off,
                }
            }
            14 => s.line_count_by = u16_of(f)?.max(1),
            15 => s.line_from_text = Some(i32_of(f)?),
            16 => {
                let mut set = vec![Story::new(Paragraph::default()); StoryId::HEADERS.len()];
                for (tag, f) in fields(f)? {
                    if tag != 1 {
                        continue;
                    }
                    let (mut k, mut st) = (usize::MAX, None);
                    for (tag, f) in fields(f)? {
                        match tag {
                            1 => k = uint(f)?.min(usize::MAX as u64) as usize,
                            2 => st = Some(read_story(f)?),
                            _ => {}
                        }
                    }
                    if let (Some(slot), Some(st)) = (set.get_mut(k), st) {
                        *slot = st;
                    }
                }
                s.headers = Some(Arc::new(set));
            }
            _ => {}
        }
    }
    Ok(s)
}

fn read_document_props(b: &[u8]) -> Result<DocumentProps, Damaged> {
    let mut p = DocumentProps::default();
    for (tag, f) in fields(b)? {
        match tag {
            1 => p.page_width = i32_of(f)?,
            2 => p.page_height = i32_of(f)?,
            3 => p.margin_left = i32_of(f)?,
            4 => p.margin_right = i32_of(f)?,
            5 => p.margin_top = i32_of(f)?,
            6 => p.margin_bottom = i32_of(f)?,
            7 => p.top_exact = flag(f)?,
            8 => p.bottom_exact = flag(f)?,
            9 => p.gutter = i32_of(f)?,
            10 => p.mirror_margins = flag(f)?,
            11 => p.default_tab = i32_of(f)?,
            12 => p.widow_control = flag(f)?,
            13 => p.print_hidden = flag(f)?,
            14 => p.even_odd_headers = flag(f)?,
            15 => {
                p.footnote_position = match uint(f)? {
                    1 => FootnotePosition::BeneathText,
                    2 => FootnotePosition::EndOfSection,
                    3 => FootnotePosition::EndOfDocument,
                    _ => FootnotePosition::BottomOfPage,
                }
            }
            16 => p.footnote_restart_each_page = flag(f)?,
            17 => p.footnote_start = u16_of(f)?.max(1),
            18 => p.track_changes = flag(f)?,
            19 => {
                let mut t = quill_core::contents::TocOptions::default();
                for (tag, g) in fields(f)? {
                    match tag {
                        1 => t.outline = flag(g)?,
                        2 => t.page_numbers = flag(g)?,
                        3 => t.from = (uint(g)?.clamp(1, 9)) as u8,
                        4 => t.to = (uint(g)?.clamp(1, 9)) as u8,
                        _ => {}
                    }
                }
                t.to = t.to.max(t.from);
                p.live_toc = Some(t);
            }
            _ => {}
        }
    }
    Ok(p)
}

fn read_summary(b: &[u8]) -> Result<SummaryInfo, Damaged> {
    let mut s = SummaryInfo::default();
    for (tag, f) in fields(b)? {
        match tag {
            1 => s.title = text(f),
            2 => s.subject = text(f),
            3 => s.author = text(f),
            4 => s.version = text(f),
            5 => s.keywords = text(f),
            _ => {}
        }
    }
    Ok(s)
}

fn read_style(b: &[u8]) -> Result<Style, Damaged> {
    let mut s = Style {
        name: String::new(),
        based_on: None,
        next: 0,
        pap: ParaProps::default(),
        chp: CharProps::default(),
        builtin: false,
    };
    for (tag, f) in fields(b)? {
        match tag {
            1 => s.name = text(f),
            2 => s.based_on = Some(u16_of(f)?),
            3 => s.next = u16_of(f)?,
            4 => s.pap = read_pap(f)?,
            5 => s.chp = read_chp(f)?,
            6 => s.builtin = flag(f)?,
            _ => {}
        }
    }
    Ok(s)
}

fn read_paragraph(b: &[u8]) -> Result<Paragraph, Damaged> {
    let mut p = Paragraph::default();
    for (tag, f) in fields(b)? {
        match tag {
            1 => p.text = text(f),
            2 => {
                let mut run = Run {
                    len: 0,
                    chp: CharProps::default(),
                };
                for (tag, f) in fields(f)? {
                    match tag {
                        1 => run.len = uint(f)?.min(usize::MAX as u64) as usize,
                        2 => run.chp = read_chp(f)?,
                        _ => {}
                    }
                }
                p.runs.push(run);
            }
            3 => p.pap = read_pap(f)?,
            4 => {
                p.mark = match uint(f)? {
                    1 => Mark::CellEnd,
                    2 => Mark::RowEnd,
                    3 => Mark::SectionBreak,
                    _ => Mark::Paragraph,
                }
            }
            5 => p.mark_chp = read_chp(f)?,
            6 => p.row = Some(Box::new(read_row(f)?)),
            7 => p.section = Some(Box::new(read_section(f)?)),
            _ => {}
        }
    }
    Ok(p)
}

fn read_story(b: &[u8]) -> Result<Story, Damaged> {
    let mut paras = Vec::new();
    for (tag, f) in fields(b)? {
        if tag == 1 {
            paras.push(read_paragraph(f)?);
        }
    }
    Ok(Story { paras })
}

/// Read a `.qwp` file.
pub fn read(bytes: &[u8]) -> Result<Document, String> {
    let z = zip::Archive::open(bytes).map_err(|e| e.to_string())?;
    if z.read("mimetype").map_err(|e| e.to_string())?.as_deref() != Some(MIMETYPE.as_bytes()) {
        return Err("This is not a Quill document.".into());
    }
    let bin = z
        .read("document.bin")
        .map_err(|e| e.to_string())?
        .ok_or("The document is missing from the file.")?;
    let rest = bin
        .strip_prefix(MAGIC.as_slice())
        .ok_or("The document in the file is damaged.")?;
    let (Some(major), Some(_minor)) = (rest.get(0..2), rest.get(2..4)) else {
        return Err("The document in the file is damaged.".into());
    };
    let major = u16::from_le_bytes([major[0], major[1]]);
    if major > MAJOR {
        return Err(
            "This document was saved by a newer version of Quill. Open it there and save a \
             copy as Word Document."
                .into(),
        );
    }
    read_body(&rest[4..], &z).map_err(|_| "The document in the file is damaged.".to_string())
}

fn read_body(b: &[u8], z: &zip::Archive) -> Result<Document, Damaged> {
    let mut doc = Document::new();
    let mut fonts = Vec::new();
    let mut styles = Vec::new();
    let mut objects = Vec::new();
    // Picture files read so far, so entries sharing one share its data.
    let mut loaded: HashMap<String, Arc<[u8]>> = HashMap::new();
    for (tag, f) in fields(b)? {
        match tag {
            1 => fonts.push(text(f)),
            2 => styles.push(read_style(f)?),
            3 => doc.props = read_document_props(f)?,
            4 => doc.section = read_section(f)?,
            5 => doc.summary = read_summary(f)?,
            6 => doc.main = read_story(f)?,
            7 => doc.notes = read_story(f)?,
            8 => doc.annotations = read_story(f)?,
            9 => {
                let (mut k, mut s) = (usize::MAX, None);
                for (tag, f) in fields(f)? {
                    match tag {
                        1 => k = uint(f)?.min(usize::MAX as u64) as usize,
                        2 => s = Some(read_story(f)?),
                        _ => {}
                    }
                }
                if let (Some(&id), Some(s)) = (StoryId::HEADERS.get(k), s) {
                    *story_mut(&mut doc, id) = s;
                }
            }
            10 => objects.push(read_picture(f, z, &mut loaded)?),
            11 => {
                let (mut id, mut m) = (0, String::new());
                for (tag, f) in fields(f)? {
                    match tag {
                        1 => id = u32_of(f)?,
                        2 => m = text(f),
                        _ => {}
                    }
                }
                doc.note_marks.insert(id, m);
            }
            12 => {
                let (mut id, mut info) = (0, AnnotationInfo::default());
                for (tag, f) in fields(f)? {
                    match tag {
                        1 => id = u32_of(f)?,
                        2 => info.initials = text(f),
                        3 => info.author = text(f),
                        4 => info.date = text(f),
                        5 => info.parent = Some(u32_of(f)?),
                        6 => info.done = flag(f)?,
                        _ => {}
                    }
                }
                doc.annotation_info.insert(id, info);
            }
            _ => {}
        }
    }
    if !fonts.is_empty() {
        doc.fonts = fonts;
    }
    if !styles.is_empty() {
        doc.styles.styles = styles;
    }
    doc.objects = objects;
    repair(&mut doc);
    doc.forget_history();
    doc.mark_saved();
    Ok(doc)
}

/// The header or footer story `id`, to fill in.
fn story_mut(doc: &mut Document, id: StoryId) -> &mut Story {
    let k = StoryId::HEADERS.iter().position(|&s| s == id).unwrap_or(0);
    &mut doc.headers[k]
}

fn read_picture(
    b: &[u8],
    z: &zip::Archive,
    loaded: &mut HashMap<String, Arc<[u8]>>,
) -> Result<Object, Damaged> {
    let (mut format, mut pixels, mut natural, mut size) =
        (ImageFormat::Png, (0, 0), (0, 0), (0, 0));
    let mut crop = Crop::default();
    let mut file = String::new();
    let mut preview_file = String::new();
    let mut preview_pixels = None;
    for (tag, f) in fields(b)? {
        match tag {
            1 => {
                format = if uint(f)? == 1 {
                    ImageFormat::Jpeg
                } else {
                    ImageFormat::Png
                }
            }
            2 => pixels.0 = u32_of(f)?,
            3 => pixels.1 = u32_of(f)?,
            4 => natural.0 = i32_of(f)?,
            5 => natural.1 = i32_of(f)?,
            6 => size.0 = i32_of(f)?,
            7 => size.1 = i32_of(f)?,
            8 => crop.left = i32_of(f)?,
            9 => crop.top = i32_of(f)?,
            10 => crop.right = i32_of(f)?,
            11 => crop.bottom = i32_of(f)?,
            12 => file = text(f),
            13 => preview_file = text(f),
            14 => preview_pixels.get_or_insert((0, 0)).0 = u32_of(f)?,
            15 => preview_pixels.get_or_insert((0, 0)).1 = u32_of(f)?,
            _ => {}
        }
    }
    let data = match loaded.get(&file) {
        Some(d) => d.clone(),
        None => {
            let d: Arc<[u8]> = z.read(&file).map_err(|_| Damaged)?.ok_or(Damaged)?.into();
            loaded.insert(file, d.clone());
            d
        }
    };
    let preview = if preview_file.is_empty() {
        None
    } else {
        let d = match loaded.get(&preview_file) {
            Some(d) => d.clone(),
            None => {
                let d: Arc<[u8]> = z
                    .read(&preview_file)
                    .map_err(|_| Damaged)?
                    .ok_or(Damaged)?
                    .into();
                loaded.insert(preview_file, d.clone());
                d
            }
        };
        let (format, w, h, _, _) = quill_core::picture::image_info(&d).ok_or(Damaged)?;
        if format != ImageFormat::Png || preview_pixels != Some((w, h)) {
            return Err(Damaged);
        }
        Some(d)
    };
    Ok(Object::Picture(Picture {
        data,
        preview,
        preview_pixels,
        format,
        pixels,
        natural,
        size,
        crop,
    }))
}

/// Make what was read safe to use: every story has a paragraph, runs fit
/// their text, and font, style and picture numbers point at something.
fn repair(doc: &mut Document) {
    let fonts = doc.fonts.len().max(1) as u16;
    let styles = doc.styles.styles.len().max(1) as u16;
    let fix_chp = |c: &mut CharProps| {
        if c.font >= fonts {
            c.font = 0;
        }
    };
    let fix_pap = |p: &mut ParaProps| {
        if p.style >= styles {
            p.style = 0;
        }
    };
    for s in &mut doc.styles.styles {
        fix_pap(&mut s.pap);
        fix_chp(&mut s.chp);
        if s.based_on.is_some_and(|b| b >= styles) {
            s.based_on = None;
        }
        if s.next >= styles {
            s.next = 0;
        }
    }
    let fix_story = |story: &mut Story| {
        if story.paras.is_empty() {
            story.paras.push(Paragraph::default());
        }
        for p in &mut story.paras {
            fix_pap(&mut p.pap);
            fix_chp(&mut p.mark_chp);
            // Runs must cover the text exactly, on char boundaries, with
            // nothing empty; if not, the text keeps the first run's format.
            let total: usize = p.runs.iter().map(|r| r.len).sum();
            let mut at = 0;
            let sound = total == p.text.len()
                && p.runs.iter().all(|r| {
                    at += r.len;
                    r.len > 0 && p.text.is_char_boundary(at)
                });
            if !sound {
                let chp = p.runs.first().map_or(p.mark_chp, |r| r.chp);
                p.runs.clear();
                if !p.text.is_empty() {
                    p.runs.push(Run {
                        len: p.text.len(),
                        chp,
                    });
                }
            }
            // Neighboring runs with the same format become one.
            let mut merged: Vec<Run> = Vec::with_capacity(p.runs.len());
            for mut r in p.runs.drain(..) {
                fix_chp(&mut r.chp);
                match merged.last_mut() {
                    Some(last) if last.chp == r.chp => last.len += r.len,
                    _ => merged.push(r),
                }
            }
            p.runs = merged;
        }
    };
    fix_story(&mut doc.notes);
    fix_story(&mut doc.annotations);
    for h in &mut doc.headers {
        fix_story(h);
    }
    // Sections' own headers and footers, on their breaks and the last.
    let fix_section = |s: &mut SectionProps| {
        if let Some(set) = &mut s.headers {
            for st in Arc::make_mut(set) {
                fix_story(st);
            }
        }
    };
    fix_section(&mut doc.section);
    for p in &mut doc.main.paras {
        if let Some(s) = &mut p.section {
            fix_section(s);
        }
    }
    fix_story(&mut doc.main);
}
