//! Markdown export: what Markdown can say of a document. Headings become
//! `#` lines, bold, italic, strikethrough and links their marks, typed
//! bullets and numbers list items, a ruled left edge a quote, shaded
//! Courier paragraphs a fenced code block, and tables GitHub-style tables.
//! Underline, superscript and subscript use the HTML tags renderers
//! accept. Pictures are copied into a folder beside the file and named where
//! they stand; where there is nowhere to put files (Source view) they go at
//! the end as reference definitions holding the picture itself. A picture the
//! document only links to on the web stays a link.

use super::is_mono;
use super::read::{CODE_SHADING, QUOTE_GAP, QUOTE_INDENT, bare_address, linked_picture_alt};
use crate::export::{Block, base64, blocks_at};
use quill_core::Document;
use quill_core::para::{Mark, Paragraph, chars};
use quill_core::props::{Align, BorderStyle, CharProps, Underline};

/// The Markdown for `doc`, with every tracked change taken as accepted and
/// its pictures held in the text itself.
pub fn write(doc: &Document) -> String {
    write_to(doc, None).0
}

/// A picture saved beside the Markdown: its file name and its bytes.
pub struct PictureFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// The Markdown for `doc` with its pictures named as files in the folder
/// `folder` beside it, and those files. The caller writes the folder.
pub fn write_with_pictures(doc: &Document, folder: &str) -> (String, Vec<PictureFile>) {
    write_to(doc, Some(folder))
}

fn write_to(doc: &Document, folder: Option<&str>) -> (String, Vec<PictureFile>) {
    let accepted;
    let paras = if quill_core::tracking::any_changes(&doc.main.paras) {
        accepted = quill_core::tracking::accepted(&doc.main.paras);
        &accepted
    } else {
        &doc.main.paras
    };
    let mut w = Writer {
        doc,
        images: Vec::new(),
        folder: folder.map(str::to_string),
        paren: false,
        last_number: None,
    };
    let mut chunks: Vec<Chunk> = Vec::new();
    // The depth of the list item last written, for indenting the next.
    let mut item_depth: Option<usize> = None;
    let blocks = blocks_at(paras, 0);
    let mut i = 0;
    while i < blocks.len() {
        match &blocks[i] {
            Block::Table(rows) => {
                let text = w.table(rows, paras);
                chunks.push(Chunk::other(text, 0));
                item_depth = None;
            }
            Block::Paragraph(p) => {
                // The run of code paragraphs this one starts.
                if w.is_code(p) {
                    let mut lines = Vec::new();
                    let quote = quote_depth(p);
                    while let Some(Block::Paragraph(c)) = blocks.get(i)
                        && w.is_code(c)
                    {
                        lines.extend(w.code_lines(c));
                        i += 1;
                    }
                    chunks.push(Chunk::other(fenced(&lines, chunks.is_empty()), quote));
                    item_depth = None;
                    continue;
                }
                if let Some(chunk) = w.paragraph(p, &mut item_depth) {
                    chunks.push(chunk);
                }
            }
        }
        i += 1;
    }
    let mut out = String::new();
    for (k, chunk) in chunks.iter().enumerate() {
        if k > 0 {
            let prev = &chunks[k - 1];
            let tight = match (&prev.kind, &chunk.kind) {
                (
                    Kind::Item {
                        depth: before,
                        ordered: a,
                        ..
                    },
                    Kind::Item {
                        depth,
                        ordered: b,
                        needs_gap,
                    },
                ) => a == b && !(depth > before && *needs_gap),
                _ => false,
            } && prev.quote == chunk.quote;
            let shared = prev.quote.min(chunk.quote);
            out.push('\n');
            if !tight {
                out.push_str(quote_prefix(shared).trim_end());
                out.push('\n');
            }
        }
        let prefix = quote_prefix(chunk.quote);
        for (n, line) in chunk.text.split('\n').enumerate() {
            if n > 0 {
                out.push('\n');
            }
            if line.is_empty() {
                out.push_str(prefix.trim_end());
            } else {
                out.push_str(&prefix);
                out.push_str(line);
            }
        }
    }
    let mut files = Vec::new();
    if folder.is_some() {
        for (n, id) in w.images.iter().enumerate() {
            if let Some(pic) = doc.picture(*id) {
                files.push(PictureFile {
                    name: picture_name(n, pic),
                    bytes: pic.data.to_vec(),
                });
            }
        }
    } else if !w.images.is_empty() {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        for (n, id) in w.images.iter().enumerate() {
            if let Some(pic) = doc.picture(*id) {
                if n > 0 {
                    out.push('\n');
                }
                out.push_str(&format!(
                    "[img{}]: data:{};base64,{}",
                    n + 1,
                    pic.format.mime(),
                    base64(&pic.data)
                ));
            }
        }
    }
    out.push('\n');
    (out, files)
}

/// The file name of the `n`th picture (from 0) written.
fn picture_name(n: usize, pic: &quill_core::Picture) -> String {
    // New bytes get a new name, so preparing a save never changes the
    // pictures referenced by the Markdown file still on disk.
    format!(
        "image{}-{:08x}.{}",
        n + 1,
        crate::zip::crc32(&pic.data),
        pic.format.extension()
    )
}

/// Whether a link's text is just its address, which Markdown readers make a
/// link by themselves, so it is written bare.
fn shows_its_address(text: &str, link: u32) -> bool {
    link != 0
        && bare_address(text).is_some_and(|(taken, _, url)| {
            taken == text.len() && url == quill_core::link::url(link)
        })
}

/// Picture descriptions are link text: brackets in them would end it.
fn escape_alt(alt: &str) -> String {
    alt.replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

fn quote_prefix(depth: u8) -> String {
    "> ".repeat(depth as usize)
}

/// How many levels of quote a paragraph is in: the gap between its rule
/// and its text says.
fn quote_depth(p: &Paragraph) -> u8 {
    let left = p.pap.borders.left;
    if left.style == BorderStyle::None {
        0
    } else {
        (left.space / QUOTE_GAP).clamp(1, 8) as u8
    }
}

enum Kind {
    /// A list item.
    Item {
        depth: usize,
        ordered: bool,
        /// A numbered item that is not 1, which cannot start a list right
        /// after a line of text without a blank line.
        needs_gap: bool,
    },
    Other,
}

struct Chunk {
    text: String,
    quote: u8,
    kind: Kind,
}

impl Chunk {
    fn other(text: String, quote: u8) -> Chunk {
        Chunk {
            text,
            quote,
            kind: Kind::Other,
        }
    }
}

/// Lines of code as a fenced block. A YAML header kept from a file's
/// start goes back without a fence.
fn fenced(lines: &[String], first: bool) -> String {
    if first
        && lines.len() >= 2
        && lines[0] == "---"
        && matches!(lines[lines.len() - 1].as_str(), "---" | "...")
    {
        return lines.join("\n");
    }
    let longest = lines
        .iter()
        .map(|l| {
            let t = l.trim_start();
            t.len() - t.trim_start_matches('`').len()
        })
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    let mut out = format!("{fence}\n");
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&fence);
    out
}

/// The bullet or number a paragraph starts with, and whether it counts.
pub(super) fn list_label(text: &str) -> Option<(bool, String)> {
    let (head, _) = text.split_once('\t')?;
    if matches!(
        head,
        "\u{2022}"
            | "\u{25E6}"
            | "\u{25AA}"
            | "\u{25CB}"
            | "\u{25A0}"
            | "\u{00B7}"
            | "-"
            | "*"
            | "\u{2013}"
    ) {
        return Some((false, "-".into()));
    }
    let digits = head.strip_suffix('.').or_else(|| head.strip_suffix(')'))?;
    (!digits.is_empty() && digits.len() <= 9 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| (true, format!("{digits}.")))
}

/// What can be told of a run's look, apart from what its style gives.
#[derive(Clone, Copy, PartialEq, Default)]
struct Look {
    bold: bool,
    italic: bool,
    strike: bool,
    under: bool,
    sup: bool,
    sub: bool,
    link: u32,
}

#[derive(Clone, Copy, PartialEq)]
enum Mk {
    Link(u32),
    Bold,
    Italic,
    Strike,
    Under,
    Sup,
    Sub,
}

impl Look {
    fn marks(&self) -> Vec<Mk> {
        let mut m = Vec::new();
        if self.link != 0 {
            m.push(Mk::Link(self.link));
        }
        for (on, mk) in [
            (self.bold, Mk::Bold),
            (self.italic, Mk::Italic),
            (self.strike, Mk::Strike),
            (self.under, Mk::Under),
            (self.sup, Mk::Sup),
            (self.sub, Mk::Sub),
        ] {
            if on {
                m.push(mk);
            }
        }
        m
    }
}

fn open_mark(m: Mk) -> &'static str {
    match m {
        Mk::Link(_) => "[",
        Mk::Bold => "**",
        Mk::Italic => "*",
        Mk::Strike => "~~",
        Mk::Under => "<u>",
        Mk::Sup => "<sup>",
        Mk::Sub => "<sub>",
    }
}

fn close_mark(m: Mk) -> String {
    match m {
        Mk::Link(id) => format!("]({})", destination(&quill_core::link::url(id))),
        Mk::Bold => "**".into(),
        Mk::Italic => "*".into(),
        Mk::Strike => "~~".into(),
        Mk::Under => "</u>".into(),
        Mk::Sup => "</sup>".into(),
        Mk::Sub => "</sub>".into(),
    }
}

/// A link's address as Markdown writes it: in angle brackets when it has
/// spaces or parentheses that would end it early.
fn destination(url: &str) -> String {
    let depth_ok = {
        let mut depth = 0i32;
        let mut ok = true;
        for c in url.chars() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    ok &= depth >= 0;
                }
                _ => {}
            }
        }
        ok && depth == 0
    };
    // A backslash before punctuation would be read as an escape.
    let url = url.replace('\\', "\\\\");
    if url.is_empty()
        || url.contains(|c: char| c.is_whitespace() || c.is_control() || c == '<')
        || !depth_ok
    {
        format!("<{}>", url.replace('<', "%3C").replace('>', "%3E"))
    } else {
        url
    }
}

fn code_span(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat(longest + 1);
    let pad = text.starts_with('`')
        || text.ends_with('`')
        || (text.starts_with(' ') && text.ends_with(' ') && !text.trim().is_empty());
    if pad {
        format!("{fence} {text} {fence}")
    } else {
        format!("{fence}{text}{fence}")
    }
}

/// Whether `rest` (after an &) reads as an entity such as `amp;` or `#9;`.
fn entity_like(rest: &[char]) -> bool {
    let body = rest.strip_prefix(&['#']).unwrap_or(rest);
    let name = body
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric())
        .count();
    name > 0 && body.get(name) == Some(&';')
}

/// Text of one word with the characters Markdown would take for marks
/// escaped. `line_start` also escapes what would make a line a heading,
/// quote, list or rule.
fn escape(word: &str, line_start: bool, table: bool) -> String {
    let cs: Vec<char> = word.chars().collect();
    let mut out = String::with_capacity(word.len());
    for (i, &c) in cs.iter().enumerate() {
        let prev = i.checked_sub(1).map(|k| cs[k]);
        let next = cs.get(i + 1).copied();
        match c {
            '\\' | '`' | '*' | '[' | ']' | '<' | '~' => {
                out.push('\\');
                out.push(c);
            }
            '_' if prev.is_some_and(char::is_alphanumeric)
                && next.is_some_and(char::is_alphanumeric) =>
            {
                out.push(c)
            }
            '_' => out.push_str("\\_"),
            '|' if table => out.push_str("\\|"),
            '&' if entity_like(&cs[i + 1..]) => out.push_str("\\&"),
            '#' | '>' | '+' | '-' | '=' if line_start && i == 0 => {
                out.push('\\');
                out.push(c);
            }
            '.' | ')'
                if line_start && i > 0 && i <= 9 && cs[..i].iter().all(char::is_ascii_digit) =>
            {
                out.push('\\');
                out.push(c);
            }
            '\u{00A0}' => out.push_str("&nbsp;"),
            '\u{00AD}' => out.push_str("&shy;"),
            '\u{2011}' => out.push_str("&#8209;"),
            c => out.push(c),
        }
    }
    out
}

/// Inline text being written: marks open in `stack`, and white space held
/// back until the next word shows whether a mark closes before it (a mark
/// can't end after a space).
struct Inline {
    s: String,
    ws: String,
    stack: Vec<Mk>,
    table: bool,
    heading: bool,
}

impl Inline {
    fn at_line_start(&self) -> bool {
        self.ws.is_empty() && (self.s.is_empty() || self.s.ends_with('\n'))
    }

    fn flush_ws(&mut self) {
        if self.s.is_empty() || self.s.ends_with('\n') {
            // Markdown drops a line's leading white space; a tab is kept
            // as a character.
            self.ws.retain(|c| c == '\t');
            self.ws = self.ws.replace('\t', "&#9;");
        }
        let ws = std::mem::take(&mut self.ws);
        self.s.push_str(&ws);
    }

    /// Change the open marks to `want`, closing any that no longer apply
    /// (and those opened after them) before the held-back space.
    fn switch(&mut self, want: &[Mk]) {
        if let Some(bad) = self.stack.iter().position(|m| !want.contains(m)) {
            while self.stack.len() > bad {
                let m = self.stack.pop().expect("longer than bad");
                self.s.push_str(&close_mark(m));
            }
        }
        self.flush_ws();
        for &m in want {
            if !self.stack.contains(&m) {
                self.s.push_str(open_mark(m));
                self.stack.push(m);
            }
        }
    }

    fn word(&mut self, text: &str, look: Look, mono: bool) {
        let want = look.marks();
        let line_start = self.at_line_start() && !self.table;
        self.switch(&want);
        if mono {
            self.s.push_str(&code_span(text));
        } else {
            self.s.push_str(&escape(
                text,
                line_start && self.stack.is_empty(),
                self.table,
            ));
        }
    }

    fn space(&mut self, c: char) {
        if self.s.is_empty() && self.stack.is_empty() && !self.table {
            // Leading space means nothing in Markdown.
            if c != '\t' {
                return;
            }
        }
        self.ws.push(c);
    }

    fn line_break(&mut self) {
        // The marks end before a break and start again after it: a mark
        // can't close right after the break's own characters.
        while let Some(m) = self.stack.pop() {
            self.s.push_str(&close_mark(m));
        }
        if self.heading {
            self.ws.push(' ');
        } else if self.table {
            self.flush_ws();
            self.s.push_str("<br>");
        } else if !self.s.is_empty() && !self.s.ends_with('\n') {
            self.ws.clear();
            self.s.push_str("\\\n");
        }
    }

    fn finish(mut self) -> String {
        while let Some(m) = self.stack.pop() {
            self.s.push_str(&close_mark(m));
        }
        let mut s = self.s;
        while s.ends_with("\\\n") || s.ends_with("<br>") && self.table {
            let cut = if s.ends_with("<br>") { 4 } else { 2 };
            s.truncate(s.len() - cut);
        }
        s
    }
}

struct Writer<'a> {
    doc: &'a Document,
    /// Pictures written so far, by object id; `[imgN]` is its place + 1.
    images: Vec<u32>,
    /// The folder the pictures are saved in, beside the Markdown, if they
    /// are saved as files.
    folder: Option<String>,
    /// Numbered lists are written with `)` instead of `.` after one that
    /// restarts at a lower number, since only a change of mark starts a
    /// new list in Markdown.
    paren: bool,
    /// The depth and number of the numbered item just written.
    last_number: Option<(usize, u64)>,
}

impl Writer<'_> {
    fn mono(&self, chp: &CharProps) -> bool {
        is_mono(self.doc.font_name(chp.font))
    }

    fn is_code(&self, p: &Paragraph) -> bool {
        p.pap.shading >= CODE_SHADING && p.mark == Mark::Paragraph && self.mono(&p.chp_at(0))
    }

    fn code_lines(&self, p: &Paragraph) -> Vec<String> {
        p.text
            .replace(chars::LINE_BREAK, "\n")
            .split('\n')
            .map(str::to_string)
            .collect()
    }

    /// The paragraph's text from byte `from` as Markdown, its look judged
    /// against `base`.
    fn inline(
        &mut self,
        p: &Paragraph,
        from: usize,
        base: &CharProps,
        table: bool,
        heading: bool,
    ) -> String {
        let mut out = Inline {
            s: String::new(),
            ws: String::new(),
            stack: Vec::new(),
            table,
            heading,
        };
        let mut start = 0;
        for run in &p.runs {
            let end = start + run.len;
            let chp = run.chp;
            let from_here = from.max(start);
            if from_here >= end {
                start = end;
                continue;
            }
            let look = Look {
                bold: chp.bold && !base.bold,
                italic: chp.italic && !base.italic,
                strike: chp.strike && !base.strike,
                under: chp.underline != Underline::None
                    && base.underline == Underline::None
                    && chp.link == 0,
                sup: chp.position > base.position && chp.object == 0,
                sub: chp.position < base.position && chp.object == 0,
                link: if shows_its_address(&p.text[from_here..end], chp.link) {
                    0
                } else {
                    chp.link
                },
            };
            // A picture on the web, kept as a link: written as the picture
            // reference it came from.
            if chp.link != 0
                && let Some(alt) = linked_picture_alt(&p.text[from_here..end])
            {
                out.switch(&Look { link: 0, ..look }.marks());
                out.s.push_str(&format!(
                    "![{}]({})",
                    escape_alt(alt),
                    destination(&quill_core::link::url(chp.link))
                ));
                start = end;
                continue;
            }
            let mono = self.mono(&chp) && !self.mono(base);
            let mut word = String::new();
            let flush = |out: &mut Inline, word: &mut String| {
                if !word.is_empty() {
                    out.word(word, look, mono);
                    word.clear();
                }
            };
            for c in p.text[from_here..end].chars() {
                match c {
                    chars::LINE_BREAK => {
                        flush(&mut out, &mut word);
                        out.line_break();
                    }
                    chars::OBJECT => {
                        flush(&mut out, &mut word);
                        if self.doc.picture(chp.object).is_some() {
                            let n = match self.images.iter().position(|&id| id == chp.object) {
                                Some(n) => n,
                                None => {
                                    self.images.push(chp.object);
                                    self.images.len() - 1
                                }
                            };
                            out.switch(&look.marks());
                            match (&self.folder, self.doc.picture(chp.object)) {
                                (Some(folder), Some(pic)) => {
                                    let path = format!("{folder}/{}", picture_name(n, pic));
                                    out.s
                                        .push_str(&format!("![Picture]({})", destination(&path)));
                                }
                                _ => out.s.push_str(&format!("![Picture][img{}]", n + 1)),
                            }
                        }
                    }
                    chars::PAGE_BREAK
                    | chars::FIELD_PAGE
                    | chars::FIELD_DATE
                    | chars::FIELD_TIME
                    | chars::FOOTNOTE
                    | chars::ANNOTATION => flush(&mut out, &mut word),
                    c if c.is_whitespace() && c != chars::NONBREAKING_SPACE && !mono => {
                        flush(&mut out, &mut word);
                        out.space(c);
                    }
                    c if c.is_control() => {}
                    c => word.push(c),
                }
            }
            flush(&mut out, &mut word);
            start = end;
        }
        out.finish().trim_end().to_string()
    }

    /// One paragraph as a chunk, or None when it holds nothing Markdown
    /// can show.
    fn paragraph(&mut self, p: &Paragraph, item_depth: &mut Option<usize>) -> Option<Chunk> {
        let doc = self.doc;
        let style = doc.styles.get(p.pap.style);
        let quote = quote_depth(p);
        let rule = p.text.trim().is_empty()
            && p.pap.borders.bottom.style != BorderStyle::None
            && p.pap.borders.top.style == BorderStyle::None;
        if rule {
            *item_depth = None;
            return Some(Chunk::other("---".into(), quote));
        }
        if let Some(level) = doc.styles.heading_level(p.pap.style) {
            let text = self.inline(p, 0, &style.chp, false, true);
            *item_depth = None;
            if text.is_empty() {
                return None;
            }
            return Some(Chunk::other(
                format!("{} {text}", "#".repeat(level.min(6) as usize)),
                quote,
            ));
        }
        let base = style.chp;
        let quote_indent = quote as i32 * QUOTE_INDENT;
        if let Some((ordered, label)) = list_label(&p.text) {
            let from = p.text.find('\t').map_or(0, |t| t + 1);
            let start = p.pap.left + p.pap.first - quote_indent;
            let wanted = ((start.max(0) + 180) / 360) as usize;
            let depth = match *item_depth {
                Some(prev) => wanted.min(prev + 1),
                None => 0,
            };
            *item_depth = Some(depth);
            let mut label = label;
            if ordered {
                let number: u64 = label.trim_end_matches('.').parse().unwrap_or(1);
                if self
                    .last_number
                    .is_some_and(|(d, n)| d == depth && number <= n)
                {
                    self.paren = !self.paren;
                }
                self.last_number = Some((depth, number));
                if self.paren {
                    label = format!("{number})");
                }
            } else {
                self.last_number = None;
            }
            let text = self.inline(p, from, &base, false, false);
            // A bullet with nothing after it can't start a list inside an
            // item in Markdown, and says nothing.
            if text.is_empty() {
                return None;
            }
            let indent = " ".repeat(4 * depth);
            let hang = " ".repeat(4 * depth + 4);
            let mut lines = text.split('\n');
            let mut out = format!("{indent}{label} {}", lines.next().unwrap_or(""));
            for line in lines {
                out.push('\n');
                out.push_str(&hang);
                out.push_str(line);
            }
            return Some(Chunk {
                text: out,
                quote,
                kind: Kind::Item {
                    depth,
                    ordered,
                    needs_gap: ordered && !(label.starts_with("1.") || label.starts_with("1)")),
                },
            });
        }
        self.last_number = None;
        let text = self.inline(p, 0, &base, false, false);
        if text.is_empty() {
            return None;
        }
        // A paragraph set in under the list item before it belongs to it.
        if let Some(depth) = *item_depth {
            let wanted = (p.pap.left - quote_indent + 180) / 360;
            if wanted as usize == depth + 1 && p.pap.first == 0 {
                let hang = " ".repeat(4 * depth + 4);
                let text = text
                    .split('\n')
                    .map(|l| format!("{hang}{l}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                return Some(Chunk::other(text, quote));
            }
        }
        *item_depth = None;
        Some(Chunk::other(text, quote))
    }

    fn table(&mut self, rows: &[quill_core::table::Row], paras: &[Paragraph]) -> String {
        let columns = rows.iter().map(|r| r.cells.len()).max().unwrap_or(0).max(1);
        let mut lines = Vec::new();
        let mut align = vec![Align::Left; columns];
        for (r, row) in rows.iter().enumerate() {
            let mut cells = Vec::new();
            for (c, &(a, b)) in row.cells.iter().enumerate() {
                let mut parts = Vec::new();
                for (k, p) in paras[a..=b].iter().enumerate() {
                    if p.mark == Mark::RowEnd {
                        continue;
                    }
                    if r == 0 && k == 0 {
                        align[c] = p.pap.align;
                    }
                    let mut base = self.doc.styles.get(p.pap.style).chp;
                    // A header row is bold already.
                    base.bold |= r == 0;
                    let text = self.inline(p, 0, &base, true, false);
                    if !text.is_empty() {
                        parts.push(text);
                    }
                }
                cells.push(parts.join("<br>"));
            }
            cells.resize(columns, String::new());
            lines.push(format!("| {} |", cells.join(" | ")));
            if r == 0 {
                let rule: Vec<&str> = align
                    .iter()
                    .map(|a| match a {
                        Align::Center => ":---:",
                        Align::Right => "---:",
                        _ => "---",
                    })
                    .collect();
                lines.push(format!("| {} |", rule.join(" | ")));
            }
        }
        lines.join("\n")
    }
}
