//! HTML import, for files and the clipboard: text, formatting, headings,
//! lists, tables, embedded pictures and equations. Bounded in size and
//! depth; it never runs scripts and never fetches anything a page links
//! to (style sheets, pictures, frames), which the import report names.

use crate::report::{self as notes, ImportReport};
use crate::xml::{Element, Node};
use quill_core::para::{Mark, Paragraph, chars};
use quill_core::props::{
    Align, BorderLine, BorderStyle, Borders, CellProps, CharProps, LineSpacing, ParaProps, Rgb,
    RowHeight, RowProps, TABLE_GAP, Underline,
};
use quill_core::{Document, Object};

/// A width or height: absolute, or a percentage of what holds it.
#[derive(Clone, Copy, Debug)]
enum Size {
    Twips(i32),
    Percent(f64),
}

/// The format an element gives what is in it.
#[derive(Clone)]
struct Frame {
    chp: CharProps,
    pap: ParaProps,
    pre: bool,
    skip: bool,
    /// A border round the element itself (for table cells); not inherited.
    border: bool,
    width: Option<Size>,
    height: Option<i32>,
    /// list-style-type: none.
    list_none: bool,
    /// The parent's color, for `color: inherit`.
    parent_color: quill_core::Rgb,
}

fn append(p: &mut Paragraph, text: &str, chp: CharProps) {
    p.text.push_str(text);
    if let Some(last) = p.runs.last_mut().filter(|r| r.chp == chp) {
        last.len += text.len();
    } else if !text.is_empty() {
        p.runs.push(quill_core::para::Run {
            len: text.len(),
            chp,
        });
    }
}

fn entities(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|&n| n <= 32) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let value = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            "copy" => Some('©'),
            "reg" => Some('®'),
            "trade" => Some('™'),
            "ndash" => Some('\u{2013}'),
            "mdash" => Some('\u{2014}'),
            "hellip" => Some('…'),
            "lsquo" => Some('‘'),
            "rsquo" => Some('’'),
            "ldquo" => Some('“'),
            "rdquo" => Some('”'),
            "bull" => Some('•'),
            "euro" => Some('€'),
            "shy" => Some('\u{ad}'),
            n if n.starts_with("#x") || n.starts_with("#X") => u32::from_str_radix(&n[2..], 16)
                .ok()
                .and_then(char::from_u32),
            n if n.starts_with('#') => n[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        if let Some(c) = value {
            out.push(c);
        } else if let Some(value) = quick_xml::escape::resolve_html5_entity(name) {
            out.push_str(value);
        } else {
            out.push_str(&rest[..=end]);
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn attributes(mut s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    while !s.trim_start().is_empty() {
        s = s.trim_start();
        let end = s
            .find(|c: char| c.is_ascii_whitespace() || matches!(c, '=' | '/' | '>'))
            .unwrap_or(s.len());
        if end == 0 {
            s = &s[1..];
            continue;
        }
        let name = s[..end].to_ascii_lowercase();
        s = s[end..].trim_start();
        let mut value = String::new();
        if let Some(tail) = s.strip_prefix('=') {
            s = tail.trim_start();
            if let Some(quote @ ('\'' | '"')) = s.chars().next() {
                s = &s[1..];
                let end = s.find(quote).unwrap_or(s.len());
                value = entities(&s[..end]);
                s = &s[end..];
                if s.starts_with(quote) {
                    s = &s[1..];
                }
            } else {
                let end = s.find(char::is_whitespace).unwrap_or(s.len());
                value = entities(&s[..end]);
                s = &s[end..];
            }
        }
        out.push((name, value));
    }
    out
}

fn length(s: &str, size: u16) -> Option<f64> {
    let s = s.trim().to_ascii_lowercase();
    let (n, factor) = if let Some(n) = s.strip_suffix("rem") {
        (n, 12.)
    } else if let Some(n) = s.strip_suffix("em") {
        (n, size as f64 / 2.)
    } else if let Some(n) = s.strip_suffix('%') {
        (n, size as f64 / 200.)
    } else if let Some(n) = s.strip_suffix("px") {
        (n, 0.75)
    } else if let Some(n) = s.strip_suffix("pt") {
        (n, 1.)
    } else if let Some(n) = s.strip_suffix("in") {
        (n, 72.)
    } else if let Some(n) = s.strip_suffix("cm") {
        (n, 72. / 2.54)
    } else if let Some(n) = s.strip_suffix("mm") {
        (n, 72. / 25.4)
    } else {
        (s.as_str(), 0.75)
    };
    let n = n.trim().parse::<f64>().ok()? * factor;
    n.is_finite().then_some(n)
}

fn color(s: &str) -> Option<Rgb> {
    let s = s.trim().to_ascii_lowercase();
    if let Some(hex) = s.strip_prefix('#') {
        let value = match hex.len() {
            3 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
            6 => hex.to_string(),
            _ => return None,
        };
        return u32::from_str_radix(&value, 16).ok().map(Rgb);
    }
    if (s.starts_with("rgb(") || s.starts_with("rgba(")) && s.ends_with(')') {
        let start = s.find('(')? + 1;
        let values: Vec<_> = s[start..s.len() - 1]
            .split([',', ' ', '/'])
            .filter(|s| !s.is_empty())
            .collect();
        if values.len() < 3 {
            return None;
        }
        let mut rgb = 0;
        for value in &values[..3] {
            let (n, factor) = value.strip_suffix('%').map_or((*value, 1.), |n| (n, 2.55));
            let n = n.parse::<f64>().ok()? * factor;
            if !n.is_finite() {
                return None;
            }
            rgb = (rgb << 8) | n.round().clamp(0., 255.) as u32;
        }
        return Some(Rgb(rgb));
    }
    Some(Rgb(match s.as_str() {
        "black" => 0,
        "white" => 0xffffff,
        "red" => 0xff0000,
        "green" => 0x008000,
        "blue" => 0x0000ff,
        "yellow" => 0xffff00,
        "cyan" | "aqua" => 0x00ffff,
        "magenta" | "fuchsia" => 0xff00ff,
        // CSS standardizes both spellings.
        "gray" | "grey" => 0x808080,
        "silver" => 0xc0c0c0,
        "maroon" => 0x800000,
        "olive" => 0x808000,
        "purple" => 0x800080,
        "teal" => 0x008080,
        "navy" => 0x000080,
        "orange" => 0xffa500,
        "lime" => 0x00ff00,
        "rebeccapurple" => 0x663399,
        _ => return None,
    }))
}

/// Generic families, CSS-wide keywords and system aliases name no font.
fn generic_font(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "serif"
            | "sans-serif"
            | "monospace"
            | "cursive"
            | "fantasy"
            | "math"
            | "emoji"
            | "fangsong"
            | "system-ui"
            | "ui-serif"
            | "ui-sans-serif"
            | "ui-monospace"
            | "ui-rounded"
            | "blinkmacsystemfont"
            | "inherit"
            | "initial"
            | "unset"
            | "revert"
            | "revert-layer"
    )
}

/// The first real family in a CSS font list. Web pages often lead with
/// aliases such as -apple-system; those would become fonts nobody has.
fn font(doc: &mut Document, value: &str) -> Option<u16> {
    value
        .split(',')
        .map(|name| name.trim().trim_matches(['\'', '"']).trim())
        .find(|name| {
            !name.is_empty() && !name.starts_with('-') && !name.contains('(') && !generic_font(name)
        })
        .map(|name| doc.font_id(name))
}

fn css(doc: &mut Document, frame: &mut Frame, style: &str) {
    let mut declarations = Vec::new();
    let mut quote = None;
    let mut start = 0;
    for (at, c) in style.char_indices() {
        if matches!(c, '\'' | '"') {
            if quote == Some(c) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(c);
            }
        }
        if c == ';' && quote.is_none() {
            declarations.push(&style[start..at]);
            start = at + 1;
        }
    }
    declarations.push(&style[start..]);
    for declaration in declarations {
        let Some((name, value)) = declaration.split_once(':') else {
            continue;
        };
        let value = value
            .trim()
            .strip_suffix("!important")
            .unwrap_or(value.trim())
            .trim();
        let lower = value.to_ascii_lowercase();
        let c = &mut frame.chp;
        let p = &mut frame.pap;
        let twips =
            || length(value, c.size).map(|n| (n * 20.).round().clamp(-288_000., 288_000.) as i32);
        match name.trim().to_ascii_lowercase().as_str() {
            "font-family" => {
                if let Some(id) = font(doc, value) {
                    c.font = id;
                }
            }
            "font-size" => {
                if let Some(size) = length(value, c.size) {
                    c.size = (size * 2.).round().clamp(2., 3276.) as u16;
                }
            }
            "font-weight" => {
                c.bold = lower == "bold"
                    || lower == "bolder"
                    || lower.parse::<u16>().is_ok_and(|n| n >= 600)
            }
            "font-style" => c.italic = lower == "italic" || lower == "oblique",
            "color" => {
                if let Some(color) = color(value) {
                    c.color = color;
                } else if lower == "inherit" {
                    c.color = frame.parent_color;
                }
            }
            "text-decoration" | "text-decoration-line" => {
                c.underline = if lower.contains("underline") {
                    Underline::Single
                } else {
                    Underline::None
                };
                c.strike = lower.contains("line-through");
            }
            "text-decoration-style" if c.underline != Underline::None => {
                c.underline = match lower.as_str() {
                    "double" => Underline::Double,
                    "dotted" => Underline::Dotted,
                    _ => Underline::Single,
                }
            }
            "font-variant" | "font-variant-caps" => c.small_caps = lower == "small-caps",
            "text-transform" => c.all_caps = lower == "uppercase",
            "letter-spacing" => c.spacing = twips().unwrap_or(0),
            "vertical-align" => {
                c.position = match lower.as_str() {
                    "super" => c.size as i32 * 3,
                    "sub" => -(c.size as i32 * 3),
                    _ => twips().unwrap_or(0),
                }
            }
            "background-color" => {
                if lower == "transparent" {
                    p.shading_color = None;
                    p.shading = 0;
                } else if let Some(c) = color(value) {
                    p.shading_color = Some(c);
                    p.shading = 0;
                }
            }
            "text-align" => {
                p.align = match lower.as_str() {
                    "right" | "end" => Align::Right,
                    "center" => Align::Center,
                    "justify" => Align::Justify,
                    _ => Align::Left,
                }
            }
            // The shorthand, as Quill's own HTML and most browsers write it:
            // one to four values for top, right, bottom and left.
            "margin" => {
                let parts: Vec<&str> = value.split_ascii_whitespace().collect();
                let [top, right, bottom, left] = match parts.len() {
                    1 => [0, 0, 0, 0],
                    2 => [0, 1, 0, 1],
                    3 => [0, 1, 2, 1],
                    4 => [0, 1, 2, 3],
                    _ => continue,
                };
                let side = |i: usize| {
                    length(parts[i], c.size)
                        .map(|n| (n * 20.).round().clamp(-288_000., 288_000.) as i32)
                };
                if let Some(n) = side(top) {
                    p.before = n.max(0);
                }
                if let Some(n) = side(right) {
                    p.right = n;
                }
                if let Some(n) = side(bottom) {
                    p.after = n.max(0);
                }
                if let Some(n) = side(left) {
                    p.left = n;
                }
            }
            "margin-left" => {
                if let Some(n) = twips() {
                    p.left = n;
                }
            }
            "margin-right" => {
                if let Some(n) = twips() {
                    p.right = n;
                }
            }
            "margin-top" => {
                if let Some(n) = twips() {
                    p.before = n.max(0);
                }
            }
            "margin-bottom" => {
                if let Some(n) = twips() {
                    p.after = n.max(0);
                }
            }
            "text-indent" => {
                if let Some(n) = twips() {
                    p.first = n;
                }
            }
            "line-height" => {
                p.line = if lower == "normal" {
                    LineSpacing::Auto
                } else if let Ok(n) = lower.parse::<f64>() {
                    LineSpacing::Multiple((n * 240.).clamp(1., 65535.) as u16)
                } else {
                    twips()
                        .map(LineSpacing::AtLeast)
                        .unwrap_or(LineSpacing::Auto)
                }
            }
            "white-space" => frame.pre = lower.starts_with("pre") || lower == "break-spaces",
            "break-before" | "page-break-before" => {
                p.page_break_before = matches!(lower.as_str(), "page" | "always")
            }
            "break-after" | "page-break-after" => p.keep_with_next = lower == "avoid",
            "break-inside" | "page-break-inside" => p.keep_together = lower == "avoid",
            "width" | "height" => {
                let size = match lower.strip_suffix('%') {
                    Some(n) => n.trim().parse::<f64>().ok().map(Size::Percent),
                    None => twips().map(Size::Twips),
                };
                if name.trim().eq_ignore_ascii_case("width") {
                    frame.width = size;
                } else if let Some(Size::Twips(h)) = size {
                    frame.height = Some(h);
                }
            }
            "list-style" | "list-style-type" => frame.list_none = lower.contains("none"),
            "border" | "border-style" | "border-top" | "border-bottom" | "border-left"
            | "border-right" => {
                frame.border =
                    !(lower.contains("none") || lower.contains("hidden") || lower.starts_with('0'))
            }
            "display" if lower == "none" => frame.skip = true,
            "visibility" if lower == "hidden" => frame.skip = true,
            _ => {}
        }
    }
}

/// How much HTML one read takes on.
struct Limits {
    bytes: usize,
    depth: usize,
    tags: usize,
    /// "HTML clipboard content" or "The HTML file", for error messages.
    what: &'static str,
    /// Refuse a tag the input ends inside, rather than dropping it as a
    /// browser does at the end of a file.
    strict: bool,
}

const CLIPBOARD: Limits = Limits {
    bytes: 8 << 20,
    depth: 128,
    tags: 100_000,
    what: "HTML clipboard content",
    strict: true,
};

const FILE: Limits = Limits {
    bytes: 64 << 20,
    depth: 512,
    tags: 2_000_000,
    what: "The HTML file",
    strict: false,
};

/// Elements with no content and no end tag.
const VOID: &[&str] = &[
    "area", "base", "basefont", "br", "col", "embed", "frame", "hr", "img", "input", "keygen",
    "link", "meta", "param", "source", "track", "wbr",
];

/// Elements whose content is text up to their end tag, never markup.
const RAW: &[&str] = &[
    "script", "style", "textarea", "title", "xmp", "noembed", "noframes", "iframe",
];

/// Pop the elements from `at` up, each into its parent.
fn pop_to(stack: &mut Vec<Element>, at: usize) {
    while stack.len() > at.max(1) {
        let e = stack.pop().expect("checked");
        if let Some(parent) = stack.last_mut() {
            parent.children.push(Node::Element(e));
        }
    }
}

/// Close the nearest open element named in `names`, unless one of
/// `boundary` is nearer: a new list item closes the last one, but not one
/// in an outer list.
fn close_open(stack: &mut Vec<Element>, names: &[&str], boundary: &[&str]) {
    for k in (1..stack.len()).rev() {
        let name = stack[k].name.as_str();
        if names.contains(&name) {
            pop_to(stack, k);
            return;
        }
        if boundary.contains(&name) {
            return;
        }
    }
}

/// The ends HTML leaves out: a block closes an open paragraph, a list
/// item the last item, a cell the last cell, and so on.
fn close_implied(stack: &mut Vec<Element>, tag: &str) {
    const SCOPE: &[&str] = &[
        "table", "td", "th", "caption", "li", "dd", "dt", "button", "body",
    ];
    match tag {
        "li" => close_open(stack, &["li"], &["ul", "ol", "menu", "table", "td", "th"]),
        "dt" | "dd" => close_open(stack, &["dt", "dd"], &["dl", "table", "td", "th"]),
        "tr" => close_open(stack, &["tr"], &["table"]),
        "td" | "th" => close_open(stack, &["td", "th"], &["tr", "table"]),
        "thead" | "tbody" | "tfoot" => close_open(stack, &["thead", "tbody", "tfoot"], &["table"]),
        "option" => close_open(stack, &["option"], &["select", "datalist"]),
        _ => {}
    }
    if matches!(
        tag,
        "p" | "div"
            | "ul"
            | "ol"
            | "dl"
            | "li"
            | "table"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "pre"
            | "blockquote"
            | "hr"
            | "section"
            | "article"
            | "aside"
            | "header"
            | "footer"
            | "nav"
            | "main"
            | "form"
            | "figure"
            | "address"
            | "fieldset"
            | "details"
            | "menu"
            | "center"
    ) {
        close_open(stack, &["p"], SCOPE);
    }
}

/// HTML as a tree, the way a browser would build it from loose markup.
fn parse(html: &str, limits: &Limits) -> Result<Element, String> {
    let what = limits.what;
    let mut stack = vec![Element {
        name: "#root".into(),
        ..Element::default()
    }];
    let text = |stack: &mut Vec<Element>, t: String| {
        if let Some(top) = stack.last_mut() {
            match top.children.last_mut() {
                Some(Node::Text(s)) => s.push_str(&t),
                _ => top.children.push(Node::Text(t)),
            }
        }
    };
    let mut i = 0;
    let mut tags = 0;
    while i < html.len() {
        let rest = &html[i..];
        if let Some(comment) = rest.strip_prefix("<!--") {
            i = comment.find("-->").map_or(html.len(), |n| i + 4 + n + 3);
            continue;
        }
        if !rest.starts_with('<') {
            let end = rest.find('<').map_or(html.len(), |n| i + n);
            text(&mut stack, entities(&html[i..end]));
            i = end;
            continue;
        }
        // A < that starts no tag is text.
        if !rest[1..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, '/' | '!' | '?'))
        {
            text(&mut stack, "<".into());
            i += 1;
            continue;
        }
        let mut quote = None;
        let mut end = None;
        for (offset, c) in rest[1..].char_indices() {
            if matches!(c, '\'' | '"') {
                if quote == Some(c) {
                    quote = None;
                } else if quote.is_none() {
                    quote = Some(c);
                }
            }
            if c == '>' && quote.is_none() {
                end = Some(i + 1 + offset);
                break;
            }
        }
        let Some(end) = end else {
            if limits.strict {
                return Err(format!("{what} has an unfinished tag."));
            }
            break;
        };
        tags += 1;
        if tags > limits.tags {
            return Err(format!("{what} has too many elements."));
        }
        let raw = html[i + 1..end].trim();
        i = end + 1;
        if raw.starts_with(['!', '?']) {
            continue;
        }
        let close = raw.starts_with('/');
        let body = raw.trim_start_matches('/');
        let name_end = body
            .find(|c: char| c.is_ascii_whitespace() || c == '/')
            .unwrap_or(body.len());
        let tag = body[..name_end].to_ascii_lowercase();
        if close && tag != "br" {
            if let Some(at) = stack.iter().rposition(|e| e.name == tag)
                && at > 0
            {
                pop_to(&mut stack, at);
            }
            continue;
        }
        close_implied(&mut stack, &tag);
        let mut e = Element {
            name: tag.clone(),
            attrs: if close {
                Vec::new()
            } else {
                attributes(&body[name_end..])
            },
            children: Vec::new(),
        };
        if RAW.contains(&tag.as_str()) && !raw.ends_with('/') {
            let lower = html[i..].to_ascii_lowercase();
            let stop = lower
                .find(&format!("</{tag}"))
                .map_or(html.len(), |n| i + n);
            let content = &html[i..stop];
            let content = if matches!(tag.as_str(), "title" | "textarea") {
                entities(content)
            } else {
                content.to_string()
            };
            e.children.push(Node::Text(content));
            i = html[stop..].find('>').map_or(html.len(), |n| stop + n + 1);
            if let Some(top) = stack.last_mut() {
                top.children.push(Node::Element(e));
            }
            continue;
        }
        if VOID.contains(&tag.as_str()) || raw.ends_with('/') {
            if let Some(top) = stack.last_mut() {
                top.children.push(Node::Element(e));
            }
            continue;
        }
        if stack.len() > limits.depth {
            return Err(format!("{what} is nested too deeply."));
        }
        stack.push(e);
    }
    pop_to(&mut stack, 1);
    Ok(stack.pop().expect("the root stays"))
}

// Style sheets: simple selectors only (tag, .class, #id, and one parent
// or ancestor), which covers Quill's own exports and plain pages.

#[derive(Clone, Debug, Default)]
struct Simple {
    tag: Option<String>,
    classes: Vec<String>,
    id: Option<String>,
}

impl Simple {
    fn parse(s: &str) -> Option<Simple> {
        if s.is_empty() || s.contains([':', '[', '(', '+', '~']) {
            return None;
        }
        let mut out = Simple::default();
        let mut rest = s;
        let head = rest.find(['.', '#']).unwrap_or(rest.len());
        let tag = &rest[..head];
        if !tag.is_empty() && tag != "*" {
            out.tag = Some(tag.to_ascii_lowercase());
        }
        rest = &rest[head..];
        while let Some(kind) = rest.chars().next() {
            let end = rest[1..].find(['.', '#']).map_or(rest.len(), |n| n + 1);
            let name = rest[1..end].to_string();
            if name.is_empty() {
                return None;
            }
            if kind == '.' {
                out.classes.push(name);
            } else {
                out.id = Some(name);
            }
            rest = &rest[end..];
        }
        Some(out)
    }

    fn matches(&self, e: &Element) -> bool {
        if self.tag.as_ref().is_some_and(|t| *t != e.name) {
            return false;
        }
        if self
            .id
            .as_ref()
            .is_some_and(|id| e.attr("id") != Some(id.as_str()))
        {
            return false;
        }
        let classes: Vec<&str> = e
            .attr("class")
            .unwrap_or_default()
            .split_whitespace()
            .collect();
        self.classes.iter().all(|c| classes.contains(&c.as_str()))
    }

    fn weight(&self) -> u32 {
        self.id.is_some() as u32 * 100 + self.classes.len() as u32 * 10 + self.tag.is_some() as u32
    }
}

struct Rule {
    target: Simple,
    /// The element's parent (true) or any ancestor (false) must match.
    context: Option<(bool, Simple)>,
    weight: u32,
    declarations: String,
}

/// The rules of a `<style>` sheet. At-rules (@media, @page and so on)
/// are skipped; true when it imports another sheet, which is never
/// fetched.
fn style_sheet(css: &str, rules: &mut Vec<Rule>) -> bool {
    let mut text = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(at) = rest.find("/*") {
        text.push_str(&rest[..at]);
        rest = rest[at + 2..]
            .find("*/")
            .map_or("", |n| &rest[at + 2 + n + 2..]);
    }
    text.push_str(rest);
    let mut imports = false;
    let mut rest = text.as_str();
    while rules.len() < 10_000 {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        if rest.starts_with('@') {
            imports |= rest[1..].to_ascii_lowercase().starts_with("import");
            let semi = rest.find(';');
            let open = rest.find('{');
            match (semi, open) {
                (Some(s), Some(o)) if s < o => rest = &rest[s + 1..],
                (Some(s), None) => rest = &rest[s + 1..],
                (_, Some(o)) => {
                    // Skip the block, nested braces and all.
                    let mut depth = 0;
                    let mut end = rest.len();
                    for (k, c) in rest[o..].char_indices() {
                        match c {
                            '{' => depth += 1,
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    end = o + k + 1;
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    rest = &rest[end..];
                }
                (None, None) => break,
            }
            continue;
        }
        let Some(open) = rest.find('{') else { break };
        let close = rest[open..].find('}').map_or(rest.len(), |n| open + n);
        let declarations = rest[open + 1..close].to_string();
        for selector in rest[..open].split(',') {
            let spaced = selector.replace('>', " > ");
            let parts: Vec<&str> = spaced.split_whitespace().collect();
            let (target, context) = match parts.as_slice() {
                [t] => (Simple::parse(t), None),
                [.., c, ">", t] => (Simple::parse(t), Simple::parse(c).map(|c| (true, c))),
                [.., c, t] => (Simple::parse(t), Simple::parse(c).map(|c| (false, c))),
                _ => (None, None),
            };
            let Some(target) = target else { continue };
            if parts.len() > 1 && context.is_none() {
                continue;
            }
            let weight = target.weight() + context.as_ref().map_or(0, |c| c.1.weight());
            rules.push(Rule {
                target,
                context,
                weight,
                declarations: declarations.clone(),
            });
        }
        rest = &rest[(close + 1).min(rest.len())..];
    }
    imports
}

/// Every element named `name` under `e`.
fn find_all<'e>(e: &'e Element, name: &str, out: &mut Vec<&'e Element>) {
    for c in e.elements() {
        if c.name == name {
            out.push(c);
        }
        find_all(c, name, out);
    }
}

/// A list being read: numbered or not, the count so far and the style
/// of its numbers.
struct List {
    ordered: bool,
    n: i32,
    kind: char,
    none: bool,
}

struct Reader<'a> {
    doc: Document,
    report: ImportReport,
    rules: Vec<Rule>,
    /// The elements being read, outermost first, for style sheet rules.
    path: Vec<&'a Element>,
    /// The paragraph being written, and whether to keep it when empty.
    cur: Option<Paragraph>,
    keep: bool,
    space: bool,
    lists: Vec<List>,
    tables: u32,
    /// Reading a file rather than the clipboard: styles named by Quill's
    /// export are added to the document.
    file: bool,
}

/// The bullet for a list this deep.
fn bullet(depth: usize) -> char {
    ['\u{2022}', '\u{25E6}', '\u{25AA}'][(depth.max(1) - 1) % 3]
}

fn list_number(n: i32, kind: char) -> String {
    let letters = |upper: bool| {
        let mut s = String::new();
        let mut k = n.max(1);
        while k > 0 {
            k -= 1;
            let base = if upper { b'A' } else { b'a' };
            s.insert(0, (base + (k % 26) as u8) as char);
            k /= 26;
        }
        s
    };
    let roman = |upper: bool| {
        let mut s = String::new();
        let mut k = n.clamp(1, 3999);
        for (v, r) in [
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
        ] {
            while k >= v {
                s.push_str(r);
                k -= v;
            }
        }
        if upper { s.to_uppercase() } else { s }
    };
    match kind {
        'a' => letters(false),
        'A' => letters(true),
        'i' => roman(false),
        'I' => roman(true),
        _ => n.to_string(),
    }
}

impl<'a> Reader<'a> {
    /// The format for `e` inside `parent`: what the tag means, then the
    /// style sheet's rules, then its own style attribute.
    fn frame_for(&mut self, e: &Element, parent: &Frame) -> Frame {
        let mut f = parent.clone();
        f.parent_color = parent.chp.color;
        f.width = None;
        f.height = None;
        f.border = false;
        let size = f.chp.size;
        match e.name.as_str() {
            "b" | "strong" => f.chp.bold = true,
            "i" | "em" | "cite" | "var" | "dfn" | "address" => f.chp.italic = true,
            "u" | "ins" => f.chp.underline = Underline::Single,
            // A link to another page, blue and underlined as a browser
            // shows it unless the page's styles say otherwise. Links to
            // places on the same page stay plain text.
            "a" if let Some(href) = e
                .attr("href")
                .map(str::trim)
                .filter(|h| !h.is_empty() && !h.starts_with('#')) =>
            {
                f.chp.link = quill_core::link::intern(quill_core::Link {
                    new_window: e.attr("target") == Some("_blank"),
                    ..quill_core::Link::new(href)
                });
                f.chp.color = quill_core::link::BROWSER_BLUE;
                f.chp.underline = Underline::Single;
            }
            "s" | "strike" | "del" => f.chp.strike = true,
            "sup" => f.chp.position = size as i32 * 3,
            "sub" => f.chp.position = -(size as i32 * 3),
            "small" => f.chp.size = (size * 5 / 6).max(2),
            "big" => f.chp.size = size * 6 / 5,
            "code" | "tt" | "kbd" | "samp" | "pre" => {
                f.chp.font = self.doc.font_id("Courier New");
                f.pre |= e.name == "pre";
            }
            "center" => f.pap.align = Align::Center,
            "blockquote" | "dd" => f.pap.left += 720,
            "th" => {
                f.chp.bold = true;
                f.pap.align = Align::Center;
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = e.name[1..].parse::<u16>().unwrap_or(1);
                if let Some(id) = self.doc.styles.find(&format!("heading {level}")) {
                    let align = f.pap.align;
                    f.pap = self.doc.styles.get(id).pap.clone();
                    f.pap.align = align;
                }
                f.chp.bold = true;
                f.chp.size = [48, 36, 28, 24, 20, 16][level.clamp(1, 6) as usize - 1];
            }
            _ => {}
        }
        for (name, value) in &e.attrs {
            match name.as_str() {
                "face" if e.name == "font" => {
                    if let Some(id) = font(&mut self.doc, value) {
                        f.chp.font = id;
                    }
                }
                "color" if e.name == "font" => {
                    if let Some(color) = color(value) {
                        f.chp.color = color;
                    }
                }
                "size" if e.name == "font" => {
                    if let Ok(n) = value.trim().parse::<usize>() {
                        f.chp.size = [16, 20, 24, 28, 36, 48, 72][n.clamp(1, 7) - 1];
                    }
                }
                "align" if e.name != "img" && e.name != "table" => {
                    f.pap.align = match value.to_ascii_lowercase().as_str() {
                        "center" => Align::Center,
                        "right" => Align::Right,
                        "justify" => Align::Justify,
                        _ => Align::Left,
                    }
                }
                "width" | "height" => {
                    let v = value.trim();
                    let size = match v.strip_suffix('%') {
                        Some(n) => n.trim().parse::<f64>().ok().map(Size::Percent),
                        None => length(v, f.chp.size).map(|pt| Size::Twips((pt * 20.) as i32)),
                    };
                    if name == "width" {
                        f.width = size;
                    } else if let Some(Size::Twips(h)) = size {
                        f.height = Some(h);
                    }
                }
                "hidden" => f.skip = true,
                _ => {}
            }
        }
        let mut matched: Vec<(u32, usize)> = self
            .rules
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                r.target.matches(e)
                    && match &r.context {
                        None => true,
                        Some((true, c)) => self.path.last().is_some_and(|p| c.matches(p)),
                        Some((false, c)) => self.path.iter().any(|p| c.matches(p)),
                    }
            })
            .map(|(k, r)| (r.weight, k))
            .collect();
        matched.sort();
        for (_, k) in matched {
            let declarations = self.rules[k].declarations.clone();
            css(&mut self.doc, &mut f, &declarations);
        }
        if let Some(style) = e.attr("style") {
            css(&mut self.doc, &mut f, style);
        }
        if let Some(name) = e.attr("data-quill-character") {
            if let Some(id) = self.doc.styles.find_character(name) {
                f.chp.character_style = id;
                f.chp.direct_format = e
                    .attr("data-quill-direct")
                    .and_then(|n| n.parse::<u16>().ok())
                    .unwrap_or(0)
                    & quill_core::character_styles::CharacterFormat::ALL;
            } else {
                self.report.note(notes::CHARACTER_STYLES);
            }
        }
        // The page's own margins are not the paragraphs'.
        if matches!(e.name.as_str(), "body" | "html") {
            f.pap = parent.pap.clone();
        }
        f
    }

    /// End the paragraph being written. A last line break adds no line in
    /// a browser, so it goes; a paragraph that had one stays, empty.
    fn flush(&mut self, out: &mut Vec<Paragraph>) {
        self.space = false;
        let Some(mut p) = self.cur.take() else {
            return;
        };
        let broke = p.text.ends_with(chars::LINE_BREAK);
        if broke {
            let end = p.text.len();
            p.remove(end - chars::LINE_BREAK.len_utf8(), end);
        }
        if p.text.is_empty() && !self.keep && !broke {
            return;
        }
        p.pap
            .set_level(self.tables.min(quill_core::limits::MAX_TABLE_LEVEL as u32) as u8);
        out.push(p);
        self.keep = false;
    }

    /// Start a paragraph for a block in format `f`.
    fn start(&mut self, out: &mut Vec<Paragraph>, f: &Frame, keep: bool) {
        self.flush(out);
        self.cur = Some(Paragraph::new(f.pap.clone(), f.chp));
        self.keep = keep;
    }

    /// The paragraph being written, starting one in format `f` if none is.
    fn para(&mut self, f: &Frame) -> &mut Paragraph {
        self.cur
            .get_or_insert_with(|| Paragraph::new(f.pap.clone(), f.chp))
    }

    fn text(&mut self, t: &str, f: &Frame) {
        if f.skip {
            return;
        }
        for c in t.chars() {
            if !f.pre && c.is_ascii_whitespace() {
                self.space = self.cur.as_ref().is_some_and(|p| !p.text.is_empty());
                continue;
            }
            if c.is_control() && !matches!(c, '\n' | '\t' | '\r') {
                continue;
            }
            let space = std::mem::take(&mut self.space);
            let p = self.para(f);
            if space {
                append(p, " ", f.chp);
            }
            if f.pre && matches!(c, '\n' | '\r') {
                append(p, &chars::LINE_BREAK.to_string(), f.chp);
            } else {
                let mut buf = [0u8; 4];
                append(p, c.encode_utf8(&mut buf), f.chp);
            }
        }
    }

    fn children(&mut self, e: &'a Element, f: &Frame, out: &mut Vec<Paragraph>) {
        for n in &e.children {
            match n {
                Node::Text(t) => self.text(t, f),
                Node::Element(c) => self.element(c, f, out),
            }
        }
    }

    fn element(&mut self, e: &'a Element, parent: &Frame, out: &mut Vec<Paragraph>) {
        let name = e.name.as_str();
        match name {
            "script" | "style" | "head" | "template" | "noscript" | "title" | "meta" | "link"
            | "base" | "param" | "source" | "track" | "datalist" | "option" | "map" | "area" => {
                return;
            }
            "iframe" | "frame" | "frameset" => {
                self.report.note(notes::EMBEDDED_DOCUMENTS);
                return;
            }
            "object" | "embed" | "applet" | "video" | "audio" | "canvas" => {
                self.report.note(notes::OBJECTS);
                return;
            }
            "svg" => {
                self.report.note(notes::DRAWINGS);
                return;
            }
            "input" | "select" | "textarea" => {
                self.report.note(notes::CONTROLS);
                return;
            }
            _ => {}
        }
        let f = self.frame_for(e, parent);
        if f.skip {
            return;
        }
        self.path.push(e);
        match name {
            "br" => {
                let c = if e.attr("class").is_some_and(|c| c.contains("page-break")) {
                    chars::PAGE_BREAK
                } else {
                    chars::LINE_BREAK
                };
                self.space = false;
                let p = self.para(&f);
                append(p, &c.to_string(), f.chp);
            }
            "img" => self.picture(e, &f),
            "math" => {
                self.report.note(notes::EQUATIONS);
                self.space = false;
                let p = self.para(&f);
                crate::math::mathml(e, p, f.chp);
            }
            "hr" => {
                self.start(out, &f, true);
                if let Some(p) = &mut self.cur {
                    p.pap.borders.bottom = BorderLine {
                        style: BorderStyle::Single,
                        space: 0,
                        ..Default::default()
                    };
                }
                self.flush(out);
            }
            "table" => self.table(e, &f, out),
            "ul" | "ol" | "menu" | "dir" => {
                self.flush(out);
                let kind = e.attr("type").and_then(|t| t.chars().next()).unwrap_or('1');
                self.lists.push(List {
                    ordered: name == "ol",
                    n: e.attr("start")
                        .and_then(|s| s.trim().parse().ok())
                        .unwrap_or(1)
                        - 1,
                    kind,
                    none: f.list_none,
                });
                self.children(e, &f, out);
                self.flush(out);
                self.lists.pop();
            }
            "li" => {
                let depth = self.lists.len().max(1);
                let mut f = f;
                // Each list sits a level in from what holds it.
                f.pap.left = parent.pap.left + 360;
                f.pap.first = -360;
                let label = match self.lists.last_mut() {
                    Some(l) if !l.none && !f.list_none => {
                        if l.ordered {
                            l.n = e
                                .attr("value")
                                .and_then(|v| v.trim().parse().ok())
                                .unwrap_or(l.n + 1);
                            Some(format!("{}.", list_number(l.n, l.kind)))
                        } else {
                            Some(bullet(depth).to_string())
                        }
                    }
                    None => Some(bullet(1).to_string()),
                    Some(_) => None,
                };
                self.start(out, &f, true);
                if let Some(label) = label {
                    self.report.note(notes::NUMBERING);
                    let p = self.para(&f);
                    append(p, &format!("{label}\t"), f.chp);
                }
                // Text after a list inside this item lines up with it.
                f.pap.first = 0;
                self.children(e, &f, out);
                self.flush(out);
            }
            "q" => {
                self.text("\u{201C}", &f);
                self.children(e, &f, out);
                self.text("\u{201D}", &f);
            }
            _ if block(name) => {
                let keep = name == "p";
                let mut f = f;
                if let Some(style) = e.attr("data-quill-style") {
                    f.pap.style = self.named_style(style, e, &f);
                }
                self.start(out, &f, keep);
                self.children(e, &f, out);
                self.flush(out);
            }
            _ => self.children(e, &f, out),
        }
        self.path.pop();
    }

    /// The style a paragraph from Quill's own export names, adding it (with
    /// the paragraph's format and its runs' class format) when the document
    /// has none by that name.
    fn named_style(&mut self, name: &str, e: &'a Element, f: &Frame) -> u16 {
        if let Some(id) = self.doc.styles.find(name) {
            return id;
        }
        if !self.file {
            return f.pap.style;
        }
        let run = Element {
            name: "span".into(),
            attrs: vec![("class".into(), "run".into())],
            children: Vec::new(),
        };
        self.path.push(e);
        let chp = self.frame_for(&run, f).chp;
        self.path.pop();
        self.doc.styles.add(name, 0, f.pap.clone(), chp)
    }

    /// A picture the page carries itself (a data: URL); one it links to
    /// is never fetched.
    fn picture(&mut self, e: &Element, f: &Frame) {
        let src = e.attr("src").unwrap_or_default().trim();
        let Some(data) = src.strip_prefix("data:") else {
            if !src.is_empty() {
                self.report.note(notes::LINKED_PICTURES);
            }
            return;
        };
        let picture = data
            .split_once(";base64,")
            .and_then(|(_, b64)| crate::odt_read::base64_decode(b64))
            .and_then(crate::images::picture);
        let Some(mut pic) = picture else {
            self.report.note(notes::PICTURES);
            return;
        };
        let (w, h) = pic.size;
        let width = match f.width {
            Some(Size::Twips(t)) => Some(t),
            Some(Size::Percent(p)) => Some((self.doc.props.text_width() as f64 * p / 100.) as i32),
            None => None,
        };
        pic.size = match (width, f.height) {
            (Some(nw), Some(nh)) => (nw, nh),
            (Some(nw), None) if w > 0 => (nw, (h as i64 * nw as i64 / w as i64) as i32),
            (None, Some(nh)) if h > 0 => ((w as i64 * nh as i64 / h as i64) as i32, nh),
            _ => (w, h),
        };
        pic.size = (pic.size.0.clamp(20, 31_680), pic.size.1.clamp(20, 31_680));
        let id = self.doc.add_object(Object::Picture(pic));
        let space = std::mem::take(&mut self.space);
        let p = self.para(f);
        if space {
            append(p, " ", f.chp);
        }
        append(
            p,
            &chars::OBJECT.to_string(),
            CharProps {
                object: id,
                ..f.chp
            },
        );
    }

    /// A table as Quill's: each cell's paragraphs, the last ending in a cell
    /// mark, then the row's end with its cells' widths.
    fn table(&mut self, e: &'a Element, f: &Frame, out: &mut Vec<Paragraph>) {
        self.flush(out);
        let mut rows: Vec<&'a Element> = Vec::new();
        let mut captions = Vec::new();
        for c in e.elements() {
            match c.name.as_str() {
                "tr" => rows.push(c),
                "thead" | "tbody" | "tfoot" => rows.extend(c.children_named("tr")),
                "caption" => captions.push(c),
                _ => {}
            }
        }
        for c in captions {
            self.element(c, f, out);
            self.flush(out);
        }
        let cells_of =
            |row: &'a Element| row.elements().filter(|c| c.name == "td" || c.name == "th");
        if self.tables >= quill_core::limits::MAX_TABLE_LEVEL as u32 {
            // Past the depth Quill keeps, the inner table's text becomes
            // paragraphs of the cell it is in.
            self.report.note(notes::NESTED_TABLES);
            for row in rows {
                for cell in cells_of(row) {
                    let cf = self.frame_for(cell, f);
                    self.start(out, &cf, false);
                    self.children(cell, &cf, out);
                    self.flush(out);
                }
            }
            return;
        }
        let span = |c: &Element| {
            c.attr("colspan")
                .and_then(|s| s.trim().parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, MAX_COLUMNS)
        };
        let columns = rows
            .iter()
            .map(|r| cells_of(r).map(span).sum::<usize>())
            .max()
            .unwrap_or(0)
            .clamp(1, MAX_COLUMNS);
        let text_width = self.doc.props.text_width();
        let table_width = match f.width {
            Some(Size::Twips(t)) => t,
            Some(Size::Percent(p)) => (text_width as f64 * p / 100.) as i32,
            None => text_width,
        }
        .clamp(720, 31_680);
        let ruled = e.attr("border").is_some_and(|b| b.trim() != "0") || f.border;
        let align = match e.attr("align").map(str::to_ascii_lowercase).as_deref() {
            Some("center") => Align::Center,
            Some("right") => Align::Right,
            _ => Align::Left,
        };
        let line = BorderLine {
            style: BorderStyle::Single,
            space: 0,
            ..Default::default()
        };
        self.path.push(e);
        for row in rows {
            self.path.push(row);
            let rf = self.frame_for(row, f);
            let mut cells: Vec<CellProps> = Vec::new();
            let mut row_chp = f.chp;
            self.tables += 1;
            for cell in cells_of(row) {
                let full = cells.len() >= MAX_COLUMNS;
                if full {
                    // Past the most cells a row can have, the text joins
                    // the row's last cell rather than being lost.
                    self.report.note(notes::WIDE_ROWS);
                    if let Some(last) = out.last_mut() {
                        last.mark = Mark::Paragraph;
                    }
                }
                let cf = self.frame_for(cell, &rf);
                let start = out.len();
                self.start(out, &cf, false);
                self.path.push(cell);
                self.children(cell, &cf, out);
                self.path.pop();
                self.flush(out);
                // A cell ends in a paragraph of its own, even after a table
                // in it.
                if out.len() == start
                    || out
                        .last()
                        .is_some_and(|p| p.pap.level() as u32 > self.tables)
                {
                    let mut p = Paragraph::new(cf.pap.clone(), cf.chp);
                    p.pap.set_level(self.tables as u8);
                    out.push(p);
                }
                let last = out.last_mut().expect("pushed");
                last.mark = Mark::CellEnd;
                row_chp = last.mark_chp;
                if full {
                    continue;
                }
                let width = match cf.width {
                    Some(Size::Twips(t)) => t,
                    Some(Size::Percent(p)) => (table_width as f64 * p / 100.) as i32,
                    None => table_width * span(cell) as i32 / columns as i32,
                }
                .clamp(144, 31_680);
                let edge = if ruled || cf.border {
                    line
                } else {
                    BorderLine::default()
                };
                cells.push(CellProps {
                    width,
                    borders: Borders {
                        top: edge,
                        bottom: edge,
                        left: edge,
                        right: edge,
                        ..Borders::default()
                    },
                    shading: 0,
                    shading_color: cf.pap.shading_color,
                    valign: Default::default(),
                });
            }
            self.tables -= 1;
            self.path.pop();
            if cells.is_empty() {
                continue;
            }
            let n = self.doc.styles.get(0);
            let mut end = Paragraph::new(n.pap.clone(), row_chp);
            end.pap.set_level(self.tables as u8 + 1);
            end.mark = Mark::RowEnd;
            end.row = Some(Box::new(RowProps {
                cells,
                gap: TABLE_GAP,
                indent: 0,
                height: RowHeight::Auto,
                align,
            }));
            out.push(end);
        }
        self.path.pop();
    }
}

/// The most cells a row can have, as in the other readers.
const MAX_COLUMNS: usize = 255;

fn block(tag: &str) -> bool {
    matches!(
        tag,
        "p" | "div"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "pre"
            | "blockquote"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "main"
            | "nav"
            | "aside"
            | "address"
            | "figure"
            | "figcaption"
            | "caption"
            | "dl"
            | "dt"
            | "dd"
            | "center"
            | "form"
            | "fieldset"
            | "legend"
            | "details"
            | "summary"
            | "body"
    )
}

/// Read HTML into a document: the whole of it, or `limits` exceeded.
fn build(html: &str, limits: &Limits, file: bool) -> Result<(Document, ImportReport), String> {
    let normalized = html.replace("\r\n", "\n").replace('\r', "\n");
    let root = parse(&normalized, limits)?;
    let mut r = Reader {
        doc: Document::new(),
        report: ImportReport::default(),
        rules: Vec::new(),
        path: Vec::new(),
        cur: None,
        keep: false,
        space: false,
        lists: Vec::new(),
        tables: 0,
        file,
    };
    let mut sheets = Vec::new();
    find_all(&root, "style", &mut sheets);
    for sheet in sheets {
        if style_sheet(&sheet.text(), &mut r.rules) {
            r.report.note(notes::STYLE_SHEETS);
        }
    }
    let mut links = Vec::new();
    find_all(&root, "link", &mut links);
    if links.iter().any(|l| {
        l.attr("rel")
            .is_some_and(|r| r.to_ascii_lowercase().contains("stylesheet"))
    }) {
        r.report.note(notes::STYLE_SHEETS);
    }
    if file {
        let mut titles = Vec::new();
        find_all(&root, "title", &mut titles);
        if let Some(t) = titles.first() {
            r.doc.summary.title = t.text().split_whitespace().collect::<Vec<_>>().join(" ");
        }
    }
    let mut metadata = Vec::new();
    find_all(&root, "meta", &mut metadata);
    for e in metadata {
        if e.attr("name") != Some("quill-character-style") {
            continue;
        }
        if let (Some(name), Some(font), Some(encoded)) = (
            e.attr("content"),
            e.attr("data-font"),
            e.attr("data-format"),
        ) {
            if name.trim().is_empty()
                || name.chars().count() > 255
                || font.len() > 1024
                || r.doc.styles.characters.len() >= u16::MAX as usize
                || r.doc.styles.find_character(name).is_some()
            {
                r.report.note(notes::CHARACTER_STYLES);
                continue;
            }
            if let Some(mut format) = crate::character_styles::decode(encoded, 0) {
                if format.mask & 1 != 0 {
                    if r.doc.fonts.len() >= u16::MAX as usize {
                        r.report.note(notes::CHARACTER_STYLES);
                        continue;
                    }
                    format.props.font = r.doc.font_id(font);
                }
                if r.doc.styles.add_character(name, format).is_none() {
                    r.report.note(notes::CHARACTER_STYLES);
                }
            } else {
                r.report.note(notes::CHARACTER_STYLES);
            }
        }
    }
    let normal = r.doc.styles.get(0).clone();
    let base = Frame {
        chp: normal.chp,
        pap: normal.pap.clone(),
        pre: false,
        skip: false,
        border: false,
        width: None,
        height: None,
        list_none: false,
        parent_color: normal.chp.color,
    };
    let mut paras = Vec::new();
    r.children(&root, &base, &mut paras);
    r.flush(&mut paras);
    if paras.is_empty() {
        paras.push(Paragraph::new(normal.pap.clone(), normal.chp));
    }
    // A table cannot be the very last thing.
    if paras.last().is_some_and(|p| p.pap.in_table) {
        paras.push(Paragraph::new(normal.pap, normal.chp));
    }
    r.doc.main.paras = paras;
    r.doc.forget_history();
    Ok((r.doc, r.report))
}

/// Import clipboard HTML: text, formatting, lists and tables. Scripts are
/// never run and nothing it links to is fetched.
pub fn read_clipboard(html: &str) -> Result<Document, String> {
    if html.len() > CLIPBOARD.bytes {
        return Err("HTML clipboard content is too large.".into());
    }
    build(html, &CLIPBOARD, false).map(|(doc, _)| doc)
}

/// Open an HTML file, with what could not be kept. The character set is
/// the file's byte-order mark, its `<meta charset>`, or UTF-8 when the
/// bytes are valid UTF-8 and Windows Latin otherwise.
pub fn read_file(bytes: &[u8]) -> Result<(Document, ImportReport), String> {
    if bytes.len() > FILE.bytes {
        return Err("The HTML file is too large.".into());
    }
    let mut report = ImportReport::default();
    let text = decode(bytes, &mut report);
    let (doc, mut more) = build(&text, &FILE, true)?;
    for note in report.notes() {
        more.note(note);
    }
    Ok((doc, more))
}

/// Whether bytes look like an HTML page rather than plain text.
pub fn is_html(bytes: &[u8]) -> bool {
    let head: String = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)])
        .trim_start_matches('\u{FEFF}')
        .trim_start()
        .chars()
        .take(512)
        .collect::<String>()
        .to_ascii_lowercase();
    head.starts_with("<!doctype html")
        || head.starts_with("<html")
        || ((head.starts_with("<?xml") || head.starts_with("<!--")) && head.contains("<html"))
}

fn decode(bytes: &[u8], report: &mut ImportReport) -> String {
    if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        return String::from_utf8_lossy(rest).into_owned();
    }
    for (bom, little) in [(b"\xFF\xFE", true), (b"\xFE\xFF", false)] {
        if let Some(rest) = bytes.strip_prefix(bom) {
            let units: Vec<u16> = rest
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&c| {
                    if little {
                        u16::from_le_bytes(c)
                    } else {
                        u16::from_be_bytes(c)
                    }
                })
                .collect();
            return String::from_utf16_lossy(&units);
        }
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]).to_ascii_lowercase();
    let charset = head.find("charset").map(|at| {
        head[at + 7..]
            .trim_start_matches([' ', '=', '"', '\''])
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            .collect::<String>()
    });
    let latin = |table: &dyn Fn(u8) -> char| bytes.iter().map(|&b| table(b)).collect::<String>();
    let cp1252 = |b: u8| match b {
        0x80..=0x9F => crate::rtf::read::CP1252[(b - 0x80) as usize],
        _ => b as char,
    };
    match charset.as_deref() {
        Some("windows-1252" | "iso-8859-1" | "latin1" | "iso-8859-15" | "us-ascii" | "ascii") => {
            latin(&cp1252)
        }
        Some("macintosh" | "x-mac-roman" | "mac") => latin(&|b: u8| match b {
            0x80.. => crate::rtf::read::MAC_ROMAN[(b - 0x80) as usize],
            _ => b as char,
        }),
        Some(c) if !c.is_empty() && !matches!(c, "utf-8" | "utf8") => {
            report.note(notes::ENCODING);
            String::from_utf8_lossy(bytes).into_owned()
        }
        _ => match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => latin(&cp1252),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loose_markup_closes_the_way_browsers_close_it() {
        let root = parse(
            "<p>one<p>two<ul><li>a<li>b</ul><table><tr><td>x<td>y</table>",
            &FILE,
        )
        .unwrap();
        let names = |e: &Element| e.elements().map(|c| c.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&root), ["p", "p", "ul", "table"]);
        let ul = root.child("ul").unwrap();
        assert_eq!(names(ul), ["li", "li"]);
        let tr = root.child("table").unwrap().child("tr").unwrap();
        assert_eq!(names(tr), ["td", "td"]);
    }

    #[test]
    fn a_page_reads_with_headings_lists_tables_and_its_title() {
        let (doc, report) = read_file(
            br#"<!doctype html><html><head><title>My  Page</title><style>.note{color:#ff0000} p > .big{font-size:24pt}</style><link rel="stylesheet" href="site.css"></head>
            <body><h1>Heading</h1><p class="note">Red <span class="big">big</span></p>
            <ol start="3"><li>three</li><li>four<ul><li>dot</li></ul></li></ol>
            <table border="1"><tr><th>A</th><td>B</td></tr><tr><td colspan="2">wide</td></tr></table>
            <p><img src="https://example.invalid/x.png"> <a href="https://example.invalid/">link</a></p></body></html>"#,
        )
        .unwrap();
        assert_eq!(doc.summary.title, "My Page");
        let texts: Vec<&str> = doc.main.paras.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "Heading",
                "Red big",
                "3.\tthree",
                "4.\tfour",
                "\u{25E6}\tdot",
                "A",
                "B",
                "",
                "wide",
                "",
                "link"
            ]
        );
        let p = &doc.main.paras;
        assert_eq!(doc.styles.get(p[0].pap.style).name, "heading 1");
        assert_eq!(p[1].chp_of_char(0).color, Rgb(0xff0000));
        assert_eq!(p[1].chp_of_char(4).size, 48);
        assert_eq!(p[4].pap.left, 720);
        assert_eq!(p[5].mark, Mark::CellEnd);
        assert!(p[5].chp_of_char(0).bold);
        let row = p[7].row.as_ref().unwrap();
        assert_eq!(row.cells.len(), 2);
        assert!(!row.cells[0].borders.top.is_none());
        assert_eq!(p[9].row.as_ref().unwrap().cells.len(), 1);
        let link = p[10].chp_of_char(0);
        assert_eq!(quill_core::link::url(link.link), "https://example.invalid/");
        assert_eq!(link.color, quill_core::link::BROWSER_BLUE);
        assert_eq!(link.underline, Underline::Single);
        assert_eq!(
            report.notes(),
            [
                notes::STYLE_SHEETS,
                notes::NUMBERING,
                notes::LINKED_PICTURES
            ]
        );
    }

    #[test]
    fn character_sets() {
        let (doc, _) =
            read_file(b"<html><meta charset=\"windows-1252\"><p>caf\xE9 \x93q\x94</p></html>")
                .unwrap();
        assert_eq!(doc.main.paras[0].text, "caf\u{E9} \u{201C}q\u{201D}");
        let (doc, _) = read_file(b"<html><p>caf\xE9</p></html>").unwrap();
        assert_eq!(doc.main.paras[0].text, "caf\u{E9}");
        let (doc, _) = read_file("<html><p>café</p></html>".as_bytes()).unwrap();
        assert_eq!(doc.main.paras[0].text, "café");
        assert!(is_html(b"\xEF\xBB\xBF  <!DOCTYPE html><p>x"));
        assert!(!is_html(b"plain <html> text"));
    }

    #[test]
    fn quill_s_own_export_opens_with_its_styles() {
        let mut doc = Document::new();
        let id = doc.styles.add(
            "Quote",
            0,
            ParaProps {
                left: 720,
                ..ParaProps::default()
            },
            CharProps {
                italic: true,
                ..CharProps::default()
            },
        );
        doc.main.paras[0].insert(
            0,
            "Quoted\ttext",
            CharProps {
                italic: true,
                ..CharProps::default()
            },
        );
        doc.main.paras[0].pap = doc.styles.get(id).pap.clone();
        let html = crate::html::write(&doc);
        let (back, _) = read_file(&html).unwrap();
        let p = &back.main.paras[0];
        assert_eq!(p.text, "Quoted\ttext");
        let style = back.styles.get(p.pap.style);
        assert_eq!(style.name, "Quote");
        assert_eq!(p.pap.left, 720);
        assert!(p.chp_of_char(0).italic);
    }
}
