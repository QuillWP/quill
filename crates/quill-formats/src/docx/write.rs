//! Writing .docx (Office Open XML WordprocessingML).
//!
//! Styles are written as differences from the style they are based on,
//! and direct formatting as differences from the paragraph's style, the
//! way Word itself writes files. Element order follows the schema, which
//! Word checks.

use super::names;
use crate::zip;
use quill_core::document::{FootnotePosition, SavedView, blank_header};
use quill_core::para::Mark;
use quill_core::para::{Paragraph, chars};
use quill_core::props::{
    Align, BorderLine, BorderStyle, Borders, CharProps, LineSpacing, ParaProps, Rgb, StyleId,
    TabAlign, TabLeader, Underline,
};
use quill_core::{Document, PageNumberFormat, SectionProps, SectionStart, StoryId};
use std::fmt::Write as _;

pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // XML 1.0 cannot carry most control characters at all.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

const W_NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:quill="urn:quill:docx:1" mc:Ignorable="quill""#;

/// The pictures the document uses: object id and file name in word/media.
fn pictures(doc: &Document) -> Vec<(u32, String)> {
    let own: Vec<&quill_core::Story> = (1..doc.sections().len())
        .filter(|&k| doc.has_own_headers(k))
        .flat_map(|k| doc.headers_of(k).iter())
        .collect();
    // Every story that is written: the notes and comments hold pictures
    // too, and a picture missing from here is lost on saving.
    let mut ids: Vec<u32> = std::iter::once(&doc.main)
        .chain([&doc.notes, &doc.annotations])
        .chain(doc.headers.iter())
        .chain(own)
        .flat_map(|s| s.paras.iter())
        .flat_map(|p| {
            p.text
                .char_indices()
                .filter(|&(_, c)| c == quill_core::para::chars::OBJECT)
                .map(|(at, _)| p.chp_of_char(at).object)
        })
        .filter(|&id| doc.picture(id).is_some())
        .collect();
    ids.sort();
    ids.dedup();
    let mut files: Vec<(u32, String)> = Vec::new();
    ids.into_iter()
        .map(|id| {
            let pic = doc.picture(id).expect("picture id was checked above");
            let name = files
                .iter()
                .find_map(|(other, name)| {
                    (doc.picture(*other).is_some_and(|p| p.data == pic.data)).then(|| name.clone())
                })
                .unwrap_or_else(|| format!("media/image{id}.{}", pic.format.extension()));
            files.push((id, name.clone()));
            (id, name)
        })
        .collect()
}

/// Relationships for the pictures, for each part that may show them.
fn picture_rels(pics: &[(u32, String)]) -> String {
    pics.iter()
        .map(|(id, name)| {
            format!(
                r#"<Relationship Id="rIdPic{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="{name}"/>"#
            )
        })
        .collect()
}

/// A picture as an inline drawing.
fn drawing(id: u32, pic: &quill_core::Picture, out: &mut String) {
    let (cx, cy) = (pic.size.0 as i64 * 635, pic.size.1 as i64 * 635);
    let c = &pic.crop;
    let crop = if *c == quill_core::Crop::default() {
        String::new()
    } else {
        format!(
            r#"<a:srcRect l="{}" t="{}" r="{}" b="{}"/>"#,
            c.left, c.top, c.right, c.bottom
        )
    };
    let _ = write!(
        out,
        r#"<w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{id}" name="Picture {id}"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="{id}" name="Picture {id}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rIdPic{id}"/>{crop}<a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing>"#
    );
}

/// Write `doc` as a .docx file's bytes. `now` is Unix time for the zip
/// entry dates and the document's modified date.
pub fn write(doc: &Document, now: u64) -> Vec<u8> {
    let ids = names::style_ids(doc);
    let mut z = zip::Writer::new(now);
    let parts = header_parts(doc);
    let mut types = CONTENT_TYPES.to_string();
    let mut rels = DOC_RELS.to_string();
    let pics = pictures(doc);
    let pic_rels = picture_rels(&pics);
    for ext in ["png", "jpeg"] {
        if pics.iter().any(|(_, n)| n.ends_with(ext)) {
            let at = types.rfind("</Types>").unwrap_or(types.len());
            types.insert_str(
                at,
                &format!(r#"<Default Extension="{ext}" ContentType="image/{ext}"/>"#),
            );
        }
    }
    let at = rels.rfind("</Relationships>").unwrap_or(rels.len());
    rels.insert_str(at, &pic_rels);
    for (_, id, name, rid) in &parts {
        let kind = if id.is_footer() { "footer" } else { "header" };
        let at = types.rfind("</Types>").unwrap_or(types.len());
        types.insert_str(
            at,
            &format!(
                r#"<Override PartName="/word/{name}" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.{kind}+xml"/>"#
            ),
        );
        let at = rels.rfind("</Relationships>").unwrap_or(rels.len());
        rels.insert_str(
            at,
            &format!(
                r#"<Relationship Id="{rid}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}" Target="{name}"/>"#
            ),
        );
    }
    // Footnotes, as endnotes when they go at the end of sections or of
    // the document.
    let notes = !quill_core::notes::note_ids(&doc.main.paras).is_empty();
    let end = ends_notes(doc);
    let kind = if end { "endnotes" } else { "footnotes" };
    if notes {
        let at = types.rfind("</Types>").unwrap_or(types.len());
        types.insert_str(
            at,
            &format!(
                r#"<Override PartName="/word/{kind}.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.{kind}+xml"/>"#
            ),
        );
        let at = rels.rfind("</Relationships>").unwrap_or(rels.len());
        rels.insert_str(
            at,
            &format!(
                r#"<Relationship Id="rIdNotes" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}" Target="{kind}.xml"/>"#
            ),
        );
    }
    // Annotations, as comments, with their replies and resolved state in
    // Word's commentsExtended part.
    let comments = !quill_core::notes::annotation_ids(&doc.main.paras).is_empty();
    if comments {
        let at = types.rfind("</Types>").unwrap_or(types.len());
        types.insert_str(
            at,
            r#"<Override PartName="/word/comments.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"/><Override PartName="/word/commentsExtended.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.commentsExtended+xml"/>"#,
        );
        let at = rels.rfind("</Relationships>").unwrap_or(rels.len());
        rels.insert_str(
            at,
            r#"<Relationship Id="rIdComments" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="comments.xml"/><Relationship Id="rIdCommentsEx" Type="http://schemas.microsoft.com/office/2011/relationships/commentsExtended" Target="commentsExtended.xml"/>"#,
        );
    }
    z.add("[Content_Types].xml", types.as_bytes());
    z.add("_rels/.rels", ROOT_RELS.as_bytes());
    z.add("word/_rels/document.xml.rels", rels.as_bytes());
    let mut body = document_xml(doc, &ids);
    if end {
        body = body.replace("<w:footnoteReference ", "<w:endnoteReference ");
    }
    z.add("word/document.xml", body.as_bytes());
    if comments {
        z.add("word/comments.xml", comments_xml(doc, &ids).as_bytes());
        z.add(
            "word/commentsExtended.xml",
            comments_extended_xml(doc).as_bytes(),
        );
        if !pics.is_empty() {
            let r = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{pic_rels}</Relationships>"#
            );
            z.add("word/_rels/comments.xml.rels", r.as_bytes());
        }
    }
    if notes {
        z.add(
            &format!("word/{kind}.xml"),
            notes_xml(doc, &ids, end).as_bytes(),
        );
        if !pics.is_empty() {
            let r = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{pic_rels}</Relationships>"#
            );
            z.add(&format!("word/_rels/{kind}.xml.rels"), r.as_bytes());
        }
    }
    for (k, id, name, _) in &parts {
        z.add(
            &format!("word/{name}"),
            header_xml(doc, &ids, *k, *id).as_bytes(),
        );
        if !pics.is_empty() {
            let r = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{pic_rels}</Relationships>"#
            );
            z.add(&format!("word/_rels/{name}.rels"), r.as_bytes());
        }
    }
    let mut written = Vec::new();
    for (id, name) in &pics {
        if written.contains(name) {
            continue;
        }
        if let Some(p) = doc.picture(*id) {
            z.add(&format!("word/{name}"), &p.data);
            written.push(name.clone());
        }
    }
    z.add("word/styles.xml", styles_xml(doc, &ids).as_bytes());
    z.add("word/settings.xml", settings_xml(doc).as_bytes());
    z.add("word/fontTable.xml", font_table_xml(doc).as_bytes());
    z.add("docProps/core.xml", core_xml(doc, now).as_bytes());
    z.add("docProps/app.xml", APP_XML.as_bytes());
    z.finish()
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/><Override PartName="/word/fontTable.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>"#;

const DOC_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/fontTable" Target="fontTable.xml"/></Relationships>"#;

const APP_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>Quill</Application></Properties>"#;

fn on_off(out: &mut String, tag: &str, value: bool, base: bool) {
    if value != base {
        if value {
            let _ = write!(out, "<w:{tag}/>");
        } else {
            let _ = write!(out, r#"<w:{tag} w:val="0"/>"#);
        }
    }
}

/// Character properties that differ from `base`, as the inside of rPr.
fn rpr(doc: &Document, c: &CharProps, base: &CharProps) -> String {
    let Some(style) = doc.styles.character(c.character_style) else {
        return rpr_values(doc, c, base, 0);
    };
    let effective = style.format.resolved(*base);
    // Paragraph IDs contain only letters and digits, so this prefix cannot collide.
    let mut out = format!(
        r#"<w:rStyle w:val="Quill_Character_{}"/>"#,
        c.character_style
    );
    out.push_str(&rpr_values(doc, c, &effective, c.direct_format));
    out
}

fn rpr_values(doc: &Document, c: &CharProps, base: &CharProps, force: u16) -> String {
    let mut o = String::new();
    if c.font != base.font || force & (1 << 0) != 0 {
        let f = esc(doc.font_name(c.font));
        let _ = write!(o, r#"<w:rFonts w:ascii="{f}" w:hAnsi="{f}" w:cs="{f}"/>"#);
    }
    on_off(
        &mut o,
        "b",
        c.bold,
        if force & (1 << 2) != 0 {
            !c.bold
        } else {
            base.bold
        },
    );
    on_off(
        &mut o,
        "i",
        c.italic,
        if force & (1 << 3) != 0 {
            !c.italic
        } else {
            base.italic
        },
    );
    on_off(
        &mut o,
        "caps",
        c.all_caps,
        if force & (1 << 9) != 0 {
            !c.all_caps
        } else {
            base.all_caps
        },
    );
    on_off(
        &mut o,
        "smallCaps",
        c.small_caps,
        if force & (1 << 8) != 0 {
            !c.small_caps
        } else {
            base.small_caps
        },
    );
    on_off(
        &mut o,
        "strike",
        c.strike,
        if force & (1 << 5) != 0 {
            !c.strike
        } else {
            base.strike
        },
    );
    on_off(
        &mut o,
        "outline",
        c.outline,
        if force & (1 << 6) != 0 {
            !c.outline
        } else {
            base.outline
        },
    );
    on_off(
        &mut o,
        "shadow",
        c.shadow,
        if force & (1 << 7) != 0 {
            !c.shadow
        } else {
            base.shadow
        },
    );
    on_off(
        &mut o,
        "vanish",
        c.hidden,
        if force & (1 << 10) != 0 {
            !c.hidden
        } else {
            base.hidden
        },
    );
    if c.color != base.color || force & (1 << 11) != 0 {
        let _ = write!(o, r#"<w:color w:val="{:06X}"/>"#, c.color.0);
    }
    if c.spacing != base.spacing || force & (1 << 13) != 0 {
        let _ = write!(o, r#"<w:spacing w:val="{}"/>"#, c.spacing);
    }
    if c.position != base.position || force & (1 << 12) != 0 {
        // .docx measures the baseline shift in half-points.
        let _ = write!(
            o,
            r#"<w:position w:val="{}"/>"#,
            crate::half_points(c.position)
        );
    }
    if c.size != base.size || force & (1 << 1) != 0 {
        let _ = write!(o, r#"<w:sz w:val="{0}"/><w:szCs w:val="{0}"/>"#, c.size);
    }
    if c.underline != base.underline || force & (1 << 4) != 0 {
        let u = match c.underline {
            Underline::None => "none",
            Underline::Single => "single",
            Underline::Word => "words",
            Underline::Double => "double",
            Underline::Dotted => "dotted",
        };
        let _ = write!(o, r#"<w:u w:val="{u}"/>"#);
    }
    if c.script != base.script || force & (1 << 14) != 0 {
        let v = match c.script {
            quill_core::Script::None => "baseline",
            quill_core::Script::Super => "superscript",
            quill_core::Script::Sub => "subscript",
        };
        let _ = write!(o, r#"<w:vertAlign w:val="{v}"/>"#);
    }
    o
}

fn border(out: &mut String, tag: &str, b: &BorderLine, shadow: bool) {
    let (val, sz) = match b.style {
        // Said outright, so the side doesn't come from the style.
        BorderStyle::None => {
            let _ = write!(out, r#"<w:{tag} w:val="nil"/>"#);
            return;
        }
        BorderStyle::Dotted => ("dotted", 4),
        BorderStyle::Hairline => ("single", 2),
        BorderStyle::Single => ("single", 4),
        BorderStyle::Thick => ("single", 12),
        BorderStyle::Double => ("double", 4),
    };
    let shadow = if shadow { r#" w:shadow="1""# } else { "" };
    let color = if b.color == Rgb::BLACK {
        "auto".to_string()
    } else {
        format!("{:06X}", b.color.0 & 0xFF_FFFF)
    };
    let _ = write!(
        out,
        r#"<w:{tag} w:val="{val}" w:sz="{sz}" w:space="{}" w:color="{color}"{shadow}/>"#,
        b.space / 20
    );
}

/// Every side of the borders, so none is taken from the style.
fn borders(out: &mut String, b: &Borders) {
    out.push_str("<w:pBdr>");
    border(out, "top", &b.top, false);
    border(out, "left", &b.left, false);
    border(out, "bottom", &b.bottom, b.shadow);
    border(out, "right", &b.right, b.shadow);
    border(out, "between", &b.between, false);
    out.push_str("</w:pBdr>");
}

fn shading_val(tenths: u16) -> &'static str {
    match tenths {
        0 => "clear",
        1..=75 => "pct5",
        76..=112 => "pct10",
        113..=137 => "pct12",
        138..=175 => "pct15",
        176..=225 => "pct20",
        226..=275 => "pct25",
        276..=325 => "pct30",
        326..=362 => "pct35",
        363..=387 => "pct37",
        388..=425 => "pct40",
        426..=475 => "pct45",
        476..=525 => "pct50",
        526..=575 => "pct55",
        576..=612 => "pct60",
        613..=637 => "pct62",
        638..=675 => "pct65",
        676..=725 => "pct70",
        726..=775 => "pct75",
        776..=825 => "pct80",
        826..=862 => "pct85",
        863..=887 => "pct87",
        888..=925 => "pct90",
        926..=975 => "pct95",
        _ => "solid",
    }
}

/// Paragraph properties that differ from `base`, as the inside of pPr
/// (without pStyle and rPr, which callers add).
/// A frame as `w:framePr`.
fn frame_pr(o: &mut String, f: &quill_core::Frame) {
    use quill_core::{FrameH, FrameRel, FrameV};
    o.push_str("<w:framePr");
    if f.drop_lines > 0 {
        let kind = if f.in_margin { "margin" } else { "drop" };
        let _ = write!(o, r#" w:dropCap="{kind}" w:lines="{}""#, f.drop_lines);
    }
    if let Some(w) = f.width {
        let _ = write!(o, r#" w:w="{w}""#);
    }
    let _ = write!(
        o,
        r#" w:hSpace="{0}" w:vSpace="{0}" w:wrap="around""#,
        f.from_text
    );
    let anchor = |r: FrameRel| match r {
        FrameRel::Column => "text",
        FrameRel::Margin => "margin",
        FrameRel::Page => "page",
    };
    let v_anchor = if f.v == FrameV::InLine {
        "text"
    } else {
        anchor(f.v_rel)
    };
    let _ = write!(
        o,
        r#" w:hAnchor="{}" w:vAnchor="{v_anchor}""#,
        anchor(f.h_rel)
    );
    let _ = match f.h {
        FrameH::At(t) => write!(o, r#" w:x="{t}""#),
        h => write!(
            o,
            r#" w:xAlign="{}""#,
            match h {
                FrameH::Center => "center",
                FrameH::Right => "right",
                FrameH::Inside => "inside",
                FrameH::Outside => "outside",
                _ => "left",
            }
        ),
    };
    let _ = match f.v {
        FrameV::At(t) => write!(o, r#" w:y="{t}""#),
        FrameV::InLine => Ok(()),
        v => write!(
            o,
            r#" w:yAlign="{}""#,
            match v {
                FrameV::Center => "center",
                FrameV::Bottom => "bottom",
                _ => "top",
            }
        ),
    };
    o.push_str("/>");
}

fn ppr(p: &ParaProps, base: &ParaProps) -> String {
    let mut o = String::new();
    on_off(&mut o, "keepNext", p.keep_with_next, base.keep_with_next);
    on_off(&mut o, "keepLines", p.keep_together, base.keep_together);
    on_off(
        &mut o,
        "pageBreakBefore",
        p.page_break_before,
        base.page_break_before,
    );
    if let Some(f) = &p.frame
        && p.frame != base.frame
    {
        frame_pr(&mut o, f);
    }
    on_off(
        &mut o,
        "suppressLineNumbers",
        p.suppress_line_numbers,
        base.suppress_line_numbers,
    );
    if p.borders != base.borders {
        borders(&mut o, &p.borders);
    }
    if p.shading != base.shading || p.shading_color != base.shading_color {
        if let Some(color) = p.shading_color {
            let _ = write!(
                o,
                r#"<w:shd w:val="clear" w:color="auto" w:fill="{:06X}"/>"#,
                color.0
            );
        } else {
            let _ = write!(
                o,
                r#"<w:shd w:val="{}" w:color="auto" w:fill="auto"/>"#,
                shading_val(p.shading)
            );
        }
    }
    if p.tabs != base.tabs {
        o.push_str("<w:tabs>");
        for t in &base.tabs {
            if !p.tabs.contains(t) {
                let _ = write!(o, r#"<w:tab w:val="clear" w:pos="{}"/>"#, t.pos);
            }
        }
        for t in &p.tabs {
            if base.tabs.contains(t) {
                continue;
            }
            let val = match t.align {
                TabAlign::Left => "left",
                TabAlign::Center => "center",
                TabAlign::Right => "right",
                TabAlign::Decimal => "decimal",
                TabAlign::Bar => "bar",
            };
            let leader = match t.leader {
                TabLeader::None => "",
                TabLeader::Dots => r#" w:leader="dot""#,
                TabLeader::Dashes => r#" w:leader="hyphen""#,
                TabLeader::Underline => r#" w:leader="underscore""#,
            };
            let _ = write!(o, r#"<w:tab w:val="{val}"{leader} w:pos="{}"/>"#, t.pos);
        }
        o.push_str("</w:tabs>");
    }
    if p.before != base.before || p.after != base.after || p.line != base.line {
        let (line, rule) = match p.line {
            LineSpacing::Auto => (240, "auto"),
            LineSpacing::AtLeast(t) => (t, "atLeast"),
            LineSpacing::Exactly(t) => (t, "exact"),
            LineSpacing::Multiple(m) => (m as i32, "auto"),
        };
        let _ = write!(
            o,
            r#"<w:spacing w:before="{}" w:after="{}" w:line="{line}" w:lineRule="{rule}"/>"#,
            p.before, p.after
        );
    }
    if p.left != base.left || p.right != base.right || p.first != base.first {
        let first = if p.first < 0 {
            format!(r#"w:hanging="{}""#, -p.first)
        } else {
            format!(r#"w:firstLine="{}""#, p.first)
        };
        let _ = write!(
            o,
            r#"<w:ind w:left="{}" w:right="{}" {first}/>"#,
            p.left, p.right
        );
    }
    if p.align != base.align {
        let jc = match p.align {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "both",
        };
        let _ = write!(o, r#"<w:jc w:val="{jc}"/>"#);
    }
    o
}

/// Numbers for tracked changes' `w:id`, which must differ within a file.
static CHANGE_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

/// A tracked change's `w:id`, `w:author` and `w:date` attributes.
fn change_attrs(by: &quill_core::revision::Stamp) -> String {
    let id = CHANGE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut s = format!(r#" w:id="{id}" w:author="{}""#, esc(&by.author));
    if !by.date.is_empty() {
        let _ = write!(s, r#" w:date="{}""#, esc(&by.date));
    }
    s
}

/// What goes round a run with tracked change `rev`: its opening w:ins
/// and w:del, their closing tags, and whether its text is deleted (and
/// so written as w:delText).
fn change_wrap(rev: u32) -> (String, String, bool) {
    let Some(r) = quill_core::revision::get(rev) else {
        return (String::new(), String::new(), false);
    };
    let (mut open, mut close) = (String::new(), String::new());
    if let Some(by) = &r.inserted {
        let _ = write!(open, "<w:ins{}>", change_attrs(by));
        close.insert_str(0, "</w:ins>");
    }
    if let Some(by) = &r.deleted {
        let _ = write!(open, "<w:del{}>", change_attrs(by));
        close.insert_str(0, "</w:del>");
    }
    (open, close, r.deleted.is_some())
}

/// A run's or mark's rPr with its tracked format change, which the
/// schema puts last.
fn rpr_tracked(doc: &Document, c: &CharProps, base: &CharProps) -> String {
    let mut o = rpr(doc, c, base);
    if let Some(r) = quill_core::revision::get(c.rev)
        && let Some((by, old)) = &r.formatted
    {
        let _ = write!(
            o,
            "<w:rPrChange{}><w:rPr>{}</w:rPr></w:rPrChange>",
            change_attrs(by),
            rpr(doc, old, base)
        );
    }
    o
}

fn paragraph(doc: &Document, ids: &[String], p: &Paragraph, out: &mut String) {
    let style = doc.styles.get(p.pap.style);
    out.push_str("<w:p><w:pPr>");
    if p.pap.style != 0 {
        let _ = write!(
            out,
            r#"<w:pStyle w:val="{}"/>"#,
            esc(&ids[p.pap.style as usize])
        );
    }
    let mut base = style.pap.clone();
    base.in_table = p.pap.in_table;
    out.push_str(&ppr(&p.pap, &base));
    // A tracked change to the mark itself comes first.
    let mut mark = String::new();
    if let Some(r) = quill_core::revision::get(p.mark_chp.rev) {
        if let Some(by) = &r.inserted {
            let _ = write!(mark, "<w:ins{}/>", change_attrs(by));
        }
        if let Some(by) = &r.deleted {
            let _ = write!(mark, "<w:del{}/>", change_attrs(by));
        }
    }
    mark.push_str(&rpr_tracked(doc, &p.mark_chp, &style.chp));
    if !mark.is_empty() {
        let _ = write!(out, "<w:rPr>{mark}</w:rPr>");
    }
    // A section break: the settings of the section it ends.
    if p.mark == Mark::SectionBreak {
        // Which section it ends: breaks before it in the main text.
        let k = doc
            .main
            .paras
            .iter()
            .take_while(|q| !std::ptr::eq(*q, p))
            .filter(|q| q.mark == Mark::SectionBreak)
            .count();
        section_props(doc, k, p.section.as_deref().unwrap_or(&doc.section), out);
    }
    out.push_str("</w:pPr>");
    let main = main_index(doc, p);
    // A live table of contents is a TOC field, so Word can update it too.
    let toc = doc.props.live_toc.zip(doc.existing_toc());
    if let (Some((o, _)), Some(0)) = (toc, main) {
        let _ = write!(
            out,
            r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> {} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r>"#,
            toc_instruction(&o)
        );
    }
    // Hyperlinks become HYPERLINK fields around their runs: these work the
    // same in every part, with no relationships to keep.
    let mut link = 0;
    for (s, e, chp) in p.run_bounds() {
        comment_marks(out, main.map(|i| (i, s)), true);
        if chp.link != link {
            if link != 0 {
                out.push_str(r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#);
            }
            link = chp.link;
            if let Some(l) = quill_core::link::get(link) {
                let target = if l.new_window { r#" \t "_blank""# } else { "" };
                let _ = write!(
                    out,
                    r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> HYPERLINK "{}"{} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r>"#,
                    esc(&field_quote(&l.url)),
                    esc(target)
                );
            } else {
                link = 0;
            }
        }
        let (open, close, deleted) = change_wrap(chp.rev);
        if chp.object != 0 && p.text[s..e].contains(chars::OBJECT) {
            if let Some(pic) = doc.picture(chp.object) {
                let props = rpr_tracked(doc, chp, &style.chp);
                for _ in p.text[s..e].chars() {
                    out.push_str(&open);
                    out.push_str("<w:r>");
                    if !props.is_empty() {
                        let _ = write!(out, "<w:rPr>{props}</w:rPr>");
                    }
                    drawing(chp.object, pic, out);
                    out.push_str("</w:r>");
                    out.push_str(&close);
                }
            }
            comment_marks(out, main.map(|i| (i, e)), false);
            continue;
        }
        let (t, instr_text) = if deleted {
            ("delText", "delInstrText")
        } else {
            ("t", "instrText")
        };
        let props = rpr_tracked(doc, chp, &style.chp);
        out.push_str(&open);
        out.push_str("<w:r>");
        if !props.is_empty() {
            let _ = write!(out, "<w:rPr>{props}</w:rPr>");
        }
        let mut text = String::new();
        let flush = |text: &mut String, out: &mut String| {
            if !text.is_empty() {
                let _ = write!(out, r#"<w:{t} xml:space="preserve">{}</w:{t}>"#, esc(text));
                text.clear();
            }
        };
        for c in p.text[s..e].chars() {
            let instr = match c {
                chars::FIELD_PAGE => Some((" PAGE ", "1")),
                chars::FIELD_DATE => Some((r#" DATE \@ "M/d/yy" "#, "")),
                chars::FIELD_TIME => Some((r#" TIME \@ "h:mm AM/PM" "#, "")),
                _ => None,
            };
            if let Some((instr, shown)) = instr {
                // Close this run, write the field as Word's own runs, and
                // start the run again after it.
                flush(&mut text, out);
                out.push_str("</w:r>");
                let r = if props.is_empty() {
                    String::new()
                } else {
                    format!("<w:rPr>{props}</w:rPr>")
                };
                let _ = write!(
                    out,
                    r#"<w:r>{r}<w:fldChar w:fldCharType="begin"/></w:r><w:r>{r}<w:{instr_text} xml:space="preserve">{}</w:{instr_text}></w:r><w:r>{r}<w:fldChar w:fldCharType="separate"/></w:r><w:r>{r}<w:{t}>{}</w:{t}></w:r><w:r>{r}<w:fldChar w:fldCharType="end"/></w:r><w:r>{r}"#,
                    esc(instr),
                    shown
                );
                continue;
            }
            let special = match c {
                chars::TAB => "<w:tab/>",
                chars::LINE_BREAK => "<w:br/>",
                chars::PAGE_BREAK => r#"<w:br w:type="page"/>"#,
                chars::OPTIONAL_HYPHEN => "<w:softHyphen/>",
                chars::NONBREAKING_HYPHEN => "<w:noBreakHyphen/>",
                chars::OBJECT => "",
                chars::FOOTNOTE => {
                    // A footnote reference; in the note itself this
                    // becomes its own mark (see `notes_xml`).
                    flush(&mut text, out);
                    match doc.note_marks.get(&chp.object) {
                        Some(m) => {
                            let _ = write!(
                                out,
                                r#"<w:footnoteReference w:customMarkFollows="1" w:id="{}"/><w:{t} xml:space="preserve">{}</w:{t}>"#,
                                chp.object,
                                esc(m)
                            );
                        }
                        None => {
                            let _ = write!(out, r#"<w:footnoteReference w:id="{}"/>"#, chp.object);
                        }
                    }
                    continue;
                }
                chars::ANNOTATION => {
                    // An annotation mark; in the annotation itself this
                    // becomes its own mark (see `comments_xml`).
                    flush(&mut text, out);
                    let _ = write!(out, r#"<w:commentReference w:id="{}"/>"#, chp.object);
                    continue;
                }
                _ => {
                    text.push(c);
                    continue;
                }
            };
            flush(&mut text, out);
            out.push_str(special);
        }
        flush(&mut text, out);
        out.push_str("</w:r>");
        out.push_str(&close);
        comment_marks(out, main.map(|i| (i, e)), false);
    }
    if link != 0 {
        out.push_str(r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#);
    }
    if toc.is_some_and(|(_, end)| main == Some(end)) {
        out.push_str(r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#);
    }
    out.push_str("</w:p>");
}

/// The TOC field instruction for a table of contents made with `o`:
/// headings of the levels it lists, or TC entries; \n leaves out the
/// page numbers.
pub(crate) fn toc_instruction(o: &quill_core::contents::TocOptions) -> String {
    let mut s = if o.outline {
        format!(r#"TOC \o "{}-{}""#, o.from, o.to)
    } else {
        r#"TOC \f"#.to_string()
    };
    if !o.page_numbers {
        s.push_str(r#" \n"#);
    }
    s
}

/// The settings a TOC field instruction asks for, if it is one Quill can
/// keep up to date: headings by level (\o) or TC entries (\f).
pub(crate) fn toc_options(instr: &str) -> Option<quill_core::contents::TocOptions> {
    let mut words = instr.split_whitespace();
    if !words.next()?.eq_ignore_ascii_case("TOC") {
        return None;
    }
    let mut o = quill_core::contents::TocOptions {
        outline: false,
        ..Default::default()
    };
    let mut known = false;
    let rest: Vec<&str> = words.collect();
    for (k, w) in rest.iter().enumerate() {
        match *w {
            "\\o" => {
                o.outline = true;
                known = true;
                let range = rest.get(k + 1).map(|r| r.trim_matches('"'));
                if let Some((a, b)) = range.and_then(|r| r.split_once('-'))
                    && let (Ok(a), Ok(b)) = (a.parse::<u8>(), b.parse::<u8>())
                {
                    o.from = a.clamp(1, 9);
                    o.to = b.clamp(1, 9).max(o.from);
                }
            }
            "\\f" => known = true,
            "\\n" => o.page_numbers = false,
            _ => {}
        }
    }
    known.then_some(o)
}

/// A field argument's text, with the backslashes and quotes Word's field
/// syntax would otherwise read as its own.
pub(crate) fn field_quote(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn section_props(doc: &Document, section: usize, sec: &SectionProps, out: &mut String) {
    let p = &doc.props;
    // A negative top or bottom margin means "exactly" in .docx.
    let top = if p.top_exact {
        -p.margin_top
    } else {
        p.margin_top
    };
    let bottom = if p.bottom_exact {
        -p.margin_bottom
    } else {
        p.margin_bottom
    };
    let kind = match sec.start {
        SectionStart::NoBreak => "continuous",
        SectionStart::NewColumn => "nextColumn",
        SectionStart::NewPage => "nextPage",
        SectionStart::EvenPage => "evenPage",
        SectionStart::OddPage => "oddPage",
    };
    out.push_str("<w:sectPr>");
    for (_, id, _, rid) in header_parts(doc).into_iter().filter(|p| p.0 == section) {
        let tag = if id.is_footer() { "footer" } else { "header" };
        let kind = match id {
            StoryId::FirstHeader | StoryId::FirstFooter => "first",
            StoryId::EvenHeader | StoryId::EvenFooter => "even",
            _ => "default",
        };
        let _ = write!(out, r#"<w:{tag}Reference w:type="{kind}" r:id="{rid}"/>"#);
    }
    let _ = write!(
        out,
        r#"<w:type w:val="{kind}"/><w:pgSz w:w="{}" w:h="{}"{}/><w:pgMar w:top="{top}" w:right="{}" w:bottom="{bottom}" w:left="{}" w:header="{}" w:footer="{}" w:gutter="{}"/>"#,
        p.page_width,
        p.page_height,
        // Word takes the orientation from this, not from the sizes.
        if p.page_width > p.page_height {
            r#" w:orient="landscape""#
        } else {
            ""
        },
        p.margin_right,
        p.margin_left,
        sec.header_from_top,
        sec.footer_from_bottom,
        p.gutter
    );
    let fmt = match sec.page_number_format {
        PageNumberFormat::Arabic => "decimal",
        PageNumberFormat::UpperRoman => "upperRoman",
        PageNumberFormat::LowerRoman => "lowerRoman",
        PageNumberFormat::UpperLetter => "upperLetter",
        PageNumberFormat::LowerLetter => "lowerLetter",
    };
    let start = if sec.restart_page_numbers {
        r#" w:start="1""#
    } else {
        ""
    };
    if sec.line_numbers != quill_core::LineNumbering::Off {
        let restart = match sec.line_numbers {
            quill_core::LineNumbering::ByPage => "newPage",
            quill_core::LineNumbering::BySection => "newSection",
            _ => "continuous",
        };
        let distance = sec
            .line_from_text
            .map_or(String::new(), |t| format!(r#" w:distance="{t}""#));
        let _ = write!(
            out,
            r#"<w:lnNumType w:countBy="{}"{distance} w:restart="{restart}"/>"#,
            sec.line_count_by.max(1)
        );
    }
    let _ = write!(out, r#"<w:pgNumType w:fmt="{fmt}"{start}/>"#);
    let _ = write!(
        out,
        r#"<w:cols w:num="{}" w:space="{}"/>"#,
        sec.columns.max(1),
        sec.column_spacing
    );
    if sec.different_first_page {
        out.push_str("<w:titlePg/>");
    }
    // Footnote placement and numbering are section properties in OOXML.
    // Quill exposes them document-wide, so repeat the same values in every
    // section. Word preserves them here; putting them in settings.xml makes
    // Word replace them with only its separator-note references.
    let note_position = match p.footnote_position {
        FootnotePosition::BottomOfPage => "pageBottom",
        FootnotePosition::BeneathText => "beneathText",
        FootnotePosition::EndOfSection => "sectEnd",
        FootnotePosition::EndOfDocument => "docEnd",
    };
    let restart = if p.footnote_restart_each_page {
        r#"<w:numRestart w:val="eachPage"/>"#
    } else {
        ""
    };
    let _ = write!(
        out,
        r#"<w:footnotePr><w:pos w:val="{note_position}"/><w:numStart w:val="{}"/>{restart}</w:footnotePr>"#,
        p.footnote_start.max(1)
    );
    // Settings .docx has no place for, in Quill's own namespace, which the
    // file marks as one Word may skip (see W_NS).
    if sec.margin_page_numbers {
        let _ = write!(
            out,
            r#"<quill:marginPageNumbers quill:top="{}" quill:right="{}"/>"#,
            sec.page_number_from_top, sec.page_number_from_right
        );
    }
    if sec.include_endnotes {
        out.push_str("<quill:includeEndnotes/>");
    }
    out.push_str("</w:sectPr>");
}

/// Cells and rows are written as `w:tbl`. Cell widths come later with
/// table layout; for now columns share the text width equally.
/// The parts holding headers and footers that have text: the story, the
/// part's file name and its relationship id.
fn header_parts(doc: &Document) -> Vec<(usize, StoryId, String, String)> {
    let mut out = Vec::new();
    let sections = doc.sections().len();
    for k in 0..sections {
        if !doc.has_own_headers(k) {
            continue;
        }
        for id in StoryId::HEADERS {
            // The first section leaves out empty ones that are as Quill
            // reads a missing one. A later section with its own writes
            // them all: one it leaves out would show the section before's
            // in Word.
            let empty = *doc.header_story(k, id) == blank_header(&doc.styles, id);
            if k == 0 && empty {
                continue;
            }
            let n = out.len() + 1;
            let kind = if id.is_footer() { "footer" } else { "header" };
            out.push((k, id, format!("{kind}{n}.xml"), format!("rIdH{n}")));
        }
    }
    out
}

/// Whether the notes are written as endnotes.
fn ends_notes(doc: &Document) -> bool {
    matches!(
        doc.props.footnote_position,
        FootnotePosition::EndOfSection | FootnotePosition::EndOfDocument
    )
}

/// comments.xml: each annotation with who wrote it and when, its own
/// mark written as `annotationRef`.
fn comments_xml(doc: &Document, ids: &[String]) -> String {
    let ns = W_NS.replace(
        r#"mc:Ignorable="quill""#,
        r#"xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" mc:Ignorable="quill w14""#,
    );
    let mut out =
        format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:comments {ns}>"#);
    let ranges = doc.annotation_ranges();
    let mut seen = std::collections::HashSet::new();
    for id in quill_core::notes::annotation_ids(&doc.main.paras) {
        if !seen.insert(id) {
            continue;
        }
        let Some(range) = ranges.get(&id) else {
            continue;
        };
        let info = doc.annotation_info.get(&id).cloned().unwrap_or_default();
        let mut body = String::new();
        blocks(doc, ids, &doc.annotations.paras[range.clone()], &mut body);
        let own = format!(r#"<w:commentReference w:id="{id}"/>"#);
        let mut body = body.replace(&own, "<w:annotationRef/>");
        // Replies and resolved state name a comment by its last
        // paragraph's id.
        if let Some(at) = body.rfind("<w:p>") {
            body.replace_range(
                at..at + "<w:p>".len(),
                &format!(
                    r#"<w:p w14:paraId="{}" w14:textId="77777777">"#,
                    para_id(id)
                ),
            );
        }
        // Older annotations have only the day.
        let date = match info.date.len() {
            0 => String::new(),
            10 => format!(r#" w:date="{}T00:00:00Z""#, esc(&info.date)),
            _ => format!(r#" w:date="{}""#, esc(&info.date)),
        };
        let _ = write!(
            out,
            r#"<w:comment w:id="{id}" w:author="{}" w:initials="{}"{date}>{body}</w:comment>"#,
            esc(&info.author),
            esc(&info.initials)
        );
    }
    out.push_str("</w:comments>");
    out
}

/// The `w14:paraId` of annotation `id`'s last paragraph: 8 hex digits,
/// below 0x80000000 as Word requires.
fn para_id(id: u32) -> String {
    format!("{:08X}", 0x1000_0000 | (id & 0x0FFF_FFFF))
}

/// commentsExtended.xml: which comment each one replies to, and which are
/// resolved.
fn comments_extended_xml(doc: &Document) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w15:commentsEx xmlns:w15="http://schemas.microsoft.com/office/word/2012/wordml" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="w15">"#,
    );
    let live = quill_core::notes::annotation_ids(&doc.main.paras);
    for &id in &live {
        let info = doc.annotation_info.get(&id).cloned().unwrap_or_default();
        let _ = write!(out, r#"<w15:commentEx w15:paraId="{}""#, para_id(id));
        if let Some(parent) = info.parent.filter(|p| live.contains(p)) {
            let _ = write!(out, r#" w15:paraIdParent="{}""#, para_id(parent));
        }
        let _ = write!(out, r#" w15:done="{}"/>"#, u8::from(info.done));
    }
    out.push_str("</w15:commentsEx>");
    out
}

/// footnotes.xml (or endnotes.xml): Word's two separators, then each
/// note, its own mark written as `footnoteRef`.
fn notes_xml(doc: &Document, ids: &[String], end: bool) -> String {
    let (root, el, r) = if end {
        ("endnotes", "endnote", "endnoteRef")
    } else {
        ("footnotes", "footnote", "footnoteRef")
    };
    let mut out = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:{root} {W_NS}><w:{el} w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:{el}><w:{el} w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:{el}>"#
    );
    let ranges = doc.note_ranges();
    let mut seen = std::collections::HashSet::new();
    for id in quill_core::notes::note_ids(&doc.main.paras) {
        if !seen.insert(id) {
            continue;
        }
        let Some(range) = ranges.get(&id) else {
            continue;
        };
        let mut body = String::new();
        blocks(doc, ids, &doc.notes.paras[range.clone()], &mut body);
        let own = format!(r#"<w:footnoteReference w:id="{id}"/>"#);
        let custom = format!(r#"<w:footnoteReference w:customMarkFollows="1" w:id="{id}"/>"#);
        let body = body
            .replace(&own, &format!("<w:{r}/>"))
            .replace(&custom, "");
        let _ = write!(out, r#"<w:{el} w:id="{id}">{body}</w:{el}>"#);
    }
    let _ = write!(out, "</w:{root}>");
    out
}

fn header_xml(doc: &Document, ids: &[String], section: usize, id: StoryId) -> String {
    let tag = if id.is_footer() { "ftr" } else { "hdr" };
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:{tag} {W_NS}>"#
    );
    blocks(doc, ids, &doc.header_story(section, id).paras, &mut out);
    let _ = write!(out, "</w:{tag}>");
    out
}

/// Where each comment's text starts and ends in the main text, as
/// (paragraph, byte offset) -> comment ids, while document.xml is being
/// written.
#[derive(Default)]
pub(crate) struct CommentEdges {
    pub(crate) starts: std::collections::HashMap<(usize, usize), Vec<u32>>,
    pub(crate) ends: std::collections::HashMap<(usize, usize), Vec<u32>>,
}

thread_local! {
    static COMMENT_EDGES: std::cell::RefCell<CommentEdges> = Default::default();
}

pub(crate) fn comment_edges(doc: &Document) -> CommentEdges {
    let live = quill_core::notes::annotation_ids(&doc.main.paras);
    // Each comment's first and last (paragraph, offset).
    type Span = ((usize, usize), (usize, usize));
    let mut span: std::collections::HashMap<u32, Span> = Default::default();
    for (i, p) in doc.main.paras.iter().enumerate() {
        for (s, e, c) in p.run_bounds() {
            if c.comment != 0 && live.contains(&c.comment) {
                span.entry(c.comment).or_insert(((i, s), (i, e))).1 = (i, e);
            }
        }
    }
    let mut edges = CommentEdges::default();
    let mut ids: Vec<_> = span.into_iter().collect();
    ids.sort();
    for (id, (start, end)) in ids {
        edges.starts.entry(start).or_default().push(id);
        edges.ends.entry(end).or_default().push(id);
    }
    edges
}

/// Which paragraph of the main text `p` is, if it is one.
pub(crate) fn main_index(doc: &Document, p: &Paragraph) -> Option<usize> {
    let paras = &doc.main.paras;
    let base = paras.as_ptr() as usize;
    let at = (p as *const Paragraph as usize).checked_sub(base)?;
    let i = at / std::mem::size_of::<Paragraph>();
    (i < paras.len() && std::ptr::eq(&paras[i], p)).then_some(i)
}

/// The comment range marks at `(para, offset)` of the main text.
fn comment_marks(out: &mut String, at: Option<(usize, usize)>, start: bool) {
    let Some(at) = at else {
        return;
    };
    COMMENT_EDGES.with(|e| {
        let e = e.borrow();
        let map = if start { &e.starts } else { &e.ends };
        for id in map.get(&at).into_iter().flatten() {
            let tag = if start {
                "commentRangeStart"
            } else {
                "commentRangeEnd"
            };
            let _ = write!(out, r#"<w:{tag} w:id="{id}"/>"#);
        }
    });
}

fn document_xml(doc: &Document, ids: &[String]) -> String {
    COMMENT_EDGES.with(|e| *e.borrow_mut() = comment_edges(doc));
    let out = document_body(doc, ids);
    COMMENT_EDGES.with(|e| *e.borrow_mut() = CommentEdges::default());
    out
}

fn document_body(doc: &Document, ids: &[String]) -> String {
    let mut out = String::with_capacity(doc.main.paras.len() * 200);
    let _ = write!(
        out,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {W_NS}><w:body>"#
    );
    blocks(doc, ids, &doc.main.paras, &mut out);
    section_props(doc, doc.sections().len() - 1, &doc.section, &mut out);
    out.push_str("</w:body></w:document>");
    out
}

/// A table: its grid (every cell edge in any row), then the rows with each
/// cell's width, span, borders and shading. `paras` are those of the table
/// and what is in its cells, and the table is in a cell `outer` tables
/// deep (0 for one in the body).
fn table(doc: &Document, ids: &[String], paras: &[Paragraph], outer: u8, out: &mut String) {
    let text_width = doc.props.text_width();
    let mut rows = Vec::new();
    let mut i = 0;
    while let Some(r) = quill_core::table::row_at_level(paras, i, outer + 1) {
        i = r.end + 1;
        let props = quill_core::table::row_props(paras, &r, text_width);
        rows.push((r, props));
    }
    if rows.is_empty() {
        for p in paras {
            paragraph(doc, ids, p, out);
        }
        return;
    }
    let mut edges: Vec<i32> = Vec::new();
    for (_, props) in &rows {
        let lefts = props.lefts(text_width);
        edges.extend(&lefts);
        edges.push(lefts[0] + props.width());
    }
    edges.sort_unstable();
    edges.dedup();
    let first = &rows[0].1;
    let jc = match first.align {
        quill_core::Align::Center => "center",
        quill_core::Align::Right => "right",
        _ => "left",
    };
    let _ = write!(
        out,
        r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/><w:jc w:val="{jc}"/><w:tblInd w:w="{}" w:type="dxa"/><w:tblLayout w:type="fixed"/><w:tblCellMar><w:left w:w="{}" w:type="dxa"/><w:right w:w="{}" w:type="dxa"/></w:tblCellMar><w:tblLook w:val="0000"/></w:tblPr><w:tblGrid>"#,
        edges[0],
        first.gap / 2,
        first.gap - first.gap / 2
    );
    for w in edges.windows(2) {
        let _ = write!(out, r#"<w:gridCol w:w="{}"/>"#, w[1] - w[0]);
    }
    out.push_str("</w:tblGrid>");
    let all: Vec<_> = rows.iter().map(|(_, p)| p.clone()).collect();
    let merged = quill_core::table::merges(&all, text_width);
    for (ri, (row, props)) in rows.iter().enumerate() {
        let lefts = props.lefts(text_width);
        out.push_str("<w:tr>");
        // A centered or right-aligned row keeps its indent for when it is
        // aligned left again; Word ignores it. A row whose cells are
        // further apart or closer than the first row's says so.
        let mut own = String::new();
        if props.align != quill_core::Align::Left {
            let _ = write!(
                own,
                r#"<w:tblInd w:w="{}" w:type="dxa"/>"#,
                props.indent - props.gap / 2
            );
        }
        if props.gap != first.gap {
            let _ = write!(
                own,
                r#"<w:tblCellMar><w:left w:w="{}" w:type="dxa"/><w:right w:w="{}" w:type="dxa"/></w:tblCellMar>"#,
                props.gap / 2,
                props.gap - props.gap / 2
            );
        }
        if !own.is_empty() {
            let _ = write!(out, "<w:tblPrEx>{own}</w:tblPrEx>");
        }
        out.push_str("<w:trPr><w:cantSplit/>");
        // A row that starts further in than the table skips grid columns.
        let before = edges.iter().filter(|&&e| e < lefts[0]).count();
        if before > 0 {
            let _ = write!(
                out,
                r#"<w:gridBefore w:val="{before}"/><w:wBefore w:w="{}" w:type="dxa"/>"#,
                lefts[0] - edges[0]
            );
        }
        let jc = match props.align {
            quill_core::Align::Center => "center",
            quill_core::Align::Right => "right",
            _ => "left",
        };
        let _ = write!(out, r#"<w:jc w:val="{jc}"/>"#);
        match props.height {
            quill_core::RowHeight::Auto => {}
            quill_core::RowHeight::AtLeast(h) => {
                let _ = write!(out, r#"<w:trHeight w:val="{h}" w:hRule="atLeast"/>"#);
            }
            quill_core::RowHeight::Exactly(h) => {
                let _ = write!(out, r#"<w:trHeight w:val="{h}" w:hRule="exact"/>"#);
            }
        }
        out.push_str("</w:trPr>");
        for (k, &(a, b)) in row.cells.iter().enumerate() {
            let cell = &props.cells[k];
            let l = lefts[k];
            let r = l + cell.width;
            let span = edges.iter().filter(|&&e| e > l && e <= r).count().max(1);
            let _ = write!(
                out,
                r#"<w:tc><w:tcPr><w:tcW w:w="{}" w:type="dxa"/>"#,
                cell.width
            );
            if span > 1 {
                let _ = write!(out, r#"<w:gridSpan w:val="{span}"/>"#);
            }
            match merged[ri][k] {
                quill_core::table::Merged::No => {}
                quill_core::table::Merged::Top(_) => out.push_str(r#"<w:vMerge w:val="restart"/>"#),
                quill_core::table::Merged::Under => out.push_str("<w:vMerge/>"),
            }
            if !cell.borders.is_none() {
                out.push_str("<w:tcBorders>");
                border(out, "top", &cell.borders.top, false);
                border(out, "left", &cell.borders.left, false);
                border(out, "bottom", &cell.borders.bottom, cell.borders.shadow);
                border(out, "right", &cell.borders.right, cell.borders.shadow);
                out.push_str("</w:tcBorders>");
            }
            if let Some(color) = cell.shading_color {
                let _ = write!(
                    out,
                    r#"<w:shd w:val="clear" w:color="auto" w:fill="{:06X}"/>"#,
                    color.0
                );
            } else if cell.shading > 0 {
                let _ = write!(
                    out,
                    r#"<w:shd w:val="{}" w:color="auto" w:fill="auto"/>"#,
                    shading_val(cell.shading)
                );
            }
            match cell.valign {
                quill_core::props::VAlign::Top => {}
                quill_core::props::VAlign::Center => out.push_str(r#"<w:vAlign w:val="center"/>"#),
                quill_core::props::VAlign::Bottom => out.push_str(r#"<w:vAlign w:val="bottom"/>"#),
            }
            out.push_str("</w:tcPr>");
            blocks_at(doc, ids, &paras[a..=b], outer + 1, out);
            out.push_str("</w:tc>");
        }
        out.push_str("</w:tr>");
    }
    out.push_str("</w:tbl>");
}

/// Paragraphs and tables.
fn blocks(doc: &Document, ids: &[String], paras: &[Paragraph], out: &mut String) {
    blocks_at(doc, ids, paras, 0, out);
}

/// The paragraphs of a story or cell that is `level` tables deep. Those
/// deeper make tables in it.
fn blocks_at(doc: &Document, ids: &[String], paras: &[Paragraph], level: u8, out: &mut String) {
    let mut i = 0;
    while i < paras.len() {
        if paras[i].pap.level() <= level {
            paragraph(doc, ids, &paras[i], out);
            i += 1;
            continue;
        }
        // A table: rows end at row marks, cells at cell marks.
        let start = i;
        let mut end = i;
        while end < paras.len() && paras[end].pap.level() > level {
            end += 1;
        }
        table(doc, ids, &paras[start..end], level, out);
        i = end;
    }
}

/// Styles the document needs: Normal, every style a paragraph uses, every
/// style those are based on or followed by, and all user styles.
fn styles_to_write(doc: &Document) -> Vec<StyleId> {
    // The whole sheet: a style used only in the notes, or one redefined
    // but not used yet, is part of the document too.
    (0..doc.styles.styles.len() as StyleId).collect()
}

fn styles_xml(doc: &Document, ids: &[String]) -> String {
    let normal = doc.styles.get(0);
    let defaults = CharProps {
        font: normal.chp.font,
        size: normal.chp.size,
        ..Default::default()
    };
    let font = esc(doc.font_name(defaults.font));
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles {W_NS}><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="{font}" w:hAnsi="{font}" w:cs="{font}"/><w:sz w:val="{0}"/><w:szCs w:val="{0}"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr>{1}</w:pPr></w:pPrDefault></w:docDefaults>"#,
        defaults.size,
        if doc.props.widow_control {
            ""
        } else {
            r#"<w:widowControl w:val="0"/>"#
        }
    );
    for id in styles_to_write(doc) {
        let s = doc.styles.get(id);
        let (base_pap, base_chp) = match s.based_on {
            Some(b) => (doc.styles.get(b).pap.clone(), doc.styles.get(b).chp),
            None => (ParaProps::default(), defaults),
        };
        let _ = write!(
            out,
            r#"<w:style w:type="paragraph"{} w:styleId="{}"><w:name w:val="{}"/>"#,
            if id == 0 { r#" w:default="1""# } else { "" },
            esc(&ids[id as usize]),
            esc(&s.name)
        );
        if let Some(b) = s.based_on {
            let _ = write!(out, r#"<w:basedOn w:val="{}"/>"#, esc(&ids[b as usize]));
        }
        if s.next != id {
            let _ = write!(out, r#"<w:next w:val="{}"/>"#, esc(&ids[s.next as usize]));
        }
        if let Some(level) = doc.styles.heading_level(id) {
            let _ = write!(out, r#"<w:uiPriority w:val="9"/><w:qFormat/><w:pPr>"#);
            let mut p = ppr(&s.pap, &base_pap);
            let _ = write!(p, r#"<w:outlineLvl w:val="{}"/>"#, level - 1);
            let _ = write!(out, "{p}</w:pPr>");
        } else {
            let p = ppr(&s.pap, &base_pap);
            if !p.is_empty() {
                let _ = write!(out, "<w:pPr>{p}</w:pPr>");
            }
        }
        let r = rpr(doc, &s.chp, &base_chp);
        if !r.is_empty() {
            let _ = write!(out, "<w:rPr>{r}</w:rPr>");
        }
        out.push_str("</w:style>");
    }
    for (i, style) in doc.styles.characters.iter().enumerate() {
        let props = rpr_values(
            doc,
            &style.format.props,
            &CharProps::default(),
            style.format.mask,
        );
        let _ = write!(
            out,
            r#"<w:style w:type="character" w:styleId="Quill_Character_{}"><w:name w:val="{}"/><w:rPr>{props}</w:rPr></w:style>"#,
            i + 1,
            esc(&style.name)
        );
    }
    out.push_str("</w:styles>");
    out
}

fn settings_xml(doc: &Document) -> String {
    let p = &doc.props;
    let mut extra = String::new();
    if let Some(view) = p.view {
        let val = match view {
            SavedView::Normal => "normal",
            SavedView::Outline => "outline",
            SavedView::PageLayout => "print",
        };
        let _ = write!(extra, r#"<w:view w:val="{val}"/>"#);
    }
    if p.mirror_margins {
        extra.push_str("<w:mirrorMargins/>");
    }
    if p.track_changes {
        extra.push_str("<w:trackRevisions/>");
    }
    let _ = write!(extra, r#"<w:defaultTabStop w:val="{}"/>"#, p.default_tab);
    if p.even_odd_headers {
        extra.push_str("<w:evenAndOddHeaders/>");
    }
    // Quill's own setting, which Word may skip (see W_NS).
    if p.print_hidden {
        extra.push_str("<quill:printHiddenText/>");
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:settings {W_NS}>{extra}<w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat></w:settings>"#
    )
}

fn font_table_xml(doc: &Document) -> String {
    let mut out =
        format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:fonts {W_NS}>"#);
    for f in &doc.fonts {
        let _ = write!(out, r#"<w:font w:name="{}"/>"#, esc(f));
    }
    out.push_str("</w:fonts>");
    out
}

fn core_xml(doc: &Document, now: u64) -> String {
    let s = &doc.summary;
    let (y, mo, d) = zip::civil_from_days((now / 86_400) as i64);
    let t = now % 86_400;
    let stamp = format!(
        "{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}Z",
        t / 3600,
        t % 3600 / 60,
        t % 60
    );
    let mut o = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">"#,
    );
    let mut field = |tag: &str, v: &str| {
        if !v.is_empty() {
            let _ = write!(o, "<{tag}>{}</{tag}>", esc(v));
        }
    };
    field("dc:title", &s.title);
    field("dc:subject", &s.subject);
    field("dc:creator", &s.author);
    field("cp:keywords", &s.keywords);
    field("cp:version", &s.version);
    let _ = write!(
        o,
        r#"<dcterms:modified xsi:type="dcterms:W3CDTF">{stamp}</dcterms:modified>"#
    );
    o.push_str("</cp:coreProperties>");
    o
}
