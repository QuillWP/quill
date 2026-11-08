//! The original's editing tools that work on the text itself: Change Case,
//! Sort, Calculate, Renumber and Bullet.

use crate::document::{Document, StoryId};
use crate::para::{Mark, Paragraph, chars};
use crate::props::TWIPS_PER_INCH;
use crate::story::Position;
use crate::table;
use std::cmp::Ordering;

/// Format > Change Case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    Upper,
    Lower,
    Title,
    Sentence,
    Toggle,
}

/// Rebuild `p` with the text in `start..end` passed through `f` run by
/// run, so each piece keeps its format. `f` sees each character and the
/// one before it. Returns the new end offset.
fn map_text(
    p: &Paragraph,
    start: usize,
    end: usize,
    f: &mut impl FnMut(char, Option<char>) -> String,
) -> (Paragraph, usize) {
    let mut out = Paragraph::new(p.pap.clone(), p.mark_chp);
    out.mark = p.mark;
    out.row = p.row.clone();
    let mut prev = p.text[..start].chars().next_back();
    let mut new_end = end;
    let mut at = 0;
    for (s, e, chp) in p.run_bounds() {
        let mut piece = String::new();
        for (i, c) in p.text[s..e].char_indices() {
            let pos = s + i;
            if pos >= start && pos < end {
                let mapped = f(c, prev);
                new_end = new_end + mapped.len() - c.len_utf8();
                piece.push_str(&mapped);
            } else {
                piece.push(c);
            }
            prev = Some(c);
        }
        out.insert(at, &piece, *chp);
        at += piece.len();
    }
    (out, new_end)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '’'
}

/// Change the case of `start..end`. Returns the new end, which moves when
/// a letter changes length (ß becomes SS).
pub fn change_case(
    doc: &mut Document,
    story: StoryId,
    start: Position,
    end: Position,
    case: Case,
) -> Position {
    let (start, end) = (start.min(end), start.max(end));
    let paras = &doc.story(story).paras;
    let mut new = Vec::new();
    let mut new_end = end;
    // Sentence case starts a sentence at the selection and after . ! ?
    let mut sentence_start = true;
    for (i, p) in paras.iter().enumerate().take(end.para + 1).skip(start.para) {
        let s = if i == start.para { start.offset } else { 0 };
        let e = if i == end.para { end.offset } else { p.len() };
        if i != start.para {
            sentence_start = true;
        }
        let mut f = |c: char, prev: Option<char>| -> String {
            match case {
                Case::Upper => c.to_uppercase().collect(),
                Case::Lower => c.to_lowercase().collect(),
                Case::Toggle => {
                    if c.is_uppercase() {
                        c.to_lowercase().collect()
                    } else {
                        c.to_uppercase().collect()
                    }
                }
                Case::Title => {
                    if prev.is_none_or(|p| !is_word_char(p)) {
                        c.to_uppercase().collect()
                    } else {
                        c.to_lowercase().collect()
                    }
                }
                Case::Sentence => {
                    let out = if sentence_start && c.is_alphabetic() {
                        sentence_start = false;
                        c.to_uppercase().collect()
                    } else {
                        c.to_lowercase().collect()
                    };
                    if matches!(c, '.' | '!' | '?') {
                        sentence_start = true;
                    }
                    out
                }
            }
        };
        let (q, e2) = map_text(p, s, e, &mut f);
        if i == end.para {
            new_end = Position::new(i, e2);
        }
        new.push(q);
    }
    if new[..] != doc.story(story).paras[start.para..=end.para] {
        let n = new.len();
        doc.splice(story, start.para, n, new);
    }
    new_end
}

/// Compare two texts the way Sort does: ignoring case and leading spaces,
/// with runs of digits compared as numbers.
pub fn sort_compare(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.trim_start(), b.trim_start());
    let mut x = a.chars().peekable();
    let mut y = b.chars().peekable();
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = x.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    x.next();
                }
                let mut nb = String::new();
                while let Some(d) = y.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(d);
                    y.next();
                }
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(c), Some(d)) => {
                let (lc, ld) = (c.to_lowercase().next(), d.to_lowercase().next());
                if lc != ld {
                    return lc.cmp(&ld);
                }
                x.next();
                y.next();
            }
        }
    }
}

/// Tools > Sort: put paragraphs `first..=last` in order. Inside a table
/// whole rows move, ordered by the cell in column `column`. Returns false
/// if there was nothing to sort.
pub fn sort(
    doc: &mut Document,
    story: StoryId,
    first: usize,
    last: usize,
    column: usize,
    descending: bool,
) -> bool {
    let paras = &doc.story(story).paras;
    // Each item is a group of paragraphs that moves together and its key.
    let mut items: Vec<(String, Vec<Paragraph>)> = Vec::new();
    let span_first;
    let span_last;
    if paras[first].pap.in_table {
        let all = table::table_rows(paras, first);
        let rows: Vec<_> = all
            .iter()
            .filter(|r| r.end >= first && r.first <= last)
            .cloned()
            .collect();
        // Inside one row, the whole table is sorted.
        let rows = if rows.len() < 2 { all } else { rows };
        let (Some(a), Some(b)) = (rows.first(), rows.last()) else {
            return false;
        };
        span_first = a.first;
        span_last = b.end;
        for r in &rows {
            let key = match r.cells.get(column).or(r.cells.first()) {
                Some(&(a, b)) => paras[a..=b]
                    .iter()
                    .map(|p| p.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                None => String::new(),
            };
            items.push((key, paras[r.first..=r.end].to_vec()));
        }
    } else {
        span_first = first;
        span_last = last;
        for p in &paras[first..=last] {
            if p.pap.in_table || p.mark != Mark::Paragraph {
                return false;
            }
            items.push((p.text.clone(), vec![p.clone()]));
        }
    }
    if items.len() < 2 {
        return false;
    }
    items.sort_by(|a, b| {
        let o = sort_compare(&a.0, &b.0);
        if descending { o.reverse() } else { o }
    });
    let new: Vec<Paragraph> = items.into_iter().flat_map(|(_, ps)| ps).collect();
    if new[..] != doc.story(story).paras[span_first..=span_last] {
        doc.splice(story, span_first, span_last - span_first + 1, new);
    }
    true
}

// ----- Calculate -----

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Op(char),
    Open,
    Close,
}

/// How the numbers were written, so the answer can be written alike.
#[derive(Clone, Copy, Debug, Default)]
struct Style {
    decimals: usize,
    dollars: bool,
    commas: bool,
    divided: bool,
}

fn tokenize(text: &str, style: &mut Style) -> Vec<Tok> {
    let cs: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_ascii_digit() || (c == '.' && cs.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let mut s = String::new();
            let mut decimals = None;
            while i < cs.len() {
                let d = cs[i];
                if d.is_ascii_digit() {
                    s.push(d);
                    if let Some(n) = decimals.as_mut() {
                        *n += 1;
                    }
                } else if d == ','
                    && decimals.is_none()
                    && cs.get(i + 1).is_some_and(char::is_ascii_digit)
                {
                    style.commas = true;
                } else if d == '.' && decimals.is_none() {
                    s.push('.');
                    decimals = Some(0);
                } else {
                    break;
                }
                i += 1;
            }
            style.decimals = style.decimals.max(decimals.unwrap_or(0));
            let mut v: f64 = s.parse().unwrap_or(0.0);
            if cs.get(i) == Some(&'%') {
                v /= 100.0;
                i += 1;
            }
            out.push(Tok::Num(v));
            continue;
        }
        match c {
            '$' => style.dollars = true,
            '+' | '-' | '*' | '/' | '^' => {
                if c == '/' {
                    style.divided = true;
                }
                out.push(Tok::Op(c));
            }
            '(' => out.push(Tok::Open),
            ')' => out.push(Tok::Close),
            _ => {}
        }
        i += 1;
    }
    out
}

struct Parser {
    toks: Vec<Tok>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<Tok> {
        self.toks.get(self.at).copied()
    }

    /// Sums, with numbers side by side (a column of figures) added up.
    fn sum(&mut self) -> Option<f64> {
        let mut v = self.product()?;
        loop {
            match self.peek() {
                Some(Tok::Op('+')) => {
                    self.at += 1;
                    v += self.product()?;
                }
                Some(Tok::Op('-')) => {
                    self.at += 1;
                    v -= self.product()?;
                }
                Some(Tok::Num(_)) | Some(Tok::Open) => v += self.product()?,
                _ => return Some(v),
            }
        }
    }

    fn product(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        loop {
            match self.peek() {
                Some(Tok::Op('*')) => {
                    self.at += 1;
                    v *= self.power()?;
                }
                Some(Tok::Op('/')) => {
                    self.at += 1;
                    let d = self.power()?;
                    if d == 0.0 {
                        return None;
                    }
                    v /= d;
                }
                _ => return Some(v),
            }
        }
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.peek() == Some(Tok::Op('^')) {
            self.at += 1;
            let e = self.power()?;
            return Some(base.powf(e));
        }
        Some(base)
    }

    fn unary(&mut self) -> Option<f64> {
        match self.peek()? {
            Tok::Op('-') => {
                self.at += 1;
                Some(-self.unary()?)
            }
            Tok::Op('+') => {
                self.at += 1;
                self.unary()
            }
            Tok::Num(v) => {
                self.at += 1;
                Some(v)
            }
            Tok::Open => {
                self.at += 1;
                let lone = matches!(
                    self.toks.get(self.at..self.at + 2),
                    Some([Tok::Num(_), Tok::Close])
                );
                let v = self.sum()?;
                if self.peek() == Some(Tok::Close) {
                    self.at += 1;
                }
                // A number alone in parentheses is negative, as in accounts.
                Some(if lone { -v } else { v })
            }
            _ => None,
        }
    }
}

fn group_thousands(digits: &str) -> String {
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Tools > Calculate: work out the numbers and operators in `text`. The
/// answer is written with as many decimals as the numbers had, and with a
/// dollar sign or thousands commas if they used them. None if there is
/// nothing to work out.
pub fn calculate(text: &str) -> Option<String> {
    let mut style = Style::default();
    let toks = tokenize(text, &mut style);
    if !toks.iter().any(|t| matches!(t, Tok::Num(_))) {
        return None;
    }
    let mut p = Parser { toks, at: 0 };
    let v = p.sum()?;
    if !v.is_finite() {
        return None;
    }
    let mut decimals = style.decimals;
    if decimals == 0 && style.divided && v.fract().abs() > 1e-9 {
        decimals = 2;
    }
    let s = format!("{:.*}", decimals, v.abs());
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i.to_string(), Some(f.to_string())),
        None => (s.clone(), None),
    };
    let int = if style.commas {
        group_thousands(&int)
    } else {
        int
    };
    let mut out = String::new();
    let rounded_zero = s.chars().all(|c| c == '0' || c == '.');
    if v < 0.0 && !rounded_zero {
        out.push('-');
    }
    if style.dollars {
        out.push('$');
    }
    out.push_str(&int);
    if let Some(f) = frac {
        out.push('.');
        out.push_str(&f);
    }
    Some(out)
}

// ----- Renumber -----

/// Tools > Renumber's Numbers setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Numbers {
    /// 1, 2, 3.
    Plain,
    /// 1, 1.1, 1.2, 2 by outline level.
    Outline,
    /// Like the first numbered paragraph.
    ByExample,
    Remove,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Renumber {
    /// Only paragraphs that already start with a number.
    pub only_numbered: bool,
    pub start: u32,
    /// An example of the number, like "1." or "(a)" or "IV)". Empty for
    /// the default "1.".
    pub format: String,
    pub numbers: Numbers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Numeral {
    Arabic,
    UpperLetter,
    LowerLetter,
    UpperRoman,
    LowerRoman,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NumFormat {
    before: String,
    numeral: Numeral,
    after: String,
}

impl NumFormat {
    fn parse(s: &str) -> Option<NumFormat> {
        let s = s.trim();
        let start = s.find(|c: char| c.is_alphanumeric())?;
        let body_len = s[start..]
            .find(|c: char| !c.is_alphanumeric())
            .unwrap_or(s.len() - start);
        let body = &s[start..start + body_len];
        let numeral = if body.chars().all(|c| c.is_ascii_digit()) {
            Numeral::Arabic
        } else if body.chars().all(|c| "IVXLCDM".contains(c)) && body != "D" && body != "C" {
            Numeral::UpperRoman
        } else if body.chars().all(|c| "ivxlcdm".contains(c)) && body != "d" && body != "c" {
            Numeral::LowerRoman
        } else if body.len() == 1 && body.chars().all(|c| c.is_ascii_uppercase()) {
            Numeral::UpperLetter
        } else if body.len() == 1 && body.chars().all(|c| c.is_ascii_lowercase()) {
            Numeral::LowerLetter
        } else {
            return None;
        };
        let after = &s[start + body_len..];
        // Only the punctuation straight after the number counts.
        let after: String = after.chars().take_while(|c| ".)]:".contains(*c)).collect();
        Some(NumFormat {
            before: s[..start].to_string(),
            numeral,
            after,
        })
    }

    fn write(&self, n: u32) -> String {
        format!("{}{}{}", self.before, numeral(self.numeral, n), self.after)
    }
}

fn roman(mut n: u32) -> String {
    const T: [(u32, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (v, s) in T {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

fn letters(mut n: u32) -> String {
    // 1 is a, 26 is z, 27 is aa.
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        out.push((b'a' + (n % 26) as u8) as char);
        n /= 26;
    }
    out.iter().rev().collect()
}

fn numeral(kind: Numeral, n: u32) -> String {
    match kind {
        Numeral::Arabic => n.to_string(),
        Numeral::LowerLetter => letters(n),
        Numeral::UpperLetter => letters(n).to_uppercase(),
        Numeral::LowerRoman => roman(n),
        Numeral::UpperRoman => roman(n).to_uppercase(),
    }
}

/// The number at the start of a paragraph and the space or tab after it:
/// "1. ", "(a)\t", "2.3\t", "IV) ". Returns its length in bytes and the
/// number as written, without the separator.
pub fn leading_number(text: &str) -> Option<(usize, &str)> {
    let mut i = 0;
    let b = text.as_bytes();
    if b.first() == Some(&b'(') {
        i += 1;
    }
    let body_start = i;
    while i < b.len()
        && (b[i].is_ascii_alphanumeric()
            || (b[i] == b'.' && i + 1 < b.len() && b[i + 1].is_ascii_digit()))
    {
        i += 1;
    }
    let body = &text[body_start..i];
    if body.is_empty() {
        return None;
    }
    let numeric = body.chars().all(|c| c.is_ascii_digit() || c == '.');
    let single_letter = body.len() == 1 && body.chars().all(|c| c.is_ascii_alphabetic());
    let roman = body.chars().all(|c| "ivxlcdmIVXLCDM".contains(c));
    if !(numeric || single_letter || roman) {
        return None;
    }
    let mut punct = false;
    while i < b.len() && matches!(b[i], b'.' | b')' | b']' | b':') {
        i += 1;
        punct = true;
    }
    let number_end = i;
    match b.get(i) {
        Some(b'\t') => {}
        Some(b' ') if punct => {}
        // Letters and Roman numerals need punctuation, or "I think" would
        // count as numbered.
        None if punct || numeric => {}
        _ => return None,
    }
    if !punct && !numeric && b.get(i) != Some(&b'\t') {
        return None;
    }
    if i < b.len() {
        i += 1;
    }
    Some((i, &text[..number_end]))
}

/// A paragraph's outline level for 1.1 numbering: the heading style's
/// level, or one level per half inch of left indent.
fn level(doc: &Document, p: &Paragraph) -> usize {
    let name = &doc.styles.get(p.pap.style).name;
    if let Some(n) = name
        .to_ascii_lowercase()
        .strip_prefix("heading ")
        .and_then(|n| n.trim().parse::<usize>().ok())
    {
        return n.clamp(1, 9) - 1;
    }
    (p.pap.left.max(0) / (TWIPS_PER_INCH / 2)) as usize
}

/// Tools > Renumber over paragraphs `first..=last`. Empty paragraphs and
/// table end marks are skipped.
pub fn renumber(doc: &mut Document, story: StoryId, first: usize, last: usize, opts: &Renumber) {
    let paras = &doc.story(story).paras;
    let example = match opts.numbers {
        Numbers::ByExample => paras[first..=last]
            .iter()
            .find_map(|p| leading_number(&p.text))
            .and_then(|(_, n)| {
                NumFormat::parse(n.rsplit('.').find(|s| !s.is_empty()).unwrap_or(n)).map(|mut f| {
                    // "1." parses its body as "1" and loses the dot; keep it.
                    if n.ends_with('.') && f.after.is_empty() {
                        f.after = ".".into();
                    }
                    f
                })
            }),
        _ => None,
    };
    let fmt = example
        .or_else(|| NumFormat::parse(&opts.format))
        .unwrap_or(NumFormat {
            before: String::new(),
            numeral: Numeral::Arabic,
            after: ".".into(),
        });
    let mut counters = [0u32; 9];
    let mut started = false;
    let mut new = Vec::new();
    for p in &paras[first..=last] {
        let mut q = p.clone();
        let existing = leading_number(&p.text);
        let skip = p.text.is_empty()
            || p.mark == Mark::RowEnd
            || (opts.only_numbered && existing.is_none())
            || (opts.numbers == Numbers::Remove && existing.is_none());
        if skip {
            new.push(q);
            continue;
        }
        // Keep the separator the paragraph had, a tab by default.
        let sep = match existing {
            Some((len, n)) if len > n.len() => p.text[n.len()..len].to_string(),
            _ => chars::TAB.to_string(),
        };
        let chp = q.chp_of_char(0);
        if let Some((len, _)) = existing {
            q.remove(0, len);
        }
        if opts.numbers != Numbers::Remove {
            let lv = if opts.numbers == Numbers::Outline {
                level(doc, p)
            } else {
                0
            };
            if !started {
                counters[lv] = opts.start.max(1) - 1;
                started = true;
            }
            counters[lv] += 1;
            for c in counters.iter_mut().skip(lv + 1) {
                *c = 0;
            }
            let number = if opts.numbers == Numbers::Outline {
                let parts: Vec<String> = counters[..=lv]
                    .iter()
                    .map(|&n| numeral(fmt.numeral, n.max(1)))
                    .collect();
                format!("{}{}{}", fmt.before, parts.join("."), fmt.after)
            } else {
                fmt.write(counters[0])
            };
            q.insert(0, &format!("{number}{sep}"), chp);
        }
        new.push(q);
    }
    if new[..] != doc.story(story).paras[first..=last] {
        let n = new.len();
        doc.splice(story, first, n, new);
    }
}

/// Insert > Bullet: a bullet and a tab at the start of each paragraph,
/// with a quarter-inch hanging indent. Paragraphs that already start with
/// a bullet are left alone.
pub fn bullet(doc: &mut Document, story: StoryId, first: usize, last: usize) {
    let paras = &doc.story(story).paras;
    let new: Vec<Paragraph> = paras[first..=last]
        .iter()
        .map(|p| {
            let mut q = p.clone();
            if p.mark == Mark::RowEnd || p.text.starts_with('•') {
                return q;
            }
            add_list_mark(&mut q, '•');
            q
        })
        .collect();
    if new[..] != doc.story(story).paras[first..=last] {
        let n = new.len();
        doc.splice(story, first, n, new);
    }
}

/// Put `mark` and a tab at the start of `p`, with a hanging indent.
fn add_list_mark(p: &mut Paragraph, mark: char) {
    let quarter = TWIPS_PER_INCH / 4;
    let chp = p.chp_at(0);
    p.insert(0, &format!("{mark}\t"), chp);
    if p.pap.first >= 0 {
        p.pap.left += quarter;
        p.pap.first = -quarter;
    }
}

/// How many bytes of `text` are a list paragraph's bullet and its tab, the
/// two of which act as one thing when editing.
pub fn list_mark_len(text: &str) -> Option<usize> {
    let mark = text.chars().next()?;
    let is_mark = matches!(
        mark,
        '•' | '\u{25E6}' | '\u{25AA}' | '\u{25CB}' | '\u{25A0}' | '\u{00B7}' | '-' | '*'
    );
    let len = mark.len_utf8();
    (is_mark && text[len..].starts_with('\t')).then_some(len + 1)
}

/// The length of a numbered list mark ("1." or "1)" and a tab, as a
/// Markdown numbered list has) at the start of `text`, if there is one.
pub fn numbered_mark_len(text: &str) -> Option<usize> {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    let rest = &text.as_bytes()[digits..];
    ((1..=9).contains(&digits) && matches!(rest, [b'.' | b')', b'\t', ..])).then_some(digits + 2)
}

/// The mark for the item after one marked `mark`: the next number for a
/// numbered mark, the same mark otherwise.
pub fn next_list_mark(mark: &str) -> String {
    match numbered_mark_len(mark) {
        Some(len) => {
            let n: u64 = mark[..len - 2].parse().unwrap_or(0);
            format!("{}{}", n.saturating_add(1), &mark[len - 2..len])
        }
        None => mark.to_string(),
    }
}

/// Turn the `typed` bytes at the start of paragraph `para` into a list mark.
pub fn start_list(doc: &mut Document, story: StoryId, para: usize, typed: usize, mark: char) {
    let mut p = doc.story(story).paras[para].clone();
    p.remove(0, typed);
    add_list_mark(&mut p, mark);
    doc.splice(story, para, 1, vec![p]);
}

/// Take the list mark and hanging indent off paragraph `para`, keeping its
/// text. Returns the length of what went.
pub fn end_list(doc: &mut Document, story: StoryId, para: usize) -> usize {
    let mut p = doc.story(story).paras[para].clone();
    let Some(len) = list_mark_len(&p.text).or_else(|| numbered_mark_len(&p.text)) else {
        return 0;
    };
    p.remove(0, len);
    if p.pap.first < 0 {
        p.pap.left = (p.pap.left + p.pap.first).max(0);
        p.pap.first = 0;
    }
    doc.splice(story, para, 1, vec![p]);
    len
}

/// Insert > Drop Cap: put the first `len` bytes of paragraph `para` in a
/// framed paragraph of their own, `lines` lines of `line_height` (twips)
/// tall, in font `font`. Returns false for a framed or table paragraph.
#[allow(clippy::too_many_arguments)]
pub fn drop_cap(
    doc: &mut Document,
    story: StoryId,
    para: usize,
    len: usize,
    lines: u8,
    line_height: crate::props::Twips,
    font: crate::props::FontId,
    in_margin: bool,
    small_caps: bool,
) -> bool {
    let p = doc.story(story).paras[para].clone();
    if p.pap.frame.is_some() || p.pap.in_table || len == 0 || len > p.len() {
        return false;
    }
    let height = line_height * lines as i32;
    let mut cap = p.slice(0, len);
    let size = (height / 10).clamp(2, 3276) as u16;
    let mut style = |c: &mut crate::props::CharProps| {
        c.font = font;
        c.size = size;
        c.small_caps = small_caps;
    };
    cap.format_range(0, len, &mut style);
    style(&mut cap.mark_chp);
    cap.mark = Mark::Paragraph;
    cap.row = None;
    cap.pap.left = 0;
    cap.pap.first = 0;
    cap.pap.right = 0;
    cap.pap.after = 0;
    cap.pap.line = crate::props::LineSpacing::Exactly(height);
    cap.pap.frame = Some(crate::props::Frame {
        drop_lines: lines,
        in_margin,
        from_text: 40,
        ..Default::default()
    });
    let rest = p.slice(len, p.len());
    doc.splice(story, para, 1, vec![cap, rest]);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::props::ParaProps;

    fn doc(paras: &[&str]) -> Document {
        let mut d = Document::new();
        let chp = d.main.paras[0].mark_chp;
        d.main.paras = paras
            .iter()
            .map(|t| Paragraph::with_text(t, chp, ParaProps::default()))
            .collect();
        d
    }

    fn texts(d: &Document) -> Vec<&str> {
        d.main.paras.iter().map(|p| p.text.as_str()).collect()
    }

    fn p(para: usize, offset: usize) -> Position {
        Position::new(para, offset)
    }

    #[test]
    fn cases() {
        let cases = [
            (Case::Upper, "HELLO WORLD. GOOD DAY"),
            (Case::Lower, "hello world. good day"),
            (Case::Title, "Hello World. Good Day"),
            (Case::Sentence, "Hello world. Good day"),
            (Case::Toggle, "HELLO wORLD. GOOD DAY"),
        ];
        for (case, want) in cases {
            let mut d = doc(&["hello World. good day"]);
            change_case(&mut d, StoryId::Main, p(0, 0), p(0, 21), case);
            assert_eq!(d.main.paras[0].text, want, "{case:?}");
        }
    }

    #[test]
    fn case_keeps_formatting_and_tracks_length() {
        let mut d = doc(&["straße ok"]);
        d.format_chars(StoryId::Main, p(0, 8), p(0, 10), |c| c.bold = true);
        let end = change_case(&mut d, StoryId::Main, p(0, 0), p(0, 10), Case::Upper);
        assert_eq!(d.main.paras[0].text, "STRASSE OK");
        assert_eq!(end, p(0, 10));
        assert!(d.main.paras[0].chp_of_char(8).bold);
        assert!(!d.main.paras[0].chp_of_char(0).bold);
    }

    #[test]
    fn sorts_paragraphs() {
        let mut d = doc(&["pear", "10 apples", "Apple", "2 apples", ""]);
        assert!(sort(&mut d, StoryId::Main, 0, 3, 0, false));
        d.commit(Default::default());
        assert_eq!(texts(&d), ["2 apples", "10 apples", "Apple", "pear", ""]);
        assert!(sort(&mut d, StoryId::Main, 0, 3, 0, true));
        d.commit(Default::default());
        assert_eq!(texts(&d), ["pear", "Apple", "10 apples", "2 apples", ""]);
        d.undo();
        assert_eq!(texts(&d), ["2 apples", "10 apples", "Apple", "pear", ""]);
    }

    #[test]
    fn sorts_table_rows_by_column() {
        let mut d = doc(&[""]);
        d.insert_table(StoryId::Main, p(0, 0), 3, 2, 2880);
        let cells = ["b", "1", "a", "3", "c", "2"];
        let mut k = 0;
        for i in 0..d.main.paras.len() {
            if d.main.paras[i].mark == Mark::CellEnd {
                let chp = d.main.paras[i].mark_chp;
                d.main.paras[i].insert(0, cells[k], chp);
                k += 1;
            }
        }
        assert!(sort(&mut d, StoryId::Main, 0, 0, 1, false));
        let firsts: Vec<&str> = d
            .main
            .paras
            .iter()
            .filter(|p| p.mark == Mark::CellEnd)
            .map(|p| p.text.as_str())
            .collect();
        assert_eq!(firsts, ["b", "1", "c", "2", "a", "3"]);
    }

    #[test]
    fn calculates() {
        assert_eq!(calculate("2 + 3 * 4").as_deref(), Some("14"));
        assert_eq!(calculate("10\n20\n30").as_deref(), Some("60"));
        assert_eq!(calculate("$1,200.50 - $200").as_deref(), Some("$1,000.50"));
        assert_eq!(calculate("(5) + 10").as_deref(), Some("5"));
        assert_eq!(calculate("(2 + 3) * 2").as_deref(), Some("10"));
        assert_eq!(calculate("2^10").as_deref(), Some("1024"));
        assert_eq!(calculate("50% * 30").as_deref(), Some("15"));
        assert_eq!(calculate("10 / 4").as_deref(), Some("2.50"));
        assert_eq!(calculate("apples").as_deref(), None);
        assert_eq!(calculate("1 / 0").as_deref(), None);
    }

    #[test]
    fn finds_leading_numbers() {
        assert_eq!(leading_number("1.\tOne"), Some((3, "1.")));
        assert_eq!(leading_number("(a) Two"), Some((4, "(a)")));
        assert_eq!(leading_number("2.3\tThree"), Some((4, "2.3")));
        assert_eq!(leading_number("IV) Four"), Some((4, "IV)")));
        assert_eq!(leading_number("I think"), None);
        assert_eq!(leading_number("Hello"), None);
        assert_eq!(leading_number("12 apples"), None);
    }

    #[test]
    fn renumbers() {
        let mut d = doc(&["One", "", "3.\tTwo", "Three"]);
        let mut opts = Renumber {
            only_numbered: false,
            start: 1,
            format: String::new(),
            numbers: Numbers::Plain,
        };
        renumber(&mut d, StoryId::Main, 0, 3, &opts);
        assert_eq!(texts(&d), ["1.\tOne", "", "2.\tTwo", "3.\tThree"]);
        opts.format = "(a)".into();
        opts.start = 2;
        renumber(&mut d, StoryId::Main, 0, 3, &opts);
        assert_eq!(texts(&d), ["(b)\tOne", "", "(c)\tTwo", "(d)\tThree"]);
        opts.numbers = Numbers::Remove;
        renumber(&mut d, StoryId::Main, 0, 3, &opts);
        assert_eq!(texts(&d), ["One", "", "Two", "Three"]);
    }

    #[test]
    fn renumbers_by_outline_level() {
        let mut d = doc(&["A", "B", "C", "D"]);
        d.main.paras[1].pap.left = 720;
        d.main.paras[2].pap.left = 720;
        let opts = Renumber {
            only_numbered: false,
            start: 1,
            format: String::new(),
            numbers: Numbers::Outline,
        };
        renumber(&mut d, StoryId::Main, 0, 3, &opts);
        assert_eq!(texts(&d), ["1.\tA", "1.1.\tB", "1.2.\tC", "2.\tD"]);
    }

    #[test]
    fn bullets() {
        let mut d = doc(&["one", "two"]);
        bullet(&mut d, StoryId::Main, 0, 1);
        bullet(&mut d, StoryId::Main, 0, 1);
        assert_eq!(texts(&d), ["•\tone", "•\ttwo"]);
        assert_eq!(d.main.paras[0].pap.left, 360);
        assert_eq!(d.main.paras[0].pap.first, -360);
    }

    #[test]
    fn list_marks_come_and_go_with_their_indent() {
        assert_eq!(list_mark_len("•\tone"), Some(4));
        assert_eq!(list_mark_len("-\tone"), Some(2));
        assert_eq!(list_mark_len("-one"), None);
        assert_eq!(list_mark_len("\tone"), None);
        assert_eq!(list_mark_len(""), None);
        assert_eq!(numbered_mark_len("1.\tone"), Some(3));
        assert_eq!(numbered_mark_len("12)\tone"), Some(4));
        assert_eq!(numbered_mark_len("1. one"), None);
        assert_eq!(numbered_mark_len(".\tone"), None);
        assert_eq!(next_list_mark("9.\t"), "10.\t");
        assert_eq!(next_list_mark("3)\t"), "4)\t");
        assert_eq!(next_list_mark("•\t"), "•\t");
        let mut d = doc(&["* one"]);
        start_list(&mut d, StoryId::Main, 0, 2, '•');
        assert_eq!(texts(&d), ["•\tone"]);
        assert_eq!(d.main.paras[0].pap.first, -360);
        assert_eq!(end_list(&mut d, StoryId::Main, 0), 4);
        assert_eq!(texts(&d), ["one"]);
        assert_eq!(
            (d.main.paras[0].pap.left, d.main.paras[0].pap.first),
            (0, 0)
        );
    }

    #[test]
    fn drop_caps() {
        let mut d = doc(&["Once upon a time"]);
        assert!(drop_cap(
            &mut d,
            StoryId::Main,
            0,
            1,
            3,
            320,
            0,
            false,
            false
        ));
        assert_eq!(texts(&d), ["O", "nce upon a time"]);
        let cap = &d.main.paras[0];
        assert_eq!(cap.chp_of_char(0).size, 96);
        assert_eq!(cap.pap.frame.unwrap().drop_lines, 3);
        // Not twice: the capital is framed now.
        assert!(!drop_cap(
            &mut d,
            StoryId::Main,
            0,
            1,
            3,
            320,
            0,
            false,
            false
        ));
    }
}
