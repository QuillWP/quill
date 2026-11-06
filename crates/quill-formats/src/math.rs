//! Equations as linear text. Quill has no equation layout, so a Word
//! (OMML) or OpenDocument (MathML) equation comes in as one line of text
//! that reads the same way: "x = (−b ± √(b²−4ac))/2a". Superscripts and
//! subscripts are raised and lowered the way Quill raises any text, and
//! everything else is spelled out with brackets, / and √.

use crate::xml::Element;
use quill_core::para::{Paragraph, chars};
use quill_core::props::{CharProps, Underline};

/// The most text one equation (or one part of it) is written as. Real
/// equations are far shorter; the limit keeps a hostile file's nested
/// equation from being copied level by level without end.
const MAX_TEXT: usize = 4096;

/// Append a Word equation (`m:oMath` or `m:oMathPara`) to `p`.
pub(crate) fn omml(e: &Element, p: &mut Paragraph, chp: CharProps) {
    let mut out = Out::new(chp);
    out.omml(e, chp, false);
    out.finish(p);
}

/// Append a MathML equation (`math`) to `p`.
pub(crate) fn mathml(e: &Element, p: &mut Paragraph, chp: CharProps) {
    let mut out = Out::new(chp);
    out.mathml(e, chp);
    out.finish(p);
}

/// The equation being written: its text, and its runs as (length,
/// format), kept apart from a Paragraph so that copying parts about stays
/// cheap.
struct Out {
    text: String,
    runs: Vec<(usize, CharProps)>,
    /// The format of the text around the equation.
    chp: CharProps,
}

impl Out {
    fn new(chp: CharProps) -> Out {
        Out {
            text: String::new(),
            runs: Vec::new(),
            chp,
        }
    }

    fn push(&mut self, text: &str, chp: CharProps) {
        if text.is_empty() || self.text.len() + text.len() > MAX_TEXT {
            return;
        }
        self.text.push_str(text);
        match self.runs.last_mut() {
            Some((len, c)) if *c == chp => *len += text.len(),
            _ => self.runs.push((text.len(), chp)),
        }
    }

    /// Each run's text and format.
    fn pieces(&self) -> impl Iterator<Item = (&str, CharProps)> {
        let mut at = 0;
        self.runs.iter().map(move |&(len, chp)| {
            let piece = &self.text[at..at + len];
            at += len;
            (piece, chp)
        })
    }

    /// The format of the last character.
    fn last_chp(&self) -> CharProps {
        self.runs.last().map_or(self.chp, |r| r.1)
    }

    /// Append what another `Out` wrote.
    fn splice(&mut self, other: Out) {
        for (text, chp) in other.pieces() {
            self.push(text, chp);
        }
    }

    /// Append `part`, in brackets unless it reads as one thing already.
    fn grouped(&mut self, part: Out, chp: CharProps) {
        if atomic(&part.text) {
            self.splice(part);
        } else {
            self.push("(", chp);
            self.splice(part);
            self.push(")", chp);
        }
    }

    fn finish(self, p: &mut Paragraph) {
        for (text, chp) in self.pieces() {
            let at = p.len();
            p.insert(at, text, chp);
        }
    }

    /// Write `text`, spacing out relations such as = and ≤ in the main
    /// line (not in scripts, where the space would look loose).
    fn text(&mut self, text: &str, chp: CharProps) {
        if chp.position != 0 || !text.chars().any(relation) {
            return self.push(text, chp);
        }
        let mut spaced = String::new();
        for c in text.chars() {
            if relation(c) {
                if !spaced.is_empty() && !spaced.ends_with(' ') {
                    spaced.push(' ');
                }
                spaced.push(c);
                spaced.push(' ');
            } else if !(c == ' ' && spaced.ends_with(' ')) {
                spaced.push(c);
            }
        }
        if self.text.is_empty() || self.text.ends_with(' ') {
            spaced = spaced.trim_start().to_string();
        } else if !spaced.starts_with(' ') && text.starts_with(relation) {
            spaced.insert(0, ' ');
        }
        self.push(&spaced, chp)
    }

    // Word's OMML.

    /// Write the children of `e` in order.
    fn omml_row(&mut self, e: &Element, chp: CharProps, upright: bool) {
        for c in e.elements() {
            self.omml(c, chp, upright);
        }
    }

    /// `e`'s child `name` written apart, to be placed by the caller.
    fn omml_part(&self, e: &Element, name: &str, chp: CharProps, upright: bool) -> Out {
        let mut out = Out::new(chp);
        if let Some(c) = e.child(name) {
            out.omml_row(c, chp, upright);
        }
        out
    }

    fn omml(&mut self, e: &Element, chp: CharProps, upright: bool) {
        // A property of the element's own *Pr child, such as m:chr.
        let prop = |pr: &str, name: &str| -> Option<String> {
            e.child(pr)
                .and_then(|p| p.child(name))
                .map(|x| x.attr("val").unwrap_or_default().to_string())
        };
        match e.name.as_str() {
            "oMathPara" => {
                for (k, m) in e.children_named("oMath").enumerate() {
                    if k > 0 {
                        self.push(&chars::LINE_BREAK.to_string(), chp);
                    }
                    self.omml(m, chp, upright);
                }
            }
            "r" => {
                let style = e
                    .child("rPr")
                    .and_then(|r| r.child("sty"))
                    .and_then(|s| s.attr("val"));
                let plain = upright
                    || style == Some("p")
                    || style == Some("b")
                    || e.child("rPr").and_then(|r| r.child("nor")).is_some();
                let bold = matches!(style, Some("b" | "bi"));
                let text: String = e.children_named("t").map(Element::text).collect();
                // Letters are italic in an equation, digits and signs
                // upright, as Word sets them.
                let mut rest = text.as_str();
                while let Some(first) = rest.chars().next() {
                    let letters = first.is_alphabetic();
                    let end = rest
                        .char_indices()
                        .find(|&(_, c)| c.is_alphabetic() != letters)
                        .map_or(rest.len(), |(at, _)| at);
                    let c = CharProps {
                        italic: chp.italic || (letters && !plain),
                        bold: chp.bold || bold,
                        ..chp
                    };
                    self.text(&rest[..end], c);
                    rest = &rest[end..];
                }
            }
            "f" => {
                let kind = prop("fPr", "type");
                let num = self.omml_part(e, "num", chp, upright);
                let den = self.omml_part(e, "den", chp, upright);
                self.grouped(num, chp);
                // A fraction without a bar is a binomial coefficient.
                let sign = if kind.as_deref() == Some("noBar") {
                    " "
                } else {
                    "/"
                };
                self.push(sign, chp);
                self.grouped(den, chp);
            }
            "sSup" | "sSub" | "sSubSup" | "sPre" => {
                let pre = e.name == "sPre";
                if !pre {
                    self.omml_row_of(e, "e", chp, upright);
                }
                if e.name != "sSup" {
                    self.omml_row_of(e, "sub", lower(chp), upright);
                }
                if e.name != "sSub" {
                    self.omml_row_of(e, "sup", raise(chp), upright);
                }
                if pre {
                    self.omml_row_of(e, "e", chp, upright);
                }
            }
            "rad" => {
                let hidden = e
                    .child("radPr")
                    .and_then(|p| p.child("degHide"))
                    .is_some_and(Element::on);
                let deg = self.omml_part(e, "deg", chp, upright);
                let body = self.omml_part(e, "e", chp, upright);
                self.root(deg, hidden, body, chp);
            }
            "d" => {
                let open = prop("dPr", "begChr").unwrap_or_else(|| "(".into());
                let close = prop("dPr", "endChr").unwrap_or_else(|| ")".into());
                let sep = prop("dPr", "sepChr").unwrap_or_else(|| "|".into());
                self.push(&open, chp);
                for (k, part) in e.children_named("e").enumerate() {
                    if k > 0 {
                        self.push(&sep, chp);
                    }
                    self.omml_row(part, chp, upright);
                }
                self.push(&close, chp);
            }
            "nary" => {
                let sign = prop("naryPr", "chr").unwrap_or_else(|| "\u{222B}".into());
                let hide = |name: &str| {
                    e.child("naryPr")
                        .and_then(|p| p.child(name))
                        .is_some_and(Element::on)
                };
                self.push(&sign, chp);
                if !hide("subHide") {
                    self.omml_row_of(e, "sub", lower(chp), upright);
                }
                if !hide("supHide") {
                    self.omml_row_of(e, "sup", raise(chp), upright);
                }
                self.omml_row_of(e, "e", chp, upright);
            }
            "func" => {
                self.omml_row_of(e, "fName", chp, true);
                let body = self.omml_part(e, "e", chp, upright);
                self.applied(body, chp);
            }
            "acc" => {
                let mark = prop("accPr", "chr").unwrap_or_else(|| "\u{0302}".into());
                let body = self.omml_part(e, "e", chp, upright);
                self.accent(body, &mark, chp);
            }
            "bar" => {
                let top = prop("barPr", "pos").as_deref() == Some("top");
                let body = self.omml_part(e, "e", chp, upright);
                if top {
                    self.accent(body, "\u{0305}", chp);
                } else {
                    for (text, c) in body.pieces() {
                        let under = CharProps {
                            underline: Underline::Single,
                            ..c
                        };
                        self.push(text, under);
                    }
                }
            }
            "limLow" | "limUpp" => {
                self.omml_row_of(e, "e", chp, upright);
                let c = if e.name == "limLow" {
                    lower(chp)
                } else {
                    raise(chp)
                };
                self.omml_row_of(e, "lim", c, upright);
            }
            "m" => {
                for (k, row) in e.children_named("mr").enumerate() {
                    if k > 0 {
                        self.push("; ", chp);
                    }
                    for (j, cell) in row.children_named("e").enumerate() {
                        if j > 0 {
                            self.push(", ", chp);
                        }
                        self.omml_row(cell, chp, upright);
                    }
                }
            }
            "eqArr" => {
                for (k, row) in e.children_named("e").enumerate() {
                    if k > 0 {
                        self.push(&chars::LINE_BREAK.to_string(), chp);
                    }
                    self.omml_row(row, chp, upright);
                }
            }
            "phant" => {
                let shown = e
                    .child("phantPr")
                    .and_then(|p| p.child("show"))
                    .is_none_or(Element::on);
                if shown {
                    self.omml_row_of(e, "e", chp, upright);
                }
            }
            // Deleted text and properties are not written.
            "del" => {}
            name if name.ends_with("Pr") => {}
            // Everything else (oMath, e, box, borderBox, groupChr, ins and
            // so on) is its children in order.
            _ => self.omml_row(e, chp, upright),
        }
    }

    fn omml_row_of(&mut self, e: &Element, name: &str, chp: CharProps, upright: bool) {
        if let Some(c) = e.child(name) {
            self.omml_row(c, chp, upright);
        }
    }

    // Shared by both formats.

    /// √ before `body`, with its degree raised before it unless hidden.
    fn root(&mut self, deg: Out, hidden: bool, body: Out, chp: CharProps) {
        match deg.text.trim() {
            _ if hidden => self.push("\u{221A}", chp),
            "" | "2" => self.push("\u{221A}", chp),
            "3" => self.push("\u{221B}", chp),
            "4" => self.push("\u{221C}", chp),
            _ => {
                for (text, c) in deg.pieces() {
                    self.push(text, raise(c));
                }
                self.push("\u{221A}", chp);
            }
        }
        self.grouped(body, chp);
    }

    /// A function's argument after its name: "sin x", "sin(x+y)".
    fn applied(&mut self, body: Out, chp: CharProps) {
        let bracketed = body.text.starts_with(['(', '[', '{']);
        if !bracketed && atomic(&body.text) {
            self.push(" ", chp);
        }
        if bracketed {
            self.splice(body);
        } else {
            self.grouped(body, chp);
        }
    }

    /// `body` with an accent such as a hat or a bar over it: a combining
    /// mark after one character, or after the brackets round several.
    fn accent(&mut self, body: Out, mark: &str, chp: CharProps) {
        let mark = combining(mark);
        if body.text.chars().count() == 1 {
            let c = body.last_chp();
            self.splice(body);
            self.push(mark, c);
        } else {
            self.push("(", chp);
            self.splice(body);
            self.push(")", chp);
            self.push(mark, chp);
        }
    }

    // MathML, as OpenDocument formulas keep it.

    fn mathml_row<'e>(&mut self, children: impl Iterator<Item = &'e Element>, chp: CharProps) {
        for c in children {
            self.mathml(c, chp);
        }
    }

    fn mathml_part(&self, e: Option<&Element>, chp: CharProps) -> Out {
        let mut out = Out::new(chp);
        if let Some(e) = e {
            out.mathml(e, chp);
        }
        out
    }

    fn mathml(&mut self, e: &Element, chp: CharProps) {
        let kids: Vec<&Element> = e.elements().collect();
        let nth = |k: usize| kids.get(k).copied();
        match e.name.as_str() {
            // The equation itself; the StarMath source beside it is not
            // shown.
            "semantics" => {
                if let Some(first) = nth(0) {
                    self.mathml(first, chp);
                }
            }
            "annotation" | "annotation-xml" | "mphantom" | "none" | "mprescripts" => {}
            "mi" => {
                let text = e.text();
                let text = text.trim();
                // A one-letter identifier is italic unless marked normal.
                let italic = match e.attr("mathvariant") {
                    Some(v) => v.contains("italic"),
                    None => text.chars().count() == 1 && text.chars().all(char::is_alphabetic),
                };
                let bold = e.attr("mathvariant").is_some_and(|v| v.contains("bold"));
                let c = CharProps {
                    italic: chp.italic || italic,
                    bold: chp.bold || bold,
                    ..chp
                };
                self.text(text, c);
            }
            "mn" | "mo" | "mtext" | "ms" => {
                let text = e.text();
                let text = if e.name == "mtext" {
                    text.as_str()
                } else {
                    text.trim()
                };
                self.text(text, chp);
            }
            "mspace" => self.push(" ", chp),
            "mfrac" => {
                let num = self.mathml_part(nth(0), chp);
                let den = self.mathml_part(nth(1), chp);
                self.grouped(num, chp);
                let bar = e.attr("linethickness").is_none_or(|t| t.trim() != "0");
                self.push(if bar { "/" } else { " " }, chp);
                self.grouped(den, chp);
            }
            "msup" | "msub" | "msubsup" => {
                self.mathml_row(nth(0).into_iter(), chp);
                match e.name.as_str() {
                    "msup" => self.mathml_row(nth(1).into_iter(), raise(chp)),
                    "msub" => self.mathml_row(nth(1).into_iter(), lower(chp)),
                    _ => {
                        self.mathml_row(nth(1).into_iter(), lower(chp));
                        self.mathml_row(nth(2).into_iter(), raise(chp));
                    }
                }
            }
            "munder" | "mover" | "munderover" => {
                let accent = e.attr("accent") == Some("true")
                    || nth(1).is_some_and(|o| o.name == "mo" && accent_mark(o.text().trim()));
                if e.name == "mover" && accent {
                    let body = self.mathml_part(nth(0), chp);
                    let mark = nth(1).map(Element::text).unwrap_or_default();
                    self.accent(body, mark.trim(), chp);
                    return;
                }
                self.mathml_row(nth(0).into_iter(), chp);
                match e.name.as_str() {
                    "munder" => self.mathml_row(nth(1).into_iter(), lower(chp)),
                    "mover" => self.mathml_row(nth(1).into_iter(), raise(chp)),
                    _ => {
                        self.mathml_row(nth(1).into_iter(), lower(chp));
                        self.mathml_row(nth(2).into_iter(), raise(chp));
                    }
                }
            }
            "msqrt" => {
                let mut body = Out::new(chp);
                body.mathml_row(kids.iter().copied(), chp);
                self.root(Out::new(chp), true, body, chp);
            }
            "mroot" => {
                let body = self.mathml_part(nth(0), chp);
                let deg = self.mathml_part(nth(1), chp);
                self.root(deg, false, body, chp);
            }
            "mfenced" => {
                let open = e.attr("open").unwrap_or("(");
                let close = e.attr("close").unwrap_or(")");
                let seps: Vec<char> = e
                    .attr("separators")
                    .unwrap_or(",")
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect();
                self.push(open, chp);
                for (k, c) in kids.iter().enumerate() {
                    if k > 0 && !seps.is_empty() {
                        let sep = seps[(k - 1).min(seps.len() - 1)];
                        self.push(&sep.to_string(), chp);
                    }
                    self.mathml(c, chp);
                }
                self.push(close, chp);
            }
            "mtable" => {
                for (k, row) in kids.iter().enumerate() {
                    if k > 0 {
                        self.push("; ", chp);
                    }
                    let cells = row.elements().filter(|c| c.name == "mtd");
                    for (j, cell) in cells.enumerate() {
                        if j > 0 {
                            self.push(", ", chp);
                        }
                        self.mathml(cell, chp);
                    }
                }
            }
            // math, mrow, mstyle, mpadded, menclose, mtd and anything
            // else: the children in order.
            _ => self.mathml_row(kids.iter().copied(), chp),
        }
    }
}

/// Raised for a superscript: a third of the size up, two thirds as big,
/// as Quill reads Word's superscripts.
fn raise(c: CharProps) -> CharProps {
    CharProps {
        position: c.position + crate::superscript_rise(c.size),
        size: (c.size * 2).div_ceil(3).max(8),
        ..c
    }
}

fn lower(c: CharProps) -> CharProps {
    CharProps {
        position: c.position - crate::subscript_drop(c.size),
        size: (c.size * 2).div_ceil(3).max(8),
        ..c
    }
}

/// Text that reads as one thing without brackets: one character, a
/// number or word such as "2a", "1.5" or "n!", or text already in one pair
/// of brackets.
fn atomic(text: &str) -> bool {
    let text = text.trim();
    let term = |c: char| c.is_alphanumeric() || matches!(c, '.' | '!' | '\u{2032}' | '\u{2033}');
    if text.chars().count() <= 1 || text.chars().all(term) {
        return true;
    }
    let (Some(open), Some(close)) = (text.chars().next(), text.chars().last()) else {
        return true;
    };
    let pair = matches!((open, close), ('(', ')') | ('[', ']') | ('{', '}'));
    if !pair {
        return false;
    }
    // "(a)+(b)" starts and ends with brackets but is not one group.
    let mut depth = 0;
    let n = text.chars().count();
    for (k, c) in text.chars().enumerate() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 && k + 1 < n {
                return false;
            }
        }
    }
    true
}

fn relation(c: char) -> bool {
    matches!(
        c,
        '=' | '<'
            | '>'
            | '\u{2260}'
            | '\u{2264}'
            | '\u{2265}'
            | '\u{2248}'
            | '\u{2261}'
            | '\u{221D}'
            | '\u{2192}'
            | '\u{21D2}'
            | '\u{21D4}'
    )
}

/// A spacing accent as the combining mark that goes after a character.
fn combining(mark: &str) -> &str {
    match mark {
        "^" | "\u{02C6}" => "\u{0302}",
        "~" | "\u{02DC}" => "\u{0303}",
        "\u{00AF}" | "\u{203E}" | "\u{2015}" | "_" => "\u{0305}",
        "\u{02D9}" | "." => "\u{0307}",
        "\u{00A8}" => "\u{0308}",
        "\u{2192}" | "\u{20D7}" => "\u{20D7}",
        "\u{02C7}" => "\u{030C}",
        "\u{00B4}" => "\u{0301}",
        "`" => "\u{0300}",
        other => other,
    }
}

fn accent_mark(text: &str) -> bool {
    combining(text) != text || text.chars().all(|c| ('\u{0300}'..='\u{036F}').contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml;
    use quill_core::props::ParaProps;

    fn plain(p: &Paragraph) -> String {
        p.text.clone()
    }

    fn from_omml(body: &str) -> Paragraph {
        let e = xml::parse(format!(r#"<m:oMath xmlns:m="m">{body}</m:oMath>"#).as_bytes()).unwrap();
        let mut p = Paragraph::new(ParaProps::default(), CharProps::default());
        omml(&e, &mut p, CharProps::default());
        p
    }

    fn r(t: &str) -> String {
        format!("<m:r><m:t>{t}</m:t></m:r>")
    }

    #[test]
    fn the_quadratic_formula_reads_as_one_line() {
        let p = from_omml(&format!(
            "{x}{eq}<m:f><m:num>{minus}{b}{pm}<m:rad><m:radPr><m:degHide m:val=\"1\"/></m:radPr><m:deg/><m:e><m:sSup><m:e>{b}</m:e><m:sup>{two}</m:sup></m:sSup>{four}</m:e></m:rad></m:num><m:den>{twoa}</m:den></m:f>",
            x = r("x"),
            eq = r("="),
            minus = r("\u{2212}"),
            b = r("b"),
            pm = r("\u{00B1}"),
            two = r("2"),
            four = r("\u{2212}4ac"),
            twoa = r("2a"),
        ));
        assert_eq!(
            plain(&p),
            "x = (\u{2212}b\u{00B1}\u{221A}(b2\u{2212}4ac))/2a"
        );
        // The 2 of b² is raised and smaller; x is italic, = is not.
        let two = p.text.find("b2").unwrap() + 1;
        let c = p.chp_of_char(two);
        assert!(c.position > 0 && c.size < 24);
        assert!(p.chp_of_char(0).italic);
        assert!(!p.chp_of_char(2).italic);
    }

    #[test]
    fn sums_functions_and_delimiters() {
        let p = from_omml(&format!(
            "<m:nary><m:naryPr><m:chr m:val=\"\u{2211}\"/></m:naryPr><m:sub>{i}</m:sub><m:sup>{n}</m:sup><m:e>{k}</m:e></m:nary>{plus}<m:func><m:fName>{sin}</m:fName><m:e>{x}</m:e></m:func><m:d><m:dPr><m:begChr m:val=\"[\"/><m:endChr m:val=\"]\"/></m:dPr><m:e>{a}</m:e><m:e>{b}</m:e></m:d>",
            i = r("i=1"),
            n = r("n"),
            k = r("i"),
            plus = r("+"),
            sin = r("sin"),
            x = r("x"),
            a = r("a"),
            b = r("b"),
        ));
        assert_eq!(plain(&p), "\u{2211}i=1ni+sin x[a|b]");
        // "sin" is a function name, so upright.
        let s = p.text.find("sin").unwrap();
        assert!(!p.chp_of_char(s).italic);
    }

    #[test]
    fn mathml_from_an_opendocument_formula() {
        let e = xml::parse(
            br#"<math xmlns="http://www.w3.org/1998/Math/MathML"><semantics><mrow><mi>E</mi><mo>=</mo><mi>m</mi><msup><mi>c</mi><mn>2</mn></msup><mo>+</mo><mfrac><mrow><mi>a</mi><mo>+</mo><mi>b</mi></mrow><mn>2</mn></mfrac><mo>+</mo><msqrt><mi>x</mi></msqrt><mover accent="true"><mi>v</mi><mo>&#x2192;</mo></mover></mrow><annotation encoding="StarMath 5.0">E = m c^2 + {a+b} over 2 + sqrt x + vec v</annotation></semantics></math>"#,
        )
        .unwrap();
        let mut p = Paragraph::new(ParaProps::default(), CharProps::default());
        mathml(&e, &mut p, CharProps::default());
        assert_eq!(plain(&p), "E = mc2+(a+b)/2+\u{221A}xv\u{20D7}");
    }

    #[test]
    fn a_huge_equation_is_cut_short_quickly() {
        let mut body = "<m:r><m:t>x+</m:t></m:r>".repeat(50_000);
        for _ in 0..80 {
            body =
                format!("<m:f><m:num>{body}</m:num><m:den><m:r><m:t>y</m:t></m:r></m:den></m:f>");
        }
        let start = std::time::Instant::now();
        let p = from_omml(&body);
        assert!(p.len() <= MAX_TEXT);
        assert!(p.text.starts_with("((((") && p.text.contains("x+x+"));
        assert!(start.elapsed().as_secs() < 5);
    }

    #[test]
    fn grouping() {
        assert!(atomic("2a"));
        assert!(atomic("2!"));
        assert!(atomic("(a+b)"));
        assert!(!atomic("(a)+(b)"));
        assert!(!atomic("a+b"));
    }
}
