//! Edit > Advanced: Grep, Regex Search and Sed.
//!
//! Both work on paragraphs, the way the Unix tools work on lines, and both
//! use the same pattern syntax on every system (the Rust regex crate's:
//! extended syntax, so groups are `(...)`, not `\(...\)`). This file holds
//! the text side only: compiling patterns, finding matching paragraphs and
//! working out the edits a Sed script makes. The dialogs are in
//! `advanced_dialog.rs` and the document changes in `docview/tools.rs`.

use regex::{Regex, RegexBuilder};

/// Longest pattern or script Quill will compile.
const MAX_SCRIPT: usize = 4000;

/// Compiled patterns may not grow past this many bytes, so a pattern like
/// `(a{1000}){1000}` is refused instead of using all the memory.
const SIZE_LIMIT: usize = 4 << 20;

/// A readable message for a pattern the regex crate refused.
fn explain(error: &regex::Error) -> String {
    let text = error.to_string();
    // The crate's own message has a caret diagram; the last "error:" line
    // is the part that says what is wrong.
    let reason = text
        .lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix("error: "))
        .unwrap_or(text.trim());
    format!("The pattern is not valid: {reason}.")
}

fn build(pattern: &str, ignore_case: bool) -> Result<Regex, String> {
    if pattern.len() > MAX_SCRIPT {
        return Err("The pattern is too long.".into());
    }
    RegexBuilder::new(pattern)
        .case_insensitive(ignore_case)
        .size_limit(SIZE_LIMIT)
        .build()
        .map_err(|e| explain(&e))
}

/// A paragraph as the tools see it: its text, and the byte ranges of it
/// that tracked changes have deleted. Deleted text is still in the
/// paragraph, but it is no longer part of what the document says, so a
/// pattern never matches it, and cannot match it again after it was
/// replaced with the change tracked.
#[derive(Clone, Debug, Default)]
pub struct Line<'a> {
    pub text: &'a str,
    pub deleted: Vec<(usize, usize)>,
}

impl<'a> Line<'a> {
    /// A paragraph with nothing deleted.
    #[cfg(test)]
    pub fn plain(text: &'a str) -> Line<'a> {
        Line {
            text,
            deleted: Vec::new(),
        }
    }

    fn visible(&self) -> Visible<'a> {
        if self.deleted.is_empty() {
            return Visible {
                text: std::borrow::Cow::Borrowed(self.text),
                kept: Vec::new(),
                len: self.text.len(),
            };
        }
        // One pass over the deleted ranges in order, keeping what lies
        // between them. Each kept stretch is remembered by where it starts
        // in both texts, not byte by byte.
        let mut deleted = self.deleted.clone();
        deleted.sort_unstable();
        let mut text = String::with_capacity(self.text.len());
        let mut kept = Vec::with_capacity(deleted.len() + 1);
        let mut at = 0;
        let mut keep = |from: usize, to: usize, text: &mut String| {
            if to > from {
                kept.push((text.len(), from));
                text.push_str(&self.text[from..to]);
            }
        };
        for &(a, b) in &deleted {
            let (a, b) = (a.min(self.text.len()), b.min(self.text.len()));
            if a > at {
                keep(at, a, &mut text);
            }
            at = at.max(b);
        }
        keep(at, self.text.len(), &mut text);
        Visible {
            text: std::borrow::Cow::Owned(text),
            kept,
            len: self.text.len(),
        }
    }
}

/// A paragraph's text without what was deleted, and where it lies in the
/// paragraph's own text.
struct Visible<'a> {
    text: std::borrow::Cow<'a, str>,
    /// Each stretch of the visible text that is in one piece in the
    /// paragraph: where it starts in the visible text, and in the
    /// paragraph's. Empty when nothing is deleted.
    kept: Vec<(usize, usize)>,
    /// The length of the paragraph's own text.
    len: usize,
}

impl Visible<'_> {
    /// Where byte `i` of the visible text is in the paragraph's own text.
    fn byte(&self, i: usize) -> usize {
        let k = self.kept.partition_point(|&(v, _)| v <= i);
        match k.checked_sub(1).map(|k| self.kept[k]) {
            Some((v, o)) => o + (i - v),
            None => i,
        }
    }

    /// Bytes `a..b` of the visible text, as bytes of the paragraph's own.
    fn original(&self, a: usize, b: usize) -> (usize, usize) {
        if self.kept.is_empty() && matches!(self.text, std::borrow::Cow::Borrowed(_)) {
            return (a, b);
        }
        let start = if a < self.text.len() {
            self.byte(a)
        } else {
            self.len
        };
        let end = if b > a { self.byte(b - 1) + 1 } else { start };
        (start, end)
    }
}

// ----- Grep -----

/// What Grep looks for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GrepOptions {
    pub ignore_case: bool,
    /// Match whole words only (grep -w).
    pub whole_word: bool,
    /// List the paragraphs that do not match (grep -v).
    pub invert: bool,
}

/// One paragraph Grep lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrepHit {
    pub para: usize,
    /// The first match in the paragraph, as byte offsets. Empty at the
    /// start for an inverted search, which has nothing to point at.
    pub start: usize,
    pub end: usize,
    /// The paragraph's text, for the list.
    pub text: String,
}

/// The paragraphs of `paras` that match `pattern`, in order.
pub fn grep(pattern: &str, options: &GrepOptions, paras: &[Line]) -> Result<Vec<GrepHit>, String> {
    if pattern.is_empty() {
        return Err("Type a pattern to look for.".into());
    }
    let source = if options.whole_word {
        format!(r"\b(?:{pattern})\b")
    } else {
        pattern.to_string()
    };
    let re = build(&source, options.ignore_case)?;
    let mut hits = Vec::new();
    for (para, line) in paras.iter().enumerate() {
        let seen = line.visible();
        let found = re.find(&seen.text);
        let matched = found.is_some();
        if matched == options.invert {
            continue;
        }
        let (start, end) = found.map_or((0, 0), |m| seen.original(m.start(), m.end()));
        hits.push(GrepHit {
            para,
            start,
            end,
            text: line.text.to_string(),
        });
    }
    Ok(hits)
}

// ----- Sed -----

/// One piece of a Sed replacement.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Piece {
    Text(String),
    /// A captured group; 0 is the whole match (`&` and `\0`).
    Group(usize),
}

/// Which paragraphs a command applies to.
#[derive(Clone, Debug)]
enum Address {
    All,
    /// A paragraph number, from 1.
    Line(usize),
    /// A range of paragraph numbers; `None` is `$`, the last paragraph.
    Range(usize, Option<usize>),
    /// The last paragraph.
    Last,
    /// Paragraphs that match.
    Matching(Regex),
}

impl Address {
    fn covers(&self, number: usize, last: usize, text: &str) -> bool {
        match self {
            Address::All => true,
            Address::Line(n) => *n == number,
            Address::Range(a, b) => number >= *a && number <= b.unwrap_or(last),
            Address::Last => number == last,
            Address::Matching(re) => re.is_match(text),
        }
    }
}

/// One `s` command.
#[derive(Clone, Debug)]
struct Substitute {
    address: Address,
    re: Regex,
    replacement: Vec<Piece>,
    /// Replace every match from the `nth` on, not only that one.
    global: bool,
    /// The first match to replace, from 1.
    nth: usize,
    /// Leave empty matches alone, as Find does.
    skip_empty: bool,
}

/// A parsed Sed script: the `s` commands, in order.
#[derive(Clone, Debug)]
pub struct Sed {
    commands: Vec<Substitute>,
}

/// A change Sed makes inside one paragraph: bytes `start..end` become
/// `text`, where a `\n` in `text` starts a new paragraph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub para: usize,
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// Where a Sed script may change text: from `.0` to `.1`, each a
/// (paragraph, byte offset). A match has to lie wholly inside.
pub type Span = ((usize, usize), (usize, usize));

struct Reader<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl Reader<'_> {
    fn skip_space(&mut self) {
        while self.chars.next_if(|c| c.is_whitespace()).is_some() {}
    }

    /// Read up to an unescaped `delim`. `\delim` becomes the delimiter
    /// itself (literal, when it means something in a pattern); any other
    /// escape is kept for the caller. None if the delimiter never comes.
    fn until(&mut self, delim: char, in_pattern: bool) -> Option<String> {
        let mut out = String::new();
        loop {
            match self.chars.next()? {
                c if c == delim => return Some(out),
                '\\' => match self.chars.next()? {
                    c if c == delim => {
                        // Keep it literal where it would mean something.
                        let meaningful = if in_pattern {
                            "\\.+*?()|[]{}^$#&-~".contains(c)
                        } else {
                            c == '&' || c.is_ascii_digit()
                        };
                        if meaningful {
                            out.push('\\');
                        }
                        out.push(c);
                    }
                    c => {
                        out.push('\\');
                        out.push(c);
                    }
                },
                c => out.push(c),
            }
        }
    }
}

fn parse_number(r: &mut Reader) -> Option<usize> {
    let mut digits = String::new();
    while let Some(c) = r.chars.next_if(|c| c.is_ascii_digit()) {
        digits.push(c);
    }
    digits.parse().ok()
}

fn parse_address(r: &mut Reader) -> Result<Address, String> {
    match r.chars.peek() {
        Some('$') => {
            r.chars.next();
            Ok(Address::Last)
        }
        Some('/') => {
            r.chars.next();
            let pattern = r
                .until('/', true)
                .ok_or("An address starting with / needs a closing /.")?;
            Ok(Address::Matching(build(&pattern, false)?))
        }
        Some(c) if c.is_ascii_digit() => {
            let from = parse_number(r)
                .filter(|n| *n > 0)
                .ok_or("Paragraph numbers start at 1.")?;
            if r.chars.next_if_eq(&',').is_none() {
                return Ok(Address::Line(from));
            }
            if r.chars.next_if_eq(&'$').is_some() {
                return Ok(Address::Range(from, None));
            }
            let to =
                parse_number(r).ok_or("A range needs a paragraph number or $ after the comma.")?;
            Ok(Address::Range(from, Some(to)))
        }
        _ => Ok(Address::All),
    }
}

/// Turn the text between the last two delimiters of `s/.../.../` into
/// pieces. `groups` is how many groups the pattern has.
fn parse_replacement(text: &str, groups: usize, pattern: &str) -> Result<Vec<Piece>, String> {
    let mut pieces = Vec::new();
    let mut literal = String::new();
    let mut chars = text.chars();
    let flush = |literal: &mut String, pieces: &mut Vec<Piece>| {
        if !literal.is_empty() {
            pieces.push(Piece::Text(std::mem::take(literal)));
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '&' => {
                flush(&mut literal, &mut pieces);
                pieces.push(Piece::Group(0));
            }
            '\\' => match chars.next() {
                None => return Err("The replacement ends with a lone \\.".into()),
                Some(d) if d.is_ascii_digit() => {
                    let n = d.to_digit(10).unwrap() as usize;
                    if n > groups {
                        let hint = if groups == 0 && pattern.contains("\\(") {
                            " Patterns use (...) for groups, not \\(...\\)."
                        } else {
                            ""
                        };
                        return Err(format!(
                            "The replacement uses \\{n}, but the pattern has {groups} group{}.{hint}",
                            if groups == 1 { "" } else { "s" }
                        ));
                    }
                    flush(&mut literal, &mut pieces);
                    pieces.push(Piece::Group(n));
                }
                Some('n') => literal.push('\n'),
                Some('t') => literal.push('\t'),
                Some(d @ ('U' | 'L' | 'u' | 'l' | 'E')) => {
                    return Err(format!(
                        "Case changes like \\{d} in a replacement are not supported."
                    ));
                }
                Some(d) => literal.push(d),
            },
            c => literal.push(c),
        }
    }
    flush(&mut literal, &mut pieces);
    Ok(pieces)
}

impl Sed {
    /// Parse `script`: `s` commands separated by `;` or new lines, each
    /// with an optional address.
    pub fn parse(script: &str) -> Result<Sed, String> {
        if script.len() > MAX_SCRIPT {
            return Err("The script is too long.".into());
        }
        let mut r = Reader {
            chars: script.chars().peekable(),
        };
        let mut commands = Vec::new();
        loop {
            while r
                .chars
                .next_if(|c| c.is_whitespace() || *c == ';')
                .is_some()
            {}
            if r.chars.peek().is_none() {
                break;
            }
            let address = parse_address(&mut r)?;
            r.skip_space();
            match r.chars.next() {
                Some('s') => {}
                Some(c) => {
                    return Err(format!(
                        "The \"{c}\" command is not supported. Sed here has only s, as in s/old/new/g."
                    ));
                }
                None => {
                    return Err("An address needs a command after it, such as s/old/new/.".into());
                }
            }
            let delim = match r.chars.next() {
                Some(c) if !c.is_alphanumeric() && c != '\\' && !c.is_whitespace() => c,
                _ => return Err("s needs a delimiter after it, as in s/old/new/.".into()),
            };
            let pattern = r
                .until(delim, true)
                .ok_or("The s command needs three delimiters, as in s/old/new/.")?;
            let replacement = r
                .until(delim, false)
                .ok_or("The s command needs three delimiters, as in s/old/new/.")?;
            let mut global = false;
            let mut ignore_case = false;
            let mut nth = None;
            while let Some(&c) = r.chars.peek() {
                match c {
                    'g' => global = true,
                    'i' | 'I' => ignore_case = true,
                    '0'..='9' => {
                        let n = parse_number(&mut r).unwrap_or(0);
                        if n == 0 {
                            return Err("A number flag counts from 1.".into());
                        }
                        nth = Some(n);
                        continue;
                    }
                    ';' | '\n' | '\r' | ' ' | '\t' => break,
                    other => {
                        return Err(format!(
                            "The flag \"{other}\" is not supported. Use g, i or a number."
                        ));
                    }
                }
                r.chars.next();
            }
            if pattern.is_empty() {
                return Err("The pattern in an s command may not be empty.".into());
            }
            let re = build(&pattern, ignore_case)?;
            let replacement = parse_replacement(&replacement, re.captures_len() - 1, &pattern)?;
            commands.push(Substitute {
                address,
                re,
                replacement,
                global,
                nth: nth.unwrap_or(1),
                skip_empty: false,
            });
        }
        if commands.is_empty() {
            return Err("Type a script, such as s/old/new/g.".into());
        }
        Ok(Sed { commands })
    }

    /// How many commands the script has.
    pub fn command_count(&self) -> usize {
        self.commands.len()
    }

    /// The edits command `index` makes to `paras`, within `span`, in
    /// document order. Apply them last first so the offsets stay true.
    #[cfg(test)]
    pub fn edits(&self, index: usize, paras: &[Line], span: Span) -> Vec<Edit> {
        self.edits_within(index, paras, span, Room::UNLIMITED)
            .expect("no limit")
    }

    /// `edits`, stopping as soon as they would grow the document past
    /// `room`, so a script cannot build more replacement text than the
    /// document could hold. Says why when it stops.
    pub fn edits_within(
        &self,
        index: usize,
        paras: &[Line],
        span: Span,
        room: Room,
    ) -> Result<Vec<Edit>, String> {
        let cmd = &self.commands[index];
        let mut added = 0usize;
        let mut removed = 0usize;
        let mut new_paras = 0usize;
        let last = paras.len();
        let mut edits = Vec::new();
        let (from, to) = span;
        for (para, line) in paras.iter().enumerate() {
            if para < from.0 || para > to.0 {
                continue;
            }
            let seen = line.visible();
            let text: &str = &seen.text;
            if !cmd.address.covers(para + 1, last, text) {
                continue;
            }
            let lo = if para == from.0 { from.1 } else { 0 };
            let hi = if para == to.0 {
                to.1.min(line.text.len())
            } else {
                line.text.len()
            };
            // Matches are numbered among those inside the span, so `s/a/b/2`
            // changes the second one there, not the second in the paragraph.
            let mut inside = 0;
            for caps in cmd.re.captures_iter(text) {
                let whole = caps.get(0).unwrap();
                if cmd.skip_empty && whole.is_empty() {
                    continue;
                }
                let (start, end) = seen.original(whole.start(), whole.end());
                if start < lo || end > hi {
                    continue;
                }
                inside += 1;
                if inside < cmd.nth {
                    continue;
                }
                let mut out = String::new();
                for piece in &cmd.replacement {
                    match piece {
                        Piece::Text(t) => out.push_str(t),
                        Piece::Group(n) => {
                            out.push_str(caps.get(*n).map_or("", |m| m.as_str()));
                        }
                    }
                }
                // A match that changes nothing is not an edit.
                if out != whole.as_str() {
                    added += out.chars().count();
                    if !room.keeps_old {
                        removed += whole.as_str().chars().count();
                    }
                    new_paras += out.matches('\n').count();
                    if added.saturating_sub(removed) > room.characters {
                        return Err(format!(
                            "it would have more than {} characters, Quill's limit",
                            room.limit_characters
                        ));
                    }
                    if new_paras > room.paragraphs {
                        return Err(format!(
                            "it would have more than {} paragraphs, Quill's limit",
                            room.limit_paragraphs
                        ));
                    }
                    edits.push(Edit {
                        para,
                        start,
                        end,
                        text: out,
                    });
                }
                if !cmd.global {
                    break;
                }
            }
        }
        Ok(edits)
    }
}

/// How much a script may still add to a document: characters and
/// paragraphs, against Quill's limits. Text an edit replaces counts as
/// given back unless `keeps_old`: a tracked change keeps it, struck
/// through.
#[derive(Clone, Copy, Debug)]
pub struct Room {
    pub characters: usize,
    pub paragraphs: usize,
    pub keeps_old: bool,
    /// The limits, to say in a message.
    pub limit_characters: usize,
    pub limit_paragraphs: usize,
}

impl Room {
    #[cfg(test)]
    pub const UNLIMITED: Room = Room {
        characters: usize::MAX,
        paragraphs: usize::MAX,
        keeps_old: false,
        limit_characters: usize::MAX,
        limit_paragraphs: usize::MAX,
    };

    /// What is left for a document of `size`.
    pub fn left(size: quill_core::limits::Size, keeps_old: bool) -> Room {
        use quill_core::limits::{MAX_CHARACTERS, MAX_PARAGRAPHS};
        Room {
            characters: MAX_CHARACTERS.saturating_sub(size.characters),
            paragraphs: MAX_PARAGRAPHS.saturating_sub(size.paragraphs),
            keeps_old,
            limit_characters: MAX_CHARACTERS,
            limit_paragraphs: MAX_PARAGRAPHS,
        }
    }
}

// ----- Regex Search -----

/// A pattern and its replacement, for Regex Search: Find Next, Replace and
/// Replace All with the same syntax as Grep and Sed.
#[derive(Clone, Debug)]
pub struct Search {
    re: Regex,
    replacement: Vec<Piece>,
}

impl Search {
    /// Compile `pattern`; `replacement` is read as in Sed (`&`, `\1`, `\n`).
    pub fn new(
        pattern: &str,
        replacement: &str,
        ignore_case: bool,
        whole_word: bool,
    ) -> Result<Search, String> {
        if pattern.is_empty() {
            return Err("Type a pattern to look for.".into());
        }
        let source = if whole_word {
            format!(r"\b(?:{pattern})\b")
        } else {
            pattern.to_string()
        };
        let re = build(&source, ignore_case)?;
        let replacement = parse_replacement(replacement, re.captures_len() - 1, pattern)?;
        Ok(Search { re, replacement })
    }

    /// The first non-empty match at or after `from` (paragraph, byte
    /// offset), looking forward through `paras`. When nothing lies ahead it
    /// wraps to the top; the flag says it did. Returns (paragraph, start,
    /// end).
    pub fn find(
        &self,
        paras: &[Line],
        from: (usize, usize),
    ) -> Option<((usize, usize, usize), bool)> {
        let ahead = |para: usize, lo: usize| {
            let seen = paras[para].visible();
            self.re
                .find_iter(&seen.text)
                .filter(|m| !m.is_empty())
                .map(|m| seen.original(m.start(), m.end()))
                .find(|&(start, _)| start >= lo)
                .map(|(start, end)| (para, start, end))
        };
        for para in from.0..paras.len() {
            let lo = if para == from.0 { from.1 } else { 0 };
            if let Some(hit) = ahead(para, lo) {
                return Some((hit, false));
            }
        }
        // Wrapped: the top of the document up to where the search began.
        (0..=from.0.min(paras.len().saturating_sub(1)))
            .find_map(|para| ahead(para, 0))
            .map(|hit| (hit, true))
    }

    /// The edit that replaces the match at `start..end` of paragraph
    /// `para`, or `None` when that text is not a match of the pattern.
    pub fn edit_at(&self, line: &Line, para: usize, start: usize, end: usize) -> Option<Edit> {
        let seen = line.visible();
        let caps = self.re.captures_iter(&seen.text).find(|c| {
            c.get(0)
                .is_some_and(|m| !m.is_empty() && seen.original(m.start(), m.end()) == (start, end))
        })?;
        let mut out = String::new();
        for piece in &self.replacement {
            match piece {
                Piece::Text(t) => out.push_str(t),
                Piece::Group(n) => out.push_str(caps.get(*n).map_or("", |m| m.as_str())),
            }
        }
        Some(Edit {
            para,
            start,
            end,
            text: out,
        })
    }

    /// The same search as a Sed script that replaces every match, for
    /// Replace All.
    pub fn replace_all_script(&self) -> Sed {
        Sed {
            commands: vec![Substitute {
                address: Address::All,
                re: self.re.clone(),
                replacement: self.replacement.clone(),
                global: true,
                nth: 1,
                skip_empty: true,
            }],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<Line<'_>> {
        text.split('\n').map(Line::plain).collect()
    }

    /// Run the whole script on plain lines, as the document does: one
    /// command at a time, edits applied last first.
    fn run(script: &str, text: &str) -> Result<String, String> {
        let sed = Sed::parse(script)?;
        let mut current: Vec<String> = text.split('\n').map(String::from).collect();
        for i in 0..sed.command_count() {
            let refs: Vec<Line> = current.iter().map(|l| Line::plain(l)).collect();
            let end = (refs.len() - 1, refs[refs.len() - 1].text.len());
            let edits = sed.edits(i, &refs, ((0, 0), end));
            for e in edits.iter().rev() {
                let line = &mut current[e.para];
                let mut new = format!("{}{}{}", &line[..e.start], e.text, &line[e.end..]);
                if new.contains('\n') {
                    let parts: Vec<String> = new.split('\n').map(String::from).collect();
                    current.splice(e.para..=e.para, parts);
                    continue;
                }
                std::mem::swap(line, &mut new);
            }
        }
        Ok(current.join("\n"))
    }

    #[test]
    fn grep_lists_matching_paragraphs_with_the_first_match() {
        let paras = lines("apple pie\nbanana\nCrab apple");
        let hits = grep("apple", &GrepOptions::default(), &paras).unwrap();
        let found: Vec<_> = hits.iter().map(|h| (h.para, h.start, h.end)).collect();
        assert_eq!(found, [(0, 0, 5), (2, 5, 10)]);
        let ignoring = GrepOptions {
            ignore_case: true,
            ..Default::default()
        };
        assert_eq!(grep("crab", &ignoring, &paras).unwrap().len(), 1);
        assert!(
            grep("crab", &GrepOptions::default(), &paras)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn grep_whole_word_and_invert() {
        let paras = lines("cat\ncatalog\nthe cat sat");
        let word = GrepOptions {
            whole_word: true,
            ..Default::default()
        };
        let hits = grep("cat", &word, &paras).unwrap();
        assert_eq!(hits.iter().map(|h| h.para).collect::<Vec<_>>(), [0, 2]);
        // An alternation stays inside the word boundaries.
        let hits = grep("cat|sat", &word, &paras).unwrap();
        assert_eq!(hits.iter().map(|h| h.para).collect::<Vec<_>>(), [0, 2]);
        let invert = GrepOptions {
            invert: true,
            ..Default::default()
        };
        let hits = grep("cat", &invert, &paras).unwrap();
        assert!(hits.is_empty());
        let hits = grep("^c", &invert, &paras).unwrap();
        assert_eq!(hits.iter().map(|h| h.para).collect::<Vec<_>>(), [2]);
    }

    #[test]
    fn grep_says_what_is_wrong_with_a_pattern() {
        let paras = lines("x");
        let e = grep("(", &GrepOptions::default(), &paras).unwrap_err();
        assert!(e.starts_with("The pattern is not valid:"), "{e}");
        assert!(!e.contains('^'), "no caret diagram: {e}");
        // The things the regex crate does not do are named.
        let e = grep("(?=a)", &GrepOptions::default(), &paras).unwrap_err();
        assert!(e.contains("look-around"), "{e}");
        let e = grep(r"(a)\1", &GrepOptions::default(), &paras).unwrap_err();
        assert!(e.contains("backreferences"), "{e}");
        assert!(grep("", &GrepOptions::default(), &paras).is_err());
        let long = "a".repeat(MAX_SCRIPT + 1);
        assert!(grep(&long, &GrepOptions::default(), &paras).is_err());
        assert!(grep("(a{1000}){1000}", &GrepOptions::default(), &paras).is_err());
    }

    #[test]
    fn grep_understands_unicode_words_and_case() {
        let paras = lines("Caf\u{e9} au lait\nna\u{ef}ve");
        let opts = GrepOptions {
            ignore_case: true,
            whole_word: true,
            ..Default::default()
        };
        let hits = grep("caf\u{c9}", &opts, &paras).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            grep(r"\w+", &GrepOptions::default(), &lines("\u{e9}t\u{e9}")).unwrap()[0].end,
            5
        );
    }

    #[test]
    fn sed_substitutes_the_first_match_or_all_with_g() {
        assert_eq!(run("s/a/X/", "banana\napple").unwrap(), "bXnana\nXpple");
        assert_eq!(run("s/a/X/g", "banana\napple").unwrap(), "bXnXnX\nXpple");
        assert_eq!(run("s/a/X/2", "banana").unwrap(), "banXna");
        assert_eq!(run("s/a/X/2g", "banana").unwrap(), "banXnX");
        assert_eq!(run("s/A/X/gi", "banana").unwrap(), "bXnXnX");
    }

    #[test]
    fn sed_replacements_use_groups_and_the_whole_match() {
        assert_eq!(
            run(r"s/(\w+) (\w+)/\2 \1/", "hello world").unwrap(),
            "world hello"
        );
        assert_eq!(run("s/cat/[&]/g", "cat cat").unwrap(), "[cat] [cat]");
        assert_eq!(run(r"s/cat/\&/", "cat").unwrap(), "&");
        assert_eq!(run(r"s/a/\\/", "a").unwrap(), "\\");
        // A group that did not take part is empty.
        assert_eq!(run(r"s/a(x)?b/[\1]/", "ab").unwrap(), "[]");
    }

    #[test]
    fn sed_uses_any_delimiter_and_escapes_it() {
        assert_eq!(run("s|/usr|/opt|", "/usr/bin").unwrap(), "/opt/bin");
        assert_eq!(run(r"s/a\/b/c/", "a/b").unwrap(), "c");
        // An escaped delimiter that means something in a pattern is literal.
        assert_eq!(run(r"s|a\|b|X|", "a|b a").unwrap(), "X a");
        assert_eq!(run("s,a,b\\,c,", "a").unwrap(), "b,c");
    }

    #[test]
    fn sed_new_line_and_tab_in_a_replacement() {
        assert_eq!(run(r"s/, /,\n/g", "a, b, c").unwrap(), "a,\nb,\nc");
        assert_eq!(run(r"s/ /\t/", "a b").unwrap(), "a\tb");
    }

    #[test]
    fn sed_commands_run_in_order_and_addresses_pick_paragraphs() {
        assert_eq!(run("s/a/b/; s/b/c/", "a").unwrap(), "c");
        assert_eq!(run("s/a/b/\ns/b/c/", "a").unwrap(), "c");
        assert_eq!(run("2s/x/y/", "x\nx\nx").unwrap(), "x\ny\nx");
        assert_eq!(run("2,3s/x/y/", "x\nx\nx").unwrap(), "x\ny\ny");
        assert_eq!(run("2,$s/x/y/", "x\nx\nx").unwrap(), "x\ny\ny");
        assert_eq!(run("$s/x/y/", "x\nx\nx").unwrap(), "x\nx\ny");
        assert_eq!(
            run("/keep/s/x/y/", "x keep\nx\nx keep").unwrap(),
            "y keep\nx\ny keep"
        );
    }

    #[test]
    fn sed_with_the_empty_match_and_anchors() {
        assert_eq!(run("s/^/> /", "a\nb").unwrap(), "> a\n> b");
        assert_eq!(run("s/$/;/", "a\nb").unwrap(), "a;\nb;");
        assert_eq!(run("s/ +$//", "a  \nb").unwrap(), "a\nb");
        assert_eq!(run("s/x*/-/g", "abc").unwrap(), "-a-b-c-");
    }

    #[test]
    fn sed_only_changes_what_lies_inside_the_span() {
        let sed = Sed::parse("s/a/X/g").unwrap();
        let paras = lines("aaa\naaa\naaa");
        let edits = sed.edits(0, &paras, ((0, 1), (2, 2)));
        let spots: Vec<_> = edits.iter().map(|e| (e.para, e.start)).collect();
        assert_eq!(
            spots,
            [(0, 1), (0, 2), (1, 0), (1, 1), (1, 2), (2, 0), (2, 1)]
        );
        // A match that runs past the end of the span is left alone.
        let sed = Sed::parse("s/aa/X/").unwrap();
        assert!(sed.edits(0, &lines("aaa"), ((0, 0), (0, 1))).is_empty());
    }

    #[test]
    fn sed_leaves_a_match_that_does_not_change_alone() {
        let sed = Sed::parse("s/a/a/g").unwrap();
        assert!(sed.edits(0, &lines("aaa"), ((0, 0), (0, 3))).is_empty());
    }

    #[test]
    fn sed_scripts_that_cannot_run_say_why() {
        let msg = |s: &str| Sed::parse(s).unwrap_err();
        assert!(msg("").contains("Type a script"));
        assert!(msg("d").contains("not supported"));
        assert!(msg("s/a/b").contains("three delimiters"));
        assert!(msg("s").contains("delimiter"));
        assert!(msg("s//b/").contains("empty"));
        assert!(msg("s/a/b/x").contains("flag"));
        assert!(msg("s/a/b/0").contains("from 1"));
        assert!(msg("0s/a/b/").contains("start at 1"));
        assert!(msg("2").contains("command"));
        assert!(msg("s/(a/b/").contains("not valid"));
        assert!(msg(r"s/a/\1/").contains("0 groups"));
        assert!(msg(r"s/(a)/\2/").contains("1 group."));
        assert!(msg(r"s/\(a\)/\1/").contains("(...)"));
        assert!(msg(r"s/a/\U/").contains("not supported"));
        assert!(msg("s/a/b\\").contains("three delimiters"));
    }

    #[test]
    fn sed_reads_ignore_case_in_the_pattern_too() {
        assert_eq!(run("s/(?i)HELLO/hi/", "Hello").unwrap(), "hi");
    }

    #[test]
    fn sed_numbers_matches_inside_the_selection_only() {
        // Select "a a" (bytes 2 to 5) of "a a a a".
        let paras = lines("a a a a");
        let span = ((0, 2), (0, 5));
        // The first match in the selection, not the paragraph.
        let sed = Sed::parse("s/a/X/").unwrap();
        let edits = sed.edits(0, &paras, span);
        assert_eq!(edits.len(), 1);
        assert_eq!((edits[0].start, edits[0].end), (2, 3));
        // The second one in the selection is the third in the paragraph.
        let sed = Sed::parse("s/a/X/2").unwrap();
        let edits = sed.edits(0, &paras, span);
        assert_eq!(edits.len(), 1);
        assert_eq!((edits[0].start, edits[0].end), (4, 5));
        // Past the last one in the selection there is nothing to change.
        let sed = Sed::parse("s/a/X/3").unwrap();
        assert!(sed.edits(0, &paras, span).is_empty());
    }

    #[test]
    fn tracked_deleted_text_is_not_matched_or_counted() {
        // "one cat two" with "cat" deleted by a tracked change.
        let line = Line {
            text: "one cat two",
            deleted: vec![(4, 7)],
        };
        let paras = [line];
        let span = ((0, 0), (0, 11));
        let sed = Sed::parse("s/cat/dog/").unwrap();
        assert!(sed.edits(0, &paras, span).is_empty());
        // The text around it reads as if it were not there: a match across
        // the gap covers the deleted text, and anchors see the new ends.
        let sed = Sed::parse("s/one  two/X/").unwrap();
        let edits = sed.edits(0, &paras, span);
        assert_eq!((edits[0].start, edits[0].end), (0, 11));
        let sed = Sed::parse("s/two$/X/").unwrap();
        assert_eq!(sed.edits(0, &paras, span).len(), 1);
        // Grep does not list it either.
        let hits = grep("cat", &GrepOptions::default(), &paras).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn regex_find_and_replace_all_agree_about_empty_matches() {
        let search = Search::new("x*", "-", false, false).unwrap();
        let paras = lines("abc");
        // Every place is an empty match for x*, and Find skips them all.
        assert!(search.find(&paras, (0, 0)).is_none());
        let all = search.replace_all_script();
        assert!(all.edits(0, &paras, ((0, 0), (0, 3))).is_empty());
        // Real matches next to empty ones are still found and replaced.
        let search = Search::new("b*", "-", false, false).unwrap();
        let paras = lines("abbc");
        let found = search.find(&paras, (0, 0)).unwrap().0;
        assert_eq!(found, (0, 1, 3));
        let edits = search
            .replace_all_script()
            .edits(0, &paras, ((0, 0), (0, 4)));
        assert_eq!(edits.len(), 1);
        assert_eq!((edits[0].start, edits[0].end), (1, 3));
        // Sed itself still treats an empty match as a place to put text.
        let sed = Sed::parse("s/^/> /").unwrap();
        assert_eq!(sed.edits(0, &lines("abc"), ((0, 0), (0, 3))).len(), 1);
    }

    #[test]
    fn text_between_deletions_maps_back_to_its_own_place() {
        // "a" "c" "fé" stay; "b" and "de" are deleted, the latter given
        // twice and out of order, as overlapping ranges.
        let line = Line {
            text: "abcdefé",
            deleted: vec![(3, 5), (1, 2), (3, 4)],
        };
        let seen = line.visible();
        assert_eq!(seen.text, "acfé");
        assert_eq!(seen.original(0, 1), (0, 1));
        assert_eq!(seen.original(1, 2), (2, 3));
        assert_eq!(seen.original(1, 3), (2, 6));
        assert_eq!(seen.original(3, 5), (6, 8));
        assert_eq!(seen.original(5, 5), (8, 8));
        // Many deletions are walked once, not once a character.
        let long = "ab".repeat(50_000);
        let deleted: Vec<(usize, usize)> = (0..50_000).map(|i| (2 * i + 1, 2 * i + 2)).collect();
        let line = Line {
            text: &long,
            deleted,
        };
        let seen = line.visible();
        assert_eq!(seen.text.len(), 50_000);
        assert_eq!(seen.original(49_999, 50_000), (99_998, 99_999));
    }
}
