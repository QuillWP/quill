//! Basic self-contained HTML5: named paragraph styles become CSS classes,
//! direct formatting becomes inline CSS, and pictures use data URLs.

use crate::export::{Block, base64, blocks_at, escape, points};
use quill_core::Document;
use std::fmt::Write as _;

pub use crate::html_read::{is_html, read_clipboard, read_file};
use quill_core::para::{Paragraph, chars};
use quill_core::props::{
    Align, BorderLine, BorderStyle, CharProps, LineSpacing, ParaProps, Rgb, Underline,
};

fn css_string(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() || c == ' ' || c == '-' {
            out.push(c);
        } else {
            let _ = write!(out, "\\{:x} ", c as u32);
        }
    }
    out
}

fn character(doc: &Document, c: &CharProps, base: Option<&CharProps>) -> String {
    let mut out = String::new();
    macro_rules! prop {
        ($field:ident, $name:literal, $value:expr) => {
            if base.is_none_or(|b| c.$field != b.$field) {
                let _ = write!(out, concat!($name, ":{};"), $value);
            }
        };
    }
    prop!(
        font,
        "font-family",
        format!("'{}'", css_string(doc.font_name(c.font)))
    );
    prop!(
        size,
        "font-size",
        format!("{}pt", points(c.size as i32 * 10))
    );
    prop!(color, "color", format!("#{:06x}", c.color.0 & 0xffffff));
    prop!(bold, "font-weight", if c.bold { "bold" } else { "normal" });
    prop!(
        italic,
        "font-style",
        if c.italic { "italic" } else { "normal" }
    );
    prop!(
        small_caps,
        "font-variant",
        if c.small_caps { "small-caps" } else { "normal" }
    );
    prop!(
        all_caps,
        "text-transform",
        if c.all_caps { "uppercase" } else { "none" }
    );
    prop!(hidden, "display", if c.hidden { "none" } else { "inline" });
    prop!(
        spacing,
        "letter-spacing",
        format!("{}pt", points(c.spacing))
    );
    prop!(
        position,
        "vertical-align",
        format!("{}pt", points(c.position))
    );
    prop!(
        shadow,
        "text-shadow",
        if c.shadow { "1px 1px 1px #888" } else { "none" }
    );
    prop!(
        outline,
        "-webkit-text-stroke-width",
        if c.outline { "0.5px" } else { "0" }
    );
    if base.is_none_or(|b| c.underline != b.underline || c.strike != b.strike) {
        let mut lines = Vec::new();
        if c.underline != Underline::None {
            lines.push("underline");
        }
        if c.strike {
            lines.push("line-through");
        }
        let _ = write!(
            out,
            "text-decoration-line:{};text-decoration-style:{};",
            if lines.is_empty() {
                "none".into()
            } else {
                lines.join(" ")
            },
            match c.underline {
                Underline::Double => "double",
                Underline::Dotted => "dotted",
                _ => "solid",
            }
        );
    }
    out
}

fn paragraph_css(p: &ParaProps) -> String {
    let mut out = format!(
        "text-align:{};margin:{}pt {}pt {}pt {}pt;text-indent:{}pt;",
        match p.align {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "justify",
        },
        points(p.before),
        points(p.right),
        points(p.after),
        points(p.left),
        points(p.first)
    );
    match p.line {
        LineSpacing::Auto => out.push_str("line-height:normal;"),
        LineSpacing::Multiple(n) => {
            let _ = write!(out, "line-height:{:.4};", n as f64 / 240.);
        }
        LineSpacing::Exactly(n) | LineSpacing::AtLeast(n) => {
            let _ = write!(out, "line-height:{}pt;", points(n));
        }
    }
    let _ = write!(
        out,
        "break-before:{};break-after:{};break-inside:{};",
        if p.page_break_before { "page" } else { "auto" },
        if p.keep_with_next { "avoid" } else { "auto" },
        if p.keep_together { "avoid" } else { "auto" }
    );
    let gray = 255 - (255u32 * p.shading.min(1000) as u32 / 1000);
    let color = p.shading_color.map_or(gray * 0x010101, |c| c.0);
    if p.shading_color.is_some() || p.shading > 0 {
        let _ = write!(out, "background-color:#{color:06x};");
    } else {
        out.push_str("background-color:transparent;");
    }
    for (edge, line) in [
        ("top", p.borders.top),
        ("right", p.borders.right),
        ("bottom", p.borders.bottom),
        ("left", p.borders.left),
    ] {
        let _ = write!(
            out,
            "border-{edge}:{};padding-{edge}:{}pt;",
            border_css(&line),
            points(line.space)
        );
    }
    out
}

fn border_css(line: &BorderLine) -> String {
    let kind = match line.style {
        BorderStyle::None => return "none".to_string(),
        BorderStyle::Dotted => "0.5pt dotted",
        BorderStyle::Hairline => "0.25pt solid",
        BorderStyle::Single => "0.5pt solid",
        BorderStyle::Thick => "1.5pt solid",
        BorderStyle::Double => "1.5pt double",
    };
    if line.color == Rgb::BLACK {
        kind.to_string()
    } else {
        format!("{kind} #{:06x}", line.color.0 & 0xFF_FFFF)
    }
}

/// A table cell's borders and shading. The page style gives every `td` a
/// thin gray border, so each edge is written, "none" included, or a
/// borderless cell would gain one.
fn cell_css(cell: &quill_core::props::CellProps) -> String {
    let b = &cell.borders;
    let mut out = String::new();
    if b.top.style == b.bottom.style && b.top.style == b.left.style && b.top.style == b.right.style
    {
        let _ = write!(out, ";border:{}", border_css(&b.top));
    } else {
        for (edge, line) in [
            ("top", b.top),
            ("right", b.right),
            ("bottom", b.bottom),
            ("left", b.left),
        ] {
            let _ = write!(out, ";border-{edge}:{}", border_css(&line));
        }
    }
    if let Some(color) = cell.shading_color {
        let _ = write!(out, ";background-color:#{:06x}", color.0);
    } else if cell.shading > 0 {
        let gray = 255 - (255u32 * cell.shading.min(1000) as u32 / 1000);
        let _ = write!(out, ";background-color:#{gray:02x}{gray:02x}{gray:02x}");
    }
    out
}

struct Writer<'a> {
    doc: &'a Document,
    inline: bool,
    /// Each link and color met, whose CSS class is `l` and its place
    /// here: a link's three colors need :visited and :hover rules, which
    /// a style attribute cannot hold.
    links: std::cell::RefCell<Vec<(u32, quill_core::Rgb)>>,
}

impl Writer<'_> {
    fn link_class(&self, link: u32, color: quill_core::Rgb) -> usize {
        let mut links = self.links.borrow_mut();
        match links.iter().position(|&l| l == (link, color)) {
            Some(k) => k,
            None => {
                links.push((link, color));
                links.len() - 1
            }
        }
    }

    /// The style sheet's rules for the links written so far.
    fn link_css(&self) -> String {
        let mut out = String::new();
        for (k, &(link, color)) in self.links.borrow().iter().enumerate() {
            if let Some(l) = quill_core::link::get(link) {
                let _ = writeln!(
                    out,
                    "a.l{k}{{color:#{:06x};}}a.l{k}:visited{{color:#{:06x};}}a.l{k}:hover{{color:#{:06x};}}",
                    color.0 & 0xffffff,
                    l.visited.0 & 0xffffff,
                    l.hover.0 & 0xffffff
                );
            }
        }
        out
    }

    fn paragraph(&self, p: &Paragraph, out: &mut String, note: bool) {
        let style = self.doc.styles.get(p.pap.style);
        let id = if (p.pap.style as usize) < self.doc.styles.styles.len() {
            p.pap.style
        } else {
            0
        };
        let css = if !self.inline && p.pap == style.pap {
            String::new()
        } else {
            paragraph_css(&p.pap)
        };
        let _ = write!(
            out,
            "<p class=\"s{id}\" data-quill-style=\"{}\" style=\"{}\">",
            escape(&style.name),
            escape(&css)
        );
        let mut start = 0;
        for run in &p.runs {
            let link = quill_core::link::get(run.chp.link);
            let mut chp = run.chp;
            if link.is_some() && !self.inline {
                // The link's class gives its color.
                chp.color = style.chp.color;
            }
            if let Some(l) = &link {
                let class = if self.inline {
                    String::new()
                } else {
                    format!(
                        " class=\"lk l{}\"",
                        self.link_class(run.chp.link, run.chp.color)
                    )
                };
                let target = if l.new_window {
                    " target=\"_blank\" rel=\"noopener\""
                } else {
                    ""
                };
                let _ = write!(out, "<a href=\"{}\"{target}{class}>", escape(&l.url));
            }
            let css = character(self.doc, &chp, (!self.inline).then_some(&style.chp));
            let _ = write!(out, "<span class=\"run\" style=\"{}\">", escape(&css));
            let end = start + run.len;
            for c in p.text[start..end].chars() {
                match c {
                    chars::LINE_BREAK | '\n' => out.push_str("<br>"),
                    chars::PAGE_BREAK => out.push_str("<br class=\"page-break\">"),
                    chars::OBJECT => {
                        if let Some(pic) = self.doc.picture(run.chp.object) {
                            let _ = write!(
                                out,
                                "<img alt=\"\" src=\"data:{};base64,{}\" style=\"width:{}pt;height:{}pt;max-width:100%;\">",
                                pic.format.mime(),
                                base64(&pic.data),
                                points(pic.size.0),
                                points(pic.size.1)
                            );
                        }
                    }
                    chars::FOOTNOTE if !note => {
                        let id = run.chp.object;
                        let _ = write!(
                            out,
                            "<sup><a href=\"#note-{id}\">{}</a></sup>",
                            escape(
                                &self
                                    .doc
                                    .note_labels(None)
                                    .get(&id)
                                    .cloned()
                                    .unwrap_or_else(|| id.to_string())
                            )
                        );
                    }
                    chars::ANNOTATION | chars::FOOTNOTE => {}
                    chars::FIELD_PAGE => out.push_str("[Page]"),
                    chars::FIELD_DATE => out.push_str("[Date]"),
                    chars::FIELD_TIME => out.push_str("[Time]"),
                    c => out.push_str(&escape(&c.to_string())),
                }
            }
            out.push_str("</span>");
            if link.is_some() {
                out.push_str("</a>");
            }
            start = end;
        }
        if p.text.is_empty() {
            let css = character(self.doc, &p.mark_chp, (!self.inline).then_some(&style.chp));
            let _ = write!(
                out,
                "<span class=\"run\" style=\"{}\"><br></span>",
                escape(&css)
            );
        }
        out.push_str("</p>\n");
    }

    fn story(&self, paras: &[Paragraph], out: &mut String, note: bool) {
        self.story_at(paras, 0, out, note);
    }

    /// The paragraphs of a story, or of a cell `level` tables deep.
    fn story_at(&self, paras: &[Paragraph], level: u8, out: &mut String, note: bool) {
        for block in blocks_at(paras, level) {
            match block {
                Block::Paragraph(p) => self.paragraph(p, out, note),
                Block::Table(rows) => {
                    out.push_str("<table><tbody>\n");
                    for row in rows {
                        let props =
                            quill_core::table::row_props(paras, &row, self.doc.props.text_width());
                        out.push_str("<tr>");
                        for (i, (a, b)) in row.cells.iter().copied().enumerate() {
                            let valign = match props.cells[i].valign {
                                quill_core::props::VAlign::Top => "",
                                quill_core::props::VAlign::Center => ";vertical-align:middle",
                                quill_core::props::VAlign::Bottom => ";vertical-align:bottom",
                            };
                            let _ = write!(
                                out,
                                "<td style=\"width:{}pt{valign}{}\">",
                                points(props.cells[i].width),
                                cell_css(&props.cells[i])
                            );
                            self.story_at(&paras[a..=b], level + 1, out, note);
                            out.push_str("</td>");
                        }
                        out.push_str("</tr>\n");
                    }
                    out.push_str("</tbody></table>\n");
                }
            }
        }
    }
}

pub fn write(doc: &Document) -> Vec<u8> {
    let mut out = format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title>\n<style>\n",
        escape(&doc.summary.title)
    );
    out.push_str("body{margin:2rem auto;padding:0 1rem;}p{white-space:pre-wrap;tab-size:4;}table{border-collapse:collapse;}td{border:1px solid #aaa;padding:4pt;vertical-align:top;}img{vertical-align:baseline;} .page-break{display:block;break-after:page;}\n");
    let _ = writeln!(
        out,
        "body{{max-width:{}pt;}}@page{{size:{}pt {}pt;margin:{}pt {}pt {}pt {}pt;}}",
        points(doc.props.text_width().max(720)),
        points(doc.props.page_width),
        points(doc.props.page_height),
        points(doc.props.margin_top),
        points(doc.props.margin_right),
        points(doc.props.margin_bottom),
        points(doc.props.margin_left)
    );
    for (id, style) in doc.styles.styles.iter().enumerate() {
        let _ = writeln!(
            out,
            ".s{id}{{{}}}.s{id} .run{{{}}}",
            paragraph_css(&style.pap),
            character(doc, &style.chp, None)
        );
    }
    let writer = Writer {
        doc,
        inline: false,
        links: Default::default(),
    };
    let mut body = String::new();
    writer.story(&doc.main.paras, &mut body, false);
    let ranges = doc.note_ranges();
    let labels = doc.note_labels(None);
    let ids = quill_core::notes::note_ids(&doc.main.paras);
    if !ids.is_empty() {
        body.push_str("<section aria-label=\"Footnotes\"><hr>\n");
        for id in ids {
            if let Some(range) = ranges.get(&id) {
                let _ = write!(
                    body,
                    "<div id=\"note-{id}\"><span>{}</span>",
                    escape(labels.get(&id).map(String::as_str).unwrap_or("*"))
                );
                writer.story(&doc.notes.paras[range.clone()], &mut body, true);
                body.push_str("</div>\n");
            }
        }
        body.push_str("</section>\n");
    }
    // A linked run's span takes its color from the link.
    out.push_str("a.lk>.run{color:inherit;}\n");
    out.push_str(&writer.link_css());
    out.push_str("</style></head><body>\n");
    out.push_str(&body);
    out.push_str("</body></html>\n");
    out.into_bytes()
}

/// A clipboard fragment needs inline properties: receiving apps may discard
/// the document head and its stylesheet when inserting the selection.
pub fn write_clipboard(doc: &Document) -> String {
    let mut out = String::from("<div>");
    Writer {
        doc,
        inline: true,
        links: Default::default(),
    }
    .story(&doc.main.paras, &mut out, false);
    out.push_str("</div>");
    out
}
