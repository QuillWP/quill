//! Markdown import: CommonMark's headings, paragraphs, emphasis, code,
//! quotes, lists and links, plus GitHub's tables and strikethrough. Raw
//! HTML is kept as text, apart from the few inline tags Quill's own export
//! writes (<u>, <sup>, <sub> and <br>).
//!
//! Quill has no list objects, so a list item is a paragraph that starts with
//! its bullet or number and a tab, with a hanging indent, the same as
//! Insert > Bullet makes. A quote is a paragraph with a rule down its left
//! edge, and a code block is a run of shaded Courier paragraphs.

use super::MONO;
use crate::report::{self as notes, ImportReport};
use quill_core::para::{Mark, Paragraph, chars};
use quill_core::props::{
    Align, BorderLine, BorderStyle, Borders, CellProps, CharProps, ParaProps, RowHeight, RowProps,
    TABLE_GAP, Underline,
};
use quill_core::{Document, Object};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Quotes and lists nested deeper than this are read as plain text.
const MAX_DEPTH: usize = 24;
/// The most cells a row can have, as in the other readers.
const MAX_COLUMNS: usize = 255;
/// The largest picture file read from beside the Markdown file.
const MAX_PICTURE: u64 = 64 << 20;

/// Twips a list level and a quote level indent by.
pub(super) const LIST_INDENT: i32 = 360;
pub(super) const QUOTE_INDENT: i32 = 720;
/// Twips between a quote's rule and its text, per level of quoting. The
/// writer reads the depth back from this.
pub(super) const QUOTE_GAP: i32 = 80;
/// Shading (tenths of a percent) that marks a paragraph as code.
pub(super) const CODE_SHADING: u16 = 100;

enum Block {
    Para(String),
    Heading(u8, String),
    Code(Vec<String>),
    Quote(Vec<Block>),
    List {
        ordered: bool,
        start: i32,
        items: Vec<Vec<Block>>,
    },
    Rule,
    Table {
        align: Vec<Align>,
        head: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Html(Vec<String>),
}

/// Columns of leading white space (a tab goes to the next multiple of 4)
/// and the byte offset where the text starts.
fn indent_of(line: &str) -> (usize, usize) {
    let mut col = 0;
    for (i, c) in line.char_indices() {
        match c {
            ' ' => col += 1,
            '\t' => col += 4 - col % 4,
            _ => return (col, i),
        }
    }
    (col, line.len())
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// `line` without its first `n` columns of indentation.
fn strip_indent(line: &str, n: usize) -> String {
    let mut col = 0;
    for (i, c) in line.char_indices() {
        if col >= n {
            return line[i..].to_string();
        }
        match c {
            ' ' => col += 1,
            '\t' => {
                let width = 4 - col % 4;
                if col + width > n {
                    return " ".repeat(col + width - n) + &line[i + 1..];
                }
                col += width;
            }
            _ => return line[i..].to_string(),
        }
    }
    String::new()
}

/// A code fence's character, length and indentation.
fn fence_open(line: &str) -> Option<(char, usize, usize)> {
    let (indent, at) = indent_of(line);
    if indent > 3 {
        return None;
    }
    let rest = &line[at..];
    let c = rest.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let n = rest.chars().take_while(|&x| x == c).count();
    if n < 3 || (c == '`' && rest[n..].contains('`')) {
        return None;
    }
    Some((c, n, indent))
}

fn fence_closes(line: &str, c: char, len: usize) -> bool {
    let (indent, at) = indent_of(line);
    let rest = line[at..].trim_end();
    indent <= 3 && rest.len() >= len && rest.chars().all(|x| x == c)
}

fn atx(line: &str) -> Option<(u8, String)> {
    let (indent, at) = indent_of(line);
    if indent > 3 {
        return None;
    }
    let rest = &line[at..];
    let n = rest.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&n) {
        return None;
    }
    let after = &rest[n..];
    if !after.is_empty() && !after.starts_with([' ', '\t']) {
        return None;
    }
    let mut text = after.trim();
    let stripped = text.trim_end_matches('#');
    if stripped.len() < text.len() && (stripped.is_empty() || stripped.ends_with([' ', '\t'])) {
        text = stripped.trim_end();
    }
    Some((n as u8, text.to_string()))
}

fn is_rule(line: &str) -> bool {
    let (indent, at) = indent_of(line);
    let text = line[at..].trim_end();
    let Some(c) = text.chars().next().filter(|c| matches!(c, '-' | '*' | '_')) else {
        return false;
    };
    indent <= 3
        && text.chars().all(|x| x == c || x == ' ' || x == '\t')
        && text.chars().filter(|&x| x == c).count() >= 3
}

/// 1 or 2 for a line of = or - under a paragraph (a setext heading).
fn setext(line: &str) -> Option<u8> {
    let (indent, at) = indent_of(line);
    let text = line[at..].trim_end();
    let c = text.chars().next()?;
    (indent <= 3 && matches!(c, '=' | '-') && text.chars().all(|x| x == c)).then_some(if c == '=' {
        1
    } else {
        2
    })
}

fn quote_strip(line: &str) -> Option<String> {
    let (indent, at) = indent_of(line);
    if indent > 3 {
        return None;
    }
    let rest = line[at..].strip_prefix('>')?;
    Some(rest.strip_prefix([' ', '\t']).unwrap_or(rest).to_string())
}

struct Marker {
    ordered: bool,
    start: i32,
    /// The bullet character, or the `.` or `)` after a number.
    ch: char,
    /// Columns from the line start to where the item's text begins.
    content: usize,
    /// The item's first line, after its marker.
    first: String,
}

fn list_marker(line: &str) -> Option<Marker> {
    let (indent, at) = indent_of(line);
    if indent > 3 {
        return None;
    }
    let rest = &line[at..];
    let (ordered, start, ch, width) = match rest.chars().next()? {
        c @ ('-' | '+' | '*') => (false, 0, c, 1),
        _ => {
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            if digits == 0 || digits > 9 {
                return None;
            }
            let delim = rest[digits..]
                .chars()
                .next()
                .filter(|c| matches!(c, '.' | ')'))?;
            (true, rest[..digits].parse().ok()?, delim, digits + 1)
        }
    };
    let after = &rest[width..];
    if !(after.is_empty() || after.starts_with([' ', '\t'])) {
        return None;
    }
    let (spaces, text_at) = indent_of(after);
    let blank = after.trim().is_empty();
    let gap = if blank || spaces >= 5 { 1 } else { spaces };
    let first = if blank {
        String::new()
    } else if spaces >= 5 {
        after[1..].to_string()
    } else {
        after[text_at..].to_string()
    };
    Some(Marker {
        ordered,
        start,
        ch,
        content: indent + width + gap,
        first,
    })
}

const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "body",
    "center",
    "dd",
    "details",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "html",
    "iframe",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "ul",
    "script",
    "style",
];

fn html_start(line: &str) -> bool {
    let (indent, at) = indent_of(line);
    let rest = &line[at..];
    if indent > 3 || !rest.starts_with('<') {
        return false;
    }
    if rest.starts_with("<!") || rest.starts_with("<?") {
        return true;
    }
    let name: String = rest[1..]
        .strip_prefix('/')
        .unwrap_or(&rest[1..])
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    BLOCK_TAGS.contains(&name.as_str())
}

/// Whether `line` could not continue a paragraph.
fn interrupts(line: &str) -> bool {
    if atx(line).is_some()
        || fence_open(line).is_some()
        || is_rule(line)
        || quote_strip(line).is_some()
        || html_start(line)
    {
        return true;
    }
    // Only a list that starts with 1, and an item with text in it, can
    // break into a paragraph.
    list_marker(line).is_some_and(|m| !m.first.trim().is_empty() && (!m.ordered || m.start == 1))
}

/// The cells of a table row, with the `\|` escapes left for the inline
/// reader.
fn split_row(line: &str) -> Vec<String> {
    let text = line.trim();
    let ended = text.ends_with('|') && !text.ends_with("\\|");
    let text = text.strip_prefix('|').unwrap_or(text);
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut it = text.chars();
    while let Some(c) = it.next() {
        match c {
            '\\' => {
                cur.push(c);
                if let Some(n) = it.next() {
                    cur.push(n);
                }
            }
            '|' => {
                cells.push(cur.trim().to_string());
                cur.clear();
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() || !ended {
        cells.push(cur.trim().to_string());
    }
    cells
}

/// The columns' alignments if `line` is a table's row of dashes.
fn table_delimiter(line: &str) -> Option<Vec<Align>> {
    if !line.contains('|') || !line.contains('-') || indent_of(line).0 > 3 {
        return None;
    }
    split_row(line)
        .iter()
        .map(|cell| {
            let left = cell.starts_with(':');
            let right = cell.ends_with(':');
            let dashes = cell.trim_matches(':');
            (!dashes.is_empty() && dashes.bytes().all(|b| b == b'-')).then_some(
                match (left, right) {
                    (true, true) => Align::Center,
                    (false, true) => Align::Right,
                    _ => Align::Left,
                },
            )
        })
        .collect()
}

fn blocks(lines: &[String], depth: usize) -> Vec<Block> {
    let mut out = Vec::new();
    let n = lines.len();
    let mut i = 0;
    while i < n {
        let line = &lines[i];
        if is_blank(line) {
            i += 1;
            continue;
        }
        if depth > MAX_DEPTH {
            let text: Vec<&str> = lines[i..].iter().map(|l| l.trim()).collect();
            out.push(Block::Para(text.join("\n")));
            break;
        }
        if indent_of(line).0 >= 4 {
            let mut code = Vec::new();
            let mut j = i;
            while j < n && (is_blank(&lines[j]) || indent_of(&lines[j]).0 >= 4) {
                code.push(strip_indent(&lines[j], 4));
                j += 1;
            }
            while code.last().is_some_and(|l| l.trim().is_empty()) {
                code.pop();
            }
            out.push(Block::Code(code));
            i = j;
            continue;
        }
        if let Some((c, len, indent)) = fence_open(line) {
            let mut code = Vec::new();
            let mut j = i + 1;
            while j < n && !fence_closes(&lines[j], c, len) {
                code.push(strip_indent(&lines[j], indent));
                j += 1;
            }
            out.push(Block::Code(code));
            i = (j + 1).min(n);
            continue;
        }
        if let Some((level, text)) = atx(line) {
            out.push(Block::Heading(level, text));
            i += 1;
            continue;
        }
        if is_rule(line) {
            out.push(Block::Rule);
            i += 1;
            continue;
        }
        if quote_strip(line).is_some() {
            let mut inner: Vec<String> = Vec::new();
            let mut j = i;
            while j < n {
                if let Some(s) = quote_strip(&lines[j]) {
                    inner.push(s);
                } else if !is_blank(&lines[j])
                    && inner.last().is_some_and(|l| {
                        !is_blank(l)
                            && indent_of(l).0 < 4
                            && atx(l).is_none()
                            && !is_rule(l)
                            && fence_open(l).is_none()
                    })
                    && !interrupts(&lines[j])
                {
                    inner.push(lines[j].trim_start().to_string());
                } else {
                    break;
                }
                j += 1;
            }
            out.push(Block::Quote(blocks(&inner, depth + 1)));
            i = j;
            continue;
        }
        if let Some(first) = list_marker(line) {
            let (ordered, start, ch) = (first.ordered, first.start, first.ch);
            let mut items = Vec::new();
            let mut j = i;
            while let Some(mk) = list_marker(&lines[j]) {
                let mut item = vec![mk.first.clone()];
                j += 1;
                let mut blanks = 0;
                while j < n {
                    let l = &lines[j];
                    if is_blank(l) {
                        blanks += 1;
                        j += 1;
                        continue;
                    }
                    if indent_of(l).0 >= mk.content {
                        item.extend(std::iter::repeat_n(String::new(), blanks));
                        blanks = 0;
                        item.push(strip_indent(l, mk.content));
                    } else if blanks == 0
                        && item.last().is_some_and(|p| {
                            !is_blank(p) && fence_open(p).is_none() && indent_of(p).0 < 4
                        })
                        && !interrupts(l)
                        && list_marker(l).is_none()
                    {
                        item.push(l.trim_start().to_string());
                    } else {
                        break;
                    }
                    j += 1;
                }
                items.push(blocks(&item, depth + 1));
                let same = j < n
                    && !is_rule(&lines[j])
                    && list_marker(&lines[j]).is_some_and(|m| m.ordered == ordered && m.ch == ch);
                if !same {
                    break;
                }
            }
            out.push(Block::List {
                ordered,
                start,
                items,
            });
            i = j;
            continue;
        }
        if html_start(line) {
            let comment = line.trim_start().starts_with("<!--");
            let mut html = Vec::new();
            let mut j = i;
            while j < n && (comment || !is_blank(&lines[j])) {
                html.push(lines[j].trim().to_string());
                j += 1;
                if comment && html.last().is_some_and(|l| l.contains("-->")) {
                    break;
                }
            }
            out.push(Block::Html(html));
            i = j;
            continue;
        }
        if i + 1 < n
            && line.contains('|')
            && let Some(align) = table_delimiter(&lines[i + 1])
            && split_row(line).len() == align.len()
        {
            let mut head = split_row(line);
            head.truncate(MAX_COLUMNS);
            let columns = head.len();
            let mut rows = Vec::new();
            let mut j = i + 2;
            while j < n && !is_blank(&lines[j]) && !interrupts(&lines[j]) {
                let mut row = split_row(&lines[j]);
                row.resize(columns, String::new());
                rows.push(row);
                j += 1;
            }
            let mut align = align;
            align.truncate(columns);
            out.push(Block::Table { align, head, rows });
            i = j;
            continue;
        }
        let mut text = vec![line.trim_start()];
        let mut j = i + 1;
        let mut heading = None;
        while j < n && !is_blank(&lines[j]) {
            if let Some(level) = setext(&lines[j]) {
                heading = Some(level);
                j += 1;
                break;
            }
            if interrupts(&lines[j]) {
                break;
            }
            text.push(lines[j].trim_start());
            j += 1;
        }
        let text = text.join("\n");
        out.push(match heading {
            Some(level) => Block::Heading(level, text.trim().to_string()),
            None => Block::Para(text),
        });
        i = j;
    }
    out
}

/// A reference's label as links match it: lower case, one space between
/// words.
fn label_key(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// `[label]: destination "title"` on one line.
fn definition(line: &str) -> Option<(String, String)> {
    let (indent, at) = indent_of(line);
    let rest = line[at..].strip_prefix('[')?;
    if indent > 3 {
        return None;
    }
    let (label, after) = rest.split_once("]:")?;
    if label.is_empty() || label.starts_with('^') || label.contains(['[', ']']) {
        return None;
    }
    let after = after.trim();
    let dest = if let Some(angled) = after.strip_prefix('<') {
        angled.split_once('>')?.0
    } else {
        after.split_whitespace().next()?
    };
    Some((label_key(label), dest.to_string()))
}

#[derive(Clone, Copy, Default, PartialEq)]
struct Flags {
    bold: bool,
    italic: bool,
    strike: bool,
    underline: bool,
    sup: bool,
    sub: bool,
    code: bool,
    link: u32,
}

struct Seg {
    text: String,
    f: Flags,
    object: u32,
}

impl Seg {
    fn plain(text: String) -> Seg {
        Seg {
            text,
            f: Flags::default(),
            object: 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Tag {
    Bold,
    Italic,
    Strike,
    Underline,
    Sup,
    Sub,
}

enum Tok {
    Seg(Seg),
    Delim {
        ch: char,
        n: usize,
        open: bool,
        close: bool,
    },
    Tag {
        tag: Tag,
        open: bool,
        raw: String,
    },
}

fn is_punct(c: char) -> bool {
    c.is_ascii_punctuation() || (!c.is_alphanumeric() && !c.is_whitespace() && !c.is_control())
}

fn entity(text: &str) -> Option<(char, usize)> {
    let body = text.strip_prefix('&')?;
    let end = body.find(';').filter(|&e| e <= 10)?;
    let name = &body[..end];
    let c = if let Some(num) = name.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse().ok()?,
        };
        char::from_u32(code).filter(|&c| c != '\0')?
    } else {
        match name {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            "nbsp" => chars::NONBREAKING_SPACE,
            "shy" => chars::OPTIONAL_HYPHEN,
            "copy" => '\u{A9}',
            "reg" => '\u{AE}',
            "trade" => '\u{2122}',
            "mdash" => '\u{2014}',
            "ndash" => '\u{2013}',
            "hellip" => '\u{2026}',
            "lsquo" => '\u{2018}',
            "rsquo" => '\u{2019}',
            "ldquo" => '\u{201C}',
            "rdquo" => '\u{201D}',
            _ => return None,
        }
    };
    Some((c, end + 2))
}

/// Everything in `label` after backslash escapes are taken out.
fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match (c, it.peek()) {
            ('\\', Some(n)) if n.is_ascii_punctuation() => {
                out.push(*n);
                it.next();
            }
            _ => out.push(c),
        }
    }
    out
}

/// The `]` that closes the `[` at `open`, skipping nested brackets, escapes
/// and code spans.
fn closing_bracket(s: &str, open: usize) -> Option<usize> {
    let b = s.as_bytes();
    let mut depth = 0;
    let mut i = open;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            b'`' => {
                let run = b[i..].iter().take_while(|&&c| c == b'`').count();
                if let Some(end) = code_end(s, i + run, run) {
                    i = end - 1;
                } else {
                    i += run - 1;
                }
            }
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Where the backtick run of exactly `run` characters that closes a code
/// span starting at `from` ends.
fn code_end(s: &str, from: usize, run: usize) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = from;
    while i < b.len() {
        if b[i] == b'`' {
            let n = b[i..].iter().take_while(|&&c| c == b'`').count();
            if n == run {
                return Some(i + n);
            }
            i += n;
        } else {
            i += 1;
        }
    }
    None
}

/// A link's destination and the byte just past the `)` that ends it.
fn inline_destination(s: &str, at: usize) -> Option<(String, usize)> {
    let b = s.as_bytes();
    let skip = |mut i: usize| {
        while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n') {
            i += 1;
        }
        i
    };
    let mut i = skip(at);
    let dest;
    if b.get(i) == Some(&b'<') {
        let end = s[i + 1..].find('>')? + i + 1;
        dest = s[i + 1..end].to_string();
        i = end + 1;
    } else {
        let start = i;
        let mut depth = 0;
        while i < b.len() {
            match b[i] {
                b'\\' => i += 1,
                b'(' => depth += 1,
                b')' if depth == 0 => break,
                b')' => depth -= 1,
                b' ' | b'\t' | b'\n' => break,
                _ => {}
            }
            i += 1;
        }
        dest = s.get(start..i.min(b.len()))?.to_string();
    }
    i = skip(i);
    if let Some(&q) = b.get(i).filter(|&&q| matches!(q, b'"' | b'\'' | b'(')) {
        let close = if q == b'(' { b')' } else { q };
        let mut k = i + 1;
        while k < b.len() && b[k] != close {
            if b[k] == b'\\' {
                k += 1;
            }
            k += 1;
        }
        if k >= b.len() {
            return None;
        }
        i = skip(k + 1);
    }
    (b.get(i) == Some(&b')')).then(|| (unescape(&dest), i + 1))
}

/// Whether a picture's address is on the web or another server, not a file.
fn is_remote(src: &str) -> bool {
    let src = src.trim();
    src.contains("://") && !src.starts_with("file://")
}

/// What a picture linked from the web shows in the text: `[image: alt]`.
pub(super) fn linked_picture_text(alt: &str) -> String {
    if alt.is_empty() {
        "[image]".into()
    } else {
        format!("[image: {alt}]")
    }
}

/// The description in text made by `linked_picture_text`.
pub(super) fn linked_picture_alt(text: &str) -> Option<&str> {
    if text == "[image]" {
        return Some("");
    }
    text.strip_prefix("[image: ")?.strip_suffix(']')
}

/// A web address that starts `text`, as GitHub finds them: `http://`,
/// `https://` or `www.`, up to a space or `<`, without the punctuation or
/// closing bracket that ends the sentence. Returns how many bytes of `text`
/// it takes, what to show and where it points. A backslash before
/// punctuation is an escape, as the writer puts in front of "_" or "~".
pub(super) fn bare_address(text: &str) -> Option<(usize, String, String)> {
    let lower = |n: usize| text.get(..n).map(str::to_ascii_lowercase);
    let www = lower(4).is_some_and(|p| p == "www.");
    let https = lower(8).is_some_and(|p| p == "https://");
    let http = lower(7).is_some_and(|p| p == "http://");
    if !www && !http && !https {
        return None;
    }
    let mut end = text
        .find(|c: char| c.is_whitespace() || c == '<')
        .unwrap_or(text.len());
    loop {
        let shown = &text[..end];
        let last = shown.chars().next_back()?;
        let unbalanced = last == ')' && shown.matches(')').count() > shown.matches('(').count();
        if "?!.,:;*_~'\"".contains(last) || unbalanced {
            end -= last.len_utf8();
        } else {
            break;
        }
    }
    let raw = &text[..end];
    let shown = unescape(raw);
    // Something must follow the scheme or the "www.".
    let after = if www {
        4
    } else if https {
        8
    } else {
        7
    };
    if shown.len() <= after || !shown[after..].contains(|c: char| c.is_alphanumeric()) {
        return None;
    }
    let url = if www {
        format!("http://{shown}")
    } else {
        shown.clone()
    };
    Some((end, shown, url))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// An inline tag Quill's export or common Markdown uses for formatting.
fn known_tag(inner: &str) -> Option<(Option<Tag>, bool)> {
    let (closing, name) = match inner.strip_prefix('/') {
        Some(n) => (true, n),
        None => (false, inner),
    };
    let name = name.trim_end_matches('/').trim().to_ascii_lowercase();
    let tag = match name.as_str() {
        "u" | "ins" => Tag::Underline,
        "sup" => Tag::Sup,
        "sub" => Tag::Sub,
        "b" | "strong" => Tag::Bold,
        "i" | "em" => Tag::Italic,
        "s" | "del" | "strike" => Tag::Strike,
        "br" if !closing => return Some((None, true)),
        _ => return None,
    };
    Some((Some(tag), !closing))
}

struct Reader {
    doc: Document,
    base: Option<PathBuf>,
    report: ImportReport,
    refs: HashMap<String, String>,
    images: HashMap<String, Option<u32>>,
    /// Inside a link's label, where a web address is not made a link again.
    in_link: usize,
}

#[derive(Clone, Copy, Default)]
struct Ctx {
    quote: u8,
    list: u8,
}

impl Reader {
    fn new(base: Option<&Path>) -> Reader {
        let mut doc = Document::new();
        let font = doc.styles.styles[0].chp.font;
        doc.styles.styles[0].pap.after = 120;
        // Headings by size and weight alone, in the body font.
        for (level, size) in [40u16, 32, 28, 26, 24, 24].into_iter().enumerate() {
            if let Some(id) = doc.styles.find(&format!("heading {}", level + 1)) {
                let style = &mut doc.styles.styles[id as usize];
                style.chp.font = font;
                style.chp.size = size;
                style.chp.bold = true;
                style.chp.italic = false;
                style.chp.underline = Underline::None;
                style.pap.left = 0;
                style.pap.before = 240;
                style.pap.after = 120;
            }
        }
        Reader {
            doc,
            base: base.map(Path::to_path_buf),
            report: ImportReport::default(),
            refs: HashMap::new(),
            images: HashMap::new(),
            in_link: 0,
        }
    }

    // ---- inline text ----

    fn inline(&mut self, text: &str, depth: usize) -> Vec<Seg> {
        let mut toks = self.tokens(text.trim(), depth);
        emphasis(&mut toks);
        let mut open: Vec<(usize, Tag)> = Vec::new();
        for i in 0..toks.len() {
            let (tag, is_open) = match &toks[i] {
                Tok::Tag { tag, open, .. } => (*tag, *open),
                _ => continue,
            };
            if is_open {
                open.push((i, tag));
            } else if let Some(at) = open.iter().rposition(|&(_, t)| t == tag) {
                let (from, _) = open.remove(at);
                for tok in &mut toks[from + 1..i] {
                    if let Tok::Seg(s) = tok {
                        set_tag(&mut s.f, tag);
                    }
                }
                toks[from] = Tok::Seg(Seg::plain(String::new()));
                toks[i] = Tok::Seg(Seg::plain(String::new()));
            }
        }
        let mut out: Vec<Seg> = Vec::new();
        for tok in toks {
            let seg = match tok {
                Tok::Seg(s) => s,
                Tok::Delim { ch, n, .. } => Seg::plain(ch.to_string().repeat(n)),
                Tok::Tag { raw, .. } => Seg::plain(raw),
            };
            match out.last_mut() {
                Some(last) if last.f == seg.f && last.object == 0 && seg.object == 0 => {
                    last.text.push_str(&seg.text)
                }
                _ if seg.text.is_empty() => {}
                _ => out.push(seg),
            }
        }
        out
    }

    fn tokens(&mut self, s: &str, depth: usize) -> Vec<Tok> {
        let mut toks: Vec<Tok> = Vec::new();
        let mut text = String::new();
        let mut i = 0;
        macro_rules! flush {
            () => {
                if !text.is_empty() {
                    toks.push(Tok::Seg(Seg::plain(std::mem::take(&mut text))));
                }
            };
        }
        while i < s.len() {
            let c = s[i..].chars().next().unwrap_or(' ');
            let rest = &s[i..];
            match c {
                '\\' => match rest[1..].chars().next() {
                    Some('\n') => {
                        flush!();
                        toks.push(Tok::Seg(Seg::plain(chars::LINE_BREAK.to_string())));
                        i += 2;
                    }
                    Some(n) if n.is_ascii_punctuation() => {
                        text.push(n);
                        i += 1 + n.len_utf8();
                    }
                    _ => {
                        text.push('\\');
                        i += 1;
                    }
                },
                '`' => {
                    let run = rest.bytes().take_while(|&b| b == b'`').count();
                    match code_end(s, i + run, run) {
                        Some(end) => {
                            flush!();
                            let inner = s[i + run..end - run].replace('\n', " ");
                            let pad = inner.len() > 2
                                && inner.starts_with(' ')
                                && inner.ends_with(' ')
                                && !inner.trim().is_empty();
                            let inner = if pad {
                                &inner[1..inner.len() - 1]
                            } else {
                                &inner
                            };
                            toks.push(Tok::Seg(Seg {
                                text: inner.to_string(),
                                f: Flags {
                                    code: true,
                                    ..Flags::default()
                                },
                                object: 0,
                            }));
                            i = end;
                        }
                        None => {
                            text.push_str(&rest[..run]);
                            i += run;
                        }
                    }
                }
                '&' => match entity(rest) {
                    Some((e, len)) => {
                        text.push(e);
                        i += len;
                    }
                    None => {
                        text.push('&');
                        i += 1;
                    }
                },
                '\n' => {
                    let hard = text.ends_with("  ");
                    let kept = text.trim_end_matches(' ').len();
                    text.truncate(kept);
                    flush!();
                    toks.push(Tok::Seg(Seg::plain(if hard {
                        chars::LINE_BREAK.to_string()
                    } else {
                        " ".to_string()
                    })));
                    i += 1;
                }
                '!' | '[' => {
                    let image = c == '!';
                    let open = if image { i + 1 } else { i };
                    if (!image || s[open..].starts_with('['))
                        && depth < MAX_DEPTH
                        && let Some((segs, end)) = self.link(s, open, image, depth)
                    {
                        flush!();
                        toks.extend(segs.into_iter().map(Tok::Seg));
                        i = end;
                    } else {
                        text.push(c);
                        i += 1;
                    }
                }
                '<' => {
                    let close = rest.find('>');
                    let inner = close.map(|e| &rest[1..e]).filter(|t| !t.contains('\n'));
                    match (close, inner) {
                        (Some(end), Some(inner)) => {
                            let is_url = inner.split_once(':').is_some_and(|(scheme, _)| {
                                (2..=32).contains(&scheme.len())
                                    && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                                    && scheme
                                        .bytes()
                                        .all(|b| b.is_ascii_alphanumeric() || b"+.-".contains(&b))
                            }) && !inner.contains([' ', '<']);
                            let is_mail =
                                !is_url && inner.contains('@') && !inner.contains([' ', '<', ':']);
                            if is_url || is_mail {
                                flush!();
                                let url = if is_mail {
                                    format!("mailto:{inner}")
                                } else {
                                    inner.to_string()
                                };
                                toks.push(Tok::Seg(Seg {
                                    text: inner.to_string(),
                                    f: Flags {
                                        link: link_id(&url),
                                        ..Flags::default()
                                    },
                                    object: 0,
                                }));
                                i += end + 1;
                            } else if let Some((tag, open)) = known_tag(inner) {
                                flush!();
                                toks.push(match tag {
                                    Some(tag) => Tok::Tag {
                                        tag,
                                        open,
                                        raw: rest[..=end].to_string(),
                                    },
                                    None => Tok::Seg(Seg::plain(chars::LINE_BREAK.to_string())),
                                });
                                i += end + 1;
                            } else {
                                if inner.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/')
                                {
                                    self.report.note(notes::MARKDOWN_HTML);
                                }
                                text.push('<');
                                i += 1;
                            }
                        }
                        _ => {
                            text.push('<');
                            i += 1;
                        }
                    }
                }
                '*' | '_' | '~' => {
                    let run = rest.chars().take_while(|&x| x == c).count();
                    if c == '~' && run != 2 {
                        text.push_str(&rest[..run]);
                        i += run;
                        continue;
                    }
                    let prev = s[..i].chars().next_back().unwrap_or(' ');
                    let next = s[i + run..].chars().next().unwrap_or(' ');
                    let left = !next.is_whitespace()
                        && (!is_punct(next) || prev.is_whitespace() || is_punct(prev));
                    let right = !prev.is_whitespace()
                        && (!is_punct(prev) || next.is_whitespace() || is_punct(next));
                    let (open, close) = if c == '_' {
                        (
                            left && (!right || is_punct(prev)),
                            right && (!left || is_punct(next)),
                        )
                    } else {
                        (left, right)
                    };
                    flush!();
                    toks.push(Tok::Delim {
                        ch: c,
                        n: run,
                        open,
                        close,
                    });
                    i += run;
                }
                c => {
                    // A bare web address is a link, as on GitHub.
                    if (c == 'h' || c == 'H' || c == 'w' || c == 'W')
                        && self.in_link == 0
                        && s[..i]
                            .chars()
                            .next_back()
                            .is_none_or(|p| !p.is_alphanumeric() && p != '/' && p != '.')
                        && let Some((taken, shown, url)) = bare_address(rest)
                    {
                        flush!();
                        i += taken;
                        toks.push(Tok::Seg(Seg {
                            text: shown,
                            f: Flags {
                                link: link_id(&url),
                                ..Flags::default()
                            },
                            object: 0,
                        }));
                        continue;
                    }
                    text.push(c);
                    i += c.len_utf8();
                }
            }
        }
        flush!();
        toks
    }

    /// A link or picture whose `[` is at `open`: its segments and the byte
    /// after its end.
    fn link(
        &mut self,
        s: &str,
        open: usize,
        image: bool,
        depth: usize,
    ) -> Option<(Vec<Seg>, usize)> {
        let close = closing_bracket(s, open)?;
        let label = &s[open + 1..close];
        let after = close + 1;
        let (dest, end) = if s[after..].starts_with('(') {
            inline_destination(s, after + 1)?
        } else {
            let key = if s[after..].starts_with('[') {
                let end = closing_bracket(s, after)?;
                let inner = &s[after + 1..end];
                (
                    label_key(if inner.is_empty() { label } else { inner }),
                    end + 1,
                )
            } else {
                (label_key(label), after)
            };
            if key.0.starts_with('^') {
                return None;
            }
            (unescape(self.refs.get(&key.0)?), key.1)
        };
        if image {
            let segs = match self.picture(&dest) {
                Some(id) => vec![Seg {
                    text: chars::OBJECT.to_string(),
                    f: Flags::default(),
                    object: id,
                }],
                None => {
                    self.in_link += 1;
                    let alt: String = self
                        .inline(label, depth + 1)
                        .into_iter()
                        .map(|s| s.text)
                        .collect();
                    self.in_link -= 1;
                    if is_remote(&dest) {
                        // A picture on the web stays a link to it, which the
                        // writer turns back into the same picture reference.
                        vec![Seg {
                            text: linked_picture_text(&alt),
                            f: Flags {
                                link: link_id(dest.trim()),
                                ..Flags::default()
                            },
                            object: 0,
                        }]
                    } else {
                        vec![Seg::plain(if alt.is_empty() {
                            "[image]".into()
                        } else {
                            alt
                        })]
                    }
                }
            };
            return Some((segs, end));
        }
        self.in_link += 1;
        let mut segs = self.inline(label, depth + 1);
        self.in_link -= 1;
        if !dest.is_empty() {
            let id = link_id(&dest);
            for seg in &mut segs {
                if seg.f.link == 0 {
                    seg.f.link = id;
                }
            }
        }
        Some((segs, end))
    }

    fn picture(&mut self, src: &str) -> Option<u32> {
        if let Some(&known) = self.images.get(src) {
            return known;
        }
        let found = self.load_picture(src);
        self.images.insert(src.to_string(), found);
        found
    }

    fn load_picture(&mut self, src: &str) -> Option<u32> {
        let src = src.trim();
        let bytes = if let Some(data) = src.strip_prefix("data:") {
            crate::odt_read::base64_decode(data.split_once(";base64,")?.1)?
        } else if is_remote(src) {
            self.report.note(notes::MARKDOWN_LINKED_PICTURES);
            return None;
        } else {
            let name = percent_decode(src.strip_prefix("file://").unwrap_or(src));
            let path = match &self.base {
                Some(base) => base.join(name),
                None => PathBuf::from(name),
            };
            match crate::read_file_with_limit(&path, MAX_PICTURE) {
                Ok(bytes) => bytes,
                Err(_) => {
                    self.report.note(notes::MARKDOWN_PICTURES);
                    return None;
                }
            }
        };
        let Some(mut pic) = crate::images::picture(bytes) else {
            self.report.note(notes::PICTURES);
            return None;
        };
        let width = self.doc.props.text_width().max(720);
        if pic.size.0 > width {
            pic.size = (
                width,
                (pic.size.1 as i64 * width as i64 / pic.size.0 as i64).max(20) as i32,
            );
        }
        Some(self.doc.add_object(Object::Picture(pic)))
    }

    // ---- blocks ----

    fn chp(&self, s: &Seg, base: CharProps) -> CharProps {
        let mut c = base;
        let f = s.f;
        c.bold |= f.bold;
        c.italic |= f.italic;
        c.strike |= f.strike;
        if f.underline {
            c.underline = Underline::Single;
        }
        if f.sup {
            c.script = quill_core::Script::Super;
        } else if f.sub {
            c.script = quill_core::Script::Sub;
        }
        if f.link != 0 {
            c.link = f.link;
            c.color = quill_core::link::BROWSER_BLUE;
            c.underline = Underline::Single;
        }
        c.object = s.object;
        c
    }

    fn mono_chp(&mut self, mut chp: CharProps) -> CharProps {
        chp.font = self.doc.font_id(MONO);
        chp
    }

    /// Paragraph properties for a block in `cx`; `item` for the first
    /// paragraph of a list item, which hangs its bullet.
    fn pap(&self, cx: Ctx, item: bool) -> ParaProps {
        let mut pap = self.doc.styles.get(0).pap.clone();
        pap.left = cx.quote as i32 * QUOTE_INDENT + cx.list as i32 * LIST_INDENT;
        if item {
            pap.left += LIST_INDENT;
            pap.first = -LIST_INDENT;
            pap.after = 0;
        }
        if cx.quote > 0 {
            pap.borders.left = BorderLine {
                style: BorderStyle::Single,
                space: QUOTE_GAP * cx.quote as i32,
                ..Default::default()
            };
        }
        pap
    }

    fn text_paragraph(
        &mut self,
        text: &str,
        pap: ParaProps,
        base: CharProps,
        label: Option<String>,
    ) -> Paragraph {
        let mut p = Paragraph::new(pap, base);
        if let Some(label) = label {
            p.insert(0, &format!("{label}\t"), base);
        }
        for seg in self.inline(text, 0) {
            let mut chp = self.chp(&seg, base);
            if seg.f.code {
                chp = self.mono_chp(chp);
            }
            let at = p.len();
            p.insert(at, &seg.text, chp);
        }
        p
    }

    fn emit(&mut self, blocks: &[Block], cx: Ctx, out: &mut Vec<Paragraph>) {
        let normal = self.doc.styles.get(0).chp;
        for block in blocks {
            match block {
                Block::Para(text) => {
                    let pap = self.pap(cx, false);
                    out.push(self.text_paragraph(text, pap, normal, None));
                }
                Block::Heading(level, text) => {
                    let id = self
                        .doc
                        .styles
                        .find(&format!("heading {level}"))
                        .unwrap_or(0);
                    let style = self.doc.styles.get(id);
                    let (mut pap, chp) = (style.pap.clone(), style.chp);
                    pap.left += cx.quote as i32 * QUOTE_INDENT + cx.list as i32 * LIST_INDENT;
                    out.push(self.text_paragraph(text, pap, chp, None));
                }
                Block::Code(lines) => {
                    let chp = self.mono_chp(normal);
                    for (n, line) in lines.iter().enumerate() {
                        let mut pap = self.pap(cx, false);
                        pap.after = if n + 1 == lines.len() { 120 } else { 0 };
                        pap.shading = CODE_SHADING;
                        let mut p = Paragraph::new(pap, chp);
                        p.insert(0, &line.replace('\t', "    "), chp);
                        out.push(p);
                    }
                    if lines.is_empty() {
                        let mut pap = self.pap(cx, false);
                        pap.shading = CODE_SHADING;
                        out.push(Paragraph::new(pap, chp));
                    }
                }
                Block::Quote(inner) => {
                    let cx = Ctx {
                        quote: cx.quote.saturating_add(1),
                        ..cx
                    };
                    self.emit(inner, cx, out);
                }
                Block::List {
                    ordered,
                    start,
                    items,
                } => {
                    for (n, item) in items.iter().enumerate() {
                        let label = if *ordered {
                            format!("{}.", start.saturating_add(n as i32))
                        } else {
                            ['\u{2022}', '\u{25E6}', '\u{25AA}'][cx.list as usize % 3].to_string()
                        };
                        let inner = Ctx {
                            list: cx.list.saturating_add(1),
                            ..cx
                        };
                        // The bullet paragraph hangs from the outer level.
                        self.emit_item(item, cx, inner, label, out);
                    }
                }
                Block::Rule => {
                    let mut pap = self.pap(cx, false);
                    pap.borders.bottom = BorderLine {
                        style: BorderStyle::Single,
                        space: 0,
                        ..Default::default()
                    };
                    out.push(Paragraph::new(pap, normal));
                }
                Block::Table { align, head, rows } => self.table(align, head, rows, out),
                Block::Html(lines) => {
                    self.report.note(notes::MARKDOWN_HTML);
                    for line in lines {
                        let pap = self.pap(cx, false);
                        out.push(Paragraph::with_text(line, normal, pap));
                    }
                }
            }
        }
    }

    /// A list item: its first paragraph starts with `label` and hangs from
    /// `outer`; everything else in it sits at `inner`.
    fn emit_item(
        &mut self,
        item: &[Block],
        outer: Ctx,
        inner: Ctx,
        label: String,
        out: &mut Vec<Paragraph>,
    ) {
        let normal = self.doc.styles.get(0).chp;
        match item.first() {
            Some(Block::Para(text)) => {
                let pap = self.pap(outer, true);
                out.push(self.text_paragraph(text, pap, normal, Some(label)));
                self.emit(&item[1..], inner, out);
            }
            _ => {
                let pap = self.pap(outer, true);
                out.push(Paragraph::with_text(&format!("{label}\t"), normal, pap));
                self.emit(item, inner, out);
            }
        }
    }

    fn table(
        &mut self,
        align: &[Align],
        head: &[String],
        rows: &[Vec<String>],
        out: &mut Vec<Paragraph>,
    ) {
        let normal = self.doc.styles.get(0).chp;
        let columns = head.len().clamp(1, MAX_COLUMNS);
        let width = (self.doc.props.text_width() / columns as i32).max(144);
        let line = BorderLine {
            style: BorderStyle::Single,
            space: 0,
            ..Default::default()
        };
        let edge = Borders {
            top: line,
            bottom: line,
            left: line,
            right: line,
            ..Borders::default()
        };
        for (r, row) in std::iter::once(head)
            .chain(rows.iter().map(Vec::as_slice))
            .enumerate()
        {
            for (c, cell) in row.iter().enumerate().take(columns) {
                let mut pap = self.pap(Ctx::default(), false);
                pap.set_level(1);
                pap.after = 0;
                pap.align = align.get(c).copied().unwrap_or_default();
                let mut base = normal;
                base.bold = r == 0;
                let mut p = self.text_paragraph(cell, pap, base, None);
                p.mark = Mark::CellEnd;
                out.push(p);
            }
            let mut pap = self.pap(Ctx::default(), false);
            pap.set_level(1);
            pap.after = 0;
            let mut end = Paragraph::new(pap, normal);
            end.mark = Mark::RowEnd;
            end.row = Some(Box::new(RowProps {
                cells: vec![
                    CellProps {
                        width,
                        borders: edge,
                        ..CellProps::default()
                    };
                    columns
                ],
                gap: TABLE_GAP,
                indent: 0,
                height: RowHeight::Auto,
                align: Align::Left,
            }));
            out.push(end);
        }
    }
}

fn link_id(url: &str) -> u32 {
    quill_core::link::intern(quill_core::Link::new(url))
}

fn set_tag(f: &mut Flags, tag: Tag) {
    match tag {
        Tag::Bold => f.bold = true,
        Tag::Italic => f.italic = true,
        Tag::Strike => f.strike = true,
        Tag::Underline => f.underline = true,
        Tag::Sup => f.sup = true,
        Tag::Sub => f.sub = true,
    }
}

/// CommonMark's emphasis rules over the delimiter runs in `toks`.
fn emphasis(toks: &mut [Tok]) {
    let mut i = 0;
    while i < toks.len() {
        let (ch, close_n, opens) = match &toks[i] {
            Tok::Delim {
                ch,
                n,
                open,
                close: true,
            } if *n > 0 => (*ch, *n, *open),
            _ => {
                i += 1;
                continue;
            }
        };
        let mut found = None;
        for j in (0..i).rev() {
            if let Tok::Delim {
                ch: c,
                n,
                open: true,
                close,
            } = &toks[j]
                && *c == ch
                && *n > 0
            {
                // The rule of three: runs that could both open and close
                // match only when their lengths don't add to a multiple
                // of three.
                if (*close || opens) && (n + close_n) % 3 == 0 && !(n % 3 == 0 && close_n % 3 == 0)
                {
                    continue;
                }
                found = Some(j);
                break;
            }
        }
        let Some(j) = found else {
            i += 1;
            continue;
        };
        let open_n = match &toks[j] {
            Tok::Delim { n, .. } => *n,
            _ => 0,
        };
        let used = if ch == '~' || (open_n >= 2 && close_n >= 2) {
            2
        } else {
            1
        };
        if ch == '~' && open_n < 2 {
            i += 1;
            continue;
        }
        for tok in &mut toks[j + 1..i] {
            match tok {
                Tok::Seg(s) => match (ch, used) {
                    ('~', _) => s.f.strike = true,
                    (_, 2) => s.f.bold = true,
                    _ => s.f.italic = true,
                },
                Tok::Delim { open, close, .. } => {
                    *open = false;
                    *close = false;
                }
                Tok::Tag { .. } => {}
            }
        }
        if let Tok::Delim { n, .. } = &mut toks[j] {
            *n -= used;
        }
        if let Tok::Delim { n, .. } = &mut toks[i] {
            *n -= used;
            if *n == 0 {
                i += 1;
            }
        }
    }
}

/// Read Markdown. `base` is the folder the file is in, for pictures it
/// names; nothing is fetched from the network.
pub fn read(text: &str, base: Option<&Path>) -> (Document, ImportReport) {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<String> = text.split('\n').map(String::from).collect();
    let mut r = Reader::new(base);

    // YAML front matter at the very top is kept as a block of code.
    let mut front = Vec::new();
    if lines.first().is_some_and(|l| l.trim_end() == "---")
        && let Some(end) = lines
            .iter()
            .skip(1)
            .position(|l| matches!(l.trim_end(), "---" | "..."))
    {
        front = lines.drain(..end + 2).collect();
    }

    // Link reference definitions, which can be anywhere outside code.
    let mut fence: Option<(char, usize)> = None;
    for line in &mut lines {
        match fence {
            Some((c, len)) => {
                if fence_closes(line, c, len) {
                    fence = None;
                }
            }
            None => {
                if let Some((c, len, _)) = fence_open(line) {
                    fence = Some((c, len));
                } else if let Some((key, dest)) = definition(line) {
                    r.refs.entry(key).or_insert(dest);
                    line.clear();
                } else if line.trim_start().starts_with("[^") && line.contains("]:") {
                    r.report.note(notes::MARKDOWN_NOTES);
                }
            }
        }
    }

    let parsed = blocks(&lines, 0);
    let mut paras = Vec::new();
    if !front.is_empty() {
        let normal = r.doc.styles.get(0).chp;
        let chp = r.mono_chp(normal);
        let n = front.len();
        for (k, line) in front.iter().enumerate() {
            let mut pap = r.pap(Ctx::default(), false);
            pap.after = if k + 1 == n { 120 } else { 0 };
            pap.shading = CODE_SHADING;
            let mut p = Paragraph::new(pap, chp);
            p.insert(0, line, chp);
            paras.push(p);
        }
    }
    r.emit(&parsed, Ctx::default(), &mut paras);
    let normal = r.doc.styles.get(0).clone();
    // A table cannot be the very last thing, and a document is never empty.
    if paras.is_empty() || paras.last().is_some_and(|p| p.pap.in_table) {
        paras.push(Paragraph::new(normal.pap, normal.chp));
    }
    r.doc.main.paras = paras;
    r.doc.forget_history();
    (r.doc, r.report)
}
