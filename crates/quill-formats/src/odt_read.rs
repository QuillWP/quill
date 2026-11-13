//! Reading OpenDocument Text: .odt packages and flat .fodt files.
//!
//! Like the .docx reader, it keeps what the original can express and maps the
//! rest to the nearest 5.1 feature. List numbers and bullets become literal
//! text and a tab, character styles become direct formatting, and the text
//! of frames and shapes follows the paragraph they are anchored to. Text is
//! never dropped on purpose.

use crate::report::{self as notes, ImportReport};
use crate::xml::{self, Element, Node};
use crate::zip::Archive;
use quill_core::character_styles::CharacterFormat;
use quill_core::document::FootnotePosition;
use quill_core::notes::{AnnotationInfo, NoteKind};
use quill_core::para::{Mark, Paragraph, chars};
use quill_core::props::{
    Align, BorderLine, BorderStyle, Borders, CellProps, CharProps, Frame, FrameH, FrameRel, FrameV,
    LineSpacing, ParaProps, Rgb, RowHeight, RowProps, StyleId, TabAlign, TabLeader, TabStop, Twips,
    Underline,
};
use quill_core::{Document, Object, Picture, SectionStart, Story, StoryId};
use std::collections::HashMap;
use std::sync::Arc;

/// Columns, rows and repeated cells beyond these are not expanded, so a
/// hostile file cannot ask for millions of empty cells.
const MAX_COLUMNS: usize = 255;
const MAX_REPEATED_ROWS: usize = 1000;
/// The most rows a cell spanning down is read as covering.
const MAX_ROWS_SPANNED: i32 = 10_000;

/// Fields Quill keeps as their current text.
const FIELDS: &[&str] = &[
    "page-count",
    "page-continuation",
    "sequence",
    "sequence-ref",
    "chapter",
    "title",
    "subject",
    "description",
    "keywords",
    "author-name",
    "author-initials",
    "creator",
    "initial-creator",
    "file-name",
    "template-name",
    "user-defined",
    "variable-get",
    "variable-set",
    "variable-input",
    "user-field-get",
    "user-field-input",
    "expression",
    "text-input",
    "drop-down",
    "bookmark-ref",
    "reference-ref",
    "note-ref",
    "conditional-text",
    "database-display",
    "database-name",
    "database-row-number",
    "word-count",
    "character-count",
    "paragraph-count",
    "table-count",
    "image-count",
    "object-count",
    "editing-cycles",
    "editing-duration",
    "creation-date",
    "creation-time",
    "modification-date",
    "modification-time",
    "print-date",
    "print-time",
    "printed-by",
    "placeholder",
    "sender-firstname",
    "sender-lastname",
    "sender-company",
    "sender-street",
    "sender-city",
    "sender-email",
];

/// Read an .odt (or .ott) package.
pub fn read(data: &[u8]) -> Result<Document, String> {
    read_reported(data).map(|(doc, _)| doc)
}

/// Like `read`, with what could not be kept exactly.
pub fn read_reported(data: &[u8]) -> Result<(Document, ImportReport), String> {
    let zip = Archive::open(data).map_err(|e| e.to_string())?;
    let part = |name: &str| -> Result<Option<Element>, String> {
        match zip.read(name).map_err(|e| e.to_string())? {
            Some(bytes) => xml::parse(&bytes).map(Some),
            None => Ok(None),
        }
    };
    let encrypted = zip
        .read("META-INF/manifest.xml")
        .ok()
        .flatten()
        .is_some_and(|m| m.windows(15).any(|w| w == b"encryption-data"));
    if encrypted {
        return Err("the document is protected by a password, which Quill cannot open".into());
    }
    let content = part("content.xml")?.ok_or("the file has no content.xml")?;
    let styles = part("styles.xml")?;
    let meta = part("meta.xml")?;
    let settings = part("settings.xml")?;
    let pictures = |href: &str| -> Option<Arc<[u8]>> {
        let path = href.trim_start_matches("./");
        zip.read(path).ok().flatten().map(Into::into)
    };
    build(
        &content,
        styles.as_ref(),
        meta.as_ref(),
        settings.as_ref(),
        &pictures,
    )
}

/// Whether `bytes` look like a flat OpenDocument Text file: one XML
/// document holding styles and content together.
pub fn is_flat(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(8192)]);
    head.contains("<office:document ")
        && head.contains("urn:oasis:names:tc:opendocument:xmlns:office:1.0")
        && head.contains("application/vnd.oasis.opendocument.text")
}

/// Read a flat .fodt file, with what could not be kept exactly. Its
/// pictures are inline, as base64.
pub fn read_flat_reported(data: &[u8]) -> Result<(Document, ImportReport), String> {
    let root = xml::parse(data)?;
    if root.name != "document" {
        return Err("the file is not an OpenDocument document".into());
    }
    build(&root, Some(&root), Some(&root), Some(&root), &|_| None)
}

/// The styles a document defines, by family and name, and its fonts.
#[derive(Default)]
struct Styles<'a> {
    fonts: HashMap<&'a str, String>,
    named: HashMap<(&'a str, &'a str), &'a Element>,
    defaults: HashMap<&'a str, &'a Element>,
    /// Automatic styles of the body (content.xml) and of the master pages
    /// (styles.xml); the same name can mean different styles in each.
    body_auto: HashMap<(&'a str, &'a str), &'a Element>,
    master_auto: HashMap<(&'a str, &'a str), &'a Element>,
    body_lists: HashMap<&'a str, &'a Element>,
    master_lists: HashMap<&'a str, &'a Element>,
    named_lists: HashMap<&'a str, &'a Element>,
    page_layouts: HashMap<&'a str, &'a Element>,
    master_pages: Vec<&'a Element>,
    outline: Option<&'a Element>,
    notes_config: Vec<&'a Element>,
}

impl<'a> Styles<'a> {
    fn add_fonts(&mut self, root: &'a Element) {
        for decls in root.children_named("font-face-decls") {
            for f in decls.children_named("font-face") {
                if let (Some(name), Some(family)) = (f.attr("name"), f.attr("font-family")) {
                    self.fonts.insert(name, first_family(family));
                }
            }
        }
    }

    fn add_automatic(&mut self, root: &'a Element, master: bool) {
        for auto in root.children_named("automatic-styles") {
            for s in auto.elements() {
                let Some(name) = s.attr("name") else {
                    continue;
                };
                match s.name.as_str() {
                    "style" => {
                        let family = s.attr("family").unwrap_or("paragraph");
                        if master {
                            self.master_auto.insert((family, name), s);
                        } else {
                            self.body_auto.insert((family, name), s);
                        }
                    }
                    "list-style" => {
                        if master {
                            self.master_lists.insert(name, s);
                        } else {
                            self.body_lists.insert(name, s);
                        }
                    }
                    "page-layout" => {
                        self.page_layouts.insert(name, s);
                    }
                    _ => {}
                }
            }
        }
    }

    fn add_named(&mut self, root: &'a Element) {
        for styles in root.children_named("styles") {
            for s in styles.elements() {
                match s.name.as_str() {
                    "style" => {
                        if let Some(name) = s.attr("name") {
                            let family = s.attr("family").unwrap_or("paragraph");
                            self.named.insert((family, name), s);
                        }
                    }
                    "default-style" => {
                        self.defaults
                            .insert(s.attr("family").unwrap_or("paragraph"), s);
                    }
                    "list-style" => {
                        if let Some(name) = s.attr("name") {
                            self.named_lists.insert(name, s);
                        }
                    }
                    "outline-style" => self.outline = Some(s),
                    "notes-configuration" => self.notes_config.push(s),
                    _ => {}
                }
            }
        }
        for masters in root.children_named("master-styles") {
            self.master_pages
                .extend(masters.children_named("master-page"));
        }
    }
}

/// The character properties a text-properties element states, as a
/// character style's mask (see `CharacterFormat`).
fn stated(e: &Element) -> u16 {
    let has = |names: &[&str]| names.iter().any(|n| e.attr(n).is_some());
    let mut mask = 0;
    for (bit, names) in [
        (0, &["font-name", "font-family"][..]),
        (1, &["font-size"][..]),
        (2, &["font-weight"][..]),
        (3, &["font-style"][..]),
        (4, &["text-underline-style"][..]),
        (5, &["text-line-through-style"][..]),
        (6, &["text-outline"][..]),
        (7, &["text-shadow"][..]),
        (8, &["font-variant"][..]),
        (9, &["text-transform"][..]),
        (10, &["display"][..]),
        (11, &["color", "use-window-font-color"][..]),
        (13, &["letter-spacing"][..]),
    ] {
        if has(names) {
            mask |= 1 << bit;
        }
    }
    if let Some(v) = e.attr("text-position") {
        mask |= match script_of(v) {
            (_, quill_core::Script::None) => 1 << 12,
            _ => 1 << 14,
        };
    }
    mask
}

/// What an ODT text-position says: a superscript or subscript ("super",
/// "sub", or a shift with a smaller size, as LibreOffice writes one with
/// its own amounts), or else a shift by hand, in percent of the size,
/// which comes back with the script as the part not explained by it.
fn script_of(v: &str) -> (f64, quill_core::Script) {
    let mut words = v.split_whitespace();
    let first = words.next().unwrap_or("0%");
    let scale = words.next().and_then(percent).unwrap_or(100.0);
    match first {
        "super" => (0.0, quill_core::Script::Super),
        "sub" => (0.0, quill_core::Script::Sub),
        w => {
            let raise = percent(w).unwrap_or(0.0);
            if raise != 0.0 && scale < 100.0 {
                let script = if raise > 0.0 {
                    quill_core::Script::Super
                } else {
                    quill_core::Script::Sub
                };
                (0.0, script)
            } else {
                (raise, quill_core::Script::None)
            }
        }
    }
}

/// The first family in a font-family list, without its quotes.
fn first_family(v: &str) -> String {
    let first = v.split(',').next().unwrap_or(v).trim();
    first
        .trim_matches(|c| c == '\'' || c == '"')
        .trim()
        .to_string()
}

/// A paragraph style, resolved through its parents.
#[derive(Clone)]
struct ParaStyle {
    pap: ParaProps,
    chp: CharProps,
    id: StyleId,
    /// The paragraph's own style sets its left indent, which then wins
    /// over a list's.
    own_indent: bool,
    master: Option<String>,
    list_style: Option<String>,
    break_after: bool,
}

/// A list paragraph's label and indents.
#[derive(Default)]
struct ListLabel {
    text: Option<String>,
    left: Option<Twips>,
    first: Option<Twips>,
}

struct Reader<'a> {
    /// Stops a small file with repeated rows and cells from building
    /// more than Quill's limits allow.
    budget: crate::ReadBudget,
    /// Spaces made from `text:s` counts so far.
    repeated_spaces: usize,
    doc: Document,
    st: Styles<'a>,
    pictures: &'a dyn Fn(&str) -> Option<Arc<[u8]>>,
    picture_cache: HashMap<String, Option<Picture>>,
    /// Named paragraph style -> our style.
    styles: HashMap<&'a str, StyleId>,
    characters: HashMap<&'a str, u16>,
    base: (ParaProps, CharProps),
    resolved: HashMap<(bool, String), ParaStyle>,
    tabs_relative: bool,
    /// Reading the master pages (headers and footers), whose automatic
    /// styles are styles.xml's.
    in_master: bool,
    /// Reading the main text, not a note, annotation or header.
    main: bool,
    /// Comments met in the footnote being read. Quill keeps comments only
    /// in the main text, so these go on the note's mark.
    note_comments: Option<Vec<&'a Element>>,
    in_table: bool,
    /// How many tables deep the cell being read is: 0 outside any table.
    table_depth: u8,
    /// Text of frames and shapes, to follow the paragraph being read.
    pending: Vec<Paragraph>,
    /// Floating pictures and text boxes as framed paragraphs, to go before
    /// the paragraph they are anchored to.
    floating: Vec<Paragraph>,
    /// Reading the text of a frame, whose own frames stay in it.
    in_box: u32,
    break_next: bool,
    seen_paragraph: bool,
    first_master: Option<String>,
    counters: HashMap<(String, usize), i32>,
    next_id: u32,
    notes: HashMap<u32, Vec<Paragraph>>,
    footnotes: bool,
    endnotes: bool,
    annotations: HashMap<u32, Vec<Paragraph>>,
    columns_used: bool,
    report: ImportReport,
}

fn build<'a>(
    content: &'a Element,
    styles: Option<&'a Element>,
    meta: Option<&'a Element>,
    settings: Option<&'a Element>,
    pictures: &'a dyn Fn(&str) -> Option<Arc<[u8]>>,
) -> Result<(Document, ImportReport), String> {
    let mut st = Styles::default();
    st.add_fonts(content);
    if let Some(s) = styles {
        if !std::ptr::eq(s, content) {
            st.add_fonts(s);
        }
        st.add_named(s);
        st.add_automatic(s, true);
    }
    st.add_automatic(content, false);
    if styles.is_some_and(|s| std::ptr::eq(s, content)) {
        // A flat file has one set of automatic styles for both.
        st.master_auto = st.body_auto.clone();
        st.master_lists = st.body_lists.clone();
    }
    // A body other than text (damaged, or from another application) is
    // still read for whatever paragraphs it has.
    let body = content
        .child("body")
        .and_then(|b| b.child("text").or_else(|| b.elements().next()))
        .ok_or("the document has no text body")?;
    let mut r = Reader {
        budget: crate::ReadBudget::default(),
        repeated_spaces: 0,
        doc: Document::new(),
        st,
        pictures,
        picture_cache: HashMap::new(),
        styles: HashMap::new(),
        characters: HashMap::new(),
        base: (ParaProps::default(), CharProps::default()),
        resolved: HashMap::new(),
        tabs_relative: true,
        in_master: false,
        main: true,
        note_comments: None,
        in_table: false,
        table_depth: 0,
        pending: Vec::new(),
        floating: Vec::new(),
        in_box: 0,
        break_next: false,
        seen_paragraph: false,
        first_master: None,
        counters: HashMap::new(),
        next_id: 1,
        notes: HashMap::new(),
        footnotes: false,
        endnotes: false,
        annotations: HashMap::new(),
        columns_used: false,
        report: ImportReport::default(),
    };
    if let Some(s) = settings {
        r.read_settings(s);
    }
    if let Some(m) = meta.and_then(|m| m.child("meta")) {
        r.read_meta(m);
    }
    r.read_styles();
    let mut paras = Vec::new();
    r.read_block(body, &mut paras, false);
    r.budget.note_paragraph_count(paras.len());
    if let Some(why) = r.budget.too_large() {
        return Err(format!("the document is too large for Quill: {why}"));
    }
    r.read_page();
    if paras.is_empty() {
        let n = r.doc.styles.get(0);
        paras.push(Paragraph::new(n.pap.clone(), n.chp));
    }
    if paras.last().is_some_and(|p| p.pap.in_table) {
        let n = r.doc.styles.get(0);
        paras.push(Paragraph::new(n.pap.clone(), n.chp));
    }
    if r.columns_used {
        r.doc.section.start = SectionStart::NoBreak;
    }
    r.doc.main.paras = paras;
    if !r.notes.is_empty() {
        if r.endnotes && !r.footnotes {
            r.doc.props.footnote_position = FootnotePosition::EndOfDocument;
        } else if r.endnotes {
            r.report.note(notes::ENDNOTES);
        }
        let mut story = Vec::new();
        for id in quill_core::notes::note_ids(&r.doc.main.paras) {
            story.extend(r.notes.remove(&id).unwrap_or_default());
        }
        if !story.is_empty() {
            r.doc.set_notes(Story { paras: story });
        }
        r.doc.reconcile_notes();
    }
    if !r.annotations.is_empty() {
        let mut story = Vec::new();
        for id in quill_core::notes::annotation_ids(&r.doc.main.paras) {
            story.extend(r.annotations.remove(&id).unwrap_or_default());
        }
        if !story.is_empty() {
            r.doc.set_annotations(Story { paras: story });
        }
        r.doc.reconcile(NoteKind::Annotation);
    }
    r.doc.forget_history();
    // Headers, footers and notes are read after the body.
    if let Some(why) = r.budget.too_large() {
        return Err(format!("the document is too large for Quill: {why}"));
    }
    Ok((r.doc, r.report))
}

/// A length in twips: "2.5cm", "0.5in", "12pt", "3mm", "1pc", "96px".
pub fn length(v: &str) -> Option<Twips> {
    let v = v.trim();
    let split = v
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .unwrap_or(v.len());
    let (num, unit) = v.split_at(split);
    let n: f64 = num.trim().parse().ok()?;
    let per = match unit.trim() {
        "in" | "inch" => 1440.0,
        "cm" => 1440.0 / 2.54,
        "mm" => 144.0 / 2.54,
        "pt" | "" => 20.0,
        "pc" => 240.0,
        "px" => 15.0,
        _ => return None,
    };
    let t = (n * per).round();
    (t.abs() < 2.0e7).then_some(t as Twips)
}

fn percent(v: &str) -> Option<f64> {
    v.trim().strip_suffix('%')?.trim().parse().ok()
}

fn parse_color(v: &str) -> Option<Rgb> {
    let hex = v.trim().strip_prefix('#')?;
    Rgb::from_hex(hex)
}

/// Shading as the original's gray percentage, from a background color.
fn shading(v: &str) -> u16 {
    let Some(c) = parse_color(v) else {
        return 0;
    };
    let lum = (c.r() as u32 * 30 + c.g() as u32 * 59 + c.b() as u32 * 11) / 100;
    let pct = ((255 - lum) * 1000 / 255) as u16;
    quill_core::props::SHADING_STEPS
        .iter()
        .copied()
        .min_by_key(|s| s.abs_diff(pct))
        .unwrap_or(0)
}

/// A border like "0.5pt solid #000000", or "none".
fn border(v: Option<&str>, padding: Option<&str>) -> BorderLine {
    let Some(v) = v else {
        return BorderLine::default();
    };
    let mut width = None;
    let mut kind = "solid";
    let mut color = Rgb::BLACK;
    for word in v.split_whitespace() {
        if let Some(w) = length(word) {
            width = Some(w);
        } else if word.starts_with('#') {
            color = parse_color(word).unwrap_or(color);
        } else {
            kind = word;
        }
    }
    let width = width.unwrap_or(10);
    let style = match kind {
        "none" | "hidden" => BorderStyle::None,
        _ if width == 0 => BorderStyle::None,
        "double" => BorderStyle::Double,
        "dotted" | "dashed" | "dash" => BorderStyle::Dotted,
        _ if width < 10 => BorderStyle::Hairline,
        _ if width >= 50 => BorderStyle::Thick,
        _ => BorderStyle::Single,
    };
    BorderLine {
        style,
        space: if style == BorderStyle::None {
            0
        } else {
            padding.and_then(length).unwrap_or(0).max(0)
        },
        color,
    }
}

/// The four borders of a set of properties, with `fo:border` filling in
/// any side not given its own.
fn borders(e: &Element) -> Borders {
    let all = e.attr("border");
    let pad = e.attr("padding");
    let side = |s: &str| {
        border(
            e.attr(&format!("border-{s}")).or(all),
            e.attr(&format!("padding-{s}")).or(pad),
        )
    };
    Borders {
        top: side("top"),
        bottom: side("bottom"),
        left: side("left"),
        right: side("right"),
        between: BorderLine::default(),
        shadow: e.attr("shadow").is_some_and(|s| s != "none"),
    }
}

fn format_number(n: i32, fmt: &str) -> String {
    match fmt {
        "" => String::new(),
        "a" => crate::numfmt::letters(n, false),
        "A" => crate::numfmt::letters(n, true),
        "i" => crate::numfmt::roman(n).to_lowercase(),
        "I" => crate::numfmt::roman(n),
        _ => n.to_string(),
    }
}

/// Undo the `_20_` escapes in a style's internal name.
fn decode_name(name: &str) -> String {
    let mut out = String::new();
    let mut rest = name;
    while let Some(i) = rest.find('_') {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        match tail.find('_') {
            Some(j) if j > 0 && j <= 4 && tail[..j].chars().all(|c| c.is_ascii_hexdigit()) => {
                let code = u32::from_str_radix(&tail[..j], 16).ok();
                match code.and_then(char::from_u32) {
                    Some(c) => out.push(c),
                    None => out.push_str(&rest[i..i + j + 2]),
                }
                rest = &tail[j + 1..];
            }
            _ => {
                out.push('_');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Quill's name for an OpenDocument paragraph style, so LibreOffice's
/// standard styles land on the original's.
fn quill_style_name(display: &str) -> String {
    let lower = display.to_ascii_lowercase();
    match lower.as_str() {
        "standard" | "default" | "default paragraph style" | "default style" => {
            return "Normal".into();
        }
        "footnote" | "footnote text" => return "footnote text".into(),
        _ => {}
    }
    for (from, to) in [("contents ", "toc "), ("index ", "index ")] {
        if let Some(n) = lower.strip_prefix(from)
            && n.parse::<u8>().is_ok_and(|n| (1..=9).contains(&n))
        {
            return format!("{to}{n}");
        }
    }
    display.to_string()
}

pub(crate) fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            c if c.is_ascii_whitespace() => continue,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Whether collapsible white space was the last thing written, so more of
/// it collapses into it (ODF keeps one space for each run of them, and
/// none at the start of a paragraph).
struct Space(bool);

fn append(p: &mut Paragraph, text: &str, chp: CharProps) {
    if !text.is_empty() {
        let at = p.len();
        p.insert(at, text, chp);
    }
}

impl<'a> Reader<'a> {
    fn new_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// A style by family and name: this part's automatic ones first.
    fn style(&self, family: &str, name: &str) -> Option<&'a Element> {
        let auto = if self.in_master {
            &self.st.master_auto
        } else {
            &self.st.body_auto
        };
        auto.get(&(family, name))
            .or_else(|| self.st.named.get(&(family, name)))
            .copied()
    }

    fn list_style(&self, name: &str) -> Option<&'a Element> {
        let auto = if self.in_master {
            &self.st.master_lists
        } else {
            &self.st.body_lists
        };
        auto.get(name)
            .or_else(|| self.st.named_lists.get(name))
            .copied()
    }

    /// A style and its parents, the furthest parent first.
    fn chain(&self, family: &str, name: Option<&str>) -> Vec<&'a Element> {
        let mut out = Vec::new();
        let mut cur = name.and_then(|n| self.style(family, n));
        while let Some(s) = cur
            && out.len() < 20
        {
            out.insert(0, s);
            cur = s
                .attr("parent-style-name")
                .filter(|p| Some(*p) != s.attr("name"))
                .and_then(|p| self.st.named.get(&(family, p)).copied());
        }
        out
    }

    fn apply_text(&mut self, e: &Element, c: &mut CharProps) {
        let font = e
            .attr("font-name")
            .and_then(|n| self.st.fonts.get(n).cloned())
            .or_else(|| e.attr("font-family").map(first_family));
        if let Some(f) = font.filter(|f| !f.is_empty()) {
            c.font = self.doc.font_id(&f);
        }
        if let Some(v) = e.attr("font-size") {
            if let Some(pct) = percent(v) {
                c.size = ((c.size as f64 * pct / 100.0).round() as u16).clamp(2, 3200);
            } else if let Some(t) = length(v) {
                c.size = (t / 10).clamp(2, 3200) as u16;
            }
        }
        if let Some(v) = e.attr("font-weight") {
            c.bold = match v {
                "bold" | "bolder" => true,
                "normal" | "lighter" => false,
                n => n.parse::<u16>().is_ok_and(|n| n >= 600),
            };
        }
        if let Some(v) = e.attr("font-style") {
            c.italic = matches!(v, "italic" | "oblique");
        }
        if let Some(v) = e.attr("text-underline-style") {
            c.underline = if v == "none" {
                Underline::None
            } else if e.attr("text-underline-type") == Some("double") {
                Underline::Double
            } else if matches!(
                v,
                "dotted" | "dash" | "long-dash" | "dot-dash" | "dot-dot-dash"
            ) {
                Underline::Dotted
            } else if e.attr("text-underline-mode") == Some("skip-white-space") {
                Underline::Word
            } else {
                Underline::Single
            };
        }
        if let Some(v) = e.attr("text-line-through-style") {
            c.strike = v != "none";
        }
        if e.attr("use-window-font-color") == Some("true") {
            c.color = Rgb::BLACK;
        } else if let Some(color) = e.attr("color").and_then(parse_color) {
            c.color = color;
        }
        if let Some(v) = e.attr("font-variant") {
            c.small_caps = v == "small-caps";
        }
        if let Some(v) = e.attr("text-transform") {
            c.all_caps = v == "uppercase";
        }
        if let Some(v) = e.attr("letter-spacing") {
            c.spacing = length(v).unwrap_or(0);
        }
        if let Some(v) = e.attr("display") {
            c.hidden = v == "none";
        }
        if let Some(v) = e.attr("text-outline") {
            c.outline = v == "true";
        }
        if let Some(v) = e.attr("text-shadow") {
            c.shadow = v != "none";
        }
        if let Some(v) = e.attr("text-position") {
            let (raise, script) = script_of(v);
            c.script = script;
            c.position = (c.size as f64 * 10.0 * raise / 100.0).round() as Twips;
        }
    }

    fn apply_para(&self, e: &Element, p: &mut ParaProps) {
        if let Some(v) = e.attr("text-align") {
            p.align = match v {
                "center" => Align::Center,
                "end" | "right" => Align::Right,
                "justify" => Align::Justify,
                _ => Align::Left,
            };
        }
        let len = |name: &str| e.attr(name).and_then(length);
        if let Some(v) = len("margin-left").or_else(|| len("margin")) {
            p.left = v;
        }
        if let Some(v) = len("margin-right").or_else(|| len("margin")) {
            p.right = v;
        }
        if let Some(v) = len("margin-top").or_else(|| len("margin")) {
            p.before = v.max(0);
        }
        if let Some(v) = len("margin-bottom").or_else(|| len("margin")) {
            p.after = v.max(0);
        }
        if let Some(v) = len("text-indent") {
            p.first = v;
        }
        if let Some(v) = e.attr("line-height") {
            p.line = if v == "normal" {
                LineSpacing::Auto
            } else if let Some(pct) = percent(v) {
                let n = (pct * 240.0 / 100.0).round() as i32;
                if n == 240 {
                    LineSpacing::Auto
                } else {
                    LineSpacing::Multiple(n.clamp(1, 2400) as u16)
                }
            } else {
                length(v).map_or(LineSpacing::Auto, |t| LineSpacing::Exactly(t.abs()))
            };
        }
        if let Some(v) = len("line-height-at-least") {
            p.line = LineSpacing::AtLeast(v.max(0));
        }
        if let Some(v) = e.attr("break-before") {
            p.page_break_before = v == "page";
        }
        if let Some(v) = e.attr("keep-with-next") {
            p.keep_with_next = v == "always";
        }
        if let Some(v) = e.attr("keep-together") {
            p.keep_together = v == "always";
        }
        if let Some(v) = e.attr("background-color") {
            p.shading = shading(v);
            p.shading_color = parse_color(v);
        }
        if e.attrs.iter().any(|(k, _)| k.starts_with("border")) {
            p.borders = borders(e);
        }
        if let Some(tabs) = e.child("tab-stops") {
            p.tabs.clear();
            for t in tabs.children_named("tab-stop") {
                let Some(pos) = t.attr("position").and_then(length) else {
                    continue;
                };
                let align = match t.attr("type").unwrap_or("left") {
                    "center" => TabAlign::Center,
                    "right" => TabAlign::Right,
                    "char" => TabAlign::Decimal,
                    _ => TabAlign::Left,
                };
                let leader = match t.attr("leader-text") {
                    Some(".") | Some("·") => TabLeader::Dots,
                    Some("-") => TabLeader::Dashes,
                    Some("_") => TabLeader::Underline,
                    _ => match t.attr("leader-style").unwrap_or("none") {
                        "dotted" => TabLeader::Dots,
                        "dash" | "long-dash" => TabLeader::Dashes,
                        "solid" => TabLeader::Underline,
                        _ => TabLeader::None,
                    },
                };
                p.set_tab(TabStop { pos, align, leader });
            }
        }
    }

    fn read_settings(&mut self, root: &Element) {
        fn find<'e>(e: &'e Element, name: &str) -> Option<&'e Element> {
            for c in e.elements() {
                if c.name == "config-item" && c.attr("name") == Some(name) {
                    return Some(c);
                }
                if let Some(d) = find(c, name) {
                    return Some(d);
                }
            }
            None
        }
        if let Some(settings) = root.child("settings")
            && let Some(item) = find(settings, "TabsRelativeToIndent")
        {
            self.tabs_relative = item.text().trim() != "false";
        }
    }

    fn read_meta(&mut self, m: &Element) {
        let get = |name: &str| m.child(name).map(|x| x.text()).unwrap_or_default();
        let s = &mut self.doc.summary;
        s.title = get("title");
        s.subject = get("subject");
        s.author = Some(get("initial-creator"))
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| get("creator"));
        s.keywords = m
            .children_named("keyword")
            .map(|k| k.text())
            .collect::<Vec<_>>()
            .join(", ");
    }

    /// The document defaults, then each named paragraph style as one of
    /// ours.
    fn read_styles(&mut self) {
        let mut chp = CharProps {
            font: 0,
            size: 24,
            ..Default::default()
        };
        let mut pap = ParaProps::default();
        if let Some(d) = self.st.defaults.get("paragraph").copied() {
            if let Some(t) = d.child("text-properties") {
                self.apply_text(t, &mut chp);
            }
            if let Some(p) = d.child("paragraph-properties") {
                self.apply_para(p, &mut pap);
                if let Some(t) = p.attr("tab-stop-distance").and_then(length)
                    && t > 0
                {
                    self.doc.props.default_tab = t;
                }
            }
        }
        self.base = (pap, chp);
        let mut names: Vec<&'a str> = self
            .st
            .named
            .keys()
            .filter(|(f, _)| *f == "paragraph")
            .map(|(_, n)| *n)
            .collect();
        names.sort();
        for name in &names {
            let style = self.st.named[&("paragraph", *name)];
            let display = style
                .attr("display-name")
                .map(str::to_string)
                .unwrap_or_else(|| decode_name(name));
            let ours = quill_style_name(&display);
            let ps = self.resolve(Some(name));
            let id = match self
                .doc
                .styles
                .styles
                .iter()
                .position(|s| s.name.eq_ignore_ascii_case(&ours))
            {
                Some(existing) => existing as StyleId,
                None => self.doc.styles.add(&ours, 0, ps.pap.clone(), ps.chp),
            };
            let pap = self.finish_tabs(ps.pap);
            let s = &mut self.doc.styles.styles[id as usize];
            s.pap = pap;
            s.pap.style = id;
            s.chp = ps.chp;
            self.styles.insert(name, id);
        }
        if !self.styles.values().any(|&s| s == 0) {
            let s = &mut self.doc.styles.styles[0];
            s.pap = self.base.0.clone();
            s.chp = self.base.1;
        }
        for name in &names {
            let style = self.st.named[&("paragraph", *name)];
            let ours = self.styles[name];
            let parent = style
                .attr("parent-style-name")
                .and_then(|p| self.styles.get(p))
                .copied();
            let next = style
                .attr("next-style-name")
                .and_then(|n| self.styles.get(n))
                .copied();
            let s = &mut self.doc.styles.styles[ours as usize];
            if ours != 0 {
                s.based_on = parent.or(Some(0));
            }
            if let Some(n) = next {
                s.next = n;
            }
        }
        self.doc.styles.break_based_on_loops();
        let mut names: Vec<_> = self
            .st
            .named
            .keys()
            .filter(|(family, _)| *family == "text")
            .map(|(_, n)| *n)
            .collect();
        names.sort();
        for name in names {
            let style = self.st.named[&("text", name)];
            if let (Some(encoded), Some(font)) =
                (style.attr("character-format"), style.attr("character-font"))
            {
                if font.len() > 1024 {
                    self.report.note(notes::CHARACTER_STYLES);
                    continue;
                }
                let display = style.attr("display-name").unwrap_or(name);
                if let Some(mut format) = crate::character_styles::decode(encoded, 0) {
                    if display.trim().is_empty()
                        || display.chars().count() > 255
                        || self.doc.styles.characters.len() >= u16::MAX as usize
                        || self.doc.styles.find_character(display).is_some()
                    {
                        self.report.note(notes::CHARACTER_STYLES);
                        continue;
                    }
                    if format.mask & 1 != 0 {
                        if self.doc.fonts.len() >= u16::MAX as usize {
                            self.report.note(notes::CHARACTER_STYLES);
                            continue;
                        }
                        format.props.font = self.doc.font_id(font);
                    }
                    if let Some(id) = self.doc.styles.add_character(display, format) {
                        self.characters.insert(name, id);
                    } else {
                        self.report.note(notes::CHARACTER_STYLES);
                    }
                } else {
                    self.report.note(notes::CHARACTER_STYLES);
                }
            } else {
                // A character style from another program (LibreOffice,
                // OpenOffice): what its text properties say becomes the
                // definition.
                let display = style.attr("display-name").unwrap_or(name).to_string();
                if display.trim().is_empty()
                    || display.chars().count() > 255
                    || self.doc.styles.characters.len() >= u16::MAX as usize
                    || self.doc.styles.find_character(&display).is_some()
                {
                    self.report.note(notes::CHARACTER_STYLES);
                    continue;
                }
                let format = self.foreign_character_format(name);
                match self.doc.styles.add_character(&display, format) {
                    Some(id) => {
                        self.characters.insert(name, id);
                    }
                    None => self.report.note(notes::CHARACTER_STYLES),
                }
            }
        }
        self.resolved.clear();
    }

    /// The definition of named text style `name` from another program: the
    /// properties it and its parents state, read against Normal. On/off
    /// properties are kept only when on, since a character style switches
    /// the paragraph's value.
    fn foreign_character_format(&mut self, name: &'a str) -> CharacterFormat {
        let base = self.doc.styles.get(0).chp;
        let mut c = base;
        let mut mask = 0;
        for s in self.chain("text", Some(name)) {
            if let Some(t) = s.child("text-properties") {
                mask |= stated(t);
                self.apply_text(t, &mut c);
            }
        }
        for (bit, on) in [
            (2, c.bold),
            (3, c.italic),
            (5, c.strike),
            (6, c.outline),
            (7, c.shadow),
            (8, c.small_caps),
            (9, c.all_caps),
            (10, c.hidden),
        ] {
            if !on {
                mask &= !(1 << bit);
            }
        }
        CharacterFormat::new(c, mask)
    }

    /// Tab stops count from the paragraph's indent in LibreOffice's files;
    /// Quill's count from the margin.
    fn finish_tabs(&self, mut pap: ParaProps) -> ParaProps {
        if self.tabs_relative && pap.left != 0 {
            for t in &mut pap.tabs {
                t.pos += pap.left;
            }
        }
        pap
    }

    fn resolve(&mut self, name: Option<&str>) -> ParaStyle {
        let key = (self.in_master, name.unwrap_or_default().to_string());
        if let Some(done) = self.resolved.get(&key) {
            return done.clone();
        }
        let chain = self.chain("paragraph", name);
        let (mut pap, mut chp) = self.base.clone();
        let mut out = ParaStyle {
            pap: ParaProps::default(),
            chp: CharProps::default(),
            id: 0,
            own_indent: false,
            master: None,
            list_style: None,
            break_after: false,
        };
        for s in &chain {
            if let Some(p) = s.child("paragraph-properties") {
                self.apply_para(p, &mut pap);
                if let Some(v) = p.attr("break-after") {
                    out.break_after = v == "page";
                }
            }
            if let Some(t) = s.child("text-properties") {
                self.apply_text(t, &mut chp);
            }
            if let Some(l) = s.attr("list-style-name").filter(|l| !l.is_empty()) {
                out.list_style = Some(l.to_string());
            }
        }
        if let Some(leaf) = chain.last() {
            out.master = leaf
                .attr("master-page-name")
                .filter(|m| !m.is_empty())
                .map(str::to_string);
            let automatic = leaf
                .attr("name")
                .is_some_and(|n| !self.styles.contains_key(n));
            out.own_indent = automatic
                && leaf
                    .child("paragraph-properties")
                    .is_some_and(|p| p.attr("margin-left").is_some());
        }
        out.id = chain
            .iter()
            .rev()
            .find_map(|s| s.attr("name").and_then(|n| self.styles.get(n)))
            .copied()
            .unwrap_or(0);
        out.pap = pap;
        out.chp = chp;
        self.resolved.insert(key, out.clone());
        out
    }

    fn apply_text_style(&mut self, name: Option<&str>, c: &mut CharProps) {
        let chain = self.chain("text", name);
        for s in chain {
            let before = *c;
            if let Some(t) = s.child("text-properties") {
                self.apply_text(t, c);
            }
            if let Some(id) = s.attr("name").and_then(|n| self.characters.get(n)).copied() {
                c.character_style = id;
                c.direct_format = 0;
            } else if s
                .attr("name")
                .is_some_and(|name| self.st.named.contains_key(&("text", name)))
            {
                self.report.note(notes::CHARACTER_STYLES);
            } else if c.character_style != 0 {
                c.direct_format |=
                    quill_core::character_styles::CharacterFormat::difference(before, *c);
            }
            if let Some(name) = s.attr("character") {
                if let Some(id) = self.doc.styles.find_character(name) {
                    c.character_style = id;
                    c.direct_format = s
                        .attr("direct")
                        .and_then(|n| n.parse::<u16>().ok())
                        .unwrap_or(0)
                        & quill_core::character_styles::CharacterFormat::ALL;
                } else {
                    self.report.note(notes::CHARACTER_STYLES);
                }
            }
        }
    }

    /// Read block content (paragraphs, lists, tables, sections) into `out`.
    fn read_block(&mut self, e: &'a Element, out: &mut Vec<Paragraph>, in_table: bool) {
        for child in e.elements() {
            // Past the limits the file is refused when reading ends, so
            // nothing more is built from it.
            if self.budget.full() {
                return;
            }
            self.read_block_child(child, out, in_table);
        }
    }

    fn read_block_child(&mut self, child: &'a Element, out: &mut Vec<Paragraph>, in_table: bool) {
        match child.name.as_str() {
            "p" => {
                let p = self.read_paragraph(child, in_table, None);
                out.append(&mut self.floating);
                self.budget.add_paragraph(&p);
                out.push(p);
                out.append(&mut self.pending);
            }
            "h" => {
                let label = child
                    .int("outline-level")
                    .and_then(|level| self.outline_label(level));
                let p = self.read_paragraph(child, in_table, label);
                out.append(&mut self.floating);
                self.budget.add_paragraph(&p);
                out.push(p);
                out.append(&mut self.pending);
            }
            "list" => self.read_list(child, out, in_table, 0, None),
            "numbered-paragraph" => {
                let depth = child.int("level").unwrap_or(1).clamp(1, 10) as usize - 1;
                let style = child.attr("style-name");
                for p in child.elements().filter(|p| p.name == "p" || p.name == "h") {
                    let label = style.and_then(|s| self.list_label(s, depth, None));
                    let para = self.read_paragraph(p, in_table, label);
                    out.append(&mut self.floating);
                    self.budget.add_paragraph(&para);
                    out.push(para);
                    out.append(&mut self.pending);
                }
            }
            "table" => self.read_table(child, out),
            "section" => self.read_section(child, out, in_table),
            "frame" if self.frame_math(child).is_some() => {
                self.report.note(notes::EQUATIONS);
                let n = self.doc.styles.get(0);
                let mut p = Paragraph::new(n.pap.clone(), n.chp);
                p.pap.in_table = in_table;
                if let Some(m) = self.frame_math(child) {
                    crate::math::mathml(&m, &mut p, n.chp);
                }
                self.budget.add_paragraph(&p);
                out.push(p);
            }
            "frame"
                if self.can_float(child) && self.float_frame(child, self.doc.styles.get(0).chp) =>
            {
                out.append(&mut self.floating);
            }
            "frame" => {
                let picture = self.frame_picture(child);
                self.frame_notes(child, picture.is_some());
                if let Some(pic) = picture {
                    let n = self.doc.styles.get(0);
                    let (pap, chp) = (n.pap.clone(), n.chp);
                    let mut p = Paragraph::new(pap, chp);
                    p.pap.in_table = in_table;
                    let id = self.doc.add_object(Object::Picture(pic));
                    append(
                        &mut p,
                        &chars::OBJECT.to_string(),
                        CharProps { object: id, ..chp },
                    );
                    self.budget.add_paragraph(&p);
                    out.push(p);
                }
                self.read_block(child, out, in_table);
            }
            "tracked-changes" => {
                if child.elements().next().is_some() {
                    self.report.note(notes::TRACKED_CHANGES);
                }
            }
            "table-of-content" | "alphabetical-index" | "illustration-index" | "table-index"
            | "object-index" | "user-index" | "bibliography" => {
                self.report.note(notes::GENERATED_INDEXES);
                self.read_block(child, out, in_table);
            }
            // Index templates and declarations: nothing the reader sees
            // on the page.
            "table-of-content-source"
            | "alphabetical-index-source"
            | "illustration-index-source"
            | "table-index-source"
            | "object-index-source"
            | "user-index-source"
            | "bibliography-source"
            | "sequence-decls"
            | "variable-decls"
            | "user-field-decls"
            | "dde-connection-decls"
            | "forms"
            | "soft-page-break"
            | "title"
            | "desc"
            | "image-map"
            | "event-listeners" => {}
            _ => self.read_block(child, out, in_table),
        }
    }

    fn read_paragraph(
        &mut self,
        e: &'a Element,
        in_table: bool,
        label: Option<ListLabel>,
    ) -> Paragraph {
        let ps = self.resolve(e.attr("style-name"));
        let mut pap = ps.pap;
        pap.style = ps.id;
        pap.in_table = in_table;
        if self.main && !in_table {
            if !self.seen_paragraph {
                self.first_master = ps.master.clone();
                pap.page_break_before = false;
            } else if ps.master.is_some() || self.break_next {
                pap.page_break_before = true;
                if ps.master.is_some() && ps.master != self.first_master {
                    self.report.note(notes::PAGE_STYLES);
                }
            }
            self.seen_paragraph = true;
            self.break_next = ps.break_after;
        }
        if let Some(l) = &label
            && !ps.own_indent
        {
            if let Some(left) = l.left {
                pap.left = left;
            }
            if let Some(first) = l.first {
                pap.first = first;
            }
        }
        let pap = self.finish_tabs(pap);
        let mut p = Paragraph::new(pap, ps.chp);
        if let Some(text) = label.and_then(|l| l.text) {
            self.report.note(notes::NUMBERING);
            append(&mut p, &text, ps.chp);
        }
        let was = self.in_table;
        self.in_table = in_table;
        self.read_inline(e, &mut p, ps.chp, &mut Space(true));
        self.in_table = was;
        p
    }

    fn read_inline(&mut self, e: &'a Element, p: &mut Paragraph, chp: CharProps, ws: &mut Space) {
        for n in &e.children {
            let x = match n {
                Node::Text(t) => {
                    let mut text = String::with_capacity(t.len());
                    for c in t.chars() {
                        if matches!(c, ' ' | '\t' | '\n' | '\r') {
                            if !ws.0 {
                                text.push(' ');
                                ws.0 = true;
                            }
                        } else {
                            text.push(c);
                            ws.0 = false;
                        }
                    }
                    append(p, &text, chp);
                    continue;
                }
                Node::Element(x) => x,
            };
            let formula = (x.name == "frame").then(|| self.frame_math(x)).flatten();
            match x.name.as_str() {
                "frame" if formula.is_some() => {
                    self.report.note(notes::EQUATIONS);
                    if let Some(m) = &formula {
                        crate::math::mathml(m, p, chp);
                    }
                }
                "s" => {
                    let n = x.int("c").unwrap_or(1).clamp(1, 1000) as usize;
                    // A few bytes in the file make a thousand spaces, so
                    // stop adding them once they alone are past the limit.
                    // The file is refused when its paragraphs are counted.
                    if self.repeated_spaces <= quill_core::limits::MAX_CHARACTERS {
                        self.repeated_spaces += n;
                        append(p, &" ".repeat(n), chp);
                    }
                    ws.0 = false;
                }
                "tab" => {
                    append(p, &chars::TAB.to_string(), chp);
                    ws.0 = false;
                }
                "line-break" => {
                    append(p, &chars::LINE_BREAK.to_string(), chp);
                    ws.0 = false;
                }
                "span" | "a" => {
                    let mut c = chp;
                    match x.attr("href").filter(|_| x.name == "a") {
                        // A link to a place in this document, which Quill
                        // has no bookmarks for.
                        Some(href) if href.starts_with('#') => self.report.note(notes::HYPERLINKS),
                        Some(href) => {
                            c.link = quill_core::link::intern(quill_core::Link {
                                new_window: x.attr("target-frame-name") == Some("_blank"),
                                ..quill_core::Link::new(href)
                            });
                        }
                        None => {}
                    }
                    self.apply_text_style(x.attr("style-name"), &mut c);
                    self.read_inline(x, p, c, ws);
                }
                "hidden-text" => {
                    let c = CharProps {
                        hidden: x.attr("is-hidden") != Some("false"),
                        ..chp
                    };
                    self.read_inline(x, p, c, ws);
                }
                "page-number" => {
                    append(p, &chars::FIELD_PAGE.to_string(), chp);
                    ws.0 = false;
                }
                "date" | "time" if x.attr("fixed") != Some("true") => {
                    let c = if x.name == "date" {
                        chars::FIELD_DATE
                    } else {
                        chars::FIELD_TIME
                    };
                    append(p, &c.to_string(), chp);
                    ws.0 = false;
                }
                "note" => {
                    self.read_note(x, p, chp);
                    ws.0 = false;
                }
                "annotation" => {
                    self.read_annotation(x, p, chp);
                }
                "frame" if self.can_float(x) && self.float_frame(x, chp) => {}
                "frame" | "g" | "custom-shape" | "rect" | "ellipse" | "circle" | "polygon"
                | "polyline" | "path" | "line" | "connector" | "caption" | "measure"
                | "regular-polygon" | "control" => {
                    let picture = (x.name == "frame").then(|| self.frame_picture(x)).flatten();
                    if x.name == "frame" {
                        self.frame_notes(x, picture.is_some());
                    } else if x.name == "control" {
                        self.report.note(notes::CONTROLS);
                    } else {
                        self.report.note(notes::DRAWINGS);
                    }
                    if let Some(pic) = picture {
                        let id = self.doc.add_object(Object::Picture(pic));
                        append(
                            p,
                            &chars::OBJECT.to_string(),
                            CharProps { object: id, ..chp },
                        );
                        ws.0 = false;
                    }
                    // Text in a frame or shape follows the paragraph.
                    let mut held = std::mem::take(&mut self.pending);
                    let before = held.len();
                    let in_table = self.in_table;
                    self.read_block(x, &mut held, in_table);
                    self.in_table = in_table;
                    if held[before..].iter().any(|p| !p.text.is_empty()) {
                        self.report.note(notes::TEXT_BOXES);
                    }
                    self.pending = held;
                }
                // Word marks the last edit with a hidden _GoBack bookmark;
                // only bookmarks someone made are worth a note.
                "bookmark" | "bookmark-start"
                    if !x.attr("name").unwrap_or_default().starts_with('_') =>
                {
                    self.report.note(notes::BOOKMARKS);
                }
                "toc-mark"
                | "toc-mark-start"
                | "alphabetical-index-mark"
                | "alphabetical-index-mark-start"
                | "user-index-mark"
                | "user-index-mark-start" => {
                    self.report.note(notes::INDEX_ENTRIES);
                }
                // Quill cannot set the guide above its characters, so it
                // follows them in brackets.
                "ruby-text" => {
                    self.report.note(notes::RUBY);
                    let guide = x.text();
                    let guide = guide.trim();
                    if !guide.is_empty() {
                        let (open, close) = crate::docx::read::ruby_brackets(guide);
                        append(p, &format!("{open}{guide}{close}"), chp);
                    }
                }
                name if FIELDS.contains(&name) => {
                    self.report.note(notes::FIELDS);
                    self.read_inline(x, p, chp, ws);
                }
                "annotation-end"
                | "bookmark"
                | "bookmark-start"
                | "bookmark-end"
                | "reference-mark"
                | "reference-mark-start"
                | "reference-mark-end"
                | "toc-mark-end"
                | "alphabetical-index-mark-end"
                | "user-index-mark-end"
                | "bibliography-mark"
                | "soft-page-break"
                | "change"
                | "change-start"
                | "change-end"
                | "event-listeners" => {}
                _ => self.read_inline(x, p, chp, ws),
            }
            // As LibreOffice does, white space after any element starts a
            // new run, even after an empty one like a bookmark.
            ws.0 = false;
        }
    }

    /// Whether frame `f` floats and can become a Quill frame: in the main
    /// text, outside tables and other frames, as the original allows.
    fn can_float(&self, f: &Element) -> bool {
        self.main
            && !self.in_table
            && self.in_box == 0
            && !matches!(f.attr("anchor-type"), None | Some("as-char"))
    }

    /// A floating frame as framed paragraphs before its paragraph: its
    /// picture, then its text. False when it has neither.
    fn float_frame(&mut self, f: &'a Element, chp: CharProps) -> bool {
        // Text that runs through the frame does not flow round it: a frame
        // would push the text aside, so it stays in the text.
        let chain = self.chain("graphic", f.attr("style-name"));
        let wrap = chain
            .iter()
            .rev()
            .find_map(|s| s.child("graphic-properties").and_then(|g| g.attr("wrap")));
        if wrap == Some("run-through") {
            return false;
        }
        let picture = self.frame_picture(f);
        self.frame_notes(f, picture.is_some());
        let mut frame = self.odt_frame(f);
        let mut paras = Vec::new();
        if let Some(pic) = picture {
            let n = self.doc.styles.get(0);
            let mut p = Paragraph::new(n.pap.clone(), n.chp);
            let id = self.doc.add_object(Object::Picture(pic));
            append(
                &mut p,
                &chars::OBJECT.to_string(),
                CharProps { object: id, ..chp },
            );
            paras.push(p);
        } else {
            // A text box is as wide as it was drawn; a picture sets its own.
            frame.width = f
                .attr("width")
                .or_else(|| f.attr("min-width"))
                .and_then(length)
                .map(|w| w.clamp(144, 31_680));
        }
        let held = std::mem::take(&mut self.pending);
        let floating = std::mem::take(&mut self.floating);
        self.in_box += 1;
        let mut text = Vec::new();
        self.read_block(f, &mut text, false);
        self.in_box -= 1;
        text.append(&mut self.pending);
        self.pending = held;
        self.floating = floating;
        if text.iter().any(|p| !p.text.is_empty()) {
            paras.extend(text);
        }
        if paras.is_empty() {
            return false;
        }
        for p in &mut paras {
            p.pap.frame = Some(frame);
        }
        self.floating.extend(paras);
        true
    }

    /// Where a floating frame sits, from its graphic style and offsets.
    /// The original's frames cannot sit at an offset from a paragraph, so a
    /// frame placed by its paragraph goes where the paragraph is.
    fn odt_frame(&self, f: &Element) -> Frame {
        let chain = self.chain("graphic", f.attr("style-name"));
        let get = |name: &str| {
            chain
                .iter()
                .rev()
                .find_map(|s| s.child("graphic-properties").and_then(|g| g.attr(name)))
        };
        let hrel = get("horizontal-rel").unwrap_or("paragraph");
        let h_rel = match hrel {
            "page-content" => FrameRel::Margin,
            r if r.starts_with("page") => FrameRel::Page,
            _ => FrameRel::Column,
        };
        let h = match get("horizontal-pos").unwrap_or("center") {
            "left" => FrameH::Left,
            "right" => FrameH::Right,
            "inside" => FrameH::Inside,
            "outside" => FrameH::Outside,
            "from-left" | "from-inside" => FrameH::At(f.attr("x").and_then(length).unwrap_or(0)),
            _ => FrameH::Center,
        };
        let page = f.attr("anchor-type") == Some("page");
        let vrel = get("vertical-rel").unwrap_or(if page { "page" } else { "paragraph" });
        let v_rel = match vrel {
            "page" => Some(FrameRel::Page),
            "page-content" => Some(FrameRel::Margin),
            _ => None,
        };
        let v = match (v_rel, get("vertical-pos").unwrap_or("top")) {
            (None, _) => FrameV::InLine,
            (Some(_), "middle") => FrameV::Center,
            (Some(_), "bottom") => FrameV::Bottom,
            (Some(_), "from-top") => FrameV::At(f.attr("y").and_then(length).unwrap_or(0)),
            (Some(_), _) => FrameV::Top,
        };
        let margin = |name: &str| get(name).and_then(length).unwrap_or(0);
        Frame {
            h,
            h_rel,
            v,
            v_rel: v_rel.unwrap_or(FrameRel::Margin),
            width: None,
            from_text: margin("margin-left")
                .max(margin("margin-right"))
                .clamp(0, 1440),
            ..Frame::default()
        }
    }

    /// The MathML of a formula in frame `f`: inside the frame in a flat
    /// file, or in the object's own content.xml in a package.
    fn frame_math(&self, f: &Element) -> Option<Element> {
        let object = f.child("object")?;
        if let Some(math) = object.child("math") {
            return Some(math.clone());
        }
        let href = object.attr("href")?.trim_start_matches("./");
        let bytes = (self.pictures)(&format!("{}/content.xml", href.trim_end_matches('/')))?;
        let root = xml::parse(&bytes).ok()?;
        (root.name == "math").then_some(root)
    }

    /// Notes for a frame: a picture Quill cannot show, an embedded object,
    /// text in a text box, or a picture moved into the text.
    fn frame_notes(&mut self, f: &Element, shown: bool) {
        let has_image = f.child("image").is_some();
        if has_image && !shown {
            self.report.note(notes::PICTURES);
        }
        if !shown
            && f.elements().any(|c| {
                matches!(
                    c.name.as_str(),
                    "object" | "object-ole" | "plugin" | "applet"
                )
            })
        {
            self.report.note(notes::OBJECTS);
        }
        if shown && !matches!(f.attr("anchor-type"), None | Some("as-char")) {
            self.report.note(notes::ANCHORED);
        }
        if f.child("text-box")
            .is_some_and(|t| !t.text().trim().is_empty())
        {
            self.report.note(notes::TEXT_BOXES);
        }
    }

    /// The picture in a frame, if it holds one Quill can show. A frame can
    /// offer several images (an SVG and a PNG fallback); the first that
    /// decodes is used.
    fn frame_picture(&mut self, f: &Element) -> Option<Picture> {
        for img in f.children_named("image") {
            let picture = match img.attr("href") {
                Some(h) if !h.is_empty() => {
                    if let Some(cached) = self.picture_cache.get(h) {
                        cached.clone()
                    } else if !self.budget.add_picture(h) {
                        // Past the picture limit; the file is refused when
                        // reading ends.
                        return None;
                    } else {
                        let picture = (self.pictures)(h).and_then(crate::images::picture);
                        self.picture_cache.insert(h.to_string(), picture.clone());
                        picture
                    }
                }
                _ => img
                    .child("binary-data")
                    .and_then(|b| base64_decode(&b.text()))
                    .map(Arc::<[u8]>::from)
                    .and_then(crate::images::picture),
            };
            if let Some(mut pic) = picture {
                let w = f.attr("width").and_then(length);
                let h = f.attr("height").and_then(length);
                if let (Some(w), Some(h)) = (w, h) {
                    pic.size = (w.max(20), h.max(20));
                }
                return Some(pic);
            }
        }
        None
    }

    fn read_note(&mut self, x: &'a Element, p: &mut Paragraph, chp: CharProps) {
        if !self.main {
            // Quill keeps notes only in the main text; a note in a header
            // or another note keeps its text in place.
            self.report.note(notes::ASIDE_NOTES);
            if let Some(body) = x.child("note-body") {
                let mut ws = Space(false);
                for para in body.elements() {
                    self.read_inline(para, p, chp, &mut ws);
                }
            }
            return;
        }
        let id = self.new_id();
        if x.attr("note-class") == Some("endnote") {
            self.endnotes = true;
        } else {
            self.footnotes = true;
        }
        // A mark of its own: LibreOffice names it in text:label; other
        // writers just put it in the citation, where a number would be.
        let citation = x.child("note-citation");
        let label = citation
            .and_then(|c| c.attr("label"))
            .map(str::to_string)
            .or_else(|| {
                citation
                    .map(|c| c.text().trim().to_string())
                    .filter(|t| !t.is_empty() && !t.chars().all(|c| c.is_ascii_digit()))
                    .filter(|_| self.numbers_notes())
            })
            .filter(|l| !l.is_empty());
        if let Some(label) = label {
            self.doc.note_marks.insert(id, label);
        }
        let mark = CharProps {
            object: id,
            ..self.doc.note_mark_chp(chp)
        };
        append(p, &chars::FOOTNOTE.to_string(), mark);
        self.note_comments = Some(Vec::new());
        let mut paras = self.read_aside(x.child("note-body"));
        for comment in self.note_comments.take().unwrap_or_default() {
            self.report.note(notes::NOTE_COMMENTS);
            self.read_annotation(comment, p, chp);
        }
        if paras.is_empty() {
            let text = self.doc.styles.find("footnote text").unwrap_or(0);
            let st = self.doc.styles.get(text);
            let mut first = Paragraph::new(st.pap.clone(), st.chp);
            first.pap.style = text;
            paras.push(first);
        }
        let first = &mut paras[0];
        let c = self.doc.note_mark_chp(first.chp_at(0));
        first.insert(
            0,
            &chars::FOOTNOTE.to_string(),
            CharProps { object: id, ..c },
        );
        self.notes.insert(id, paras);
    }

    /// Footnotes are numbered 1, 2, 3, so a citation that is not a number
    /// is a mark of its own.
    fn numbers_notes(&self) -> bool {
        self.st
            .notes_config
            .iter()
            .filter(|c| c.attr("note-class") != Some("endnote"))
            .all(|c| c.attr("num-format").is_none_or(|f| f == "1"))
    }

    fn read_annotation(&mut self, x: &'a Element, p: &mut Paragraph, chp: CharProps) {
        if !self.main {
            match &mut self.note_comments {
                Some(held) => held.push(x),
                None => self.report.note(notes::ASIDE_COMMENTS),
            }
            return;
        }
        let id = self.new_id();
        let text = |name: &str| x.child(name).map(|c| c.text()).unwrap_or_default();
        let author = text("creator");
        // LibreOffice writes initials in its own namespace.
        let mut initials = text("creator-initials");
        if initials.is_empty() {
            initials = text("sender-initials");
        }
        if initials.is_empty() {
            initials = author
                .split_whitespace()
                .filter_map(|w| w.chars().next())
                .collect();
        }
        self.doc.annotation_info.insert(
            id,
            AnnotationInfo {
                initials,
                author,
                date: text("date").chars().take(10).collect(),
                ..Default::default()
            },
        );
        append(
            p,
            &chars::ANNOTATION.to_string(),
            CharProps { object: id, ..chp },
        );
        let mut paras = self.read_aside(Some(x));
        if paras.is_empty() {
            let n = self.doc.styles.get(0);
            paras.push(Paragraph::new(n.pap.clone(), n.chp));
        }
        let first = &mut paras[0];
        let c = first.chp_at(0);
        first.insert(
            0,
            &chars::ANNOTATION.to_string(),
            CharProps { object: id, ..c },
        );
        self.annotations.insert(id, paras);
    }

    /// The paragraphs of a note or annotation, read apart from the text
    /// around it.
    fn read_aside(&mut self, e: Option<&'a Element>) -> Vec<Paragraph> {
        let Some(e) = e else {
            return Vec::new();
        };
        let held = std::mem::take(&mut self.pending);
        let (main, in_table, break_next) = (self.main, self.in_table, self.break_next);
        self.main = false;
        let mut paras = Vec::new();
        self.read_block(e, &mut paras, false);
        self.main = main;
        self.in_table = in_table;
        self.break_next = break_next;
        self.pending = held;
        paras
    }

    /// The number or bullet a list paragraph starts with, and the list
    /// level's indents.
    fn list_label(&mut self, name: &str, depth: usize, start: Option<i32>) -> Option<ListLabel> {
        let style = self.list_style(name)?;
        let level = (depth + 1).min(10);
        let levels: Vec<&Element> = style
            .elements()
            .filter(|e| e.name.starts_with("list-level-style-"))
            .collect();
        let lvl = levels
            .iter()
            .find(|e| e.int("level") == Some(level as i32))
            .copied()?;
        Some(self.label_for(name, &levels, lvl, level, start))
    }

    fn label_for(
        &mut self,
        key: &str,
        levels: &[&Element],
        lvl: &Element,
        level: usize,
        start: Option<i32>,
    ) -> ListLabel {
        let mut out = ListLabel::default();
        let mut followed = "listtab";
        if let Some(props) = lvl.child("list-level-properties") {
            if let Some(align) = props.child("list-level-label-alignment") {
                out.left = align.attr("margin-left").and_then(length);
                out.first = align.attr("text-indent").and_then(length);
                followed = align.attr("label-followed-by").unwrap_or("listtab");
            } else {
                let before = props.attr("space-before").and_then(length).unwrap_or(0);
                let width = props.attr("min-label-width").and_then(length).unwrap_or(0);
                if before != 0 || width != 0 {
                    out.left = Some(before + width);
                    out.first = Some(-width);
                }
            }
        }
        let level_start = |e: &Element| e.int("start-value").unwrap_or(1);
        let n = {
            let c = self
                .counters
                .entry((key.to_string(), level))
                .or_insert(level_start(lvl) - 1);
            *c = match start {
                Some(s) => s,
                None => *c + 1,
            };
            *c
        };
        for deeper in level + 1..=10 {
            self.counters.remove(&(key.to_string(), deeper));
        }
        let text = if lvl.name == "list-level-style-number" {
            let fmt = lvl.attr("num-format").unwrap_or("");
            let shown = lvl
                .int("display-levels")
                .unwrap_or(1)
                .clamp(1, level as i32) as usize;
            let mut parts = Vec::new();
            for k in level + 1 - shown..=level {
                let e = levels
                    .iter()
                    .find(|e| e.int("level") == Some(k as i32))
                    .copied();
                let value = if k == level {
                    n
                } else {
                    self.counters
                        .get(&(key.to_string(), k))
                        .copied()
                        .unwrap_or_else(|| e.map_or(1, level_start))
                };
                let f = if k == level {
                    fmt
                } else {
                    e.and_then(|e| e.attr("num-format")).unwrap_or("1")
                };
                let s = format_number(value, f);
                if !s.is_empty() {
                    parts.push(s);
                }
            }
            let prefix = lvl.attr("num-prefix").unwrap_or("");
            let suffix = lvl.attr("num-suffix").unwrap_or("");
            format!("{prefix}{}{suffix}", parts.join("."))
        } else {
            match lvl.attr("bullet-char").and_then(|b| b.chars().next()) {
                // Symbol and Wingdings bullets live in the private use area.
                Some(c) if ('\u{E000}'..='\u{F8FF}').contains(&c) || c == '·' => "•".into(),
                Some(c) => c.to_string(),
                None => "•".into(),
            }
        };
        if !text.is_empty() {
            let gap = match followed {
                "space" => " ",
                "nothing" => "",
                _ => "\t",
            };
            out.text = Some(format!("{text}{gap}"));
        }
        out
    }

    /// A heading's number from the outline style, if it numbers that level.
    fn outline_label(&mut self, level: i32) -> Option<ListLabel> {
        let outline = self.st.outline?;
        let level = level.clamp(1, 10) as usize;
        let levels: Vec<&Element> = outline
            .elements()
            .filter(|e| e.name.starts_with("outline-level-style"))
            .collect();
        let lvl = levels
            .iter()
            .find(|e| e.int("level") == Some(level as i32))
            .copied()?;
        let numbered = !lvl.attr("num-format").unwrap_or("").is_empty()
            || !lvl.attr("num-prefix").unwrap_or("").is_empty()
            || !lvl.attr("num-suffix").unwrap_or("").is_empty();
        if !numbered {
            return None;
        }
        // Read the outline levels as numbered list levels.
        let mut label = self.label_for("\u{0}outline", &levels, lvl, level, None);
        // Headings keep their own style's indents.
        label.left = None;
        label.first = None;
        Some(label)
    }

    fn read_list(
        &mut self,
        l: &'a Element,
        out: &mut Vec<Paragraph>,
        in_table: bool,
        depth: usize,
        inherited: Option<&'a str>,
    ) {
        let style = l.attr("style-name").or(inherited);
        if depth == 0
            && l.attr("continue-numbering") != Some("true")
            && l.attr("continue-list").is_none()
            && let Some(s) = style
        {
            self.counters.retain(|(k, _), _| k != s);
        }
        for item in l.elements() {
            let header = match item.name.as_str() {
                "list-item" => false,
                "list-header" => true,
                _ => continue,
            };
            let mut start = item.int("start-value");
            let mut first = true;
            for c in item.elements() {
                match c.name.as_str() {
                    "p" | "h" => {
                        let own = || {
                            self.resolve(c.attr("style-name"))
                                .list_style
                                .map(|s| s.to_string())
                        };
                        let name = style.map(str::to_string).or_else(own);
                        let mut label = name
                            .as_deref()
                            .and_then(|n| self.list_label_peek(n, depth, first && !header, start));
                        if first && !header {
                            start = None;
                        }
                        if !(first && !header)
                            && let Some(l) = &mut label
                        {
                            l.text = None;
                        }
                        first = false;
                        let p = self.read_paragraph(c, in_table, label);
                        out.append(&mut self.floating);
                        self.budget.add_paragraph(&p);
                        out.push(p);
                        out.append(&mut self.pending);
                    }
                    "list" => {
                        first = false;
                        self.read_list(c, out, in_table, depth + 1, style);
                    }
                    _ => {
                        first = false;
                        self.read_block_child(c, out, in_table);
                    }
                }
            }
        }
    }

    /// A list label, counting the paragraph only when it carries the
    /// item's number; later paragraphs of an item get the indents alone.
    fn list_label_peek(
        &mut self,
        name: &str,
        depth: usize,
        counts: bool,
        start: Option<i32>,
    ) -> Option<ListLabel> {
        if counts {
            return self.list_label(name, depth, start);
        }
        let saved = self.counters.clone();
        let label = self.list_label(name, depth, start);
        self.counters = saved;
        label
    }

    fn table_rows(e: &'a Element, out: &mut Vec<&'a Element>) {
        for c in e.elements() {
            match c.name.as_str() {
                "table-row" => {
                    let n = c
                        .int("number-rows-repeated")
                        .unwrap_or(1)
                        .clamp(1, MAX_REPEATED_ROWS as i32);
                    for _ in 0..n {
                        if out.len() < 32_767 {
                            out.push(c);
                        }
                    }
                }
                "table-header-rows" | "table-rows" | "table-row-group" => Self::table_rows(c, out),
                _ => {}
            }
        }
    }

    fn table_columns(&self, e: &'a Element, out: &mut Vec<(Option<Twips>, Option<f64>)>) {
        for c in e.elements() {
            match c.name.as_str() {
                "table-column" => {
                    let props = c
                        .attr("style-name")
                        .and_then(|s| self.style("table-column", s))
                        .and_then(|s| s.child("table-column-properties"));
                    let width = props.and_then(|p| p.attr("column-width")).and_then(length);
                    let rel = props
                        .and_then(|p| p.attr("rel-column-width"))
                        .and_then(|v| v.trim_end_matches('*').trim().parse::<f64>().ok());
                    let n = c.int("number-columns-repeated").unwrap_or(1).clamp(1, 64);
                    for _ in 0..n {
                        if out.len() < MAX_COLUMNS {
                            out.push((width, rel));
                        }
                    }
                }
                "table-columns" | "table-header-columns" | "table-column-group" => {
                    self.table_columns(c, out)
                }
                _ => {}
            }
        }
    }

    fn read_table(&mut self, t: &'a Element, out: &mut Vec<Paragraph>) {
        let mut rows = Vec::new();
        Self::table_rows(t, &mut rows);
        if self.table_depth >= quill_core::limits::MAX_TABLE_LEVEL {
            self.report.note(notes::NESTED_TABLES);
            // Past the depth Quill keeps, the inner table's text becomes
            // paragraphs of the cell it is in.
            for row in rows {
                for cell in row.children_named("table-cell") {
                    self.read_block(cell, out, true);
                }
            }
            return;
        }
        self.table_depth += 1;
        let props = t
            .attr("style-name")
            .and_then(|s| self.style("table", s))
            .and_then(|s| s.child("table-properties"));
        let text_width = self.doc.props.text_width();
        let table_width = props
            .and_then(|p| p.attr("width"))
            .and_then(length)
            .unwrap_or(text_width);
        let margin_left = props
            .and_then(|p| p.attr("margin-left"))
            .and_then(length)
            .unwrap_or(0);
        let align = match props.and_then(|p| p.attr("align")) {
            Some("center") => Align::Center,
            Some("right") => Align::Right,
            _ => Align::Left,
        };
        let mut columns = Vec::new();
        self.table_columns(t, &mut columns);
        let rel_total: f64 = columns.iter().filter_map(|c| c.1).sum();
        let widths: Vec<Twips> = columns
            .iter()
            .map(|&(abs, rel)| match (abs, rel) {
                (Some(w), _) if w > 0 => w,
                (_, Some(r)) if rel_total > 0.0 => {
                    (table_width as f64 * r / rel_total).round() as Twips
                }
                _ => 0,
            })
            .collect();
        let mut gap = None;
        // Cells spanning rows down: by grid column, how many more rows they
        // cover, how many columns wide, and their settings, which the cells
        // under them take.
        let mut down: Vec<Option<(usize, usize, CellProps)>> = Vec::new();
        for row in rows {
            if !self.budget.note_paragraph_count(out.len()) {
                break;
            }
            let mut cells = Vec::new();
            let mut row_chp = self.doc.styles.get(0).chp;
            let mut col = 0usize;
            for cell in row.elements() {
                let covered = match cell.name.as_str() {
                    "table-cell" => false,
                    "covered-table-cell" => true,
                    _ => continue,
                };
                let repeat = cell
                    .int("number-columns-repeated")
                    .unwrap_or(1)
                    .clamp(1, MAX_COLUMNS as i32) as usize;
                if covered {
                    // Under a cell spanning rows: an empty cell continuing
                    // it, as Quill and Word keep a vertical merge. Beside a
                    // cell spanning columns: nothing.
                    let mut c = 0;
                    while c < repeat {
                        match down.get(col).cloned().flatten() {
                            Some((left, cols, props)) => {
                                let n = self.doc.styles.get(0);
                                let mut p = Paragraph::new(n.pap.clone(), n.chp);
                                p.pap.set_level(self.table_depth);
                                p.mark = Mark::CellEnd;
                                self.budget.add_paragraph(&p);
                                out.push(p);
                                cells.push(CellProps {
                                    width: widths.iter().skip(col).take(cols).sum(),
                                    vmerge: quill_core::props::VMerge::Continue,
                                    ..props.clone()
                                });
                                down[col] = (left > 1).then_some((left - 1, cols, props));
                                col += cols;
                                c += cols;
                            }
                            None => {
                                col += 1;
                                c += 1;
                            }
                        }
                    }
                    continue;
                }
                let rows_spanned = cell
                    .int("number-rows-spanned")
                    .unwrap_or(1)
                    .clamp(1, MAX_ROWS_SPANNED) as usize;
                let span = cell
                    .int("number-columns-spanned")
                    .unwrap_or(1)
                    .clamp(1, MAX_COLUMNS as i32) as usize;
                let cp = cell
                    .attr("style-name")
                    .and_then(|s| self.style("table-cell", s))
                    .and_then(|s| s.child("table-cell-properties"));
                if gap.is_none() {
                    let pad = |side: &str| {
                        cp.and_then(|p| p.attr(&format!("padding-{side}")).or(p.attr("padding")))
                            .and_then(length)
                    };
                    gap = Some(match (pad("left"), pad("right")) {
                        (Some(l), Some(r)) => l + r,
                        _ => 114,
                    });
                }
                for _ in 0..repeat {
                    if !self.budget.note_paragraph_count(out.len()) {
                        break;
                    }
                    // Past the most cells a row can have, the text joins
                    // the row's last cell rather than being lost.
                    let full = cells.len() >= MAX_COLUMNS;
                    if full {
                        self.report.note(notes::WIDE_ROWS);
                        if let Some(last) = out.last_mut() {
                            last.mark = Mark::Paragraph;
                        }
                    }
                    let start = out.len();
                    self.read_block(cell, out, true);
                    // Paragraphs read in a cell are in a table; this deep.
                    for p in &mut out[start..] {
                        if p.pap.in_table && p.pap.level() < self.table_depth {
                            p.pap.set_level(self.table_depth);
                        }
                    }
                    if out.len() == start && full {
                        // Nothing to add to the last cell.
                        if let Some(last) = out.last_mut() {
                            last.mark = Mark::CellEnd;
                        }
                        continue;
                    }
                    // A cell ends in a paragraph of its own, even after a
                    // table in it.
                    if out.len() == start
                        || out.last().is_some_and(|p| p.pap.level() > self.table_depth)
                    {
                        let n = self.doc.styles.get(0);
                        let mut p = Paragraph::new(n.pap.clone(), n.chp);
                        p.pap.set_level(self.table_depth);
                        self.budget.add_paragraph(&p);
                        out.push(p);
                    }
                    let last = out.last_mut().unwrap();
                    last.mark = Mark::CellEnd;
                    row_chp = last.mark_chp;
                    let width: Twips = widths.iter().skip(col).take(span).sum();
                    let at = col;
                    col += span;
                    if full {
                        continue;
                    }
                    cells.push(CellProps {
                        width,
                        borders: cp.map(borders).unwrap_or_default(),
                        shading: cp
                            .and_then(|p| p.attr("background-color"))
                            .map_or(0, shading),
                        shading_color: cp
                            .and_then(|p| p.attr("background-color"))
                            .and_then(parse_color),
                        valign: match cp.and_then(|p| p.attr("vertical-align")) {
                            Some("middle") => quill_core::props::VAlign::Center,
                            Some("bottom") => quill_core::props::VAlign::Bottom,
                            _ => quill_core::props::VAlign::Top,
                        },
                        vmerge: if rows_spanned > 1 {
                            quill_core::props::VMerge::Restart
                        } else {
                            quill_core::props::VMerge::None
                        },
                    });
                    if rows_spanned > 1 && repeat == 1 {
                        if down.len() < at + 1 {
                            down.resize(at + 1, None);
                        }
                        let props = cells.last().cloned().unwrap_or_default();
                        down[at] = Some((rows_spanned - 1, span, props));
                    }
                }
            }
            if cells.is_empty() {
                continue;
            }
            let n = cells.len() as Twips;
            for c in &mut cells {
                if c.width <= 0 {
                    c.width = table_width / n;
                }
            }
            let rp = row
                .attr("style-name")
                .and_then(|s| self.style("table-row", s))
                .and_then(|s| s.child("table-row-properties"));
            let height = if let Some(h) = rp.and_then(|p| p.attr("row-height")).and_then(length) {
                RowHeight::Exactly(h)
            } else if let Some(h) = rp.and_then(|p| p.attr("min-row-height")).and_then(length) {
                RowHeight::AtLeast(h)
            } else {
                RowHeight::Auto
            };
            let gap = gap.unwrap_or(114);
            let n = self.doc.styles.get(0);
            let mut row_end = Paragraph::new(n.pap.clone(), row_chp);
            row_end.pap.set_level(self.table_depth);
            row_end.mark = Mark::RowEnd;
            row_end.row = Some(Box::new(RowProps {
                cells,
                gap,
                indent: if align == Align::Left {
                    margin_left + gap / 2
                } else {
                    gap / 2
                },
                height,
                align,
            }));
            self.budget.add_paragraph(&row_end);
            out.push(row_end);
        }
        self.table_depth -= 1;
    }

    /// A section in columns becomes a Word section of its own; any other
    /// section is just its text.
    fn read_section(&mut self, s: &'a Element, out: &mut Vec<Paragraph>, in_table: bool) {
        // A section linked to another file (as in a master document) shows
        // the copy saved here; the linked file is never opened.
        if s.child("section-source").is_some() {
            self.report.note(notes::LINKED_SECTIONS);
        }
        let columns = s
            .attr("style-name")
            .and_then(|n| self.style("section", n))
            .and_then(|st| st.child("section-properties"))
            .and_then(|p| p.child("columns"));
        let count = columns
            .and_then(|c| c.int("column-count"))
            .unwrap_or(1)
            .clamp(1, 12) as u16;
        if count <= 1 || in_table || !self.main {
            self.read_block(s, out, in_table);
            return;
        }
        let spacing = columns
            .and_then(|c| c.attr("column-gap"))
            .and_then(length)
            .unwrap_or(self.doc.section.column_spacing);
        let outer = self.doc.section.clone();
        if let Some(last) = out.last_mut()
            && last.mark == Mark::Paragraph
        {
            last.mark = Mark::SectionBreak;
            last.section = Some(Box::new(outer.clone()));
        }
        let start = out.len();
        self.read_block(s, out, in_table);
        if out.len() > start
            && let Some(last) = out.last_mut()
            && last.mark == Mark::Paragraph
        {
            last.mark = Mark::SectionBreak;
            last.section = Some(Box::new(quill_core::SectionProps {
                columns: count,
                column_spacing: spacing,
                start: SectionStart::NoBreak,
                ..outer
            }));
            self.columns_used = true;
        }
    }

    /// The page size, margins, columns, headers and footers, from the
    /// master page the text starts on.
    fn read_page(&mut self) {
        let masters = self.st.master_pages.clone();
        let find = |name: &str| {
            masters
                .iter()
                .find(|m| m.attr("name") == Some(name))
                .copied()
        };
        let first = self
            .first_master
            .as_deref()
            .and_then(find)
            .or_else(|| find("Standard"))
            .or_else(|| masters.first().copied());
        let Some(first) = first else {
            return;
        };
        // LibreOffice's First Page style leads on to another for the rest.
        let next = first
            .attr("next-style-name")
            .filter(|n| Some(*n) != first.attr("name"))
            .and_then(find);
        let (title, main) = match next {
            Some(n) => (Some(first), n),
            None => (None, first),
        };
        let layout = main
            .attr("page-layout-name")
            .and_then(|n| self.st.page_layouts.get(n).copied());
        let mut header_space = None;
        let mut footer_space = None;
        if let Some(layout) = layout {
            if let Some(pp) = layout.child("page-layout-properties") {
                let p = &mut self.doc.props;
                let len = |n: &str| pp.attr(n).and_then(length);
                if let Some(w) = len("page-width").filter(|w| *w > 0) {
                    p.page_width = w;
                }
                if let Some(h) = len("page-height").filter(|h| *h > 0) {
                    p.page_height = h;
                }
                p.margin_top = len("margin-top").unwrap_or(p.margin_top).max(0);
                p.margin_bottom = len("margin-bottom").unwrap_or(p.margin_bottom).max(0);
                p.margin_left = len("margin-left").unwrap_or(p.margin_left).max(0);
                p.margin_right = len("margin-right").unwrap_or(p.margin_right).max(0);
                if let Some(c) = pp.child("columns")
                    && let Some(n) = c.int("column-count").filter(|n| *n > 1)
                {
                    self.doc.section.columns = n.clamp(1, 12) as u16;
                    if let Some(g) = c.attr("column-gap").and_then(length) {
                        self.doc.section.column_spacing = g;
                    }
                }
            }
            let space = |name: &str, gap: &str| {
                layout
                    .child(name)
                    .and_then(|h| h.child("header-footer-properties"))
                    .map(|p| {
                        let h = p
                            .attr("min-height")
                            .or(p.attr("height"))
                            .and_then(length)
                            .unwrap_or(0);
                        h + p.attr(gap).and_then(length).unwrap_or(0)
                    })
            };
            header_space = space("header-style", "margin-bottom");
            footer_space = space("footer-style", "margin-top");
        }
        let shown = |e: Option<&'a Element>| e.filter(|e| e.attr("display") != Some("false"));
        self.in_master = true;
        self.resolved.clear();
        let mut any_header = false;
        let mut any_footer = false;
        let mut stories = vec![
            (StoryId::Header, shown(main.child("header"))),
            (StoryId::Footer, shown(main.child("footer"))),
            (StoryId::EvenHeader, shown(main.child("header-left"))),
            (StoryId::EvenFooter, shown(main.child("footer-left"))),
            (StoryId::FirstHeader, shown(main.child("header-first"))),
            (StoryId::FirstFooter, shown(main.child("footer-first"))),
        ];
        if let Some(t) = title {
            stories.push((StoryId::FirstHeader, shown(t.child("header"))));
            stories.push((StoryId::FirstFooter, shown(t.child("footer"))));
            self.doc.section.different_first_page = true;
        }
        for (story, e) in stories {
            let Some(e) = e else {
                continue;
            };
            match story {
                StoryId::EvenHeader | StoryId::EvenFooter => self.doc.props.even_odd_headers = true,
                StoryId::FirstHeader | StoryId::FirstFooter => {
                    self.doc.section.different_first_page = true
                }
                _ => {}
            }
            if story.is_footer() {
                any_footer = true;
            } else {
                any_header = true;
            }
            let paras = self.read_aside(Some(e));
            if !paras.is_empty() {
                self.doc.set_story(story, Story { paras });
            }
        }
        self.in_master = false;
        self.resolved.clear();
        // ODF measures the margin to the header; Word measures it to the
        // text and puts the header inside it.
        let p = &mut self.doc.props;
        if any_header && let Some(space) = header_space {
            self.doc.section.header_from_top = p.margin_top;
            p.margin_top += space;
        }
        if any_footer && let Some(space) = footer_space {
            self.doc.section.footer_from_bottom = p.margin_bottom;
            p.margin_bottom += space;
        }
        for c in &self.st.notes_config {
            if c.attr("note-class") == Some("endnote") {
                continue;
            }
            if let Some(n) = c.int("start-value") {
                p.footnote_start = (n + 1).clamp(1, 32767) as u16;
            }
            if c.attr("footnotes-position") == Some("document") {
                p.footnote_position = FootnotePosition::EndOfDocument;
            }
            p.footnote_restart_each_page = c.attr("start-numbering-at") == Some("page");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content(body: &str, auto: &str) -> String {
        format!(
            r#"<?xml version="1.0"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="t" xmlns:style="s" xmlns:fo="f" xmlns:table="tb" xmlns:draw="d" xmlns:svg="v" xmlns:dc="dc" xmlns:xlink="x"><office:automatic-styles>{auto}</office:automatic-styles><office:body><office:text>{body}</office:text></office:body></office:document-content>"#
        )
    }

    fn read_content(body: &str, auto: &str) -> Document {
        let c = xml::parse(content(body, auto).as_bytes()).unwrap();
        build(&c, None, None, None, &|_| None).unwrap().0
    }

    #[test]
    fn character_styles_from_other_programs_stay_named_styles() {
        // As LibreOffice writes them: named text styles in styles.xml with no
        // Quill attributes, one based on another, used directly and through
        // an automatic style that adds to it.
        let styles = xml::parse(
            r##"<?xml version="1.0"?><office:document-styles xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="s" xmlns:fo="f" xmlns:text="t"><office:styles><style:style style:name="Emphasis" style:display-name="My Emphasis" style:family="text"><style:text-properties fo:font-style="italic" fo:color="#cc2200" fo:font-weight="normal"/></style:style><style:style style:name="Strong" style:family="text" style:parent-style-name="Emphasis"><style:text-properties fo:font-weight="bold"/></style:style></office:styles></office:document-styles>"##
                .as_bytes(),
        )
        .unwrap();
        let c = xml::parse(
            content(
                r#"<text:p><text:span text:style-name="Emphasis">one</text:span> <text:span text:style-name="T1">two</text:span> <text:span text:style-name="Strong">three</text:span></text:p>"#,
                r#"<style:style style:name="T1" style:family="text" style:parent-style-name="Emphasis"><style:text-properties style:text-underline-style="solid"/></style:style>"#,
            )
            .as_bytes(),
        )
        .unwrap();
        let doc = build(&c, Some(&styles), None, None, &|_| None).unwrap().0;
        let emphasis = doc
            .styles
            .find_character("My Emphasis")
            .expect("named by its display name");
        let strong = doc.styles.find_character("Strong").unwrap();
        let e = doc.styles.character(emphasis).unwrap().format;
        // Italic and color; "normal" weight switches nothing, so it is not kept.
        assert_eq!(e.mask, (1 << 3) | (1 << 11));
        assert_eq!(e.props.color, Rgb(0xCC2200));
        let s = doc.styles.character(strong).unwrap().format;
        assert_eq!(
            s.mask,
            (1 << 2) | (1 << 3) | (1 << 11),
            "it takes its parent's too"
        );
        let p = &doc.main.paras[0];
        let one = p.chp_of_char(0);
        assert_eq!((one.character_style, one.direct_format), (emphasis, 0));
        assert!(one.italic);
        let two = p.chp_of_char(4);
        assert_eq!(two.character_style, emphasis);
        assert_eq!(two.direct_format, 1 << 4, "the underline is its own");
        let three = p.chp_of_char(8);
        assert_eq!(three.character_style, strong);
        assert!(three.bold && three.italic);
    }

    #[test]
    fn plain_paragraphs_stop_at_the_size_limit() {
        // Nine bytes each in the file, and they compress to almost nothing.
        let body = "<text:p/>".repeat(quill_core::limits::MAX_PARAGRAPHS * 2);
        let c = xml::parse(content(&body, "").as_bytes()).unwrap();
        let err = build(&c, None, None, None, &|_| None).err().unwrap();
        // The count in the message shows where building stopped.
        let stopped = format!("{}", quill_core::limits::MAX_PARAGRAPHS + 1);
        assert!(err.contains("too large") && err.contains(&stopped), "{err}");
    }

    #[test]
    fn repeated_spaces_stop_at_the_size_limit() {
        // Each element is a thousand spaces: far more than Quill holds.
        let spaces = r#"<text:s text:c="1000"/>"#.repeat(200_000);
        let c = xml::parse(content(&format!("<text:p>{spaces}</text:p>"), "").as_bytes()).unwrap();
        let err = build(&c, None, None, None, &|_| None).err().unwrap();
        assert!(err.contains("too large"), "{err}");
        // It stopped just past the limit, not at all two hundred million.
        assert!(err.contains("33555000 characters"), "{err}");
    }

    #[test]
    fn repeated_rows_and_cells_stop_at_the_size_limit() {
        // Each table is tiny in the file but repeats into thousands of
        // paragraphs; many of them must not all be built.
        let table = r#"<table:table><table:table-column/><table:table-row table:number-rows-repeated="1000"><table:table-cell table:number-columns-repeated="255"><text:p/></table:table-cell></table:table-row></table:table>"#;
        let c = xml::parse(content(&table.repeat(40), "").as_bytes()).unwrap();
        let err = build(&c, None, None, None, &|_| None).err().unwrap();
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn border_colors_take_exactly_six_digits() {
        assert_eq!(
            border(Some("0.5pt solid #ff000000"), None).color,
            Rgb::BLACK
        );
        assert_eq!(
            border(Some("0.5pt solid #c0ffee"), None).color,
            Rgb(0xC0FFEE)
        );
    }

    #[test]
    fn lengths_in_every_unit() {
        assert_eq!(length("1in"), Some(1440));
        assert_eq!(length("2.54cm"), Some(1440));
        assert_eq!(length("25.4mm"), Some(1440));
        assert_eq!(length("12pt"), Some(240));
        assert_eq!(length("1pc"), Some(240));
        assert_eq!(length("96px"), Some(1440));
        assert_eq!(length("-0.25in"), Some(-360));
        assert_eq!(length("50%"), None);
        assert_eq!(length("wide"), None);
    }

    #[test]
    fn white_space_collapses_and_text_s_keeps_spaces() {
        let doc = read_content(
            "<text:p>  Hello \n  world<text:s text:c=\"3\"/>again<text:tab/>x<text:line-break/>y</text:p>",
            "",
        );
        assert_eq!(doc.main.paras[0].text, "Hello world   again\tx\u{000B}y");
    }

    #[test]
    fn space_after_any_element_starts_a_new_run_as_in_libreoffice() {
        // From LibreOffice's whitespace.odt: an empty bookmark or a frame
        // between two spaces keeps both.
        let doc = read_content(
            "<text:p>X <text:bookmark text:name=\"b\"/> X</text:p><text:p>X <draw:frame><draw:text-box><text:p/></draw:text-box></draw:frame> X</text:p>",
            "",
        );
        assert_eq!(doc.main.paras[0].text, "X  X");
        assert_eq!(doc.main.paras[1].text, "X  X");
    }

    #[test]
    fn spans_take_their_automatic_text_styles() {
        let doc = read_content(
            r#"<text:p>a<text:span text:style-name="T1">b</text:span><text:span text:style-name="T2">c</text:span></text:p>"#,
            r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold" fo:font-style="italic"/></style:style><style:style style:name="T2" style:family="text"><style:text-properties style:text-position="super 58%" fo:font-size="20pt"/></style:style>"#,
        );
        let p = &doc.main.paras[0];
        assert!(!p.chp_of_char(0).bold);
        assert!(p.chp_of_char(1).bold && p.chp_of_char(1).italic);
        let sup = p.chp_of_char(2);
        // The text's own size; the superscript is drawn smaller.
        assert_eq!(sup.size, 40);
        assert_eq!(sup.script, quill_core::Script::Super);
        assert_eq!(sup.position, 0);
    }

    #[test]
    fn lists_become_literal_numbers_and_bullets() {
        let auto = r#"<text:list-style style:name="L1"><text:list-level-style-number text:level="1" style:num-format="1" style:num-suffix="."><style:list-level-properties text:list-level-position-and-space-mode="label-alignment"><style:list-level-label-alignment text:label-followed-by="listtab" fo:margin-left="0.5in" fo:text-indent="-0.25in"/></style:list-level-properties></text:list-level-style-number><text:list-level-style-bullet text:level="2" text:bullet-char="•"/></text:list-style>"#;
        let doc = read_content(
            r#"<text:list text:style-name="L1"><text:list-item><text:p>one</text:p></text:list-item><text:list-item><text:p>two</text:p><text:list><text:list-item><text:p>inner</text:p></text:list-item></text:list></text:list-item><text:list-item><text:p>three</text:p></text:list-item></text:list>"#,
            auto,
        );
        let texts: Vec<&str> = doc.main.paras.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(texts, ["1.\tone", "2.\ttwo", "•\tinner", "3.\tthree"]);
        assert_eq!(doc.main.paras[0].pap.left, 720);
        assert_eq!(doc.main.paras[0].pap.first, -360);
    }

    #[test]
    fn tables_keep_cells_spans_and_widths() {
        let auto = r#"<style:style style:name="C1" style:family="table-column"><style:table-column-properties style:column-width="1in"/></style:style>"#;
        let doc = read_content(
            r#"<table:table><table:table-column table:style-name="C1" table:number-columns-repeated="2"/><table:table-row><table:table-cell table:number-columns-spanned="2"><text:p>wide</text:p></table:table-cell><table:covered-table-cell/></table:table-row><table:table-row><table:table-cell><text:p>a</text:p></table:table-cell><table:table-cell><text:p>b</text:p></table:table-cell></table:table-row></table:table>"#,
            auto,
        );
        let rows: Vec<_> = doc
            .main
            .paras
            .iter()
            .filter_map(|p| p.row.as_ref())
            .collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].cells.len(), 1);
        assert_eq!(rows[0].cells[0].width, 2880);
        assert_eq!(rows[1].cells.len(), 2);
        assert_eq!(doc.main.paras[0].text, "wide");
        assert!(!doc.main.paras.last().unwrap().pap.in_table);
    }

    #[test]
    fn a_comment_in_a_footnote_goes_on_the_footnote_mark() {
        let c = content(
            r#"<text:p>See<text:note text:note-class="footnote"><text:note-body><text:p>Note<office:annotation><dc:creator>Ada</dc:creator><text:p>Odd.</text:p></office:annotation></text:p></text:note-body></text:note> on.</text:p>"#,
            "",
        );
        let c = xml::parse(c.as_bytes()).unwrap();
        let (doc, report) = build(&c, None, None, None, &|_| None).unwrap();
        assert_eq!(doc.main.paras[0].text, "See\u{E003}\u{E004} on.");
        assert_eq!(doc.notes.paras[0].text, "\u{E003}Note");
        assert_eq!(doc.annotations.paras[0].text, "\u{E004}Odd.");
        assert_eq!(report.notes(), [notes::NOTE_COMMENTS]);
    }

    #[test]
    fn floating_text_boxes_become_frames_before_their_paragraph() {
        let doc = read_content(
            r#"<text:p>Body<draw:frame draw:style-name="fr1" text:anchor-type="paragraph" svg:width="2in"><draw:text-box><text:p>Aside</text:p></draw:text-box></draw:frame></text:p><draw:frame draw:style-name="fr2" text:anchor-type="page" svg:x="1in" svg:y="2in" svg:width="1in"><draw:text-box><text:p>Placed</text:p></draw:text-box></draw:frame><text:p>Next</text:p>"#,
            r#"<style:style style:name="fr1" style:family="graphic"><style:graphic-properties style:horizontal-pos="right" style:horizontal-rel="paragraph" fo:margin-left="0.1in"/></style:style><style:style style:name="fr2" style:family="graphic"><style:graphic-properties style:horizontal-pos="from-left" style:horizontal-rel="page" style:vertical-pos="from-top" style:vertical-rel="page"/></style:style>"#,
        );
        let texts: Vec<&str> = doc.main.paras.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(texts, ["Aside", "Body", "Placed", "Next"]);
        let aside = doc.main.paras[0].pap.frame.unwrap();
        assert_eq!(
            (aside.h, aside.h_rel, aside.v, aside.width, aside.from_text),
            (
                FrameH::Right,
                FrameRel::Column,
                FrameV::InLine,
                Some(2880),
                144
            )
        );
        let placed = doc.main.paras[2].pap.frame.unwrap();
        assert_eq!(
            (placed.h, placed.h_rel, placed.v, placed.v_rel),
            (
                FrameH::At(1440),
                FrameRel::Page,
                FrameV::At(2880),
                FrameRel::Page
            )
        );
        assert!(doc.main.paras[1].pap.frame.is_none());
    }

    #[test]
    fn linked_sections_show_their_saved_copy() {
        let c = content(
            r#"<text:section text:name="part.odt"><text:section-source xlink:href="../part.odt"/><text:p>Saved copy</text:p></text:section>"#,
            "",
        );
        let c = xml::parse(c.as_bytes()).unwrap();
        let (doc, report) = build(&c, None, None, None, &|_| None).unwrap();
        assert_eq!(doc.main.paras[0].text, "Saved copy");
        assert_eq!(report.notes(), [notes::LINKED_SECTIONS]);
    }

    #[test]
    fn notes_and_annotations_get_their_own_stories() {
        let doc = read_content(
            r#"<text:p>See<text:note text:note-class="footnote"><text:note-citation>1</text:note-citation><text:note-body><text:p>The note.</text:p></text:note-body></text:note> here<office:annotation><dc:creator>Ada Lovelace</dc:creator><dc:date>2026-10-21T10:00:00</dc:date><text:p>Check this.</text:p></office:annotation>.</text:p>"#,
            "",
        );
        let p = &doc.main.paras[0];
        assert!(p.text.contains(chars::FOOTNOTE));
        assert!(p.text.contains(chars::ANNOTATION));
        assert!(doc.notes.paras[0].text.ends_with("The note."));
        assert!(doc.annotations.paras[0].text.ends_with("Check this."));
        let info = doc.annotation_info.values().next().unwrap();
        assert_eq!(
            (info.initials.as_str(), info.date.as_str()),
            ("AL", "2026-10-21")
        );
    }

    #[test]
    fn frame_text_follows_its_paragraph_and_fields_become_quills() {
        let doc = read_content(
            r#"<text:p>Page <text:page-number>3</text:page-number><draw:frame><draw:text-box><text:p>boxed</text:p></draw:text-box></draw:frame></text:p><text:p>after</text:p>"#,
            "",
        );
        let texts: Vec<&str> = doc.main.paras.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(texts, ["Page \u{E000}", "boxed", "after"]);
    }

    #[test]
    fn deleted_text_and_index_templates_are_not_read() {
        let doc = read_content(
            r#"<text:tracked-changes><text:changed-region><text:deletion><text:p>gone</text:p></text:deletion></text:changed-region></text:tracked-changes><text:p>kept</text:p>"#,
            "",
        );
        assert_eq!(doc.main.paras.len(), 1);
        assert_eq!(doc.main.paras[0].text, "kept");
    }

    #[test]
    fn style_names_land_on_word_51_styles() {
        assert_eq!(quill_style_name("Standard"), "Normal");
        assert_eq!(quill_style_name("Contents 2"), "toc 2");
        assert_eq!(quill_style_name("Footnote"), "footnote text");
        assert_eq!(quill_style_name("Heading 1"), "Heading 1");
        assert_eq!(decode_name("Heading_20_1"), "Heading 1");
        assert_eq!(decode_name("My_Style"), "My_Style");
    }

    #[test]
    fn base64_decodes_pictures() {
        assert_eq!(base64_decode("aGVs\nbG8=").unwrap(), b"hello");
        assert!(base64_decode("not*base64").is_none());
    }

    #[test]
    fn repeated_picture_files_are_converted_once() {
        let gif = base64_decode("R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==").unwrap();
        let c = xml::parse(
            content(
                r#"<text:p><draw:frame text:anchor-type="as-char"><draw:image xlink:href="Pictures/a.gif"/></draw:frame></text:p><text:p><draw:frame text:anchor-type="as-char"><draw:image xlink:href="Pictures/a.gif"/></draw:frame></text:p>"#,
                "",
            )
            .as_bytes(),
        )
        .unwrap();
        let (doc, _) = build(&c, None, None, None, &|_| Some(gif.clone().into())).unwrap();
        let Object::Picture(picture) = &doc.objects[0] else {
            panic!("pictures were not read")
        };
        assert_eq!(doc.objects.len(), 1);
        assert_eq!(picture.format, quill_core::ImageFormat::Png);
        assert_eq!(doc.main.paras[0].runs[0].chp.object, 1);
        assert_eq!(doc.main.paras[1].runs[0].chp.object, 1);
    }
}
