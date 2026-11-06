//! OpenDocument Text 1.3 export. No private document payload or external tools.

use crate::export::{Block, blocks_at, escape, points};
use crate::zip;
use quill_core::Document;
use quill_core::para::{Paragraph, chars};
use quill_core::props::{
    Align, BorderLine, BorderStyle, CellProps, CharProps, LineSpacing, ParaProps, Underline, VAlign,
};
use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;

const MIME: &str = "application/vnd.oasis.opendocument.text";
const NS: &str = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:dc="http://purl.org/dc/elements/1.1/" office:version="1.3""#;

/// Whether `bytes` are an OpenDocument Text package (or its template).
pub fn is_odt(bytes: &[u8]) -> bool {
    zip::Archive::open(bytes)
        .ok()
        .and_then(|z| z.read("mimetype").ok().flatten())
        .is_some_and(|m| {
            m == MIME.as_bytes()
                || m == b"application/vnd.oasis.opendocument.text-template"
                || m == b"application/vnd.oasis.opendocument.text-master"
        })
}

fn text_props(c: &CharProps, base: Option<&CharProps>) -> String {
    let full = format!(
        r##"<style:text-properties style:font-name="f{}" fo:font-size="{}pt" fo:font-weight="{}" fo:font-style="{}" fo:color="#{:06x}" fo:font-variant="{}" fo:text-transform="{}" fo:letter-spacing="{}pt" text:display="{}" style:text-outline="{}" fo:text-shadow="{}" style:text-position="{}% 100%" style:text-underline-style="{}" style:text-underline-type="{}" style:text-underline-mode="{}" style:text-line-through-style="{}"/>"##,
        c.font,
        points(c.size as i32 * 10),
        if c.bold { "bold" } else { "normal" },
        if c.italic { "italic" } else { "normal" },
        c.color.0 & 0xffffff,
        if c.small_caps { "small-caps" } else { "normal" },
        if c.all_caps { "uppercase" } else { "none" },
        points(c.spacing),
        if c.hidden { "none" } else { "true" },
        c.outline,
        if c.shadow { "1pt 1pt #808080" } else { "none" },
        c.position as f64 * 100. / (c.size.max(1) as f64 * 10.),
        match c.underline {
            Underline::None => "none",
            Underline::Dotted => "dotted",
            _ => "solid",
        },
        if c.underline == Underline::Double {
            "double"
        } else {
            "single"
        },
        if c.underline == Underline::Word {
            "skip-white-space"
        } else {
            "continuous"
        },
        if c.strike { "solid" } else { "none" }
    );
    let Some(base) = base else {
        return full;
    };
    // Keep named styles effective in other editors: write only changed
    // attributes on direct runs, including explicit resets such as bold=false.
    let inherited = text_props(base, None);
    let own = crate::xml::parse(full.as_bytes()).expect("generated text properties");
    let inherited = crate::xml::parse(inherited.as_bytes()).expect("generated text properties");
    let mut attrs = String::new();
    for (name, value) in &own.attrs {
        if inherited.attrs.iter().any(|(n, v)| n == name && v == value) {
            continue;
        }
        let prefix = match name.as_str() {
            "font-name"
            | "text-outline"
            | "text-position"
            | "text-underline-style"
            | "text-underline-type"
            | "text-underline-mode"
            | "text-line-through-style" => "style",
            "display" => "text",
            _ => "fo",
        };
        let _ = write!(attrs, " {prefix}:{name}=\"{}\"", escape(value));
    }
    format!("<style:text-properties{attrs}/>")
}

fn para_props(p: &ParaProps) -> String {
    let line = match p.line {
        LineSpacing::Auto => "fo:line-height=\"normal\"".into(),
        LineSpacing::Multiple(n) => format!("fo:line-height=\"{}%\"", n as f64 * 100. / 240.),
        LineSpacing::Exactly(n) => format!("fo:line-height=\"{}pt\"", points(n)),
        LineSpacing::AtLeast(n) => format!("style:line-height-at-least=\"{}pt\"", points(n)),
    };
    let gray = 255 - (255u32 * p.shading.min(1000) as u32 / 1000);
    let fill = if p.shading_color.is_some() || p.shading > 0 {
        format!("#{:06x}", p.shading_color.map_or(gray * 0x010101, |c| c.0))
    } else {
        "transparent".into()
    };
    format!(
        r##"<style:paragraph-properties fo:text-align="{}" fo:margin-left="{}pt" fo:margin-right="{}pt" fo:margin-top="{}pt" fo:margin-bottom="{}pt" fo:text-indent="{}pt" {line} fo:break-before="{}" fo:keep-with-next="{}" fo:keep-together="{}" fo:background-color="{fill}"/>"##,
        match p.align {
            Align::Left => "start",
            Align::Center => "center",
            Align::Right => "end",
            Align::Justify => "justify",
        },
        points(p.left),
        points(p.right),
        points(p.before),
        points(p.after),
        points(p.first),
        if p.page_break_before { "page" } else { "auto" },
        if p.keep_with_next { "always" } else { "auto" },
        if p.keep_together { "always" } else { "auto" }
    )
}

/// A border as `fo:border` writes it. The widths are chosen so the reader
/// maps each back to the same line style.
fn border_value(line: &BorderLine) -> String {
    let kind = match line.style {
        BorderStyle::None => return "none".to_string(),
        BorderStyle::Dotted => "0.5pt dotted",
        BorderStyle::Hairline => "0.25pt solid",
        BorderStyle::Single => "0.5pt solid",
        BorderStyle::Thick => "3pt solid",
        BorderStyle::Double => "1.5pt double",
    };
    format!("{kind} #{:06x}", line.color.0 & 0xFF_FFFF)
}

struct Writer<'a> {
    doc: &'a Document,
    automatic: String,
    pictures: BTreeSet<u32>,
    serial: usize,
    labels: HashMap<u32, String>,
    notes: HashMap<u32, std::ops::Range<usize>>,
}

impl Writer<'_> {
    fn id(&mut self, prefix: &str) -> String {
        self.serial += 1;
        format!("{prefix}{}", self.serial)
    }

    fn paragraph_style(&mut self, p: &ParaProps) -> String {
        let name = self.id("p");
        let style = if (p.style as usize) < self.doc.styles.styles.len() {
            p.style
        } else {
            0
        };
        let _ = write!(
            self.automatic,
            r#"<style:style style:name="{name}" style:family="paragraph" style:parent-style-name="s{style}">{} </style:style>"#,
            if *p == self.doc.styles.get(p.style).pap {
                String::new()
            } else {
                para_props(p)
            }
        );
        name
    }

    /// A table-cell style for a cell's borders, shading and vertical
    /// alignment, one per distinct set.
    fn cell_style(&mut self, cell: &CellProps) -> String {
        let name = self.id("c");
        let b = &cell.borders;
        let mut props = String::new();
        for (edge, line) in [
            ("top", b.top),
            ("bottom", b.bottom),
            ("left", b.left),
            ("right", b.right),
        ] {
            let _ = write!(props, r#" fo:border-{edge}="{}""#, border_value(&line));
        }
        if let Some(color) = cell.shading_color {
            let _ = write!(props, r##" fo:background-color="#{:06x}""##, color.0);
        } else if cell.shading > 0 {
            let gray = 255 - (255u32 * cell.shading.min(1000) as u32 / 1000);
            let _ = write!(
                props,
                r##" fo:background-color="#{gray:02x}{gray:02x}{gray:02x}""##
            );
        }
        match cell.valign {
            VAlign::Top => {}
            VAlign::Center => props.push_str(r#" style:vertical-align="middle""#),
            VAlign::Bottom => props.push_str(r#" style:vertical-align="bottom""#),
        }
        let _ = write!(
            self.automatic,
            r#"<style:style style:name="{name}" style:family="table-cell"><style:table-cell-properties{props}/></style:style>"#
        );
        name
    }

    fn paragraph(&mut self, p: &Paragraph, out: &mut String, in_note: bool) {
        let name = self.paragraph_style(&p.pap);
        let _ = write!(out, r#"<text:p text:style-name="{name}">"#);
        let mut start = 0;
        for run in &p.runs {
            let t = self.id("t");
            let _ = write!(
                self.automatic,
                r#"<style:style style:name="{t}" style:family="text">{}</style:style>"#,
                text_props(&run.chp, Some(&self.doc.styles.get(p.pap.style).chp))
            );
            // A linked run's text goes in a text:a inside its span.
            let (a_open, a_close) = match quill_core::link::get(run.chp.link) {
                Some(l) => (
                    format!(
                        r#"<text:a xlink:type="simple" xlink:href="{}"{}>"#,
                        escape(&l.url),
                        if l.new_window {
                            r#" office:target-frame-name="_blank" xlink:show="new""#
                        } else {
                            ""
                        }
                    ),
                    "</text:a>",
                ),
                None => (String::new(), ""),
            };
            let _ = write!(out, r#"<text:span text:style-name="{t}">{a_open}"#);
            let end = start + run.len;
            for c in p.text[start..end].chars() {
                match c {
                    ' ' => out.push_str("<text:s/>"),
                    '\t' => out.push_str("<text:tab/>"),
                    chars::LINE_BREAK | '\n' => out.push_str("<text:line-break/>"),
                    chars::PAGE_BREAK => {
                        let mut pap = p.pap.clone();
                        pap.page_break_before = true;
                        let next = self.paragraph_style(&pap);
                        let _ = write!(
                            out,
                            r#"{a_close}</text:span></text:p><text:p text:style-name="{next}"><text:span text:style-name="{t}">{a_open}"#
                        );
                    }
                    chars::OBJECT => {
                        let id = run.chp.object;
                        if let Some(pic) = self.doc.picture(id) {
                            self.pictures.insert(id);
                            let _ = write!(
                                out,
                                r#"<draw:frame text:anchor-type="as-char" svg:width="{}pt" svg:height="{}pt"><draw:image xlink:href="Pictures/image{id}.{}" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/></draw:frame>"#,
                                points(pic.size.0),
                                points(pic.size.1),
                                pic.format.extension()
                            );
                        }
                    }
                    chars::FOOTNOTE if !in_note => {
                        let id = run.chp.object;
                        if let Some(range) = self.notes.get(&id).cloned() {
                            let label =
                                escape(self.labels.get(&id).map(String::as_str).unwrap_or("*"));
                            let _ = write!(
                                out,
                                r#"<text:note text:id="note{id}" text:note-class="footnote"><text:note-citation>{label}</text:note-citation><text:note-body>"#
                            );
                            for p in &self.doc.notes.paras[range] {
                                self.paragraph(p, out, true);
                            }
                            out.push_str("</text:note-body></text:note>");
                        }
                    }
                    chars::FOOTNOTE => {}
                    chars::ANNOTATION => {
                        if !in_note {
                            let _ = write!(
                                out,
                                "<office:annotation><text:p>{}</text:p></office:annotation>",
                                escape(&self.doc.annotation_text(run.chp.object))
                            );
                        }
                    }
                    chars::FIELD_PAGE => out.push_str("<text:page-number>1</text:page-number>"),
                    chars::FIELD_DATE => out.push_str("<text:date>[Date]</text:date>"),
                    chars::FIELD_TIME => out.push_str("<text:time>[Time]</text:time>"),
                    c => out.push_str(&escape(&c.to_string())),
                }
            }
            out.push_str(a_close);
            out.push_str("</text:span>");
            start = end;
        }
        out.push_str("</text:p>\n");
    }

    fn story(&mut self, paras: &[Paragraph], out: &mut String) {
        self.story_at(paras, 0, out);
    }

    /// The paragraphs of a story, or of a cell `level` tables deep.
    fn story_at(&mut self, paras: &[Paragraph], level: u8, out: &mut String) {
        for block in blocks_at(paras, level) {
            match block {
                Block::Paragraph(p) => self.paragraph(p, out, false),
                Block::Table(rows) => {
                    let name = self.id("Table");
                    let columns = rows.iter().map(|r| r.cells.len()).max().unwrap_or(1);
                    let _ = write!(
                        out,
                        r#"<table:table table:name="{name}"><table:table-column table:number-columns-repeated="{columns}"/>"#
                    );
                    for row in rows {
                        out.push_str("<table:table-row>");
                        let props =
                            quill_core::table::row_props(paras, &row, self.doc.props.text_width());
                        let n = row.cells.len();
                        for (i, (a, b)) in row.cells.into_iter().enumerate() {
                            let style = self.cell_style(&props.cells[i]);
                            let _ = write!(
                                out,
                                "<table:table-cell table:style-name=\"{style}\" office:value-type=\"string\">"
                            );
                            self.story_at(&paras[a..=b], level + 1, out);
                            out.push_str("</table:table-cell>");
                        }
                        for _ in n..columns {
                            out.push_str("<table:table-cell><text:p/></table:table-cell>");
                        }
                        out.push_str("</table:table-row>");
                    }
                    out.push_str("</table:table>\n");
                }
            }
        }
    }
}

pub fn write(doc: &Document, now: u64) -> Vec<u8> {
    let mut writer = Writer {
        doc,
        automatic: String::new(),
        pictures: BTreeSet::new(),
        serial: 0,
        labels: doc.note_labels(None),
        notes: doc.note_ranges(),
    };
    let mut body = String::new();
    writer.story(&doc.main.paras, &mut body);
    let mut fonts = String::new();
    for (id, name) in doc.fonts.iter().enumerate() {
        // CSS-style font-family strings need quoting inside this XML attribute.
        let name = name.replace('\\', "\\\\").replace('\'', "\\'");
        let _ = write!(
            fonts,
            r#"<style:font-face style:name="f{id}" svg:font-family="{}"/>"#,
            escape(&format!("'{name}'"))
        );
    }
    let mut named = String::new();
    for (id, style) in doc.styles.styles.iter().enumerate() {
        let _ = write!(
            named,
            r#"<style:style style:name="s{id}" style:display-name="{}" style:family="paragraph">{}{}</style:style>"#,
            escape(&style.name),
            para_props(&style.pap),
            text_props(&style.chp, None)
        );
    }
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content {NS}><office:font-face-decls>{fonts}</office:font-face-decls><office:automatic-styles>{}</office:automatic-styles><office:body><office:text>{body}</office:text></office:body></office:document-content>"#,
        writer.automatic
    );
    let p = &doc.props;
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-styles {NS}><office:font-face-decls>{fonts}</office:font-face-decls><office:styles>{named}</office:styles><office:automatic-styles><style:page-layout style:name="Page"><style:page-layout-properties fo:page-width="{}pt" fo:page-height="{}pt" style:print-orientation="{}" fo:margin-top="{}pt" fo:margin-bottom="{}pt" fo:margin-left="{}pt" fo:margin-right="{}pt"/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name="Standard" style:page-layout-name="Page"/></office:master-styles></office:document-styles>"#,
        points(p.page_width),
        points(p.page_height),
        if p.page_width > p.page_height {
            "landscape"
        } else {
            "portrait"
        },
        points(p.margin_top),
        points(p.margin_bottom),
        points(p.margin_left),
        points(p.margin_right)
    );
    let mut manifest = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3"><manifest:file-entry manifest:full-path="/" manifest:media-type="{MIME}"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="meta.xml" manifest:media-type="text/xml"/>"#
    );
    let mut zip = zip::Writer::new(now);
    zip.add_stored("mimetype", MIME.as_bytes());
    zip.add("content.xml", content.as_bytes());
    zip.add("styles.xml", styles.as_bytes());
    let meta = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-meta {NS}><office:meta><dc:title>{}</dc:title><dc:creator>{}</dc:creator><dc:subject>{}</dc:subject></office:meta></office:document-meta>"#,
        escape(&doc.summary.title),
        escape(&doc.summary.author),
        escape(&doc.summary.subject)
    );
    zip.add("meta.xml", meta.as_bytes());
    for id in writer.pictures {
        let pic = doc.picture(id).unwrap();
        let path = format!("Pictures/image{id}.{}", pic.format.extension());
        zip.add(&path, &pic.data);
        let _ = write!(
            manifest,
            r#"<manifest:file-entry manifest:full-path="{path}" manifest:media-type="{}"/>"#,
            pic.format.mime()
        );
    }
    manifest.push_str("</manifest:manifest>");
    zip.add("META-INF/manifest.xml", manifest.as_bytes());
    zip.finish()
}
