//! Laying out one paragraph into lines.
//!
//! Like the original this works on Latin text with per-glyph advances and
//! kerning. All positions are in points, measured from the left edge of
//! the text column (the left margin) and the top of the paragraph.

use crate::fonts::{FaceKey, Fonts};
use ab_glyph::GlyphId;
use quill_core::para::{Paragraph, chars};
use quill_core::props::{
    Align, BorderLine, BorderStyle, CharProps, LineSpacing, ParaProps, TabAlign, TabLeader,
    TabStop, Underline,
};

/// Settings that affect layout but are not stored in the paragraph.
#[derive(Clone, Debug)]
pub struct LayoutOptions {
    /// Width of the text column in points (page width minus margins).
    pub width: f32,
    /// Distance between default tab stops, in points.
    pub default_tab: f32,
    /// Lay out hidden text instead of collapsing it.
    pub show_hidden: bool,
    /// Draw spaces, tabs and marks (Show ¶). Does not move anything.
    pub show_marks: bool,
    /// Show tracked changes marked up (View > Show Changes). Off shows the
    /// text as it would be with every change accepted: deleted text
    /// collapses like hidden text.
    pub show_changes: bool,
    /// What the page number, date and time fields show.
    pub page_number: u32,
    pub page_format: quill_core::PageNumberFormat,
    /// What each footnote reference shows, by note id.
    pub note_labels: std::sync::Arc<std::collections::HashMap<u32, String>>,
    /// The initials in each annotation mark, by id.
    pub annotation_labels: std::sync::Arc<std::collections::HashMap<u32, String>>,
    pub date: String,
    pub time: String,
    /// Width and height in points of each object (picture), by id less
    /// one.
    pub objects: std::sync::Arc<Vec<(f32, f32)>>,
    /// Areas lines must keep clear of, for text flowing round frames: top
    /// and bottom from the top of the paragraph, left and right from the
    /// left margin, all in points.
    pub wrap: Vec<Clear>,
}

/// An annotation mark's width and the size of the initials in it, in
/// points, as the original draws them.
pub const ANNOTATION_WIDTH: f32 = 23.0;
pub const ANNOTATION_TEXT: f32 = 9.0;

/// A rectangle text keeps out of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clear {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        LayoutOptions {
            width: 432.0,
            default_tab: 36.0,
            show_hidden: false,
            show_marks: false,
            show_changes: true,
            page_number: 1,
            page_format: quill_core::PageNumberFormat::Arabic,
            note_labels: Default::default(),
            annotation_labels: Default::default(),
            date: String::new(),
            time: String::new(),
            objects: Default::default(),
            wrap: Vec::new(),
        }
    }
}

/// What a laid-out character is, for drawing and line breaking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Space,
    /// A tab, with the leader that fills it.
    Tab(TabLeader),
    LineBreak,
    PageBreak,
    /// Optional hyphen: invisible unless the line breaks after it.
    SoftHyphen,
    /// Collapsed hidden text.
    Hidden,
    Object,
    /// A page number, date or time field: its value's glyphs are
    /// `ParaLayout::fields[n]`.
    Field(u16),
}

/// One character placed on a line.
#[derive(Clone, Copy, Debug)]
pub struct Glyph {
    /// Byte offset of the character in the paragraph text.
    pub offset: u32,
    pub x: f32,
    pub advance: f32,
    pub id: GlyphId,
    pub face: FaceKey,
    pub kind: Kind,
    /// Index into `ParaLayout::styles`.
    pub style: u16,
}

/// Drawing attributes shared by a run of glyphs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphStyle {
    pub size: f32,
    /// Baseline shift in points, positive is up.
    pub raise: f32,
    pub chp: CharProps,
}

#[derive(Clone, Debug, Default)]
pub struct Line {
    /// Glyphs `first..last` belong to this line.
    pub first: usize,
    pub last: usize,
    /// Byte range of the line's text.
    pub start: usize,
    pub end: usize,
    /// Top of the line from the top of the paragraph.
    pub top: f32,
    pub height: f32,
    /// Baseline, from the top of the line.
    pub baseline: f32,
    /// Where the caret goes at the end of the line.
    pub end_x: f32,
    /// Left edge of the line's text, after indent and alignment.
    pub left: f32,
    /// The line ends with a page break, so pagination can tell without
    /// the glyphs.
    pub hard_break: bool,
    /// The line holds a tracked change, shown, so it gets a bar in the
    /// margin.
    pub changed: bool,
}

/// A horizontal rule drawn with the text: underline, strike-through or
/// tab leader. Coordinates are relative to the line's baseline.
#[derive(Clone, Copy, Debug)]
pub struct Rule {
    pub line: u32,
    pub x0: f32,
    pub x1: f32,
    /// Distance below the baseline (negative is above).
    pub dy: f32,
    pub kind: RuleKind,
    pub color: u32,
    /// Font and size for leaders drawn with characters.
    pub face: FaceKey,
    pub size: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleKind {
    Solid,
    Double,
    Dotted,
    /// A tab leader of repeated characters (periods or hyphens), as the original
    /// draws them.
    Leader(char),
}

/// A paragraph's share of a border box. Consecutive paragraphs with the
/// same borders and indents form one box: the first draws its top line,
/// the last its bottom line, and the ones after the first draw the
/// "between" line above themselves.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BorderBox {
    /// The inside of the box (the shaded part), from the paragraph's top
    /// and the left margin.
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    /// Lines drawn just outside each edge of the inside.
    pub top_line: BorderStyle,
    pub bottom_line: BorderStyle,
    pub left_line: BorderStyle,
    pub right_line: BorderStyle,
    /// Shading in tenths of a percent.
    pub shading: u16,
    /// Shadow preset: the right and bottom lines are drawn heavier.
    pub shadow: bool,
}

/// How thick a border line is, in points.
pub fn border_width(style: BorderStyle) -> f32 {
    match style {
        BorderStyle::None => 0.0,
        BorderStyle::Hairline => 0.5,
        BorderStyle::Dotted | BorderStyle::Single => 1.0,
        BorderStyle::Thick => 2.0,
        BorderStyle::Double => 3.0,
    }
}

/// Whether two neighboring paragraphs share a border box.
pub fn joins(a: &ParaProps, b: &ParaProps) -> bool {
    a.borders == b.borders && a.left == b.left && a.right == b.right && a.shading == b.shading
}

/// Whether the paragraph above and below share this paragraph's box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Joins {
    pub above: bool,
    pub below: bool,
}

impl Joins {
    /// Which neighbors share paragraph `i`'s box. Only a paragraph with
    /// borders or shading has one.
    pub fn around(paras: &[Paragraph], i: usize) -> Joins {
        let pap = &paras[i].pap;
        let boxed = !pap.borders.is_none() || pap.shading > 0;
        Joins {
            above: boxed && i > 0 && joins(&paras[i - 1].pap, pap),
            below: boxed && i + 1 < paras.len() && joins(pap, &paras[i + 1].pap),
        }
    }
}

/// One glyph of a field's value, drawn from the field's x.
#[derive(Clone, Copy, Debug)]
pub struct FieldGlyph {
    pub face: FaceKey,
    pub id: GlyphId,
    pub x: f32,
}

#[derive(Clone, Debug, Default)]
pub struct ParaLayout {
    /// The glyphs of each field's value.
    pub fields: Vec<Vec<FieldGlyph>>,
    /// The border box, if the paragraph has borders or shading.
    pub border: Option<BorderBox>,
    /// Every character, placed. Empty while `stripped`.
    pub glyphs: Vec<Glyph>,
    pub styles: Vec<GlyphStyle>,
    pub lines: Vec<Line>,
    pub rules: Vec<Rule>,
    /// Space before the first line.
    pub before: f32,
    /// Total height including space before and after.
    pub height: f32,
    /// Bar tab positions, drawn as vertical lines through each line.
    pub bars: Vec<f32>,
    /// The glyphs were dropped to save memory; the lines, heights and
    /// everything pagination needs are still here. Lay the paragraph out
    /// again before drawing it.
    pub stripped: bool,
}

/// Measured character before line breaking.
struct Item {
    offset: u32,
    id: GlyphId,
    face: FaceKey,
    advance: f32,
    kind: Kind,
    style: u16,
    ascent: f32,
    descent: f32,
    gap: f32,
    ch: char,
    /// An annotation caption occupies its own display line in the pane.
    break_after: bool,
}

/// Lay out `para` using `font_name` to map font ids to names.
pub fn layout_paragraph(
    para: &Paragraph,
    font_name: &dyn Fn(u16) -> String,
    fonts: &mut Fonts,
    opts: &LayoutOptions,
) -> ParaLayout {
    layout_paragraph_joined(para, font_name, fonts, opts, Joins::default())
}

/// Lay out `para`, knowing which neighbors share its border box.
pub fn layout_paragraph_joined(
    para: &Paragraph,
    font_name: &dyn Fn(u16) -> String,
    fonts: &mut Fonts,
    opts: &LayoutOptions,
    joins: Joins,
) -> ParaLayout {
    let pap = &para.pap;
    let pt = |t: i32| t as f32 / 20.0;
    // Room for the border lines and their distance from the text.
    let b = &pap.borders;
    let boxed = !b.is_none() || pap.shading > 0;
    let edge = |l: &BorderLine| border_width(l.style) + pt(l.space);
    let (above, below) = if !boxed {
        (0.0, 0.0)
    } else {
        let above = if joins.above {
            edge(&b.between)
        } else {
            edge(&b.top)
        };
        let below = if joins.below { 0.0 } else { edge(&b.bottom) };
        (above, below)
    };
    let mut out = ParaLayout {
        before: pt(pap.before) + above,
        ..Default::default()
    };
    let items = measure(
        para,
        font_name,
        fonts,
        opts,
        &mut out.styles,
        &mut out.fields,
    );

    // Metrics of the mark, so empty lines get a height.
    let mark_style = style_for(&mut out.styles, para.mark_chp);
    let mark_face = fonts.resolve(
        &font_name(para.mark_chp.font),
        para.mark_chp.bold,
        para.mark_chp.italic,
    );
    let mark_size = out.styles[mark_style as usize].size;
    let mark_vm = fonts.vmetrics(mark_face, mark_size);

    let left = pt(pap.left);
    let right_edge = opts.width - pt(pap.right);
    let tabs: Vec<TabStop> = pap.tabs.clone();

    let mut glyphs: Vec<Glyph> = Vec::with_capacity(items.len());
    let mut i = 0;
    let mut y = out.before;
    let mut first_line = true;
    let mut last_height = mark_vm.ascent + mark_vm.descent + mark_vm.line_gap;
    loop {
        let mut indent = if first_line {
            left + pt(pap.first)
        } else {
            left
        };
        // Text flowing round a frame: the line takes the wider side of
        // any frame beside it, and drops below the frame when that side is
        // under an inch.
        let mut right_edge = right_edge;
        loop {
            let (mut ind, mut edge, mut beside) = (indent, right_edge, None::<f32>);
            for c in &opts.wrap {
                if c.top < y + last_height && c.bottom > y && c.right > ind && c.left < edge {
                    if right_edge - c.right >= c.left - indent {
                        ind = ind.max(c.right);
                    } else {
                        edge = edge.min(c.left);
                    }
                    beside = Some(beside.map_or(c.bottom, |b: f32| b.min(c.bottom)));
                }
            }
            match beside {
                Some(bottom) if edge - ind < 72.0 && bottom > y => y = bottom,
                _ => {
                    indent = ind;
                    right_edge = edge;
                    break;
                }
            }
        }
        let line_start = i;
        let (end, hyphenated) = break_line(
            &items,
            i,
            indent,
            (right_edge, opts.width),
            &tabs,
            opts.default_tab,
            left,
        );
        // Place the line's glyphs.
        let mut x = indent;
        let mut pending: Option<(usize, f32, TabAlign)> = None;
        let mut seg_start_x = x;
        let first_glyph = glyphs.len();
        // Decimal tabs line up on the first period after them.
        let is_point = |gk: usize| items[line_start + gk - first_glyph].ch == '.';
        for (k, item) in items[line_start..end].iter().enumerate() {
            let mut adv = item.advance;
            if item.kind == Kind::SoftHyphen {
                adv = if hyphenated && line_start + k + 1 == end {
                    item.advance
                } else {
                    0.0
                };
            }
            let mut kind = item.kind;
            if let Kind::Tab(_) = item.kind {
                if let Some(p) = pending.take() {
                    finish_tab(&mut glyphs, p, x, seg_start_x, &is_point);
                    x = glyphs.last().map(|g| g.x + g.advance).unwrap_or(x);
                }
                let (stop, align, leader) = next_tab(x, &tabs, opts.default_tab, left);
                kind = Kind::Tab(leader);
                let stop = stop.min(right_edge.max(opts.width.min(stop)).max(x));
                adv = (stop - x).max(0.0);
                if align != TabAlign::Left && align != TabAlign::Bar {
                    pending = Some((glyphs.len(), stop, align));
                    adv = 0.0;
                }
                seg_start_x = x + adv;
            }
            glyphs.push(Glyph {
                offset: item.offset,
                x,
                advance: adv,
                id: item.id,
                face: item.face,
                kind,
                style: item.style,
            });
            x += adv;
        }
        if let Some(p) = pending.take() {
            finish_tab(&mut glyphs, p, x, seg_start_x, &is_point);
            x = glyphs.last().map(|g| g.x + g.advance).unwrap_or(x);
        }
        let line_glyphs = &mut glyphs[first_glyph..];

        // Width without trailing spaces, for alignment.
        let mut content_end = x;
        for g in line_glyphs.iter().rev() {
            if matches!(g.kind, Kind::Space | Kind::LineBreak | Kind::PageBreak) {
                content_end = g.x;
            } else {
                break;
            }
        }
        let last_line = end >= items.len();
        let ends_with_break = line_glyphs
            .last()
            .is_some_and(|g| g.kind == Kind::LineBreak);
        let slack = (right_edge - content_end).max(0.0);
        let has_tab = line_glyphs.iter().any(|g| matches!(g.kind, Kind::Tab(_)));
        let shift = match pap.align {
            Align::Center if !has_tab => slack / 2.0,
            Align::Right if !has_tab => slack,
            _ => 0.0,
        };
        if pap.align == Align::Justify && !last_line && !ends_with_break && !has_tab {
            let inner: Vec<usize> = line_glyphs
                .iter()
                .enumerate()
                .filter(|(_, g)| g.kind == Kind::Space && g.x < content_end)
                .map(|(k, _)| k)
                .collect();
            if !inner.is_empty() {
                let extra = slack / inner.len() as f32;
                let mut add = 0.0;
                for (k, g) in line_glyphs.iter_mut().enumerate() {
                    g.x += add;
                    if inner.contains(&k) {
                        g.advance += extra;
                        add += extra;
                    }
                }
            }
        }
        for g in line_glyphs.iter_mut() {
            g.x += shift;
        }

        // Line height from the tallest thing on it.
        let (mut asc, mut desc, mut gap) = (0.0f32, 0.0f32, 0.0f32);
        for item in &items[line_start..end] {
            if item.kind == Kind::Hidden {
                continue;
            }
            let raise = out.styles[item.style as usize].raise;
            asc = asc.max(item.ascent + raise.max(0.0));
            desc = desc.max(item.descent - raise.min(0.0));
            gap = gap.max(item.gap);
        }
        if asc == 0.0 && desc == 0.0 {
            asc = mark_vm.ascent;
            desc = mark_vm.descent;
            gap = mark_vm.line_gap;
        }
        let natural = asc + desc + gap;
        let height = match pap.line {
            LineSpacing::Auto => natural,
            LineSpacing::AtLeast(t) => natural.max(pt(t)),
            LineSpacing::Exactly(t) => pt(t),
            LineSpacing::Multiple(m) => natural * m as f32 / 240.0,
        };
        // Extra height from spacing rules goes above the text, as on the Mac.
        let baseline = (height - desc - gap).max(asc.min(height));
        let start = items
            .get(line_start)
            .map(|it| it.offset as usize)
            .unwrap_or(para.len());
        let line_end = items
            .get(end)
            .map(|it| it.offset as usize)
            .unwrap_or(para.len());
        let end_x = glyphs[first_glyph..]
            .iter()
            .rev()
            .find(|g| !matches!(g.kind, Kind::LineBreak | Kind::PageBreak))
            .map(|g| g.x + g.advance)
            .unwrap_or(indent + shift);
        let line_left = glyphs
            .get(first_glyph)
            .map(|g| g.x)
            .unwrap_or(indent + shift);
        let hard_break = glyphs[first_glyph..]
            .last()
            .is_some_and(|g| g.kind == Kind::PageBreak);
        out.lines.push(Line {
            first: first_glyph,
            last: glyphs.len(),
            start,
            end: line_end,
            top: y,
            height,
            baseline,
            end_x,
            left: line_left,
            hard_break,
            changed: false,
        });
        y += height;
        last_height = height;
        first_line = false;
        i = end;
        if i >= items.len() {
            break;
        }
    }
    out.glyphs = glyphs;
    out.height = y + pt(pap.after) + below;
    if boxed {
        let (top_line, top) = if joins.above {
            (b.between.style, border_width(b.between.style))
        } else {
            (b.top.style, border_width(b.top.style))
        };
        let (bottom_line, bottom) = if joins.below {
            (BorderStyle::None, out.height)
        } else {
            (b.bottom.style, out.height - border_width(b.bottom.style))
        };
        out.border = Some(BorderBox {
            left: left.min(left + pt(pap.first)) - pt(b.left.space),
            right: right_edge + pt(b.right.space),
            top,
            bottom,
            top_line,
            bottom_line,
            left_line: b.left.style,
            right_line: b.right.style,
            shading: pap.shading,
            shadow: b.shadow,
        });
    }
    out.bars = tabs
        .iter()
        .filter(|t| t.align == TabAlign::Bar)
        .map(|t| pt(t.pos))
        .collect();
    if opts.show_changes {
        mark_changed_lines(&mut out, para);
    }
    add_rules(&mut out);
    out
}

/// How a run looks with its tracked change shown: in its author's color,
/// underlined when inserted, struck through when deleted, and double
/// underlined when only its format changed.
pub fn shown(chp: CharProps, show_changes: bool) -> CharProps {
    let Some(r) = quill_core::revision::get(chp.rev) else {
        return chp;
    };
    if !show_changes {
        return chp;
    }
    let by = r
        .deleted
        .as_ref()
        .or(r.inserted.as_ref())
        .or(r.formatted.as_ref().map(|f| &f.0));
    let mut c = chp;
    if let Some(by) = by {
        c.color = quill_core::revision::author_color(&by.author);
    }
    if r.inserted.is_some() {
        c.underline = Underline::Single;
    } else if r.formatted.is_some() && r.deleted.is_none() {
        c.underline = Underline::Double;
    }
    if r.deleted.is_some() {
        c.strike = true;
    }
    c
}

/// Mark the lines that hold a tracked change, the paragraph mark's
/// included (it belongs to the last line).
fn mark_changed_lines(out: &mut ParaLayout, para: &Paragraph) {
    let n = out.lines.len();
    for (li, line) in out.lines.iter_mut().enumerate() {
        line.changed = (li + 1 == n && para.mark_chp.rev != 0)
            || out.glyphs[line.first..line.last]
                .iter()
                .any(|g| out.styles[g.style as usize].chp.rev != 0);
    }
}

/// Find where the line starting at item `i` ends. Returns the index after
/// the last item on the line and whether it ends at an optional hyphen.
/// `right` is the right indent's edge and the column's; text may run past
/// the indent as far as a right, center or decimal tab set beyond it, as
/// a table of contents' page numbers do.
fn break_line(
    items: &[Item],
    i: usize,
    indent: f32,
    (right, column): (f32, f32),
    tabs: &[TabStop],
    default_tab: f32,
    left: f32,
) -> (usize, bool) {
    let mut x = indent;
    let mut last_break: Option<(usize, bool)> = None;
    // A right, center or decimal tab waiting for its text: the stop, its
    // alignment, where the text started and how wide it is so far.
    let mut pending: Option<(f32, TabAlign, f32, f32)> = None;
    // Where the line ends with a pending tab's text placed.
    let end_of = |p: (f32, TabAlign, f32, f32)| {
        let (stop, align, sx, w) = p;
        match align {
            TabAlign::Center => (sx + w).max(stop + w / 2.0),
            _ => (sx + w).max(stop),
        }
    };
    let mut k = i;
    while k < items.len() {
        let item = &items[k];
        if item.break_after {
            return (k + 1, false);
        }
        match item.kind {
            Kind::LineBreak | Kind::PageBreak => return (k + 1, false),
            Kind::Space => {
                // Spaces never push a line over; they hang in the margin.
                match &mut pending {
                    Some(p) => p.3 += item.advance,
                    None => x += item.advance,
                }
                last_break = Some((k + 1, false));
                k += 1;
                continue;
            }
            Kind::Tab(_) => {
                if let Some(p) = pending.take() {
                    x = end_of(p);
                }
                let (stop, align, _) = next_tab(x, tabs, default_tab, left);
                match align {
                    TabAlign::Right | TabAlign::Center | TabAlign::Decimal => {
                        pending = Some((stop, align, x, 0.0));
                    }
                    _ => x = stop.max(x),
                }
                last_break = Some((k + 1, false));
                k += 1;
                continue;
            }
            _ => {}
        }
        let adv = if item.kind == Kind::SoftHyphen {
            0.0
        } else {
            item.advance
        };
        let extent = match pending {
            Some((stop, align, sx, w)) => end_of((stop, align, sx, w + adv)),
            None => x + adv,
        };
        let limit = match pending {
            Some((stop, _, _, _)) if stop > right => stop.min(column),
            _ => right,
        };
        if extent > limit + 0.01 && k > i {
            if let Some(b) = last_break {
                return b;
            }
            return (k, false); // one long word: break inside it
        }
        match &mut pending {
            Some(p) => p.3 += adv,
            None => x += adv,
        }
        if item.kind == Kind::SoftHyphen && extent + item.advance <= right {
            last_break = Some((k + 1, true));
        } else if item.ch == '-' || item.ch == '\u{2014}' || item.ch == '\u{2013}' {
            last_break = Some((k + 1, false));
        }
        k += 1;
    }
    (items.len(), false)
}

/// Next tab stop after `x`: a custom stop if one is further right, else the
/// next default stop. A hanging indent's left indent also acts as a stop.
fn next_tab(x: f32, tabs: &[TabStop], default_tab: f32, left: f32) -> (f32, TabAlign, TabLeader) {
    let eps = 0.01;
    let custom = tabs.iter().find(|t| t.pos as f32 / 20.0 > x + eps);
    let mut best = custom.map(|t| (t.pos as f32 / 20.0, t.align, t.leader));
    if left > x + eps && best.is_none_or(|b| left < b.0) {
        best = Some((left, TabAlign::Left, TabLeader::None));
    }
    if let Some(b) = best {
        return b;
    }
    // Default stops continue past the last custom stop.
    let after = tabs
        .last()
        .map(|t| t.pos as f32 / 20.0)
        .unwrap_or(0.0)
        .max(x);
    let step = default_tab.max(1.0);
    let n = (after / step).floor() + 1.0;
    let mut stop = n * step;
    if stop <= x + eps {
        stop += step;
    }
    (stop, TabAlign::Left, TabLeader::None)
}

/// Settle a right, center or decimal tab once the text after it is known.
fn finish_tab(
    glyphs: &mut [Glyph],
    (tab, stop, align): (usize, f32, TabAlign),
    x: f32,
    seg_start: f32,
    is_point: &dyn Fn(usize) -> bool,
) {
    let seg: Vec<usize> = (tab + 1..glyphs.len()).collect();
    let width = x - seg_start;
    let anchor = match align {
        TabAlign::Right => width,
        TabAlign::Center => width / 2.0,
        TabAlign::Decimal => seg
            .iter()
            .find(|&&k| glyphs[k].kind == Kind::Text && is_point(k))
            .map(|&k| glyphs[k].x - seg_start)
            .unwrap_or(width),
        _ => 0.0,
    };
    let tab_x = glyphs[tab].x;
    let adv = (stop - anchor - tab_x).max(0.0);
    glyphs[tab].advance = adv;
    let shift = tab_x + adv - seg_start;
    for k in seg {
        glyphs[k].x += shift;
    }
}

fn style_for(styles: &mut Vec<GlyphStyle>, chp: CharProps) -> u16 {
    if let Some(i) = styles.iter().position(|s| s.chp == chp) {
        return i as u16;
    }
    styles.push(GlyphStyle {
        size: chp.size as f32 / 2.0,
        raise: chp.position as f32 / 20.0,
        chp,
    });
    (styles.len() - 1) as u16
}

fn measure(
    para: &Paragraph,
    font_name: &dyn Fn(u16) -> String,
    fonts: &mut Fonts,
    opts: &LayoutOptions,
    styles: &mut Vec<GlyphStyle>,
    fields: &mut Vec<Vec<FieldGlyph>>,
) -> Vec<Item> {
    let mut items = Vec::with_capacity(para.text.len());
    for (start, end, chp) in para.run_bounds() {
        let mut shown = shown(*chp, opts.show_changes);
        // Text about an annotation that is gone isn't shaded.
        if shown.comment != 0 && !opts.annotation_labels.contains_key(&shown.comment) {
            shown.comment = 0;
        }
        let chp = &shown;
        let style = style_for(styles, *chp);
        let size = chp.size as f32 / 2.0;
        let face = fonts.resolve(&font_name(chp.font), chp.bold, chp.italic);
        let vm = fonts.vmetrics(face, size);
        let spacing = chp.spacing as f32 / 20.0;
        let hidden = (chp.hidden && !opts.show_hidden)
            || (!opts.show_changes && quill_core::revision::is_deleted(chp.rev));
        let mut prev: Option<(FaceKey, GlyphId)> = None;
        for (i, c) in para.text[start..end].char_indices() {
            let offset = (start + i) as u32;
            let kind = if hidden {
                Kind::Hidden
            } else {
                match c {
                    ' ' => Kind::Space,
                    chars::TAB => Kind::Tab(TabLeader::None),
                    chars::LINE_BREAK => Kind::LineBreak,
                    chars::PAGE_BREAK => Kind::PageBreak,
                    chars::OPTIONAL_HYPHEN => Kind::SoftHyphen,
                    chars::OBJECT => Kind::Object,
                    chars::FIELD_PAGE
                    | chars::FIELD_DATE
                    | chars::FIELD_TIME
                    | chars::FOOTNOTE
                    | chars::ANNOTATION => Kind::Field(fields.len() as u16),
                    _ => Kind::Text,
                }
            };
            // What actually gets drawn: caps styles change the letter, small
            // caps also shrink it.
            let (shown, glyph_size) = if chp.all_caps || (chp.small_caps && c.is_lowercase()) {
                let up = c.to_uppercase().next().unwrap_or(c);
                let sz = if chp.small_caps && !chp.all_caps && c.is_lowercase() {
                    size * 0.8
                } else {
                    size
                };
                (up, sz)
            } else {
                (c, size)
            };
            let shown = match kind {
                Kind::SoftHyphen => '-',
                Kind::Tab(_) | Kind::LineBreak | Kind::PageBreak => ' ',
                _ if c == chars::NONBREAKING_SPACE => ' ',
                _ if c == chars::NONBREAKING_HYPHEN => '-',
                _ => shown,
            };
            let (gface, id) = fonts.glyph(face, shown);
            let mut advance = match kind {
                Kind::Hidden | Kind::LineBreak | Kind::PageBreak => 0.0,
                Kind::Tab(_) => 0.0,
                _ => fonts.advance(gface, id, glyph_size) + spacing,
            };
            if let Some((pf, pid)) = prev
                && pf == gface
                && kind == Kind::Text
                && let Some(last) = items.last_mut()
            {
                let last: &mut Item = last;
                last.advance += fonts.kern(gface, pid, id, glyph_size);
            }
            prev = (kind == Kind::Text).then_some((gface, id));
            let mut ascent = vm.ascent;
            if kind == Kind::Object {
                // A picture sits on the baseline and the line grows to hold
                // it. Unknown objects get a small box.
                let (w, h) = (chp.object as usize)
                    .checked_sub(1)
                    .and_then(|i| opts.objects.get(i).copied())
                    .unwrap_or((size, size));
                advance = w;
                ascent = ascent.max(h);
            }
            let mut break_after = false;
            if let Kind::Field(_) = kind {
                let value = match c {
                    chars::FIELD_PAGE => opts.page_format.text(opts.page_number),
                    chars::FIELD_DATE => opts.date.clone(),
                    chars::FOOTNOTE => opts
                        .note_labels
                        .get(&chp.object)
                        .cloned()
                        .unwrap_or_else(|| "?".into()),
                    chars::ANNOTATION => opts
                        .annotation_labels
                        .get(&chp.object)
                        .cloned()
                        .unwrap_or_default(),
                    _ => opts.time.clone(),
                };
                // An annotation mark is the original's balloon with the initials
                // inside, in a small size.
                let balloon = c == chars::ANNOTATION;
                let size = if balloon { ANNOTATION_TEXT } else { glyph_size };
                let mut x = if balloon { 7.0 } else { 0.0 };
                let mut glyphs = Vec::new();
                // In the annotations pane the initials are followed, after
                // a tab, by who wrote it and when, beside the balloon.
                let (inside, caption) = match value.split_once('\t') {
                    Some((i, c)) if balloon => (i, c),
                    _ => (value.as_str(), ""),
                };
                for vc in inside.chars() {
                    let (f, g) = fonts.glyph(face, vc);
                    glyphs.push(FieldGlyph { face: f, id: g, x });
                    x += fonts.advance(f, g, size) + if balloon { 0.0 } else { spacing };
                }
                let mut end = ANNOTATION_WIDTH;
                if !caption.is_empty() {
                    break_after = true;
                    x = ANNOTATION_WIDTH + 3.0;
                    let (ef, eg) = fonts.glyph(face, '…');
                    let ellipsis_width = fonts.advance(ef, eg, size);
                    for vc in caption.chars() {
                        let (f, g) = fonts.glyph(face, vc);
                        let advance = fonts.advance(f, g, size);
                        if x + advance + ellipsis_width > opts.width {
                            glyphs.push(FieldGlyph {
                                face: ef,
                                id: eg,
                                x,
                            });
                            x += ellipsis_width;
                            break;
                        }
                        glyphs.push(FieldGlyph { face: f, id: g, x });
                        x += advance;
                    }
                    end = x + 6.0;
                }
                fields.push(glyphs);
                advance = if balloon { end } else { x };
                if balloon {
                    ascent = ascent.max(20.0);
                }
            }
            items.push(Item {
                offset,
                id,
                face: gface,
                advance,
                kind,
                style,
                ascent,
                descent: vm.descent,
                gap: vm.line_gap,
                ch: c,
                break_after,
            });
        }
    }
    items
}

/// Underlines, strike-throughs and tab leaders for every line.
fn add_rules(out: &mut ParaLayout) {
    let mut rules = Vec::new();
    for (li, line) in out.lines.iter().enumerate() {
        let glyphs = &out.glyphs[line.first..line.last];
        // Trailing spaces are not underlined.
        let mut visible_end = glyphs.len();
        while visible_end > 0
            && matches!(
                glyphs[visible_end - 1].kind,
                Kind::Space | Kind::LineBreak | Kind::PageBreak
            )
        {
            visible_end -= 1;
        }
        let mut k = 0;
        while k < visible_end {
            let g = glyphs[k];
            let st = out.styles[g.style as usize];
            let chp = st.chp;
            let color = chp.color.0;
            let underline_here = match chp.underline {
                Underline::None => None,
                Underline::Word if g.kind == Kind::Space || matches!(g.kind, Kind::Tab(_)) => None,
                u => Some(u),
            };
            if g.kind == Kind::Hidden {
                k += 1;
                continue;
            }
            let below = (st.size / 9.0).max(1.0) - st.raise;
            if let Some(u) = underline_here {
                let kind = match u {
                    Underline::Double => RuleKind::Double,
                    Underline::Dotted => RuleKind::Dotted,
                    _ => RuleKind::Solid,
                };
                push_rule(
                    &mut rules,
                    li as u32,
                    g.x,
                    g.x + g.advance,
                    below,
                    kind,
                    color,
                );
            }
            if chp.strike && !matches!(g.kind, Kind::Tab(_)) {
                let dy = -(st.size * 0.3) - st.raise;
                push_rule(
                    &mut rules,
                    li as u32,
                    g.x,
                    g.x + g.advance,
                    dy,
                    RuleKind::Solid,
                    color,
                );
            }
            if let Kind::Tab(leader) = g.kind
                && g.advance > 0.0
            {
                let stop = g.x + g.advance;
                let kind = match leader {
                    TabLeader::None => None,
                    TabLeader::Dots => Some((RuleKind::Leader('.'), 0.0)),
                    TabLeader::Dashes => Some((RuleKind::Leader('-'), 0.0)),
                    TabLeader::Underline => Some((RuleKind::Solid, below)),
                };
                if let Some((kind, dy)) = kind {
                    rules.push(Rule {
                        line: li as u32,
                        x0: g.x,
                        x1: stop,
                        dy,
                        kind,
                        color,
                        face: g.face,
                        size: st.size,
                    });
                }
            }
            k += 1;
        }
    }
    out.rules = rules;
}

/// Add a rule, extending the previous one when it continues it.
fn push_rule(
    rules: &mut Vec<Rule>,
    line: u32,
    x0: f32,
    x1: f32,
    dy: f32,
    kind: RuleKind,
    color: u32,
) {
    if let Some(r) = rules.last_mut()
        && r.line == line
        && r.kind == kind
        && r.color == color
        && (r.dy - dy).abs() < 0.01
        && (r.x1 - x0).abs() < 0.5
    {
        r.x1 = x1;
        return;
    }
    rules.push(Rule {
        line,
        x0,
        x1,
        dy,
        kind,
        color,
        face: FaceKey::default(),
        size: 0.0,
    });
}

impl ParaLayout {
    /// Drop the glyphs, keeping what pagination needs.
    pub fn strip(&mut self) {
        self.glyphs = Vec::new();
        self.stripped = true;
    }

    /// The glyphs of `line`: none if they were stripped, never a panic.
    pub fn line_glyphs(&self, line: &Line) -> &[Glyph] {
        self.glyphs.get(line.first..line.last).unwrap_or(&[])
    }

    /// Line containing byte `offset`. At a wrap point the offset belongs to
    /// the next line unless `upstream` asks for the end of the previous one.
    pub fn line_of(&self, offset: usize, upstream: bool) -> usize {
        for (i, line) in self.lines.iter().enumerate() {
            let last = i + 1 == self.lines.len();
            if offset < line.end || last {
                return i;
            }
            if offset == line.end && upstream {
                return i;
            }
        }
        0
    }

    /// Caret x position and line for byte `offset`.
    pub fn caret(&self, offset: usize, upstream: bool) -> (usize, f32) {
        let li = self.line_of(offset, upstream);
        let line = &self.lines[li];
        for g in self.line_glyphs(line) {
            if g.offset as usize == offset {
                return (li, g.x);
            }
        }
        (li, line.end_x)
    }

    /// Byte offset nearest to `x` on line `li`, and whether the caret should
    /// show at the end of that line rather than the start of the next.
    pub fn hit(&self, li: usize, x: f32) -> (usize, bool) {
        let line = &self.lines[li];
        for g in self.line_glyphs(line) {
            if matches!(g.kind, Kind::LineBreak | Kind::PageBreak) {
                return (g.offset as usize, false);
            }
            if x < g.x + g.advance / 2.0 {
                return (g.offset as usize, false);
            }
        }
        let last_line = li + 1 == self.lines.len();
        (line.end, !last_line)
    }

    /// Line index at height `y` (from the paragraph top), clamped.
    pub fn line_at_y(&self, y: f32) -> usize {
        for (i, line) in self.lines.iter().enumerate() {
            if y < line.top + line.height {
                return i;
            }
        }
        self.lines.len().saturating_sub(1)
    }
}
