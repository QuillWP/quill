//! What saving a document as Markdown would leave out, worded for the
//! warning Quill shows first. A document read from Markdown reports
//! nothing: it only has what Markdown can carry.

use super::read::{CODE_SHADING, LIST_INDENT, QUOTE_GAP, QUOTE_INDENT};
use super::{is_mono, list_label_of};
use quill_core::Document;
use quill_core::para::{Mark, chars};
use quill_core::props::{Align, BorderStyle, RowHeight, VAlign};

pub const FONTS: &str = "Fonts and font sizes";
pub const COLORS: &str = "Text colors";
pub const EFFECTS: &str =
    "Small caps, all caps, outline, shadow, hidden text and character spacing";
pub const ALIGNMENT: &str = "Centered, right-aligned and justified paragraphs";
pub const INDENTS: &str = "Indents, other than those of lists and quotes";
pub const TABS: &str = "Custom tab stops";
pub const BORDERS: &str = "Paragraph borders and shading";
pub const FRAMES: &str = "Frames, text boxes and drop caps";
pub const STYLES: &str = "Custom paragraph styles (headings 1 to 6 are kept)";
pub const BREAKS: &str = "Page and section breaks, and other pagination settings";
pub const PAGE: &str = "Page size, margins, columns, page numbering and line numbers";
pub const HEADERS: &str = "Headers and footers";
pub const FOOTNOTES: &str = "Footnotes and endnotes";
pub const COMMENTS: &str = "Comments";
pub const CHANGES: &str = "Tracked changes (the file gets the text with every change accepted)";
pub const FIELDS: &str = "Page number, date and time fields";
pub const PICTURES: &str = "Picture sizes and cropping";
pub const TABLE_LAYOUT: &str =
    "Table column widths, cell shading, vertical alignment and row heights";
pub const NESTED: &str = "Tables inside tables";
pub const SUMMARY: &str = "Summary information (title, author, subject and keywords)";

fn note(found: &mut Vec<&'static str>, what: &'static str) {
    if !found.contains(&what) {
        found.push(what);
    }
}

fn blank(story: &quill_core::Story) -> bool {
    story.paras.iter().all(|p| p.text.trim().is_empty())
}

/// Everything in `doc` that Markdown cannot carry, once each, in the order
/// the warning lists them.
pub fn losses(doc: &Document) -> Vec<&'static str> {
    let mut found = Vec::new();
    if doc.has_changes() {
        note(&mut found, CHANGES);
    }
    let paras = &doc.main.paras;
    if !quill_core::notes::note_ids(paras).is_empty() {
        note(&mut found, FOOTNOTES);
    }
    let own_headers = paras.iter().any(|p| {
        p.section
            .as_ref()
            .and_then(|s| s.headers.as_ref())
            .is_some_and(|h| h.iter().any(|s| !blank(s)))
    });
    if own_headers || doc.headers.iter().any(|h| !blank(h)) {
        note(&mut found, HEADERS);
    }
    let page = doc.props.clone();
    let normal = quill_core::DocumentProps::default();
    let section_differs = |s: &quill_core::SectionProps| {
        s.columns > 1
            || s.line_numbers != quill_core::LineNumbering::Off
            || s.page_number_format != quill_core::PageNumberFormat::Arabic
            || s.restart_page_numbers
            || s.margin_page_numbers
            || s.different_first_page
    };
    if (page.page_width, page.page_height) != (normal.page_width, normal.page_height)
        || (
            page.margin_left,
            page.margin_right,
            page.margin_top,
            page.margin_bottom,
            page.gutter,
        ) != (
            normal.margin_left,
            normal.margin_right,
            normal.margin_top,
            normal.margin_bottom,
            normal.gutter,
        )
        || section_differs(&doc.section)
        || paras
            .iter()
            .any(|p| p.section.as_ref().is_some_and(|s| section_differs(s)))
    {
        note(&mut found, PAGE);
    }
    let s = &doc.summary;
    if [&s.title, &s.subject, &s.author, &s.keywords]
        .iter()
        .any(|v| !v.trim().is_empty())
    {
        note(&mut found, SUMMARY);
    }

    // Whether the paragraph before was a list item (or set in under one),
    // whose indent the next may share.
    let mut in_list = false;
    for p in paras {
        if p.mark == Mark::RowEnd {
            if let Some(row) = &p.row {
                let widths = row.cells.iter().map(|c| c.width);
                let spread = widths.clone().max().unwrap_or(0) - widths.min().unwrap_or(0);
                if spread > 20
                    || row.height != RowHeight::Auto
                    || row.indent != 0
                    || row.align != Align::Left
                    || row
                        .cells
                        .iter()
                        .any(|c| c.shading > 0 || c.valign != VAlign::Top)
                {
                    note(&mut found, TABLE_LAYOUT);
                }
            }
            continue;
        }
        if p.pap.level() > 1 {
            note(&mut found, NESTED);
        }
        if p.mark == Mark::SectionBreak
            || p.pap.page_break_before
            || p.text.contains(chars::PAGE_BREAK)
        {
            note(&mut found, BREAKS);
        }
        let style = doc.styles.get(p.pap.style);
        let heading = doc
            .styles
            .heading_level(p.pap.style)
            .is_some_and(|l| l <= 6);
        if p.pap.style != 0 && !heading {
            note(&mut found, STYLES);
        }
        if p.pap.keep_with_next != style.pap.keep_with_next
            || p.pap.keep_together != style.pap.keep_together
        {
            note(&mut found, BREAKS);
        }
        if p.pap.frame.is_some() {
            note(&mut found, FRAMES);
        }
        if p.pap.align != Align::Left && p.pap.align != style.pap.align && !p.pap.in_table {
            note(&mut found, ALIGNMENT);
        }
        if p.pap.tabs != style.pap.tabs {
            note(&mut found, TABS);
        }

        let q = if p.pap.borders.left.style == BorderStyle::None {
            0
        } else {
            (p.pap.borders.left.space / QUOTE_GAP).clamp(1, 8)
        };
        let rule = p.text.trim().is_empty() && p.pap.borders.bottom.style != BorderStyle::None;
        let code = p.pap.shading >= CODE_SHADING && is_mono(doc.font_name(p.chp_at(0).font));
        let b = &p.pap.borders;
        let edges = [b.top, b.right, b.bottom, b.between];
        if b.shadow
            || edges
                .iter()
                .enumerate()
                .any(|(k, e)| e.style != BorderStyle::None && !(rule && k == 2))
            || (p.pap.shading > 0 && !code)
        {
            note(&mut found, BORDERS);
        }

        let item = list_label_of(&p.text).is_some();
        let indent = p.pap.left - q * QUOTE_INDENT;
        let steps = indent >= 0 && indent % LIST_INDENT == 0;
        let fits = if item {
            let start = p.pap.left + p.pap.first - q * QUOTE_INDENT;
            start >= 0 && start % LIST_INDENT == 0
        } else if in_list {
            steps && p.pap.first == 0
        } else {
            indent == style.pap.left && p.pap.first == style.pap.first
        };
        if !fits || p.pap.right != style.pap.right {
            note(&mut found, INDENTS);
        }
        in_list = item || (in_list && !p.text.trim().is_empty() && steps && indent > 0);

        let base = style.chp;
        let mut start = 0;
        for run in &p.runs {
            let c = run.chp;
            let text = &p.text[start..start + run.len];
            start += run.len;
            if c.comment != 0 {
                note(&mut found, COMMENTS);
            }
            if text.contains(chars::ANNOTATION) {
                note(&mut found, COMMENTS);
            }
            if text.contains([chars::FIELD_PAGE, chars::FIELD_DATE, chars::FIELD_TIME]) {
                note(&mut found, FIELDS);
            }
            if text.contains(chars::OBJECT)
                && let Some(pic) = doc.picture(c.object)
                && (pic.size != pic.natural || pic.crop != Default::default())
            {
                note(&mut found, PICTURES);
            }
            if text.trim().is_empty() {
                continue;
            }
            let mono = is_mono(doc.font_name(c.font));
            if (c.font != base.font && !mono) || (c.size != base.size && c.object == 0) {
                note(&mut found, FONTS);
            }
            if c.color != base.color && c.link == 0 {
                note(&mut found, COLORS);
            }
            if c.outline != base.outline
                || c.shadow != base.shadow
                || c.small_caps != base.small_caps
                || c.all_caps != base.all_caps
                || c.hidden != base.hidden
                || c.spacing != base.spacing
            {
                note(&mut found, EFFECTS);
            }
        }
    }
    found
}
