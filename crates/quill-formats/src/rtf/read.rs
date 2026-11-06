//! Reading Rich Text Format: what the original and its successors write,
//! including `\mac` files in the Mac Roman character set.

use crate::report::{self as notes, ImportReport};
use crate::xml::{Element, Node};
use quill_core::document::FootnotePosition;
use quill_core::para::{Mark, Paragraph, chars};
use quill_core::props::{
    Align, BorderLine, BorderStyle, CellProps, CharProps, LineSpacing, ParaProps, Rgb, RowHeight,
    RowProps, StyleId, TabAlign, TabLeader, TabStop, Underline,
};
use quill_core::{Document, PageNumberFormat, SectionProps, SectionStart};
use std::collections::HashMap;

const MAX_GROUP_DEPTH: usize = 256;
const MAX_CONTROL_WORD: usize = 64;
const MAX_PARAMETER_ABS: i32 = 10_000_000;
// Room for the largest document Quill holds (quill_core::limits); a little
// over it, so such a file gets the size message rather than this one.
const MAX_STORED_TEXT: usize = quill_core::limits::MAX_CHARACTERS * 4 + (16 << 20);
const MAX_PICTURE_BYTES: usize = 128 << 20;
const MAX_PARAGRAPHS: usize = quill_core::limits::MAX_PARAGRAPHS + 10_000;
const MAX_TABLE_ENTRIES: usize = 65_536;
const MAX_PICTURES: usize = 4_096;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dest {
    Text,
    Header(usize),
    Skip,
    FontTable,
    ColorTable,
    StyleSheet,
    Info(InfoField),
    Ignore,
    /// A `\pict` group: hex digits of a picture.
    Pict,
    /// An annotation's writer: initials and name.
    AtnId,
    AtnAuthor,
    AtnDate,
    RowData,
    /// A field's instruction, such as "PAGE".
    FieldInst,
    /// A `\\mmath` equation: OMML spelled as control words.
    Math,
    /// `\\revtbl`: the authors of tracked changes, by number.
    RevTbl,
    /// `\\oldcprops`: a format change's earlier format.
    OldProps,
    /// A comment's tag: where its text starts (`\\atrfstart`) and ends
    /// (`\\atrfend`), the tag of the annotation itself (`\\atnref`), and
    /// the one it replies to (Quill's `\\quillatnparent`).
    AtrfStart,
    AtrfEnd,
    AtnRef,
    AtnParent,
}

/// The tracked change the text being read belongs to, from `\\revised`,
/// `\\deleted`, `\\crauth` and the rest: authors as numbers in the
/// revision table, dates in RTF's packed form.
#[derive(Clone, Copy, Default)]
struct RevWords {
    revised: bool,
    revauth: i32,
    revdttm: i32,
    deleted: bool,
    revauthdel: i32,
    revdttmdel: i32,
    crauth: i32,
    crdate: i32,
    /// The format before a format change, from `\\oldcprops`.
    old: Option<CharProps>,
}

/// A picture being read from a `\pict` group.
#[derive(Default)]
struct Pict {
    bytes: Vec<u8>,
    half: Option<u8>,
    format: Option<quill_core::ImageFormat>,
    goal: (i32, i32),
    scale: (i32, i32),
    /// Inside a group Quill skips, such as `\\nonshppict`: a fallback copy.
    fallback: bool,
    /// A bitmap without its file header (`\\dibitmap`).
    dib: bool,
    /// Left, top, right, bottom in twips.
    crop: [i32; 4],
    /// Quill's exact values accompany RTF's rounded interoperability values.
    exact_crop: [Option<i32>; 4],
    exact_size: [Option<i32>; 2],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum InfoField {
    None,
    Title,
    Subject,
    Author,
    Keywords,
    Version,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Charset {
    Ansi,
    Mac,
}

/// Group state saved and restored with braces.
#[derive(Clone)]
struct State {
    dest: Dest,
    chp: CharProps,
    /// The RTF number of the font in use, for its character set.
    font: i32,
    /// Characters of fallback text to skip after `\u`.
    uc: usize,
    /// A tab stop being built: leader and alignment come before `\tx`.
    tab_align: TabAlign,
    tab_leader: TabLeader,
    /// Which border `\brdr...` words apply to.
    border: Option<u8>,
    /// The tracked change the text is part of.
    rev: RevWords,
    /// A list paragraph's number or bullet (`\listtext`).
    list_text: bool,
}

struct Reader {
    doc: Document,
    /// The settings of the section being read.
    sec: SectionProps,
    stack: Vec<State>,
    st: State,
    pap: ParaProps,
    para: Paragraph,
    paras: Vec<Paragraph>,
    charset: Charset,
    fonts: HashMap<i32, u16>,
    font_name: String,
    font_num: i32,
    /// Each font's code page, from its \\fcharset.
    font_pages: HashMap<i32, u16>,
    /// The document's code page, from \\ansicpg (Windows Latin unless
    /// it says otherwise).
    codepage: u16,
    colors: Vec<Rgb>,
    color: (u8, u8, u8),
    /// Style being defined in the style sheet.
    style_num: i32,
    style_based: Option<i32>,
    style_next: Option<i32>,
    style_name: String,
    style_pap: ParaProps,
    /// RTF style number -> our style id.
    styles: HashMap<i32, StyleId>,
    pending_style: Vec<(StyleId, Option<i32>, Option<i32>)>,
    skip: usize,
    default_font: i32,
    pict: Pict,
    /// A `\\footnote` group being read, and the text it interrupted.
    note: Option<NoteRead>,
    /// A `\\chftn` in the text, waiting for its `\\footnote` group.
    pending_ref: Option<u32>,
    /// The notes read so far, by id.
    notes: HashMap<u32, Vec<Paragraph>>,
    next_note: u32,
    /// Annotations: the writer named before the next `\chatn`, the mark
    /// waiting for its `\annotation` group, and those read so far.
    atn_initials: String,
    atn_author: String,
    atn_date: String,
    /// The revision table's authors, and the one being read.
    rev_authors: Vec<String>,
    rev_author: String,
    /// A `\\oldcprops` group's format, for the group around it.
    old_props: Option<CharProps>,
    /// The comment tag being read, and what the tags mean: provisional
    /// numbers for ranges (see `crate::comments`), the comments whose text
    /// is being read (last is innermost), annotations by tag, and replies'
    /// parents' tags.
    tag: String,
    range_tags: HashMap<String, u32>,
    open_comments: Vec<u32>,
    atn_tags: HashMap<String, u32>,
    atn_parent: Option<String>,
    atn_done: bool,
    parents: Vec<(u32, String)>,
    /// Tracked changes' authors and dates seen, to bound the revision
    /// table (see `stamp`).
    stamps: std::collections::HashSet<quill_core::revision::Stamp>,
    pending_atn: Option<u32>,
    annotations: HashMap<u32, Vec<Paragraph>>,
    row_data: String,
    pending_row: Option<RowProps>,
    /// A header/footer destination being read, and the body text it
    /// interrupted.
    header: Option<HeaderRead>,
    /// Work and allocations derived from the input are bounded even when
    /// `read` is called directly rather than through the file-size gate.
    stored_text: usize,
    paragraphs: usize,
    pictures: usize,
    error: Option<&'static str>,
    report: ImportReport,
    /// The instruction of the field being read.
    field_instr: String,
    /// The field being read is locked (\\fldlock): Word doesn't update it.
    field_locked: bool,
    /// The first half of a character beyond U+FFFF, which RTF writes as
    /// two \\u codes, waiting for its second half.
    high_surrogate: Option<u32>,
    /// The equation being read, as OMML elements: each with the group
    /// depth that opened it. The first is the `\\mmath` group.
    math: Vec<(usize, Element)>,
    /// A group just opened in an equation; its first control word names
    /// the element it is.
    math_group: bool,
    /// The first section's page size, to notice sections with another.
    page_size: (Option<i32>, Option<i32>),
}

/// What a `\\footnote` group put aside while its note is read.
struct NoteRead {
    id: u32,
    depth: usize,
    /// No `\\chftn` came before it: the note has a mark of its own.
    custom: bool,
    /// An annotation rather than a footnote.
    annotation: bool,
    para: Paragraph,
    paras: Vec<Paragraph>,
    pap: ParaProps,
}

struct HeaderRead {
    index: usize,
    depth: usize,
    para: Paragraph,
    paras: Vec<Paragraph>,
    pap: ParaProps,
}

pub fn read(data: &[u8]) -> Result<Document, String> {
    read_into(data, &mut ImportReport::default())
}

fn read_into(data: &[u8], report: &mut ImportReport) -> Result<Document, String> {
    if !data.starts_with(b"{\\rtf") {
        return Err("not an RTF file".into());
    }
    let doc = Document::new();
    let normal = doc.styles.get(0).clone();
    let mut r = Reader {
        sec: SectionProps::default(),
        note: None,
        pending_ref: None,
        notes: HashMap::new(),
        next_note: 0,
        atn_initials: String::new(),
        atn_author: String::new(),
        atn_date: String::new(),
        rev_authors: Vec::new(),
        rev_author: String::new(),
        old_props: None,
        tag: String::new(),
        range_tags: HashMap::new(),
        open_comments: Vec::new(),
        atn_tags: HashMap::new(),
        atn_parent: None,
        atn_done: false,
        parents: Vec::new(),
        stamps: Default::default(),
        pending_atn: None,
        annotations: HashMap::new(),
        row_data: String::new(),
        pending_row: None,
        header: None,
        stored_text: 0,
        paragraphs: 0,
        pictures: 0,
        error: None,
        report: ImportReport::default(),
        field_instr: String::new(),
        field_locked: false,
        high_surrogate: None,
        math: Vec::new(),
        math_group: false,
        page_size: (None, None),
        st: State {
            dest: Dest::Text,
            font: 0,
            chp: CharProps {
                font: 0,
                size: 24,
                ..Default::default()
            },
            uc: 1,
            tab_align: TabAlign::Left,
            tab_leader: TabLeader::None,
            border: None,
            rev: RevWords::default(),
            list_text: false,
        },
        stack: Vec::new(),
        pap: ParaProps::default(),
        para: Paragraph::new(normal.pap.clone(), normal.chp),
        paras: Vec::new(),
        charset: Charset::Ansi,
        fonts: HashMap::new(),
        font_name: String::new(),
        font_num: 0,
        font_pages: HashMap::new(),
        codepage: 1252,
        colors: Vec::new(),
        color: (0, 0, 0),
        style_num: 0,
        style_based: None,
        style_next: None,
        style_name: String::new(),
        style_pap: ParaProps::default(),
        styles: HashMap::new(),
        pending_style: Vec::new(),
        skip: 0,
        default_font: 0,
        pict: Pict::default(),
        doc,
    };
    if data.len() as u64 > crate::MAX_FILE_BYTES {
        return Err("The RTF file is too large.".into());
    }
    r.run(data);
    *report = std::mem::take(&mut r.report);
    r.finish()
}

/// Like `read`, with what could not be kept exactly.
pub fn read_reported(data: &[u8]) -> Result<(Document, ImportReport), String> {
    let mut report = ImportReport::default();
    let doc = read_into(data, &mut report)?;
    Ok((doc, report))
}

/// A control word's numeric parameter. Word writes revision, list and
/// template ids as full 32-bit numbers, so ids keep theirs; everything else
/// is a measurement or count, kept within a range that cannot overflow.
fn parameter(word: &str, digits: &[u8]) -> Option<i32> {
    let text = std::str::from_utf8(digits).ok()?;
    let (negative, number) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if number.is_empty() {
        return None;
    }
    // Saturate rather than fail on a number too long for any integer.
    let magnitude = number
        .bytes()
        .try_fold(0i64, |n, b| {
            n.checked_mul(10)?.checked_add((b - b'0') as i64)
        })
        .unwrap_or(i64::MAX);
    let value = if negative { -magnitude } else { magnitude };
    // Tracked changes' packed dates use all 32 bits; writers differ on
    // whether they are signed.
    if matches!(word, "revdttm" | "revdttmdel" | "crdate") {
        return Some(if (0..=u32::MAX as i64).contains(&value) {
            value as u32 as i32
        } else {
            value.clamp(i32::MIN as i64, i32::MAX as i64) as i32
        });
    }
    let id = word.ends_with("id")
        || word.contains("rsid")
        || matches!(
            word,
            "vern" | "version" | "edmins" | "nofpages" | "nofwords" | "nofchars" | "nofcharsws"
        );
    let limit = if id {
        i32::MAX as i64
    } else {
        MAX_PARAMETER_ABS as i64
    };
    Some(value.clamp(-limit, limit) as i32)
}

impl Reader {
    fn run(&mut self, d: &[u8]) {
        let mut i = 0;
        // Whether the next group is a `{\*...}` destination we may skip.
        let mut starred = false;
        let mut group_start = false;
        let mut root_closed = false;
        while i < d.len() && self.error.is_none() {
            let c = d[i];
            // Word ignores whatever follows the outer group (often padding
            // or a stray second document).
            if root_closed {
                break;
            }
            match c {
                b'{' => {
                    if self.stack.len() >= MAX_GROUP_DEPTH {
                        self.error = Some("The RTF file is nested too deeply.");
                        break;
                    }
                    self.stack.push(self.st.clone());
                    self.math_group = self.st.dest == Dest::Math;
                    group_start = true;
                    starred = false;
                    if self.st.dest == Dest::StyleSheet {
                        self.begin_style();
                    }
                    i += 1;
                }
                b'}' => {
                    if self.stack.is_empty() {
                        self.error = Some("The RTF file has an unmatched closing brace.");
                        break;
                    }
                    self.close_group();
                    i += 1;
                    root_closed = self.stack.is_empty();
                }
                b'\\' => {
                    i += 1;
                    // A file cut off after a backslash keeps what came before.
                    let Some(&n) = d.get(i) else {
                        break;
                    };
                    if n.is_ascii_alphabetic() {
                        let start = i;
                        while i < d.len() && d[i].is_ascii_alphabetic() {
                            i += 1;
                        }
                        if i - start > MAX_CONTROL_WORD {
                            self.error = Some("The RTF file has an overlong control word.");
                            break;
                        }
                        let word = std::str::from_utf8(&d[start..i]).unwrap_or("");
                        let mut param: Option<i32> = None;
                        let ns = i;
                        if i < d.len() && (d[i] == b'-' || d[i].is_ascii_digit()) {
                            i += 1;
                            while i < d.len() && d[i].is_ascii_digit() {
                                i += 1;
                            }
                            param = parameter(word, &d[ns..i]);
                            if word == "bin"
                                && param.is_none_or(|v| v.unsigned_abs() > MAX_PICTURE_BYTES as u32)
                            {
                                self.error = Some("The RTF file has an invalid numeric parameter.");
                                break;
                            }
                        }
                        if i < d.len() && d[i] == b' ' {
                            i += 1;
                        }
                        if starred
                            && group_start
                            && !known_destination(word)
                            && self.st.dest != Dest::Math
                        {
                            self.st.dest = Dest::Skip;
                        }
                        group_start = false;
                        if word == "bin" {
                            let Some(n) = param.and_then(|n| usize::try_from(n).ok()) else {
                                self.error = Some("The RTF file has an invalid binary length.");
                                break;
                            };
                            let Some(end) = i.checked_add(n).filter(|&end| end <= d.len()) else {
                                self.error = Some("The RTF file has truncated binary data.");
                                break;
                            };
                            if self.st.dest == Dest::Pict {
                                if self.pict.bytes.len().saturating_add(n) > MAX_PICTURE_BYTES {
                                    self.error = Some("An RTF picture is too large.");
                                    break;
                                }
                                self.pict.bytes.extend_from_slice(&d[i..end]);
                            }
                            i = end;
                            continue;
                        }
                        self.word(word, param);
                    } else {
                        i += 1;
                        match n {
                            b'*' => {
                                starred = true;
                                continue;
                            }
                            b'\'' => {
                                let Some(hex) = d
                                    .get(i..i.saturating_add(2))
                                    .and_then(|h| std::str::from_utf8(h).ok())
                                else {
                                    self.error = Some("The RTF file has a truncated hex escape.");
                                    break;
                                };
                                i += 2;
                                let Some(b) = u8::from_str_radix(hex, 16).ok() else {
                                    self.error = Some("The RTF file has an invalid hex escape.");
                                    break;
                                };
                                let ch = self.decode(b);
                                self.ch(ch);
                            }
                            b'~' => self.ch(chars::NONBREAKING_SPACE),
                            b'-' => self.ch(chars::OPTIONAL_HYPHEN),
                            b'_' => self.ch(chars::NONBREAKING_HYPHEN),
                            b'\n' | b'\r' => self.word("par", None),
                            b'\\' | b'{' | b'}' => self.ch(n as char),
                            b'\t' => self.ch(chars::TAB),
                            _ => {}
                        }
                        group_start = false;
                    }
                }
                b'\r' | b'\n' => i += 1,
                _ => {
                    // Plain text up to the next special byte.
                    let start = i;
                    while i < d.len() && !matches!(d[i], b'{' | b'}' | b'\\' | b'\r' | b'\n') {
                        i += 1;
                    }
                    group_start = false;
                    let bytes = &d[start..i];
                    for &b in bytes {
                        let ch = if b < 0x80 { b as char } else { self.decode(b) };
                        self.ch(ch);
                    }
                }
            }
        }
        // A file that ends before its groups close keeps what it has, as
        // in Word.
        while self.error.is_none() && !self.stack.is_empty() {
            self.close_group();
        }
    }

    /// End the innermost group, finishing a note or header it closes.
    fn close_group(&mut self) {
        self.end_group();
        self.end_math_group();
        self.st = self.stack.pop().expect("the stack was checked");
        self.math_group = false;
        // A format change's old format belongs to the group around it.
        if let Some(old) = self.old_props.take() {
            self.st.rev.old = Some(old);
            self.update_rev();
        }
        if self
            .note
            .as_ref()
            .is_some_and(|n| self.stack.len() < n.depth)
        {
            self.finish_note();
        }
        if self
            .header
            .as_ref()
            .is_some_and(|h| self.stack.len() < h.depth)
        {
            self.finish_header();
        }
    }

    /// Close the equation element the ending group opened, and write the
    /// equation when it is the `\\mmath` group.
    fn end_math_group(&mut self) {
        let depth = self.stack.len();
        if self.math.last().is_none_or(|(d, _)| *d != depth) {
            return;
        }
        let Some((_, mut e)) = self.math.pop() else {
            return;
        };
        let has_elements = e.elements().next().is_some();
        let text: String = e
            .children
            .iter()
            .filter_map(|n| match n {
                Node::Text(t) => Some(t.as_str()),
                Node::Element(_) => None,
            })
            .collect();
        if e.name == "r" {
            // A run's text, as m:t holds it.
            e.children.retain(|n| matches!(n, Node::Element(_)));
            e.children.push(Node::Element(Element {
                name: "t".into(),
                attrs: Vec::new(),
                children: vec![Node::Text(text)],
            }));
        } else if !has_elements && !text.is_empty() {
            // A property such as {\\mbegChr [}: its value.
            e.attrs.push(("val".into(), text.trim().to_string()));
            e.children.clear();
        }
        match self.math.last_mut() {
            Some((_, parent)) => parent.children.push(Node::Element(e)),
            None => {
                self.report.note(notes::EQUATIONS);
                let chp = self.stack.last().map_or(self.st.chp, |s| s.chp);
                crate::math::omml(&e, &mut self.para, chp);
            }
        }
    }

    /// A control word inside an equation: at the start of a group it names
    /// an OMML element (\\msSup is m:sSup); \\msty0 is upright text.
    fn math_word(&mut self, w: &str, v: i32) {
        let opens = std::mem::take(&mut self.math_group);
        let Some(name) = w.strip_prefix('m').filter(|n| !n.is_empty()) else {
            return;
        };
        if opens {
            if self.math.len() >= 256 {
                self.error = Some("The RTF equation is nested too deeply.");
                return;
            }
            let e = Element {
                name: name.to_string(),
                ..Element::default()
            };
            self.math.push((self.stack.len(), e));
        } else if name == "sty"
            && v == 0
            && let Some((_, run)) = self.math.last_mut()
        {
            run.children.push(Node::Element(Element {
                name: "rPr".into(),
                attrs: Vec::new(),
                children: vec![Node::Element(Element {
                    name: "sty".into(),
                    attrs: vec![("val".into(), "p".into())],
                    children: Vec::new(),
                })],
            }));
        }
    }

    fn decode(&self, b: u8) -> char {
        if b < 0x80 {
            return b as char;
        }
        match self.charset {
            _ if let Some(c) = self
                .font_pages
                .get(&self.st.font)
                .or(Some(&self.codepage).filter(|&&cp| cp != 1252))
                .and_then(|&cp| super::codepages::decode(cp, b)) =>
            {
                c
            }
            Charset::Mac => MAC_ROMAN[(b - 0x80) as usize],
            Charset::Ansi => match b {
                0x80..=0x9F => CP1252[(b - 0x80) as usize],
                _ => b as char,
            },
        }
    }

    fn ch(&mut self, c: char) {
        if self.error.is_some() {
            return;
        }
        if self.skip > 0 {
            self.skip -= 1;
            return;
        }
        // Symbol and Wingdings bullets: a middle dot or a private-use code.
        let c = if self.st.list_text && (c == '\u{B7}' || ('\u{E000}'..='\u{F8FF}').contains(&c)) {
            '•'
        } else {
            c
        };
        if matches!(
            self.st.dest,
            Dest::Text
                | Dest::Header(_)
                | Dest::FontTable
                | Dest::StyleSheet
                | Dest::Info(_)
                | Dest::AtnId
                | Dest::AtnAuthor
                | Dest::AtnDate
                | Dest::RowData
                | Dest::Math
                | Dest::RevTbl
        ) {
            self.stored_text = self.stored_text.saturating_add(c.len_utf8());
            if self.stored_text > MAX_STORED_TEXT {
                self.error = Some("The RTF file expands to too much text.");
                return;
            }
        }
        match self.st.dest {
            Dest::Text | Dest::Header(_) => {
                let at = self.para.len();
                let mut buf = [0u8; 4];
                let chp = self.text_chp();
                self.para.insert(at, c.encode_utf8(&mut buf), chp);
            }
            Dest::FontTable => {
                // Older files list several fonts in one group, each ended
                // by a semicolon.
                if c == ';' {
                    self.register_font();
                } else {
                    self.font_name.push(c);
                }
            }
            Dest::StyleSheet => {
                if c != ';' {
                    self.style_name.push(c);
                }
            }
            Dest::ColorTable => {
                if c == ';' {
                    if self.colors.len() >= MAX_TABLE_ENTRIES {
                        self.error = Some("The RTF color table has too many entries.");
                        return;
                    }
                    let (r, g, b) = self.color;
                    self.colors
                        .push(Rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32));
                    self.color = (0, 0, 0);
                }
            }
            Dest::FieldInst => {
                if self.field_instr.len() < 256 {
                    self.field_instr.push(c);
                }
            }
            Dest::Math => {
                self.math_group = false;
                if let Some((_, e)) = self.math.last_mut() {
                    match e.children.last_mut() {
                        Some(Node::Text(t)) => t.push(c),
                        _ => e.children.push(Node::Text(c.to_string())),
                    }
                }
            }
            Dest::AtnId => self.atn_initials.push(c),
            Dest::AtnAuthor => self.atn_author.push(c),
            Dest::AtnDate => self.atn_date.push(c),
            Dest::AtrfStart | Dest::AtrfEnd | Dest::AtnRef | Dest::AtnParent => {
                if self.tag.len() < 64 {
                    self.tag.push(c);
                }
            }
            Dest::RevTbl if c == ';' => {
                if self.rev_authors.len() >= MAX_TABLE_ENTRIES {
                    self.error = Some("The RTF revision table has too many entries.");
                    return;
                }
                // Names are kept exactly, spaces and all, as Word keeps them.
                let name = std::mem::take(&mut self.rev_author);
                self.rev_authors.push(name);
            }
            Dest::RevTbl => self.rev_author.push(c),
            Dest::RowData => self.row_data.push(c),
            Dest::Info(f) => {
                let s = &mut self.doc.summary;
                let target = match f {
                    InfoField::Title => &mut s.title,
                    InfoField::Subject => &mut s.subject,
                    InfoField::Author => &mut s.author,
                    InfoField::Keywords => &mut s.keywords,
                    InfoField::Version => &mut s.version,
                    InfoField::None => return,
                };
                target.push(c);
            }
            Dest::Pict => {
                if let Some(d) = c.to_digit(16) {
                    match self.pict.half.take() {
                        Some(h) => {
                            if self.pict.bytes.len() >= MAX_PICTURE_BYTES {
                                self.error = Some("An RTF picture is too large.");
                                return;
                            }
                            self.pict.bytes.push(h << 4 | d as u8);
                        }
                        None => self.pict.half = Some(d as u8),
                    }
                } else if !c.is_ascii_whitespace() {
                    self.error = Some("An RTF picture contains invalid hexadecimal data.");
                }
            }
            Dest::Skip | Dest::Ignore | Dest::OldProps => {}
        }
    }

    /// The end of a `\pict` group: put the picture in the text.
    fn finish_pict(&mut self) {
        let p = std::mem::take(&mut self.pict);
        if p.half.is_some() {
            self.error = Some("An RTF picture has an incomplete hexadecimal byte.");
            return;
        }
        let had_bytes = !p.bytes.is_empty();
        let picture = if p.dib {
            crate::images::from_dib(&p.bytes)
        } else if p.format.is_some() {
            crate::images::picture(p.bytes)
        } else {
            None
        };
        let Some(mut pic) = picture else {
            if !p.fallback && had_bytes {
                self.report.note(notes::PICTURES);
            }
            return;
        };
        let (gw, gh) = if p.goal.0 > 0 && p.goal.1 > 0 {
            p.goal
        } else {
            pic.natural
        };
        pic.natural = (gw, gh);
        let whole = quill_core::picture::CROP_WHOLE as i64;
        let frac = |v: i32, g: i32| (v as i64 * whole / g.max(1) as i64) as i32;
        pic.crop = quill_core::Crop {
            left: p.exact_crop[0].unwrap_or_else(|| frac(p.crop[0], gw)),
            top: p.exact_crop[1].unwrap_or_else(|| frac(p.crop[1], gh)),
            right: p.exact_crop[2].unwrap_or_else(|| frac(p.crop[2], gw)),
            bottom: p.exact_crop[3].unwrap_or_else(|| frac(p.crop[3], gh)),
        };
        let (sx, sy) = (
            if p.scale.0 > 0 { p.scale.0 } else { 100 },
            if p.scale.1 > 0 { p.scale.1 } else { 100 },
        );
        let (w, h) = pic.shown_natural();
        pic.size = (
            (w * sx as f32 / 100.0).round() as i32,
            (h * sy as f32 / 100.0).round() as i32,
        );
        if let [Some(w), Some(h)] = p.exact_size {
            pic.size = (w, h);
        }
        let id = self.doc.add_object(quill_core::Object::Picture(pic));
        self.pictures += 1;
        if self.pictures > MAX_PICTURES {
            self.error = Some("The RTF file contains too many pictures.");
            return;
        }
        let chp = CharProps {
            object: id,
            ..self.text_chp()
        };
        let at = self.para.len();
        self.para.insert(at, &chars::OBJECT.to_string(), chp);
    }

    /// The end of a `\\footnote` group: keep its paragraphs as the note
    /// and go back to the text.
    fn finish_note(&mut self) {
        let Some(n) = self.note.take() else {
            return;
        };
        if !self.para.is_empty() || self.paras.is_empty() {
            self.end_paragraph(Mark::Paragraph);
        }
        let mut paras = std::mem::replace(&mut self.paras, n.paras);
        self.para = n.para;
        self.pap = n.pap;
        if n.annotation {
            self.annotations.insert(n.id, paras);
            return;
        }
        if n.custom {
            // The mark is the note's first word, and the text ends with it.
            // A mark written straight before the note's text, as in ".See
            // page 4", is the longest start of the note that ends the text.
            let first = &mut paras[0];
            let word = first.text.split(' ').next().unwrap_or("");
            let mark: String = if self.para.text.ends_with(word) {
                word.to_string()
            } else {
                first
                    .text
                    .char_indices()
                    .map(|(i, c)| &first.text[..i + c.len_utf8()])
                    .take(10)
                    .filter(|m| self.para.text.ends_with(m))
                    .last()
                    .unwrap_or("")
                    .to_string()
            };
            if !mark.is_empty() && self.para.text.ends_with(&mark) {
                let end = self.para.len();
                let chp = self.para.chp_of_char(end - mark.len());
                self.para.remove(end - mark.len(), end);
                let at = self.para.len();
                self.para.insert(
                    at,
                    &chars::FOOTNOTE.to_string(),
                    CharProps {
                        object: n.id,
                        ..chp
                    },
                );
                let fchp = first.chp_of_char(0);
                first.remove(0, mark.len());
                first.insert(
                    0,
                    &chars::FOOTNOTE.to_string(),
                    CharProps {
                        object: n.id,
                        ..fchp
                    },
                );
                self.doc.note_marks.insert(n.id, mark);
            }
        }
        self.notes.insert(n.id, paras);
    }

    fn finish_header(&mut self) {
        let Some(h) = self.header.take() else {
            return;
        };
        if !self.para.is_empty() || self.paras.is_empty() {
            self.end_paragraph(Mark::Paragraph);
        }
        let story = quill_core::Story {
            paras: std::mem::replace(&mut self.paras, h.paras),
        };
        self.para = h.para;
        self.pap = h.pap;
        let headers = self
            .sec
            .headers
            .get_or_insert_with(|| std::sync::Arc::new(self.doc.headers.clone()));
        if let Some(slot) = std::sync::Arc::make_mut(headers).get_mut(h.index) {
            *slot = story;
        }
    }

    fn end_group(&mut self) {
        match self.st.dest {
            Dest::Pict => self.finish_pict(),
            Dest::FontTable => self.register_font(),
            Dest::StyleSheet if !self.style_name.is_empty() => {
                if self.styles.len() >= MAX_TABLE_ENTRIES {
                    self.error = Some("The RTF style sheet has too many entries.");
                    return;
                }
                let name = self.style_name.trim().to_string();
                self.style_name.clear();
                let mut pap = self.style_pap.clone();
                let chp = self.st.chp;
                let id = match self.doc.styles.find(&name) {
                    Some(id) => id,
                    None => self.doc.styles.add(&name, 0, pap.clone(), chp),
                };
                pap.style = id;
                pap.in_table = false;
                let s = &mut self.doc.styles.styles[id as usize];
                s.pap = pap;
                s.chp = chp;
                self.styles.insert(self.style_num, id);
                self.pending_style
                    .push((id, self.style_based, self.style_next));
            }
            Dest::RowData => {
                self.pending_row = parse_row_data(self.row_data.trim());
                self.row_data.clear();
            }
            Dest::OldProps => {
                self.old_props = Some(CharProps {
                    rev: 0,
                    ..self.st.chp
                })
            }
            Dest::AtrfStart | Dest::AtrfEnd if self.note.is_none() && self.header.is_none() => {
                let tag = std::mem::take(&mut self.tag).trim().to_string();
                let next = crate::comments::PROVISIONAL + self.range_tags.len() as u32;
                let id = *self.range_tags.entry(tag).or_insert(next);
                self.open_comments.retain(|&c| c != id);
                if self.st.dest == Dest::AtrfStart {
                    self.open_comments.push(id);
                }
            }
            Dest::AtnRef => {
                let tag = std::mem::take(&mut self.tag).trim().to_string();
                if let Some(n) = self.note.as_ref().filter(|n| n.annotation) {
                    self.atn_tags.insert(tag, n.id);
                }
            }
            Dest::AtnParent => {
                self.atn_parent = Some(std::mem::take(&mut self.tag).trim().to_string());
            }
            _ => {}
        }
    }

    /// The format of text read now: the group's, inside the main text with
    /// the comment whose text it is.
    fn text_chp(&self) -> CharProps {
        let comment = match (&self.note, &self.header) {
            (None, None) => self.open_comments.last().copied().unwrap_or(0),
            _ => 0,
        };
        CharProps {
            comment,
            ..self.st.chp
        }
    }

    /// Who made a tracked change and when, from a revision table number and
    /// a packed date. After a few thousand different ones the rest share
    /// one unnamed stamp, so a hostile file can't fill the revision table.
    fn stamp(&mut self, author: i32, date: i32) -> quill_core::revision::Stamp {
        const MAX_STAMPS: usize = 10_000;
        let name = usize::try_from(author)
            .ok()
            .and_then(|i| self.rev_authors.get(i))
            .filter(|a| !a.is_empty())
            .map_or("Unknown", |a| a.as_str());
        let by = quill_core::revision::Stamp::new(name, &undttm(date));
        if self.stamps.contains(&by) {
            return by;
        }
        if self.stamps.len() >= MAX_STAMPS {
            return quill_core::revision::Stamp::new("Unknown", "");
        }
        self.stamps.insert(by.clone());
        by
    }

    /// Give the text being read the tracked change its words describe.
    fn update_rev(&mut self) {
        use quill_core::revision::Revision;
        let w = self.st.rev;
        let inserted = w.revised.then(|| self.stamp(w.revauth, w.revdttm));
        let deleted = w.deleted.then(|| self.stamp(w.revauthdel, w.revdttmdel));
        let formatted = w
            .old
            .filter(|old| {
                *old != CharProps {
                    rev: 0,
                    ..self.st.chp
                }
            })
            .map(|old| (self.stamp(w.crauth, w.crdate), old));
        self.st.chp.rev = quill_core::revision::intern(Revision {
            inserted,
            deleted,
            formatted,
        });
    }

    fn register_font(&mut self) {
        let name = self.font_name.trim().to_string();
        self.font_name.clear();
        if !name.is_empty() {
            if self.fonts.len() >= MAX_TABLE_ENTRIES {
                self.error = Some("The RTF font table has too many entries.");
                return;
            }
            let id = self.doc.font_id(&name);
            self.fonts.insert(self.font_num, id);
        }
    }

    fn begin_style(&mut self) {
        self.style_num = 0;
        self.style_based = None;
        self.style_next = None;
        self.style_name.clear();
        self.style_pap = ParaProps::default();
        self.st.chp = CharProps {
            font: self.font(self.default_font),
            size: 24,
            ..Default::default()
        };
    }

    fn font(&self, n: i32) -> u16 {
        self.fonts.get(&n).copied().unwrap_or(0)
    }

    /// The paragraph format being built: the document's, or the style's
    /// while reading the style sheet.
    fn pap_mut(&mut self) -> &mut ParaProps {
        if self.st.dest == Dest::StyleSheet {
            &mut self.style_pap
        } else {
            &mut self.pap
        }
    }

    fn end_paragraph(&mut self, mark: Mark) {
        if self.paragraphs >= MAX_PARAGRAPHS {
            self.error = Some("The RTF file contains too many paragraphs.");
            return;
        }
        self.paragraphs += 1;
        let mut pap = self.pap.clone();
        if mark == Mark::CellEnd || mark == Mark::RowEnd {
            pap.in_table = true;
        }
        let mut p = std::mem::replace(
            &mut self.para,
            Paragraph::new(ParaProps::default(), self.st.chp),
        );
        p.pap = pap;
        p.mark = mark;
        p.mark_chp = self.st.chp;
        self.paras.push(p);
    }

    fn word(&mut self, w: &str, p: Option<i32>) {
        let v = p.unwrap_or(0);
        let on = p.is_none_or(|v| v != 0);
        if self.st.dest == Dest::Math {
            // The font still says what character set bytes are in.
            if w == "f" {
                self.st.font = v;
            }
            // Unicode escapes still need their fallback skipped.
            if w == "u" {
                self.math_group = false;
            } else if w != "uc" {
                return self.math_word(w, v);
            }
        } else if w == "mmath" && matches!(self.st.dest, Dest::Text | Dest::Header(_)) {
            self.st.dest = Dest::Math;
            self.math_group = false;
            self.math.clear();
            let root = Element {
                name: "math".into(),
                ..Element::default()
            };
            self.math.push((self.stack.len(), root));
            return;
        }
        // Destinations first.
        match w {
            "fonttbl" => return self.st.dest = Dest::FontTable,
            "colortbl" => return self.st.dest = Dest::ColorTable,
            "stylesheet" => return self.st.dest = Dest::StyleSheet,
            "info" => return self.st.dest = Dest::Info(InfoField::None),
            "title" | "subject" | "author" | "keywords"
                if matches!(self.st.dest, Dest::Info(_)) =>
            {
                let f = match w {
                    "title" => InfoField::Title,
                    "subject" => InfoField::Subject,
                    "author" => InfoField::Author,
                    _ => InfoField::Keywords,
                };
                return self.st.dest = Dest::Info(f);
            }
            "operator" | "comment" | "doccomm" | "creatim" | "revtim" | "printim" | "buptim"
            | "company" | "category" | "manager" | "hlinkbase" => {
                return self.st.dest = Dest::Ignore;
            }
            "pict" => {
                self.pict = Pict {
                    fallback: self.st.dest == Dest::Skip,
                    ..Pict::default()
                };
                return self.st.dest = Dest::Pict;
            }
            "field" => return self.field_locked = false,
            "fldlock" => return self.field_locked = true,
            "fldinst" if matches!(self.st.dest, Dest::Text | Dest::Header(_)) => {
                self.field_instr.clear();
                return self.st.dest = Dest::FieldInst;
            }
            // A page number, date or time becomes Quill's field and its
            // stored result is skipped; a button shows the text in its
            // instruction; any other field keeps its result, as does a
            // locked one.
            "fldrslt" if matches!(self.st.dest, Dest::Text | Dest::Header(_)) => {
                let instr = std::mem::take(&mut self.field_instr);
                let known = crate::docx::read::known_field(&instr).filter(|_| !self.field_locked);
                match known {
                    Some(c) => {
                        let at = self.para.len();
                        self.para.insert(at, &c.to_string(), self.text_chp());
                        self.st.dest = Dest::Skip;
                    }
                    None if let Some(shown) = crate::docx::read::button_text(&instr) => {
                        let at = self.para.len();
                        self.para.insert(at, &shown, self.text_chp());
                        self.st.dest = Dest::Skip;
                        self.report.note(notes::FIELDS);
                    }
                    None if let Some(link) = crate::docx::read::hyperlink_field(&instr) => {
                        // The result's text is the link's; the group's end
                        // ends it.
                        self.st.chp.link = quill_core::link::intern(link);
                    }
                    None if instr
                        .trim_start()
                        .to_ascii_uppercase()
                        .starts_with("HYPERLINK") =>
                    {
                        self.report.note(notes::HYPERLINKS)
                    }
                    None if !instr.trim().is_empty() => self.report.note(notes::FIELDS),
                    None => {}
                }
                return;
            }
            "chpgn" | "chdate" | "chtime"
                if matches!(self.st.dest, Dest::Text | Dest::Header(_)) =>
            {
                let c = match w {
                    "chpgn" => chars::FIELD_PAGE,
                    "chdate" => chars::FIELD_DATE,
                    _ => chars::FIELD_TIME,
                };
                let at = self.para.len();
                self.para.insert(at, &c.to_string(), self.text_chp());
                return;
            }
            "listtext" | "pntext" if matches!(self.st.dest, Dest::Text | Dest::Header(_)) => {
                self.report.note(notes::NUMBERING);
                self.st.list_text = true;
                return;
            }
            "revised" | "deleted" | "revauth" | "revdttm" | "revauthdel" | "revdttmdel"
            | "crauth" | "crdate" => {
                let r = &mut self.st.rev;
                match w {
                    "revised" => r.revised = on,
                    "deleted" => r.deleted = on,
                    "revauth" => r.revauth = v,
                    "revdttm" => r.revdttm = v,
                    "revauthdel" => r.revauthdel = v,
                    "revdttmdel" => r.revdttmdel = v,
                    "crauth" => r.crauth = v,
                    _ => r.crdate = v,
                }
                return self.update_rev();
            }
            "revtbl" => {
                self.rev_authors.clear();
                self.rev_author.clear();
                return self.st.dest = Dest::RevTbl;
            }
            "oldcprops" => return self.st.dest = Dest::OldProps,
            "revisions" => return self.doc.props.track_changes = on,
            "object" => {
                self.report.note(notes::OBJECTS);
                return self.st.dest = Dest::Skip;
            }
            "shp" | "do" => self.report.note(notes::DRAWINGS),
            "shptxt" => self.report.note(notes::TEXT_BOXES_LOST),
            "xe" | "tc" => {
                self.report.note(notes::INDEX_ENTRIES);
                return self.st.dest = Dest::Skip;
            }
            "itap" if v >= 2 => self.report.note(notes::NESTED_TABLES),
            "pgwsxn" | "pghsxn" => {
                let first = if w == "pgwsxn" {
                    &mut self.page_size.0
                } else {
                    &mut self.page_size.1
                };
                match *first {
                    None => *first = Some(v),
                    Some(size) if size != v => self.report.note(notes::PAGE_STYLES),
                    Some(_) => {}
                }
            }
            "footnote" if self.note.is_none() && self.st.dest == Dest::Text => {
                let (id, custom) = match self.pending_ref.take() {
                    Some(id) => (id, false),
                    None => {
                        self.next_note += 1;
                        (self.next_note, true)
                    }
                };
                let chp = self.st.chp;
                self.note = Some(NoteRead {
                    id,
                    depth: self.stack.len(),
                    custom,
                    annotation: false,
                    para: std::mem::replace(
                        &mut self.para,
                        Paragraph::new(ParaProps::default(), chp),
                    ),
                    paras: std::mem::take(&mut self.paras),
                    pap: std::mem::take(&mut self.pap),
                });
                return;
            }
            "atnid" => {
                self.atn_initials.clear();
                return self.st.dest = Dest::AtnId;
            }
            "atnauthor" => {
                self.atn_author.clear();
                return self.st.dest = Dest::AtnAuthor;
            }
            "quillatndate" => {
                self.atn_date.clear();
                return self.st.dest = Dest::AtnDate;
            }
            "atrfstart" | "atrfend" | "atnref" | "quillatnparent" => {
                self.tag.clear();
                return self.st.dest = match w {
                    "atrfstart" => Dest::AtrfStart,
                    "atrfend" => Dest::AtrfEnd,
                    "atnref" => Dest::AtnRef,
                    _ => Dest::AtnParent,
                };
            }
            "quillatndone" => {
                self.atn_done = true;
                return self.st.dest = Dest::Skip;
            }
            "quillversion" => return self.st.dest = Dest::Info(InfoField::Version),
            "quillrow" => {
                self.row_data.clear();
                return self.st.dest = Dest::RowData;
            }
            "header" | "headerr" | "footer" | "footerr" | "headerf" | "footerf" | "headerl"
            | "footerl"
                if self.header.is_none() && self.note.is_none() =>
            {
                let index = match w {
                    "header" | "headerr" => 0,
                    "footer" | "footerr" => 1,
                    "headerf" => 2,
                    "footerf" => 3,
                    "headerl" => 4,
                    _ => 5,
                };
                let chp = self.st.chp;
                self.header = Some(HeaderRead {
                    index,
                    depth: self.stack.len(),
                    para: std::mem::replace(
                        &mut self.para,
                        Paragraph::new(ParaProps::default(), chp),
                    ),
                    paras: std::mem::take(&mut self.paras),
                    pap: std::mem::take(&mut self.pap),
                });
                return self.st.dest = Dest::Header(index);
            }
            "annotation" if self.note.is_none() && self.st.dest == Dest::Text => {
                let id = match self.pending_atn.take() {
                    Some(id) => id,
                    None => {
                        self.next_note += 1;
                        self.next_note
                    }
                };
                let chp = self.st.chp;
                self.note = Some(NoteRead {
                    id,
                    depth: self.stack.len(),
                    custom: false,
                    annotation: true,
                    para: std::mem::replace(
                        &mut self.para,
                        Paragraph::new(ParaProps::default(), chp),
                    ),
                    paras: std::mem::take(&mut self.paras),
                    pap: std::mem::take(&mut self.pap),
                });
                return;
            }
            "chatn" if self.st.dest == Dest::Text => {
                let id = match &self.note {
                    Some(n) => n.id,
                    None => {
                        self.next_note += 1;
                        let id = self.next_note;
                        self.pending_atn = Some(id);
                        self.doc.annotation_info.insert(
                            id,
                            quill_core::notes::AnnotationInfo {
                                initials: self.atn_initials.trim().to_string(),
                                author: self.atn_author.trim().to_string(),
                                date: self.atn_date.trim().to_string(),
                                done: std::mem::take(&mut self.atn_done),
                                ..Default::default()
                            },
                        );
                        if let Some(tag) = self.atn_parent.take() {
                            self.parents.push((id, tag));
                        }
                        id
                    }
                };
                let chp = CharProps {
                    object: id,
                    ..self.st.chp
                };
                let at = self.para.len();
                self.para.insert(at, &chars::ANNOTATION.to_string(), chp);
                return;
            }
            "chftn" if self.st.dest == Dest::Text => {
                let id = match &self.note {
                    Some(n) => n.id,
                    None => {
                        self.next_note += 1;
                        self.pending_ref = Some(self.next_note);
                        self.next_note
                    }
                };
                let chp = CharProps {
                    object: id,
                    ..self.text_chp()
                };
                let at = self.para.len();
                self.para.insert(at, &chars::FOOTNOTE.to_string(), chp);
                return;
            }
            // Not yet supported: kept out of the text rather than shown as
            // gibberish.
            "nonshppict" | "footnote" | "fldinst" | "annotation" | "bkmkstart" | "bkmkend"
            | "themedata" | "colorschememapping" | "latentstyles" | "datastore" | "listtable"
            | "listoverridetable" | "rsidtbl" | "generator" | "pntext" | "pntxtb" | "pntxta"
            | "listtext" => {
                return self.st.dest = Dest::Skip;
            }
            _ => {}
        }
        if matches!(self.st.dest, Dest::Skip | Dest::Ignore) {
            return;
        }
        if self.st.dest == Dest::Pict {
            let p = &mut self.pict;
            match w {
                "pngblip" => p.format = Some(quill_core::ImageFormat::Png),
                "jpegblip" => p.format = Some(quill_core::ImageFormat::Jpeg),
                "dibitmap" => p.dib = true,
                "picwgoal" => p.goal.0 = v,
                "pichgoal" => p.goal.1 = v,
                "picscalex" => p.scale.0 = v,
                "picscaley" => p.scale.1 = v,
                "piccropl" => p.crop[0] = v,
                "piccropt" => p.crop[1] = v,
                "piccropr" => p.crop[2] = v,
                "piccropb" => p.crop[3] = v,
                "quillcropl" => p.exact_crop[0] = Some(v),
                "quillcropt" => p.exact_crop[1] = Some(v),
                "quillcropr" => p.exact_crop[2] = Some(v),
                "quillcropb" => p.exact_crop[3] = Some(v),
                "quillsizew" => p.exact_size[0] = Some(v),
                "quillsizeh" => p.exact_size[1] = Some(v),
                _ => {}
            }
            return;
        }
        if self.st.dest == Dest::ColorTable {
            match w {
                "red" => self.color.0 = v as u8,
                "green" => self.color.1 = v as u8,
                "blue" => self.color.2 = v as u8,
                _ => {}
            }
            return;
        }
        if self.st.dest == Dest::FontTable {
            if w == "f" {
                self.font_num = v;
            } else if w == "u" {
                // A name's other characters, and its semicolons: only a
                // plain one ends it.
                let c = if v < 0 { (v + 65536) as u32 } else { v as u32 };
                if let Some(c) = char::from_u32(c) {
                    self.font_name.push(c);
                }
                self.skip = self.st.uc;
            } else if w == "uc" {
                self.st.uc = v.max(0) as usize;
            } else if w == "fcharset" {
                // The character set a font's bytes are in, as a code page.
                let page = match v {
                    161 => 1253,
                    162 => 1254,
                    163 => 1258,
                    177 => 1255,
                    178 => 1256,
                    186 => 1257,
                    204 => 1251,
                    222 => 874,
                    238 => 1250,
                    _ => 0,
                };
                if page != 0 && self.font_pages.len() < MAX_TABLE_ENTRIES {
                    self.font_pages.insert(self.font_num, page);
                }
            }
            return;
        }
        match w {
            "mac" => self.charset = Charset::Mac,
            "ansi" | "pc" | "pca" => self.charset = Charset::Ansi,
            "ansicpg" => self.codepage = u16::try_from(v).unwrap_or(1252),
            "deff" => {
                self.default_font = v;
                self.st.font = v;
            }
            "uc" => self.st.uc = v.max(0) as usize,
            "u" => {
                let c = if v < 0 { (v + 65536) as u32 } else { v as u32 };
                // An emoji or other character beyond U+FFFF comes as a pair.
                let c = match (self.high_surrogate.take(), c) {
                    (_, 0xD800..=0xDBFF) => {
                        self.high_surrogate = Some(c);
                        self.skip = self.st.uc;
                        return;
                    }
                    (Some(high), 0xDC00..=0xDFFF) => {
                        0x10000 + ((high - 0xD800) << 10) + (c - 0xDC00)
                    }
                    (_, c) => c,
                };
                match char::from_u32(c) {
                    // An escaped semicolon is part of a style's name, not
                    // the end of it.
                    Some(c) if self.st.dest == Dest::StyleSheet => self.style_name.push(c),
                    Some(c) => self.ch(c),
                    None => {}
                }
                self.skip = self.st.uc;
            }
            // Document settings.
            "paperw" => self.doc.props.page_width = v,
            "paperh" => self.doc.props.page_height = v,
            "margl" => self.doc.props.margin_left = v,
            "margr" => self.doc.props.margin_right = v,
            "margt" => {
                self.doc.props.margin_top = v.abs();
                self.doc.props.top_exact = v < 0;
            }
            "margb" => {
                self.doc.props.margin_bottom = v.abs();
                self.doc.props.bottom_exact = v < 0;
            }
            "gutter" => self.doc.props.gutter = v,
            "deftab" if v > 0 => self.doc.props.default_tab = v,
            "widowctrl" => self.doc.props.widow_control = on,
            "margmirror" => self.doc.props.mirror_margins = true,
            "printhidden" => self.doc.props.print_hidden = on,
            "facingp" => self.doc.props.even_odd_headers = on,
            // Paragraph structure.
            "par" => {
                if matches!(self.st.dest, Dest::Text | Dest::Header(_)) {
                    self.end_paragraph(Mark::Paragraph)
                }
            }
            "sect" => {
                self.end_paragraph(Mark::SectionBreak);
                let first_section = self
                    .paras
                    .iter()
                    .filter(|p| p.mark == Mark::SectionBreak)
                    .count()
                    == 1;
                if first_section && let Some(headers) = self.sec.headers.take() {
                    self.doc.headers = (*headers).clone();
                }
                if let Some(p) = self.paras.last_mut() {
                    p.section = Some(Box::new(self.sec.clone()));
                }
            }
            // Section settings, for the section they come before.
            "sectd" => {
                self.sec = SectionProps {
                    start: SectionStart::NewPage,
                    ..SectionProps::default()
                }
            }
            "sbknone" => self.sec.start = SectionStart::NoBreak,
            "sbkcol" => self.sec.start = SectionStart::NewColumn,
            "sbkpage" => self.sec.start = SectionStart::NewPage,
            "sbkeven" => self.sec.start = SectionStart::EvenPage,
            "sbkodd" => self.sec.start = SectionStart::OddPage,
            "cols" => self.sec.columns = v.clamp(1, 100) as u16,
            "colsx" => self.sec.column_spacing = v.max(0),
            "headery" => self.sec.header_from_top = v.max(0),
            "footery" => self.sec.footer_from_bottom = v.max(0),
            "titlepg" => self.sec.different_first_page = true,
            "pgnrestart" => self.sec.restart_page_numbers = true,
            "pgndec" => self.sec.page_number_format = PageNumberFormat::Arabic,
            "pgnucrm" => self.sec.page_number_format = PageNumberFormat::UpperRoman,
            "pgnlcrm" => self.sec.page_number_format = PageNumberFormat::LowerRoman,
            "pgnucltr" => self.sec.page_number_format = PageNumberFormat::UpperLetter,
            "pgnlcltr" => self.sec.page_number_format = PageNumberFormat::LowerLetter,
            "pgnx" => {
                self.sec.margin_page_numbers = true;
                self.sec.page_number_from_right = v.max(0);
            }
            "pgny" => {
                self.sec.margin_page_numbers = true;
                self.sec.page_number_from_top = v.max(0);
            }
            "endnhere" => self.sec.include_endnotes = true,
            "linemod" if v > 0 => {
                self.sec.line_count_by = v.min(100) as u16;
                if self.sec.line_numbers == quill_core::LineNumbering::Off {
                    self.sec.line_numbers = quill_core::LineNumbering::BySection;
                }
            }
            "linex" => self.sec.line_from_text = Some(v.max(0)),
            "lineppage" => self.sec.line_numbers = quill_core::LineNumbering::ByPage,
            "linerestart" => self.sec.line_numbers = quill_core::LineNumbering::BySection,
            "linecont" => self.sec.line_numbers = quill_core::LineNumbering::Continuous,
            // Footnote settings.
            "ftnbj" => self.doc.props.footnote_position = FootnotePosition::BottomOfPage,
            "ftntj" => self.doc.props.footnote_position = FootnotePosition::BeneathText,
            "endnotes" => self.doc.props.footnote_position = FootnotePosition::EndOfSection,
            "enddoc" => self.doc.props.footnote_position = FootnotePosition::EndOfDocument,
            "ftnstart" => self.doc.props.footnote_start = v.clamp(1, 32767) as u16,
            "ftnrstpg" => self.doc.props.footnote_restart_each_page = true,
            "cell" => self.end_paragraph(Mark::CellEnd),
            "row" => {
                self.para = Paragraph::new(ParaProps::default(), self.st.chp);
                self.end_paragraph(Mark::RowEnd);
                if let (Some(row), Some(p)) = (self.pending_row.take(), self.paras.last_mut()) {
                    p.row = Some(Box::new(row));
                }
            }
            "line" => self.ch(chars::LINE_BREAK),
            "page" => self.ch(chars::PAGE_BREAK),
            "tab" => self.ch(chars::TAB),
            "emdash" => self.ch('\u{2014}'),
            "endash" => self.ch('\u{2013}'),
            "bullet" => self.ch('•'),
            "lquote" => self.ch('‘'),
            "rquote" => self.ch('’'),
            "ldblquote" => self.ch('“'),
            "rdblquote" => self.ch('”'),
            "emspace" | "enspace" => self.ch(' '),
            // Character formatting.
            "plain" => {
                self.st.font = self.default_font;
                self.st.rev = RevWords::default();
                // \plain resets formatting, not the link it is inside.
                self.st.chp = CharProps {
                    font: self.font(self.default_font),
                    size: 24,
                    link: self.st.chp.link,
                    ..Default::default()
                }
            }
            "f" => {
                self.st.chp.font = self.font(v);
                self.st.font = v;
            }
            "fs" => self.st.chp.size = v.clamp(2, 3200) as u16,
            "b" => self.st.chp.bold = on,
            "i" => self.st.chp.italic = on,
            "strike" | "striked" => self.st.chp.strike = on,
            "outl" => self.st.chp.outline = on,
            "shad" => self.st.chp.shadow = on,
            "scaps" => self.st.chp.small_caps = on,
            "caps" => self.st.chp.all_caps = on,
            "v" => self.st.chp.hidden = on,
            "ul" => {
                self.st.chp.underline = if on {
                    Underline::Single
                } else {
                    Underline::None
                }
            }
            "ulw" => self.st.chp.underline = Underline::Word,
            "uldb" => self.st.chp.underline = Underline::Double,
            "uld" | "uldash" | "uldashd" => self.st.chp.underline = Underline::Dotted,
            "ulnone" => self.st.chp.underline = Underline::None,
            "cf" => self.st.chp.color = self.colors.get(v as usize).copied().unwrap_or(Rgb::BLACK),
            "up" => self.st.chp.position = if p.is_none() { 60 } else { v * 10 },
            "dn" => self.st.chp.position = -(if p.is_none() { 60 } else { v * 10 }),
            "super" => self.st.chp.position = crate::superscript_rise(self.st.chp.size),
            "sub" => self.st.chp.position = -crate::subscript_drop(self.st.chp.size),
            "nosupersub" => self.st.chp.position = 0,
            "expndtw" => self.st.chp.spacing = v,
            "expnd" => self.st.chp.spacing = v * 5,
            // Paragraph formatting.
            "pard" => {
                self.pap = ParaProps::default();
                self.st.tab_align = TabAlign::Left;
                self.st.tab_leader = TabLeader::None;
            }
            "s" => {
                if self.st.dest == Dest::StyleSheet {
                    self.style_num = v;
                } else {
                    self.pap.style = self.styles.get(&v).copied().unwrap_or(0);
                }
            }
            "sbasedon" => self.style_based = Some(v),
            "snext" => self.style_next = Some(v),
            "intbl" => self.pap_mut().in_table = true,
            "ql" => self.pap_mut().align = Align::Left,
            "qc" => self.pap_mut().align = Align::Center,
            "qr" => self.pap_mut().align = Align::Right,
            "qj" => self.pap_mut().align = Align::Justify,
            "li" => self.pap_mut().left = v,
            "ri" => self.pap_mut().right = v,
            "fi" => self.pap_mut().first = v,
            "sb" => self.pap_mut().before = v,
            "sa" => self.pap_mut().after = v,
            "sl" => {
                self.pap_mut().line = match v {
                    0 => LineSpacing::Auto,
                    v if v < 0 => LineSpacing::Exactly(-v),
                    v => LineSpacing::AtLeast(v),
                }
            }
            "slmult" => {
                if v == 1
                    && let LineSpacing::AtLeast(t) = self.pap_mut().line
                {
                    self.pap_mut().line = if t == 240 {
                        LineSpacing::Auto
                    } else {
                        LineSpacing::Multiple(t as u16)
                    };
                }
            }
            "keepn" => self.pap_mut().keep_with_next = true,
            "absw" | "phcol" | "phmrg" | "phpg" | "posx" | "posxl" | "posxc" | "posxr"
            | "posxi" | "posxo" | "pvmrg" | "pvpg" | "pvpara" | "posy" | "posyil" | "posyt"
            | "posyc" | "posyb" | "dxfrtext" | "dropcapli" | "dropcapt" => {
                use quill_core::{FrameH, FrameRel, FrameV};
                let f = self.pap_mut().frame.get_or_insert_with(Default::default);
                match w {
                    "absw" => f.width = (v > 0).then_some(v),
                    "phcol" => f.h_rel = FrameRel::Column,
                    "phmrg" => f.h_rel = FrameRel::Margin,
                    "phpg" => f.h_rel = FrameRel::Page,
                    "posx" => f.h = FrameH::At(v),
                    "posxl" => f.h = FrameH::Left,
                    "posxc" => f.h = FrameH::Center,
                    "posxr" => f.h = FrameH::Right,
                    "posxi" => f.h = FrameH::Inside,
                    "posxo" => f.h = FrameH::Outside,
                    "pvmrg" => f.v_rel = FrameRel::Margin,
                    "pvpara" => f.v_rel = FrameRel::Column,
                    "pvpg" => f.v_rel = FrameRel::Page,
                    "posy" => f.v = FrameV::At(v),
                    "posyil" => f.v = FrameV::InLine,
                    "posyt" => f.v = FrameV::Top,
                    "posyc" => f.v = FrameV::Center,
                    "posyb" => f.v = FrameV::Bottom,
                    "dxfrtext" => f.from_text = v.max(0),
                    "dropcapli" => f.drop_lines = v.clamp(1, 10) as u8,
                    _ => f.in_margin = v == 2,
                }
            }
            "keep" => self.pap_mut().keep_together = true,
            "pagebb" => self.pap_mut().page_break_before = true,
            "noline" => self.pap_mut().suppress_line_numbers = true,
            "tqc" => self.st.tab_align = TabAlign::Center,
            "tqr" => self.st.tab_align = TabAlign::Right,
            "tqdec" => self.st.tab_align = TabAlign::Decimal,
            "tldot" | "tlmdot" => self.st.tab_leader = TabLeader::Dots,
            "tlhyph" => self.st.tab_leader = TabLeader::Dashes,
            "tlul" | "tlth" => self.st.tab_leader = TabLeader::Underline,
            "tx" | "tb" => {
                let align = if w == "tb" {
                    TabAlign::Bar
                } else {
                    self.st.tab_align
                };
                let leader = self.st.tab_leader;
                self.pap_mut().set_tab(TabStop {
                    pos: v,
                    align,
                    leader,
                });
                self.st.tab_align = TabAlign::Left;
                self.st.tab_leader = TabLeader::None;
            }
            "brdrt" => self.st.border = Some(0),
            "brdrb" => self.st.border = Some(1),
            "brdrl" => self.st.border = Some(2),
            "brdrr" => self.st.border = Some(3),
            "brdrbtw" => self.st.border = Some(4),
            "box" => self.st.border = Some(5),
            "brdrsh" => self.pap_mut().borders.shadow = true,
            "brdrs" | "brdrth" | "brdrdb" | "brdrdot" | "brdrhair" | "brdrdash" | "brdrw"
            | "brsp" => {
                let style = match w {
                    "brdrs" => Some(BorderStyle::Single),
                    "brdrth" => Some(BorderStyle::Thick),
                    "brdrdb" => Some(BorderStyle::Double),
                    "brdrdot" | "brdrdash" => Some(BorderStyle::Dotted),
                    "brdrhair" => Some(BorderStyle::Hairline),
                    _ => None,
                };
                let side = self.st.border;
                let b = &mut self.pap_mut().borders;
                let lines: Vec<&mut BorderLine> = match side {
                    Some(0) => vec![&mut b.top],
                    Some(1) => vec![&mut b.bottom],
                    Some(2) => vec![&mut b.left],
                    Some(3) => vec![&mut b.right],
                    Some(4) => vec![&mut b.between],
                    Some(5) => vec![&mut b.top, &mut b.bottom, &mut b.left, &mut b.right],
                    _ => vec![],
                };
                for l in lines {
                    if let Some(s) = style {
                        l.style = s;
                    }
                    if w == "brsp" {
                        l.space = v;
                    }
                }
            }
            "shading" => self.pap_mut().shading = (v / 10).clamp(0, 1000) as u16,
            _ => {}
        }
    }

    fn finish(mut self) -> Result<Document, String> {
        if let Some(error) = self.error {
            return Err(error.into());
        }
        self.finish_note();
        self.finish_header();
        if let Some(error) = self.error {
            return Err(error.into());
        }
        if !self.para.is_empty() {
            self.end_paragraph(Mark::Paragraph);
        }
        if !self.paras.iter().any(|p| p.mark == Mark::SectionBreak)
            && let Some(headers) = self.sec.headers.take()
        {
            self.doc.headers = (*headers).clone();
        }
        self.doc.section = self.sec.clone();
        for (id, based, next) in std::mem::take(&mut self.pending_style) {
            let based = based.and_then(|b| self.styles.get(&b).copied());
            let next = next.and_then(|n| self.styles.get(&n).copied());
            let s = &mut self.doc.styles.styles[id as usize];
            if id != 0 {
                s.based_on = based.or(Some(0));
            }
            if let Some(n) = next {
                s.next = n;
            }
        }
        if self.paras.is_empty() {
            let n = self.doc.styles.get(0);
            self.paras.push(Paragraph::new(n.pap.clone(), n.chp));
        }
        if self.paras.last().is_some_and(|p| p.pap.in_table) {
            let chp = self.paras.last().unwrap().mark_chp;
            self.paras.push(Paragraph::new(ParaProps::default(), chp));
        }
        self.doc.main.paras = self.paras;
        if !self.notes.is_empty() {
            let mut story = Vec::new();
            for id in quill_core::notes::note_ids(&self.doc.main.paras) {
                story.extend(self.notes.remove(&id).unwrap_or_default());
            }
            if !story.is_empty() {
                self.doc.set_notes(quill_core::Story { paras: story });
            }
            self.doc.reconcile_notes();
            self.doc.forget_history();
        }
        // Replies, then the text each comment is about.
        for (id, tag) in std::mem::take(&mut self.parents) {
            let parent = self.atn_tags.get(&tag).copied().filter(|&p| p != id);
            if let Some(info) = self.doc.annotation_info.get_mut(&id) {
                info.parent = parent;
            }
        }
        let real: HashMap<u32, u32> = self
            .range_tags
            .iter()
            .filter_map(|(tag, &prov)| Some((prov, *self.atn_tags.get(tag)?)))
            .collect();
        crate::comments::settle(&mut self.doc, &real);
        if !self.annotations.is_empty() {
            let mut story = Vec::new();
            for id in quill_core::notes::annotation_ids(&self.doc.main.paras) {
                story.extend(self.annotations.remove(&id).unwrap_or_default());
            }
            if !story.is_empty() {
                self.doc.set_annotations(quill_core::Story { paras: story });
            }
            self.doc.reconcile(quill_core::notes::NoteKind::Annotation);
        }
        self.doc.forget_history();
        Ok(self.doc)
    }
}

/// Destinations that may appear after `\*` and that we understand.
/// RTF's packed date (see `write::dttm`) as Word writes dates in .docx:
/// "2026-10-23T14:05:00Z". "" for 0 or nonsense.
fn undttm(v: i32) -> String {
    let v = v as u32;
    let (mi, h, d, mo, y) = (
        v & 0x3F,
        (v >> 6) & 0x1F,
        (v >> 11) & 0x1F,
        (v >> 16) & 0xF,
        ((v >> 20) & 0x1FF) + 1900,
    );
    if v == 0 || !(1..=12).contains(&mo) || d == 0 || h > 23 || mi > 59 {
        return String::new();
    }
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:00Z")
}

fn known_destination(word: &str) -> bool {
    matches!(
        word,
        "atrfstart"
            | "atrfend"
            | "atnref"
            | "quillatnparent"
            | "quillatndone"
            | "revtbl"
            | "oldcprops"
            | "fldinst"
            | "shppict"
            | "annotation"
            | "atnid"
            | "atnauthor"
            | "quillatndate"
            | "quillversion"
            | "quillrow"
            | "header"
            | "headerr"
            | "footer"
            | "footerr"
            | "headerf"
            | "footerf"
            | "headerl"
            | "footerl"
    )
}

fn parse_border_style(n: i32) -> Option<BorderStyle> {
    Some(match n {
        0 => BorderStyle::None,
        1 => BorderStyle::Dotted,
        2 => BorderStyle::Hairline,
        3 => BorderStyle::Single,
        4 => BorderStyle::Thick,
        5 => BorderStyle::Double,
        _ => return None,
    })
}

fn parse_row_data(data: &str) -> Option<RowProps> {
    let mut parts = data.split('|');
    let head: Vec<i32> = parts
        .next()?
        .split(',')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    if head.len() != 5 {
        return None;
    }
    let height = match head[2] {
        0 => RowHeight::Auto,
        1 => RowHeight::AtLeast(head[3].max(0)),
        2 => RowHeight::Exactly(head[3].max(0)),
        _ => return None,
    };
    let align = match head[4] {
        0 => Align::Left,
        1 => Align::Center,
        2 => Align::Right,
        _ => return None,
    };
    let mut cells = Vec::new();
    for part in parts {
        if cells.len() >= MAX_TABLE_ENTRIES {
            return None;
        }
        let v: Vec<i32> = part
            .split(',')
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        if v.len() != 13 || v[0] < 0 || !(0..=1000).contains(&v[1]) {
            return None;
        }
        let mut borders = quill_core::props::Borders {
            shadow: v[2] != 0,
            ..Default::default()
        };
        for (line, pair) in [
            (&mut borders.top, &v[3..5]),
            (&mut borders.bottom, &v[5..7]),
            (&mut borders.left, &v[7..9]),
            (&mut borders.right, &v[9..11]),
            (&mut borders.between, &v[11..13]),
        ] {
            line.style = parse_border_style(pair[0])?;
            line.space = pair[1];
        }
        cells.push(CellProps {
            width: v[0],
            shading: v[1] as u16,
            borders,
        });
    }
    if cells.is_empty() {
        return None;
    }
    Some(RowProps {
        cells,
        gap: head[0].max(0),
        indent: head[1],
        height,
        align,
    })
}

/// Bytes 0x80-0xFF in the Mac Roman character set.
pub(crate) const MAC_ROMAN: [char; 128] = [
    'Ä', 'Å', 'Ç', 'É', 'Ñ', 'Ö', 'Ü', 'á', 'à', 'â', 'ä', 'ã', 'å', 'ç', 'é', 'è', 'ê', 'ë', 'í',
    'ì', 'î', 'ï', 'ñ', 'ó', 'ò', 'ô', 'ö', 'õ', 'ú', 'ù', 'û', 'ü', '†', '°', '¢', '£', '§', '•',
    '¶', 'ß', '®', '©', '™', '´', '¨', '≠', 'Æ', 'Ø', '∞', '±', '≤', '≥', '¥', 'µ', '∂', '∑', '∏',
    'π', '∫', 'ª', 'º', 'Ω', 'æ', 'ø', '¿', '¡', '¬', '√', 'ƒ', '≈', '∆', '«', '»', '…', '\u{A0}',
    'À', 'Ã', 'Õ', 'Œ', 'œ', '\u{2013}', '\u{2014}', '“', '”', '‘', '’', '÷', '◊', 'ÿ', 'Ÿ', '⁄',
    '€', '‹', '›', 'ﬁ', 'ﬂ', '‡', '·', '‚', '„', '‰', 'Â', 'Ê', 'Á', 'Ë', 'È', 'Í', 'Î', 'Ï', 'Ì',
    'Ó', 'Ô', '\u{F8FF}', 'Ò', 'Ú', 'Û', 'Ù', 'ı', 'ˆ', '˜', '¯', '˘', '˙', '˚', '¸', '˝', '˛',
    'ˇ',
];

/// Bytes 0x80-0x9F in Windows-1252, which differ from Latin-1.
pub(crate) const CP1252: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}',
    '\u{90}', '‘', '’', '“', '”', '•', '\u{2013}', '\u{2014}', '˜', '™', 'š', '›', 'œ', '\u{9D}',
    'ž', 'Ÿ',
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_broken_escapes_and_binary_data() {
        for bad in [
            b"}{\\rtf1 x}".as_slice(),
            b"{\\rtf1 \\'a}".as_slice(),
            b"{\\rtf1 \\'zz}".as_slice(),
            b"{\\rtf1 \\bin4 ab}".as_slice(),
            b"{\\rtf1 \\bin-4 ab}".as_slice(),
        ] {
            assert!(read(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn tolerates_what_word_tolerates() {
        // Cut off, trailing data, a stray brace and oversized numbers all
        // keep the text, as Word does.
        for (rtf, text) in [
            (b"{\\rtf1 unclosed".as_slice(), "unclosed"),
            (b"{\\rtf1 x}}".as_slice(), "x"),
            (b"{\\rtf1 trailing}junk".as_slice(), "trailing"),
            (b"{\\rtf1 cut\\".as_slice(), "cut"),
            (b"{\\rtf1 \\fs999999999999999999999 x}".as_slice(), "x"),
            (
                b"{\\rtf1{\\*\\rsidtbl \\rsid12810266}\\pard\\insrsid13321744 y\\par}".as_slice(),
                "y",
            ),
        ] {
            let doc = read(rtf).unwrap_or_else(|e| panic!("{rtf:?}: {e}"));
            assert_eq!(doc.main.paras[0].text, text, "{rtf:?}");
        }
    }

    #[test]
    fn page_date_and_time_fields_become_quills_and_others_keep_their_text() {
        let (doc, report) = read_reported(
            br"{\rtf1 {\field{\*\fldinst { PAGE \\* MERGEFORMAT }}{\fldrslt 7}} {\field{\*\fldinst AUTHOR}{\fldrslt Ada}} \chdate {\field{\*\fldinst MACROBUTTON NoMacro [Name]}{\fldrslt }}\par}",
        )
        .unwrap();
        assert_eq!(doc.main.paras[0].text, "\u{E000} Ada \u{E001}[Name]");
        assert_eq!(report.notes(), [notes::FIELDS]);
    }

    #[test]
    fn bytes_follow_their_font_s_character_set_and_the_document_s_code_page() {
        let doc = read(
            br"{\rtf1\ansi\ansicpg1251\deff0{\fonttbl{\f0 Arial;}{\f1\fcharset161 Arial Greek;}}\'c0 {\f1 \'f0}\par}",
        )
        .unwrap();
        assert_eq!(doc.main.paras[0].text, "\u{410} \u{3C0}");
    }

    #[test]
    fn font_names_with_semicolons_and_other_scripts_survive_saving() {
        let mut doc = quill_core::text::from_plain_text("x");
        let font = doc.font_id("Times New Roman;Serif");
        doc.main.paras[0].runs[0].chp.font = font;
        let back = read(&crate::rtf::write(&doc)).unwrap();
        let chp = back.main.paras[0].chp_of_char(0);
        assert_eq!(back.font_name(chp.font), "Times New Roman;Serif");
        let mut doc = quill_core::text::from_plain_text("x");
        doc.main.paras[0].runs[0].chp.font = doc.font_id("ＭＳ 明朝");
        let back = read(&crate::rtf::write(&doc)).unwrap();
        let chp = back.main.paras[0].chp_of_char(0);
        assert_eq!(back.font_name(chp.font), "ＭＳ 明朝");
        // Older files still list several fonts in one group.
        let old = read(br"{\rtf1{\fonttbl{\f0 Arial;\f1 Courier;}}{\f1 y}\par}").unwrap();
        let chp = old.main.paras[0].chp_of_char(0);
        assert_eq!(old.font_name(chp.font), "Courier");
    }

    #[test]
    fn tables_in_headers_and_notes_keep_their_rows() {
        use quill_core::{Position, StoryId};
        let mut doc = quill_core::text::from_plain_text("Body");
        doc.insert_table(StoryId::Header, Position::new(0, 0), 2, 3, 1500);
        let rows = |d: &Document| -> Vec<quill_core::RowProps> {
            d.story(StoryId::Header)
                .paras
                .iter()
                .filter_map(|p| p.row.as_deref().cloned())
                .collect()
        };
        assert_eq!(rows(&doc).len(), 2);
        let back = read(&crate::rtf::write(&doc)).unwrap();
        assert_eq!(rows(&back), rows(&doc));
    }

    #[test]
    fn colors_in_headers_and_semicolons_in_style_names_survive_saving() {
        use quill_core::{CharProps, Position, StoryId};
        let mut doc = quill_core::text::from_plain_text("Body");
        let red = CharProps {
            color: Rgb(0xC00000),
            ..Default::default()
        };
        doc.insert_text(StoryId::Header, Position::new(0, 0), "Gray", red);
        let mut style = doc.styles.styles[0].clone();
        style.name = "Closing;signatures".into();
        style.builtin = false;
        style.pap.style = doc.styles.styles.len() as u16;
        doc.styles.styles.push(style);
        let back = read(&crate::rtf::write(&doc)).unwrap();
        let header = &back.story(StoryId::Header).paras[0];
        assert_eq!(header.chp_of_char(0).color, Rgb(0xC00000));
        assert!(back.styles.find("Closing;signatures").is_some());
    }

    #[test]
    fn emoji_and_other_characters_beyond_u_ffff_survive_saving() {
        let doc = quill_core::text::from_plain_text("Launch \u{1F680} and \u{20BB7}");
        let back = read(&crate::rtf::write(&doc)).unwrap();
        assert_eq!(back.main.paras[0].text, "Launch \u{1F680} and \u{20BB7}");
    }

    #[test]
    fn a_custom_footnote_mark_can_run_straight_into_the_note() {
        let doc = read(br"{\rtf1 Text.{\footnote .See page 4.}\par}").unwrap();
        let p = &doc.main.paras[0];
        assert_eq!(p.text, format!("Text{}", chars::FOOTNOTE));
        let id = p.chp_of_char(4).object;
        assert_eq!(doc.note_marks.get(&id).map(String::as_str), Some("."));
        assert!(doc.notes.paras[0].text.ends_with("See page 4."));
    }

    #[test]
    fn frames_keep_what_they_are_placed_from() {
        use quill_core::FrameRel;
        let doc = read(br"{\rtf1\pard\pvpara\posy100 a\par\pard\pvpg\posy100 b\par}").unwrap();
        let rel = |d: &Document, i: usize| d.main.paras[i].pap.frame.unwrap().v_rel;
        assert_eq!(rel(&doc, 0), FrameRel::Column);
        assert_eq!(rel(&doc, 1), FrameRel::Page);
        let back = read(&crate::rtf::write(&doc)).unwrap();
        assert_eq!(rel(&back, 0), FrameRel::Column);
        assert_eq!(rel(&back, 1), FrameRel::Page);
    }

    #[test]
    fn locked_fields_keep_their_text_and_quill_s_fields_save_as_rtf_s_own() {
        let doc = read(
            br#"{\rtf1{\field\fldlock{\*\fldinst DATE}{\fldrslt 1/2/03}} {\field{\*\fldinst DATE}{\fldrslt 1/2/03}} {\field{\*\fldinst DATE \\@ "d MMMM"}{\fldrslt 2 January}}\par}"#,
        )
        .unwrap();
        let date = chars::FIELD_DATE;
        assert_eq!(doc.main.paras[0].text, format!("1/2/03 {date} 2 January"));
        let mut doc = Document::new();
        let chp = doc.main.paras[0].chp_of_char(0);
        doc.main.paras[0].insert(
            0,
            &format!("{}{}{}", chars::FIELD_PAGE, date, chars::FIELD_TIME),
            chp,
        );
        let rtf = String::from_utf8(crate::rtf::write(&doc)).unwrap();
        assert!(rtf.contains(r"\chpgn \chdate \chtime "), "{rtf}");
        assert_eq!(
            read(rtf.as_bytes()).unwrap().main.paras[0].text,
            doc.main.paras[0].text
        );
    }

    #[test]
    fn equations_are_read_as_linear_text() {
        let (doc, report) = read_reported(
            br"{\rtf1 Area {\mmath{\*\moMathPara{\*\moMath{\mr\msty2 A=\u960\'3f}{\msSup{\msSupPr{\mctrlPr\f0 }}{\me{\mr r}}{\msup{\mr\msty0 2}}}{\md{\mdPr{\mbegChr [}{\mendChr ]}}{\me{\mf{\mnum{\mr a+b}}{\mden{\mr 2}}}}}}}} done\par}",
        )
        .unwrap();
        let p = &doc.main.paras[0];
        assert_eq!(p.text, "Area A = \u{3C0}r2[(a+b)/2] done");
        let two = p.text.find("r2").unwrap() + 1;
        assert!(p.chp_of_char(two).position > 0);
        assert_eq!(report.notes(), [notes::EQUATIONS]);
    }

    #[test]
    fn list_numbers_stay_as_text_and_deleted_text_stays_a_tracked_change() {
        let (doc, report) = read_reported(
            br"{\rtf1{\*\revtbl{Unknown;}{Ada Lovelace;}}{\listtext 1.\tab}First {\deleted\revauthdel1\revdttmdel1803117824 gone }kept\par{\listtext \'b7\tab}Dot\par}",
        )
        .unwrap();
        let p = &doc.main.paras[0];
        assert_eq!(p.text, "1.\tFirst gone kept");
        let gone = quill_core::revision::get(p.chp_of_char(9).rev).unwrap();
        let by = gone.deleted.as_ref().unwrap();
        assert_eq!(by.author, "Ada Lovelace");
        assert_eq!(by.date, crate::rtf::read::undttm(1803117824));
        assert!(by.date.starts_with("20"), "{}", by.date);
        assert_eq!(doc.main.paras[1].text, "•\tDot");
        assert_eq!(report.notes(), [notes::NUMBERING]);
    }

    #[test]
    fn packed_dates_go_both_ways() {
        let d = crate::rtf::write::dttm("2026-10-23T14:05:00Z");
        assert_eq!(undttm(d), "2026-10-23T14:05:00Z");
        assert_eq!(crate::rtf::write::dttm(""), 0);
        assert_eq!(undttm(0), "");
        assert_eq!(undttm(-1), "");
    }

    #[test]
    fn ids_keep_their_full_value_and_measurements_stay_in_range() {
        assert_eq!(parameter("listid", b"697112958"), Some(697112958));
        assert_eq!(parameter("styrsid", b"12810266"), Some(12810266));
        assert_eq!(parameter("fs", b"999999999999"), Some(MAX_PARAMETER_ABS));
        assert_eq!(parameter("li", b"-999999999999"), Some(-MAX_PARAMETER_ABS));
        assert_eq!(parameter("fs", b"-"), None);
    }

    #[test]
    fn refuses_excessive_nesting_and_control_words() {
        let mut deep = b"{\\rtf1 ".to_vec();
        deep.extend(std::iter::repeat_n(b'{', MAX_GROUP_DEPTH));
        deep.extend(std::iter::repeat_n(b'}', MAX_GROUP_DEPTH + 1));
        assert!(read(&deep).is_err());

        let mut word = b"{\\rtf1 \\".to_vec();
        word.extend(std::iter::repeat_n(b'a', MAX_CONTROL_WORD + 1));
        word.push(b'}');
        assert!(read(&word).is_err());
    }

    #[test]
    fn binary_picture_bytes_are_not_parsed_as_rtf_syntax() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&2u32.to_be_bytes());
        png.extend_from_slice(&3u32.to_be_bytes());
        png.extend_from_slice(&[8, 2, 0, 0, 0]);
        png.extend_from_slice(b"{}\\!"); // ignored CRC bytes, deliberately RTF syntax
        let mut rtf = format!("{{\\rtf1{{\\pict\\pngblip\\bin{} ", png.len()).into_bytes();
        rtf.extend_from_slice(&png);
        rtf.extend_from_slice(b"}}");
        let doc = read(&rtf).unwrap();
        assert_eq!(doc.objects.len(), 1);
        assert_eq!(doc.picture(1).unwrap().data.as_ref(), png);
    }

    #[test]
    fn refuses_odd_or_invalid_picture_hex() {
        assert!(read(br"{\rtf1{\pict\pngblip 0}}").is_err());
        assert!(read(br"{\rtf1{\pict\pngblip xx}}").is_err());
    }

    #[test]
    fn damaged_inputs_return_instead_of_panicking() {
        let seed = br"{\rtf1\ansi{\fonttbl{\f0 Times;}}\deff0 Hello \b world\b0\par}";
        for cut in 0..seed.len() {
            let _ = read(&seed[..cut]);
        }
        for at in 0..seed.len() {
            for byte in [0, b'{', b'}', b'\\', 0xff] {
                let mut damaged = seed.to_vec();
                damaged[at] = byte;
                let _ = read(&damaged);
            }
        }
    }
}
